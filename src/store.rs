// SPDX-License-Identifier: MIT OR Apache-2.0

//! The fetched headers of one source, in its directory in the data
//! directory.
//!
//! - `headers.jsonl` has one [`StoredHeader`] per line. New headers are
//!   appended, so a fetch that stops halfway keeps what it fetched. Only
//!   an incomplete last line is lost, which the next fetch removes.
//! - `folders.json` has the UIDs of the emails per folder at the end of
//!   the last complete fetch. Only these emails count: emails that were
//!   deleted on the server, and folders that were deleted, drop out.

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, ErrorKind, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

use crate::imap::FetchedHeader;

const HEADERS_FILE: &str = "headers.jsonl";
const FOLDERS_FILE: &str = "folders.json";

/// The header of an email, as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredHeader {
    pub folder: String,
    pub uidvalidity: u32,
    pub uid: u32,
    /// Arrival time on the server, e.g. `17-Jul-1996 02:44:25 -0700`.
    pub internaldate: Option<String>,
    /// The raw header. Bytes that are no UTF-8, which only old emails
    /// without encoded words (RFC 2047) have, are replaced by U+FFFD.
    pub header: String,
}

/// The emails of a folder on the server.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FolderState {
    pub uidvalidity: u32,
    pub uids: Vec<u32>,
    /// Whether the folder holds sent emails (`\Sent`, RFC 6154). False in
    /// stores of older versions, until the next fetch.
    #[serde(default)]
    pub sent: bool,
}

/// Identifies a stored header.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
struct Key {
    folder: String,
    uidvalidity: u32,
    uid: u32,
}

/// The store of one source, open to append headers.
#[derive(Debug)]
pub struct Store {
    dir: PathBuf,
    stored: HashSet<Key>,
    headers: BufWriter<File>,
}

impl Store {
    /// Opens the store of a source, and creates it if needed.
    ///
    /// # Arguments
    ///
    /// - `dir`: The directory of the source in the data directory.
    pub fn open(dir: &Path) -> anyhow::Result<Self> {
        fs::create_dir_all(dir)
            .with_context(|| format!("failed to create {}", dir.display()))?;
        let path = dir.join(HEADERS_FILE);
        let (stored, complete) = read_keys(&path)?;
        // Removes an incomplete last line, left by a fetch that stopped.
        // Not with the file opened to append, which Windows does not allow.
        if fs::metadata(&path).is_ok_and(|m| m.len() != complete) {
            OpenOptions::new()
                .write(true)
                .open(&path)
                .and_then(|file| file.set_len(complete))
                .with_context(|| {
                    format!("failed to repair {}", path.display())
                })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("failed to open {}", path.display()))?;
        Ok(Self {
            dir: dir.to_owned(),
            stored,
            headers: BufWriter::new(file),
        })
    }

    /// Returns the UIDs of the emails of a folder that are not stored yet.
    ///
    /// # Arguments
    ///
    /// - `folder`: The name of the folder.
    /// - `uidvalidity`: The current UIDVALIDITY of the folder.
    /// - `uids`: The UIDs of all emails in the folder.
    #[must_use]
    pub fn missing(
        &self,
        folder: &str,
        uidvalidity: u32,
        uids: &[u32],
    ) -> Vec<u32> {
        let mut key = Key {
            folder: folder.to_owned(),
            uidvalidity,
            uid: 0,
        };
        uids.iter()
            .copied()
            .filter(|&uid| {
                key.uid = uid;
                !self.stored.contains(&key)
            })
            .collect()
    }

    /// Appends the header of an email.
    ///
    /// # Arguments
    ///
    /// - `folder`: The name of the folder of the email.
    /// - `uidvalidity`: The current UIDVALIDITY of the folder.
    /// - `header`: The fetched header.
    pub fn append(
        &mut self,
        folder: &str,
        uidvalidity: u32,
        header: FetchedHeader,
    ) -> anyhow::Result<()> {
        let stored = StoredHeader {
            folder: folder.to_owned(),
            uidvalidity,
            uid: header.uid,
            internaldate: header.internaldate,
            header: String::from_utf8_lossy(&header.header).into(),
        };
        serde_json::to_writer(&mut self.headers, &stored)?;
        self.headers.write_all(b"\n")?;
        self.stored.insert(Key {
            folder: stored.folder,
            uidvalidity,
            uid: stored.uid,
        });
        Ok(())
    }

    /// Records the emails of all folders, at the end of a complete fetch.
    ///
    /// # Arguments
    ///
    /// - `folders`: The emails per folder name, as they are on the server.
    pub fn save_folders(
        &mut self,
        folders: &BTreeMap<String, FolderState>,
    ) -> anyhow::Result<()> {
        // The headers first, so that the folders never name an email
        // whose header is missing.
        self.headers.flush()?;
        self.headers.get_ref().sync_all()?;
        let path = self.dir.join(FOLDERS_FILE);
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_vec(folders)?).with_context(
            || format!("failed to write {}", temporary.display()),
        )?;
        fs::rename(&temporary, &path)
            .with_context(|| format!("failed to write {}", path.display()))
    }
}

