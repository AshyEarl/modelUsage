use crate::model::{FileDailyRow, UsageTotals};
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

pub fn parse_file(path: &Path, aggregation_tz: &AggregationTz) -> Result<Vec<FileDailyRow>> {
    let profile_enabled = profile::enabled();
    let started = Instant::now();
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();

    let mut session_start_time: Option<DateTime<Utc>> = None;
    let mut project = "<unknown-project>".to_string();
    let mut current_model: Option<String> = None;

    // A single events.jsonl may contain multiple segments (resume cycles). Each segment ends
    // with session.shutdown whose modelMetrics cover only that segment, NOT the cumulative total.
    // We must sum all shutdown events plus any trailing segment without shutdown (abnormal exit).
    // 一个 events.jsonl 可能包含多个分段（resume 周期）。每次 session.shutdown 的 modelMetrics
    // 只覆盖该分段，不是累计值。需要把所有 shutdown 事件和尾部无 shutdown 的分段全部加起来。
    let mut all_shutdown_metrics: Vec<BTreeMap<String, CopilotModelMetric>> = Vec::new();

    // Compaction tokens tracked per-segment; reset after each shutdown.
    // 压缩 token 按分段跟踪，每次 shutdown 后重置。
    let mut seg_compaction_input: u64 = 0;
    let mut seg_compaction_output: u64 = 0;
    let mut seg_compaction_cache_read: u64 = 0;
    // Accumulated compaction totals across all segments.
    // 所有分段的压缩 token 累计。
    let mut total_compaction_input: u64 = 0;
    let mut total_compaction_output: u64 = 0;
    let mut total_compaction_cache_read: u64 = 0;

    // Fallback output for the trailing segment without shutdown (abnormal exit / still running).
    // 尾部没有 shutdown 的分段（异常退出或仍在运行）的 output 备用累加。
    let mut trailing_fallback_output: u64 = 0;

    let mut record_no: u64 = 0;
    let mut parsed_records: u64 = 0;
    let mut invalid_records: u64 = 0;

    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        record_no += 1;

        match serde_json::from_str::<CopilotEvent>(trimmed) {
            Ok(event) => {
                parsed_records += 1;
                match event.event_type.as_str() {
                    "session.start" => {
                        if let Some(data) = event.data {
                            if let Some(start_time) = data.start_time.and_then(|s| parse_timestamp(&s)) {
                                session_start_time = Some(start_time);
                            }
                            if let Some(ctx) = data.context {
                                if let Some(cwd) = ctx.cwd {
                                    project = cwd;
                                }
                            }
                        }
                    }
                    "session.shutdown" => {
                        if let Some(data) = event.data {
                            if session_start_time.is_none() {
                                if let Some(epoch_ms) = data.session_start_time {
                                    session_start_time = DateTime::from_timestamp_millis(epoch_ms as i64);
                                }
                            }
                            if let Some(cm) = &data.current_model {
                                current_model = Some(cm.clone());
                            }
                            if let Some(mm) = data.model_metrics {
                                all_shutdown_metrics.push(mm);
                            }
                            // Flush segment compaction into total and reset.
                            // 将本段压缩 token 汇入总计并重置。
                            total_compaction_input += seg_compaction_input;
                            total_compaction_output += seg_compaction_output;
                            total_compaction_cache_read += seg_compaction_cache_read;
                            seg_compaction_input = 0;
                            seg_compaction_output = 0;
                            seg_compaction_cache_read = 0;
                            trailing_fallback_output = 0;
                        }
                    }
                    "session.compaction_complete" => {
                        if let Some(data) = event.data {
                            if let Some(u) = data.compaction_tokens_used {
                                seg_compaction_input += u.input.unwrap_or(0);
                                seg_compaction_output += u.output.unwrap_or(0);
                                seg_compaction_cache_read += u.cached_input.unwrap_or(0);
                            }
                        }
                    }
                    "assistant.message" => {
                        if let Some(data) = event.data {
                            trailing_fallback_output += data.output_tokens.unwrap_or(0);
                        }
                    }
                    "session.model_change" => {
                        if let Some(data) = event.data {
                            if let Some(m) = data.new_model {
                                current_model = Some(m);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Err(err) => {
                invalid_records += 1;
                eprintln!(
                    "\x1b[31mwarning: skipped invalid Copilot JSONL record {}:{} ({})\x1b[0m",
                    path.display(),
                    record_no,
                    err
                );
            }
        }
    }

    // Include trailing segment compaction (segment still running or crashed).
    // 把尾部分段的压缩 token 也算进去。
    total_compaction_input += seg_compaction_input;
    total_compaction_output += seg_compaction_output;
    total_compaction_cache_read += seg_compaction_cache_read;

    let mut daily: BTreeMap<(NaiveDate, String, String), UsageTotals> = BTreeMap::new();
    let day = session_start_time
        .map(|ts| aggregation_tz.date_for(ts))
        .unwrap_or_else(|| chrono::Utc::now().date_naive());

    let has_compaction = total_compaction_input > 0 || total_compaction_output > 0;

    // Sum all shutdown segments.
    // 累加所有 shutdown 分段。
    for metrics in &all_shutdown_metrics {
        for (raw_model, metric) in metrics {
            let model = normalize_copilot_model(raw_model);
            let usage = metric.usage.as_ref().cloned().unwrap_or_default();
            let raw_input = usage.input_tokens.unwrap_or(0);
            let output = usage.output_tokens.unwrap_or(0);
            let cache_read = usage.cache_read_tokens.unwrap_or(0);
            let cache_write = usage.cache_write_tokens.unwrap_or(0);
            // Copilot's inputTokens follows Anthropic API semantics: it includes cacheReadTokens.
            // Copilot 的 inputTokens 包含了 cacheReadTokens，需要减去。
            let input = raw_input.saturating_sub(cache_read);
            let total = input + output + cache_read + cache_write;

            let totals = UsageTotals {
                input,
                output,
                reasoning: 0,
                cache_write_5m: cache_write,
                cache_write_1h: 0,
                cache_read,
                total,
            };

            let key = (day, project.clone(), model);
            daily.entry(key).or_default().add_assign(&totals);
        }
    }

    // Add trailing segment fallback (no shutdown = abnormal exit or still running).
    // 尾部无 shutdown 分段的兜底（异常退出或仍在运行）。
    if trailing_fallback_output > 0 {
        let model = current_model
            .as_deref()
            .map(|s| normalize_copilot_model(s))
            .unwrap_or_else(|| "unknown".to_string());
        let totals = UsageTotals {
            input: 0,
            output: trailing_fallback_output,
            reasoning: 0,
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_read: 0,
            total: trailing_fallback_output,
        };
        let key = (day, project.clone(), model);
        daily.entry(key).or_default().add_assign(&totals);
    }

    // Add compaction tokens. Not tracked in any shutdown's modelMetrics.
    // 累加压缩 token，不包含在任何 shutdown 的 modelMetrics 里。
    if has_compaction {
        let comp_model = current_model
            .as_deref()
            .or_else(|| {
                all_shutdown_metrics
                    .last()
                    .and_then(|m| m.keys().next())
                    .map(|s| s.as_str())
            })
            .map(|s| normalize_copilot_model(s))
            .unwrap_or_else(|| "unknown".to_string());
        let comp_input = total_compaction_input.saturating_sub(total_compaction_cache_read);
        let comp_total = comp_input + total_compaction_output + total_compaction_cache_read;
        let comp_totals = UsageTotals {
            input: comp_input,
            output: total_compaction_output,
            reasoning: 0,
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_read: total_compaction_cache_read,
            total: comp_total,
        };
        let key = (day, project.clone(), comp_model);
        daily.entry(key).or_default().add_assign(&comp_totals);
    }

    if profile_enabled {
        profile::log(format!(
            "copilot parse file={} parsed={} invalid={} daily_rows={} elapsed_ms={}",
            path.display(),
            parsed_records,
            invalid_records,
            daily.len(),
            started.elapsed().as_millis()
        ));
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

// --- Deserialization structs ---

#[derive(Debug, Deserialize)]
struct CopilotEvent {
    #[serde(rename = "type")]
    event_type: String,
    data: Option<CopilotEventData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CopilotEventData {
    start_time: Option<String>,
    session_start_time: Option<u64>,
    context: Option<CopilotContext>,
    model_metrics: Option<BTreeMap<String, CopilotModelMetric>>,
    current_model: Option<String>,
    compaction_tokens_used: Option<CompactionTokensUsed>,
    output_tokens: Option<u64>,
    new_model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CopilotContext {
    cwd: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CopilotModelMetric {
    usage: Option<CopilotUsage>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CopilotUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    cache_write_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompactionTokensUsed {
    input: Option<u64>,
    output: Option<u64>,
    cached_input: Option<u64>,
}

fn parse_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Normalize Copilot model names to match pricing keys.
/// Copilot uses dots in Claude versions (e.g. claude-opus-4.6) while pricing uses dashes (opus-4-6).
/// 将 Copilot 的模型名归一化到 pricing key 格式。
/// Copilot 对 Claude 版本号用点（如 claude-opus-4.6），pricing 文件用短横线（opus-4-6）。
pub fn normalize_copilot_model(raw: &str) -> String {
    let model = raw.trim();
    if let Some(stripped) = model.strip_prefix("claude-") {
        stripped.replace('.', "-")
    } else {
        model.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_copilot_model, parse_file};
    use crate::timezone::AggregationTz;
    use serde_json::{Value, json};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn normalizes_claude_models() {
        assert_eq!(normalize_copilot_model("claude-opus-4.6"), "opus-4-6");
        assert_eq!(normalize_copilot_model("claude-opus-4-6"), "opus-4-6");
        assert_eq!(normalize_copilot_model("claude-sonnet-4-5"), "sonnet-4-5");
        assert_eq!(normalize_copilot_model("claude-haiku-4.5"), "haiku-4-5");
    }

    #[test]
    fn keeps_non_claude_models() {
        assert_eq!(normalize_copilot_model("gpt-5.4"), "gpt-5.4");
        assert_eq!(normalize_copilot_model("gpt-5"), "gpt-5");
        assert_eq!(normalize_copilot_model("gpt-5-codex"), "gpt-5-codex");
    }

    #[test]
    fn parses_shutdown_event_with_model_metrics() {
        let path = write_temp_jsonl(&[
            session_start("2026-03-15T10:00:00Z", "/repo/demo"),
            session_shutdown(json!({
                "claude-opus-4.6": {
                    "requests": { "count": 2, "cost": 6 },
                    "usage": {
                        "inputTokens": 50000,
                        "outputTokens": 2000,
                        "cacheReadTokens": 40000,
                        "cacheWriteTokens": 5000
                    }
                }
            })),
        ]);

        let rows = parse_file(&path, &AggregationTz::parse(Some("UTC")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].project, "/repo/demo");
        assert_eq!(rows[0].model, "opus-4-6");
        assert_eq!(rows[0].date.to_string(), "2026-03-15");
        assert_eq!(rows[0].usage.input, 10000); // 50000 - 40000 (cache_read subtracted)
        assert_eq!(rows[0].usage.output, 2000);
        assert_eq!(rows[0].usage.cache_read, 40000);
        assert_eq!(rows[0].usage.cache_write_5m, 5000);
        assert_eq!(rows[0].usage.total, 57000); // 10000 + 2000 + 40000 + 5000
    }

    #[test]
    fn adds_compaction_tokens_to_shutdown_metrics() {
        let path = write_temp_jsonl(&[
            session_start("2026-03-15T10:00:00Z", "/repo/demo"),
            compaction_complete(120000, 3000, 110000),
            session_shutdown(json!({
                "claude-opus-4.6": {
                    "requests": { "count": 5, "cost": 15 },
                    "usage": {
                        "inputTokens": 200000,
                        "outputTokens": 8000,
                        "cacheReadTokens": 180000,
                        "cacheWriteTokens": 0
                    }
                }
            })),
        ]);

        let rows = parse_file(&path, &AggregationTz::parse(Some("UTC")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(rows.len(), 1);
        // shutdown: input=200000-180000=20000, output=8000, cache_read=180000
        // compaction: input=120000-110000=10000, output=3000, cache_read=110000
        // total: input=30000, output=11000, cache_read=290000
        assert_eq!(rows[0].usage.input, 30000);
        assert_eq!(rows[0].usage.output, 11000);
        assert_eq!(rows[0].usage.cache_read, 290000);
    }

    #[test]
    fn sums_multiple_shutdown_segments() {
        let path = write_temp_jsonl(&[
            session_start("2026-03-15T10:00:00Z", "/repo/demo"),
            // First segment
            session_shutdown(json!({
                "claude-opus-4.6": {
                    "requests": { "count": 10, "cost": 30 },
                    "usage": {
                        "inputTokens": 500000,
                        "outputTokens": 20000,
                        "cacheReadTokens": 400000,
                        "cacheWriteTokens": 0
                    }
                }
            })),
            // Second segment (resume)
            session_shutdown(json!({
                "claude-opus-4.6": {
                    "requests": { "count": 5, "cost": 15 },
                    "usage": {
                        "inputTokens": 200000,
                        "outputTokens": 8000,
                        "cacheReadTokens": 180000,
                        "cacheWriteTokens": 0
                    }
                }
            })),
        ]);

        let rows = parse_file(&path, &AggregationTz::parse(Some("UTC")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(rows.len(), 1);
        // segment 1: input=500000-400000=100000, output=20000, cache_read=400000
        // segment 2: input=200000-180000=20000, output=8000, cache_read=180000
        // total: input=120000, output=28000, cache_read=580000
        assert_eq!(rows[0].usage.input, 120000);
        assert_eq!(rows[0].usage.output, 28000);
        assert_eq!(rows[0].usage.cache_read, 580000);
    }

    #[test]
    fn includes_trailing_segment_without_shutdown() {
        let path = write_temp_jsonl(&[
            session_start("2026-03-15T10:00:00Z", "/repo/demo"),
            // First segment with shutdown
            session_shutdown(json!({
                "claude-opus-4.6": {
                    "requests": { "count": 5, "cost": 15 },
                    "usage": {
                        "inputTokens": 100000,
                        "outputTokens": 5000,
                        "cacheReadTokens": 80000,
                        "cacheWriteTokens": 0
                    }
                }
            })),
            // Trailing segment (still running / crashed)
            assistant_message(1000),
            assistant_message(500),
        ]);

        let rows = parse_file(&path, &AggregationTz::parse(Some("UTC")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(rows.len(), 1);
        // shutdown: input=20000, output=5000
        // trailing: output=1500
        assert_eq!(rows[0].usage.output, 6500);
    }

    #[test]
    fn falls_back_to_assistant_messages_on_abnormal_exit() {
        let path = write_temp_jsonl(&[
            session_start("2026-03-15T10:00:00Z", "/repo/demo"),
            model_change("claude-opus-4.6"),
            assistant_message(500),
            assistant_message(300),
        ]);

        let rows = parse_file(&path, &AggregationTz::parse(Some("UTC")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].model, "opus-4-6");
        assert_eq!(rows[0].usage.output, 800);
        assert_eq!(rows[0].usage.input, 0);
    }

    #[test]
    fn aggregates_multiple_models_in_same_session() {
        let path = write_temp_jsonl(&[
            session_start("2026-03-15T10:00:00Z", "/repo/demo"),
            session_shutdown(json!({
                "claude-opus-4.6": {
                    "requests": { "count": 1, "cost": 3 },
                    "usage": {
                        "inputTokens": 10000,
                        "outputTokens": 500,
                        "cacheReadTokens": 0,
                        "cacheWriteTokens": 0
                    }
                },
                "gpt-5.4": {
                    "requests": { "count": 1, "cost": 1 },
                    "usage": {
                        "inputTokens": 5000,
                        "outputTokens": 300,
                        "cacheReadTokens": 0,
                        "cacheWriteTokens": 0
                    }
                }
            })),
        ]);

        let rows = parse_file(&path, &AggregationTz::parse(Some("UTC")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(rows.len(), 2);
        let opus_row = rows.iter().find(|r| r.model == "opus-4-6").unwrap();
        let gpt_row = rows.iter().find(|r| r.model == "gpt-5.4").unwrap();
        assert_eq!(opus_row.usage.input, 10000); // no cache_read, so unchanged
        assert_eq!(gpt_row.usage.input, 5000);
    }

    #[test]
    fn respects_timezone_for_date_bucketing() {
        let path = write_temp_jsonl(&[
            session_start("2026-03-15T20:30:00Z", "/repo/demo"),
            session_shutdown(json!({
                "claude-opus-4.6": {
                    "requests": { "count": 1, "cost": 3 },
                    "usage": {
                        "inputTokens": 1000,
                        "outputTokens": 100,
                        "cacheReadTokens": 0,
                        "cacheWriteTokens": 0
                    }
                }
            })),
        ]);

        let rows = parse_file(&path, &AggregationTz::parse(Some("UTC+8")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].date.to_string(), "2026-03-16");
    }

    #[test]
    fn returns_empty_when_no_shutdown_event() {
        let path = write_temp_jsonl(&[
            session_start("2026-03-15T10:00:00Z", "/repo/demo"),
        ]);

        let rows = parse_file(&path, &AggregationTz::parse(Some("UTC")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);
        assert!(rows.is_empty());
    }

    fn write_temp_jsonl(lines: &[Value]) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("modelusage-copilot-test-{nanos}.jsonl"));
        let mut payload = String::new();
        for line in lines {
            payload.push_str(&line.to_string());
            payload.push('\n');
        }
        fs::write(&path, payload).unwrap();
        path
    }

    fn session_start(ts: &str, cwd: &str) -> Value {
        json!({
            "type": "session.start",
            "data": {
                "startTime": ts,
                "context": { "cwd": cwd }
            },
            "id": "test-id",
            "timestamp": ts,
            "parentId": null
        })
    }

    fn session_shutdown(model_metrics: Value) -> Value {
        json!({
            "type": "session.shutdown",
            "data": {
                "shutdownType": "routine",
                "totalPremiumRequests": 3,
                "currentModel": "claude-opus-4.6",
                "modelMetrics": model_metrics
            },
            "id": "test-shutdown-id",
            "timestamp": "2026-03-15T11:00:00Z",
            "parentId": null
        })
    }

    fn compaction_complete(input: u64, output: u64, cached_input: u64) -> Value {
        json!({
            "type": "session.compaction_complete",
            "data": {
                "success": true,
                "compactionTokensUsed": {
                    "input": input,
                    "output": output,
                    "cachedInput": cached_input
                }
            },
            "id": "test-compact-id",
            "timestamp": "2026-03-15T10:30:00Z",
            "parentId": null
        })
    }

    fn assistant_message(output_tokens: u64) -> Value {
        json!({
            "type": "assistant.message",
            "data": {
                "outputTokens": output_tokens
            },
            "id": "test-msg-id",
            "timestamp": "2026-03-15T10:15:00Z",
            "parentId": null
        })
    }

    fn model_change(model: &str) -> Value {
        json!({
            "type": "session.model_change",
            "data": {
                "newModel": model,
                "previousModel": "unknown"
            },
            "id": "test-model-change-id",
            "timestamp": "2026-03-15T10:05:00Z",
            "parentId": null
        })
    }
}
