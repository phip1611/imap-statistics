// SPDX-License-Identifier: MIT OR Apache-2.0

//! Fetches the headers of all new emails of a source into its store.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

use anyhow::Context;

use crate::config::Source;
use crate::imap::{self, Session};
use crate::store::{FolderState, Store};

/// Headers per `UID FETCH`. Limits the length of the command, with up to
/// 11 bytes per UID, and sets how often the progress is reported.
const BATCH: usize = 500;

/// Counts of a fetch.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    /// Folders that were fetched.
    pub folders: usize,
    /// Emails in these folders.
    pub emails: usize,
    /// Emails whose header was new.
    pub fetched: usize,
}

/// Connects to a source and fetches its new headers into its store.
///
/// # Arguments
///
/// - `source`: The source from the config.
/// - `data_dir`: The data directory, which has the store of every source.
/// - `progress`: Receives messages for the user. They contain counts only,
///   no data of the mailbox.
pub fn fetch_source(
    source: &Source,
    data_dir: &Path,
    progress: impl FnMut(&str),
) -> anyhow::Result<Summary> {
    let mut store = Store::open(&data_dir.join(&source.name))?;
    let client = imap::connect(&source.host, source.port)?;
    let mut session =
        client.login(&source.username, source.password.expose())?;
    let summary = fetch(&mut session, &mut store, progress)?;
    session.logout()?;
    Ok(summary)
}

/// Fetches the new headers of all selected folders into the store.
///
/// Also records which emails each folder has, so that emails deleted on the
/// server drop out of the store.
///
/// # Arguments
///
/// - `session`: The session, after the login.
/// - `store`: The store of the source.
/// - `progress`: Receives messages for the user, with counts only.
pub fn fetch<S: Read + Write>(
    session: &mut Session<S>,
    store: &mut Store,
    mut progress: impl FnMut(&str),
) -> anyhow::Result<Summary> {
    let folders = imap::select_folders(session.list()?);
    let mut summary = Summary {
        folders: folders.len(),
        ..Summary::default()
    };
    let mut states = BTreeMap::new();
    for (index, folder) in folders.iter().enumerate() {
        let number = index + 1;
        let (state, fetched) =
            fetch_folder(session, store, &folder.name, |message| {
                progress(&format!(
                    "folder {number} of {}: {message}",
                    folders.len()
                ));
            })
            .with_context(|| {
                format!("failed to fetch folder {:?}", folder.name)
            })?;
        summary.emails += state.uids.len();
        summary.fetched += fetched;
        states.insert(folder.name.clone(), state);
    }
    store.save_folders(&states)?;
    Ok(summary)
}

