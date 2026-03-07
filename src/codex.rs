use crate::model::{FileDailyRow, UsageTotals};
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
    let mut current_model = String::new();
    let mut previous_total: Option<RawUsage> = None;
    let mut daily: BTreeMap<(NaiveDate, String), UsageTotals> = BTreeMap::new();

    for line in reader.lines() {
        let line = line?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        match value.get("type").and_then(Value::as_str) {
            Some("turn_context") => {
                if let Some(model) = value
                    .get("payload")
                    .and_then(|v| v.get("model"))
                    .and_then(Value::as_str)
                {
                    current_model = normalize_codex_model(model);
                }
            }
            Some("event_msg") => {
                let timestamp = match value
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .and_then(parse_timestamp)
                {
                    Some(ts) => ts,
                    None => continue,
                };
                let payload = match value.get("payload") {
                    Some(payload)
                        if payload.get("type").and_then(Value::as_str) == Some("token_count") =>
                    {
                        payload
                    }
                    _ => continue,
                };
                let info = match payload.get("info") {
                    Some(info) => info,
                    None => continue,
                };
                let last_usage = info.get("last_token_usage").and_then(parse_raw_usage);
                let total_usage = info.get("total_token_usage").and_then(parse_raw_usage);
                // last_token_usage is the preferred Codex usage source.
                // Fall back to total_token_usage - previous_total only when last_token_usage is missing.
                // Codex 本地日志更可靠的口径是 last_token_usage。
                // 只有缺少 last_token_usage 时，才退回到 total_token_usage - previous_total。
                let raw_usage = if let Some(last) = last_usage {
                    last
                } else if let Some(total) = total_usage.clone() {
                    if let Some(prev) = previous_total.as_ref() {
                        total.delta(prev)
                    } else {
                        total
                    }
                } else {
                    continue;
                };
                if let Some(total) = total_usage {
                    previous_total = Some(total);
                }
                if raw_usage.is_zero() {
                    continue;
                }
                let model = if current_model.is_empty() {
                    "unknown-codex-model".to_string()
                } else {
                    current_model.clone()
                };
                let key = (timestamp.date_naive(), model);
                daily
                    .entry(key)
                    .or_default()
                    .add_assign(&raw_usage.into_usage_totals());
            }
            _ => {}
        }
    }

    Ok(daily
        .into_iter()
        .map(|((date, model), usage)| FileDailyRow { date, model, usage })
        .collect())
}

#[derive(Debug, Clone)]
struct RawUsage {
    input: u64,
    cached_input: u64,
    output: u64,
    reasoning: u64,
    total: u64,
}

impl RawUsage {
    fn delta(&self, previous: &Self) -> Self {
        Self {
            input: self.input.saturating_sub(previous.input),
            cached_input: self.cached_input.saturating_sub(previous.cached_input),
            output: self.output.saturating_sub(previous.output),
            reasoning: self.reasoning.saturating_sub(previous.reasoning),
            total: self.total.saturating_sub(previous.total),
        }
    }

    fn into_usage_totals(self) -> UsageTotals {
        UsageTotals {
            input: self.input,
            output: self.output,
            reasoning: self.reasoning,
            cache_write_5m: 0,
            cache_write_1h: 0,
            // cached_input is a subset of input for Codex, so map it directly to cache_read.
            // Codex 的 cached_input 属于 input 的子集，统一映射到 cache_read。
            cache_read: self.cached_input.min(self.input),
            total: if self.total > 0 {
                self.total
            } else {
                self.input + self.output
            },
        }
    }

    fn is_zero(&self) -> bool {
        self.input == 0
            && self.cached_input == 0
            && self.output == 0
            && self.reasoning == 0
            && self.total == 0
    }
}

fn parse_raw_usage(value: &Value) -> Option<RawUsage> {
    Some(RawUsage {
        input: value
            .get("input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cached_input: value
            .get("cached_input_tokens")
            .or_else(|| value.get("cache_read_input_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output: value
            .get("output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        reasoning: value
            .get("reasoning_output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        total: value
            .get("total_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    })
}

fn parse_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

pub fn normalize_codex_model(raw: &str) -> String {
    let mut model = raw.trim();
    // Strip provider prefixes only; do not merge minor versions such as 5.2 or 5.3.
    // 只去 provider 前缀，不合并 5.2/5.3 这类小版本，便于后续精确看版本差异。
    for prefix in ["openai/", "openrouter/openai/"] {
        if let Some(stripped) = model.strip_prefix(prefix) {
            model = stripped;
            break;
        }
    }
    model.to_string()
}

#[cfg(test)]
mod tests {
    use super::normalize_codex_model;

    #[test]
    fn strips_provider_prefix() {
        assert_eq!(
            normalize_codex_model("openrouter/openai/gpt-5-codex"),
            "gpt-5-codex"
        );
        assert_eq!(normalize_codex_model("gpt-5.3-codex"), "gpt-5.3-codex");
        assert_eq!(normalize_codex_model("gpt-5.2"), "gpt-5.2");
    }
}
