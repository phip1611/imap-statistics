// SPDX-License-Identifier: MIT OR Apache-2.0

//! The allowlist of the commands that cannot change a mailbox, and the
//! stream that enforces it.

use std::io::{self, ErrorKind, Read, Write};
use std::str;

use anyhow::bail;

/// What `UID FETCH` fetches of every email. `BODY.PEEK[HEADER]`, unlike
/// `BODY[HEADER]`, does not set `\Seen`.
pub(super) const FETCH_ITEMS: &str = "(UID INTERNALDATE BODY.PEEK[HEADER])";

/// Refuses every command that could change a mailbox.
///
/// All commands of [`Client`](super::Client) pass. This is a second line
/// of defense against bugs, e.g. a fetch item that sets `\Seen`.
///
/// # Arguments
///
/// - `command`: The command, without tag and line ending.
pub fn check_read_only(command: &str) -> anyhow::Result<()> {
    let name = command_name(command);
    let allowed = !command.contains(['\r', '\n'])
        && match name {
            "LOGIN" | "LIST" | "EXAMINE" | "LOGOUT" => true,
            "UID SEARCH" => command == "UID SEARCH ALL",
            "UID FETCH" => command
                .strip_prefix("UID FETCH ")
                .and_then(|rest| rest.strip_suffix(FETCH_ITEMS))
                .and_then(|set| set.strip_suffix(' '))
                .is_some_and(|set| {
                    !set.is_empty()
                        && set.bytes().all(|b| b.is_ascii_digit() || b == b',')
                }),
            _ => false,
        };
    if !allowed {
        bail!("refusing IMAP command {name}, it could change a mailbox");
    }
    Ok(())
}

/// A stream that refuses to send commands that could change a mailbox.
///
/// It checks every command line with [`check_read_only`] before it reaches
/// the server, so the guard holds whoever builds the commands. A refused
/// command fails the write with [`ErrorKind::PermissionDenied`].
#[derive(Debug)]
pub struct ReadOnlyStream<S> {
    inner: S,
    /// The start of a command line that is not complete yet.
    pending: Vec<u8>,
}

impl<S> ReadOnlyStream<S> {
    /// Wraps a stream to a server.
    ///
    /// # Arguments
    ///
    /// - `inner`: The stream to the server.
    pub const fn new(inner: S) -> Self {
        Self {
            inner,
            pending: Vec::new(),
        }
    }
}

impl<S: Read> Read for ReadOnlyStream<S> {
    /// Reads from the server, unchecked.
    ///
    /// # Arguments
    ///
    /// - `buf`: Receives the bytes.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl<S: Write> Write for ReadOnlyStream<S> {
    /// Collects bytes and sends each complete command line once checked.
    ///
    /// # Arguments
    ///
    /// - `buf`: The bytes, any part of one or more command lines.
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(buf);
        while let Some(end) =
            self.pending.windows(2).position(|pair| pair == b"\r\n")
        {
            let line: Vec<u8> = self.pending.drain(..end + 2).collect();
            check_line(&line[..end])?;
            self.inner.write_all(&line)?;
        }
        Ok(buf.len())
    }

    /// Flushes the server stream. An incomplete line stays pending.
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Checks a command line, with its tag but without its line ending.
///
/// # Arguments
///
/// - `line`: The command line, e.g. `A1 LOGOUT`.
fn check_line(line: &[u8]) -> io::Result<()> {
    let refuse =
        |message: String| io::Error::new(ErrorKind::PermissionDenied, message);
    let line = str::from_utf8(line).map_err(|_| {
        refuse("refusing an IMAP command that is no UTF-8".into())
    })?;
    let command = line.split_once(' ').map_or("", |(_tag, command)| command);
    check_read_only(command).map_err(|error| refuse(error.to_string()))
}

/// Returns the name of a command, e.g. `LOGIN` or `UID FETCH`.
///
/// # Arguments
///
/// - `command`: The command, without tag and line ending.
pub(super) fn command_name(command: &str) -> &str {
    let words = if command.starts_with("UID ") { 2 } else { 1 };
    let end = command
        .match_indices(' ')
        .nth(words - 1)
        .map_or(command.len(), |(index, _)| index);
    &command[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_the_commands_of_the_client() {
        for command in [
            r#"LOGIN "a" "b""#,
            r#"LIST "" "*""#,
            r#"EXAMINE "INBOX""#,
            "UID SEARCH ALL",
            "UID FETCH 1,2 (UID INTERNALDATE BODY.PEEK[HEADER])",
            "LOGOUT",
        ] {
            check_read_only(command).expect(command);
        }
    }

    #[test]
    fn refuses_commands_that_could_change_a_mailbox() {
        for command in [
            r#"SELECT "INBOX""#,
            r"STORE 1 +FLAGS (\Deleted)",
            r"UID STORE 1 +FLAGS (\Seen)",
            "UID FETCH 1 (BODY[HEADER])",
            "UID FETCH 1 BODY[] (UID INTERNALDATE BODY.PEEK[HEADER])",
            "UID FETCH 1 (RFC822)",
            r#"UID COPY 1 "Trash""#,
            r#"UID MOVE 1 "Trash""#,
            "EXPUNGE",
            "UID SEARCH UNSEEN",
            "LOGIN \"a\" \"b\"\r\nA2 EXPUNGE",
        ] {
            let error = check_read_only(command)
                .expect_err("command should be refused");
            assert!(
                error.to_string().starts_with("refusing IMAP command"),
                "{command}: {error}"
            );
        }
    }

    #[test]
    fn sends_allowed_commands_written_in_parts() {
        let mut stream = ReadOnlyStream::new(Vec::new());
        stream.write_all(b"A1 LOG").expect("write should work");
        assert!(stream.inner.is_empty());
        stream
            .write_all(b"OUT\r\nA2 LIST \"\" \"*\"\r\nA3")
            .expect("write should work");
        assert_eq!(stream.inner, b"A1 LOGOUT\r\nA2 LIST \"\" \"*\"\r\n");
    }

    #[test]
    fn refuses_to_send_commands_that_could_change_a_mailbox() {
        for line in [
            &b"A1 STORE 1 +FLAGS (\\Deleted)\r\n"[..],
            b"A1 SELECT \"INBOX\"\r\n",
            b"LOGOUT\r\n",
        ] {
            let mut stream = ReadOnlyStream::new(Vec::new());
            let error = stream.write_all(line).expect_err("write should fail");
            assert_eq!(error.kind(), ErrorKind::PermissionDenied);
            assert!(stream.inner.is_empty(), "{line:?}");
        }
    }

    #[test]
    fn names_commands() {
        assert_eq!(command_name("LOGIN \"a\" \"b\""), "LOGIN");
        assert_eq!(command_name("UID FETCH 1 (UID)"), "UID FETCH");
        assert_eq!(command_name("LOGOUT"), "LOGOUT");
    }
}
