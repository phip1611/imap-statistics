// SPDX-License-Identifier: MIT OR Apache-2.0

//! Bar charts as standalone SVG files, for the Markdown report.
//!
//! The charts have no background, so that they fit on any page, follow
//! the color scheme of the viewer (`prefers-color-scheme`), and show the
//! value of each bar on hover (`<title>`), where the viewer supports it.
//! The colors pass the contrast and colorblind checks on white and on
//! GitHub's dark background (`#0d1117`).

use std::fmt::Write;

const WIDTH: f64 = 720.0;
const FONT: &str = r#"12px system-ui, -apple-system, "Segoe UI", sans-serif"#;
/// Bars are at most this thick, so that few bars do not look heavy.
const MAX_BAR: f64 = 24.0;
/// Rounded data end of a bar. The end at the baseline stays square.
const RADIUS: f64 = 4.0;
/// Space between bars, so that they never touch.
const GAP: f64 = 2.0;
/// Width of a character at 12px, generous, so that wide fonts fit too.
const CHAR_WIDTH: f64 = 7.5;
/// CSS classes of the series of a chart, in the order of their colors.
const SERIES_CLASSES: [&str; 2] = ["bar", "bar2"];
/// Labels of bars are cut to this many characters.
const MAX_LABEL: usize = 40;

/// Returns a column chart: one column per label, left to right.
///
/// With several series, their values stack in each column, the first at
/// the bottom, and a legend names them. At most two series, as the style
/// has two colors.
///
/// # Arguments
///
/// - `title`: The title, for screen readers and on hover.
/// - `series`: The names of the series, for the legend.
/// - `data`: The label and the value of each series of each column.
#[must_use]
pub fn columns(
    title: &str,
    series: &[&str],
    data: &[(String, Vec<u64>)],
) -> String {
    const HEIGHT: f64 = 280.0;
    const LEFT: f64 = 48.0;
    const RIGHT: f64 = 16.0;
    const BOTTOM: f64 = 28.0;
    // Minimal distance between labels on the x axis, enough for "2026".
    const LABEL_DISTANCE: f64 = 40.0;
    let legend = series.len() > 1;
    let top_margin = if legend { 40.0 } else { 16.0 };
    let maximum = data
        .iter()
        .map(|(_, values)| values.iter().sum::<u64>())
        .max()
        .unwrap_or(0);
    if maximum == 0 {
        return empty(title, HEIGHT);
    }
    let (step, top) = nice_scale(maximum);
    let baseline = HEIGHT - BOTTOM;
    let plot_height = baseline - top_margin;
    let y = |value: u64| baseline - value as f64 / top as f64 * plot_height;
    let band = (WIDTH - LEFT - RIGHT) / data.len() as f64;
    let width = (band - GAP).min(MAX_BAR);
    // Every label, or every 2nd, 5th, ... if they get crowded.
    let every = [1, 2, 5, 10, 20, 50]
        .into_iter()
        .find(|&n| n as f64 * band >= LABEL_DISTANCE)
        .unwrap_or(100);

    let mut body = String::new();
    if legend {
        body.push_str(&legend_items(series, LEFT, 16.0));
    }
    for value in (0..=top).step_by(usize::try_from(step).unwrap_or(1)) {
        let y = y(value);
        let class = if value == 0 { "axis" } else { "grid" };
        let _ = write!(
            body,
            "<line class=\"{class}\" x1=\"{LEFT}\" x2=\"{}\" \
             y1=\"{y:.1}\" y2=\"{y:.1}\"/>",
            WIDTH - RIGHT
        );
        let _ = write!(
            body,
            r#"<text x="{}" y="{:.1}" text-anchor="end">{}</text>"#,
            LEFT - 8.0,
            y + 4.0,
            compact(value)
        );
    }
    for (index, (label, values)) in data.iter().enumerate() {
        let center = LEFT + (index as f64 + 0.5) * band;
        let x = center - width / 2.0;
        let last = values.iter().rposition(|&value| value > 0);
        let mut below = 0;
        for (series_index, &value) in values.iter().enumerate() {
            if value == 0 {
                continue;
            }
            // A gap between stacked segments, so that they never touch.
            let bottom = if below == 0 { baseline } else { y(below) - GAP };
            below += value;
            let path = if Some(series_index) == last {
                column_path(x, y(below), width, bottom)
            } else {
                rect_path(x, y(below), width, bottom)
            };
            let name = if legend {
                format!("{}, {}", escape(label), escape(series[series_index]))
            } else {
                escape(label)
            };
            let _ = write!(
                body,
                "<path class=\"{}\" d=\"{path}\">\
                 <title>{name}: {}</title></path>",
                SERIES_CLASSES[series_index],
                number(value)
            );
        }
        if index % every == 0 {
            let _ = write!(
                body,
                "<text x=\"{center:.1}\" y=\"{}\" \
                 text-anchor=\"middle\">{}</text>",
                baseline + 18.0,
                escape(label)
            );
        }
    }
    document(title, HEIGHT, &body)
}

