// SPDX-License-Identifier: MIT OR Apache-2.0

//! CLI to generate statistics about the emails in IMAP accounts.

use std::collections::HashSet;
use std::env;
use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use jiff::Timestamp;

use imap_statistics::config::{Config, Source};
use imap_statistics::fetch;
use imap_statistics::hint::hint;
use imap_statistics::index::{INDEX_FILE, Index, SourceEmails};
use imap_statistics::pdf::{self, PDF_FILE};
use imap_statistics::report::{self, REPORT_FILE};
use imap_statistics::store;

/// Fetches the headers of all new emails from IMAP accounts, indexes them,
/// and generates a Markdown report with statistics.
///
/// Every run continues where the last one stopped, so it can be repeated
/// at any time, e.g. after Ctrl+C.
#[derive(Debug, Parser)]
#[command(version)]
struct Cli {
    /// Path of the config file.
    #[arg(short, long, global = true, default_value = "imap-statistics.toml")]
    config: PathBuf,
    /// Uses the fetched emails only, without connecting to the sources.
    #[arg(long)]
    offline: bool,
    /// Directory for the report [default: "report" in the data directory].
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Number of senders and recipients in each list of top ones.
    #[arg(long, default_value_t = 5)]
    top: usize,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Checks the config file and lists the sources, without connecting to
    /// them.
    CheckConfig,
}

/// Runs the command of the command line.
fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = Config::load(&cli.config)?;
    match cli.command {
        Some(Command::CheckConfig) => {
            check_config(&config);
            Ok(())
        }
        None => run(&config, cli.offline, cli.output, cli.top),
    }
}

/// Fetches all sources, indexes the emails, and generates the report.
///
/// A failed source keeps its emails of earlier runs, so that the report is
/// still useful, but the run fails at the end.
///
/// # Arguments
///
/// - `config`: The config.
/// - `offline`: Whether to skip the fetch.
/// - `output`: The directory for the report, or `None` for the default.
/// - `top`: The number of senders and recipients in each list of top
///   ones.
fn run(
    config: &Config,
    offline: bool,
    output: Option<PathBuf>,
    top: usize,
) -> anyhow::Result<()> {
    let fetched = if offline { Ok(()) } else { fetch_all(config) };
    let index = index(config)?;
    report(config, &index, output, top)?;
    fetched
}

/// Prints the data directory and the sources of the config.
///
/// # Arguments
///
/// - `config`: The config.
fn check_config(config: &Config) {
    println!("Data directory: {}", config.data_dir.display());
    println!("Sources:");
    for source in &config.sources {
        println!(
            "  {}: {} at {}:{}",
            source.name, source.username, source.host, source.port
        );
    }
}

/// Fetches all sources, also if one of them fails.
///
/// # Arguments
///
/// - `config`: The config.
fn fetch_all(config: &Config) -> anyhow::Result<()> {
    let mut failed = 0;
    for source in &config.sources {
        let name = &source.name;
        eprintln!("{name}: fetching from {}:{}", source.host, source.port);
        let progress = |message: &str| eprintln!("{name}: {message}");
        match fetch::fetch_source(source, &config.data_dir, progress) {
            Ok(summary) => eprintln!(
                "{name}: done, {} emails in {} folders, {} new",
                summary.emails, summary.folders, summary.fetched
            ),
            Err(error) => {
                eprintln!("{name}: error: {error:#}");
                if let Some(hint) = hint(&error) {
                    eprintln!("{name}: hint: {hint}");
                }
                failed += 1;
            }
        }
    }
    if failed > 0 {
        bail!("{failed} of {} sources failed", config.sources.len());
    }
    Ok(())
}

/// Builds the index of all fetched emails, and writes it.
///
/// # Arguments
///
/// - `config`: The config.
fn index(config: &Config) -> anyhow::Result<Index> {
    let mut sources = Vec::new();
    for source in &config.sources {
        let dir = config.data_dir.join(&source.name);
        let headers = store::load(&dir)?;
        if headers.is_empty() {
            eprintln!("{}: no fetched emails", source.name);
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
    let index = Index::build(&sources, &own_addresses, now()?.as_second());
    let path = config.data_dir.join(INDEX_FILE);
    index.write(&path)?;
    let copies: usize = sources.iter().map(|s| s.headers.len()).sum();
    let sent = index.emails.iter().filter(|e| e.sent).count();
    let undated = index.emails.iter().filter(|e| e.year.is_none()).count();
    eprintln!(
        "Indexed {} emails ({sent} sent) from {copies} copies, {undated} \
         without date, into {}",
        index.emails.len(),
        path.display()
    );
    Ok(index)
}

/// Generates the report.
///
/// # Arguments
///
/// - `config`: The config.
/// - `index`: The index of all emails.
/// - `output`: The directory for the report, or `None` for the default.
/// - `top`: The number of senders and recipients in each list of top
///   ones.
fn report(
    config: &Config,
    index: &Index,
    output: Option<PathBuf>,
    top: usize,
) -> anyhow::Result<()> {
    let output = output.unwrap_or_else(|| config.data_dir.join("report"));
    report::write(index, &config.sources, now()?, &output, top)?;
    eprintln!("Wrote {}", output.join(REPORT_FILE).display());
    // The PDF is a convenience, so its errors are only warnings.
    match pdf::convert(&output, REPORT_FILE) {
        Ok(()) => eprintln!("Wrote {}", output.join(PDF_FILE).display()),
        Err(error) => eprintln!("warning: {error:#}"),
    }
    Ok(())
}

/// The variable that overrides the current time, in seconds since 1970.
///
/// It is not `SOURCE_DATE_EPOCH`, which build environments set to the time
/// of a build, e.g. `nix develop` to 1980-01-01. As "now", that time would
/// make every later date implausible.
const NOW_VARIABLE: &str = "IMAP_STATISTICS_NOW";

/// Returns the current time.
///
/// [`NOW_VARIABLE`] overrides it, so that the demo report is the same every
/// time.
fn now() -> anyhow::Result<Timestamp> {
    let Ok(seconds) = env::var(NOW_VARIABLE) else {
        return Ok(Timestamp::now());
    };
    seconds
        .trim()
        .parse()
        .ok()
        .and_then(|seconds| Timestamp::from_second(seconds).ok())
        .with_context(|| format!("invalid {NOW_VARIABLE} {seconds:?}"))
}
