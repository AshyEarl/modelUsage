mod app;
mod cache;
mod claude;
mod cli;
mod codex;
mod copilot;
mod model;
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
            table::render_daily_report(&report, report_label(&cli))
        );
    }
    if let Err(err) = update::maybe_check_for_updates(&cli) {
        eprintln!("warning: {err:#}");
    }
    Ok(())
}

fn report_label(cli: &Cli) -> &'static str {
    match (cli.claude, cli.codex, cli.copilot) {
        (true, false, false) => "Claude",
        (false, true, false) => "Codex",
        (false, false, true) => "Copilot",
        (true, true, false) => "Claude + Codex",
        (true, false, true) => "Claude + Copilot",
        (false, true, true) => "Codex + Copilot",
        _ => "Claude + Codex + Copilot",
    }
}
