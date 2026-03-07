use crate::cache::{build_file_entry, file_changed, load_stats_cache, save_stats_cache};
use crate::claude;
use crate::cli::Cli;
use crate::codex;
use crate::model::{DailyReport, FileCacheEntry, SourceKind, StatsCache};
use crate::pricing;
use crate::report;
use anyhow::{Context, Result};
use chrono::Days;
use dirs::home_dir;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn run(cli: Cli) -> Result<DailyReport> {
    // Keep source selection simple: default to both sources, or only the explicitly requested one.
    // 参数规则保持简单：不传时默认两边都统计，只传一个时就只扫一个来源。
    let include_claude = cli.claude || !cli.codex;
    let include_codex = cli.codex || !cli.claude;

    // The stats cache stores per-file aggregated daily rows so subsequent runs only reparse changed JSONL files.
    // stats cache 保存的是“文件 -> 已聚合日报”的中间结果，第二次运行时只重算变更过的 JSONL。
    let mut cache = if cli.refresh {
        StatsCache::default()
    } else {
        load_stats_cache()?
    };

    let mut next_files: BTreeMap<String, FileCacheEntry> = BTreeMap::new();

    if include_claude {
        let root = home_dir()
            .context("failed to resolve home directory")?
            .join(".claude/projects");
        scan_source(SourceKind::Claude, &root, &mut cache, &mut next_files)?;
    }
    if include_codex {
        let root = home_dir()
            .context("failed to resolve home directory")?
            .join(".codex/sessions");
        scan_source(SourceKind::Codex, &root, &mut cache, &mut next_files)?;
    }

    cache.files = next_files;
    save_stats_cache(&cache)?;

    let prices = pricing::load_prices()?;
    let mut daily_report = report::build_daily_report(cache.files.into_values(), &prices);
    if !cli.all {
        trim_to_latest_month(&mut daily_report);
    }
    Ok(daily_report)
}

fn scan_source(
    source: SourceKind,
    root: &Path,
    cache: &mut StatsCache,
    next_files: &mut BTreeMap<String, FileCacheEntry>,
) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }

    for path in jsonl_files(root) {
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let key = path.to_string_lossy().to_string();
        let existing = cache.files.get(&key);
        let entry = if !file_changed(existing, &metadata) {
            existing.cloned().unwrap()
        } else {
            // Reparse only changed files so we do not rescan the full history on every run.
            // 文件变了才重新解析，避免每次都把历史日志全扫一遍。
            let daily_rows = match source {
                SourceKind::Claude => claude::parse_file(&path),
                SourceKind::Codex => codex::parse_file(&path),
            }
            .with_context(|| format!("failed to parse {}", path.display()))?;
            build_file_entry(source, &path, &metadata, daily_rows)
        };
        next_files.insert(key, entry);
    }

    Ok(())
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

fn trim_to_latest_month(report: &mut DailyReport) {
    let Some(latest_date) = report.rows.last().map(|row| row.date) else {
        return;
    };
    let cutoff = latest_date
        .checked_sub_days(Days::new(29))
        .unwrap_or(latest_date);

    report.rows.retain(|row| row.date >= cutoff);

    let mut totals = crate::model::ReportTotals {
        usage: crate::model::UsageTotals::default(),
        cost_usd: Some(0.0),
        partial_cost: false,
        unpriced_models: Default::default(),
    };
    let mut any_priced = false;

    for row in &report.rows {
        totals.usage.add_assign(&row.usage);
        if let Some(cost) = row.cost_usd {
            let current = totals.cost_usd.get_or_insert(0.0);
            *current += cost;
            any_priced = true;
        }
        if row.partial_cost {
            totals.partial_cost = true;
        }
        totals
            .unpriced_models
            .extend(row.unpriced_models.iter().cloned());
    }

    if !any_priced {
        totals.cost_usd = None;
    } else if totals.partial_cost && totals.cost_usd == Some(0.0) {
        totals.cost_usd = None;
    }

    report.totals = totals;
}
