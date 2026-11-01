// SPDX-License-Identifier: MIT OR Apache-2.0

//! Converts the Markdown report into a PDF with pandoc, if it is installed.
//!
//! The Markdown report is the main result, the PDF a convenience. pandoc is
//! a separate program, so that this tool stays small. pandoc needs a PDF
//! engine: typst, if it is installed, as it handles the SVG charts and all
//! scripts without more tools. Otherwise pandoc's default engine, which
//! needs LaTeX and rsvg-convert.

use std::io::ErrorKind;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, bail};

/// The name of the PDF file next to the Markdown report.
pub const PDF_FILE: &str = "report.pdf";
/// Typst rules for the layout of the PDF.
const TYPST_RULES: [&str; 11] = [
    // pandoc puts each table into a figure, which typst keeps on one page,
    // so that long tables ran over the bottom of the page.
    "#show figure: set block(breakable: true)",
    // The heading above a chart already names it.
    "#show figure.caption: none",
    // Charts as wide as the text, and tables aligned with it.
    "#show figure.where(kind: image): set image(width: 100%)",
    "#show figure.where(kind: table): set align(left)",
    // A shaded header and every other row keep long lists readable.
    "#set table(fill: (_, y) => if y == 0 { luma(225) } \
     else if calc.even(y) { luma(244) })",
    "#show table.cell.where(y: 0): strong",
    // Hyphens would change addresses, and justified narrow cells get wide
    // gaps. The smaller text leaves more room for long addresses, and
    // digits of the same width align the numbers.
    "#show table: set text(size: 9pt, hyphenate: false, \
     number-width: \"tabular\")",
    "#show table: set par(justify: false)",
    // Headings and links in the blue of the charts, and a line below each
    // section heading.
    "#show heading: set text(fill: rgb(\"#2a78d6\"))",
    "#show heading.where(level: 1): it => block(width: 100%, \
     stroke: (bottom: 0.5pt + luma(200)), inset: (bottom: 0.3em), it)",
    "#show link: set text(fill: rgb(\"#2a78d6\"))",
];
/// Page margins of the PDF, narrower than pandoc's 1.25 inches, so that
/// wide tables fit.
const TYPST_MARGIN: &str = r#"margin:{"x":"2cm","y":"2.2cm"}"#;
/// The first pandoc version with `--variable-json`, which the margins need.
/// Older versions reject the option, and with it the whole PDF.
const MARGIN_VERSION: (u32, u32) = (3, 7);
/// Line width of the Markdown for pandoc, larger than any table line.
///
/// pandoc gives the columns of a table with a line wider than this (72 by
/// default) widths in proportion to the dashes of its separator line,
/// which are too narrow for short columns such as the year. Without, typst
/// sizes the columns by their content. LaTeX does not wrap such columns,
/// so only typst gets this.
const TYPST_COLUMNS: usize = 10_000;
/// The font of the PDF with the typst engine.
const TYPST_FONT: &str = "Libertinus Serif";

/// Converts the Markdown report in a directory into a PDF next to it.
///
/// # Arguments
///
/// - `dir`: The directory of the report.
/// - `markdown`: The name of the Markdown report in `dir`.
pub fn convert(dir: &Path, markdown: &str) -> anyhow::Result<()> {
    convert_with("pandoc", "typst", dir, markdown)
}

/// Converts the Markdown report into a PDF with the given programs.
///
/// # Arguments
///
/// - `pandoc`: The name of the pandoc program.
/// - `typst`: The name of the typst program, the preferred PDF engine.
/// - `dir`: The directory of the report.
/// - `markdown`: The name of the Markdown report in `dir`.
fn convert_with(
    pandoc: &str,
    typst: &str,
    dir: &Path,
    markdown: &str,
) -> anyhow::Result<()> {
    let typst = is_installed(typst).then_some(typst);
    let margins = version(pandoc).is_some_and(|v| v >= MARGIN_VERSION);
    let args = arguments(markdown, typst, margins);
    let status = Command::new(pandoc)
        .current_dir(dir)
        .args(&args)
        .stdout(Stdio::null())
        // Messages of pandoc can quote the report, e.g. sender addresses.
        .stderr(Stdio::null())
        .status();
    let status = match status {
        Ok(status) => status,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            bail!("{pandoc} not found in PATH, skipping {PDF_FILE}");
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to run {pandoc}"));
        }
    };
    if !status.success() {
        bail!(
            "{pandoc} failed ({status}), skipping {PDF_FILE}. Run `{pandoc} \
             {}` in {} to see why",
            quoted(&args).join(" "),
            dir.display()
        );
    }
    Ok(())
}

