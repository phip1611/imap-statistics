// SPDX-License-Identifier: MIT OR Apache-2.0

//! CLI to generate statistics about the emails in IMAP accounts.

mod config;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::config::Config;

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
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = Config::load(&cli.config)?;
    match cli.command {
        Command::CheckConfig => check_config(&config),
    }
    Ok(())
}

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
