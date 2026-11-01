// SPDX-License-Identifier: MIT OR Apache-2.0

//! Sorts emails into coarse categories by their headers.
//!
//! The rules only look at the sender, the subject, and a few headers that
//! mailing lists and programs add, never at the content. They are simple
//! on purpose: for statistics, a few emails in the wrong category are
//! fine. The first rule that matches wins, in the order of [`Category`].
//! The senders and words of each category are in [`rules`].

pub mod rules;

use serde::{Deserialize, Serialize};

use crate::category::rules::Rules;
use crate::email::Email;

/// A coarse category of emails.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    /// From a social network.
    Social,
    /// From a mailing list or a newsletter.
    Newsletter,
    /// About an order, an invoice, or a delivery.
    Shopping,
    /// Sent by a program, e.g. a notification or a bounce.
    Notification,
    /// Everything else, mostly written by people.
    Personal,
}

impl Category {
    /// All categories, in the order of their rules.
    pub const ALL: [Self; 5] = [
        Self::Social,
        Self::Newsletter,
        Self::Shopping,
        Self::Notification,
        Self::Personal,
    ];

    /// Returns the name of the category, as in the index and the report.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Social => "social",
            Self::Newsletter => "newsletter",
            Self::Shopping => "shopping",
            Self::Notification => "notification",
            Self::Personal => "personal",
        }
    }

    /// Returns the category of an email.
    ///
    /// # Arguments
    ///
    /// - `email`: The parsed headers of the email.
    #[must_use]
    pub fn of(email: &Email) -> Self {
        let address = email.sender.as_ref().map_or("", |s| s.address.as_str());
        let (local_part, domain) =
            address.rsplit_once('@').unwrap_or((address, ""));
        let subject = normalize(&email.subject);
        let words: Vec<&str> = subject.split(' ').collect();
        let matches = |rules: &Rules| {
            rules.domains.iter().any(|d| is_domain_of(domain, d))
                || rules.senders.iter().any(|s| is_sender(local_part, s))
                || rules.words.iter().any(|w| has_words(&words, w))
        };
        let list = email.list
            || matches!(
                email.precedence.as_deref(),
                Some("bulk" | "list" | "junk")
            );
        let automatic =
            email.auto_submitted.as_deref().is_some_and(|v| v != "no");
        if matches(&rules::SOCIAL) {
            Self::Social
        } else if list || matches(&rules::NEWSLETTER) {
            Self::Newsletter
        } else if matches(&rules::SHOPPING) {
            Self::Shopping
        } else if automatic || matches(&rules::NOTIFICATION) {
            Self::Notification
        } else {
            Self::Personal
        }
    }
}

/// Checks whether a domain is another domain or one of its subdomains.
///
/// # Arguments
///
/// - `domain`: The domain to check, e.g. `mail.xing.com`.
/// - `parent`: The other domain, e.g. `xing.com`.
fn is_domain_of(domain: &str, parent: &str) -> bool {
    domain == parent
        || domain
            .strip_suffix(parent)
            .is_some_and(|sub| sub.ends_with('.'))
}

/// Checks whether a local part is a sender, also with a tag.
///
/// E.g. `noreply`, `noreply-shop`, and `noreply+news` are the sender
/// `noreply`, but `noreplyfan` is not.
///
/// # Arguments
///
/// - `local_part`: The part of an address before the `@`.
/// - `sender`: The sender of a rule.
fn is_sender(local_part: &str, sender: &str) -> bool {
    local_part.strip_prefix(sender).is_some_and(|rest| {
        rest.is_empty() || rest.starts_with(['+', '-', '.', '_'])
    })
}

/// Checks whether words contain a word or phrase of a rule.
///
/// # Arguments
///
/// - `words`: The normalized words of a subject.
/// - `rule`: A word or phrase, which matches the start of its last word
///   if it ends with `*`.
fn has_words(words: &[&str], rule: &str) -> bool {
    let (phrase, prefix) = rule
        .strip_suffix('*')
        .map_or((rule, false), |phrase| (phrase, true));
    let wanted: Vec<&str> = phrase.split(' ').collect();
    words.windows(wanted.len()).any(|window| {
        window
            .iter()
            .zip(&wanted)
            .enumerate()
            .all(|(i, (word, want))| {
                if prefix && i == wanted.len() - 1 {
                    word.starts_with(want)
                } else {
                    word == want
                }
            })
    })
}

