// SPDX-License-Identifier: MIT OR Apache-2.0

//! CSV files of the report, for spreadsheets.
//!
//! Fields are quoted as in RFC 4180 [0], but lines end with LF, which all
//! common spreadsheet programs read, so that the files are the same on
//! every OS.
//!
//! [0]: https://www.rfc-editor.org/rfc/rfc4180

use std::iter;

/// Renders a CSV file.
///
/// # Arguments
///
/// - `header`: The titles of the columns.
/// - `rows`: The fields of each row.
pub(super) fn csv(
    header: &[&str],
    rows: impl IntoIterator<Item = Vec<String>>,
) -> String {
    let header = header.iter().map(|&title| title.to_owned()).collect();
    let mut csv = String::new();
    for row in iter::once(header).chain(rows) {
        let fields: Vec<String> = row.iter().map(|f| field(f)).collect();
        csv.push_str(&fields.join(","));
        csv.push('\n');
    }
    csv
}

/// Returns a field of a CSV file, quoted if needed.
///
/// Anybody can choose the name of a sender, so a field that a spreadsheet
/// would run as a formula gets a `'` in front [0].
///
/// [0]: https://owasp.org/www-community/attacks/CSV_Injection
///
/// # Arguments
///
/// - `text`: The text of the field.
fn field(text: &str) -> String {
    let text = if text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{text}")
    } else {
        text.to_owned()
    };
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_fields_with_separators() {
        let rows = [vec!["Miller, Ben".to_owned(), "say \"hi\"".to_owned()]];
        assert_eq!(
            csv(&["a", "b"], rows),
            "a,b\n\"Miller, Ben\",\"say \"\"hi\"\"\"\n"
        );
    }

    /// A spreadsheet must not run a sender name as a formula.
    #[test]
    fn defuses_formulas() {
        assert_eq!(field("=HYPERLINK(\"x\")"), "\"'=HYPERLINK(\"\"x\"\")\"");
        assert_eq!(field("@SUM(1)"), "'@SUM(1)");
        assert_eq!(field("Ben"), "Ben");
    }
}
