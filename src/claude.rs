use crate::model::{ClaudeMessageRow, FileDailyRow, UsageEvent, UsageTotals};
use crate::profile;
use crate::timezone::AggregationTz;
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, NaiveDate, Utc};
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
    let mut unique_messages: BTreeMap<String, (UsageEvent, bool)> = BTreeMap::new();
    let mut compact_records = Vec::new();
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
                if let Some(compact) = parse_compact_record(&record) {
                    compact_records.push(compact);
                    continue;
                }
                let Some((message_key, event)) = parse_event(record) else {
                    continue;
                };
                unique_messages.insert(message_key, (event, false));
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

    // Claude Code currently persists only compact pre/post token estimates, not the exact API usage
    // kept in memory during compaction. Convert those boundaries into estimated billable events so
    // compact calls no longer disappear from token and cost reports.
    // Claude Code 当前只持久化 compact 前后的 token 估算，没有写入压缩时内存里的精确 API
    // usage。这里把 boundary 转成估算计费事件，避免 compact 调用从 token 与成本报表中消失。
    let context_events: Vec<&UsageEvent> =
        unique_messages.values().map(|(event, _)| event).collect();
    let compact_events: Vec<(String, UsageEvent, bool)> = compact_records
        .iter()
        .filter_map(|compact| synthesize_compact_event(compact, &context_events))
        .collect();
    drop(context_events);
    for (message_key, event, estimated) in compact_events {
        unique_messages.insert(message_key, (event, estimated));
    }

    let unique_message_count = unique_messages.len();
    let mut message_rows = Vec::with_capacity(unique_message_count);
    for (message_key, (event, estimated)) in unique_messages {
        message_rows.push(ClaudeMessageRow {
            message_key,
            timestamp: event.timestamp,
            project: event.project.clone(),
            model: event.normalized_model.clone(),
            usage: event.usage.clone(),
            estimated,
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
    #[serde(rename = "type")]
    record_type: Option<String>,
    subtype: Option<String>,
    timestamp: Option<String>,
    cwd: Option<String>,
    uuid: Option<String>,
    model: Option<String>,
    message: Option<ClaudeMessage>,
    #[serde(rename = "compactMetadata", alias = "compact_metadata")]
    compact_metadata: Option<ClaudeCompactMetadata>,
    #[serde(rename = "compactionUsage", alias = "compaction_usage")]
    compaction_usage: Option<ClaudeUsage>,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeCompactMetadata {
    #[serde(alias = "pre_tokens")]
    pre_tokens: Option<u64>,
    #[serde(alias = "post_tokens")]
    post_tokens: Option<u64>,
    #[serde(alias = "duration_ms")]
    duration_ms: Option<u64>,
    #[serde(alias = "compaction_usage")]
    compaction_usage: Option<ClaudeUsage>,
}

#[derive(Debug)]
struct ClaudeCompactRecord {
    message_key: String,
    timestamp: DateTime<Utc>,
    project: String,
    raw_model: Option<String>,
    pre_tokens: Option<u64>,
    post_tokens: Option<u64>,
    duration_ms: u64,
    exact_usage: Option<UsageTotals>,
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

    let message_key = message.id.or(value.uuid)?;
    Some((
        message_key,
        UsageEvent {
            source: crate::model::SourceKind::Claude,
            timestamp,
            project: value.cwd.unwrap_or_else(|| "<unknown-project>".to_string()),
            raw_model,
            normalized_model,
            usage: usage_totals_from_claude_usage(&usage),
        },
    ))
}

fn parse_compact_record(value: &ClaudeRecord) -> Option<ClaudeCompactRecord> {
    if value.record_type.as_deref() != Some("system")
        || value.subtype.as_deref() != Some("compact_boundary")
    {
        return None;
    }
    let metadata = value.compact_metadata.as_ref()?;
    let timestamp = parse_timestamp(value.timestamp.as_deref()?)?;
    let uuid = value.uuid.as_deref()?;
    let exact_usage = value
        .compaction_usage
        .as_ref()
        .or(metadata.compaction_usage.as_ref())
        .map(usage_totals_from_claude_usage);
    Some(ClaudeCompactRecord {
        message_key: format!("compact:{uuid}"),
        timestamp,
        project: value
            .cwd
            .clone()
            .unwrap_or_else(|| "<unknown-project>".to_string()),
        raw_model: value.model.clone(),
        pre_tokens: metadata.pre_tokens,
        post_tokens: metadata.post_tokens,
        duration_ms: metadata.duration_ms.unwrap_or(0),
        exact_usage,
    })
}

fn synthesize_compact_event(
    compact: &ClaudeCompactRecord,
    context_events: &[&UsageEvent],
) -> Option<(String, UsageEvent, bool)> {
    let nearby_model_event = compact_model_event(compact.timestamp, context_events);
    let raw_model = compact
        .raw_model
        .clone()
        .or_else(|| nearby_model_event.map(|event| event.raw_model.clone()))
        .unwrap_or_else(|| "<unknown-compact-model>".to_string());
    let normalized_model = normalize_claude_model(&raw_model);
    if normalized_model == "<synthetic>" {
        return None;
    }
    let (usage, estimated) = if let Some(usage) = compact.exact_usage.clone() {
        (usage, false)
    } else {
        (
            estimate_compact_usage(compact, &normalized_model, context_events)?,
            true,
        )
    };
    Some((
        compact.message_key.clone(),
        UsageEvent {
            source: crate::model::SourceKind::Claude,
            timestamp: compact.timestamp,
            project: compact.project.clone(),
            raw_model,
            normalized_model,
            usage,
        },
        estimated,
    ))
}

fn compact_model_event<'a>(
    timestamp: DateTime<Utc>,
    context_events: &'a [&UsageEvent],
) -> Option<&'a UsageEvent> {
    // A response shortly after the boundary reflects the model selected for the compact command,
    // including sessions resumed with a different model. Otherwise fall back to the latest prior call.
    // boundary 后很快出现的响应最能反映 compact 命令实际选择的模型，也覆盖恢复会话后切模型；
    // 如果没有这样的响应，再退回到压缩前最后一次调用。
    let next_limit = timestamp + Duration::minutes(10);
    context_events
        .iter()
        .copied()
        .filter(|event| event.timestamp >= timestamp && event.timestamp <= next_limit)
        .min_by_key(|event| event.timestamp)
        .or_else(|| {
            context_events
                .iter()
                .copied()
                .filter(|event| event.timestamp < timestamp)
                .max_by_key(|event| event.timestamp)
        })
}

fn estimate_compact_usage(
    compact: &ClaudeCompactRecord,
    normalized_model: &str,
    context_events: &[&UsageEvent],
) -> Option<UsageTotals> {
    let pre_tokens = compact.pre_tokens?;
    let post_tokens = compact.post_tokens?;
    let duration_ms = i64::try_from(compact.duration_ms).unwrap_or(i64::MAX);
    let compact_started_at = compact
        .timestamp
        .checked_sub_signed(Duration::milliseconds(duration_ms))
        .unwrap_or(compact.timestamp);
    let previous = context_events
        .iter()
        .copied()
        .filter(|event| event.timestamp <= compact_started_at)
        .max_by_key(|event| event.timestamp);

    // Compaction skips new cache writes. Reuse only the prior cached prefix when the model matches
    // and its recorded 5m/1h cache is still fresh; the remainder is ordinary input.
    // compact 不会新建缓存。只有模型一致且上一条记录的 5 分钟/1 小时缓存仍有效时，
    // 才把此前缓存前缀计为 cache read，其余部分按普通 input 估算。
    let cache_read = previous
        .filter(|event| event.normalized_model == normalized_model)
        .and_then(|event| {
            let ttl = compact_cache_ttl(&event.usage)?;
            let age = compact_started_at.signed_duration_since(event.timestamp);
            (age >= Duration::zero() && age <= ttl).then(|| {
                pre_tokens.min(
                    event
                        .usage
                        .cache_read
                        .saturating_add(event.usage.cache_write()),
                )
            })
        })
        .unwrap_or(0);
    let input = pre_tokens.saturating_sub(cache_read);
    Some(UsageTotals {
        input,
        output: post_tokens,
        reasoning: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_read,
        total: pre_tokens.saturating_add(post_tokens),
    })
}

fn compact_cache_ttl(usage: &UsageTotals) -> Option<Duration> {
    if usage.cache_write_1h > 0 {
        Some(Duration::hours(1))
    } else if usage.cache_write_5m > 0 || usage.cache_read > 0 {
        Some(Duration::minutes(5))
    } else {
        None
    }
}

fn usage_totals_from_claude_usage(usage: &ClaudeUsage) -> UsageTotals {
    let cache_write_5m = usage
        .cache_creation
        .as_ref()
        .and_then(|value| value.ephemeral_5m_input_tokens)
        .unwrap_or(0);
    let cache_write_1h = usage
        .cache_creation
        .as_ref()
        .and_then(|value| value.ephemeral_1h_input_tokens)
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
    UsageTotals {
        input,
        output,
        reasoning: 0,
        cache_write_5m: cache_write_5m + remaining_cache_write,
        cache_write_1h,
        cache_read,
        total,
    }
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
        assert_eq!(normalize_claude_model("claude-opus-5"), "opus-5");
        assert_eq!(
            normalize_claude_model("claude-opus-5-20260727"),
            "opus-5"
        );
        assert_eq!(
            normalize_claude_model("claude-fable-5-20260601"),
            "fable-5"
        );
        assert_eq!(normalize_claude_model("claude-fable-5"), "fable-5");
        assert_eq!(
            normalize_claude_model("claude-fable-5-1-20260901"),
            "fable-5-1"
        );
        assert_eq!(normalize_claude_model("claude-fable-5-1"), "fable-5-1");
        assert_eq!(normalize_claude_model("claude-sonnet-5"), "sonnet-5");
        assert_eq!(
            normalize_claude_model("claude-sonnet-5-20260701"),
            "sonnet-5"
        );
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
    fn includes_compact_estimate_with_fresh_one_hour_cache() {
        let path = write_temp_jsonl(&[
            cached_event("2026-03-01T00:00:00Z", "claude-fable-5"),
            compact_event("2026-03-01T00:02:00Z", "compact-fresh", 1200, 100, 60000),
            event(
                "2026-03-01T00:03:00Z",
                "msg-after",
                "claude-fable-5",
                1,
                10,
                0,
                0,
                11,
            ),
        ]);

        let parsed =
            parse_file_detailed(&path, &AggregationTz::parse(Some("UTC")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);
        let compact = parsed
            .message_rows
            .iter()
            .find(|row| row.message_key == "compact:compact-fresh")
            .unwrap();
        assert!(compact.estimated);
        assert_eq!(compact.model, "fable-5");
        assert_eq!(compact.project, "/repo/demo");
        assert_eq!(compact.usage.input, 200);
        assert_eq!(compact.usage.output, 100);
        assert_eq!(compact.usage.cache_read, 1000);
        assert_eq!(compact.usage.total, 1300);
    }

    #[test]
    fn treats_expired_compact_cache_as_regular_input() {
        let path = write_temp_jsonl(&[
            cached_event("2026-03-01T00:00:00Z", "claude-fable-5"),
            compact_event("2026-03-01T02:01:00Z", "compact-expired", 1200, 100, 60000),
            event(
                "2026-03-01T02:02:00Z",
                "msg-after",
                "claude-fable-5",
                1,
                10,
                0,
                0,
                11,
            ),
        ]);

        let parsed =
            parse_file_detailed(&path, &AggregationTz::parse(Some("UTC")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);
        let compact = parsed
            .message_rows
            .iter()
            .find(|row| row.message_key == "compact:compact-expired")
            .unwrap();
        assert!(compact.estimated);
        assert_eq!(compact.usage.input, 1200);
        assert_eq!(compact.usage.cache_read, 0);
        assert_eq!(compact.usage.output, 100);
    }

    #[test]
    fn uses_post_compact_model_and_does_not_reuse_other_model_cache() {
        let path = write_temp_jsonl(&[
            cached_event("2026-03-01T00:00:00Z", "claude-opus-5"),
            compact_event(
                "2026-03-01T00:02:00Z",
                "compact-model-switch",
                1200,
                100,
                60000,
            ),
            event(
                "2026-03-01T00:03:00Z",
                "msg-after",
                "claude-fable-5",
                1,
                10,
                0,
                0,
                11,
            ),
        ]);

        let parsed =
            parse_file_detailed(&path, &AggregationTz::parse(Some("UTC")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);
        let compact = parsed
            .message_rows
            .iter()
            .find(|row| row.message_key == "compact:compact-model-switch")
            .unwrap();
        assert_eq!(compact.model, "fable-5");
        assert_eq!(compact.usage.input, 1200);
        assert_eq!(compact.usage.cache_read, 0);
    }

    #[test]
    fn prefers_exact_compaction_usage_when_present() {
        let path = write_temp_jsonl(&[json!({
            "type": "system",
            "subtype": "compact_boundary",
            "timestamp": "2026-03-01T00:02:00Z",
            "cwd": "/repo/demo",
            "uuid": "compact-exact",
            "model": "claude-fable-5",
            "compactMetadata": {},
            "compactionUsage": {
                "input_tokens": 12,
                "output_tokens": 34,
                "cache_read_input_tokens": 56,
                "cache_creation_input_tokens": 0,
                "total_tokens": 102
            }
        })]);

        let parsed =
            parse_file_detailed(&path, &AggregationTz::parse(Some("UTC")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);
        let compact = parsed
            .message_rows
            .iter()
            .find(|row| row.message_key == "compact:compact-exact")
            .unwrap();
        assert!(!compact.estimated);
        assert_eq!(compact.model, "fable-5");
        assert_eq!(compact.usage.input, 12);
        assert_eq!(compact.usage.output, 34);
        assert_eq!(compact.usage.cache_read, 56);
        assert_eq!(compact.usage.total, 102);
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

    fn cached_event(ts: &str, model: &str) -> Value {
        json!({
            "type": "assistant",
            "timestamp": ts,
            "cwd": "/repo/demo",
            "uuid": "assistant-before",
            "message": {
                "id": "msg-before",
                "model": model,
                "usage": {
                    "input_tokens": 2,
                    "output_tokens": 20,
                    "cache_read_input_tokens": 800,
                    "cache_creation_input_tokens": 200,
                    "cache_creation": {
                        "ephemeral_5m_input_tokens": 0,
                        "ephemeral_1h_input_tokens": 200
                    }
                }
            }
        })
    }

    fn compact_event(
        ts: &str,
        uuid: &str,
        pre_tokens: u64,
        post_tokens: u64,
        duration_ms: u64,
    ) -> Value {
        json!({
            "type": "system",
            "subtype": "compact_boundary",
            "timestamp": ts,
            "cwd": "/repo/demo",
            "uuid": uuid,
            "compactMetadata": {
                "preTokens": pre_tokens,
                "postTokens": post_tokens,
                "durationMs": duration_ms
            }
        })
    }
}
