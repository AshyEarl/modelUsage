use crate::model::{FileCacheEntry, PricingCache, SourceKind, StatsCache};
use anyhow::{Context, Result};
use dirs::cache_dir;
use serde::de::DeserializeOwned;
use std::fs;
use std::path::{Path, PathBuf};

const CACHE_DIR_NAME: &str = "modelUsage";
const STATS_FILE_NAME: &str = "stats.json";
const PRICING_FILE_NAME: &str = "pricing.json";

pub fn cache_dir_path() -> Result<PathBuf> {
    // Store cache files under the system cache directory to avoid polluting the repo or home root.
    // 统一放到系统 cache 目录，避免污染项目目录和 home 根目录。
    let dir = cache_dir().context("failed to resolve cache directory")?.join(CACHE_DIR_NAME);
    fs::create_dir_all(&dir).with_context(|| format!("failed to create cache dir {}", dir.display()))?;
    Ok(dir)
}

pub fn stats_cache_path() -> Result<PathBuf> {
    Ok(cache_dir_path()?.join(STATS_FILE_NAME))
}

pub fn pricing_cache_path() -> Result<PathBuf> {
    Ok(cache_dir_path()?.join(PRICING_FILE_NAME))
}

pub fn load_stats_cache() -> Result<StatsCache> {
    load_json(stats_cache_path()?.as_path()).map(|opt| opt.unwrap_or_default())
}

pub fn save_stats_cache(cache: &StatsCache) -> Result<()> {
    save_json(stats_cache_path()?.as_path(), cache)
}

pub fn load_pricing_cache() -> Result<Option<PricingCache>> {
    load_json(pricing_cache_path()?.as_path())
}

pub fn save_pricing_cache(cache: &PricingCache) -> Result<()> {
    save_json(pricing_cache_path()?.as_path(), cache)
}

pub fn file_changed(entry: Option<&FileCacheEntry>, metadata: &fs::Metadata) -> bool {
    // This version uses file-level incremental parsing: changed files are fully reparsed, unchanged files reuse cache.
    // 第一版只做文件级增量。文件一旦变了，就整文件重算；没变则直接复用缓存。
    let Some(entry) = entry else {
        return true;
    };
    let size = metadata.len();
    let mtime_ms = file_mtime_ms(metadata).unwrap_or_default();
    entry.size != size || entry.mtime_ms != mtime_ms
}

pub fn build_file_entry(
    source: SourceKind,
    path: &Path,
    metadata: &fs::Metadata,
    daily_rows: Vec<crate::model::FileDailyRow>,
) -> FileCacheEntry {
    FileCacheEntry {
        source,
        path: path.to_path_buf(),
        size: metadata.len(),
        mtime_ms: file_mtime_ms(metadata).unwrap_or_default(),
        daily_rows,
    }
}

fn load_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = serde_json::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(parsed))
}

fn save_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let raw = serde_json::to_string_pretty(value)?;
    fs::write(path, raw).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn file_mtime_ms(metadata: &fs::Metadata) -> Result<u128> {
    let modified = metadata.modified()?;
    let duration = modified.duration_since(std::time::UNIX_EPOCH)?;
    Ok(duration.as_millis())
}
