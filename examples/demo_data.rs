// SPDX-License-Identifier: MIT OR Apache-2.0

//! Writes made-up headers into a data directory, as if `fetch` had
//! fetched them, for the demo report in `examples/demo`.
//!
//! The data is deterministic, so that the demo report only changes when
//! the code does. It covers every category, copies of emails in both
//! sources, and emails without a usable date.
//!
//! ```sh
//! cargo run --example demo_data -- examples/demo/data
//! ```

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, bail};
use imap_statistics::imap::FetchedHeader;
use imap_statistics::store::{FolderState, Store};

/// The demo data spans these years. The last one is in the past, so that
/// no date is ever in the future and dropped as implausible.
const FIRST_YEAR: u32 = 2012;
const LAST_YEAR: u32 = 2025;
const UIDVALIDITY: u32 = 1;

/// A made-up sender.
struct Sender {
    name: &'static str,
    address: &'static str,
    /// Headers that decide the category, e.g. `List-Id`.
    extra: &'static str,
    subject: &'static str,
    /// Emails in the first and the last year, linear in between.
    first: u32,
    last: u32,
    /// The source that receives the emails.
    source: &'static str,
}

const SENDERS: &[Sender] = &[
    Sender {
        name: "Anna Schmidt",
        address: "anna.schmidt@example.org",
        extra: "",
        subject: "Re: Weekend plans",
        first: 60,
        last: 25,
        source: "private",
    },
    Sender {
        name: "Ben Miller",
        address: "ben.miller@example.com",
        extra: "",
        subject: "Project update",
        first: 40,
        last: 90,
        source: "work",
    },
    Sender {
        name: "Chen Li",
        address: "chen.li@example.net",
        extra: "",
        subject: "Re: Meeting notes",
        first: 20,
        last: 45,
        source: "work",
    },
    Sender {
        name: "Example Shop",
        address: "news@shop.example.com",
        extra: "List-Unsubscribe: <mailto:unsubscribe@shop.example.com>\r\n",
        subject: "Our offers of the week",
        first: 10,
        last: 120,
        source: "private",
    },
    Sender {
        name: "Example Forum",
        address: "digest@forum.example.org",
        extra: "List-Id: <digest.forum.example.org>\r\n",
        subject: "Daily digest",
        first: 80,
        last: 15,
        source: "private",
    },
    Sender {
        name: "Example Shop",
        address: "orders@shop.example.com",
        extra: "",
        subject: "Your order has shipped",
        first: 5,
        last: 30,
        source: "private",
    },
    Sender {
        name: "Example Telco",
        address: "billing@telco.example.net",
        extra: "",
        subject: "Ihre Rechnung",
        first: 12,
        last: 12,
        source: "private",
    },
    Sender {
        name: "Example CI",
        address: "noreply@ci.example.com",
        extra: "Auto-Submitted: auto-generated\r\n",
        subject: "Build passed",
        first: 0,
        last: 150,
        source: "work",
    },
    Sender {
        name: "Facebook",
        address: "notification@facebookmail.com",
        extra: "",
        subject: "You have new notifications",
        first: 50,
        last: 5,
        source: "private",
    },
    Sender {
        name: "LinkedIn",
        address: "messages-noreply@linkedin.com",
        extra: "",
        subject: "You appeared in searches",
        first: 15,
        last: 35,
        source: "work",
    },
];

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct",
    "Nov", "Dec",
];

/// Writes the made-up headers into the directory given as argument.
fn main() -> anyhow::Result<()> {
    let Some(dir) = env::args_os().nth(1).map(PathBuf::from) else {
        bail!("usage: demo_data <data directory>");
    };
    if fs::read_dir(&dir).is_ok_and(|mut entries| entries.next().is_some()) {
        bail!("{} is not empty, remove it first", dir.display());
    }
    let mut emails: BTreeMap<&str, Vec<FetchedHeader>> = BTreeMap::new();
    for (index, sender) in SENDERS.iter().enumerate() {
        for year in FIRST_YEAR..=LAST_YEAR {
            let progress = year - FIRST_YEAR;
            let span = LAST_YEAR - FIRST_YEAR;
            let trend = (sender.first * (span - progress)
                + sender.last * progress)
                / span;
            // Varies by up to 30% from year to year, the same in every run.
            let variation = 85 + (year * 7 + index as u32 * 13) % 31;
            let count = trend * variation / 100;
            for number in 0..count {
                let header = header(index, sender, year, number);
                let email = |uid| FetchedHeader {
                    uid,
                    internaldate: Some(internaldate(year, number)),
                    header: header.clone().into_bytes(),
                };
                let uid = next_uid(&emails, sender.source);
                emails.entry(sender.source).or_default().push(email(uid));
                // Every 10th personal email also went to the other
                // source, e.g. by forwarding, and counts once.
                if sender.extra.is_empty() && number % 10 == 0 {
                    let other = if sender.source == "work" {
                        "private"
                    } else {
                        "work"
                    };
                    let uid = next_uid(&emails, other);
                    emails.entry(other).or_default().push(email(uid));
                }
            }
        }
    }
    for (source, headers) in emails {
        let mut store = Store::open(&dir.join(source))?;
        let uids = headers.iter().map(|h| h.uid).collect();
        for header in headers {
            store.append("INBOX", UIDVALIDITY, header)?;
        }
        let folders = BTreeMap::from([(
            "INBOX".to_owned(),
            FolderState {
                uidvalidity: UIDVALIDITY,
                uids,
                sent: false,
            },
        )]);
        store
            .save_folders(&folders)
            .with_context(|| format!("failed to write source {source}"))?;
    }
    Ok(())
}

/// Returns the next free UID of a source.
///
/// # Arguments
///
/// - `emails`: The emails per source so far.
/// - `source`: The name of the source.
fn next_uid(emails: &BTreeMap<&str, Vec<FetchedHeader>>, source: &str) -> u32 {
    emails
        .get(source)
        .map_or(1, |headers| headers.len() as u32 + 1)
}

/// Returns the header of the `number`th email of a sender in a year.
///
/// # Arguments
///
/// - `index`: The index of the sender in [`SENDERS`].
/// - `sender`: The sender.
/// - `year`: The year of the email.
/// - `number`: The number of the email of the sender in the year.
fn header(index: usize, sender: &Sender, year: u32, number: u32) -> String {
    let month = MONTHS[(number % 12) as usize];
    let day = number % 28 + 1;
    // Every 25th email has no Date, so that the arrival on the server
    // counts. Every 100th has a broken one and arrived at an implausible
    // time too, so that it has no year.
    let date = if number % 100 == 99 {
        "Date: someday\r\n".to_owned()
    } else if number % 25 == 24 {
        String::new()
    } else {
        format!("Date: {day} {month} {year} 10:00:00 +0000\r\n")
    };
    format!(
        "Message-ID: <{index}.{year}.{number}@demo.example>\r\n\
         From: {} <{}>\r\n\
         To: me@example.com\r\n\
         Subject: {}\r\n\
         {date}{}\r\n",
        sender.name, sender.address, sender.subject, sender.extra
    )
}

/// Returns the arrival on the server, as IMAP reports it.
///
/// # Arguments
///
/// - `year`: The year of the email.
/// - `number`: The number of the email of the sender in the year.
fn internaldate(year: u32, number: u32) -> String {
    if number % 100 == 99 {
        return "01-Jan-1970 00:00:00 +0000".to_owned();
    }
    let month = MONTHS[(number % 12) as usize];
    let day = number % 28 + 1;
    format!("{day:02}-{month}-{year} 10:05:00 +0000")
}
