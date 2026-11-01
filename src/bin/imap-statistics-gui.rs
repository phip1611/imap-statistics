// SPDX-License-Identifier: MIT OR Apache-2.0

//! The window of imap-statistics, for people who do not use a terminal.

// Without a console window on Windows, which a double-click would open.
#![windows_subsystem = "windows"]

use std::env;
use std::path::PathBuf;

use imap_statistics::gui;

/// Opens the window, with the config file of the first argument, or
/// `imap-statistics.toml` in the current directory, as the CLI does.
fn main() -> anyhow::Result<()> {
    let config = env::args_os()
        .nth(1)
        .map_or_else(|| PathBuf::from("imap-statistics.toml"), PathBuf::from);
    gui::run(config)
}
