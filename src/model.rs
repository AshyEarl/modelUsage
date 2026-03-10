use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// Marks whether a usage record comes from Claude or Codex for parser-specific handling.
/// 标记一条统计数据来自 Claude 还是 Codex，便于后续扩展不同解析逻辑。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Claude,
    Codex,
}

/// Unified token accounting structure shared by Claude and Codex after parsing.
/// 统一后的 token 统计结构。Claude 和 Codex 虽然原始字段不一致，但最终都会被归一到这里。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct UsageTotals {
    pub input: u64,
    pub output: u64,
    pub reasoning: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub cache_read: u64,
    pub total: u64,
}

impl UsageTotals {
    pub fn add_assign(&mut self, other: &Self) {
        self.input += other.input;
        self.output += other.output;
        self.reasoning += other.reasoning;
        self.cache_write_5m += other.cache_write_5m;
        self.cache_write_1h += other.cache_write_1h;
        self.cache_read += other.cache_read;
        self.total += other.total;
    }

    pub fn cache_write(&self) -> u64 {
        self.cache_write_5m + self.cache_write_1h
    }
}

/// Intermediate structure produced from a single parsed raw event.
/// 单条原始事件解析后的中间结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageEvent {
    pub source: SourceKind,
    pub timestamp: DateTime<Utc>,
    pub raw_model: String,
    pub normalized_model: String,
    pub usage: UsageTotals,
}

/// Per-file daily aggregation grouped by date and model.
/// 单个日志文件按天、按模型聚合后的结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDailyRow {
    pub date: NaiveDate,
    pub model: String,
    pub usage: UsageTotals,
}

/// File-level cache entry.
/// Reuse the previous daily rows when size/mtime are unchanged to avoid full rescans.
/// 文件级缓存条目。只要 size/mtime 没变，就复用这个文件上次算出的 daily_rows，避免全量重扫。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCacheEntry {
    pub source: SourceKind,
    #[serde(default)]
    pub parser_version: u32,
    pub path: PathBuf,
    pub size: u64,
    pub mtime_ms: u128,
    pub daily_rows: Vec<FileDailyRow>,
}

/// On-disk format for the global stats cache.
/// 整体统计缓存文件格式。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsCache {
    pub version: u32,
    pub files: BTreeMap<String, FileCacheEntry>,
}

pub const STATS_CACHE_VERSION: u32 = 2;

impl Default for StatsCache {
    fn default() -> Self {
        Self {
            version: STATS_CACHE_VERSION,
            files: BTreeMap::new(),
        }
    }
}

/// Per-model pricing configuration expressed in USD per million tokens.
/// 每个模型的价格配置，单位统一成每百万 token 的美元价格。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPrice {
    pub input_cost_per_mtoken: f64,
    pub output_cost_per_mtoken: f64,
    pub cache_write_5m_cost_per_mtoken: Option<f64>,
    pub cache_write_1h_cost_per_mtoken: Option<f64>,
    pub cache_read_cost_per_mtoken: Option<f64>,
}

/// On-disk format for the pricing cache.
/// 价格缓存文件格式。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingCache {
    pub version: u32,
    pub updated_at: DateTime<Utc>,
    pub models: BTreeMap<String, ModelPrice>,
}

/// One daily row in the final external report.
/// 对外输出的日报行。
#[derive(Debug, Clone, Serialize)]
pub struct DailyRow {
    pub date: NaiveDate,
    pub models: BTreeSet<String>,
    pub usage: UsageTotals,
    pub cost_usd: Option<f64>,
    pub partial_cost: bool,
    pub unpriced_models: BTreeSet<String>,
}

/// Totals row for the final report.
/// 汇总行。
#[derive(Debug, Clone, Serialize)]
pub struct ReportTotals {
    pub usage: UsageTotals,
    pub cost_usd: Option<f64>,
    pub partial_cost: bool,
    pub unpriced_models: BTreeSet<String>,
}

/// Default report data model used by both the table renderer and JSON output.
/// 默认输出的数据模型，既可渲染表格，也可直接序列化成 JSON。
#[derive(Debug, Clone, Serialize)]
pub struct DailyReport {
    pub rows: Vec<DailyRow>,
    pub totals: ReportTotals,
}

/// On-disk format for the update-check cache.
/// 更新检查缓存文件格式。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateState {
    pub version: u32,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub latest_version: Option<String>,
    pub asset_name: Option<String>,
    pub asset_url: Option<String>,
    pub release_notes_summary: Option<String>,
}
