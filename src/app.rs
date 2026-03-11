use crate::cache::{
    build_file_entry, file_change_reason, load_stats_cache_with_state, save_stats_cache,
    FileChangeReason, StatsCacheLoadState,
};
use crate::claude;
use crate::cli::Cli;
use crate::codex;
use crate::model::{DailyReport, FileCacheEntry, SourceKind, StatsCache};
use crate::pricing;
use crate::profile;
use crate::report;
use crate::timezone::AggregationTz;
use anyhow::{Context, Result};
use chrono::Days;
use dirs::home_dir;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use walkdir::WalkDir;

#[derive(Debug, Default, Clone, Copy)]
struct FileInvalidationStats {
    missing_entry: u64,
    size_changed: u64,
    mtime_changed: u64,
    parser_version_changed: u64,
}

impl FileInvalidationStats {
    fn record(&mut self, reason: FileChangeReason) {
        match reason {
            FileChangeReason::MissingEntry => self.missing_entry += 1,
            FileChangeReason::SizeChanged => self.size_changed += 1,
            FileChangeReason::MtimeChanged => self.mtime_changed += 1,
            FileChangeReason::ParserVersionChanged => self.parser_version_changed += 1,
        }
    }

    fn add_assign(&mut self, other: &Self) {
        self.missing_entry += other.missing_entry;
        self.size_changed += other.size_changed;
        self.mtime_changed += other.mtime_changed;
        self.parser_version_changed += other.parser_version_changed;
    }
}

#[derive(Debug, Clone)]
struct SourceBuildStats {
    parsed_files: u64,
    invalidations: FileInvalidationStats,
    parse_dirs: Vec<(PathBuf, u128)>,
}

pub fn run(cli: Cli) -> Result<DailyReport> {
    let run_started = Instant::now();
    profile::set_suppressed(cli.json);
    // Keep source selection simple: default to both sources, or only the explicitly requested one.
    // 参数规则保持简单：不传时默认两边都统计，只传一个时就只扫一个来源。
    let include_claude = cli.claude || !cli.codex;
    let include_codex = cli.codex || !cli.claude;
    let debug_profile = profile::enabled();
    let emit_build_stats = !cli.json;

    let aggregation_tz = AggregationTz::parse(cli.tz.as_deref())?;
    let aggregation_tz_key = aggregation_tz.cache_key();
    if debug_profile {
        profile::log(format!(
            "run start refresh={} all={} grouping={:?} tz={} include_claude={} include_codex={}",
            cli.refresh, cli.all, cli.grouping, aggregation_tz_key, include_claude, include_codex
        ));
    }

    // The stats cache stores per-file aggregated daily rows so subsequent runs only reparse changed JSONL files.
    // stats cache 保存的是“文件 -> 已聚合日报”的中间结果，第二次运行时只重算变更过的 JSONL。
    let cache_load_started = Instant::now();
    let (mut cache, cache_state_label, cache_state_brief) = if cli.refresh {
        (
            StatsCache::empty_for_tz(aggregation_tz_key.clone()),
            "refresh".to_string(),
            "refresh".to_string(),
        )
    } else {
        let result = load_stats_cache_with_state(&aggregation_tz_key)?;
        let label = describe_cache_state(&result.state);
        let brief = describe_cache_state_brief(&result.state);
        (result.cache, label, brief)
    };
    let cache_load_ms = cache_load_started.elapsed().as_millis();
    cache.aggregation_tz_key = aggregation_tz_key.clone();
    if debug_profile {
        profile::log(format!(
            "cache state={} load_ms={}",
            cache_state_label, cache_load_ms
        ));
    }

    let mut next_files: BTreeMap<String, FileCacheEntry> = BTreeMap::new();
    let mut source_stats = Vec::new();

    if include_claude {
        let root = home_dir()
            .context("failed to resolve home directory")?
            .join(".claude/projects");
        let stats = scan_source(
            SourceKind::Claude,
            &root,
            &aggregation_tz,
            &mut cache,
            &mut next_files,
            debug_profile,
        )?;
        source_stats.push(stats);
    }
    if include_codex {
        let codex_home = home_dir()
            .context("failed to resolve home directory")?
            .join(".codex");
        for root in codex_source_roots(&codex_home) {
            let stats = scan_source(
                SourceKind::Codex,
                &root,
                &aggregation_tz,
                &mut cache,
                &mut next_files,
                debug_profile,
            )?;
            source_stats.push(stats);
        }
    }

    let save_cache_started = Instant::now();
    cache.files = next_files;
    save_stats_cache(&cache)?;
    let save_cache_ms = save_cache_started.elapsed().as_millis();

    let report_started = Instant::now();
    let prices = pricing::load_prices()?;
    let mut entries: Vec<FileCacheEntry> = cache.files.into_values().collect();
    if !cli.all {
        trim_entries_to_latest_month(&mut entries);
    }
    let has_claude_data = entries
        .iter()
        .any(|entry| entry.source == SourceKind::Claude && !entry.daily_rows.is_empty());
    let mut daily_report = report::build_report(entries.into_iter(), &prices, cli.grouping);
    if has_claude_data {
        daily_report.warnings.push(
            "Claude input/output tokens may be undercounted by upstream local logs; treat Claude input/output/cost as estimates."
                .to_string(),
        );
    }
    if emit_build_stats {
        log_build_summary(
            &cache_state_brief,
            cache_load_ms,
            save_cache_ms,
            report_started.elapsed().as_millis(),
            run_started.elapsed().as_millis(),
            &source_stats,
        );
    }
    Ok(daily_report)
}

