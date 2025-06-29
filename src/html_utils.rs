use maud::{html, Markup, PreEscaped, DOCTYPE};

// Styles remain largely the same, but will be embedded directly by Maud
pub(crate) fn get_styles() -> &'static str {
    include_str!("styles.css")
}

const TABLE_SORTING_JS: &str = include_str!("table_sorter.js");

/// Renders a full HTML document with the given title and body markup.
pub fn render_html_doc(title_text: &str, body_content: Markup) -> String {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="UTF-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                title { (title_text) }
                style { (PreEscaped(get_styles())) }
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
            // Lower than or equal to bad threshold (e.g. 0 if bad is 0, good is 100)
            1.0 // Bad (closer to red)
        } else if value <= warn_thresh {
            // Between bad and warn (e.g. 50 if warn is 50)
            0.5 // Warning (closer to yellow)
        } else {
            0.0 // Good (closer to green)
        }
    } else {
        // Lower is better
        if value <= warn_thresh {
            // Lower than or equal to warn threshold (good)
            0.0 // Good (closer to green)
        } else if value <= bad_thresh {
            // Between warn and bad threshold (warning)
            0.5 // Warning (closer to yellow)
        } else {
            1.0 // Bad (closer to red)
        }
    };

    // Hue: 0 for Red, 60 for Yellow, 120 for Green
    // We want badness_factor 0.0 (good) -> hue 120 (green)
    // badness_factor 0.5 (warn) -> hue 60 (yellow)
    // badness_factor 1.0 (bad)  -> hue 0 (red)
    let hue = 120.0 * (1.0 - badness_factor);
    format!("background-color: hsl({hue}, 100%, 80%);")
}

/// Simplified get_metric_cell_style that uses MetricRanges
pub fn get_metric_cell_style(value: f64, ranges: &MetricRanges) -> String {
    if ranges.min == ranges.max {
        // Avoid division by zero and handle single-value case
        return String::from("background-color: hsl(120, 100%, 80%);"); // Default to green if no range
    }

    // Normalize value to 0-1 range. 0 is "best", 1 is "worst".
    let normalized_value = if ranges.higher_is_better {
        (ranges.max - value).max(0.0) / (ranges.max - ranges.min)
    } else {
        (value - ranges.min).max(0.0) / (ranges.max - ranges.min)
    };

    // Clamp normalized_value to ensure it's between 0 and 1
    let clamped_value = normalized_value.clamp(0.0, 1.0);

    // Hue: 0 for Red (worst), 120 for Green (best)
    let hue = 120.0 * (1.0 - clamped_value);
    format!("background-color: hsl({hue}, 100%, 88%); text-align: right;")
}

/// Defines min/max ranges for a metric, to be used for color scaling.
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
        let mut min_val = values[0];
        let mut max_val = values[0];
        for &v in values.iter().skip(1) {
            if v < min_val {
                min_val = v;
            }
            if v > max_val {
                max_val = v;
            }
        }
        Some(MetricRanges {
            min: min_val,
            max: max_val,
            higher_is_better,
        })
    }
}

/// Renders a list of metric explanations.
pub fn render_metric_explanation_list(explanations: &[(&str, &str)]) -> Markup {
    html! {
        ul class="metric-explanations" {
            @for (term, definition) in explanations {
                li {
                    strong { (term) ": " }
                    span { (definition) }
                }
            }
        }
    }
}
