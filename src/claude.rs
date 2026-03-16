use crate::model::{ClaudeMessageRow, FileDailyRow, UsageEvent, UsageTotals};
use crate::profile;
use crate::timezone::AggregationTz;
use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Instant;

pub struct ParsedClaudeFile {
    pub daily_rows: Vec<FileDailyRow>,
    pub message_rows: Vec<ClaudeMessageRow>,
}

pub fn parse_file(path: &Path, aggregation_tz: &AggregationTz) -> Result<Vec<FileDailyRow>> {
    Ok(parse_file_detailed(path, aggregation_tz)?.daily_rows)
}

pub fn parse_file_detailed(
    path: &Path,
    aggregation_tz: &AggregationTz,
) -> Result<ParsedClaudeFile> {
    let profile_enabled = profile::enabled();
    let started = Instant::now();
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut unique_messages: BTreeMap<String, UsageEvent> = BTreeMap::new();
    let mut daily: BTreeMap<(NaiveDate, String, String), UsageTotals> = BTreeMap::new();
    let mut record_no: u64 = 0;
    let mut parsed_records: u64 = 0;
    let mut invalid_records: u64 = 0;
    let mut empty_records: u64 = 0;
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            empty_records += 1;
            continue;
        }
        record_no += 1;

        // Cheap string check to skip file-history-snapshot records before parsing JSON.
        // 廉价字符串检查，跳过 file-history-snapshot 记录，避免无谓的 JSON 解析。
        if trimmed.contains("\"type\":\"file-history-snapshot\"")
            || trimmed.contains("\"type\": \"file-history-snapshot\"")
        {
            continue;
        }

        match serde_json::from_str::<ClaudeRecord>(trimmed) {
            Ok(record) => {
                parsed_records += 1;
                let Some((message_key, event)) = parse_event(record) else {
                    continue;
                };
                unique_messages.insert(message_key, event);
            }
            Err(err) => {
                invalid_records += 1;
                eprintln!(
                    "\x1b[31mwarning: skipped invalid Claude JSONL record {}:{} ({})\x1b[0m",
                    path.display(),
                    record_no,
                    err
                );
            }
        }
    }

    let unique_message_count = unique_messages.len();
    let mut message_rows = Vec::with_capacity(unique_message_count);
    for (message_key, event) in unique_messages {
        message_rows.push(ClaudeMessageRow {
            message_key,
            timestamp: event.timestamp,
            project: event.project.clone(),
            model: event.normalized_model.clone(),
            usage: event.usage.clone(),
        });
        let day = aggregation_tz.date_for(event.timestamp);
        let key = (day, event.project.clone(), event.normalized_model.clone());
        daily.entry(key).or_default().add_assign(&event.usage);
    }

    if profile_enabled {
        profile::log(format!(
            "claude parse file={} parsed={} invalid={} empty={} unique_messages={} daily_rows={} elapsed_ms={}",
            path.display(),
            parsed_records,
            invalid_records,
            empty_records,
            unique_message_count,
            daily.len(),
            started.elapsed().as_millis()
        ));
    }

    let daily_rows = daily
        .into_iter()
        .map(|((date, project, model), usage)| FileDailyRow {
            date,
            project,
            model,
            usage,
        })
        .collect();
    Ok(ParsedClaudeFile {
        daily_rows,
        message_rows,
    })
}

#[derive(Debug, Deserialize)]
struct ClaudeRecord {
    timestamp: Option<String>,
    cwd: Option<String>,
    uuid: Option<String>,
    message: Option<ClaudeMessage>,
}

#[derive(Debug, Deserialize)]
struct ClaudeMessage {
    id: Option<String>,
    model: Option<String>,
    usage: Option<ClaudeUsage>,
}

#[derive(Debug, Deserialize)]
struct ClaudeUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    total_tokens: Option<u64>,
    cache_creation: Option<ClaudeCacheCreation>,
}

#[derive(Debug, Deserialize)]
struct ClaudeCacheCreation {
    ephemeral_5m_input_tokens: Option<u64>,
    ephemeral_1h_input_tokens: Option<u64>,
}

