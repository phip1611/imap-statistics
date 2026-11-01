// SPDX-License-Identifier: MIT OR Apache-2.0

//! Fetches the headers of all new emails of a source into its store.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Context;

use crate::config::Source;
use crate::imap::{self, Client};
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
    mut progress: impl FnMut(&str),
) -> anyhow::Result<Summary> {
    progress("reading the stored headers");
    let mut store = Store::open(&data_dir.join(&source.name))?;
    progress("connecting");
    let mut client = imap::connect(&source.host, source.port)?;
    progress("logging in");
    client.login(&source.username, source.password.expose())?;
    let summary = fetch(&mut client, &mut store, progress)?;
    client.logout()?;
    Ok(summary)
}

/// Fetches the new headers of all selected folders into the store.
///
/// Also records which emails each folder has, so that emails deleted on the
/// server drop out of the store.
///
/// # Arguments
///
/// - `client`: The client, logged in.
/// - `store`: The store of the source.
/// - `progress`: Receives messages for the user, with counts only.
pub fn fetch<S: Read + Write>(
    client: &mut Client<S>,
    store: &mut Store,
    mut progress: impl FnMut(&str),
) -> anyhow::Result<Summary> {
    progress("listing the folders");
    let folders = imap::select_folders(client.list()?);
    progress(&format!("{} folders to fetch", folders.len()));
    let mut summary = Summary {
        folders: folders.len(),
        ..Summary::default()
    };
    let mut states = BTreeMap::new();
    for (index, folder) in folders.iter().enumerate() {
        let number = index + 1;
        let (state, fetched) =
            fetch_folder(client, store, &folder.name, |message| {
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
        let sent = folder.attributes.iter().any(|a| a == "\\sent");
        states.insert(folder.name.clone(), FolderState { sent, ..state });
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
/// - `client`: The client, logged in.
/// - `store`: The store of the source.
/// - `folder`: The name of the folder, as the server listed it.
/// - `progress`: Receives messages for the user, with counts only.
fn fetch_folder<S: Read + Write>(
    client: &mut Client<S>,
    store: &mut Store,
    folder: &str,
    mut progress: impl FnMut(&str),
) -> anyhow::Result<(FolderState, usize)> {
    let status = client.examine(folder)?;
    // An empty folder has no UIDs to search, and some servers fail then.
    let uids = if status.exists == 0 {
        Vec::new()
    } else {
        // Takes a while for large folders, e.g. "All Mail" of Gmail.
        progress(&format!("searching {} emails", status.exists));
        client.search_uids()?
    };
    let missing = store.missing(folder, status.uidvalidity, &uids);
    progress(&format!("{} emails, {} new", uids.len(), missing.len()));
    let mut fetched = 0;
    let started = Instant::now();
    for batch in missing.chunks(BATCH) {
        client.fetch_headers(batch, |header| {
            fetched += 1;
            store.append(folder, status.uidvalidity, header)
        })?;
        if missing.len() > BATCH {
            progress(&batch_progress(
                started.elapsed(),
                fetched,
                missing.len(),
            ));
        }
    }
    let state = FolderState {
        uidvalidity: status.uidvalidity,
        uids,
        sent: false,
    };
    Ok((state, fetched))
}

/// Describes the progress of a folder, e.g. `fetched 500 of 2000 (25%),
/// about 3 min left`.
///
/// The time left assumes that the rest goes as fast as what was fetched.
///
/// # Arguments
///
/// - `elapsed`: The time since the first batch of the folder started.
/// - `done`: The number of fetched headers.
/// - `total`: The number of headers to fetch.
fn batch_progress(elapsed: Duration, done: usize, total: usize) -> String {
    let mut message =
        format!("fetched {done} of {total} ({}%)", done * 100 / total.max(1));
    if done > 0 && done < total {
        let left = elapsed.as_secs_f64() * (total - done) as f64 / done as f64;
        let seconds = left.round() as u64;
        let left = match seconds {
            0..60 => format!("{seconds} s"),
            60..3600 => format!("{} min", seconds / 60),
            _ => format!("{} h {} min", seconds / 3600, seconds % 3600 / 60),
        };
        message.push_str(&format!(", about {left} left"));
    }
    message
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::store;
    use crate::test_util::{TempDir, fake_client, sent};

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
        // Boring setup: a server with INBOX, an empty Sent folder, and the
        // trash.
        let dir = TempDir::new("fetch-new");
        let mut store = Store::open(dir.path()).expect("store should open");
        let responses = [
            "* LIST () \"/\" INBOX\r\n".into(),
            "* LIST (\\Sent) \"/\" Sent\r\n".into(),
            "* LIST (\\Trash) \"/\" Trash\r\n".into(),
            "A1 OK\r\n".into(),
            "* 2 EXISTS\r\n* OK [UIDVALIDITY 7] x\r\nA2 OK\r\n".into(),
            "* SEARCH 3 5\r\nA3 OK\r\n".into(),
            fetched(1, 3),
            fetched(2, 5),
            "A4 OK\r\n".into(),
            "* 0 EXISTS\r\n* OK [UIDVALIDITY 1] x\r\nA5 OK\r\n".into(),
        ]
        .concat();
        let mut client = fake_client(responses.as_bytes());
        let mut messages = Vec::new();

        let summary =
            fetch(&mut client, &mut store, |m| messages.push(m.to_owned()))
                .expect("fetch should succeed");

        // The trash is skipped, and the empty folder is not searched.
        assert_eq!(
            sent(&client),
            "A1 LIST \"\" \"*\"\r\n\
             A2 EXAMINE \"INBOX\"\r\n\
             A3 UID SEARCH ALL\r\n\
             A4 UID FETCH 3,5 (UID INTERNALDATE BODY.PEEK[HEADER])\r\n\
             A5 EXAMINE \"Sent\"\r\n"
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
                "listing the folders",
                "2 folders to fetch",
                "folder 1 of 2: searching 2 emails",
                "folder 1 of 2: 2 emails, 2 new",
                "folder 2 of 2: 0 emails, 0 new"
            ]
        );
        assert_eq!(
            uids(dir.path()),
            [("INBOX".into(), 3), ("INBOX".into(), 5)]
        );
        assert_eq!(
            store::sent_folders(dir.path()).expect("store should load"),
            HashSet::from(["Sent".to_owned()])
        );
    }

    #[test]
    fn fetches_only_headers_that_are_not_stored() {
        let dir = TempDir::new("fetch-again");
        let mut store = Store::open(dir.path()).expect("store should open");
        let first = [
            "* LIST () \"/\" INBOX\r\nA1 OK\r\n".into(),
            "* 2 EXISTS\r\n* OK [UIDVALIDITY 7] x\r\nA2 OK\r\n".into(),
            "* SEARCH 1 2\r\nA3 OK\r\n".into(),
            fetched(1, 1),
            fetched(2, 2),
            "A4 OK\r\n".into(),
        ]
        .concat();
        let mut client = fake_client(first.as_bytes());
        fetch(&mut client, &mut store, |_| {}).expect("fetch should succeed");

        // Email 1 was deleted on the server, email 3 is new.
        let second = [
            "* LIST () \"/\" INBOX\r\nA1 OK\r\n".into(),
            "* 2 EXISTS\r\n* OK [UIDVALIDITY 7] x\r\nA2 OK\r\n".into(),
            "* SEARCH 2 3\r\nA3 OK\r\n".into(),
            fetched(2, 3),
            "A4 OK\r\n".into(),
        ]
        .concat();
        let mut client = fake_client(second.as_bytes());
        let summary = fetch(&mut client, &mut store, |_| {})
            .expect("fetch should succeed");

        assert!(
            sent(&client).ends_with(
                "A4 UID FETCH 3 (UID INTERNALDATE BODY.PEEK[HEADER])\r\n"
            ),
            "{}",
            sent(&client)
        );
        assert_eq!(summary.fetched, 1);
        assert_eq!(
            uids(dir.path()),
            [("INBOX".into(), 2), ("INBOX".into(), 3)]
        );
    }

    #[test]
    fn estimates_the_time_left() {
        let progress = |seconds, done, total| {
            batch_progress(Duration::from_secs(seconds), done, total)
        };
        assert_eq!(
            progress(10, 500, 2000),
            "fetched 500 of 2000 (25%), about 30 s left"
        );
        assert_eq!(
            progress(30, 500, 1500),
            "fetched 500 of 1500 (33%), about 1 min left"
        );
        assert_eq!(
            progress(60, 1000, 121_000),
            "fetched 1000 of 121000 (0%), about 2 h 0 min left"
        );
        // Nothing to estimate yet, or nothing left.
        assert_eq!(progress(5, 0, 1000), "fetched 0 of 1000 (0%)");
        assert_eq!(progress(5, 1000, 1000), "fetched 1000 of 1000 (100%)");
    }

    #[test]
    fn names_the_folder_that_failed() {
        let dir = TempDir::new("fetch-error");
        let mut store = Store::open(dir.path()).expect("store should open");
        let responses = b"* LIST () \"/\" INBOX\r\nA1 OK\r\nA2 NO gone\r\n";
        let mut client = fake_client(responses);
        let error = fetch(&mut client, &mut store, |_| {})
            .expect_err("fetch should fail");
        assert_eq!(
            format!("{error:#}"),
            "failed to fetch folder \"INBOX\": EXAMINE failed: NO gone"
        );
    }
}