/// Returns a legend: a colored square and the name of each series.
///
/// # Arguments
///
/// - `series`: The names of the series.
/// - `left`: The left edge of the legend.
/// - `top`: The upper edge of the legend.
fn legend_items(series: &[&str], left: f64, top: f64) -> String {
    const SQUARE: f64 = 10.0;
    let mut legend = String::new();
    let mut x = left;
    for (index, name) in series.iter().enumerate() {
        let _ = write!(
            legend,
            "<rect class=\"{}\" x=\"{x:.1}\" y=\"{:.1}\" \
             width=\"{SQUARE}\" height=\"{SQUARE}\" rx=\"2\"/>",
            SERIES_CLASSES[index],
            top - SQUARE + 1.0
        );
        let _ = write!(
            legend,
            r#"<text x="{:.1}" y="{top:.1}">{}</text>"#,
            x + SQUARE + 6.0,
            escape(name)
        );
        x += SQUARE + 6.0 + name.chars().count() as f64 * CHAR_WIDTH + 16.0;
    }
    legend
}

/// Returns a horizontal bar chart: one bar per label, top to bottom.
///
/// The value is at the end of each bar.
///
/// # Arguments
///
/// - `title`: The title, for screen readers and on hover.
/// - `data`: The label and the value of each bar.
#[must_use]
pub fn bars(title: &str, data: &[(String, u64)]) -> String {
    const ROW: f64 = 26.0;
    const THICKNESS: f64 = 16.0;
    const PADDING: f64 = 8.0;
    // Room for the value at the end of the longest bar.
    const VALUE_WIDTH: f64 = 64.0;
    let maximum = data.iter().map(|(_, value)| *value).max().unwrap_or(0);
    let height = data.len() as f64 * ROW + 2.0 * PADDING;
    if maximum == 0 {
        return empty(title, ROW + 2.0 * PADDING);
    }
    let labels: Vec<_> = data.iter().map(|(label, _)| shorten(label)).collect();
    let longest = labels.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let left = longest as f64 * CHAR_WIDTH + 2.0 * PADDING;
    let length = WIDTH - left - VALUE_WIDTH;

    let mut body = String::new();
    let _ = write!(
        body,
        "<line class=\"axis\" x1=\"{left}\" x2=\"{left}\" \
         y1=\"{PADDING}\" y2=\"{}\"/>",
        height - PADDING
    );
    for (index, ((_, value), label)) in data.iter().zip(&labels).enumerate() {
        let center = PADDING + (index as f64 + 0.5) * ROW;
        let end = left + *value as f64 / maximum as f64 * length;
        let _ = write!(
            body,
            r#"<text x="{}" y="{:.1}" text-anchor="end">{}</text>"#,
            left - PADDING,
            center + 4.0,
            escape(label)
        );
        if *value > 0 {
            let path = bar_path(left, end, center - THICKNESS / 2.0, THICKNESS);
            let _ = write!(
                body,
                r#"<path class="bar" d="{path}"><title>{}: {}</title></path>"#,
                escape(label),
                number(*value)
            );
        }
        let _ = write!(
            body,
            r#"<text x="{:.1}" y="{:.1}">{}</text>"#,
            end + 6.0,
            center + 4.0,
            number(*value)
        );
    }
    document(title, height, &body)
}

/// Formats a number with thousands separators, e.g. `12,345`.
///
/// # Arguments
///
/// - `value`: The number.
#[must_use]
pub fn number(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(digit);
    }
    formatted
}

/// Formats an axis tick compactly, e.g. `1.5K`.
///
/// # Arguments
///
/// - `value`: The value of the tick.
fn compact(value: u64) -> String {
    let (divisor, suffix) = match value {
        1_000_000.. => (1_000_000, "M"),
        1_000.. => (1_000, "K"),
        _ => return value.to_string(),
    };
    let whole = value / divisor;
    let tenth = value % divisor * 10 / divisor;
    if tenth == 0 {
        format!("{whole}{suffix}")
    } else {
        format!("{whole}.{tenth}{suffix}")
    }
}

