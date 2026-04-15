use crate::model::{
    CopilotFileDetails, CopilotOtelCache, CopilotOtelSession, CopilotOtelUsageRow, FileCacheEntry,
    FileDailyRow, UsageTotals,
};
use crate::profile;
use crate::timezone::AggregationTz;
use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Instant;

const UNKNOWN_PROJECT: &str = "<unknown-project>";
const COPILOT_OTEL_CHAT_OP_NAME: &str = "chat";

#[derive(Debug, Clone)]
pub struct ParsedCopilotFile {
    pub daily_rows: Vec<FileDailyRow>,
    pub details: CopilotFileDetails,
}

#[derive(Debug, Clone)]
pub struct OtelCacheUpdate {
    pub cache: CopilotOtelCache,
    pub saw_file: bool,
    pub parsed_records: u64,
    pub invalid_records: u64,
}

pub fn parse_file(path: &Path, aggregation_tz: &AggregationTz) -> Result<Vec<FileDailyRow>> {
    Ok(parse_file_detailed(path, aggregation_tz)?.daily_rows)
}

pub fn parse_file_detailed(
    path: &Path,
    aggregation_tz: &AggregationTz,
) -> Result<ParsedCopilotFile> {
    let profile_enabled = profile::enabled();
    let started = Instant::now();
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();

    let mut session_id = session_id_from_path(path);
    let mut session_start_time: Option<DateTime<Utc>> = None;
    let mut project = UNKNOWN_PROJECT.to_string();
    let mut current_model: Option<String> = None;

    // A single events.jsonl may contain multiple segments (resume cycles). Each segment ends
    // with session.shutdown whose modelMetrics cover only that segment, NOT the cumulative total.
    // 一个 events.jsonl 可能包含多个分段（resume 周期）。每次 session.shutdown 的 modelMetrics
    // 只覆盖该分段，不是累计值。
    let mut all_shutdown_metrics: Vec<BTreeMap<String, CopilotModelMetric>> = Vec::new();

    // Compaction tokens tracked per-segment per-model; reset after each shutdown.
    // 压缩 token 按分段按模型跟踪，每次 shutdown 后重置。
    let mut seg_compaction_by_model: BTreeMap<Option<String>, (u64, u64, u64)> = BTreeMap::new();
    let mut total_compaction_by_model: BTreeMap<Option<String>, (u64, u64, u64)> = BTreeMap::new();

    // Fallback output for the trailing segment without shutdown (abnormal exit / still running).
    // 尾部没有 shutdown 的分段只保留 assistant.message 的 output 兜底。
    let mut trailing_output_by_model: BTreeMap<Option<String>, u64> = BTreeMap::new();

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
                            if session_id.is_none() {
                                session_id = data.session_id;
                            }
                            if let Some(start_time) =
                                data.start_time.and_then(|s| parse_timestamp(&s))
                            {
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
                                    session_start_time =
                                        DateTime::from_timestamp_millis(epoch_ms as i64);
                                }
                            }
                            if let Some(cm) = &data.current_model {
                                current_model = Some(cm.clone());
                            }
                            if let Some(mm) = data.model_metrics {
                                all_shutdown_metrics.push(mm);
                            }
                            for (model, (inp, out, cr)) in
                                std::mem::take(&mut seg_compaction_by_model)
                            {
                                let entry =
                                    total_compaction_by_model.entry(model).or_insert((0, 0, 0));
                                entry.0 += inp;
                                entry.1 += out;
                                entry.2 += cr;
                            }
                            trailing_output_by_model.clear();
                        }
                    }
                    "session.compaction_complete" => {
                        if let Some(data) = event.data {
                            if let Some(u) = data.compaction_tokens_used {
                                let comp_model =
                                    current_model.as_deref().map(normalize_copilot_model);
                                let entry = seg_compaction_by_model
                                    .entry(comp_model)
                                    .or_insert((0, 0, 0));
                                entry.0 += u.input.unwrap_or(0);
                                entry.1 += u.output.unwrap_or(0);
                                entry.2 += u.cached_input.unwrap_or(0);
                            }
                        }
                    }
                    "assistant.message" => {
                        if let Some(data) = event.data {
                            if let Some(tokens) = data.output_tokens {
                                if tokens > 0 {
                                    let model =
                                        current_model.as_deref().map(normalize_copilot_model);
                                    *trailing_output_by_model.entry(model).or_insert(0) += tokens;
                                }
                            }
                        }
                    }
                    "session.model_change" => {
                        if let Some(data) = event.data {
                            if let Some(m) = data.new_model {
                                current_model = Some(m);
                            }
                        }
                    }
                    // Copilot CLI emits model on each tool call; use it as a fallback when
                    // session.shutdown / session.model_change are absent.
                    // Copilot CLI 在每次工具调用完成时带上 model 字段；当没有
                    // session.shutdown / session.model_change 时用它做兜底。
                    "tool.execution_complete" => {
                        if let Some(data) = event.data {
                            if let Some(m) = data.model {
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

    for (model, (inp, out, cr)) in std::mem::take(&mut seg_compaction_by_model) {
        let entry = total_compaction_by_model.entry(model).or_insert((0, 0, 0));
        entry.0 += inp;
        entry.1 += out;
        entry.2 += cr;
    }

    let day = session_start_time
        .map(|ts| aggregation_tz.date_for(ts))
        .unwrap_or_else(|| chrono::Utc::now().date_naive());

    let mut shutdown_daily: BTreeMap<(NaiveDate, String, String), UsageTotals> = BTreeMap::new();
    for metrics in &all_shutdown_metrics {
        for (raw_model, metric) in metrics {
            let model = normalize_copilot_model(raw_model);
            let usage = metric.usage.as_ref().cloned().unwrap_or_default();
            let raw_input = usage.input_tokens.unwrap_or(0);
            let output = usage.output_tokens.unwrap_or(0);
            let cache_read = usage.cache_read_tokens.unwrap_or(0);
            let cache_write = usage.cache_write_tokens.unwrap_or(0);
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
            shutdown_daily.entry(key).or_default().add_assign(&totals);
        }
    }

    let fallback_model = || -> String {
        current_model
            .as_deref()
            .or_else(|| {
                all_shutdown_metrics
                    .last()
                    .and_then(|m| m.keys().next())
                    .map(|s| s.as_str())
            })
            .map(normalize_copilot_model)
            .unwrap_or_else(|| "unknown".to_string())
    };

    let mut trailing_daily: BTreeMap<(NaiveDate, String, String), UsageTotals> = BTreeMap::new();
    for (model_opt, output) in &trailing_output_by_model {
        if *output == 0 {
            continue;
        }
        let model = model_opt.clone().unwrap_or_else(&fallback_model);
        let totals = UsageTotals {
            input: 0,
            output: *output,
            reasoning: 0,
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_read: 0,
            total: *output,
        };
        let key = (day, project.clone(), model);
        trailing_daily.entry(key).or_default().add_assign(&totals);
    }

    let mut compaction_daily: BTreeMap<(NaiveDate, String, String), UsageTotals> = BTreeMap::new();
    for (model_opt, (comp_raw_input, comp_output, comp_cache_read)) in &total_compaction_by_model {
        let model = model_opt.clone().unwrap_or_else(&fallback_model);
        let comp_input = comp_raw_input.saturating_sub(*comp_cache_read);
        let comp_total = comp_input + comp_output + comp_cache_read;
        let comp_totals = UsageTotals {
            input: comp_input,
            output: *comp_output,
            reasoning: 0,
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_read: *comp_cache_read,
            total: comp_total,
        };
        let key = (day, project.clone(), model);
        compaction_daily
            .entry(key)
            .or_default()
            .add_assign(&comp_totals);
    }

    let shutdown_rows = rows_from_usage_map(shutdown_daily.clone());
    let compaction_rows = rows_from_usage_map(compaction_daily.clone());
    let trailing_rows = rows_from_usage_map(trailing_daily.clone());

    let mut final_daily = shutdown_daily;
    add_usage_maps(&mut final_daily, &compaction_daily);
    add_usage_maps(&mut final_daily, &trailing_daily);

    if profile_enabled {
        profile::log(format!(
            "copilot parse file={} parsed={} invalid={} daily_rows={} elapsed_ms={}",
            path.display(),
            parsed_records,
            invalid_records,
            final_daily.len(),
            started.elapsed().as_millis()
        ));
    }

    Ok(ParsedCopilotFile {
        daily_rows: rows_from_usage_map(final_daily),
        details: CopilotFileDetails {
            session_id,
            shutdown_rows,
            compaction_rows,
            trailing_output_rows: trailing_rows,
        },
    })
}

pub fn update_otel_cache(
    path: &Path,
    aggregation_tz: &AggregationTz,
    mut cache: CopilotOtelCache,
) -> Result<OtelCacheUpdate> {
    if !path.exists() {
        return Ok(OtelCacheUpdate {
            cache: CopilotOtelCache::default(),
            saw_file: false,
            parsed_records: 0,
            invalid_records: 0,
        });
    }

    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to stat Copilot OTel file {}", path.display()))?;
    let current_size = metadata.len();
    let current_mtime = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let current_inode = file_inode(&metadata);
    let reset_cache = cache.version != crate::model::COPILOT_OTEL_CACHE_VERSION
        || cache.path != path
        || cache.offset > current_size
        || (cache.inode.is_some() && current_inode.is_some() && cache.inode != current_inode);

    if reset_cache {
        cache = CopilotOtelCache {
            version: crate::model::COPILOT_OTEL_CACHE_VERSION,
            path: path.to_path_buf(),
            offset: 0,
            size: 0,
            mtime_ms: 0,
            inode: current_inode,
            sessions: BTreeMap::new(),
        };
    }

    let mut file = File::open(path)
        .with_context(|| format!("failed to open Copilot OTel {}", path.display()))?;
    file.seek(SeekFrom::Start(cache.offset))?;

    let mut payload = Vec::new();
    file.read_to_end(&mut payload)?;
    let Some(last_newline_idx) = payload.iter().rposition(|byte| *byte == b'\n') else {
        cache.path = path.to_path_buf();
        cache.size = current_size;
        cache.mtime_ms = current_mtime;
        cache.inode = current_inode;
        return Ok(OtelCacheUpdate {
            cache,
            saw_file: true,
            parsed_records: 0,
            invalid_records: 0,
        });
    };

    let consumed = &payload[..=last_newline_idx];
    let mut parsed_records = 0u64;
    let mut invalid_records = 0u64;

    for (idx, raw_line) in consumed.split(|byte| *byte == b'\n').enumerate() {
        if raw_line.is_empty() {
            continue;
        }
        let line = match std::str::from_utf8(raw_line) {
            Ok(line) => line.trim(),
            Err(err) => {
                invalid_records += 1;
                eprintln!(
                    "\x1b[31mwarning: skipped invalid Copilot OTel record {}:{} ({})\x1b[0m",
                    path.display(),
                    idx + 1,
                    err
                );
                continue;
            }
        };
        if line.is_empty() {
            continue;
        }

        match serde_json::from_str::<CopilotOtelLine>(line) {
            Ok(record) => {
                if apply_otel_record(&mut cache, record, aggregation_tz) {
                    parsed_records += 1;
                }
            }
            Err(err) => {
                invalid_records += 1;
                eprintln!(
                    "\x1b[31mwarning: skipped invalid Copilot OTel record {}:{} ({})\x1b[0m",
                    path.display(),
                    idx + 1,
                    err
                );
            }
        }
    }

    cache.path = path.to_path_buf();
    cache.offset += consumed.len() as u64;
    cache.size = current_size;
    cache.mtime_ms = current_mtime;
    cache.inode = current_inode;

    Ok(OtelCacheUpdate {
        cache,
        saw_file: true,
        parsed_records,
        invalid_records,
    })
}

pub fn merge_entries_with_otel(
    entries: Vec<FileCacheEntry>,
    otel_sessions: &BTreeMap<String, CopilotOtelSession>,
) -> Vec<FileCacheEntry> {
    let mut seen_sessions = BTreeSet::new();
    let mut merged = Vec::with_capacity(entries.len());

    for mut entry in entries {
        if entry.source != crate::model::SourceKind::Copilot {
            merged.push(entry);
            continue;
        }

        let session_id = entry
            .copilot_details
            .as_ref()
            .and_then(|details| details.session_id.clone())
            .or_else(|| session_id_from_path(&entry.path));
        let Some(session_id) = session_id else {
            merged.push(entry);
            continue;
        };
        seen_sessions.insert(session_id.clone());

        if let Some(otel) = otel_sessions.get(&session_id) {
            entry.daily_rows = merge_daily_rows_with_otel(&entry, otel);
        }
        merged.push(entry);
    }

    for (session_id, otel) in otel_sessions {
        if seen_sessions.contains(session_id) {
            continue;
        }
        let project = otel
            .project_hint
            .clone()
            .or_else(|| otel.git_root_hint.clone())
            .unwrap_or_else(|| UNKNOWN_PROJECT.to_string());
        let daily_rows = resolve_otel_daily_rows(otel, None, &project);
        if daily_rows.is_empty() {
            continue;
        }
        merged.push(FileCacheEntry {
            source: crate::model::SourceKind::Copilot,
            parser_version: crate::cache::parser_version(crate::model::SourceKind::Copilot),
            path: PathBuf::from(format!("<copilot-otel:{session_id}>")),
            size: 0,
            mtime_ms: 0,
            daily_rows,
            claude_message_rows: Vec::new(),
            copilot_details: None,
        });
    }

    merged
}

pub fn default_otel_path() -> Option<PathBuf> {
    std::env::var_os("COPILOT_OTEL_FILE_EXPORTER_PATH")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".copilot/otel.jsonl")))
}

