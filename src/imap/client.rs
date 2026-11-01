// SPDX-License-Identifier: MIT OR Apache-2.0

//! Sends the read-only commands with the `imap` crate.

use std::error;
use std::fmt;
use std::io::{Read, Write};

use anyhow::{anyhow, bail};
use imap_proto::NameAttribute;

use crate::imap::read_only::{FETCH_ITEMS, ReadOnlyStream};
use crate::imap::{FetchedHeader, Folder, FolderStatus};

/// Format of `INTERNALDATE` (RFC 3501, section 9), as the store keeps it.
const INTERNALDATE_FORMAT: &str = "%d-%b-%Y %H:%M:%S %z";

/// The server refused a command, e.g. a login with a wrong password.
#[derive(Debug)]
pub struct CommandFailed {
    /// The name of the command, e.g. `LOGIN`.
    pub command: String,
    /// The status response of the server.
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

/// An IMAP connection after the greeting of the server, before the login.
#[derive(Debug)]
pub struct Client<S: Read + Write> {
    inner: imap::Client<ReadOnlyStream<S>>,
}

impl<S: Read + Write> Client<S> {
    /// Reads the greeting of the server.
    ///
    /// # Arguments
    ///
    /// - `stream`: The connection to the server, before its greeting.
    pub fn new(stream: S) -> anyhow::Result<Self> {
        let mut inner = imap::Client::new(ReadOnlyStream::new(stream));
        let greeting = inner
            .read_greeting()
            .map_err(|error| convert(error, "greeting"))?;
        if !greeting.starts_with(b"* OK") {
            let greeting = String::from_utf8_lossy(&greeting);
            bail!("unexpected greeting: {}", greeting.trim_end());
        }
        Ok(Self { inner })
    }

    /// Logs in with a username and password.
    ///
    /// # Arguments
    ///
    /// - `username`: The username.
    /// - `password`: The password.
    pub fn login(
        self,
        username: &str,
        password: &str,
    ) -> anyhow::Result<Session<S>> {
        let inner = self
            .inner
            .login(username, password)
            .map_err(|(error, _)| convert(error, "LOGIN"))?;
        Ok(Session { inner })
    }
}

/// An IMAP connection after the login.
#[derive(Debug)]
pub struct Session<S: Read + Write> {
    inner: imap::Session<ReadOnlyStream<S>>,
}

impl<S: Read + Write> Session<S> {
    /// Lists all folders.
    pub fn list(&mut self) -> anyhow::Result<Vec<Folder>> {
        let names = self
            .inner
            .list(Some(""), Some("*"))
            .map_err(|error| convert(error, "LIST"))?;
        Ok(names
            .iter()
            .map(|name| Folder {
                name: name.name().to_owned(),
                attributes: name.attributes().iter().map(attribute).collect(),
            })
            .collect())
    }

    /// Opens a folder read-only.
    ///
    /// # Arguments
    ///
    /// - `folder`: The name of the folder, as the server listed it.
    pub fn examine(&mut self, folder: &str) -> anyhow::Result<FolderStatus> {
        let mailbox = self
            .inner
            .examine(folder)
            .map_err(|error| convert(error, "EXAMINE"))?;
        Ok(FolderStatus {
            exists: mailbox.exists,
            uidvalidity: mailbox
                .uid_validity
                .ok_or_else(|| anyhow!("server sent no UIDVALIDITY"))?,
        })
    }

    /// Returns the UIDs of all emails in the open folder, in ascending
    /// order.
    pub fn search_uids(&mut self) -> anyhow::Result<Vec<u32>> {
        let uids = self
            .inner
            .uid_search("ALL")
            .map_err(|error| convert(error, "UID SEARCH"))?;
        let mut uids: Vec<_> = uids.into_iter().collect();
        uids.sort_unstable();
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
        let fetches = self
            .inner
            .uid_fetch(set.join(","), FETCH_ITEMS)
            .map_err(|error| convert(error, "UID FETCH"))?;
        for fetch in fetches.iter() {
            // Updates of other emails, e.g. of their flags, have no
            // header.
            let (Some(uid), Some(header)) = (fetch.uid, fetch.header()) else {
                continue;
            };
            on_header(FetchedHeader {
                uid,
                internaldate: fetch
                    .internal_date()
                    .map(|date| date.format(INTERNALDATE_FORMAT).to_string()),
                header: header.to_vec(),
            })?;
        }
        Ok(())
    }

