// SPDX-License-Identifier: MIT OR Apache-2.0

//! The index: one entry per email of all sources, with what the report
//! needs. Copies of an email count once, e.g. if it is in two folders, or
//! in two sources because one account forwards to the other.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use anyhow::Context;
use jiff::Timestamp;
use jiff::tz::Offset;
use serde::{Deserialize, Serialize};

use crate::category::Category;
use crate::email::{self, Email};
use crate::store::StoredHeader;

/// Name of the index file in the data directory.
pub const INDEX_FILE: &str = "index.json";
/// 1980-01-01 00:00:00 UTC. Earlier dates are broken, e.g. 1970 from a
/// timestamp of 0, as hardly anybody has emails from before.
const EARLIEST_DATE: i64 = 315_532_800;
/// Dates more than a day in the future are broken.
const MAX_FUTURE: i64 = 24 * 60 * 60;

/// The emails of all sources.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub emails: Vec<Entry>,
}

/// An email, with all its copies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Sources with a copy of the email, in the order of the config.
    pub sources: Vec<String>,
    /// The year in UTC, of `Date` or, if it is missing or implausible, of
    /// the arrival on the server.
    pub year: Option<i64>,
    /// Address of the sender, in lowercase.
    pub sender: Option<String>,
    pub sender_name: Option<String>,
    pub category: Category,
    /// Whether the owner sent the email: it came from one of the own
    /// addresses, or a copy is in a Sent folder.
    #[serde(default)]
    pub sent: bool,
    /// The recipients of a sent email, in lowercase. Empty for received
    /// emails, whose recipient is the owner.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recipients: Vec<String>,
}

/// The fetched emails of a source, as the index needs them.
#[derive(Debug, Clone, Default)]
pub struct SourceEmails {
    /// The name of the source.
    pub name: String,
    /// The stored headers of the emails on the server.
    pub headers: Vec<StoredHeader>,
    /// The folders that hold sent emails.
    pub sent_folders: HashSet<String>,
}

impl Index {
    /// Builds the index of all sources.
    ///
    /// # Arguments
    ///
    /// - `sources`: The fetched emails of each source.
    /// - `own_addresses`: The addresses of the owner in all sources, in
    ///   lowercase.
    /// - `now`: The current time as Unix timestamp, the latest plausible date.
    #[must_use]
    pub fn build(
        sources: &[SourceEmails],
        own_addresses: &HashSet<String>,
        now: i64,
    ) -> Self {
        let mut emails: Vec<Entry> = Vec::new();
        let mut positions = HashMap::new();
        for source in sources {
            for stored in &source.headers {
                let email = Email::parse(stored.header.as_bytes());
                let sent = source.sent_folders.contains(&stored.folder)
                    || email
                        .sender
                        .as_ref()
                        .is_some_and(|s| own_addresses.contains(&s.address));
                let key = copy_key(&source.name, stored, &email);
                if let Some(&position) = positions.get(&key) {
                    let entry: &mut Entry = &mut emails[position];
                    if !entry.sources.contains(&source.name) {
                        entry.sources.push(source.name.clone());
                    }
                    // A copy in a Sent folder makes all copies sent.
                    if sent && !entry.sent {
                        entry.sent = true;
                        entry.recipients.clone_from(&email.recipients);
                    }
                    continue;
                }
                positions.insert(key, emails.len());
                let mut entry = entry(&source.name, stored, &email, now);
                if sent {
                    entry.sent = true;
                    entry.recipients = email.recipients;
                }
                emails.push(entry);
            }
        }
        Self { emails }
    }

    /// Writes the index, replacing an old one only when done.
    ///
    /// # Arguments
    ///
    /// - `path`: The path of the index file.
    pub fn write(&self, path: &Path) -> anyhow::Result<()> {
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_vec(self)?).with_context(
            || format!("failed to write {}", temporary.display()),
        )?;
        fs::rename(&temporary, path)
            .with_context(|| format!("failed to write {}", path.display()))
    }

    /// Reads an index.
    ///
    /// # Arguments
    ///
    /// - `path`: The path of the index file.
    pub fn read(path: &Path) -> anyhow::Result<Self> {
        let json = fs::read(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_slice(&json)
            .with_context(|| format!("invalid index {}", path.display()))
    }
}