pub fn normalize_copilot_model(raw: &str) -> String {
    let model = raw.trim();
    if let Some(stripped) = model.strip_prefix("claude-") {
        stripped.replace('.', "-")
    } else {
        model.to_string()
    }
}

fn merge_daily_rows_with_otel(
    entry: &FileCacheEntry,
    otel: &CopilotOtelSession,
) -> Vec<FileDailyRow> {
    let Some(details) = &entry.copilot_details else {
        return entry.daily_rows.clone();
    };

    let project = entry
        .daily_rows
        .iter()
        .map(|row| row.project.clone())
        .find(|project| !project.is_empty())
        .or_else(|| otel.project_hint.clone())
        .or_else(|| otel.git_root_hint.clone())
        .unwrap_or_else(|| UNKNOWN_PROJECT.to_string());
    let preferred_date = entry.daily_rows.iter().map(|row| row.date).min();
    let otel_rows = resolve_otel_daily_rows(otel, preferred_date, &project);
    if otel_rows.is_empty() {
        return entry.daily_rows.clone();
    }

    let shutdown_rows = canonicalize_rows(&details.shutdown_rows, preferred_date, &project);
    let compaction_rows = canonicalize_rows(&details.compaction_rows, preferred_date, &project);

    let shutdown_map = rows_to_usage_map(&shutdown_rows, true);
    let mut merged_map = rows_to_usage_map(&shutdown_rows, true);
    add_usage_maps(&mut merged_map, &rows_to_usage_map(&compaction_rows, false));

    let otel_map = rows_to_usage_map(&otel_rows, false);
    for (key, otel_usage) in otel_map {
        let shutdown_usage = shutdown_map.get(&key).cloned().unwrap_or_default();
        let delta = otel_delta_against_shutdown(&otel_usage, &shutdown_usage);
        merged_map.entry(key).or_default().add_assign(&delta);
    }

    rows_from_usage_map(merged_map)
}

