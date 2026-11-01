// SPDX-License-Identifier: MIT OR Apache-2.0

//! CLI to generate statistics about the emails in IMAP accounts.

use clap::Parser;

/// Fetches the headers of all emails from IMAP accounts and generates a
/// Markdown report with statistics.
#[derive(Debug, Parser)]
#[command(version)]
struct Cli {}

fn main() {
    Cli::parse();
}
