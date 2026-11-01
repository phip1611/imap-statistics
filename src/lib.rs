// SPDX-License-Identifier: MIT OR Apache-2.0

//! Library of the `imap-statistics` CLI.
//!
//! It exists so that every module can be built and tested on its own. It
//! is no stable API for other crates.

pub mod category;
pub mod config;
pub mod email;
pub mod fetch;
#[cfg(feature = "gui")]
pub mod gui;
pub mod hint;
pub mod imap;
pub mod index;
pub mod pdf;
pub mod report;
pub mod run;
pub mod store;
pub mod svg;
#[cfg(test)]
mod test_util;
