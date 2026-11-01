// SPDX-License-Identifier: MIT OR Apache-2.0

//! A run: fetch all sources, index the emails, and generate the report,
//! for the CLI and the GUI.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use jiff::Timestamp;

use crate::config::{Config, Source};
use crate::fetch::{self, Cancelled};
use crate::hint::hint;
use crate::index::{INDEX_FILE, Index, SourceEmails};
use crate::pdf::{self, PDF_FILE};
use crate::report::{self, REPORT_FILE};
use crate::store;

/// The result of a run whose report was written.
#[derive(Debug)]
pub struct Outcome {
    /// The directory of the report.
    pub report_dir: PathBuf,
    /// The number of sources whose fetch failed.
    pub failed: usize,
}

/// Fetches all sources, indexes the emails, and generates the report.
///
/// A failed source keeps its emails of earlier runs, so that the report is
/// still useful. The caller decides how to treat [`Outcome::failed`]. A
/// cancelled run stops with [`Cancelled`], without a report.
///
/// # Arguments
///
/// - `config`: The config.
/// - `offline`: Whether to skip the fetch.
/// - `output`: The directory for the report, or `None` for the default.
/// - `top`: The number of senders and recipients in each list of top
///   ones.
/// - `now`: The current time.
/// - `cancel`: Set when the user cancels.
/// - `log`: Receives every progress message, as one line.
pub fn run(
    config: &Config,
    offline: bool,
    output: Option<&Path>,
    top: usize,
    now: Timestamp,
    cancel: &AtomicBool,
    log: &mut impl FnMut(&str),
) -> anyhow::Result<Outcome> {
    let failed = if offline {
        0
    } else {
        fetch_all(config, cancel, log)?
    };
    if cancel.load(Ordering::Relaxed) {
        return Err(Cancelled.into());
    }
    let index = index(config, now, log)?;
    let report_dir = output
        .map_or_else(|| config.data_dir.join("report"), Path::to_path_buf);
    report::write(&index, &config.sources, now, &report_dir, top)?;
    log(&format!("Wrote {}", report_dir.join(REPORT_FILE).display()));
    // The PDF is a convenience, so its errors are only warnings.
    match pdf::convert(&report_dir, REPORT_FILE) {
        Ok(()) => {
            log(&format!("Wrote {}", report_dir.join(PDF_FILE).display()));
        }
        Err(error) => log(&format!("warning: {error:#}")),
    }
    Ok(Outcome { report_dir, failed })
}

/// Fetches all sources, also if one of them fails, and returns the number
/// of failed sources.
///
/// # Arguments
///
/// - `config`: The config.
/// - `cancel`: Set when the user cancels. Then the fetch fails with
///   [`Cancelled`].
/// - `log`: Receives every progress message.
fn fetch_all(
    config: &Config,
    cancel: &AtomicBool,
    log: &mut impl FnMut(&str),
) -> anyhow::Result<usize> {
    let mut failed = 0;
    for source in &config.sources {
        let name = &source.name;
        log(&format!(
            "{name}: fetching from {}:{}",
            source.host, source.port
        ));
        let progress = |message: &str| log(&format!("{name}: {message}"));
        match fetch::fetch_source(source, &config.data_dir, cancel, progress) {
            Ok(summary) => log(&format!(
                "{name}: done, {} emails in {} folders, {} new",
                summary.emails, summary.folders, summary.fetched
            )),
            Err(error) if error.is::<Cancelled>() => return Err(error),
            Err(error) => {
                log(&format!("{name}: error: {error:#}"));
                if let Some(hint) = hint(&error) {
                    log(&format!("{name}: hint: {hint}"));
                }
                failed += 1;
            }
        }
    }
    Ok(failed)
}

/// Builds the index of all fetched emails, and writes it.
///
/// # Arguments
///
/// - `config`: The config.
/// - `now`: The current time, the latest plausible date of an email.
/// - `log`: Receives every progress message.
fn index(
    config: &Config,
    now: Timestamp,
    log: &mut impl FnMut(&str),
) -> anyhow::Result<Index> {
    let mut sources = Vec::new();
    for source in &config.sources {
        let dir = config.data_dir.join(&source.name);
        let headers = store::load(&dir)?;
        if headers.is_empty() {
            log(&format!("{}: no fetched emails", source.name));
        }
        sources.push(SourceEmails {
            name: source.name.clone(),
            headers,
            sent_folders: store::sent_folders(&dir)?,
        });
    }
    let own_addresses: HashSet<String> = config
        .sources
        .iter()
        .flat_map(Source::own_addresses)
        .collect();
    let index = Index::build(&sources, &own_addresses, now.as_second());
    let path = config.data_dir.join(INDEX_FILE);
    index.write(&path)?;
    let copies: usize = sources.iter().map(|s| s.headers.len()).sum();
    let sent = index.emails.iter().filter(|e| e.sent).count();
    let undated = index.emails.iter().filter(|e| e.year.is_none()).count();
    log(&format!(
        "Indexed {} emails ({sent} sent) from {copies} copies, {undated} \
         without date, into {}",
        index.emails.len(),
        path.display()
    ));
    Ok(index)
}