/// Fetches the new headers of a folder.
///
/// Returns the emails of the folder and the number of fetched headers.
///
/// # Arguments
///
/// - `session`: The session, after the login.
/// - `store`: The store of the source.
/// - `folder`: The name of the folder, as the server listed it.
/// - `progress`: Receives messages for the user, with counts only.
fn fetch_folder<S: Read + Write>(
    session: &mut Session<S>,
    store: &mut Store,
    folder: &str,
    mut progress: impl FnMut(&str),
) -> anyhow::Result<(FolderState, usize)> {
    let status = session.examine(folder)?;
    // An empty folder has no UIDs to search, and some servers fail then.
    let uids = if status.exists == 0 {
        Vec::new()
    } else {
        session.search_uids()?
    };
    let missing = store.missing(folder, status.uidvalidity, &uids);
    progress(&format!("{} emails, {} new", uids.len(), missing.len()));
    let mut fetched = 0;
    for batch in missing.chunks(BATCH) {
        session.fetch_headers(batch, |header| {
            fetched += 1;
            store.append(folder, status.uidvalidity, header)
        })?;
        if missing.len() > BATCH {
            progress(&format!("fetched {fetched} of {}", missing.len()));
        }
    }
    let state = FolderState {
        uidvalidity: status.uidvalidity,
        uids,
    };
    Ok((state, fetched))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store;
    use crate::test_util::{TempDir, fake_session};

    /// Returns a FETCH response with a header.
    ///
    /// # Arguments
    ///
    /// - `sequence`: The sequence number of the email.
    /// - `uid`: The UID of the email, also its subject.
    fn fetched(sequence: u32, uid: u32) -> String {
        let header = format!("Subject: {uid}\r\n\r\n");
        let length = header.len();
        format!("* {sequence} FETCH (UID {uid} BODY[HEADER] {{{length}}}\r\n")
            + &header
            + ")\r\n"
    }

    /// Returns `(folder, uid)` of all headers in a store, sorted.
    ///
    /// # Arguments
    ///
    /// - `dir`: The directory of the store.
    fn uids(dir: &Path) -> Vec<(String, u32)> {
        let mut uids: Vec<_> = store::load(dir)
            .expect("store should load")
            .into_iter()
            .map(|h| (h.folder, h.uid))
            .collect();
        uids.sort();
        uids
    }

    #[test]
    fn fetches_new_headers_of_selected_folders() {
        // Boring setup: a server with INBOX, an empty folder, and the trash.
        let dir = TempDir::new("fetch-new");
        let mut store = Store::open(dir.path()).expect("store should open");
        let responses = [
            "* LIST () \"/\" INBOX\r\n".into(),
            "* LIST () \"/\" Empty\r\n".into(),
            "* LIST (\\Trash) \"/\" Trash\r\n".into(),
            "a2 OK done\r\n".into(),
            "* 2 EXISTS\r\n* OK [UIDVALIDITY 7] x\r\na3 OK done\r\n".into(),
            "* SEARCH 3 5\r\na4 OK done\r\n".into(),
            fetched(1, 3),
            fetched(2, 5),
            "a5 OK done\r\n".into(),
            "* 0 EXISTS\r\n* OK [UIDVALIDITY 1] x\r\na6 OK done\r\n".into(),
        ]
        .concat();
        let (mut session, sent) = fake_session(responses.as_bytes());
        let mut messages = Vec::new();

        let summary =
            fetch(&mut session, &mut store, |m| messages.push(m.to_owned()))
                .expect("fetch should succeed");

        // The trash is skipped, and the empty folder is not searched.
        assert_eq!(
            sent.text(),
            "a2 LIST \"\" *\r\n\
             a3 EXAMINE \"INBOX\"\r\n\
             a4 UID SEARCH ALL\r\n\
             a5 UID FETCH 3,5 (UID INTERNALDATE BODY.PEEK[HEADER])\r\n\
             a6 EXAMINE \"Empty\"\r\n"
        );
        assert_eq!(
            summary,
            Summary {
                folders: 2,
                emails: 2,
                fetched: 2
            }
        );
        assert_eq!(
            messages,
            [
                "folder 1 of 2: 2 emails, 2 new",
                "folder 2 of 2: 0 emails, 0 new"
            ]
        );
        assert_eq!(
            uids(dir.path()),
            [("INBOX".into(), 3), ("INBOX".into(), 5)]
        );
    }

    #[test]
    fn fetches_only_headers_that_are_not_stored() {
        let dir = TempDir::new("fetch-again");
        let mut store = Store::open(dir.path()).expect("store should open");
        let first = [
            "* LIST () \"/\" INBOX\r\na2 OK done\r\n".into(),
            "* 2 EXISTS\r\n* OK [UIDVALIDITY 7] x\r\na3 OK done\r\n".into(),
            "* SEARCH 1 2\r\na4 OK done\r\n".into(),
            fetched(1, 1),
            fetched(2, 2),
            "a5 OK done\r\n".into(),
        ]
        .concat();
        let (mut session, _) = fake_session(first.as_bytes());
        fetch(&mut session, &mut store, |_| {}).expect("fetch should succeed");

        // Email 1 was deleted on the server, email 3 is new.
        let second = [
            "* LIST () \"/\" INBOX\r\na2 OK done\r\n".into(),
            "* 2 EXISTS\r\n* OK [UIDVALIDITY 7] x\r\na3 OK done\r\n".into(),
            "* SEARCH 2 3\r\na4 OK done\r\n".into(),
            fetched(2, 3),
            "a5 OK done\r\n".into(),
        ]
        .concat();
        let (mut session, sent) = fake_session(second.as_bytes());
        let summary = fetch(&mut session, &mut store, |_| {})
            .expect("fetch should succeed");

        assert!(
            sent.text().ends_with(
                "a5 UID FETCH 3 (UID INTERNALDATE BODY.PEEK[HEADER])\r\n"
            ),
            "{}",
            sent.text()
        );
        assert_eq!(summary.fetched, 1);
        assert_eq!(
            uids(dir.path()),
            [("INBOX".into(), 2), ("INBOX".into(), 3)]
        );
    }

    #[test]
    fn names_the_folder_that_failed() {
        let dir = TempDir::new("fetch-error");
        let mut store = Store::open(dir.path()).expect("store should open");
        let responses =
            b"* LIST () \"/\" INBOX\r\na2 OK done\r\na3 NO gone\r\n";
        let (mut session, _) = fake_session(responses);
        let error = fetch(&mut session, &mut store, |_| {})
            .expect_err("fetch should fail");
        assert_eq!(
            format!("{error:#}"),
            "failed to fetch folder \"INBOX\": EXAMINE failed: NO gone"
        );
    }
}
