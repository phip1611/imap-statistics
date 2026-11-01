// SPDX-License-Identifier: MIT OR Apache-2.0

//! Sends the read-only commands and collects their responses.

use std::error;
use std::fmt;
use std::io::{BufReader, Read, Write};

use anyhow::{Context, bail};

use crate::imap::parse::{
    describe, is_status, is_untagged, parse_exists, parse_fetch, parse_list,
    parse_search, parse_uidvalidity,
};
use crate::imap::read_only::{FETCH_ITEMS, check_read_only, command_name};
use crate::imap::response::{Token, read_response};
use crate::imap::{FetchedHeader, Folder, FolderStatus};

/// The server refused a command, e.g. a login with a wrong password.
#[derive(Debug)]
pub struct CommandFailed {
    /// The name of the command, e.g. `LOGIN`.
    pub command: String,
    /// The status response of the server, without literals.
    pub response: String,
}

impl fmt::Display for CommandFailed {
    /// Writes the command and the response of the server.
    ///
    /// # Arguments
    ///
    /// - `f`: The formatter.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} failed: {}", self.command, self.response)
    }
}

impl error::Error for CommandFailed {}

/// An IMAP connection after the greeting of the server.
#[derive(Debug)]
pub struct Client<S: Read + Write> {
    stream: BufReader<S>,
    next_tag: u32,
}

impl<S: Read + Write> Client<S> {
    /// Reads the greeting of the server.
    ///
    /// # Arguments
    ///
    /// - `stream`: The connection to the server, before its greeting.
    pub fn new(stream: S) -> anyhow::Result<Self> {
        let mut stream = BufReader::new(stream);
        let greeting = read_response(&mut stream)
            .context("failed to read the greeting of the server")?;
        if !is_untagged(&greeting, "OK") {
            bail!("unexpected greeting: {}", describe(&greeting));
        }
        Ok(Self {
            stream,
            next_tag: 1,
        })
    }

    /// Logs in with a username and password.
    ///
    /// # Arguments
    ///
    /// - `username`: The username, printable ASCII only.
    /// - `password`: The password, printable ASCII only.
    pub fn login(
        &mut self,
        username: &str,
        password: &str,
    ) -> anyhow::Result<()> {
        let command = format!(
            "LOGIN {} {}",
            quote(username).context("unsupported username")?,
            quote(password).context("unsupported password")?
        );
        self.run(&command, |_| Ok(()))
    }

    /// Lists all folders.
    pub fn list(&mut self) -> anyhow::Result<Vec<Folder>> {
        let mut folders = Vec::new();
        self.run(r#"LIST "" "*""#, |tokens| {
            if is_untagged(&tokens, "LIST") {
                folders.push(parse_list(tokens)?);
            }
            Ok(())
        })?;
        Ok(folders)
    }

    /// Opens a folder read-only.
    ///
    /// # Arguments
    ///
    /// - `folder`: The name of the folder, as the server listed it.
    pub fn examine(&mut self, folder: &str) -> anyhow::Result<FolderStatus> {
        let mut exists = None;
        let mut uidvalidity = None;
        let command = format!("EXAMINE {}", quote(folder)?);
        self.run(&command, |tokens| {
            exists = exists.or(parse_exists(&tokens)?);
            uidvalidity = uidvalidity.or(parse_uidvalidity(&tokens)?);
            Ok(())
        })?;
        Ok(FolderStatus {
            exists: exists.context("server sent no EXISTS")?,
            uidvalidity: uidvalidity.context("server sent no UIDVALIDITY")?,
        })
    }

    /// Returns the UIDs of all emails in the open folder, in ascending
    /// order.
    pub fn search_uids(&mut self) -> anyhow::Result<Vec<u32>> {
        let mut uids = Vec::new();
        self.run("UID SEARCH ALL", |tokens| {
            if is_untagged(&tokens, "SEARCH") {
                uids.extend(parse_search(&tokens)?);
            }
            Ok(())
        })?;
        uids.sort_unstable();
        uids.dedup();
        Ok(uids)
    }

    /// Fetches the headers of emails in the open folder, by UID.
    ///
    /// Emails that vanished from the server in the meantime are skipped.
    ///
    /// # Arguments
    ///
    /// - `uids`: The UIDs of the emails.
    /// - `on_header`: Receives each header, in the order of the server.
    pub fn fetch_headers(
        &mut self,
        uids: &[u32],
        mut on_header: impl FnMut(FetchedHeader) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        let set: Vec<_> = uids.iter().map(u32::to_string).collect();
        let command = format!("UID FETCH {} {FETCH_ITEMS}", set.join(","));
        self.run(&command, |tokens| {
            let is_fetch = matches!(
                tokens.get(2),
                Some(Token::Atom(kind)) if kind.eq_ignore_ascii_case("FETCH")
            );
            if tokens.first() == Some(&Token::Atom("*".into())) && is_fetch {
                // Updates of other emails, e.g. of their flags, have no
                // header.
                if let Some(header) = parse_fetch(tokens)? {
                    on_header(header)?;
                }
            }
            Ok(())
        })
    }

    /// Logs out, after which the server closes the connection.
    pub fn logout(mut self) -> anyhow::Result<()> {
        self.run("LOGOUT", |_| Ok(()))
    }

    /// Sends a command and passes on its untagged responses, until the
    /// server completes the command.
    ///
    /// # Arguments
    ///
    /// - `command`: The command, without tag and line ending.
    /// - `on_untagged`: Receives each untagged response.
    fn run(
        &mut self,
        command: &str,
        mut on_untagged: impl FnMut(Vec<Token>) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        check_read_only(command)?;
        // Never the full command, which may contain the password.
        let name = command_name(command);
        let tag = format!("A{}", self.next_tag);
        self.next_tag += 1;
        let stream = self.stream.get_mut();
        stream
            .write_all(format!("{tag} {command}\r\n").as_bytes())
            .and_then(|()| stream.flush())
            .with_context(|| format!("failed to send {name}"))?;
        loop {
            let tokens =
                read_response(&mut self.stream).with_context(|| {
                    format!("failed to read the response to {name}")
                })?;
            match tokens.first() {
                Some(Token::Atom(first)) if *first == tag => {
                    if is_status(&tokens[1..], "OK") {
                        return Ok(());
                    }
                    return Err(CommandFailed {
                        command: name.to_owned(),
                        response: describe(&tokens[1..]),
                    }
                    .into());
                }
                Some(Token::Atom(first)) if first == "*" => {
                    if name != "LOGOUT" && is_untagged(&tokens, "BYE") {
                        bail!(
                            "server closed the connection during {name}: {}",
                            describe(&tokens[2..])
                        );
                    }
                    on_untagged(tokens)?;
                }
                _ => bail!("unexpected response to {name}"),
            }
        }
    }
}

