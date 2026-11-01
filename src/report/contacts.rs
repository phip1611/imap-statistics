// SPDX-License-Identifier: MIT OR Apache-2.0

//! The senders of received emails and the recipients of sent emails, with
//! what the report shows of each.

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashSet};

use crate::index::Index;

/// What the report shows of a sender or recipient.
#[derive(Debug, Default)]
pub(super) struct Contact {
    pub(super) emails: u64,
    /// How often each display name was used. Empty for recipients, as the
    /// index has only the names of senders.
    names: BTreeMap<String, u64>,
}

impl Contact {
    /// Returns the most used name, the first in alphabetical order on ties.
    pub(super) fn name(&self) -> &str {
        self.names
            .iter()
            .max_by_key(|&(name, count)| (count, Reverse(name)))
            .map(|(name, _)| name.as_str())
            .unwrap_or_default()
    }
}

/// Returns the senders of received emails, the most emails first.
///
/// Sent emails do not count, as they are all from the owner.
///
/// # Arguments
///
/// - `index`: The index of all emails.
pub(super) fn senders(index: &Index) -> Vec<(&str, Contact)> {
    let mut senders: BTreeMap<&str, Contact> = BTreeMap::new();
    for email in index.emails.iter().filter(|e| !e.sent) {
        let Some(address) = &email.sender else {
            continue;
        };
        let sender = senders.entry(address).or_default();
        sender.emails += 1;
        if let Some(name) = &email.sender_name {
            *sender.names.entry(name.clone()).or_default() += 1;
        }
    }
    rank(senders)
}

/// Returns the recipients of sent emails, the most emails first.
///
/// An email to several recipients counts for each of them.
///
/// # Arguments
///
/// - `index`: The index of all emails.
/// - `own`: The addresses of the owner, in lowercase, which do not count.
pub(super) fn recipients<'a>(
    index: &'a Index,
    own: &HashSet<String>,
) -> Vec<(&'a str, Contact)> {
    let mut recipients: BTreeMap<&str, Contact> = BTreeMap::new();
    for email in index.emails.iter().filter(|e| e.sent) {
        for address in email.recipients.iter().filter(|r| !own.contains(*r)) {
            recipients.entry(address).or_default().emails += 1;
        }
    }
    rank(recipients)
}

/// Sorts contacts by their number of emails, the most first.
///
/// # Arguments
///
/// - `contacts`: The contacts by address.
fn rank(contacts: BTreeMap<&str, Contact>) -> Vec<(&str, Contact)> {
    let mut ranked: Vec<_> = contacts.into_iter().collect();
    // Ties by address, so that the order is stable.
    ranked
        .sort_by_key(|(address, contact)| (Reverse(contact.emails), *address));
    ranked
}
