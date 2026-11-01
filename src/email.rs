// SPDX-License-Identifier: MIT OR Apache-2.0

//! The headers of an email that the statistics need.

use jiff::fmt::strtime;
use mailparse::{MailAddr, MailHeader, MailHeaderMap, addrparse_header};

/// Format of `INTERNALDATE` (RFC 3501, section 9), with its offset.
const INTERNALDATE_FORMAT: &str = "%d-%b-%Y %H:%M:%S %z";

/// The sender of an email.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sender {
    /// The address, in lowercase.
    pub address: String,
    /// The display name, e.g. `Jane Doe` of `Jane Doe <jane@example.com>`.
    pub name: Option<String>,
}

/// The parsed headers of an email. Parsing never fails: missing or broken
/// headers are `None` or empty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Email {
    /// Without the angle brackets.
    pub message_id: Option<String>,
    /// The first address of `From`.
    pub sender: Option<Sender>,
    /// `Date` as Unix timestamp.
    pub date: Option<i64>,
    pub subject: String,
    /// Whether a mailing list or newsletter sent the email: it has a
    /// `List-Id` or `List-Unsubscribe` header (RFC 2919, RFC 2369).
    pub list: bool,
    /// `Precedence`, in lowercase, e.g. `bulk` (RFC 3834, section 2).
    pub precedence: Option<String>,
    /// `Auto-Submitted`, in lowercase, e.g. `auto-generated` (RFC 3834).
    pub auto_submitted: Option<String>,
}

impl Email {
    /// Parses a raw header.
    ///
    /// # Arguments
    ///
    /// - `header`: The raw header, as fetched.
    #[must_use]
    pub fn parse(header: &[u8]) -> Self {
        let Ok((headers, _)) = mailparse::parse_headers(header) else {
            return Self::default();
        };
        let value = |key: &str| {
            headers
                .get_first_value(key)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let lowercase = |key: &str| value(key).map(|v| v.to_lowercase());
        Self {
            message_id: value("Message-ID").map(|id| {
                id.trim_start_matches('<').trim_end_matches('>').to_owned()
            }),
            sender: headers.get_first_header("From").and_then(sender),
            date: value("Date").and_then(|date| parse_date(&date)),
            subject: value("Subject").unwrap_or_default(),
            list: value("List-Id").is_some()
                || value("List-Unsubscribe").is_some(),
            precedence: lowercase("Precedence"),
            auto_submitted: lowercase("Auto-Submitted"),
        }
    }
}

/// Returns the first address of a `From` header.
///
/// # Arguments
///
/// - `header`: The `From` header.
fn sender(header: &MailHeader<'_>) -> Option<Sender> {
    let addresses = addrparse_header(header).ok()?.into_inner();
    let single = addresses.into_iter().find_map(|address| match address {
        MailAddr::Single(single) => Some(single),
        MailAddr::Group(group) => group.addrs.into_iter().next(),
    })?;
    let address = single.addr.trim().to_lowercase();
    if address.is_empty() {
        return None;
    }
    Some(Sender {
        address,
        name: single
            .display_name
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty()),
    })
}

/// Parses a date as in RFC 5322 into a Unix timestamp.
///
/// # Arguments
///
/// - `date`: The date, e.g. `Mon, 1 Jan 2024 10:00:00 +0000`.
fn parse_date(date: &str) -> Option<i64> {
    // mailparse returns 0 instead of an error for text without a date.
    mailparse::dateparse(date)
        .ok()
        .filter(|&timestamp| timestamp != 0)
}

/// Parses the arrival time of an email on the server into a Unix timestamp.
///
/// # Arguments
///
/// - `internaldate`: The arrival time as IMAP reports it in `INTERNALDATE`,
///   e.g. `17-Jul-1996 02:44:25 -0700`.
#[must_use]
pub fn parse_internaldate(internaldate: &str) -> Option<i64> {
    let parsed =
        strtime::parse(INTERNALDATE_FORMAT, internaldate.trim()).ok()?;
    Some(parsed.to_timestamp().ok()?.as_second())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2024-01-01 10:00:00 UTC
    const NEW_YEAR: i64 = 1_704_103_200;

    /// Parses a header.
    ///
    /// # Arguments
    ///
    /// - `header`: The raw header.
    fn parse(header: &str) -> Email {
        Email::parse(header.as_bytes())
    }

    #[test]
    fn parses_typical_header() {
        let email = parse(
            "Message-ID: <abc@example.com>\r\n\
             From: \"Doe, Jane\" <Jane.Doe@Example.com>\r\n\
             To: me@example.org\r\n\
             Subject: Hello\r\n\
             Date: Mon, 1 Jan 2024 11:00:00 +0100\r\n\
             \r\n",
        );
        assert_eq!(
            email,
            Email {
                message_id: Some("abc@example.com".into()),
                sender: Some(Sender {
                    address: "jane.doe@example.com".into(),
                    name: Some("Doe, Jane".into()),
                }),
                date: Some(NEW_YEAR),
                subject: "Hello".into(),
                ..Email::default()
            }
        );
    }

    #[test]
    fn decodes_encoded_words_and_folded_lines() {
        let email = parse(
            "From: =?ISO-8859-1?Q?J=FCrgen_M=FCller?=\r\n <j@example.com>\r\n\
             Subject: =?UTF-8?B?R3LDvMOfZQ==?=\r\n\
             \r\n",
        );
        let sender = email.sender.expect("sender should be parsed");
        assert_eq!(sender.name.as_deref(), Some("Jürgen Müller"));
        assert_eq!(sender.address, "j@example.com");
        assert_eq!(email.subject, "Grüße");
    }

    #[test]
    fn takes_the_first_of_several_senders() {
        let email = parse("From: a@example.com, b@example.com\r\n\r\n");
        assert_eq!(
            email.sender.map(|s| s.address).as_deref(),
            Some("a@example.com")
        );
    }

    #[test]
    fn reads_list_and_automation_headers() {
        let email = parse(
            "List-Unsubscribe: <mailto:u@example.com>\r\n\
             Precedence: Bulk\r\n\
             Auto-Submitted: Auto-Generated\r\n\
             \r\n",
        );
        assert!(email.list);
        assert_eq!(email.precedence.as_deref(), Some("bulk"));
        assert_eq!(email.auto_submitted.as_deref(), Some("auto-generated"));
        assert!(parse("List-Id: <news.example.com>\r\n\r\n").list);
    }

    #[test]
    fn leaves_missing_and_broken_headers_empty() {
        assert_eq!(parse("\r\n"), Email::default());
        let email = parse(
            "From: not an address\r\nDate: yesterday\r\nMessage-ID:\r\n\r\n",
        );
        assert_eq!(email.sender, None);
        assert_eq!(email.date, None);
        assert_eq!(email.message_id, None);
    }

    #[test]
    fn parses_internaldate() {
        assert_eq!(
            parse_internaldate("01-Jan-2024 11:00:00 +0100"),
            Some(NEW_YEAR)
        );
        assert_eq!(
            parse_internaldate(" 1-Jan-2024 10:00:00 +0000"),
            Some(NEW_YEAR)
        );
        assert_eq!(
            parse_internaldate("17-Jul-1996 02:44:25 -0700"),
            Some(837_596_665)
        );
        assert_eq!(parse_internaldate("garbage"), None);
    }
}
