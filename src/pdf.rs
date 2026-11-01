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
    let mut args = vec![
        markdown.to_owned(),
        "--output".to_owned(),
        PDF_FILE.to_owned(),
    ];
    if is_installed(typst) {
        args.push(format!("--pdf-engine={typst}"));
        // pandoc 3.7 passes an empty list of fonts, which typst 0.14
        // rejects. Libertinus Serif comes with typst and covers Latin,
        // Greek, and Cyrillic.
        args.push(format!("--variable=mainfont:{TYPST_FONT}"));
    }
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
