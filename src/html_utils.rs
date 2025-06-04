use anyhow::Result;
use std::fmt::Write;

const HTML_HEAD_CONTENT: &str = r#"
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Analysis Report</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
            margin: 0;
            background-color: #f4f4f4;
            color: #333;
            padding: 20px;
        }
        h1 {
            color: #333;
            text-align: center;
        }
        table {
            width: 90%;
            margin: 20px auto;
            border-collapse: collapse;
            box-shadow: 0 2px 15px rgba(0,0,0,0.1);
            background-color: white;
        }
        th, td {
            border: 1px solid #ddd;
            padding: 10px 12px;
            text-align: left;
        }
        th {
            background-color: #007bff;
            color: white;
            font-weight: bold;
        }
        tr:nth-child(even) {
            background-color: #f9f9f9;
        }
        tr:hover {
            background-color: #f1f1f1;
        }
        .metric-cell {
            /* Style for cells that will be color-coded */
        }
        caption {
            caption-side: top;
            font-size: 1.2em;
            font-weight: bold;
            padding: 10px;
            color: #007bff;
        }
    </style>
</head>
"#;

pub fn start_html_doc(buffer: &mut String, title: &str) -> Result<()> {
    writeln!(buffer, "<!DOCTYPE html>")?;
    writeln!(buffer, "<html lang=\"en\">")?;
    writeln!(buffer, "{}", HTML_HEAD_CONTENT)?;
    writeln!(buffer, "<body>")?;
    writeln!(buffer, "<h1>{}</h1>", title)?;
    Ok(())
}

pub fn end_html_doc(buffer: &mut String) -> Result<()> {
    writeln!(buffer, "</body>")?;
    writeln!(buffer, "</html>")?;
    Ok(())
}

pub fn start_table(buffer: &mut String, caption: Option<&str>) -> Result<()> {
    writeln!(buffer, "<table>")?;
    if let Some(cap) = caption {
        writeln!(buffer, "<caption>{}</caption>", cap)?;
    }
    Ok(())
}

pub fn end_table(buffer: &mut String) -> Result<()> {
    writeln!(buffer, "</table>")?;
    Ok(())
}

pub fn add_table_header(buffer: &mut String, headers: &[&str]) -> Result<()> {
    writeln!(buffer, "<thead><tr>")?;
    for header in headers {
        writeln!(buffer, "<th>{}</th>", header)?;
    }
    writeln!(buffer, "</tr></thead><tbody>")?;
    Ok(())
}

pub fn add_table_row(
    buffer: &mut String,
    cells: &[String],
    cell_styles: Option<&[String]>,
) -> Result<()> {
    writeln!(buffer, "<tr>")?;
    for (i, cell_content) in cells.iter().enumerate() {
        if let Some(styles) = cell_styles {
            if i < styles.len() && !styles[i].is_empty() {
                writeln!(buffer, "<td style=\"{}\">{}</td>", styles[i], cell_content)?;
            } else {
                writeln!(buffer, "<td>{}</td>", cell_content)?;
            }
        } else {
            writeln!(buffer, "<td>{}</td>", cell_content)?;
        }
    }
    writeln!(buffer, "</tr>")?;
    Ok(())
}

pub fn end_table_body(buffer: &mut String) -> Result<()> {
    writeln!(buffer, "</tbody>")?;
    Ok(())
}

/// Generates an HSL background color style string based on the value's "badness".
/// `value`: The metric value to assess.
/// `warn_threshold`: Values at or above this are entering the "yellow/orange" zone.
/// `bad_threshold`: Values at or above this are in the "red" zone.
/// `is_higher_better`: If true, higher values are good (green), lower are bad (red).
///                     If false, lower values are good (green), higher are bad (red).
/// The spectrum is Green (good) -> Yellow (warning) -> Red (bad).
pub fn get_cell_style(
    value: f64,
    warn_threshold: f64,
    bad_threshold: f64,
    is_higher_better: bool,
) -> String {
    // Ensure warn_threshold <= bad_threshold if higher is not better, and vice versa
    let (warn_thresh, bad_thresh) = if is_higher_better {
        (
            warn_threshold.max(bad_threshold),
            warn_threshold.min(bad_threshold),
        )
    } else {
        (
            warn_threshold.min(bad_threshold),
            warn_threshold.max(bad_threshold),
        )
    };

    let badness_factor = if is_higher_better {
        if value <= bad_thresh {
            1.0
        }
        // Good (green) range for higher is better
        else if value <= warn_thresh {
            0.5
        }
        // Warn (yellow) range
        else {
            0.0
        } // Bad (red) range
    } else {
        if value <= warn_thresh {
            1.0
        }
        // Good (green) range for lower is better
        else if value <= bad_thresh {
            0.5
        }
        // Warn (yellow) range
        else {
            0.0
        } // Bad (red) range
    };

    // Hue: 0 for Red, 60 for Yellow, 120 for Green
    let hue = badness_factor * 120.0;
    // Saturation and Lightness can be fixed for vibrant colors
    format!("background-color: hsl({}, 100%, 80%);", hue)
}

pub fn get_metric_cell_style(value: f64, ranges: &MetricRanges) -> String {
    let normalized_value = if ranges.max == ranges.min {
        // Avoid division by zero if all values are the same
        0.5 // Default to a neutral color (e.g., yellow) or handle as per requirements
    } else {
        (value - ranges.min) / (ranges.max - ranges.min)
    };

    let score = if ranges.higher_is_better {
        normalized_value // Higher value means higher score (closer to green)
    } else {
        1.0 - normalized_value // Higher value means lower score (closer to red)
    };

    // score is now 0 (bad/red) to 1 (good/green)
    let hue = score * 120.0; // 0 is Red, 120 is Green
    format!(
        "background-color: hsl({:.0}, 100%, 80%);",
        hue.max(0.0).min(120.0)
    )
}

pub struct MetricRanges {
    pub min: f64,
    pub max: f64,
    pub higher_is_better: bool,
}

impl MetricRanges {
    pub fn from_values(values: &[f64], higher_is_better: bool) -> Option<Self> {
        if values.is_empty() {
            return None;
        }
        let mut min = values[0];
        let mut max = values[0];
        for &val in values.iter().skip(1) {
            if val < min {
                min = val;
            }
            if val > max {
                max = val;
            }
        }
        Some(Self {
            min,
            max,
            higher_is_better,
        })
    }
}

pub fn write_metric_explanation_list(
    buffer: &mut String,
    explanations: &[(&str, &str)],
) -> Result<()> {
    writeln!(buffer, "<h2>Metric Explanations</h2>")?;
    writeln!(buffer, "<ul>")?;
    for (metric, explanation) in explanations {
        writeln!(
            buffer,
            "<li><strong>{}:</strong> {}</li>",
            metric, explanation
        )?;
    }
    writeln!(buffer, "</ul>")?;
    Ok(())
}
