// SPDX-License-Identifier: MIT OR Apache-2.0

//! Sorts emails into coarse categories by their headers.
//!
//! The rules only look at the sender, the subject, and a few headers that
//! mailing lists and programs add, never at the content. They are simple
//! on purpose: for statistics, a few emails in the wrong category are
//! fine. The first rule that matches wins, in the order of [`Category`].

use serde::{Deserialize, Serialize};

use crate::email::Email;

/// Domains of social networks, also matching their subdomains.
const SOCIAL_DOMAINS: &[&str] = &[
    "discord.com",
    "facebook.com",
    "facebookmail.com",
    "instagram.com",
    "linkedin.com",
    "pinterest.com",
    "reddit.com",
    "redditmail.com",
    "tiktok.com",
    "twitter.com",
    "x.com",
    "xing.com",
    "youtube.com",
];

/// Starts of words in subjects of orders, invoices, and deliveries, in
/// English and German, in lowercase.
const SHOPPING_WORDS: &[&str] = &[
    "bestell",
    "delivery",
    "invoice",
    "lieferung",
    "order",
    "payment",
    "purchase",
    "quittung",
    "receipt",
    "rechnung",
    "sendung",
    "shipment",
    "shipped",
    "versand",
    "zahlung",
];

/// Local parts of addresses that programs send from, in lowercase.
const AUTOMATIC_SENDERS: &[&str] = &[
    "donotreply",
    "do-not-reply",
    "mailer-daemon",
    "no-reply",
    "noreply",
    "notification",
    "notifications",
    "notify",
    "postmaster",
];

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
        if SOCIAL_DOMAINS
            .iter()
            .any(|&social| is_domain_of(domain, social))
        {
            Self::Social
        } else if email.list
            || matches!(
                email.precedence.as_deref(),
                Some("bulk" | "list" | "junk")
            )
        {
            Self::Newsletter
        } else if has_shopping_word(&email.subject) {
            Self::Shopping
        } else if email.auto_submitted.as_deref().is_some_and(|v| v != "no")
            || AUTOMATIC_SENDERS.contains(&base_local_part(local_part))
        {
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

/// Checks whether a word of a subject starts with a shopping word.
///
/// E.g. "Versandbestätigung" and "orders" match, but "Border" does not.
///
/// # Arguments
///
/// - `subject`: The subject of an email.
fn has_shopping_word(subject: &str) -> bool {
    subject
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| SHOPPING_WORDS.iter().any(|&w| word.starts_with(w)))
}

/// Strips a tag, e.g. returns `noreply` of `noreply+news`.
///
/// # Arguments
///
/// - `local_part`: The part of an address before the `@`.
fn base_local_part(local_part: &str) -> &str {
    AUTOMATIC_SENDERS
        .iter()
        .copied()
        .find(|&sender| {
            local_part.strip_prefix(sender).is_some_and(|rest| {
                rest.is_empty() || rest.starts_with(['+', '-', '.', '_'])
            })
        })
        .unwrap_or(local_part)
}

#[cfg(test)]
mod tests {
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
        let mut auto = email("robot@example.com", "Backup done");
        auto.auto_submitted = Some("auto-generated".into());
        assert_eq!(Category::of(&auto), Category::Notification);
        auto.auto_submitted = Some("no".into());
        assert_eq!(Category::of(&auto), Category::Personal);
        let person = email("noreplyfan@example.com", "Hi");
        assert_eq!(Category::of(&person), Category::Personal);
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