fn parse_event(value: ClaudeRecord) -> Option<(String, UsageEvent)> {
    let timestamp = parse_timestamp(value.timestamp.as_deref()?)?;
    let message = value.message?;
    let raw_model = message.model?.to_string();
    let normalized_model = normalize_claude_model(&raw_model);
    if normalized_model == "<synthetic>" {
        return None;
    }
    let usage = message.usage?;

    let cache_write_5m = usage
        .cache_creation
        .as_ref()
        .and_then(|v| v.ephemeral_5m_input_tokens)
        .unwrap_or(0);
    let cache_write_1h = usage
        .cache_creation
        .as_ref()
        .and_then(|v| v.ephemeral_1h_input_tokens)
        .unwrap_or(0);
    let cache_creation_total = usage.cache_creation_input_tokens.unwrap_or(0);
    // Some Claude logs only expose the total cache creation tokens without a 5m/1h split.
    // Put the remaining amount into the 5m bucket so the total token count stays intact.
    // Claude 有些日志只给 cache_creation 总数，不拆 5m/1h；剩余部分归到 5m，保证不丢数。
    let remaining_cache_write =
        cache_creation_total.saturating_sub(cache_write_5m + cache_write_1h);

    let input = usage.input_tokens.unwrap_or(0);
    let output = usage.output_tokens.unwrap_or(0);
    let cache_read = usage.cache_read_input_tokens.unwrap_or(0);
    let total = usage
        .total_tokens
        .unwrap_or(input + output + cache_creation_total + cache_read);

    let message_key = message.id.or(value.uuid)?;
    Some((
        message_key,
        UsageEvent {
            source: crate::model::SourceKind::Claude,
            timestamp,
            project: value.cwd.unwrap_or_else(|| "<unknown-project>".to_string()),
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
        },
    ))
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
    use super::{normalize_claude_model, parse_file, parse_file_detailed};
    use crate::timezone::AggregationTz;
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

        let rows = parse_file(&path, &AggregationTz::parse(Some("UTC")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].project, "<unknown-project>");
        assert_eq!(rows[0].usage.input, 1);
        assert_eq!(rows[0].usage.output, 20962);
        assert_eq!(rows[0].usage.cache_read, 100);
        assert_eq!(rows[0].usage.cache_write_5m, 10);
        assert_eq!(rows[0].usage.total, 21073);
    }

    #[test]
    fn detailed_parse_emits_message_rows_after_file_internal_dedup() {
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
        ]);

        let parsed =
            parse_file_detailed(&path, &AggregationTz::parse(Some("UTC")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(parsed.message_rows.len(), 1);
        assert_eq!(parsed.message_rows[0].message_key, "msg-1");
        assert_eq!(parsed.message_rows[0].usage.output, 20);
        assert_eq!(parsed.message_rows[0].usage.total, 131);
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

        let rows = parse_file(&path, &AggregationTz::parse(Some("UTC")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].project, "<unknown-project>");
        assert_eq!(rows[0].usage.input, 3);
        assert_eq!(rows[0].usage.output, 50);
        assert_eq!(rows[0].usage.cache_read, 150);
        assert_eq!(rows[0].usage.cache_write_5m, 15);
        assert_eq!(rows[0].usage.total, 218);
    }

    #[test]
    fn groups_by_cwd_and_target_timezone_day() {
        let path = write_temp_jsonl(&[json!({
            "timestamp": "2026-03-01T20:30:00Z",
            "cwd": "/repo/demo",
            "message": {
                "id": "msg-1",
                "model": "claude-sonnet-4-6",
                "usage": {
                    "input_tokens": 1,
                    "output_tokens": 2,
                    "cache_read_input_tokens": 3,
                    "cache_creation_input_tokens": 4,
                    "total_tokens": 10
                }
            }
        })]);

        let rows = parse_file(&path, &AggregationTz::parse(Some("UTC+8")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].project, "/repo/demo");
        assert_eq!(rows[0].date.to_string(), "2026-03-02");
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
