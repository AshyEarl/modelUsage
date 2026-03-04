use crate::model::{FileDailyRow, UsageEvent, UsageTotals};
use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub fn parse_file(path: &Path) -> Result<Vec<FileDailyRow>> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut unique_messages: BTreeMap<String, UsageEvent> = BTreeMap::new();
    let mut daily: BTreeMap<(NaiveDate, String), UsageTotals> = BTreeMap::new();

    for line in reader.lines() {
        let line = line?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(event) = parse_event(&value) else {
            continue;
        };
        // A single Claude response can appear multiple times as intermediate states in JSONL.
        // Deduplicate by message.id/uuid and keep the first usage entry to match ccusage behavior.
        // Claude 的一条最终响应在 JSONL 里可能出现多次中间态记录。
        // 这里按 message.id/uuid 去重，并保留第一次出现的 usage，行为与 ccusage 对齐。
        let message_key = value
            .get("message")
            .and_then(|msg| msg.get("id"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| value.get("uuid").and_then(Value::as_str).map(ToOwned::to_owned));
        let Some(message_key) = message_key else {
            continue;
        };
        unique_messages.entry(message_key).or_insert(event);
    }

    for event in unique_messages.into_values() {
        let key = (event.timestamp.date_naive(), event.normalized_model.clone());
        daily.entry(key).or_default().add_assign(&event.usage);
    }

    Ok(daily
        .into_iter()
        .map(|((date, model), usage)| FileDailyRow { date, model, usage })
        .collect())
}

fn parse_event(value: &Value) -> Option<UsageEvent> {
    let timestamp = parse_timestamp(value.get("timestamp")?.as_str()?)?;
    let message = value.get("message")?;
    let raw_model = message.get("model")?.as_str()?.to_string();
    let normalized_model = normalize_claude_model(&raw_model);
    if normalized_model == "<synthetic>" {
        return None;
    }
    let usage = message.get("usage")?;

    let cache_creation = usage.get("cache_creation");
    let cache_write_5m = cache_creation
        .and_then(|v| v.get("ephemeral_5m_input_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_write_1h = cache_creation
        .and_then(|v| v.get("ephemeral_1h_input_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_creation_total = usage
        .get("cache_creation_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    // Some Claude logs only expose the total cache creation tokens without a 5m/1h split.
    // Put the remaining amount into the 5m bucket so the total token count stays intact.
    // Claude 有些日志只给 cache_creation 总数，不拆 5m/1h；剩余部分归到 5m，保证不丢数。
    let remaining_cache_write = cache_creation_total.saturating_sub(cache_write_5m + cache_write_1h);

    let input = usage.get("input_tokens").and_then(Value::as_u64).unwrap_or(0);
    let output = usage.get("output_tokens").and_then(Value::as_u64).unwrap_or(0);
    let cache_read = usage
        .get("cache_read_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let total = usage
        .get("total_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(input + output + cache_creation_total + cache_read);

    Some(UsageEvent {
        source: crate::model::SourceKind::Claude,
        timestamp,
        raw_model,
        normalized_model,
        usage: UsageTotals {
            input,
            output,
            reasoning: 0,
            cache_write_5m: cache_write_5m + remaining_cache_write,
            cache_write_1h,
            cache_read,
            total,
        },
    })
}

fn parse_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw).ok().map(|dt| dt.with_timezone(&Utc))
}

pub fn normalize_claude_model(raw: &str) -> String {
    let mut model = raw.trim();
    if let Some(stripped) = model.strip_prefix("anthropic/") {
        model = stripped;
    }
    if let Some(stripped) = model.strip_prefix("claude-") {
        model = stripped;
    }
    let pieces: Vec<&str> = model.split('-').collect();
    if pieces.len() >= 3 {
        let tail = pieces.last().copied().unwrap_or_default();
        // Only strip the date suffix for Claude models; keep real model versions such as 4.5 and 4.6.
        // Claude 这边只折叠日期后缀，不动 4.5/4.6 之类真实版本信息。
        if tail.len() == 8 && tail.chars().all(|c| c.is_ascii_digit()) {
            return pieces[..pieces.len() - 1].join("-");
        }
    }
    model.to_string()
}

#[cfg(test)]
mod tests {
    use super::normalize_claude_model;

    #[test]
    fn strips_date_suffix() {
        assert_eq!(normalize_claude_model("claude-sonnet-4-5-20250929"), "sonnet-4-5");
        assert_eq!(normalize_claude_model("claude-opus-4-6"), "opus-4-6");
    }
}
