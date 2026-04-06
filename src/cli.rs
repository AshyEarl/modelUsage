use crate::model::ReportGrouping;
use clap::Parser;

#[derive(Debug, Clone, Parser)]
#[command(
    name = "modelUsage",
    version,
    about = "Summarize Claude and Codex local usage logs"
)]
pub struct Cli {
    /// Download and install the latest GitHub release binary.
    /// 下载并安装最新的 GitHub Release 二进制。
    #[arg(long, help = "Download and install the latest release binary")]
    pub update: bool,

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

    /// Group report rows by date.
    /// 按日期分组输出报表行。
    #[arg(long, help = "Group rows by date")]
    pub daily: bool,

    /// Group report rows by project (cwd).
    /// 按项目（cwd）分组输出报表行。
    #[arg(long, help = "Group rows by project (cwd)")]
    pub project: bool,

    /// Aggregation timezone, e.g. Asia/Shanghai, UTC+8, +08:00, UTC.
    /// 聚合时区，例如 Asia/Shanghai、UTC+8、+08:00、UTC。
    #[arg(long, help = "Aggregation timezone (IANA or UTC offset)")]
    pub tz: Option<String>,

    /// Backward-compatible alias; use --project.
    /// 向后兼容别名；请改用 --project。
    #[arg(long, hide = true)]
    pub by_project: bool,

    /// Only include Claude Code local logs.
    /// 只统计 Claude Code 本地日志。
    #[arg(long, help = "Only include Claude logs")]
    pub claude: bool,

    /// Only include Codex local logs.
    /// 只统计 Codex 本地日志。
    #[arg(long, help = "Only include Codex logs")]
    pub codex: bool,

    /// Only include Copilot CLI local logs.
    /// 只统计 Copilot CLI 本地日志。
    #[arg(long, help = "Only include Copilot logs")]
    pub copilot: bool,

    #[arg(skip = ReportGrouping::Daily)]
    pub grouping: ReportGrouping,
}

impl Cli {
    pub fn finalize_grouping(&mut self, argv: &[String]) {
        if self.by_project {
            self.project = true;
        }
        self.grouping = resolve_grouping(self.daily, self.project, argv);
    }
}

fn resolve_grouping(daily: bool, project: bool, argv: &[String]) -> ReportGrouping {
    match (daily, project) {
        (false, false) => ReportGrouping::Daily,
        (true, false) => ReportGrouping::Daily,
        (false, true) => ReportGrouping::Project,
        (true, true) => {
            let daily_idx = first_flag_index(argv, "--daily").unwrap_or(usize::MAX);
            let project_idx = first_flag_index(argv, "--project")
                .or_else(|| first_flag_index(argv, "--by-project"))
                .unwrap_or(usize::MAX);
            if project_idx < daily_idx {
                ReportGrouping::ProjectDaily
            } else {
                ReportGrouping::DailyProject
            }
        }
    }
}

fn first_flag_index(argv: &[String], flag: &str) -> Option<usize> {
    argv.iter().position(|arg| arg == flag)
}