fn resolve_otel_daily_rows(
    otel: &CopilotOtelSession,
    preferred_date: Option<NaiveDate>,
    project: &str,
) -> Vec<FileDailyRow> {
    let mut daily: BTreeMap<(NaiveDate, String, String), UsageTotals> = BTreeMap::new();
    for row in &otel.usage_rows {
        let date = preferred_date.unwrap_or(row.date);
        let key = (date, project.to_string(), row.model.clone());
        daily.entry(key).or_default().add_assign(&row.usage);
    }
    rows_from_usage_map(daily)
}

fn canonicalize_rows(
    rows: &[FileDailyRow],
    preferred_date: Option<NaiveDate>,
    project: &str,
) -> Vec<FileDailyRow> {
    rows.iter()
        .map(|row| FileDailyRow {
            date: preferred_date.unwrap_or(row.date),
            project: project.to_string(),
            model: row.model.clone(),
            usage: row.usage.clone(),
        })
        .collect()
}

fn rows_to_usage_map(
    rows: &[FileDailyRow],
    strip_cache_write: bool,
) -> BTreeMap<(NaiveDate, String, String), UsageTotals> {
    let mut map: BTreeMap<(NaiveDate, String, String), UsageTotals> = BTreeMap::new();
    for row in rows {
        let mut usage = row.usage.clone();
        if strip_cache_write {
            usage = usage_without_cache_write(&usage);
        }
        let key = (row.date, row.project.clone(), row.model.clone());
        map.entry(key).or_default().add_assign(&usage);
    }
    map
}