/// Normalizes text for word matching.
///
/// Lowercases it, removes apostrophes, e.g. "don't" becomes "dont", and
/// turns all other characters that are no letters or digits into single
/// spaces between words.
///
/// # Arguments
///
/// - `text`: The text, e.g. a subject.
fn normalize(text: &str) -> String {
    let cleaned: String = text
        .to_lowercase()
        .chars()
        .filter(|&c| c != '\'' && c != '’')
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::email::Sender;

    /// Returns an email with a sender and a subject.
    ///
    /// # Arguments
    ///
    /// - `from`: The address of the sender.
    /// - `subject`: The subject.
    fn email(from: &str, subject: &str) -> Email {
        Email {
            sender: Some(Sender {
                address: from.into(),
                name: None,
            }),
            subject: subject.into(),
            ..Email::default()
        }
    }

    #[test]
    fn detects_social_networks_and_their_subdomains() {
        let social = [
            "notification@facebookmail.com",
            "messages-noreply@linkedin.com",
            "info@mail.xing.com",
        ];
        for from in social {
            assert_eq!(
                Category::of(&email(from, "")),
                Category::Social,
                "{from}"
            );
        }
        let other = email("me@box.com", "Hi");
        assert_eq!(Category::of(&other), Category::Personal);
    }

    #[test]
    fn detects_newsletters_by_their_headers() {
        let mut list = email("news@shop.example", "Your order");
        list.list = true;
        assert_eq!(Category::of(&list), Category::Newsletter);
        let mut bulk = email("news@shop.example", "Offers");
        bulk.precedence = Some("bulk".into());
        assert_eq!(Category::of(&bulk), Category::Newsletter);
    }

    #[test]
    fn detects_shopping_by_the_subject() {
        for subject in [
            "Your order #123 has shipped",
            "Ihre Rechnung vom 01.01.2024",
            "Versandbestätigung",
            "Bestellbestätigung",
            "RECEIPT",
        ] {
            let shop = email("noreply@shop.example", subject);
            assert_eq!(Category::of(&shop), Category::Shopping, "{subject}");
        }
        for subject in ["Border control", "Re: Lunch?", ""] {
            let other = email("jane@example.com", subject);
            assert_eq!(Category::of(&other), Category::Personal, "{subject}");
        }
    }

    #[test]
    fn detects_notifications_by_sender_and_headers() {
        for from in [
            "noreply@example.com",
            "no-reply+alerts@example.com",
            "mailer-daemon@example.com",
            "notifications@github.example",
        ] {
            let notification = email(from, "Build failed");
            assert_eq!(
                Category::of(&notification),
                Category::Notification,
                "{from}"
            );
        }
        let mut auto = email("jane@example.com", "Hi");
        auto.auto_submitted = Some("auto-generated".into());
        assert_eq!(Category::of(&auto), Category::Notification);
        auto.auto_submitted = Some("no".into());
        assert_eq!(Category::of(&auto), Category::Personal);
        let person = email("noreplyfan@example.com", "Hi");
        assert_eq!(Category::of(&person), Category::Personal);
    }

    #[test]
    fn matches_whole_words_unless_a_rule_ends_with_a_star() {
        let category = |subject| Category::of(&email("a@b.example", subject));
        assert_eq!(category("Sales report Q3"), Category::Personal);
        assert_eq!(category("Big SALE today"), Category::Newsletter);
        assert_eq!(category("Ihre Bestellbestätigung"), Category::Shopping);
        assert_eq!(category("Don't miss our offers!"), Category::Newsletter);
        assert_eq!(category("Your sign-in code"), Category::Notification);
    }

    #[test]
    fn rules_are_normalized_unique_and_at_least_100_per_category() {
        for (category, rules) in [
            (Category::Social, &rules::SOCIAL),
            (Category::Newsletter, &rules::NEWSLETTER),
            (Category::Shopping, &rules::SHOPPING),
            (Category::Notification, &rules::NOTIFICATION),
        ] {
            let all: Vec<&str> =
                [rules.domains, rules.senders, rules.words].concat();
            let unique: HashSet<&str> = all.iter().copied().collect();
            assert!(unique.len() >= 100, "{category:?}: {}", unique.len());
            for list in [rules.domains, rules.senders, rules.words] {
                let set: HashSet<_> = list.iter().collect();
                assert_eq!(set.len(), list.len(), "{category:?}: duplicates");
            }
            for &entry in &all {
                assert_eq!(entry, entry.to_lowercase(), "{entry}");
            }
            for &word in rules.words {
                let phrase = word.strip_suffix('*').unwrap_or(word);
                assert_eq!(normalize(phrase), phrase, "{word}");
            }
        }
    }

    #[test]
    fn treats_emails_without_sender_as_personal() {
        assert_eq!(Category::of(&Email::default()), Category::Personal);
    }

    #[test]
    fn names_match_serde() {
        for category in Category::ALL {
            let json = serde_json::to_string(&category)
                .expect("category should serialize");
            assert_eq!(json, format!("\"{}\"", category.name()));
        }
    }
}