/// Returns the entry of an email, with its first copy.
///
/// # Arguments
///
/// - `source`: The name of the source of the copy.
/// - `stored`: The stored header of the copy.
/// - `email`: The parsed header.
/// - `now`: The current time as Unix timestamp, the latest plausible date.
fn entry(
    source: &str,
    stored: &StoredHeader,
    email: &Email,
    now: i64,
) -> Entry {
    let plausible =
        |date: &i64| (EARLIEST_DATE..=now + MAX_FUTURE).contains(date);
    let date = email.date.filter(plausible).or_else(|| {
        stored
            .internaldate
            .as_deref()
            .and_then(email::parse_internaldate)
            .filter(plausible)
    });
    Entry {
        sources: vec![source.to_owned()],
        year: date.and_then(year),
        sender: email.sender.as_ref().map(|s| s.address.clone()),
        sender_name: email.sender.as_ref().and_then(|s| s.name.clone()),
        category: Category::of(email),
        sent: false,
        recipients: Vec::new(),
    }
}

/// Returns a key that is the same for all copies of an email.
///
/// The `Message-ID` identifies an email. Without one, which only few
/// emails lack, date, sender, and subject do. Without these too, the email
/// counts as unique.
///
/// # Arguments
///
/// - `source`: The name of the source of the copy.
/// - `stored`: The stored header of the copy.
/// - `email`: The parsed header.
fn copy_key(source: &str, stored: &StoredHeader, email: &Email) -> String {
    if let Some(id) = &email.message_id {
        return format!("id\0{id}");
    }
    let sender = email.sender.as_ref().map(|s| s.address.as_str());
    if email.date.is_some() || sender.is_some() || !email.subject.is_empty() {
        return format!(
            "fields\0{:?}\0{:?}\0{}",
            email.date, sender, email.subject
        );
    }
    format!(
        "location\0{source}\0{}\0{}\0{}",
        stored.folder, stored.uidvalidity, stored.uid
    )
}

