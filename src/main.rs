mod app;
mod cache;
mod claude;
mod cli;
mod codex;
mod model;
mod pricing;
mod report;
mod table;

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
    let cli = Cli::parse();
    let report = app::run(cli.clone())?;
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("{}", table::render_daily_report(&report));
    }
    Ok(())
}
