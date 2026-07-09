mod app;
mod cache;
mod claude;
mod cli;
mod codex;
mod copilot;
mod model;
mod opencode;
mod pricing;
mod profile;
mod report;
mod table;
mod timezone;
mod update;

use anyhow::Result;
use clap::Parser;
use cli::Cli;

fn main() {
    if let Err(err) = real_main() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<()> {
    // Keep the CLI minimal: parse flags here and delegate the real work to the app layer.
    // CLI 本身保持极简，只负责解析参数并把执行委托给 app 层。
    let raw_args: Vec<String> = std::env::args().collect();
    let mut cli = Cli::parse();
    cli.finalize_grouping(&raw_args);
    if cli.update {
        return update::run_manual_update();
    }
    let report = app::run(cli.clone())?;
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "{}",
            table::render_daily_report(&report, &report_label(&cli))
        );
    }
    if let Err(err) = update::maybe_check_for_updates(&cli) {
        eprintln!("warning: {err:#}");
    }
    Ok(())
}

fn report_label(cli: &Cli) -> String {
    // Build the label from whichever source flags are active; with no flags, show all sources.
    // 根据生效的来源 flag 拼装标签；不传任何 flag 时展示全部来源。
    let mut parts: Vec<&str> = Vec::new();
    if cli.claude {
        parts.push("Claude");
    }
    if cli.codex {
        parts.push("Codex");
    }
    if cli.copilot {
        parts.push("Copilot");
    }
    if cli.opencode {
        parts.push("Opencode");
    }
    if parts.is_empty() {
        parts.extend(&["Claude", "Codex", "Copilot", "Opencode"]);
    }
    parts.join(" + ")
}
