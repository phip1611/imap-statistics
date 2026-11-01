// SPDX-License-Identifier: MIT OR Apache-2.0

//! Helpers for unit tests.

use std::cell::RefCell;
use std::env;
use std::fs;
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::rc::Rc;

use crate::imap::{Client, Session};

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

/// What a fake server received from the client, shared with the test.
#[derive(Debug, Clone, Default)]
pub struct Sent(Rc<RefCell<Vec<u8>>>);

impl Sent {
    /// Returns what the client sent, as text.
    pub fn text(&self) -> String {
        String::from_utf8(self.0.borrow().clone())
            .expect("commands should be UTF-8")
    }
}

/// An IMAP server that answers with prepared responses, and records what
/// the client sends.
#[derive(Debug)]
pub struct FakeServer {
    responses: Cursor<Vec<u8>>,
    sent: Sent,
}

impl FakeServer {
    /// Returns a server, and what the client sends to it.
    ///
    /// # Arguments
    ///
    /// - `responses`: Everything the server sends, greeting included.
    pub fn new(responses: &[u8]) -> (Self, Sent) {
        let sent = Sent::default();
        let server = Self {
            responses: Cursor::new(responses.to_vec()),
            sent: sent.clone(),
        };
        (server, sent)
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
        self.sent.0.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }

    /// Does nothing, as nothing is buffered.
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Returns a logged-in session whose server sends the given responses.
///
/// Also returns what the client sends to the server after the login. The
/// first command after the login has the tag `a2`.
///
/// # Arguments
///
/// - `responses`: What the server sends after the login.
pub fn fake_session(responses: &[u8]) -> (Session<FakeServer>, Sent) {
    let mut all = b"a1 OK LOGIN completed\r\n".to_vec();
    all.extend_from_slice(responses);
    let (client, sent) = fake_client(&all);
    let session = client.login("me", "secret").expect("login should succeed");
    sent.0.borrow_mut().clear();
    (session, sent)
}

/// Returns a client whose server sends the greeting, then responses.
///
/// Also returns what the client sends to the server.
///
/// # Arguments
///
/// - `responses`: What the server sends after the greeting.
pub fn fake_client(responses: &[u8]) -> (Client<FakeServer>, Sent) {
    let mut all = b"* OK IMAP4rev1 ready\r\n".to_vec();
    all.extend_from_slice(responses);
    let (server, sent) = FakeServer::new(&all);
    let client = Client::new(server).expect("greeting should be valid");
    (client, sent)
}