/// Returns the arguments of pandoc.
///
/// # Arguments
///
/// - `markdown`: The name of the Markdown report.
/// - `typst`: The name of the typst program, if it is installed.
/// - `margins`: Whether pandoc can set the margins for typst.
fn arguments(
    markdown: &str,
    typst: Option<&str>,
    margins: bool,
) -> Vec<String> {
    let mut args = vec![
        markdown.to_owned(),
        "--output".to_owned(),
        PDF_FILE.to_owned(),
        // The heading of the report becomes the title of the PDF, above
        // the table of contents, instead of an entry in it.
        "--shift-heading-level-by=-1".to_owned(),
        // The lists of all senders and recipients make the PDF long.
        "--toc".to_owned(),
        "--variable=papersize:a4".to_owned(),
        "--variable=fontsize:10pt".to_owned(),
    ];
    if let Some(typst) = typst {
        args.push(format!("--pdf-engine={typst}"));
        // pandoc 3.7 passes an empty list of fonts, which typst 0.14
        // rejects. Libertinus Serif comes with typst and covers Latin,
        // Greek, and Cyrillic.
        args.push(format!("--variable=mainfont:{TYPST_FONT}"));
        args.push(format!("--columns={TYPST_COLUMNS}"));
        if margins {
            args.push(format!("--variable-json={TYPST_MARGIN}"));
        }
        args.extend(
            TYPST_RULES
                .iter()
                .map(|rule| format!("--variable=header-includes:{rule}")),
        );
    }
    args
}

/// Quotes arguments with spaces, so that a shell takes them as one.
///
/// # Arguments
///
/// - `args`: The arguments of a command.
fn quoted(args: &[String]) -> Vec<String> {
    args.iter()
        .map(|arg| {
            if arg.contains(' ') {
                format!("\"{arg}\"")
            } else {
                arg.clone()
            }
        })
        .collect()
}

/// Checks whether a program is installed, i.e. can be run from `PATH`.
///
/// # Arguments
///
/// - `program`: The name of the program, which must accept `--version`.
fn is_installed(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Returns the major and minor version of pandoc, if it runs.
///
/// # Arguments
///
/// - `pandoc`: The name of the pandoc program.
fn version(pandoc: &str) -> Option<(u32, u32)> {
    let output = Command::new(pandoc)
        .arg("--version")
        .stderr(Stdio::null())
        .output()
        .ok()?;
    parse_version(&String::from_utf8_lossy(&output.stdout))
}

/// Parses the major and minor version from the output of
/// `pandoc --version`, whose first line is e.g. `pandoc 3.7.0.2`.
///
/// # Arguments
///
/// - `output`: The output of `pandoc --version`.
fn parse_version(output: &str) -> Option<(u32, u32)> {
    let line = output.lines().next()?;
    let mut parts = line.split_whitespace().nth(1)?.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    /// A program name that no OS has.
    const MISSING: &str = "imap-statistics-test-missing-program";

    /// pandoc is optional, so without it, the error must say so plainly, and
    /// the CLI only warns.
    #[test]
    fn skips_the_pdf_without_pandoc() {
        let dir = TempDir::new("pdf-missing");
        let error = convert_with(MISSING, MISSING, dir.path(), "report.md")
            .expect_err("conversion should fail");
        assert_eq!(
            error.to_string(),
            format!("{MISSING} not found in PATH, skipping report.pdf")
        );
    }

    /// The layout rules are Typst code, which would break other PDF
    /// engines, e.g. LaTeX, so only typst gets them. LaTeX also needs the
    /// default line width, to wrap wide tables.
    #[test]
    fn passes_layout_rules_to_typst_only() {
        let with_typst = arguments("report.md", Some("typst"), true);
        assert!(with_typst.contains(&"--pdf-engine=typst".to_owned()));
        assert!(with_typst.contains(
            &"--variable=header-includes:#show figure: set block(breakable: \
              true)"
                .to_owned()
        ));
        assert!(with_typst.contains(&"--columns=10000".to_owned()));
        let without = arguments("report.md", None, true);
        assert!(!without.iter().any(|arg| arg.contains("header-includes")));
        assert!(!without.iter().any(|arg| arg.contains("columns")));
        assert!(!without.iter().any(|arg| arg.contains("margin")));
        assert!(!without.iter().any(|arg| arg.contains("pdf-engine")));
    }

    /// pandoc before 3.7 rejects `--variable-json`, and with it the PDF.
    #[test]
    fn sets_margins_only_if_pandoc_can() {
        let old = arguments("report.md", Some("typst"), false);
        assert!(!old.iter().any(|arg| arg.contains("margin")));
        let new = arguments("report.md", Some("typst"), true);
        assert!(new.iter().any(|arg| arg.starts_with("--variable-json")));
        assert_eq!(
            parse_version("pandoc 3.7.0.2\nFeatures: +server"),
            Some((3, 7))
        );
        assert_eq!(parse_version("pandoc.exe 3.10\n"), Some((3, 10)));
        assert_eq!(parse_version("pandoc 3.6\n"), Some((3, 6)));
        assert!(parse_version("pandoc 3.6\n") < Some(MARGIN_VERSION));
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn quotes_arguments_with_spaces() {
        let args = ["a.md".to_owned(), "--x=b c".to_owned()];
        assert_eq!(quoted(&args), ["a.md", "\"--x=b c\""]);
    }

    #[test]
    fn finds_no_missing_programs() {
        assert!(!is_installed(MISSING));
    }
}
