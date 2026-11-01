// SPDX-License-Identifier: MIT OR Apache-2.0

//! Markdown of the report: paragraphs, tables, and the escaping of text.

use std::mem;

use crate::svg::number;

/// Paragraphs of the Markdown source are wrapped at this width, so that the
/// report reads well as plain text too. Tables may be wider.
pub(super) const MAX_WIDTH: usize = 80;

/// Formats a count with its noun, e.g. `1 email` or `2 emails`.
///
/// # Arguments
///
/// - `value`: The count.
/// - `one`: The noun in singular.
/// - `many`: The noun in plural.
pub(super) fn count(value: u64, one: &str, many: &str) -> String {
    let noun = if value == 1 { one } else { many };
    format!("{} {noun}", number(value))
}

/// Wraps text into lines of at most [`MAX_WIDTH`], then an empty line.
///
/// Lines break between words. A word longer than a line gets a line of its
/// own.
///
/// # Arguments
///
/// - `text`: The text of the paragraph, in one line.
pub(super) fn paragraph(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let width = line.chars().count() + 1 + word.chars().count();
        if !line.is_empty() && width > MAX_WIDTH {
            lines.push(mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines.push(String::new());
    lines
}

/// Renders a Markdown table with aligned columns, then an empty line.
///
/// Cells are padded to the width of their column, so that the table is
/// easy to read as text too. Every column has an explicit alignment, as
/// some renderers, e.g. pandoc, center columns without one.
///
/// # Arguments
///
/// - `header`: The titles of the columns.
/// - `right`: The indexes of the columns to align to the right.
/// - `rows`: The cells of each row.
pub(super) fn table(
    header: &[&str],
    right: &[usize],
    rows: impl IntoIterator<Item = Vec<String>>,
) -> Vec<String> {
    let header: Vec<String> = header.iter().map(|&h| h.to_owned()).collect();
    let rows: Vec<Vec<String>> = rows.into_iter().collect();
    // At least 3, the shortest separator that Markdown allows.
    let mut widths: Vec<usize> =
        header.iter().map(|h| h.chars().count().max(3)).collect();
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let line = |cells: &[String]| {
        let padded: Vec<String> = cells
            .iter()
            .zip(&widths)
            .enumerate()
            .map(|(column, (cell, &width))| {
                if right.contains(&column) {
                    format!("{cell:>width$}")
                } else {
                    format!("{cell:<width$}")
                }
            })
            .collect();
        format!("| {} |", padded.join(" | "))
    };
    let separator: Vec<String> = widths
        .iter()
        .enumerate()
        .map(|(column, &width)| {
            if right.contains(&column) {
                format!("{}:", "-".repeat(width - 1))
            } else {
                format!(":{}", "-".repeat(width - 1))
            }
        })
        .collect();
    let mut lines = vec![line(&header), line(&separator)];
    lines.extend(rows.iter().map(|row| line(row)));
    lines.push(String::new());
    lines
}

/// Escapes text for a cell of a Markdown table.
///
/// # Arguments
///
/// - `text`: The text.
pub(super) fn escape(text: &str) -> String {
    let mut escaped = String::new();
    for c in text.chars() {
        if matches!(c, '\\' | '|' | '*' | '_' | '`' | '<' | '>' | '[' | ']') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tables must read well as plain text, so all cells of a column have
    /// the same width.
    #[test]
    fn pads_table_cells_to_align_columns() {
        let rows = [
            vec!["a".to_owned(), "1".to_owned()],
            vec!["bbbb".to_owned(), "1,234".to_owned()],
        ];
        assert_eq!(
            table(&["Name", "Count"], &[1], rows),
            [
                "| Name | Count |",
                "| :--- | ----: |",
                "| a    |     1 |",
                "| bbbb | 1,234 |",
                "",
            ]
        );
    }

    #[test]
    fn wraps_paragraphs_between_words() {
        let text = format!("{} {} {}", "a".repeat(50), "b".repeat(29), "c");
        assert_eq!(
            paragraph(&text),
            [
                format!("{} {}", "a".repeat(50), "b".repeat(29)),
                "c".into(),
                String::new()
            ]
        );
        let long = "x".repeat(90);
        assert_eq!(paragraph(&format!("a {long} b")), ["a", &long, "b", ""]);
    }

    #[test]
    fn escapes_table_cells() {
        assert_eq!(escape("a|b *c* <d>"), r"a\|b \*c\* \<d\>");
    }
}