fn usage_without_cache_write(usage: &UsageTotals) -> UsageTotals {
    let cache_write = usage.cache_write();
    let mut stripped = usage.clone();
    stripped.cache_write_5m = 0;
    stripped.cache_write_1h = 0;
    stripped.total = stripped.total.saturating_sub(cache_write);
    stripped
}

fn otel_delta_against_shutdown(
    otel_usage: &UsageTotals,
    shutdown_usage: &UsageTotals,
) -> UsageTotals {
    let input = otel_usage.input.saturating_sub(shutdown_usage.input);
    let output = otel_usage.output.saturating_sub(shutdown_usage.output);
    let cache_read = otel_usage
        .cache_read
        .saturating_sub(shutdown_usage.cache_read);
    let cache_write_5m = otel_usage.cache_write_5m;
    let cache_write_1h = otel_usage.cache_write_1h;
    UsageTotals {
        input,
        output,
        reasoning: 0,
        cache_write_5m,
        cache_write_1h,
        cache_read,
        total: input + output + cache_read + cache_write_5m + cache_write_1h,
    }
}

fn apply_otel_record(
    cache: &mut CopilotOtelCache,
    record: CopilotOtelLine,
    aggregation_tz: &AggregationTz,
) -> bool {
    if record.record_type != "span" {
        return false;
    }
    let attributes = record.attributes.unwrap_or_default();
    let is_chat_span = record
        .name
        .as_deref()
        .map(|name| name == COPILOT_OTEL_CHAT_OP_NAME || name.starts_with("chat "))
        .unwrap_or(false)
        || attr_string(&attributes, "gen_ai.operation.name").as_deref()
            == Some(COPILOT_OTEL_CHAT_OP_NAME);
    if !is_chat_span {
        return false;
    }
    let Some(session_id) = attr_string(&attributes, "gen_ai.conversation.id") else {
        return false;
    };
    let Some(raw_model) = attr_string(&attributes, "gen_ai.response.model")
        .or_else(|| attr_string(&attributes, "gen_ai.request.model"))
    else {
        return false;
    };

    let raw_input = attr_u64(&attributes, "gen_ai.usage.input_tokens").unwrap_or(0);
    let output = attr_u64(&attributes, "gen_ai.usage.output_tokens").unwrap_or(0);
    let cache_read = attr_u64(&attributes, "gen_ai.usage.cache_read.input_tokens").unwrap_or(0);
    let cache_write =
        attr_u64(&attributes, "gen_ai.usage.cache_creation.input_tokens").unwrap_or(0);
    let input = raw_input.saturating_sub(cache_read);
    if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 {
        return false;
    }

    let timestamp = record
        .end_time
        .as_ref()
        .and_then(parse_otel_timestamp)
        .or_else(|| record.start_time.as_ref().and_then(parse_otel_timestamp))
        .unwrap_or_else(Utc::now);
    let day = aggregation_tz.date_for(timestamp);
    let model = normalize_copilot_model(&raw_model);
    let usage = UsageTotals {
        input,
        output,
        reasoning: 0,
        cache_write_5m: cache_write,
        cache_write_1h: 0,
        cache_read,
        total: input + output + cache_read + cache_write,
    };

    let session = cache.sessions.entry(session_id).or_default();
    if session.project_hint.is_none() {
        session.project_hint = attr_string(&attributes, "github.copilot.cwd");
    }
    if session.git_root_hint.is_none() {
        session.git_root_hint = attr_string(&attributes, "github.copilot.git_root");
    }

    let key = (day, model.clone());
    let mut aggregated = BTreeMap::new();
    for row in std::mem::take(&mut session.usage_rows) {
        aggregated.insert((row.date, row.model.clone()), row.usage);
    }
    aggregated.entry(key).or_default().add_assign(&usage);
    session.usage_rows = aggregated
        .into_iter()
        .map(|((date, model), usage)| CopilotOtelUsageRow { date, model, usage })
        .collect();
    true
}

