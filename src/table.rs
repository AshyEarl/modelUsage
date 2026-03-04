use crate::model::{DailyReport, DailyRow};
use comfy_table::{Cell, CellAlignment, ContentArrangement, Table, presets::UTF8_FULL};

pub fn render_daily_report(report: &DailyReport) -> String {
    // Claude logs do not expose a stable reasoning field, so hide the column when the whole report is zero.
    // Claude 本地日志没有稳定的 reasoning 字段，整份报表都为 0 时就不展示这一列。
    let show_reasoning = report.totals.usage.reasoning > 0;
    let codex_like = is_codex_like_report(report);
    let show_cache_write = !codex_like;
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    let mut header = vec!["Date", "Models", "Input", "Output"];
    if show_reasoning {
        header.push("Reasoning");
    }
    if show_cache_write {
        header.push("Cache Write");
    }
    header.extend(["Cache Read", "Total Tokens", "Cost (USD)"]);
    table.set_header(header);

    for row in &report.rows {
        table.add_row(render_row(row, show_reasoning, show_cache_write, codex_like));
    }

    let total_input = display_input_tokens(&report.totals.usage, codex_like);
    let total_tokens = display_total_tokens(&report.totals.usage, codex_like);
    let mut total_row = vec![
        Cell::new("Total"),
        Cell::new(""),
        right(total_input),
        right(report.totals.usage.output),
    ];
    if show_reasoning {
        total_row.push(right(report.totals.usage.reasoning));
    }
    if show_cache_write {
        total_row.push(right(report.totals.usage.cache_write()));
    }
    total_row.extend([
        right(report.totals.usage.cache_read),
        right(total_tokens),
        Cell::new(format_cost(report.totals.cost_usd, report.totals.partial_cost)),
    ]);
    table.add_row(total_row);

    let mut output = String::new();
    output.push_str("Daily Token Usage Report\n\n");
    output.push_str(&table.to_string());
    output.push('\n');
    output.push('\n');
    output.push_str(&format!(
        "Total: {} days, {} tokens, {}",
        report.rows.len(),
        format_number(total_tokens),
        format_cost(report.totals.cost_usd, report.totals.partial_cost)
    ));
    if report.totals.partial_cost && !report.totals.unpriced_models.is_empty() {
        output.push('\n');
        output.push_str("partial cost; unpriced models: ");
        output.push_str(&report.totals.unpriced_models.iter().cloned().collect::<Vec<_>>().join(", "));
    }
    output
}

fn render_row(row: &DailyRow, show_reasoning: bool, show_cache_write: bool, codex_like: bool) -> Vec<Cell> {
    let display_input = display_input_tokens(&row.usage, codex_like);
    let display_total = display_total_tokens(&row.usage, codex_like);
    let mut cells = vec![
        Cell::new(row.date.to_string()),
        Cell::new(row.models.iter().cloned().collect::<Vec<_>>().join(", ")),
        right(display_input),
        right(row.usage.output),
    ];
    if show_reasoning {
        cells.push(right(row.usage.reasoning));
    }
    if show_cache_write {
        cells.push(right(row.usage.cache_write()));
    }
    cells.extend([
        right(row.usage.cache_read),
        right(display_total),
        Cell::new(format_cost(row.cost_usd, row.partial_cost)),
    ]);
    cells
}

fn is_codex_like_report(report: &DailyReport) -> bool {
    !report.rows.is_empty()
        && report.totals.usage.cache_write() == 0
        && report
            .rows
            .iter()
            .flat_map(|row| row.models.iter())
            .all(|model| model.starts_with("gpt-"))
}

fn display_input_tokens(usage: &crate::model::UsageTotals, codex_like: bool) -> u64 {
    if codex_like {
        usage.input.saturating_sub(usage.cache_read)
    } else {
        usage.input
    }
}

fn display_total_tokens(usage: &crate::model::UsageTotals, codex_like: bool) -> u64 {
    if codex_like {
        display_input_tokens(usage, codex_like) + usage.output + usage.cache_read
    } else {
        usage.total
    }
}

fn right(value: u64) -> Cell {
    Cell::new(format_number(value)).set_alignment(CellAlignment::Right)
}

fn format_number(value: u64) -> String {
    let chars: Vec<char> = value.to_string().chars().rev().collect();
    let mut out = String::new();
    for (idx, ch) in chars.iter().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            out.push(',');
        }
        out.push(*ch);
    }
    out.chars().rev().collect()
}

fn format_cost(cost: Option<f64>, partial: bool) -> String {
    match cost {
        Some(cost) if partial => format!("${cost:.2}*"),
        Some(cost) => format!("${cost:.2}"),
        None => "N/A".to_string(),
    }
}
