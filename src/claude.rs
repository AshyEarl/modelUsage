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
        // Keep the last usage entry for each message.id/uuid to capture the final streamed totals.
        // Claude 的一条最终响应在 JSONL 里可能出现多次中间态记录。
        // 这里按 message.id/uuid 去重，并保留最后一次 usage，确保拿到流式输出的最终计数。
        let message_key = value
            .get("message")
            .and_then(|msg| msg.get("id"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| {
                value
                    .get("uuid")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            });
        let Some(message_key) = message_key else {
            continue;
        };
        unique_messages.insert(message_key, event);
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
    let remaining_cache_write =
        cache_creation_total.saturating_sub(cache_write_5m + cache_write_1h);

    let input = usage
        .get("input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output = usage
        .get("output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
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
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
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
    use super::{normalize_claude_model, parse_file};
    use serde_json::{Value, json};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn strips_date_suffix() {
        assert_eq!(
            normalize_claude_model("claude-sonnet-4-5-20250929"),
            "sonnet-4-5"
        );
        assert_eq!(normalize_claude_model("claude-opus-4-6"), "opus-4-6");
    }

    #[test]
    fn keeps_last_usage_for_duplicate_message_id() {
        let path = write_temp_jsonl(&[
            event(
                "2026-03-01T00:00:00Z",
                "msg-1",
                "claude-sonnet-4-6",
                1,
                8,
                100,
                10,
                119,
            ),
            event(
                "2026-03-01T00:00:01Z",
                "msg-1",
                "claude-sonnet-4-6",
                1,
                8,
                100,
                10,
                119,
            ),
            event(
                "2026-03-01T00:00:02Z",
                "msg-1",
                "claude-sonnet-4-6",
                1,
                20962,
                100,
                10,
                21073,
            ),
        ]);

        let rows = parse_file(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].usage.input, 1);
        assert_eq!(rows[0].usage.output, 20962);
        assert_eq!(rows[0].usage.cache_read, 100);
        assert_eq!(rows[0].usage.cache_write_5m, 10);
        assert_eq!(rows[0].usage.total, 21073);
    }

    #[test]
    fn aggregates_distinct_messages_once_each() {
        let path = write_temp_jsonl(&[
            event(
                "2026-03-01T00:00:00Z",
                "msg-1",
                "claude-sonnet-4-6",
                1,
                8,
                100,
                10,
                119,
            ),
            event(
                "2026-03-01T00:00:02Z",
                "msg-1",
                "claude-sonnet-4-6",
                1,
                20,
                100,
                10,
                131,
            ),
            event(
                "2026-03-01T00:00:03Z",
                "msg-2",
                "claude-sonnet-4-6",
                2,
                30,
                50,
                5,
                87,
            ),
        ]);

        let rows = parse_file(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].usage.input, 3);
        assert_eq!(rows[0].usage.output, 50);
        assert_eq!(rows[0].usage.cache_read, 150);
        assert_eq!(rows[0].usage.cache_write_5m, 15);
        assert_eq!(rows[0].usage.total, 218);
    }

    fn write_temp_jsonl(lines: &[Value]) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("modelusage-claude-test-{nanos}.jsonl"));
        let mut payload = String::new();
        for line in lines {
            payload.push_str(&line.to_string());
            payload.push('\n');
        }
        fs::write(&path, payload).unwrap();
        path
    }

    fn event(
        ts: &str,
        message_id: &str,
        model: &str,
        input: u64,
        output: u64,
        cache_read: u64,
        cache_creation: u64,
        total: u64,
    ) -> Value {
        json!({
            "timestamp": ts,
            "message": {
                "id": message_id,
                "model": model,
                "usage": {
                    "input_tokens": input,
                    "output_tokens": output,
                    "cache_read_input_tokens": cache_read,
                    "cache_creation_input_tokens": cache_creation,
                    "total_tokens": total
                }
            }
        })
    }
}
