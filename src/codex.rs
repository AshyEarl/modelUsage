use crate::model::{FileDailyRow, UsageTotals};
use crate::timezone::AggregationTz;
use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub fn parse_file(path: &Path, aggregation_tz: &AggregationTz) -> Result<Vec<FileDailyRow>> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut current_model = String::new();
    let mut project = "<unknown-project>".to_string();
    let mut previous_total: Option<RawUsage> = None;
    let mut daily: BTreeMap<(NaiveDate, String, String), UsageTotals> = BTreeMap::new();

    for line in reader.lines() {
        let line = line?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        match value.get("type").and_then(Value::as_str) {
            Some("session_meta") => {
                if let Some(cwd) = value
                    .get("payload")
                    .and_then(|v| v.get("cwd"))
                    .and_then(Value::as_str)
                {
                    project = cwd.to_string();
                }
            }
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
                // Prefer cumulative-delta when total_token_usage exists to avoid duplicate snapshot inflation.
                // 优先用 total_token_usage 做累计差分，避免重复快照（例如 rate-limit 刷新）被重复累计。
                let Some(raw_usage) = choose_raw_usage(
                    last_usage.as_ref(),
                    total_usage.as_ref(),
                    previous_total.as_ref(),
                ) else {
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
                let day = aggregation_tz.date_for(timestamp);
                let key = (day, project.clone(), model);
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
        .map(|((date, project, model), usage)| FileDailyRow {
            date,
            project,
            model,
            usage,
        })
        .collect())
}

fn choose_raw_usage(
    last_usage: Option<&RawUsage>,
    total_usage: Option<&RawUsage>,
    previous_total: Option<&RawUsage>,
) -> Option<RawUsage> {
    match (total_usage, previous_total) {
        (Some(total), Some(previous)) if !total.regressed_from(previous) => {
            Some(total.delta(previous))
        }
        // If cumulative counters regress, likely session/state reset; keep counting via last_usage.
        // 若累计计数发生回退，通常是会话/状态重置；此时回退到 last_usage，避免漏算真实新增。
        (Some(_), Some(_)) => last_usage.cloned().or_else(|| total_usage.cloned()),
        (Some(total), None) => last_usage.cloned().or_else(|| Some(total.clone())),
        (None, _) => last_usage.cloned(),
    }
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

    fn regressed_from(&self, previous: &Self) -> bool {
        self.input < previous.input
            || self.cached_input < previous.cached_input
            || self.output < previous.output
            || self.reasoning < previous.reasoning
            || self.total < previous.total
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
    use super::{normalize_codex_model, parse_file};
    use crate::timezone::AggregationTz;
    use serde_json::{Value, json};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn strips_provider_prefix() {
        assert_eq!(
            normalize_codex_model("openrouter/openai/gpt-5-codex"),
            "gpt-5-codex"
        );
        assert_eq!(normalize_codex_model("gpt-5.3-codex"), "gpt-5.3-codex");
        assert_eq!(normalize_codex_model("gpt-5.2"), "gpt-5.2");
    }

    #[test]
    fn uses_total_delta_to_skip_duplicate_snapshots() {
        let path = write_temp_jsonl(&[
            turn_context("gpt-5-codex"),
            token_count_event(
                "2026-03-01T00:00:00Z",
                100,
                80,
                20,
                0,
                100,
                100,
                80,
                20,
                0,
                100,
            ),
            token_count_event(
                "2026-03-01T00:00:01Z",
                100,
                80,
                20,
                0,
                100,
                100,
                80,
                20,
                0,
                100,
            ),
            token_count_event(
                "2026-03-01T00:00:02Z",
                160,
                120,
                40,
                0,
                160,
                60,
                40,
                20,
                0,
                60,
            ),
        ]);

        let rows = parse_file(&path, &AggregationTz::parse(Some("UTC")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].project, "<unknown-project>");
        assert_eq!(rows[0].usage.input, 160);
        assert_eq!(rows[0].usage.cache_read, 120);
        assert_eq!(rows[0].usage.output, 40);
        assert_eq!(rows[0].usage.total, 160);
    }

    #[test]
    fn falls_back_to_last_when_total_regresses() {
        let path = write_temp_jsonl(&[
            turn_context("gpt-5-codex"),
            token_count_event(
                "2026-03-01T00:00:00Z",
                100,
                80,
                20,
                0,
                100,
                100,
                80,
                20,
                0,
                100,
            ),
            token_count_event("2026-03-01T00:00:01Z", 90, 70, 20, 0, 90, 20, 10, 10, 0, 20),
            token_count_event(
                "2026-03-01T00:00:02Z",
                120,
                90,
                30,
                0,
                120,
                30,
                20,
                10,
                0,
                30,
            ),
        ]);

        let rows = parse_file(&path, &AggregationTz::parse(Some("UTC")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].project, "<unknown-project>");
        assert_eq!(rows[0].usage.input, 150);
        assert_eq!(rows[0].usage.cache_read, 110);
        assert_eq!(rows[0].usage.output, 40);
        assert_eq!(rows[0].usage.total, 150);
    }

    #[test]
    fn falls_back_to_last_when_total_is_missing() {
        let path = write_temp_jsonl(&[
            turn_context("gpt-5-codex"),
            json!({
                "timestamp": "2026-03-01T00:00:00Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": {
                        "last_token_usage": {
                            "input_tokens": 42,
                            "cached_input_tokens": 12,
                            "output_tokens": 8,
                            "reasoning_output_tokens": 0,
                            "total_tokens": 50
                        }
                    }
                }
            }),
        ]);

        let rows = parse_file(&path, &AggregationTz::parse(Some("UTC")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].project, "<unknown-project>");
        assert_eq!(rows[0].usage.input, 42);
        assert_eq!(rows[0].usage.cache_read, 12);
        assert_eq!(rows[0].usage.output, 8);
        assert_eq!(rows[0].usage.total, 50);
    }

    #[test]
    fn groups_by_session_cwd_and_target_timezone_day() {
        let path = write_temp_jsonl(&[
            session_meta("/repo/codex"),
            turn_context("gpt-5-codex"),
            token_count_event("2026-03-01T20:30:00Z", 10, 4, 6, 0, 10, 10, 4, 6, 0, 10),
        ]);

        let rows = parse_file(&path, &AggregationTz::parse(Some("UTC+8")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].project, "/repo/codex");
        assert_eq!(rows[0].date.to_string(), "2026-03-02");
    }

    fn write_temp_jsonl(lines: &[Value]) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("modelusage-codex-test-{nanos}.jsonl"));
        let mut payload = String::new();
        for line in lines {
            payload.push_str(&line.to_string());
            payload.push('\n');
        }
        fs::write(&path, payload).unwrap();
        path
    }

    fn turn_context(model: &str) -> Value {
        json!({
            "timestamp": "2026-03-01T00:00:00Z",
            "type": "turn_context",
            "payload": { "model": model }
        })
    }

    fn session_meta(cwd: &str) -> Value {
        json!({
            "timestamp": "2026-03-01T00:00:00Z",
            "type": "session_meta",
            "payload": { "cwd": cwd }
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn token_count_event(
        ts: &str,
        total_input: u64,
        total_cached: u64,
        total_output: u64,
        total_reasoning: u64,
        total_tokens: u64,
        last_input: u64,
        last_cached: u64,
        last_output: u64,
        last_reasoning: u64,
        last_tokens: u64,
    ) -> Value {
        json!({
            "timestamp": ts,
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": {
                        "input_tokens": total_input,
                        "cached_input_tokens": total_cached,
                        "output_tokens": total_output,
                        "reasoning_output_tokens": total_reasoning,
                        "total_tokens": total_tokens
                    },
                    "last_token_usage": {
                        "input_tokens": last_input,
                        "cached_input_tokens": last_cached,
                        "output_tokens": last_output,
                        "reasoning_output_tokens": last_reasoning,
                        "total_tokens": last_tokens
                    }
                }
            }
        })
    }
}