/// Returns a round tick step and the top of the axis.
///
/// The steps are 1, 2, or 5 times a power of ten, for at most five gridlines
/// above zero.
///
/// # Arguments
///
/// - `maximum`: The largest value of the chart.
fn nice_scale(maximum: u64) -> (u64, u64) {
    let raw = maximum.div_ceil(5).max(1);
    let magnitude = 10_u64.pow(raw.ilog10());
    let step = [1, 2, 5, 10]
        .into_iter()
        .map(|m| m * magnitude)
        .find(|&step| step >= raw)
        .unwrap_or(10 * magnitude);
    (step, maximum.div_ceil(step).max(1) * step)
}

/// Returns the SVG path of a column with rounded upper corners.
///
/// # Arguments
///
/// - `x`: The left edge.
/// - `top`: The upper end.
/// - `width`: The width.
/// - `baseline`: The lower end, at the axis.
fn column_path(x: f64, top: f64, width: f64, baseline: f64) -> String {
    let r = RADIUS.min(width / 2.0).min(baseline - top);
    let right = x + width;
    format!(
        "M{x:.2},{baseline:.2}V{:.2}Q{x:.2},{top:.2} {:.2},{top:.2}\
         H{:.2}Q{right:.2},{top:.2} {right:.2},{:.2}V{baseline:.2}Z",
        top + r,
        x + r,
        right - r,
        top + r
    )
}

/// Returns the SVG path of a column segment with square corners.
///
/// # Arguments
///
/// - `x`: The left edge.
/// - `top`: The upper end.
/// - `width`: The width.
/// - `bottom`: The lower end.
fn rect_path(x: f64, top: f64, width: f64, bottom: f64) -> String {
    format!("M{x:.2},{bottom:.2}V{top:.2}H{:.2}V{bottom:.2}Z", x + width)
}

/// Returns the SVG path of a bar with rounded corners at its end.
///
/// # Arguments
///
/// - `start`: The left end, at the axis.
/// - `end`: The right end.
/// - `y`: The upper edge.
/// - `thickness`: The thickness.
fn bar_path(start: f64, end: f64, y: f64, thickness: f64) -> String {
    let r = RADIUS.min(thickness / 2.0).min(end - start);
    let bottom = y + thickness;
    format!(
        "M{start:.2},{y:.2}H{:.2}Q{end:.2},{y:.2} {end:.2},{:.2}\
         V{:.2}Q{end:.2},{bottom:.2} {:.2},{bottom:.2}H{start:.2}Z",
        end - r,
        y + r,
        bottom - r,
        end - r
    )
}

/// Returns a chart that says that there is no data.
///
/// # Arguments
///
/// - `title`: The title, for screen readers and on hover.
/// - `height`: The height of the chart.
fn empty(title: &str, height: f64) -> String {
    let body = format!(
        r#"<text x="{}" y="{:.1}" text-anchor="middle">No emails</text>"#,
        WIDTH / 2.0,
        height / 2.0 + 4.0
    );
    document(title, height, &body)
}

