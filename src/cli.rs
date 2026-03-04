use clap::Parser;

#[derive(Debug, Clone, Parser)]
#[command(name = "modelUsage", version, about = "Summarize Claude and Codex local usage logs")]
pub struct Cli {
    /// Rebuild the stats cache from scratch by rescanning all local JSONL files.
    /// 丢弃已有统计缓存，重新全量扫描本地 JSONL。
    #[arg(long, help = "Rebuild the file cache from scratch")]
    pub refresh: bool,

    /// Show all historical dates; by default only the latest month is displayed.
    /// 显示全部历史日期；默认只显示最近一个月。
    #[arg(long, help = "Show all dates instead of only the latest month")]
    pub all: bool,

    /// Output JSON for shell pipelines, jq, or other scripts.
    /// 以 JSON 输出，方便后续接 shell / jq / 其他脚本。
    #[arg(long, help = "Output JSON instead of a table")]
    pub json: bool,

    /// Only include Claude Code local logs.
    /// 只统计 Claude Code 本地日志。
    #[arg(long, help = "Only include Claude logs")]
    pub claude: bool,

    /// Only include Codex local logs.
    /// 只统计 Codex 本地日志。
    #[arg(long, help = "Only include Codex logs")]
    pub codex: bool,
}
