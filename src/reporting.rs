use prettytable::{format, Cell, Row, Table};
use std::collections::HashMap;

pub fn print_report(
    component_stats: &HashMap<String, (usize, usize)>,
    grand_total: usize,
    threshold: usize,
) -> bool {
    let mut table = Table::new();
    let mut format = format::FormatBuilder::new()
        .column_separator('│')
        .borders('│')
        .separators(
            &[format::LinePosition::Top],
            format::LineSeparator::new('─', '┬', '┌', '┐'),
        )
        .separators(
            &[format::LinePosition::Intern],
            format::LineSeparator::new('─', '┼', '├', '┤'),
        )
        .separators(
            &[format::LinePosition::Bottom],
            format::LineSeparator::new('─', '┴', '└', '┘'),
        )
        .padding(1, 1)
        .build();
    table.set_format(format);

    table.add_row(Row::new(vec![
        Cell::new("Component"),
        Cell::new("Percent"),
        Cell::new("Statements"),
        Cell::new("Files"),
    ]));

    let mut sorted: Vec<_> = component_stats.iter().collect();
    sorted.sort_unstable_by_key(|(_, &(_f, st))| std::cmp::Reverse(st));

    let mut any_over_threshold = false;
    for (component, &(files, stmts)) in &sorted {
        let percent = ((stmts as f64 / grand_total as f64) * 100.0).round() as usize;
        if percent > threshold {
            any_over_threshold = true;
        }
        table.add_row(Row::new(vec![
            Cell::new(component),
            Cell::new(&format!("{} %", percent)),
            Cell::new(&stmts.to_string()),
            Cell::new(&files.to_string()),
        ]));
    }

    table.printstd();
    any_over_threshold
}