    /// Logs out, after which the server closes the connection.
    pub fn logout(mut self) -> anyhow::Result<()> {
        self.inner
            .logout()
            .map_err(|error| convert(error, "LOGOUT"))
    }
}

/// Returns the name of a folder attribute in lowercase, e.g. `\noselect`.
///
/// # Arguments
///
/// - `attribute`: The attribute as the `imap` crate parsed it.
fn attribute(attribute: &NameAttribute<'_>) -> String {
    let name = match attribute {
        NameAttribute::NoInferiors => "\\noinferiors",
        NameAttribute::NoSelect => "\\noselect",
        NameAttribute::Marked => "\\marked",
        NameAttribute::Unmarked => "\\unmarked",
        NameAttribute::All => "\\all",
        NameAttribute::Archive => "\\archive",
        NameAttribute::Drafts => "\\drafts",
        NameAttribute::Flagged => "\\flagged",
        NameAttribute::Junk => "\\junk",
        NameAttribute::Sent => "\\sent",
        NameAttribute::Trash => "\\trash",
        NameAttribute::Extension(name) => return name.to_lowercase(),
        _ => "",
    };
    name.to_owned()
}

/// Converts an error of the `imap` crate into an error for the user.
///
/// Responses that the crate could not handle are left out, as they could
/// contain data of an email. I/O errors stay in the chain of causes, so
/// that hints can find them.
///
/// # Arguments
///
/// - `error`: The error of the `imap` crate.
/// - `command`: The name of the command, e.g. `LOGIN`.
fn convert(error: imap::Error, command: &str) -> anyhow::Error {
    let failed = |response: String| {
        anyhow::Error::new(CommandFailed {
            command: command.to_owned(),
            response,
        })
    };
    match error {
        imap::Error::Io(error) => {
            anyhow::Error::new(error).context(format!("{command} failed"))
        }
        imap::Error::No(no) => failed(format!("NO {}", no.information)),
        imap::Error::Bad(bad) => failed(format!("BAD {}", bad.information)),
        imap::Error::Bye(bye) => anyhow!(
            "server closed the connection during {command}: {}",
            bye.information
        ),
        imap::Error::Unexpected(_) | imap::Error::TagMismatch(_) => {
            anyhow!("unexpected response to {command}")
        }
        error => anyhow::Error::new(error).context(format!("{command} failed")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{FakeServer, fake_client, fake_session};

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
        let (client, sent) = fake_client(b"a1 OK LOGIN completed\r\n");
        client
            .login("me@example.com", r#"pa"ss\word"#)
            .expect("login should succeed");
        assert_eq!(
            sent.text(),
            "a1 LOGIN \"me@example.com\" \"pa\\\"ss\\\\word\"\r\n"
        );
    }

    #[test]
    fn reports_failed_login_without_the_password() {
        let (client, _) = fake_client(
            b"a1 NO [AUTHENTICATIONFAILED] Invalid credentials\r\n",
        );
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
    fn refuses_to_send_line_breaks_in_credentials() {
        let (client, sent) = fake_client(b"");
        client.login("me\r\n", "x").expect_err("login should fail");
        assert_eq!(sent.text(), "");
    }

    #[test]
    fn lists_folders() {
        let (mut session, sent) = fake_session(
            b"* LIST (\\HasNoChildren) \"/\" INBOX\r\n\
              * LIST (\\HasNoChildren \\Trash) \"/\" \"Deleted Items\"\r\n\
              * LIST (\\Noselect) NIL {4}\r\nWork\r\n\
              a2 OK LIST completed\r\n",
        );
        let folders = session.list().expect("LIST should succeed");
        assert_eq!(sent.text(), "a2 LIST \"\" *\r\n");
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
        let (mut session, sent) = fake_session(
            b"* FLAGS (\\Answered \\Seen)\r\n\
              * 172 EXISTS\r\n\
              * OK [UIDVALIDITY 3857529045] UIDs valid\r\n\
              * OK [UIDNEXT 4392] Predicted next UID\r\n\
              a2 OK [READ-ONLY] EXAMINE completed\r\n",
        );
        let status = session
            .examine("Sent Items")
            .expect("EXAMINE should succeed");
        assert_eq!(sent.text(), "a2 EXAMINE \"Sent Items\"\r\n");
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
        let (mut session, _) = fake_session(b"* 1 EXISTS\r\na2 OK done\r\n");
        let error = session.examine("INBOX").expect_err("EXAMINE should fail");
        assert_eq!(error.to_string(), "server sent no UIDVALIDITY");
    }

    #[test]
    fn searches_uids_in_ascending_order() {
        let (mut session, sent) = fake_session(
            b"* SEARCH 5 3\r\n* SEARCH 4\r\na2 OK SEARCH completed\r\n",
        );
        let uids = session.search_uids().expect("SEARCH should succeed");
        assert_eq!(sent.text(), "a2 UID SEARCH ALL\r\n");
        assert_eq!(uids, [3, 4, 5]);
    }

    #[test]
    fn fetches_headers() {
        // The second email has its items in another order and flags, the
        // third response is an unsolicited flag update without a header.
        let (mut session, sent) = fake_session(
            b"* 1 FETCH (UID 7 INTERNALDATE \"17-Jul-1996 02:44:25 -0700\" \
              BODY[HEADER] {8}\r\nA: b\r\n\r\n)\r\n\
              * 2 FETCH (BODY[HEADER] \"C: d\" FLAGS (\\Seen) UID 9)\r\n\
              * 3 FETCH (UID 10 FLAGS (\\Flagged))\r\n\
              a2 OK FETCH completed\r\n",
        );
        let mut headers = Vec::new();
        session
            .fetch_headers(&[7, 9], |header| {
                headers.push(header);
                Ok(())
            })
            .expect("FETCH should succeed");
        assert_eq!(
            sent.text(),
            "a2 UID FETCH 7,9 (UID INTERNALDATE BODY.PEEK[HEADER])\r\n"
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
        let (mut session, sent) = fake_session(b"");
        session
            .fetch_headers(&[], |_| panic!("no header expected"))
            .expect("FETCH should succeed");
        assert_eq!(sent.text(), "");
    }

    #[test]
    fn fails_when_the_server_closes_the_connection() {
        let (mut session, _) = fake_session(b"* BYE shutting down\r\n");
        let error = session.list().expect_err("LIST should fail");
        assert_eq!(format!("{error:#}"), "LIST failed: Connection Lost");
    }

    #[test]
    fn logs_out_with_bye() {
        let (session, _) =
            fake_session(b"* BYE logging out\r\na2 OK LOGOUT done\r\n");
        session.logout().expect("LOGOUT should succeed");
    }
}