/// Reads the headers of the emails on the server at the last fetch.
///
/// Only the emails of the last complete fetch count, see the module docs.
///
/// # Arguments
///
/// - `dir`: The directory of the source in the data directory.
pub fn load(dir: &Path) -> anyhow::Result<Vec<StoredHeader>> {
    let folders = read_folders(dir)?;
    let mut current: HashSet<Key> = folders
        .into_iter()
        .flat_map(|(folder, state)| {
            state.uids.into_iter().map(move |uid| Key {
                folder: folder.clone(),
                uidvalidity: state.uidvalidity,
                uid,
            })
        })
        .collect();
    let mut headers = Vec::new();
    for_each_line(&dir.join(HEADERS_FILE), |line| {
        let header: StoredHeader = serde_json::from_slice(line)?;
        let key = Key {
            folder: header.folder.clone(),
            uidvalidity: header.uidvalidity,
            uid: header.uid,
        };
        // Removing the key also skips duplicates.
        if current.remove(&key) {
            headers.push(header);
        }
        Ok(())
    })?;
    Ok(headers)
}

/// Returns the names of the folders that hold sent emails.
///
/// # Arguments
///
/// - `dir`: The directory of the source in the data directory.
pub fn sent_folders(dir: &Path) -> anyhow::Result<HashSet<String>> {
    Ok(read_folders(dir)?
        .into_iter()
        .filter(|(_, state)| state.sent)
        .map(|(name, _)| name)
        .collect())
}

/// Reads the emails of all folders at the end of the last complete fetch.
///
/// Returns no folders before the first fetch.
///
/// # Arguments
///
/// - `dir`: The directory of the source in the data directory.
fn read_folders(dir: &Path) -> anyhow::Result<BTreeMap<String, FolderState>> {
    let path = dir.join(FOLDERS_FILE);
    match fs::read(&path) {
        Ok(json) => serde_json::from_slice(&json)
            .with_context(|| format!("invalid {}", path.display())),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            Ok(BTreeMap::new())
        }
        Err(error) => Err(error)
            .with_context(|| format!("failed to read {}", path.display())),
    }
}

/// Returns the keys of all stored headers and the length of their lines.
///
/// # Arguments
///
/// - `path`: The path of the headers file.
fn read_keys(path: &Path) -> anyhow::Result<(HashSet<Key>, u64)> {
    let mut keys = HashSet::new();
    let complete = for_each_line(path, |line| {
        keys.insert(serde_json::from_slice(line)?);
        Ok(())
    })?;
    Ok((keys, complete))
}