/// Returns an SVG document with the style of all charts.
///
/// # Arguments
///
/// - `title`: The title, for screen readers and on hover.
/// - `height`: The height of the chart.
/// - `body`: The elements of the chart.
fn document(title: &str, height: f64, body: &str) -> String {
    let title = escape(title);
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" role="img"
  viewBox="0 0 {WIDTH} {height}" width="{WIDTH}" height="{height}"
  aria-label="{title}">
<title>{title}</title>
<style>
.bar {{ fill: #2a78d6 }}
.bar2 {{ fill: #eb6834 }}
.grid {{ stroke: #e1e0d9; stroke-width: 1 }}
.axis {{ stroke: #c3c2b7; stroke-width: 1 }}
text {{ fill: #52514e; font: {FONT} }}
@media (prefers-color-scheme: dark) {{
  .bar {{ fill: #3987e5 }}
  .bar2 {{ fill: #d95926 }}
  .grid {{ stroke: #2c2c2a }}
  .axis {{ stroke: #383835 }}
  text {{ fill: #c3c2b7 }}
}}
</style>
{body}
</svg>
"#
    )
}

/// Cuts a label to [`MAX_LABEL`] characters.
///
/// # Arguments
///
/// - `label`: The label.
fn shorten(label: &str) -> String {
    if label.chars().count() <= MAX_LABEL {
        return label.to_owned();
    }
    let mut short: String = label.chars().take(MAX_LABEL - 3).collect();
    short.push_str("...");
    short
}

/// Escapes text for XML.
///
/// # Arguments
///
/// - `text`: The text.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns chart data with owned labels.
    ///
    /// # Arguments
    ///
    /// - `values`: The label and the value of each bar.
    fn data(values: &[(&str, u64)]) -> Vec<(String, u64)> {
        values.iter().map(|&(l, v)| (l.to_owned(), v)).collect()
    }

    #[test]
    fn formats_numbers() {
        for (value, expected) in [
            (0, "0"),
            (999, "999"),
            (1_000, "1,000"),
            (1_234_567, "1,234,567"),
        ] {
            assert_eq!(number(value), expected);
        }
        for (value, expected) in [
            (500, "500"),
            (1_000, "1K"),
            (1_500, "1.5K"),
            (2_000_000, "2M"),
        ] {
            assert_eq!(compact(value), expected);
        }
    }

    /// Axis ticks are round numbers, with at most five gridlines above zero.
    #[test]
    fn chooses_round_scales() {
        for (maximum, expected) in [
            (1, (1, 1)),
            (7, (2, 8)),
            (100, (20, 100)),
            (1_234, (500, 1_500)),
            (9_999, (2_000, 10_000)),
        ] {
            assert_eq!(nice_scale(maximum), expected, "{maximum}");
        }
    }

    #[test]
    fn draws_one_column_per_value_above_zero() {
        let svg = columns(
            "Emails per year",
            &["Emails"],
            &single(&data(&[("2023", 10), ("2024", 0), ("2025", 25)])),
        );
        assert_eq!(svg.matches(r#"class="bar""#).count(), 2);
        assert!(svg.contains("<title>2025: 25</title>"), "{svg}");
        assert!(svg.contains(">2024</text>"), "{svg}");
        assert!(svg.contains("<title>Emails per year</title>"), "{svg}");
        assert!(svg.contains("prefers-color-scheme: dark"), "{svg}");
    }

    /// Returns chart data with one series per label.
    ///
    /// # Arguments
    ///
    /// - `data`: The label and the value of each column.
    fn single(data: &[(String, u64)]) -> Vec<(String, Vec<u64>)> {
        data.iter().map(|(l, v)| (l.clone(), vec![*v])).collect()
    }

    /// Received and sent emails stack in one column per year, with a legend
    /// that names them.
    #[test]
    fn stacks_two_series_with_a_legend() {
        let data = [
            ("2024".to_owned(), vec![10, 5]),
            ("2025".to_owned(), vec![0, 3]),
        ];
        let svg = columns("Emails per year", &["Received", "Sent"], &data);
        assert!(svg.contains(">Received</text>"), "{svg}");
        assert!(svg.contains(">Sent</text>"), "{svg}");
        assert!(svg.contains("<title>2024, Received: 10</title>"), "{svg}");
        assert!(svg.contains("<title>2024, Sent: 5</title>"), "{svg}");
        assert!(svg.contains("<title>2025, Sent: 3</title>"), "{svg}");
        // Three segments and two squares in the legend.
        assert_eq!(svg.matches(r#"class="bar""#).count(), 2, "{svg}");
        assert_eq!(svg.matches(r#"class="bar2""#).count(), 3, "{svg}");
    }

    /// With many years, the labels would overlap, so only every n-th year has
    /// one.
    #[test]
    fn thins_out_crowded_column_labels() {
        let years: Vec<_> =
            (1990..2030).map(|year| (year.to_string(), 1)).collect();
        let svg = columns("Emails per year", &["Emails"], &single(&years));
        assert!(svg.contains(">1990</text>"), "{svg}");
        assert!(!svg.contains(">1991</text>"), "{svg}");
    }

    #[test]
    fn draws_bars_with_values_and_escaped_labels() {
        let long = "a".repeat(50);
        let svg = bars(
            "Top senders",
            &data(&[("<a&b@example.com>", 1_500), (&long, 3)]),
        );
        assert_eq!(svg.matches(r#"class="bar""#).count(), 2);
        assert!(svg.contains(">&lt;a&amp;b@example.com&gt;</text>"), "{svg}");
        assert!(svg.contains(">1,500</text>"), "{svg}");
        assert!(svg.contains(&format!(">{}...</text>", "a".repeat(37))));
    }

    #[test]
    fn says_so_without_data() {
        for svg in [columns("x", &["x"], &[]), bars("x", &data(&[("a", 0)]))] {
            assert!(svg.contains(">No emails</text>"), "{svg}");
            assert!(!svg.contains(r#"class="bar""#), "{svg}");
        }
    }
}