fn codex_source_roots(codex_home: &Path) -> [PathBuf; 2] {
    [
        codex_home.join("sessions"),
        codex_home.join("archived_sessions"),
    ]
}

fn scan_source(
    source: SourceKind,
    root: &Path,
    aggregation_tz: &AggregationTz,
    cache: &mut StatsCache,
    next_files: &mut BTreeMap<String, FileCacheEntry>,
    debug_profile: bool,
) -> Result<SourceBuildStats> {
    if !root.exists() {
        if debug_profile {
            profile::log(format!(
                "skip source={} root={} (not found)",
                source_name(source),
                root.display()
            ));
        }
        return Ok(SourceBuildStats {
            parsed_files: 0,
            invalidations: FileInvalidationStats::default(),
            parse_dirs: Vec::new(),
        });
    }

    let scan_started = Instant::now();
    let files = jsonl_files(root);
    if debug_profile {
        profile::log(format!(
            "scan source={} root={} files={}",
            source_name(source),
            root.display(),
            files.len()
        ));
    }
    let mut parsed_files = 0u64;
    let mut reused_files = 0u64;
    let mut parsed_rows = 0u64;
    let mut reused_rows = 0u64;
    let mut parsed_ms = 0u128;
    let mut invalidations = FileInvalidationStats::default();
    let mut parse_dir_ms: BTreeMap<PathBuf, u128> = BTreeMap::new();

    for path in files {
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let key = path.to_string_lossy().to_string();
        let existing = cache.files.get(&key);
        let change_reason = file_change_reason(source, existing, &metadata);
        let entry = if let Some(reason) = change_reason {
            invalidations.record(reason);
            // Reparse only changed files so we do not rescan the full history on every run.
            // 文件变了才重新解析，避免每次都把历史日志全扫一遍。
            let parse_started = Instant::now();
            let daily_rows = match source {
                SourceKind::Claude => claude::parse_file(&path, aggregation_tz),
                SourceKind::Codex => codex::parse_file(&path, aggregation_tz),
            }
            .with_context(|| format!("failed to parse {}", path.display()))?;
            let elapsed = parse_started.elapsed();
            parsed_ms += elapsed.as_millis();
            parsed_rows += daily_rows.len() as u64;
            parsed_files += 1;
            // Track parse hotspot by directory to quickly identify expensive trees.
            // 按目录累计解析耗时，快速定位最慢目录。
            let parse_dir = path.parent().unwrap_or(root).to_path_buf();
            *parse_dir_ms.entry(parse_dir).or_default() += elapsed.as_millis();
            if debug_profile {
                profile::log(format!(
                    "parsed source={} reason={} file={} size={} rows={} elapsed_ms={}",
                    source_name(source),
                    file_change_reason_name(reason),
                    path.display(),
                    metadata.len(),
                    daily_rows.len(),
                    elapsed.as_millis()
                ));
            }
            build_file_entry(source, &path, &metadata, daily_rows)
        } else {
            reused_files += 1;
            if let Some(existing_entry) = existing {
                reused_rows += existing_entry.daily_rows.len() as u64;
            }
            existing.cloned().unwrap()
        };
        next_files.insert(key, entry);
    }
    let total_ms = scan_started.elapsed().as_millis();

    if debug_profile {
        profile::log(format!(
            "scan done source={} root={} parsed_files={} reused_files={} parsed_rows={} reused_rows={} parse_ms={} total_ms={} invalidations={{missing:{} size:{} mtime:{} parser:{}}}",
            source_name(source),
            root.display(),
            parsed_files,
            reused_files,
            parsed_rows,
            reused_rows,
            parsed_ms,
            total_ms,
            invalidations.missing_entry,
            invalidations.size_changed,
            invalidations.mtime_changed,
            invalidations.parser_version_changed
        ));
    }

    let parse_dirs = parse_dir_ms.into_iter().collect();
    Ok(SourceBuildStats {
        parsed_files,
        invalidations,
        parse_dirs,
    })
}

fn source_name(source: SourceKind) -> &'static str {
    match source {
        SourceKind::Claude => "claude",
        SourceKind::Codex => "codex",
    }
}