fn attr_string(attributes: &BTreeMap<String, Value>, key: &str) -> Option<String> {
    attributes.get(key)?.as_str().map(str::to_string)
}

fn attr_u64(attributes: &BTreeMap<String, Value>, key: &str) -> Option<u64> {
    attributes.get(key)?.as_u64()
}

fn parse_otel_timestamp(parts: &[i64; 2]) -> Option<DateTime<Utc>> {
    if parts[0] < 0 || parts[1] < 0 {
        return None;
    }
    DateTime::<Utc>::from_timestamp(parts[0], parts[1] as u32)
}

fn rows_from_usage_map(
    daily: BTreeMap<(NaiveDate, String, String), UsageTotals>,
) -> Vec<FileDailyRow> {
    daily
        .into_iter()
        .map(|((date, project, model), usage)| FileDailyRow {
            date,
            project,
            model,
            usage,
        })
        .collect()
}

fn add_usage_maps(
    target: &mut BTreeMap<(NaiveDate, String, String), UsageTotals>,
    source: &BTreeMap<(NaiveDate, String, String), UsageTotals>,
) {
    for (key, usage) in source {
        target.entry(key.clone()).or_default().add_assign(usage);
    }
}

fn parse_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

pub fn session_id_from_path(path: &Path) -> Option<String> {
    for component in path.components().rev() {
        let raw = component.as_os_str().to_str()?;
        let trimmed = raw.strip_suffix(".jsonl").unwrap_or(raw);
        if looks_like_uuid(trimmed) {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn looks_like_uuid(value: &str) -> bool {
    if value.len() != 36 {
        return false;
    }
    for (idx, byte) in value.bytes().enumerate() {
        if matches!(idx, 8 | 13 | 18 | 23) {
            if byte != b'-' {
                return false;
            }
        } else if !byte.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

#[cfg(unix)]
fn file_inode(metadata: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.ino())
}

#[cfg(not(unix))]
fn file_inode(_metadata: &std::fs::Metadata) -> Option<u64> {
    None
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
    session_id: Option<String>,
    start_time: Option<String>,
    session_start_time: Option<u64>,
    context: Option<CopilotContext>,
    model_metrics: Option<BTreeMap<String, CopilotModelMetric>>,
    current_model: Option<String>,
    compaction_tokens_used: Option<CompactionTokensUsed>,
    output_tokens: Option<u64>,
    new_model: Option<String>,
    /// Model identifier from tool.execution_complete events (Copilot CLI format).
    /// tool.execution_complete 事件中的模型标识（Copilot CLI 格式）。
    model: Option<String>,
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

#[derive(Debug, Deserialize)]
struct CopilotOtelLine {
    #[serde(rename = "type")]
    record_type: String,
    name: Option<String>,
    #[serde(rename = "startTime")]
    start_time: Option<[i64; 2]>,
    #[serde(rename = "endTime")]
    end_time: Option<[i64; 2]>,
    attributes: Option<BTreeMap<String, Value>>,
}

#[cfg(test)]
mod tests {
    use super::{
        CopilotOtelCache, default_otel_path, merge_entries_with_otel, normalize_copilot_model,
        parse_file, parse_file_detailed, session_id_from_path, update_otel_cache,
    };
    use crate::cache::parser_version;
    use crate::model::{
        CopilotFileDetails, CopilotOtelSession, FileCacheEntry, FileDailyRow, SourceKind,
        UsageTotals,
    };
    use crate::timezone::AggregationTz;
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    use std::fs;
    use std::io::Write;
    use std::path::{Path, PathBuf};
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
    fn extracts_session_id_from_copilot_path() {
        let path = Path::new(
            "/tmp/.copilot/session-state/00dbb9de-51b2-427c-ad63-ead04dff8e6a/events.jsonl",
        );
        assert_eq!(
            session_id_from_path(path).as_deref(),
            Some("00dbb9de-51b2-427c-ad63-ead04dff8e6a")
        );
    }

    #[test]
    fn parses_shutdown_event_with_model_metrics() {
        let path = write_temp_jsonl(&[
            session_start(
                "2026-03-15T10:00:00Z",
                "/repo/demo",
                Some("00dbb9de-51b2-427c-ad63-ead04dff8e6a"),
            ),
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
        assert_eq!(rows[0].usage.input, 10000);
        assert_eq!(rows[0].usage.output, 2000);
        assert_eq!(rows[0].usage.cache_read, 40000);
        assert_eq!(rows[0].usage.cache_write_5m, 5000);
        assert_eq!(rows[0].usage.total, 57000);
    }

    #[test]
    fn adds_compaction_tokens_to_shutdown_metrics() {
        let path = write_temp_jsonl(&[
            session_start("2026-03-15T10:00:00Z", "/repo/demo", None),
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
        assert_eq!(rows[0].usage.input, 30000);
        assert_eq!(rows[0].usage.output, 11000);
        assert_eq!(rows[0].usage.cache_read, 290000);
    }

    #[test]
    fn includes_trailing_segment_without_shutdown() {
        let path = write_temp_jsonl(&[
            session_start("2026-03-15T10:00:00Z", "/repo/demo", None),
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
            assistant_message(1000),
            assistant_message(500),
        ]);

        let rows = parse_file(&path, &AggregationTz::parse(Some("UTC")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].usage.output, 6500);
    }

    #[test]
    fn extracts_model_from_tool_execution_complete_without_shutdown() {
        let path = write_temp_jsonl(&[
            session_start("2026-03-15T10:00:00Z", "/repo/cli-session", None),
            assistant_message(500),
            tool_execution_complete("claude-opus-4.6"),
            assistant_message(300),
        ]);

        let rows = parse_file(&path, &AggregationTz::parse(Some("UTC")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].model, "opus-4-6");
        assert_eq!(rows[0].usage.output, 800);
        assert_eq!(rows[0].project, "/repo/cli-session");
    }

    #[test]
    fn detailed_parse_preserves_breakdown_rows() {
        let path = write_temp_jsonl(&[
            session_start("2026-03-15T10:00:00Z", "/repo/demo", Some("session-uuid")),
            model_change("claude-opus-4.6"),
            compaction_complete(1000, 20, 800),
            assistant_message(50),
        ]);
        let parsed =
            parse_file_detailed(&path, &AggregationTz::parse(Some("UTC")).unwrap()).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(parsed.details.session_id.as_deref(), Some("session-uuid"));
        assert_eq!(parsed.details.compaction_rows.len(), 1);
        assert_eq!(parsed.details.trailing_output_rows.len(), 1);
    }

    #[test]
    fn otel_cache_reads_incrementally() {
        let path = temp_file_path("copilot-otel");
        fs::write(
            &path,
            format!(
                "{}\n{}\n",
                otel_chat_span(
                    "00dbb9de-51b2-427c-ad63-ead04dff8e6a",
                    "claude-opus-4.6",
                    22966,
                    42,
                    12873,
                    600,
                    "2026-04-13T12:36:33Z"
                ),
                json!({"type":"span","name":"invoke_agent"}).to_string()
            ),
        )
        .unwrap();

        let update = update_otel_cache(
            &path,
            &AggregationTz::parse(Some("UTC")).unwrap(),
            CopilotOtelCache::default(),
        )
        .unwrap();
        assert!(update.saw_file);
        assert_eq!(update.invalid_records, 0);
        assert_eq!(update.parsed_records, 1);
        assert_eq!(update.cache.sessions.len(), 1);

        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(
                format!(
                    "{}\n",
                    otel_chat_span(
                        "00dbb9de-51b2-427c-ad63-ead04dff8e6a",
                        "claude-opus-4.6",
                        100,
                        10,
                        20,
                        30,
                        "2026-04-13T12:40:33Z"
                    )
                )
                .as_bytes(),
            )
            .unwrap();

        let update = update_otel_cache(
            &path,
            &AggregationTz::parse(Some("UTC")).unwrap(),
            update.cache,
        )
        .unwrap();
        let rows = &update
            .cache
            .sessions
            .get("00dbb9de-51b2-427c-ad63-ead04dff8e6a")
            .unwrap()
            .usage_rows;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].usage.output, 52);
        assert_eq!(rows[0].usage.cache_write_5m, 630);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn otel_cache_accepts_chat_name_with_model_suffix() {
        let path = temp_file_path("copilot-otel-chat-suffix");
        fs::write(
            &path,
            format!(
                "{}\n",
                json!({
                    "type": "span",
                    "name": "chat claude-opus-4.6",
                    "startTime": [1776214403, 6000000],
                    "endTime": [1776214414, 479074118],
                    "attributes": {
                        "gen_ai.operation.name": "chat",
                        "gen_ai.conversation.id": "2c862578-d08b-41d5-9a23-b5426dbad00a",
                        "gen_ai.response.model": "claude-opus-4.6",
                        "gen_ai.usage.input_tokens": 45141,
                        "gen_ai.usage.output_tokens": 493,
                        "gen_ai.usage.cache_read.input_tokens": 0
                    }
                })
            ),
        )
        .unwrap();
        let update = update_otel_cache(
            &path,
            &AggregationTz::parse(Some("UTC")).unwrap(),
            CopilotOtelCache::default(),
        )
        .unwrap();
        assert_eq!(update.parsed_records, 1);
        assert!(
            update
                .cache
                .sessions
                .contains_key("2c862578-d08b-41d5-9a23-b5426dbad00a")
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn merges_shutdown_rows_with_otel_cache_write_and_active_delta() {
        let entry = FileCacheEntry {
            source: SourceKind::Copilot,
            parser_version: parser_version(SourceKind::Copilot),
            path: PathBuf::from(
                "/tmp/.copilot/session-state/00dbb9de-51b2-427c-ad63-ead04dff8e6a/events.jsonl",
            ),
            size: 1,
            mtime_ms: 1,
            daily_rows: vec![daily_row(
                "2026-04-13",
                "/repo/demo",
                "opus-4-6",
                usage(10000, 2000, 40000, 0),
            )],
            claude_message_rows: vec![],
            copilot_details: Some(CopilotFileDetails {
                session_id: Some("00dbb9de-51b2-427c-ad63-ead04dff8e6a".to_string()),
                shutdown_rows: vec![daily_row(
                    "2026-04-13",
                    "/repo/demo",
                    "opus-4-6",
                    usage(10000, 2000, 40000, 0),
                )],
                compaction_rows: vec![daily_row(
                    "2026-04-13",
                    "/repo/demo",
                    "opus-4-6",
                    usage(1000, 50, 5000, 0),
                )],
                trailing_output_rows: vec![daily_row(
                    "2026-04-13",
                    "/repo/demo",
                    "opus-4-6",
                    usage(0, 999, 0, 0),
                )],
            }),
        };
        let mut otel_sessions = BTreeMap::new();
        otel_sessions.insert(
            "00dbb9de-51b2-427c-ad63-ead04dff8e6a".to_string(),
            CopilotOtelSession {
                project_hint: Some("/repo/demo".to_string()),
                git_root_hint: None,
                usage_rows: vec![crate::model::CopilotOtelUsageRow {
                    date: chrono::NaiveDate::from_ymd_opt(2026, 4, 13).unwrap(),
                    model: "opus-4-6".to_string(),
                    usage: UsageTotals {
                        input: 13000,
                        output: 2300,
                        cache_read: 45000,
                        cache_write_5m: 700,
                        total: 61000,
                        ..UsageTotals::default()
                    },
                }],
            },
        );

        let merged = merge_entries_with_otel(vec![entry], &otel_sessions);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].daily_rows.len(), 1);
        assert_eq!(merged[0].daily_rows[0].usage.input, 14000);
        assert_eq!(merged[0].daily_rows[0].usage.output, 2350);
        assert_eq!(merged[0].daily_rows[0].usage.cache_read, 50000);
        assert_eq!(merged[0].daily_rows[0].usage.cache_write_5m, 700);
    }

    #[test]
    fn default_otel_path_uses_home_when_env_is_missing() {
        if std::env::var_os("COPILOT_OTEL_FILE_EXPORTER_PATH").is_some() {
            return;
        }
        let path = default_otel_path().unwrap();
        assert!(path.ends_with(".copilot/otel.jsonl"));
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

    fn temp_file_path(prefix: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("{prefix}-{nanos}.jsonl"));
        path
    }

    fn usage(input: u64, output: u64, cache_read: u64, cache_write: u64) -> UsageTotals {
        UsageTotals {
            input,
            output,
            cache_read,
            cache_write_5m: cache_write,
            total: input + output + cache_read + cache_write,
            ..UsageTotals::default()
        }
    }

    fn daily_row(date: &str, project: &str, model: &str, usage: UsageTotals) -> FileDailyRow {
        FileDailyRow {
            date: chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
            project: project.to_string(),
            model: model.to_string(),
            usage,
        }
    }

    fn session_start(ts: &str, cwd: &str, session_id: Option<&str>) -> Value {
        json!({
            "type": "session.start",
            "data": {
                "sessionId": session_id,
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

    fn tool_execution_complete(model: &str) -> Value {
        json!({
            "type": "tool.execution_complete",
            "data": {
                "toolCallId": "test-tool-call",
                "model": model,
                "interactionId": "test-interaction",
                "success": true,
                "result": { "content": "ok" }
            },
            "id": "test-tool-complete-id",
            "timestamp": "2026-03-15T10:10:00Z",
            "parentId": null
        })
    }

    fn otel_chat_span(
        conversation_id: &str,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        cache_write_tokens: u64,
        timestamp: &str,
    ) -> String {
        let dt = chrono::DateTime::parse_from_rfc3339(timestamp).unwrap();
        let secs = dt.timestamp();
        let nanos = dt.timestamp_subsec_nanos();
        json!({
            "type": "span",
            "traceId": "22934c6d37e1ccb99ea22d17e837d882",
            "spanId": "5695c64053a8bdda",
            "name": "chat",
            "kind": 2,
            "startTime": [secs, nanos],
            "endTime": [secs, nanos],
            "attributes": {
                "gen_ai.operation.name": "chat",
                "gen_ai.provider.name": "github",
                "gen_ai.conversation.id": conversation_id,
                "gen_ai.response.model": model,
                "gen_ai.usage.input_tokens": input_tokens,
                "gen_ai.usage.output_tokens": output_tokens,
                "gen_ai.usage.cache_read.input_tokens": cache_read_tokens,
                "gen_ai.usage.cache_creation.input_tokens": cache_write_tokens,
                "github.copilot.cwd": "/repo/demo"
            },
            "status": { "code": 0 }
        })
        .to_string()
    }
}
