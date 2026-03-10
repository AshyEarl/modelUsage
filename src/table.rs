use crate::model::{DailyReport, DailyRow, ReportGrouping};
use comfy_table::{Cell, CellAlignment, ContentArrangement, Table, presets::UTF8_FULL};

pub fn render_daily_report(report: &DailyReport, report_label: &str) -> String {
    // Claude logs do not expose a stable reasoning field, so hide the column when the whole report is zero.
    // Claude 本地日志没有稳定的 reasoning 字段，整份报表都为 0 时就不展示这一列。
    let show_reasoning = report.totals.usage.reasoning > 0;
    let codex_like = is_codex_like_report(report);
    let show_cache_write = !codex_like;
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    let mut header = header_for_grouping(report.grouping);
    header.extend(["Models", "Input", "Output"]);
    if show_reasoning {
        header.push("Reasoning");
    }
    if show_cache_write {
        header.push("Cache Write");
    }
    header.extend(["Cache Read", "Total Tokens", "Cost (USD)"]);
    table.set_header(header);

    let project_daily_project_cells = if report.grouping == ReportGrouping::ProjectDaily {
        Some(project_daily_middle_project_cells(&report.rows))
    } else {
        None
    };
    let mut previous_date: Option<chrono::NaiveDate> = None;
    for (idx, row) in report.rows.iter().enumerate() {
        let (date_cell, project_cell) = display_group_cells(
            row,
            idx,
            report.grouping,
            &mut previous_date,
            project_daily_project_cells.as_deref(),
        );
        table.add_row(render_row(
            row,
            report.grouping,
            date_cell,
            project_cell,
            show_reasoning,
            show_cache_write,
            codex_like,
        ));
    }

    let total_input = display_input_tokens(&report.totals.usage, codex_like);
    let total_tokens = display_total_tokens(&report.totals.usage, codex_like);
    let mut total_row = total_row_prefix(report.grouping);
    total_row.extend([
        Cell::new(""),
        right(total_input),
        right(report.totals.usage.output),
    ]);
    if show_reasoning {
        total_row.push(right(report.totals.usage.reasoning));
    }
    if show_cache_write {
        total_row.push(right(report.totals.usage.cache_write()));
    }
    total_row.extend([
        right(report.totals.usage.cache_read),
        right(total_tokens),
        Cell::new(format_cost(
            report.totals.cost_usd,
            report.totals.partial_cost,
        )),
    ]);
    table.add_row(total_row);

    let mut output = String::new();
    output.push_str(&format!("modelUsage v{}\n", env!("CARGO_PKG_VERSION")));
    output.push_str(&format!(
        "{} ({report_label})\n\n",
        title_for_grouping(report.grouping)
    ));
    let rendered_table = if report.grouping == ReportGrouping::ProjectDaily {
        suppress_project_column_inner_separators(table.to_string(), &report.rows)
    } else {
        table.to_string()
    };
    output.push_str(&rendered_table);
    output.push('\n');
    output.push('\n');
    let unit = if report.grouping == ReportGrouping::Daily {
        "days"
    } else {
        "rows"
    };
    output.push_str(&format!(
        "Total: {} {unit}, {} tokens, {}",
        report.rows.len(),
        format_number(total_tokens),
        format_cost(report.totals.cost_usd, report.totals.partial_cost)
    ));
    if report.totals.partial_cost && !report.totals.unpriced_models.is_empty() {
        output.push('\n');
        output.push_str("partial cost; unpriced models: ");
        output.push_str(
            &report
                .totals
                .unpriced_models
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    output
}

fn render_row(
    row: &DailyRow,
    grouping: ReportGrouping,
    date_cell: Option<String>,
    project_cell: Option<String>,
    show_reasoning: bool,
    show_cache_write: bool,
    codex_like: bool,
) -> Vec<Cell> {
    let display_input = display_input_tokens(&row.usage, codex_like);
    let display_total = display_total_tokens(&row.usage, codex_like);
    let mut cells = Vec::new();
    match grouping {
        ReportGrouping::Daily => cells.push(Cell::new(date_cell.unwrap_or_default())),
        ReportGrouping::Project => cells.push(Cell::new(project_cell.unwrap_or_default())),
        ReportGrouping::DailyProject => {
            cells.push(Cell::new(date_cell.unwrap_or_default()));
            cells.push(Cell::new(project_cell.unwrap_or_default()));
        }
        ReportGrouping::ProjectDaily => {
            cells.push(Cell::new(project_cell.unwrap_or_default()));
            cells.push(Cell::new(date_cell.unwrap_or_default()));
        }
    }
    cells.extend([
        Cell::new(row.models.iter().cloned().collect::<Vec<_>>().join(", ")),
        right(display_input),
        right(row.usage.output),
    ]);
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

fn header_for_grouping(grouping: ReportGrouping) -> Vec<&'static str> {
    match grouping {
        ReportGrouping::Daily => vec!["Date"],
        ReportGrouping::Project => vec!["Project"],
        ReportGrouping::DailyProject => vec!["Date", "Project"],
        ReportGrouping::ProjectDaily => vec!["Project", "Date"],
    }
}

fn title_for_grouping(grouping: ReportGrouping) -> &'static str {
    match grouping {
        ReportGrouping::Daily => "Daily Token Usage Report",
        ReportGrouping::Project => "Project Token Usage Report",
        ReportGrouping::DailyProject => "Daily Token Usage Report (Date -> Project)",
        ReportGrouping::ProjectDaily => "Daily Token Usage Report (Project -> Date)",
    }
}

fn total_row_prefix(grouping: ReportGrouping) -> Vec<Cell> {
    match grouping {
        ReportGrouping::Daily | ReportGrouping::Project => vec![Cell::new("Total")],
        ReportGrouping::DailyProject | ReportGrouping::ProjectDaily => {
            vec![Cell::new("Total"), Cell::new("")]
        }
    }
}

fn display_group_cells(
    row: &DailyRow,
    idx: usize,
    grouping: ReportGrouping,
    previous_date: &mut Option<chrono::NaiveDate>,
    project_daily_project_cells: Option<&[String]>,
) -> (Option<String>, Option<String>) {
    let date_value = row.date.map(|d| d.to_string()).unwrap_or_default();
    let project_value = row
        .project
        .clone()
        .unwrap_or_else(|| "<unknown-project>".to_string());
    match grouping {
        ReportGrouping::Daily => (Some(date_value), None),
        ReportGrouping::Project => (None, Some(project_value)),
        ReportGrouping::DailyProject => {
            let show_date = if *previous_date == row.date {
                "".to_string()
            } else {
                date_value
            };
            *previous_date = row.date;
            (Some(show_date), Some(project_value))
        }
        ReportGrouping::ProjectDaily => (
            Some(date_value),
            Some(
                project_daily_project_cells
                    .and_then(|v| v.get(idx))
                    .cloned()
                    .unwrap_or(project_value),
            ),
        ),
    }
}

fn project_daily_middle_project_cells(rows: &[DailyRow]) -> Vec<String> {
    let mut cells = vec![String::new(); rows.len()];
    let mut start = 0usize;
    while start < rows.len() {
        let project = rows[start]
            .project
            .clone()
            .unwrap_or_else(|| "<unknown-project>".to_string());
        let mut end = start + 1;
        while end < rows.len() && rows[end].project == rows[start].project {
            end += 1;
        }
        let middle = start + (end - start) / 2;
        cells[middle] = project;
        start = end;
    }
    cells
}

fn suppress_project_column_inner_separators(table: String, rows: &[DailyRow]) -> String {
    if rows.len() < 2 {
        return table;
    }
    let boundaries: Vec<usize> = rows
        .windows(2)
        .enumerate()
        .filter_map(|(idx, pair)| (pair[0].project == pair[1].project).then_some(idx))
        .collect();
    if boundaries.is_empty() {
        return table;
    }

    let mut lines: Vec<String> = table.lines().map(ToOwned::to_owned).collect();
    for boundary in boundaries {
        // Table layout under UTF8_FULL:
        // 0 top border, 1 header, 2 header divider, then each row takes 2 lines: row + divider.
        // UTF8_FULL 布局固定：前三行是边框与表头，之后每条数据占两行（内容行 + 分隔线）。
        let separator_line_index = 4 + boundary * 2;
        if let Some(line) = lines.get_mut(separator_line_index) {
            *line = suppress_first_column_segment(line);
        }
    }
    lines.join("\n")
}

fn suppress_first_column_segment(line: &str) -> String {
    let mut chars: Vec<char> = line.chars().collect();
    if chars.first().copied() != Some('├') {
        return line.to_string();
    }
    let Some(first_joint) = chars.iter().position(|&ch| ch == '┼') else {
        return line.to_string();
    };
    chars[0] = '│';
    for ch in chars.iter_mut().take(first_joint).skip(1) {
        *ch = ' ';
    }
    chars[first_joint] = '├';
    chars.into_iter().collect()
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
