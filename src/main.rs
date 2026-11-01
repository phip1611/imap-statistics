// SPDX-License-Identifier: MIT OR Apache-2.0

//! CLI to generate statistics about the emails in IMAP accounts.

use std::path::PathBuf;

use anyhow::bail;
use clap::{Parser, Subcommand};

use imap_statistics::config::Config;
use imap_statistics::fetch;

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
}

/// Runs the command of the command line.
fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = Config::load(&cli.config)?;
    match cli.command {
        Command::CheckConfig => check_config(&config),
        Command::Fetch => fetch_all(&config)?,
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
                failed += 1;
            }
        }
    }
    if failed > 0 {
        bail!("{failed} of {} sources failed", config.sources.len());
    }
    Ok(())
}