/// Calls a function for every complete line of a file, if it exists.
///
/// Returns the length of the complete lines. An incomplete last line, left
/// by a fetch that stopped, is skipped.
///
/// # Arguments
///
/// - `path`: The path of the file.
/// - `on_line`: Receives each line, without its line ending.
fn for_each_line(
    path: &Path,
    mut on_line: impl FnMut(&[u8]) -> anyhow::Result<()>,
) -> anyhow::Result<u64> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(0);
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to open {}", path.display()));
        }
    };
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    let mut complete = 0;
    for number in 1.. {
        line.clear();
        reader
            .read_until(b'\n', &mut line)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if line.pop() != Some(b'\n') {
            break;
        }
        if let Err(error) = on_line(&line) {
            bail!("invalid line {number} in {}: {error}", path.display());
        }
        complete += line.len() as u64 + 1;
    }
    Ok(complete)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    /// Returns a fetched header with the UID as its subject.
    ///
    /// # Arguments
    ///
    /// - `uid`: The UID of the email.
    fn header(uid: u32) -> FetchedHeader {
        FetchedHeader {
            uid,
            internaldate: Some("17-Jul-1996 02:44:25 -0700".into()),
            header: format!("Subject: {uid}\r\n\r\n").into_bytes(),
        }
    }

    /// Returns the emails of folders, as the server would list them.
    ///
    /// The folder `Sent` holds sent emails.
    ///
    /// # Arguments
    ///
    /// - `entries`: The name, UIDVALIDITY, and UIDs of each folder.
    fn folders(
        entries: &[(&str, u32, &[u32])],
    ) -> BTreeMap<String, FolderState> {
        entries
            .iter()
            .map(|&(name, uidvalidity, uids)| {
                let state = FolderState {
                    uidvalidity,
                    uids: uids.to_vec(),
                    sent: name == "Sent",
                };
                (name.to_owned(), state)
            })
            .collect()
    }

    /// Returns `(folder, uid)` of all loaded headers, sorted.
    ///
    /// # Arguments
    ///
    /// - `dir`: The directory of the store.
    fn loaded(dir: &Path) -> Vec<(String, u32)> {
        let mut keys: Vec<_> = load(dir)
            .expect("store should load")
            .into_iter()
            .map(|h| (h.folder, h.uid))
            .collect();
        keys.sort();
        keys
    }

    #[test]
    fn loads_nothing_before_the_first_fetch() {
        let dir = TempDir::new("store-empty");
        assert_eq!(loaded(dir.path()), []);
    }

    #[test]
    fn keeps_headers_across_runs() {
        let dir = TempDir::new("store-runs");
        let mut store = Store::open(dir.path()).expect("store should open");
        assert_eq!(store.missing("INBOX", 1, &[1, 2]), [1, 2]);
        store
            .append("INBOX", 1, header(1))
            .expect("append should work");
        store
            .save_folders(&folders(&[("INBOX", 1, &[1])]))
            .expect("save should work");
        drop(store);

        let store = Store::open(dir.path()).expect("store should open");
        assert_eq!(store.missing("INBOX", 1, &[1, 2]), [2]);
        let headers = load(dir.path()).expect("store should load");
        assert_eq!(
            headers,
            [StoredHeader {
                folder: "INBOX".into(),
                uidvalidity: 1,
                uid: 1,
                internaldate: Some("17-Jul-1996 02:44:25 -0700".into()),
                header: "Subject: 1\r\n\r\n".into(),
            }]
        );
    }

    /// A new UIDVALIDITY makes all known UIDs of a folder invalid, so all its
    /// emails count as missing.
    #[test]
    fn fetches_folders_again_when_their_uidvalidity_changes() {
        let dir = TempDir::new("store-uidvalidity");
        let mut store = Store::open(dir.path()).expect("store should open");
        store
            .append("INBOX", 1, header(1))
            .expect("append should work");
        assert_eq!(store.missing("INBOX", 1, &[1]), [] as [u32; 0]);
        assert_eq!(store.missing("INBOX", 2, &[1]), [1]);
        assert_eq!(store.missing("Sent", 1, &[1]), [1]);
    }

    /// The store keeps all headers ever fetched, but only the emails of the
    /// last complete fetch count.
    #[test]
    fn loads_only_emails_that_are_on_the_server() {
        let dir = TempDir::new("store-current");
        let mut store = Store::open(dir.path()).expect("store should open");
        for (folder, uidvalidity, uid) in [
            ("INBOX", 1, 1),
            ("INBOX", 1, 2),
            ("Old", 1, 1),
            ("Sent", 1, 1),
        ] {
            store
                .append(folder, uidvalidity, header(uid))
                .expect("append should work");
        }
        // INBOX 2 was deleted, "Old" too, and "Sent" got new UIDs.
        store
            .save_folders(&folders(&[("INBOX", 1, &[1]), ("Sent", 2, &[1])]))
            .expect("save should work");
        assert_eq!(loaded(dir.path()), [("INBOX".into(), 1)]);
    }

    /// Ctrl+C can stop a fetch in the middle of a line. The next fetch must
    /// repair the file, keep the complete lines, and continue.
    #[test]
    fn keeps_headers_of_fetch_that_stopped_halfway() {
        let dir = TempDir::new("store-stopped");
        let mut store = Store::open(dir.path()).expect("store should open");
        store
            .append("INBOX", 1, header(1))
            .expect("append should work");
        store.headers.flush().expect("flush should work");
        drop(store);
        // As if the fetch stopped in the middle of writing a line.
        let path = dir.path().join(HEADERS_FILE);
        let mut file = OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("file should open");
        file.write_all(br#"{"folder":"INB"#)
            .expect("write should work");
        drop(file);

        let mut store = Store::open(dir.path()).expect("store should open");
        assert_eq!(store.missing("INBOX", 1, &[1, 2]), [2]);
        store
            .append("INBOX", 1, header(2))
            .expect("append should work");
        store
            .save_folders(&folders(&[("INBOX", 1, &[1, 2])]))
            .expect("save should work");
        assert_eq!(
            loaded(dir.path()),
            [("INBOX".into(), 1), ("INBOX".into(), 2)]
        );
    }

    #[test]
    fn records_which_folders_hold_sent_emails() {
        let dir = TempDir::new("store-sent");
        assert!(
            sent_folders(dir.path())
                .expect("store should load")
                .is_empty()
        );
        let mut store = Store::open(dir.path()).expect("store should open");
        store
            .save_folders(&folders(&[("INBOX", 1, &[1]), ("Sent", 1, &[1])]))
            .expect("save should work");
        let sent = sent_folders(dir.path()).expect("store should load");
        assert_eq!(sent, HashSet::from(["Sent".to_owned()]));
    }

    /// Stores of versions before the Sent flag must still load.
    #[test]
    fn reads_folders_of_older_versions_as_not_sent() {
        let dir = TempDir::new("store-old");
        fs::write(
            dir.path().join(FOLDERS_FILE),
            r#"{"Sent":{"uidvalidity":1,"uids":[1]}}"#,
        )
        .expect("write should work");
        assert!(
            sent_folders(dir.path())
                .expect("store should load")
                .is_empty()
        );
    }

    #[test]
    fn rejects_invalid_lines() {
        let dir = TempDir::new("store-invalid");
        fs::write(dir.path().join(HEADERS_FILE), "{}\n")
            .expect("write should work");
        let error = Store::open(dir.path()).expect_err("open should fail");
        assert!(
            error.to_string().starts_with("invalid line 1 in"),
            "{error}"
        );
    }
}
