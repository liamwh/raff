use maud::{html, Markup, PreEscaped, DOCTYPE};

// Styles remain largely the same, but will be embedded directly by Maud
const CSS_STYLES: &str = r#"
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
    position: relative; /* For positioning sort arrows */
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
.sortable-header {
    cursor: pointer;
}
.sortable-header::after {
    content: '\25b2\25bc'; /* Default: up and down triangles */
    font-size: 0.9em;
    margin-left: 7px;
    color: #a0cfff; /* Lighter blue, less prominent */
    position: absolute;
    right: 8px;
    top: 50%;
    transform: translateY(-50%);
}
.sortable-header.sort-asc::after {
    content: '\25b2'; /* Up triangle */
    color: #ffffff; /* White, prominent */
}
.sortable-header.sort-desc::after {
    content: '\25bc'; /* Down triangle */
    color: #ffffff; /* White, prominent */
}
"#;

const TABLE_SORTING_JS: &str = r###"
document.addEventListener('DOMContentLoaded', function() {
    const getCellValue = (tr, idx, type) => {
        const cellContent = tr.children[idx].innerText || tr.children[idx].textContent;
        if (type === 'number') {
            // Attempt to parse float, removing common non-numeric characters like %, ,, $
            const num = parseFloat(cellContent.replace(/[%$,]/g, ''));
            return isNaN(num) ? -Infinity : num; // Treat non-numeric as very small for sorting
        }
        return cellContent.trim().toLowerCase(); // Case-insensitive string sort
    };

    // type === 'number' is already implicitly handled by getCellValue returning numbers for 'number' type.
    // The comparison logic v1 - v2 works for numbers. For strings, localeCompare is used.
    const comparer = (idx, asc, type) => (a, b) => {
        const vA = getCellValue(a, idx, type);
        const vB = getCellValue(b, idx, type);

        let comparison = 0;
        if (type === 'number') {
            comparison = vA - vB;
        } else {
            comparison = vA.toString().localeCompare(vB.toString());
        }
        return asc ? comparison : -comparison;
    };

    document.querySelectorAll('.sortable-table .sortable-header').forEach(th => {
        th.addEventListener('click', (() => {
            const table = th.closest('table');
            const tbody = table.querySelector('tbody');
            if (!tbody) return; // No table body to sort

            const columnIndex = parseInt(th.dataset.columnIndex);
            const sortType = th.dataset.sortType || 'string';

            let newAsc;
            // If already sorted by this column, toggle direction
            if (th.classList.contains('sort-asc')) {
                th.classList.remove('sort-asc');
                th.classList.add('sort-desc');
                newAsc = false;
            } else if (th.classList.contains('sort-desc')) {
                th.classList.remove('sort-desc');
                th.classList.add('sort-asc');
                newAsc = true;
            } else { // Not sorted by this column yet, or sorted by another
                // Remove sort classes from other headers
                table.querySelectorAll('.sortable-header').forEach(otherTh => {
                    otherTh.classList.remove('sort-asc', 'sort-desc');
                });
                th.classList.add('sort-asc');
                newAsc = true;
            }

            Array.from(tbody.querySelectorAll('tr'))
                .sort(comparer(columnIndex, newAsc, sortType))
                .forEach(tr => tbody.appendChild(tr)); // Re-append rows to sort them in the DOM
        }));
    });
});
"###;

/// Renders a full HTML document with the given title and body markup.
pub fn render_html_doc(title_text: &str, body_content: Markup) -> String {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="UTF-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                title { (title_text) }
                style { (PreEscaped(CSS_STYLES)) }
            }
            body {
                h1 { (title_text) }
                (body_content)
                script { (PreEscaped(TABLE_SORTING_JS)) }
            }
        }
    }
    .into_string()
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

/// Renders a list of metric explanations using Maud.
pub fn render_metric_explanation_list(explanations: &[(&str, &str)]) -> Markup {
    html! {
        h2 { "Metric Explanations" }
        ul {
            @for (metric, explanation) in explanations {
                li {
                    strong { (metric) ": " } (explanation)
                }
            }
        }
    }
}
