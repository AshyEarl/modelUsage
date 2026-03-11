use crate::model::{FileCacheEntry, PricingCache, SourceKind, StatsCache, UpdateState};
use anyhow::{Context, Result};
use dirs::cache_dir;
use serde::de::DeserializeOwned;
use std::fs;
use std::path::{Path, PathBuf};

const CACHE_DIR_NAME: &str = "modelUsage";
const STATS_FILE_NAME: &str = "stats.json";
const PRICING_FILE_NAME: &str = "pricing.json";
const UPDATE_FILE_NAME: &str = "update.json";
const CLAUDE_PARSER_VERSION: u32 = 2;
const CODEX_PARSER_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatsCacheLoadState {
    Hit {
        cached_files: usize,
    },
    MissingFile,
    VersionMismatch {
        found_version: u32,
        expected_version: u32,
        previous_tz_key: String,
        previous_files: usize,
    },
    TimezoneMismatch {
        previous_tz_key: String,
        expected_tz_key: String,
        previous_files: usize,
    },
}

#[derive(Debug, Clone)]
pub struct StatsCacheLoadResult {
    pub cache: StatsCache,
    pub state: StatsCacheLoadState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeReason {
    MissingEntry,
    SizeChanged,
    MtimeChanged,
    ParserVersionChanged,
}

pub fn cache_dir_path() -> Result<PathBuf> {
    // Store cache files under the system cache directory to avoid polluting the repo or home root.
    // 统一放到系统 cache 目录，避免污染项目目录和 home 根目录。
    let dir = cache_dir()
        .context("failed to resolve cache directory")?
        .join(CACHE_DIR_NAME);
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create cache dir {}", dir.display()))?;
    Ok(dir)
}

pub fn stats_cache_path() -> Result<PathBuf> {
    Ok(cache_dir_path()?.join(STATS_FILE_NAME))
}

pub fn pricing_cache_path() -> Result<PathBuf> {
    Ok(cache_dir_path()?.join(PRICING_FILE_NAME))
}

pub fn update_state_path() -> Result<PathBuf> {
    Ok(cache_dir_path()?.join(UPDATE_FILE_NAME))
}

pub fn load_stats_cache_with_state(expected_tz_key: &str) -> Result<StatsCacheLoadResult> {
    let cache = load_json(stats_cache_path()?.as_path())?;
    let (cache, state) = normalize_stats_cache(cache, expected_tz_key);
    Ok(StatsCacheLoadResult { cache, state })
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

pub fn load_update_state() -> Result<UpdateState> {
    load_json(update_state_path()?.as_path()).map(|opt| opt.unwrap_or_default())
}

pub fn save_update_state(state: &UpdateState) -> Result<()> {
    save_json(update_state_path()?.as_path(), state)
}

pub fn file_change_reason(
    source: SourceKind,
    entry: Option<&FileCacheEntry>,
    metadata: &fs::Metadata,
) -> Option<FileChangeReason> {
    // This version uses file-level incremental parsing: changed files are fully reparsed, unchanged files reuse cache.
    // 第一版只做文件级增量。文件一旦变了，就整文件重算；没变则直接复用缓存。
    let Some(entry) = entry else {
        return Some(FileChangeReason::MissingEntry);
    };
    let size = metadata.len();
    let mtime_ms = file_mtime_ms(metadata).unwrap_or_default();
    // Invalidate cache when parser semantics change for this source, even if the file itself is untouched.
    // 即使文件本身没变，只要该 source 的解析语义版本变化，也必须重算这个文件。
    if entry.size != size {
        return Some(FileChangeReason::SizeChanged);
    }
    if entry.mtime_ms != mtime_ms {
        return Some(FileChangeReason::MtimeChanged);
    }
    if entry.parser_version != parser_version(source) {
        return Some(FileChangeReason::ParserVersionChanged);
    }
    None
}

pub fn build_file_entry(
    source: SourceKind,
    path: &Path,
    metadata: &fs::Metadata,
    daily_rows: Vec<crate::model::FileDailyRow>,
) -> FileCacheEntry {
    FileCacheEntry {
        source,
        parser_version: parser_version(source),
        path: path.to_path_buf(),
        size: metadata.len(),
        mtime_ms: file_mtime_ms(metadata).unwrap_or_default(),
        daily_rows,
    }
}

pub fn parser_version(source: SourceKind) -> u32 {
    match source {
        SourceKind::Claude => CLAUDE_PARSER_VERSION,
        SourceKind::Codex => CODEX_PARSER_VERSION,
    }
}

fn load_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
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

fn normalize_stats_cache(
    cache: Option<StatsCache>,
    expected_tz_key: &str,
) -> (StatsCache, StatsCacheLoadState) {
    let Some(cache) = cache else {
        return (
            StatsCache::empty_for_tz(expected_tz_key.to_string()),
            StatsCacheLoadState::MissingFile,
        );
    };

    if cache.version != crate::model::STATS_CACHE_VERSION {
        // Drop stale cache when parser semantics change.
        // 解析语义版本变更后，直接丢弃旧缓存，避免复用过期统计结果。
        return (
            StatsCache::empty_for_tz(expected_tz_key.to_string()),
            StatsCacheLoadState::VersionMismatch {
                found_version: cache.version,
                expected_version: crate::model::STATS_CACHE_VERSION,
                previous_tz_key: cache.aggregation_tz_key,
                previous_files: cache.files.len(),
            },
        );
    }
    if cache.aggregation_tz_key != expected_tz_key {
        // Drop stale cache when aggregation timezone changes because date bucketing changes.
        // 聚合时区改变会改变分桶日期，需要丢弃旧缓存重建。
        return (
            StatsCache::empty_for_tz(expected_tz_key.to_string()),
            StatsCacheLoadState::TimezoneMismatch {
                previous_tz_key: cache.aggregation_tz_key,
                expected_tz_key: expected_tz_key.to_string(),
                previous_files: cache.files.len(),
            },
        );
    }

    let cached_files = cache.files.len();
    (cache, StatsCacheLoadState::Hit { cached_files })
}

#[cfg(test)]
mod tests {
    use super::{
        file_change_reason, normalize_stats_cache, parser_version, FileChangeReason,
        StatsCacheLoadState,
    };
    use crate::model::{SourceKind, StatsCache};
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn drops_old_stats_cache_version() {
        let mut files = BTreeMap::new();
        files.insert(
            "old.jsonl".to_string(),
            crate::model::FileCacheEntry {
                source: SourceKind::Codex,
                parser_version: parser_version(SourceKind::Codex),
                path: PathBuf::from("/tmp/old.jsonl"),
                size: 1,
                mtime_ms: 1,
                daily_rows: vec![],
            },
        );
        let cache = StatsCache {
            version: 1,
            aggregation_tz_key: "local".to_string(),
            files,
        };
        let (normalized, state) = normalize_stats_cache(Some(cache), "local");
        assert_eq!(normalized.version, crate::model::STATS_CACHE_VERSION);
        assert!(normalized.files.is_empty());
        assert_eq!(
            state,
            StatsCacheLoadState::VersionMismatch {
                found_version: 1,
                expected_version: crate::model::STATS_CACHE_VERSION,
                previous_tz_key: "local".to_string(),
                previous_files: 1,
            }
        );
    }

    #[test]
    fn keeps_current_stats_cache_version() {
        let mut files = BTreeMap::new();
        files.insert(
            "current.jsonl".to_string(),
            crate::model::FileCacheEntry {
                source: SourceKind::Claude,
                parser_version: parser_version(SourceKind::Claude),
                path: PathBuf::from("/tmp/current.jsonl"),
                size: 1,
                mtime_ms: 1,
                daily_rows: vec![],
            },
        );
        let cache = StatsCache {
            version: crate::model::STATS_CACHE_VERSION,
            aggregation_tz_key: "offset:+08:00".to_string(),
            files: files.clone(),
        };
        let (normalized, state) = normalize_stats_cache(Some(cache), "offset:+08:00");
        assert_eq!(normalized.version, crate::model::STATS_CACHE_VERSION);
        assert_eq!(normalized.files.len(), files.len());
        assert_eq!(state, StatsCacheLoadState::Hit { cached_files: 1 });
    }

    #[test]
    fn drops_cache_when_timezone_key_changes() {
        let cache = StatsCache {
            version: crate::model::STATS_CACHE_VERSION,
            aggregation_tz_key: "local".to_string(),
            files: BTreeMap::new(),
        };
        let (normalized, state) = normalize_stats_cache(Some(cache), "offset:+08:00");
        assert_eq!(normalized.aggregation_tz_key, "offset:+08:00");
        assert!(normalized.files.is_empty());
        assert_eq!(
            state,
            StatsCacheLoadState::TimezoneMismatch {
                previous_tz_key: "local".to_string(),
                expected_tz_key: "offset:+08:00".to_string(),
                previous_files: 0,
            }
        );
    }

    #[test]
    fn reports_missing_stats_file_as_cache_miss() {
        let (normalized, state) = normalize_stats_cache(None, "local");
        assert_eq!(normalized.aggregation_tz_key, "local");
        assert!(normalized.files.is_empty());
        assert_eq!(state, StatsCacheLoadState::MissingFile);
    }

    #[test]
    fn parser_version_change_marks_file_as_changed() {
        let path = temp_file_path("parser-version-change");
        fs::write(&path, "data").unwrap();
        let metadata = fs::metadata(&path).unwrap();
        let entry = crate::model::FileCacheEntry {
            source: SourceKind::Claude,
            parser_version: parser_version(SourceKind::Claude).saturating_sub(1),
            path: path.clone(),
            size: metadata.len(),
            mtime_ms: super::file_mtime_ms(&metadata).unwrap(),
            daily_rows: vec![],
        };

        assert_eq!(
            file_change_reason(SourceKind::Claude, Some(&entry), &metadata),
            Some(FileChangeReason::ParserVersionChanged)
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn unchanged_file_with_current_parser_version_stays_cached() {
        let path = temp_file_path("parser-version-same");
        fs::write(&path, "data").unwrap();
        let metadata = fs::metadata(&path).unwrap();
        let entry = crate::model::FileCacheEntry {
            source: SourceKind::Codex,
            parser_version: parser_version(SourceKind::Codex),
            path: path.clone(),
            size: metadata.len(),
            mtime_ms: super::file_mtime_ms(&metadata).unwrap(),
            daily_rows: vec![],
        };

        assert_eq!(
            file_change_reason(SourceKind::Codex, Some(&entry), &metadata),
            None
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn file_change_reason_reports_missing_entry() {
        let path = temp_file_path("missing-entry");
        fs::write(&path, "data").unwrap();
        let metadata = fs::metadata(&path).unwrap();

        assert_eq!(
            file_change_reason(SourceKind::Codex, None, &metadata),
            Some(FileChangeReason::MissingEntry)
        );
        let _ = fs::remove_file(path);
    }

    fn temp_file_path(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("modelusage-{prefix}-{nanos}.tmp"))
    }
}
