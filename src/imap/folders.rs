// SPDX-License-Identifier: MIT OR Apache-2.0

//! Chooses the folders to fetch.

use crate::imap::Folder;

/// Attributes of folders that cannot hold emails (RFC 3501, RFC 5258).
const UNSELECTABLE: [&str; 2] = ["\\noselect", "\\nonexistent"];
/// Attributes of folders that are skipped: deleted emails and spam
/// (RFC 6154).
const SKIPPED: [&str; 2] = ["\\trash", "\\junk"];
/// Attribute of a folder with all emails, e.g. "All Mail" on Gmail, whose
/// other folders are views of it (RFC 6154).
const ALL: &str = "\\all";

/// Returns the folders to fetch: all but unselectable ones, trash, and spam.
///
/// If a folder has all emails, only that one. On Gmail, every label is a
/// folder, so an email with several labels would count several times.
///
/// # Arguments
///
/// - `folders`: All folders, as listed by the server.
#[must_use]
pub fn select_folders(folders: Vec<Folder>) -> Vec<Folder> {
    let has = |folder: &Folder, attributes: &[&str]| {
        folder
            .attributes
            .iter()
            .any(|attribute| attributes.contains(&attribute.as_str()))
    };
    let selectable: Vec<_> = folders
        .into_iter()
        .filter(|f| !has(f, &UNSELECTABLE) && !has(f, &SKIPPED))
        .collect();
    if selectable.iter().any(|f| has(f, &[ALL])) {
        selectable.into_iter().filter(|f| has(f, &[ALL])).collect()
    } else {
        selectable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// Returns the names of folders.
    ///
    /// # Arguments
    ///
    /// - `folders`: The folders.
    fn names(folders: &[Folder]) -> Vec<&str> {
        folders.iter().map(|f| f.name.as_str()).collect()
    }

    #[test]
    fn selects_all_folders_but_unselectable_trash_and_spam() {
        let folders = select_folders(vec![
            folder("INBOX", &[]),
            folder("Archive", &["\\haschildren"]),
            folder("Archive/2020", &["\\hasnochildren"]),
            folder("Sent", &["\\sent"]),
            folder("Shared", &["\\noselect"]),
            folder("Gone", &["\\nonexistent"]),
            folder("Trash", &["\\trash"]),
            folder("Spam", &["\\junk"]),
        ]);
        assert_eq!(
            names(&folders),
            ["INBOX", "Archive", "Archive/2020", "Sent"]
        );
    }

    /// On Gmail, every label is a folder, so only "All Mail" is fetched, or an
    /// email with several labels would count several times.
    #[test]
    fn selects_only_the_folder_with_all_emails() {
        // As on Gmail, where every label is a folder.
        let folders = select_folders(vec![
            folder("INBOX", &[]),
            folder("[Gmail]", &["\\noselect", "\\haschildren"]),
            folder("[Gmail]/All Mail", &["\\all", "\\hasnochildren"]),
            folder("[Gmail]/Sent Mail", &["\\sent"]),
            folder("[Gmail]/Spam", &["\\junk"]),
            folder("[Gmail]/Trash", &["\\trash"]),
            folder("Family", &[]),
        ]);
        assert_eq!(names(&folders), ["[Gmail]/All Mail"]);
    }
}
