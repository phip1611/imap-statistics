// SPDX-License-Identifier: MIT OR Apache-2.0

//! Helpers for unit tests.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

/// A new, empty directory, which is removed again on drop.
#[derive(Debug)]
pub struct TempDir(PathBuf);

impl TempDir {
    /// Creates a new, empty directory in the temporary directory of the OS.
    ///
    /// # Arguments
    ///
    /// - `name`: A name that is unique among all tests, which run in parallel.
    pub fn new(name: &str) -> Self {
        let path = env::temp_dir()
            .join(format!("imap-statistics-{}-{name}", process::id()));
        // Left over if the same process ID ran the tests before.
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path)
            .expect("temporary directory should be creatable");
        Self(path)
    }

    /// Returns the path of the directory.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    /// Removes the directory with all its content.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