/// Quotes a string for a command (RFC 3501, section 4.3).
///
/// Only printable ASCII is supported. That covers all folder names, as
/// servers send them in modified UTF-7, and nearly all usernames and
/// passwords.
///
/// # Arguments
///
/// - `text`: The string to quote.
fn quote(text: &str) -> anyhow::Result<String> {
    if !text.bytes().all(|b| b.is_ascii() && !b.is_ascii_control()) {
        bail!("only printable ASCII characters are supported");
    }
    let escaped = text.replace('\\', r"\\").replace('"', r#"\""#);
    Ok(format!("\"{escaped}\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{FakeServer, fake_client as client};

    /// Returns a folder.
    ///
    /// # Arguments
    ///
    /// - `name`: The name of the folder.
    /// - `attributes`: Its attributes, in lowercase.
    fn folder(name: &str, attributes: &[&str]) -> Folder {
        Folder {
            name: name.into(),
            attributes: attributes.iter().map(|&a| a.into()).collect(),
        }
    }

    #[test]
    fn rejects_greeting_other_than_ok() {
        let (server, _) = FakeServer::new(b"* BYE too busy\r\n");
        let error = Client::new(server).expect_err("greeting should fail");
        assert_eq!(error.to_string(), "unexpected greeting: * BYE too busy");
    }

    #[test]
    fn logs_in_with_quoted_credentials() {
        let (mut client, sent) = client(b"A1 OK LOGIN completed\r\n");
        client
            .login("me@example.com", r#"pa"ss\word"#)
            .expect("login should succeed");
        assert_eq!(
            sent.text(),
            "A1 LOGIN \"me@example.com\" \"pa\\\"ss\\\\word\"\r\n"
        );
    }

    #[test]
    fn reports_failed_login_without_the_password() {
        let (mut client, _) =
            client(b"A1 NO [AUTHENTICATIONFAILED] Invalid credentials\r\n");
        let error = client
            .login("me@example.com", "secret")
            .expect_err("login should fail");
        assert_eq!(
            format!("{error:#}"),
            "LOGIN failed: NO [AUTHENTICATIONFAILED] Invalid credentials"
        );
        let failed = error
            .downcast_ref::<CommandFailed>()
            .expect("error should be a refused command");
        assert_eq!(failed.command, "LOGIN");
    }

    #[test]
    fn refuses_to_send_unsupported_credentials() {
        let (mut client, sent) = client(b"");
        for (username, password) in [("me", "pässword"), ("me\r\n", "x")] {
            client
                .login(username, password)
                .expect_err("login should fail");
        }
        assert_eq!(sent.text(), "");
    }

    #[test]
    fn lists_folders() {
        let (mut client, sent) = client(
            b"* LIST (\\HasNoChildren) \"/\" INBOX\r\n\
              * LIST (\\HasNoChildren \\Trash) \"/\" \"Deleted Items\"\r\n\
              * LIST (\\Noselect) NIL {4}\r\nWork\r\n\
              A1 OK LIST completed\r\n",
        );
        let folders = client.list().expect("LIST should succeed");
        assert_eq!(sent.text(), "A1 LIST \"\" \"*\"\r\n");
        assert_eq!(
            folders,
            [
                folder("INBOX", &["\\hasnochildren"]),
                folder("Deleted Items", &["\\hasnochildren", "\\trash"]),
                folder("Work", &["\\noselect"]),
            ]
        );
    }

    #[test]
    fn examines_folder() {
        let (mut client, sent) = client(
            b"* FLAGS (\\Answered \\Seen)\r\n\
              * 172 EXISTS\r\n\
              * OK [UIDVALIDITY 3857529045] UIDs valid\r\n\
              * OK [UIDNEXT 4392] Predicted next UID\r\n\
              A1 OK [READ-ONLY] EXAMINE completed\r\n",
        );
        let status = client
            .examine("Sent Items")
            .expect("EXAMINE should succeed");
        assert_eq!(sent.text(), "A1 EXAMINE \"Sent Items\"\r\n");
        assert_eq!(
            status,
            FolderStatus {
                exists: 172,
                uidvalidity: 3_857_529_045
            }
        );
    }

    #[test]
    fn rejects_examine_without_uidvalidity() {
        let (mut client, _) = client(b"* 1 EXISTS\r\nA1 OK done\r\n");
        let error = client.examine("INBOX").expect_err("EXAMINE should fail");
        assert_eq!(error.to_string(), "server sent no UIDVALIDITY");
    }

    #[test]
    fn searches_uids_in_ascending_order() {
        let (mut client, sent) = client(
            b"* SEARCH 5 3\r\n* SEARCH 4 3\r\nA1 OK SEARCH completed\r\n",
        );
        let uids = client.search_uids().expect("SEARCH should succeed");
        assert_eq!(sent.text(), "A1 UID SEARCH ALL\r\n");
        assert_eq!(uids, [3, 4, 5]);
    }

    #[test]
    fn fetches_headers() {
        // The second email has its items in another order and flags, the
        // third response is an unsolicited flag update without a header.
        let (mut client, sent) = client(
            b"* 1 FETCH (UID 7 INTERNALDATE \"17-Jul-1996 02:44:25 -0700\" \
              BODY[HEADER] {8}\r\nA: b\r\n\r\n)\r\n\
              * 2 FETCH (BODY[HEADER] \"C: d\" FLAGS (\\Seen (x)) UID 9)\r\n\
              * 3 FETCH (UID 10 FLAGS (\\Flagged))\r\n\
              A1 OK FETCH completed\r\n",
        );
        let mut headers = Vec::new();
        client
            .fetch_headers(&[7, 9], |header| {
                headers.push(header);
                Ok(())
            })
            .expect("FETCH should succeed");
        assert_eq!(
            sent.text(),
            "A1 UID FETCH 7,9 (UID INTERNALDATE BODY.PEEK[HEADER])\r\n"
        );
        assert_eq!(
            headers,
            [
                FetchedHeader {
                    uid: 7,
                    internaldate: Some("17-Jul-1996 02:44:25 -0700".into()),
                    header: b"A: b\r\n\r\n".to_vec(),
                },
                FetchedHeader {
                    uid: 9,
                    internaldate: None,
                    header: b"C: d".to_vec(),
                },
            ]
        );
    }

    #[test]
    fn fetches_nothing_without_uids() {
        let (mut client, sent) = client(b"");
        client
            .fetch_headers(&[], |_| panic!("no header expected"))
            .expect("FETCH should succeed");
        assert_eq!(sent.text(), "");
    }

    #[test]
    fn rejects_bye_during_command() {
        let (mut client, _) = client(b"* BYE shutting down\r\n");
        let error = client.list().expect_err("LIST should fail");
        assert_eq!(
            error.to_string(),
            "server closed the connection during LIST: shutting down"
        );
    }

    #[test]
    fn logs_out_with_bye() {
        let (client, _sent) =
            client(b"* BYE logging out\r\nA1 OK LOGOUT done\r\n");
        client.logout().expect("LOGOUT should succeed");
    }

    #[test]
    fn increments_tags() {
        let (mut client, sent) = client(b"A1 OK done\r\nA2 OK done\r\n");
        client.login("a", "b").expect("login should succeed");
        client.list().expect("LIST should succeed");
        assert!(sent.text().contains("\r\nA2 LIST"), "{}", sent.text());
    }
}
