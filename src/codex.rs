use crate::model::{
    CodexFileDetails, CodexTokenRow, FileCacheEntry, FileDailyRow, SourceKind, UsageTotals,
};
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

#[derive(Debug)]
pub struct ParsedCodexFile {
    pub daily_rows: Vec<FileDailyRow>,
    pub details: CodexFileDetails,
}

pub fn parse_file_detailed(path: &Path, aggregation_tz: &AggregationTz) -> Result<ParsedCodexFile> {
    let profile_enabled = profile::enabled();
    let started = Instant::now();
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    let mut current_model = String::new();
    let mut project = "<unknown-project>".to_string();
    let mut previous_total: Option<RawUsage> = None;
    let mut daily: BTreeMap<(NaiveDate, String, String), UsageTotals> = BTreeMap::new();
    let mut record_no: u64 = 0;
    let mut parsed_records: u64 = 0;
    let mut invalid_records: u64 = 0;
    let mut empty_records: u64 = 0;
    let mut token_count_events: u64 = 0;
    let mut emitted_events: u64 = 0;
    let mut details = CodexFileDetails::default();
    let mut captured_session_meta = false;
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

        match serde_json::from_str::<CodexRecord>(trimmed) {
            Ok(record) => {
                parsed_records += 1;
                match record.record_type.as_deref() {
                    Some("session_meta") => {
                        if let Some(payload) = record.payload {
                            // A v2 sub-agent rollout starts with its own metadata and then replays
                            // the parent's metadata, so session identity must come from the first row.
                            // v2 子 agent rollout 先写自己的元数据，再回放父线程元数据，
                            // 因此会话身份必须取第一条。
                            if !captured_session_meta {
                                details.thread_id = payload.id.clone();
                                details.parent_thread_id = payload.parent_thread_id.clone();
                                details.forked_from_id = payload.forked_from_id.clone();
                                captured_session_meta = true;
                            }
                            if let Some(cwd) = payload.cwd {
                                project = cwd;
                            }
                        }
                    }
                    Some("turn_context") => {
                        if let Some(model) = record.payload.and_then(|payload| payload.model) {
                            current_model = normalize_codex_model(&model);
                        }
                    }
                    Some("event_msg") => {
                        let timestamp = match record.timestamp.as_deref().and_then(parse_timestamp)
                        {
                            Some(ts) => ts,
                            None => continue,
                        };
                        let payload = match record.payload {
                            Some(payload)
                                if payload.payload_type.as_deref() == Some("token_count") =>
                            {
                                payload
                            }
                            _ => continue,
                        };
                        let info = match payload.info {
                            Some(info) => info,
                            None => continue,
                        };
                        token_count_events += 1;
                        let last_usage = info.last_token_usage.as_ref().map(parse_raw_usage);
                        let total_usage = info.total_token_usage.as_ref().map(parse_raw_usage);
                        // Prefer cumulative-delta when total_token_usage exists to avoid duplicate snapshot inflation.
                        // 优先用 total_token_usage 做累计差分，避免重复快照（例如 rate-limit 刷新）被重复累计。
                        let Some(raw_usage) = choose_raw_usage(
                            last_usage.as_ref(),
                            total_usage.as_ref(),
                            previous_total.as_ref(),
                        ) else {
                            continue;
                        };
                        if let Some(total) = total_usage.as_ref() {
                            previous_total = Some(total.clone());
                        }
                        if raw_usage.is_zero() {
                            continue;
                        }
                        emitted_events += 1;
                        let model = if current_model.is_empty() {
                            "unknown-codex-model".to_string()
                        } else {
                            current_model.clone()
                        };
                        let day = aggregation_tz.date_for(timestamp);
                        let usage = raw_usage.into_usage_totals();
                        details
                            .token_fingerprints
                            .push(token_fingerprint(last_usage.as_ref(), total_usage.as_ref()));
                        if details.forked_from_id.is_some() {
                            details.token_rows.push(CodexTokenRow {
                                date: day,
                                project: project.clone(),
                                model: model.clone(),
                                usage: usage.clone(),
                            });
                        }
                        let key = (day, project.clone(), model);
                        daily.entry(key).or_default().add_assign(&usage);
                    }
                    _ => {}
                }
            }
            Err(err) => {
                invalid_records += 1;
                eprintln!(
                    "\x1b[31mwarning: skipped invalid Codex JSONL record {}:{} ({})\x1b[0m",
                    path.display(),
                    record_no,
                    err
                );
            }
        }
    }

    if profile_enabled {
        profile::log(format!(
            "codex parse file={} parsed={} invalid={} empty={} token_events={} emitted_events={} daily_rows={} elapsed_ms={}",
            path.display(),
            parsed_records,
            invalid_records,
            empty_records,
            token_count_events,
            emitted_events,
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

    Ok(ParsedCodexFile {
        daily_rows,
        details,
    })
}

#[derive(Debug, Deserialize)]
struct CodexRecord {
    #[serde(rename = "type")]
    record_type: Option<String>,
    timestamp: Option<String>,
    payload: Option<CodexPayload>,
}

#[derive(Debug, Deserialize)]
struct CodexPayload {
    #[serde(rename = "type")]
    payload_type: Option<String>,
    id: Option<String>,
    parent_thread_id: Option<String>,
    forked_from_id: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    info: Option<CodexInfo>,
}

#[derive(Debug, Deserialize)]
struct CodexInfo {
    last_token_usage: Option<RawUsageWire>,
    total_token_usage: Option<RawUsageWire>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawUsageWire {
    input_tokens: Option<u64>,
    cached_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    reasoning_output_tokens: Option<u64>,
    total_tokens: Option<u64>,
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

fn token_fingerprint(last: Option<&RawUsage>, total: Option<&RawUsage>) -> String {
    fn usage_part(usage: Option<&RawUsage>) -> String {
        match usage {
            Some(usage) => format!(
                "{},{},{},{},{}",
                usage.input, usage.cached_input, usage.output, usage.reasoning, usage.total
            ),
            None => "-".to_string(),
        }
    }

    format!("{}|{}", usage_part(last), usage_part(total))
}

fn parse_raw_usage(value: &RawUsageWire) -> RawUsage {
    RawUsage {
        input: value.input_tokens.unwrap_or(0),
        cached_input: value
            .cached_input_tokens
            .or(value.cache_read_input_tokens)
            .unwrap_or(0),
        output: value.output_tokens.unwrap_or(0),
        reasoning: value.reasoning_output_tokens.unwrap_or(0),
        total: value.total_tokens.unwrap_or(0),
    }
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

pub fn reconcile_forked_entries(mut entries: Vec<FileCacheEntry>) -> Vec<FileCacheEntry> {
    let thread_entries: BTreeMap<String, usize> = entries
        .iter()
        .enumerate()
        .filter_map(|(idx, entry)| {
            if entry.source != SourceKind::Codex {
                return None;
            }
            entry
                .codex_details
                .as_ref()?
                .thread_id
                .clone()
                .map(|thread_id| (thread_id, idx))
        })
        .collect();

    let replay_prefixes: Vec<usize> = entries
        .iter()
        .map(|entry| {
            if entry.source != SourceKind::Codex {
                return 0;
            }
            let Some(details) = entry.codex_details.as_ref() else {
                return 0;
            };
            if details.fork_reconciled {
                return 0;
            }
            // v1 sub-agent files have parent_thread_id but no copied history.
            // Only forked_from_id marks a rollout whose parent prefix must be reconciled.
            // v1 子 agent 文件虽然有 parent_thread_id，但没有复制历史。
            // 只有 forked_from_id 才表示需要对账父线程前缀的 rollout。
            let Some(parent_id) = details.forked_from_id.as_ref() else {
                return 0;
            };
            let Some(parent_idx) = thread_entries.get(parent_id).copied() else {
                return 0;
            };
            let Some(parent_details) = entries[parent_idx].codex_details.as_ref() else {
                return 0;
            };
            common_token_prefix(
                &parent_details.token_fingerprints,
                &details.token_fingerprints,
            )
        })
        .collect();

    let mut reconciled_files = 0usize;
    let mut dropped_rows = 0usize;
    for (entry, replay_prefix) in entries.iter_mut().zip(replay_prefixes) {
        if replay_prefix == 0 {
            continue;
        }
        let Some(details) = entry.codex_details.as_ref() else {
            continue;
        };
        entry.daily_rows = daily_rows_from_tokens(details.token_rows.iter().skip(replay_prefix));
        if let Some(details) = entry.codex_details.as_mut() {
            details.fork_reconciled = true;
        }
        reconciled_files += 1;
        dropped_rows += replay_prefix;
    }

    let parent_ids: std::collections::BTreeSet<String> = entries
        .iter()
        .filter_map(|entry| {
            entry
                .codex_details
                .as_ref()?
                .forked_from_id
                .as_ref()
                .cloned()
        })
        .collect();
    for entry in &mut entries {
        let Some(details) = entry.codex_details.as_mut() else {
            continue;
        };
        if details.fork_reconciled {
            details.token_rows.clear();
        }
        let is_parent = details
            .thread_id
            .as_ref()
            .is_some_and(|thread_id| parent_ids.contains(thread_id));
        let is_unresolved_fork = details.forked_from_id.is_some() && !details.fork_reconciled;
        if !is_parent && !is_unresolved_fork {
            details.token_fingerprints.clear();
        }
    }

    if profile::enabled() && reconciled_files > 0 {
        profile::log(format!(
            "codex fork reconciliation files={} dropped_replay_rows={}",
            reconciled_files, dropped_rows
        ));
    }
    entries
}

fn common_token_prefix(parent: &[String], child: &[String]) -> usize {
    parent
        .iter()
        .zip(child)
        .take_while(|(parent_row, child_row)| parent_row == child_row)
        .count()
}

fn daily_rows_from_tokens<'a>(rows: impl Iterator<Item = &'a CodexTokenRow>) -> Vec<FileDailyRow> {
    let mut daily: BTreeMap<(NaiveDate, String, String), UsageTotals> = BTreeMap::new();
    for row in rows {
        let key = (row.date, row.project.clone(), row.model.clone());
        daily.entry(key).or_default().add_assign(&row.usage);
    }
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

#[cfg(test)]
mod tests {
    use super::{
        normalize_codex_model, parse_file_detailed, reconcile_forked_entries, ParsedCodexFile,
    };
    use crate::cache::parser_version;
    use crate::model::{FileCacheEntry, SourceKind};
    use crate::timezone::AggregationTz;
    use serde_json::{json, Value};
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
        assert_eq!(normalize_codex_model("gpt-5.6-sol"), "gpt-5.6-sol");
        assert_eq!(normalize_codex_model("gpt-5.6-terra"), "gpt-5.6-terra");
        assert_eq!(normalize_codex_model("gpt-5.6-luna"), "gpt-5.6-luna");
        assert_eq!(normalize_codex_model("gpt-5.6"), "gpt-5.6");
    }

    #[test]
    fn removes_replayed_parent_prefix_from_parallel_and_nested_forks() {
        let root_id = "11111111-1111-7111-8111-111111111111";
        let child_id = "22222222-2222-7222-8222-222222222222";
        let sibling_id = "33333333-3333-7333-8333-333333333333";
        let grandchild_id = "44444444-4444-7444-8444-444444444444";
        let root_first = token_count_event(
            "2026-03-01T00:00:01Z",
            80,
            60,
            20,
            0,
            100,
            80,
            60,
            20,
            0,
            100,
        );
        let root_second = token_count_event(
            "2026-03-01T00:00:02Z",
            130,
            100,
            30,
            0,
            160,
            50,
            40,
            10,
            0,
            60,
        );
        let child_new = token_count_event(
            "2026-03-02T00:01:00Z",
            180,
            140,
            40,
            0,
            220,
            50,
            40,
            10,
            0,
            60,
        );
        let sibling_new = token_count_event(
            "2026-03-02T00:02:00Z",
            170,
            130,
            40,
            0,
            210,
            40,
            30,
            10,
            0,
            50,
        );
        let grandchild_new = token_count_event(
            "2026-03-02T00:03:00Z",
            205,
            160,
            45,
            0,
            250,
            25,
            20,
            5,
            0,
            30,
        );

        let root = parse_entry(&[
            session_meta_with_relation("2026-03-01T00:00:00Z", root_id, "/repo/codex", None, None),
            turn_context("gpt-5.6-sol"),
            root_first.clone(),
            root_second.clone(),
        ]);
        let child = parse_entry(&[
            session_meta_with_relation(
                "2026-03-02T00:00:00Z",
                child_id,
                "/repo/codex",
                Some(root_id),
                Some(root_id),
            ),
            session_meta_with_relation("2026-03-02T00:00:00Z", root_id, "/repo/codex", None, None),
            turn_context("gpt-5.6-sol"),
            retimestamp(&root_first, "2026-03-02T00:00:00Z"),
            retimestamp(&root_second, "2026-03-02T00:00:00Z"),
            child_new.clone(),
        ]);
        let sibling = parse_entry(&[
            session_meta_with_relation(
                "2026-03-02T00:00:00Z",
                sibling_id,
                "/repo/codex",
                Some(root_id),
                Some(root_id),
            ),
            turn_context("gpt-5.6-sol"),
            retimestamp(&root_first, "2026-03-02T00:00:00Z"),
            retimestamp(&root_second, "2026-03-02T00:00:00Z"),
            sibling_new,
        ]);
        let grandchild = parse_entry(&[
            session_meta_with_relation(
                "2026-03-02T00:00:00Z",
                grandchild_id,
                "/repo/codex",
                Some(child_id),
                Some(child_id),
            ),
            turn_context("gpt-5.6-sol"),
            retimestamp(&root_first, "2026-03-02T00:00:00Z"),
            retimestamp(&root_second, "2026-03-02T00:00:00Z"),
            retimestamp(&child_new, "2026-03-02T00:00:00Z"),
            grandchild_new,
        ]);

        assert_eq!(
            child
                .codex_details
                .as_ref()
                .and_then(|details| details.thread_id.as_deref()),
            Some(child_id)
        );
        let reconciled = reconcile_forked_entries(vec![root, child, sibling, grandchild]);
        assert_eq!(thread_total(&reconciled, root_id), 160);
        assert_eq!(thread_total(&reconciled, child_id), 60);
        assert_eq!(thread_total(&reconciled, sibling_id), 50);
        assert_eq!(thread_total(&reconciled, grandchild_id), 30);
        assert_eq!(
            reconciled
                .iter()
                .flat_map(|entry| entry.daily_rows.iter())
                .map(|row| row.usage.total)
                .sum::<u64>(),
            300
        );

        let cached_reconciled = reconcile_forked_entries(reconciled);
        assert_eq!(thread_total(&cached_reconciled, root_id), 160);
        assert_eq!(thread_total(&cached_reconciled, child_id), 60);
        assert_eq!(thread_total(&cached_reconciled, sibling_id), 50);
        assert_eq!(thread_total(&cached_reconciled, grandchild_id), 30);
    }

    #[test]
    fn keeps_v1_subagent_and_unresolved_fork_usage() {
        let root_id = "11111111-1111-7111-8111-111111111111";
        let v1_id = "22222222-2222-7222-8222-222222222222";
        let missing_parent_id = "99999999-9999-7999-8999-999999999999";
        let v1 = parse_entry(&[
            session_meta_with_relation(
                "2026-03-01T00:00:00Z",
                v1_id,
                "/repo/codex",
                Some(root_id),
                None,
            ),
            turn_context("gpt-5.6-sol"),
            token_count_event("2026-03-01T00:00:01Z", 30, 20, 10, 0, 40, 30, 20, 10, 0, 40),
        ]);
        let unresolved = parse_entry(&[
            session_meta_with_relation(
                "2026-03-01T00:00:00Z",
                missing_parent_id,
                "/repo/codex",
                Some("missing"),
                Some("missing"),
            ),
            turn_context("gpt-5.6-sol"),
            token_count_event("2026-03-01T00:00:01Z", 50, 30, 20, 0, 70, 50, 30, 20, 0, 70),
        ]);

        let reconciled = reconcile_forked_entries(vec![v1, unresolved]);
        assert_eq!(thread_total(&reconciled, v1_id), 40);
        assert_eq!(thread_total(&reconciled, missing_parent_id), 70);
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

        let rows = parse_file_detailed(&path, &AggregationTz::parse(Some("UTC")).unwrap())
            .unwrap()
            .daily_rows;
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

        let rows = parse_file_detailed(&path, &AggregationTz::parse(Some("UTC")).unwrap())
            .unwrap()
            .daily_rows;
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

        let rows = parse_file_detailed(&path, &AggregationTz::parse(Some("UTC")).unwrap())
            .unwrap()
            .daily_rows;
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

        let rows = parse_file_detailed(&path, &AggregationTz::parse(Some("UTC+8")).unwrap())
            .unwrap()
            .daily_rows;
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

    fn parse_entry(lines: &[Value]) -> FileCacheEntry {
        let path = write_temp_jsonl(lines);
        let parsed = parse_file_detailed(&path, &AggregationTz::parse(Some("UTC")).unwrap())
            .expect("Codex fixture should parse");
        let metadata = fs::metadata(&path).unwrap();
        let entry = parsed_entry(path.clone(), &metadata, parsed);
        let _ = fs::remove_file(path);
        entry
    }

    fn parsed_entry(
        path: PathBuf,
        metadata: &fs::Metadata,
        parsed: ParsedCodexFile,
    ) -> FileCacheEntry {
        FileCacheEntry {
            source: SourceKind::Codex,
            parser_version: parser_version(SourceKind::Codex),
            path,
            size: metadata.len(),
            mtime_ms: 0,
            daily_rows: parsed.daily_rows,
            claude_message_rows: Vec::new(),
            codex_details: Some(parsed.details),
            copilot_details: None,
        }
    }

    fn thread_total(entries: &[FileCacheEntry], thread_id: &str) -> u64 {
        entries
            .iter()
            .find(|entry| {
                entry
                    .codex_details
                    .as_ref()
                    .and_then(|details| details.thread_id.as_deref())
                    == Some(thread_id)
            })
            .into_iter()
            .flat_map(|entry| entry.daily_rows.iter())
            .map(|row| row.usage.total)
            .sum()
    }

    fn retimestamp(value: &Value, timestamp: &str) -> Value {
        let mut value = value.clone();
        value["timestamp"] = Value::String(timestamp.to_string());
        value
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

    fn session_meta_with_relation(
        timestamp: &str,
        id: &str,
        cwd: &str,
        parent_thread_id: Option<&str>,
        forked_from_id: Option<&str>,
    ) -> Value {
        json!({
            "timestamp": timestamp,
            "type": "session_meta",
            "payload": {
                "id": id,
                "cwd": cwd,
                "parent_thread_id": parent_thread_id,
                "forked_from_id": forked_from_id
            }
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