/// Returns the year in UTC of a Unix timestamp.
///
/// Returns `None` for timestamps beyond the years -9999 to 9999, which
/// jiff supports, but no plausible date is.
///
/// # Arguments
///
/// - `timestamp`: Seconds since 1970-01-01 00:00:00 UTC.
fn year(timestamp: i64) -> Option<i64> {
    let timestamp = Timestamp::from_second(timestamp).ok()?;
    Some(i64::from(Offset::UTC.to_datetime(timestamp).year()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    /// 2026-11-01 00:00:00 UTC
    const NOW: i64 = 1_793_491_200;

    /// Returns a stored header in INBOX.
    ///
    /// # Arguments
    ///
    /// - `uid`: The UID of the email.
    /// - `header`: The raw header.
    fn stored(uid: u32, header: &str) -> StoredHeader {
        StoredHeader {
            folder: "INBOX".into(),
            uidvalidity: 1,
            uid,
            internaldate: None,
            header: header.into(),
        }
    }

    /// Returns sources as `Index::build` takes them, without Sent folders.
    ///
    /// # Arguments
    ///
    /// - `sources`: The name and the stored headers of each source.
    fn sources(sources: &[(&str, Vec<StoredHeader>)]) -> Vec<SourceEmails> {
        sources
            .iter()
            .map(|(name, headers)| SourceEmails {
                name: (*name).to_owned(),
                headers: headers.clone(),
                sent_folders: HashSet::new(),
            })
            .collect()
    }

    /// Builds an index without own addresses.
    ///
    /// # Arguments
    ///
    /// - `entries`: The name and the stored headers of each source.
    fn build(entries: &[(&str, Vec<StoredHeader>)]) -> Index {
        Index::build(&sources(entries), &HashSet::new(), NOW)
    }

    #[test]
    fn computes_the_year_in_utc() {
        for (timestamp, expected) in [
            (0, 1970),
            (-1, 1969),
            // 2000-02-29, a leap day
            (951_782_400, 2000),
            // 2023-12-31 23:59:59 and a second later
            (1_704_067_199, 2023),
            (1_704_067_200, 2024),
            // 2100-03-01, after the day that is no leap day
            (4_107_542_400, 2100),
        ] {
            assert_eq!(year(timestamp), Some(expected), "{timestamp}");
        }
    }

    #[test]
    fn indexes_sender_year_and_category() {
        let header = "From: Jane <Jane@Example.com>\r\n\
                      Date: Mon, 1 Jan 2024 10:00:00 +0000\r\n\
                      List-Id: <list.example.com>\r\n\r\n";
        let index = build(&[("work", vec![stored(1, header)])]);
        assert_eq!(
            index.emails,
            [Entry {
                sources: vec!["work".into()],
                year: Some(2024),
                sender: Some("jane@example.com".into()),
                sender_name: Some("Jane".into()),
                category: Category::Newsletter,
                sent: false,
                recipients: Vec::new(),
            }]
        );
    }

    #[test]
    fn tells_sent_from_received_emails() {
        // Boring setup: in "work", an email from an own address, one in a
        // Sent folder from an address that the config misses, and a
        // received one. In "home", a copy of the received one in Sent.
        let header = |id: u32, from: &str, to: &str| {
            format!("Message-ID: <{id}>\r\nFrom: {from}\r\nTo: {to}\r\n\r\n")
        };
        let mut in_sent =
            stored(2, &header(2, "me@old.example", "b@x.example"));
        in_sent.folder = "Sent".into();
        let received = header(3, "c@x.example", "me@example.com");
        let mut received_copy = stored(9, &received);
        received_copy.folder = "Sent".into();
        let mut sources = sources(&[
            (
                "work",
                vec![
                    stored(1, &header(1, "Me@Example.com", "a@x.example")),
                    in_sent,
                    stored(3, &received),
                ],
            ),
            ("home", vec![received_copy]),
        ]);
        sources[0].sent_folders.insert("Sent".into());
        let own = HashSet::from(["me@example.com".to_owned()]);

        let index = Index::build(&sources, &own, NOW);

        let sent: Vec<_> = index
            .emails
            .iter()
            .map(|e| (e.sent, e.recipients.clone()))
            .collect();
        assert_eq!(
            sent,
            [
                (true, vec!["a@x.example".to_owned()]),
                (true, vec!["b@x.example".to_owned()]),
                // The Sent folder of "home" is unknown, so it is received.
                (false, Vec::new()),
            ]
        );
    }

    #[test]
    fn counts_copies_once() {
        // Boring setup: the same email in two folders of "work" and in
        // "home", and another email in "home".
        let copy = "Message-ID: <1@example.com>\r\nFrom: a@example.com\r\n\r\n";
        let mut in_sent = stored(1, copy);
        in_sent.folder = "Sent".into();
        let other =
            "Message-ID: <2@example.com>\r\nFrom: a@example.com\r\n\r\n";
        let index = build(&[
            ("work", vec![stored(1, copy), in_sent]),
            ("home", vec![stored(7, other), stored(8, copy)]),
        ]);

        let copies: Vec<_> =
            index.emails.iter().map(|e| e.sources.clone()).collect();
        assert_eq!(copies, [vec!["work", "home"], vec!["home"]]);
    }

    #[test]
    fn counts_copies_without_message_id_by_their_fields() {
        let header = "From: a@example.com\r\nSubject: Hi\r\n\r\n";
        let index = build(&[(
            "work",
            vec![
                stored(1, header),
                stored(2, header),
                stored(3, "From: a@example.com\r\nSubject: Ho\r\n\r\n"),
                // Without any field, these are no copies.
                stored(4, "X: 1\r\n\r\n"),
                stored(5, "X: 1\r\n\r\n"),
            ],
        )]);
        assert_eq!(index.emails.len(), 4);
    }

    #[test]
    fn falls_back_to_the_arrival_for_missing_or_implausible_dates() {
        let mut headers = Vec::new();
        for (uid, date) in [
            (1, "Date: Mon, 1 Jan 2024 10:00:00 +0000"),
            (2, ""),
            (3, "Date: Thu, 1 Jan 1970 00:00:01 +0000"),
            (4, "Date: Fri, 1 Jan 2100 00:00:00 +0000"),
        ] {
            let mut header =
                stored(uid, &format!("Message-ID: <{uid}>\r\n{date}\r\n\r\n"));
            header.internaldate = Some("15-Jun-2020 12:00:00 +0000".into());
            headers.push(header);
        }
        // Without a plausible arrival either, the year is unknown.
        let mut undated = stored(5, "Message-ID: <5>\r\n\r\n");
        undated.internaldate = Some("01-Jan-1970 00:00:01 +0000".into());
        headers.push(undated);

        let index = build(&[("work", headers)]);
        let years: Vec<_> = index.emails.iter().map(|e| e.year).collect();
        assert_eq!(
            years,
            [Some(2024), Some(2020), Some(2020), Some(2020), None]
        );
    }

    #[test]
    fn writes_and_reads_the_index() {
        let dir = TempDir::new("index-roundtrip");
        let path = dir.path().join(INDEX_FILE);
        let header = "From: a@example.com\r\n\r\n";
        let index = build(&[("work", vec![stored(1, header)])]);
        index.write(&path).expect("index should be written");
        assert_eq!(Index::read(&path).expect("index should be read"), index);
    }
}
