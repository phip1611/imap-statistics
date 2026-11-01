// SPDX-License-Identifier: MIT OR Apache-2.0

//! Helpers for unit tests.

use std::env;
use std::fs;
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process;

use crate::imap::Client;

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

/// An IMAP server that answers with prepared responses, and records what
/// the client sends.
#[derive(Debug)]
pub struct FakeServer {
    responses: Cursor<Vec<u8>>,
    sent: Vec<u8>,
}

impl FakeServer {
    /// Returns a server that answers with the given responses.
    ///
    /// # Arguments
    ///
    /// - `responses`: Everything the server sends, greeting included.
    pub fn new(responses: &[u8]) -> Self {
        Self {
            responses: Cursor::new(responses.to_vec()),
            sent: Vec::new(),
        }
    }
}

impl Read for FakeServer {
    /// Reads the next bytes of the prepared responses.
    ///
    /// # Arguments
    ///
    /// - `buf`: Receives the bytes.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.responses.read(buf)
    }
}

impl Write for FakeServer {
    /// Records bytes that the client sends.
    ///
    /// # Arguments
    ///
    /// - `buf`: The bytes.
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.sent.extend_from_slice(buf);
        Ok(buf.len())
    }

    /// Does nothing, as nothing is buffered.
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Returns a client whose server sends the greeting, then responses.
///
/// # Arguments
///
/// - `responses`: What the server sends after the greeting.
pub fn fake_client(responses: &[u8]) -> Client<FakeServer> {
    let mut all = b"* OK IMAP4rev1 ready\r\n".to_vec();
    all.extend_from_slice(responses);
    Client::new(FakeServer::new(&all)).expect("greeting should be valid")
}

/// Returns what the client sent to the server.
///
/// # Arguments
///
/// - `client`: The client.
pub fn sent(client: &Client<FakeServer>) -> String {
    String::from_utf8(client.stream().sent.clone())
        .expect("commands should be UTF-8")
}
