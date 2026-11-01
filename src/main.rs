// SPDX-License-Identifier: MIT OR Apache-2.0

//! CLI to generate statistics about the emails in IMAP accounts.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};

use imap_statistics::config::Config;
use imap_statistics::fetch;
use imap_statistics::hint::hint;
use imap_statistics::index::{INDEX_FILE, Index};
use imap_statistics::report::{self, REPORT_FILE};
use imap_statistics::store;

/// Fetches the headers of all emails from IMAP accounts and generates a
/// Markdown report with statistics.
#[derive(Debug, Parser)]
#[command(version)]
struct Cli {
    /// Path of the config file.
    #[arg(short, long, global = true, default_value = "imap-statistics.toml")]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Checks the config file and lists the sources, without connecting to
    /// them.
    CheckConfig,
    /// Fetches the headers of all new emails of all sources into the data
    /// directory.
    Fetch,
    /// Builds the index of all fetched emails, without connecting to the
    /// sources.
    Index,
    /// Generates the Markdown report with charts from the index.
    Report {
        /// Directory for the report [default: "report" in the data
        /// directory].
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Number of senders in the list of top senders.
        #[arg(long, default_value_t = 50)]
        top: usize,
    },
}

/// Runs the command of the command line.
fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = Config::load(&cli.config)?;
    match cli.command {
        Command::CheckConfig => check_config(&config),
        Command::Fetch => fetch_all(&config)?,
        Command::Index => index(&config)?,
        Command::Report { output, top } => report(&config, output, top)?,
    }
    Ok(())
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

/// Builds the index of all fetched emails and writes it.
///
/// # Arguments
///
/// - `config`: The config.
fn index(config: &Config) -> anyhow::Result<()> {
    let mut sources = Vec::new();
    for source in &config.sources {
        let headers = store::load(&config.data_dir.join(&source.name))?;
        if headers.is_empty() {
            eprintln!("{}: no emails, run `fetch` first", source.name);
        }
        sources.push((source.name.clone(), headers));
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let index = Index::build(&sources, i64::try_from(now)?);
    let path = config.data_dir.join(INDEX_FILE);
    index.write(&path)?;
    let copies: usize = sources.iter().map(|(_, headers)| headers.len()).sum();
    let undated = index.emails.iter().filter(|e| e.year.is_none()).count();
    eprintln!(
        "Indexed {} emails from {copies} copies, {undated} without date, \
         into {}",
        index.emails.len(),
        path.display()
    );
    Ok(())
}

/// Generates the report from the index.
///
/// # Arguments
///
/// - `config`: The config.
/// - `output`: The directory for the report, or `None` for the default.
/// - `top`: The number of senders in the list of top senders.
fn report(
    config: &Config,
    output: Option<PathBuf>,
    top: usize,
) -> anyhow::Result<()> {
    let index = Index::read(&config.data_dir.join(INDEX_FILE))
        .context("no index, run `index` first")?;
    let output = output.unwrap_or_else(|| config.data_dir.join("report"));
    report::write(&index, &output, top)?;
    eprintln!("Wrote {}", output.join(REPORT_FILE).display());
    Ok(())
}
