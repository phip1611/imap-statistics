// SPDX-License-Identifier: MIT OR Apache-2.0

//! The senders of received emails and the recipients of sent emails, with
//! what the report shows of each.

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashSet};

use crate::category::Category;
use crate::index::{Entry, Index};

/// What the report shows of a sender or recipient.
#[derive(Debug, Default)]
pub(super) struct Contact {
    pub(super) emails: u64,
    /// The emails per year, without the emails that have no year.
    pub(super) years: BTreeMap<i64, u64>,
    /// How often each display name was used. Empty for recipients, as the
    /// index has only the names of senders.
    names: BTreeMap<String, u64>,
    /// How many emails are in each category. Empty for recipients, as the
    /// categories of sent emails say little.
    categories: BTreeMap<Category, u64>,
}

impl Contact {
    /// Counts an email of the contact.
    ///
    /// # Arguments
    ///
    /// - `email`: The email.
    fn add(&mut self, email: &Entry) {
        self.emails += 1;
        if let Some(year) = email.year {
            *self.years.entry(year).or_default() += 1;
        }
    }

    /// Returns the most used name, the first in alphabetical order on ties.
    pub(super) fn name(&self) -> &str {
        self.names
            .iter()
            .max_by_key(|&(name, count)| (count, Reverse(name)))
            .map(|(name, _)| name.as_str())
            .unwrap_or_default()
    }

    /// Returns the most common category, the first in the order of the
    /// rules on ties.
    pub(super) fn category(&self) -> Option<Category> {
        self.categories
            .iter()
            .max_by_key(|&(category, count)| (count, Reverse(category)))
            .map(|(&category, _)| category)
    }

    /// Returns the years of the first and the last email.
    pub(super) fn period(&self) -> Option<(i64, i64)> {
        let first = self.years.keys().next()?;
        let last = self.years.keys().next_back()?;
        Some((*first, *last))
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
        sender.add(email);
        *sender.categories.entry(email.category).or_default() += 1;
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
            recipients.entry(address).or_default().add(email);
        }
    }
    rank(recipients)
}

/// Returns the contacts of each year with their emails in that year, the
/// most emails first.
///
/// # Arguments
///
/// - `contacts`: The contacts.
pub(super) fn per_year<'a>(
    contacts: &'a [(&'a str, Contact)],
) -> BTreeMap<i64, Vec<(&'a str, &'a Contact, u64)>> {
    let mut years: BTreeMap<i64, Vec<_>> = BTreeMap::new();
    for (address, contact) in contacts {
        for (&year, &emails) in &contact.years {
            years
                .entry(year)
                .or_default()
                .push((*address, contact, emails));
        }
    }
    for contacts in years.values_mut() {
        // Ties by address, so that the order is stable.
        contacts
            .sort_by_key(|&(address, _, emails)| (Reverse(emails), address));
    }
    years
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
