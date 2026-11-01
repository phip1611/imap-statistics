// SPDX-License-Identifier: MIT OR Apache-2.0

//! CLI to generate statistics about the emails in IMAP accounts.

use std::env;
use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use jiff::Timestamp;

use imap_statistics::config::Config;
use imap_statistics::run;

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
    let mut log = |line: &str| eprintln!("{line}");
    let outcome =
        run::run(config, offline, output.as_deref(), top, now()?, &mut log)?;
    if outcome.failed > 0 {
        bail!(
            "{} of {} sources failed",
            outcome.failed,
            config.sources.len()
        );
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