fn file_change_reason_name(reason: FileChangeReason) -> &'static str {
    match reason {
        FileChangeReason::MissingEntry => "missing_entry",
        FileChangeReason::SizeChanged => "size_changed",
        FileChangeReason::MtimeChanged => "mtime_changed",
        FileChangeReason::ParserVersionChanged => "parser_version_changed",
    }
}

fn describe_cache_state(state: &StatsCacheLoadState) -> String {
    match state {
        StatsCacheLoadState::Hit { cached_files } => {
            format!("hit(cached_files={cached_files})")
        }
        StatsCacheLoadState::MissingFile => "miss(no_cache_file)".to_string(),
        StatsCacheLoadState::VersionMismatch {
            found_version,
            expected_version,
            previous_tz_key,
            previous_files,
        } => format!(
            "invalidated(version_mismatch found={found_version} expected={expected_version} prev_tz={previous_tz_key} prev_files={previous_files})"
        ),
        StatsCacheLoadState::TimezoneMismatch {
            previous_tz_key,
            expected_tz_key,
            previous_files,
        } => format!(
            "invalidated(tz_mismatch prev_tz={previous_tz_key} expected_tz={expected_tz_key} prev_files={previous_files})"
        ),
    }
}

fn describe_cache_state_brief(state: &StatsCacheLoadState) -> String {
    match state {
        StatsCacheLoadState::Hit { .. } => "cache_hit".to_string(),
        StatsCacheLoadState::MissingFile => "cache_missing".to_string(),
        StatsCacheLoadState::VersionMismatch { .. } => "cache_version_mismatch".to_string(),
        StatsCacheLoadState::TimezoneMismatch { .. } => "cache_timezone_mismatch".to_string(),
    }
}

fn log_build_summary(
    cache_state_label: &str,
    cache_load_ms: u128,
    _save_cache_ms: u128,
    report_ms: u128,
    run_ms: u128,
    source_stats: &[SourceBuildStats],
) {
    let mut parsed_files = 0u64;
    let mut invalidations = FileInvalidationStats::default();
    let mut parse_dirs: BTreeMap<PathBuf, u128> = BTreeMap::new();
    for stats in source_stats {
        parsed_files += stats.parsed_files;
        invalidations.add_assign(&stats.invalidations);
        for (dir, ms) in &stats.parse_dirs {
            *parse_dirs.entry(dir.clone()).or_default() += *ms;
        }
    }
    if parsed_files == 0 {
        profile::build_log(format!(
            "Cache valid[cache_hit], no rebuild, use {}ms (cache_load={}ms, report={}ms).",
            run_ms, cache_load_ms, report_ms
        ));
        return;
    }

    let rebuild_reason = if cache_state_label != "cache_hit" {
        cache_state_label.to_string()
    } else {
        dominant_invalidation_reason(&invalidations)
            .map(|(name, _)| name.to_string())
            .unwrap_or_else(|| "file_changed".to_string())
    };

    profile::build_log(format!(
        "Cache invalid[{}], rebuilding...",
        rebuild_reason
    ));
    profile::build_log(format!(
        "Cache rebuild finish, use {}ms, top 3 hotspot dirs:",
        run_ms
    ));

    let mut hotspots: Vec<(PathBuf, u128)> = parse_dirs.into_iter().collect();
    hotspots.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    for idx in 0..3 {
        if let Some((dir, ms)) = hotspots.get(idx) {
            profile::build_log(format!("{}ms - {}", ms, dir.display()));
        } else {
            profile::build_log("0ms - N/A");
        }
    }
}

fn dominant_invalidation_reason(stats: &FileInvalidationStats) -> Option<(&'static str, u64)> {
    [
        ("missing_entry", stats.missing_entry),
        ("size_changed", stats.size_changed),
        ("mtime_changed", stats.mtime_changed),
        ("parser_version_changed", stats.parser_version_changed),
    ]
    .into_iter()
    .filter(|(_, count)| *count > 0)
    .max_by_key(|(_, count)| *count)
}

fn jsonl_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            files.push(entry.path().to_path_buf());
        }
    }
    files.sort();
    files
}

fn trim_entries_to_latest_month(entries: &mut [FileCacheEntry]) {
    let latest_date = entries
        .iter()
        .flat_map(|entry| entry.daily_rows.iter().map(|row| row.date))
        .max();
    let Some(latest_date) = latest_date else {
        return;
    };
    let cutoff = latest_date
        .checked_sub_days(Days::new(29))
        .unwrap_or(latest_date);

    for entry in entries {
        entry.daily_rows.retain(|row| row.date >= cutoff);
    }
}

#[cfg(test)]
mod tests {
    use super::codex_source_roots;
    use std::path::Path;

    #[test]
    fn codex_roots_include_archived_sessions() {
        let roots = codex_source_roots(Path::new("/tmp/.codex"));
        assert_eq!(roots[0], Path::new("/tmp/.codex/sessions"));
        assert_eq!(roots[1], Path::new("/tmp/.codex/archived_sessions"));
    }
}
