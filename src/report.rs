// SPDX-License-Identifier: MIT OR Apache-2.0

//! The report: a Markdown file with statistics of the index, and SVG
//! charts next to it.

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;

use anyhow::Context;
use jiff::Timestamp;

use crate::category::Category;
use crate::config::Source;
use crate::index::Index;
use crate::report::contacts::Contact;
use crate::report::markdown::{count, escape, list, paragraph, table};
use crate::svg::{self, number};

mod contacts;
mod markdown;

pub const REPORT_FILE: &str = "report.md";
const YEARS_CHART: &str = "emails-per-year.svg";
const CATEGORIES_CHART: &str = "emails-per-category.svg";
const SENDERS_CHART: &str = "top-senders.svg";
const RECIPIENTS_CHART: &str = "top-recipients.svg";
/// Format of the time of the report: RFC 3339 in UTC, with a space
/// instead of the `T`, which RFC 3339 allows for readability, so that
/// people and programs can read it, e.g. `2026-11-01 13:45:07Z`.
const GENERATED_FORMAT: &str = "%Y-%m-%d %H:%M:%SZ";
/// Senders or recipients in a chart. More would make it too high to read.
const CHART_SENDERS: usize = 20;

/// Writes the report and its charts into a directory.
///
/// # Arguments
///
/// - `index`: The index of all emails.
/// - `sources`: The email accounts of the config.
/// - `generated`: The time of the report.
/// - `dir`: The directory, which is created if needed.
/// - `top`: The number of senders and recipients in each list of top
///   ones.
pub fn write(
    index: &Index,
    sources: &[Source],
    generated: Timestamp,
    dir: &Path,
    top: usize,
) -> anyhow::Result<()> {
    fs::create_dir_all(dir)
        .with_context(|| format!("failed to create {}", dir.display()))?;
    let report = render(index, sources, generated, top);
    for (file, content) in report.files {
        let path = dir.join(file);
        fs::write(&path, content)
            .with_context(|| format!("failed to write {}", path.display()))?;
    }
    Ok(())
}

/// The files of a report, as `(file name, content)`.
#[derive(Debug)]
struct Report {
    files: Vec<(&'static str, String)>,
}

/// Renders the report and its charts.
///
/// # Arguments
///
/// - `index`: The index of all emails.
/// - `sources`: The email accounts of the config.
/// - `generated`: The time of the report.
/// - `top`: The number of senders and recipients in each list of top
///   ones.
fn render(
    index: &Index,
    sources: &[Source],
    generated: Timestamp,
    top: usize,
) -> Report {
    let total = index.emails.len();
    let own: HashSet<String> =
        sources.iter().flat_map(Source::own_addresses).collect();
    let senders = contacts::senders(index);
    let recipients = contacts::recipients(index, &own);
    let per_year = count_years(index);
    let per_category = count_categories(index);
    let mut markdown = vec!["# Email statistics".to_owned(), String::new()];
    markdown.extend(paragraph(&format!(
        "{}, each counted once, also if it is in several folders or \
         sources. Generated on {} by [imap-statistics]({}) {}.",
        count(total as u64, "email", "emails"),
        generated.strftime(GENERATED_FORMAT),
        env!("CARGO_PKG_REPOSITORY"),
        env!("CARGO_PKG_VERSION")
    )));
    markdown.extend(overview(
        index,
        &per_year,
        &per_category,
        &senders,
        &recipients,
    ));
    markdown.extend(accounts(index, sources));
    let mut files = Vec::new();
    // Keeps the chart of a section and returns its lines.
    let mut charted = |(lines, chart): (Vec<String>, _)| {
        files.push(chart);
        lines
    };
    markdown.extend(charted(years(index, &per_year)));
    markdown.extend(charted(categories(&per_category)));
    markdown.extend(charted(top_senders(index, &senders, top)));
    markdown.extend(top_per_year(
        "Top senders per year",
        "Sender",
        &senders,
        top,
        true,
    ));
    markdown.extend(charted(top_recipients(&recipients, top)));
    markdown.extend(top_per_year(
        "Top recipients per year",
        "Recipient",
        &recipients,
        top,
        false,
    ));
    markdown.extend(all_senders(&senders));
    markdown.extend(all_recipients(&recipients));
    files.push((REPORT_FILE, markdown.join("\n")));
    Report { files }
}

/// Returns the section with the key numbers of the other sections.
///
/// # Arguments
///
/// - `index`: The index of all emails.
/// - `per_year`: The received and sent emails per year.
/// - `per_category`: The received emails per category, the most first.
/// - `senders`: The senders of received emails, the most emails first.
/// - `recipients`: The recipients of sent emails, the most emails first.
fn overview(
    index: &Index,
    per_year: &BTreeMap<i64, [u64; 2]>,
    per_category: &[(Category, u64)],
    senders: &[(&str, Contact)],
    recipients: &[(&str, Contact)],
) -> Vec<String> {
    let mut items = Vec::new();
    if let (Some(first), Some(last)) =
        (per_year.keys().next(), per_year.keys().next_back())
    {
        items.push(format!("Period: {first} to {last}."));
    }
    let sent = index.emails.iter().filter(|e| e.sent).count() as u64;
    items.push(format!(
        "Emails: {} received and {} sent.",
        number(index.emails.len() as u64 - sent),
        number(sent)
    ));
    // The earliest year on ties.
    let busiest = per_year
        .iter()
        .max_by_key(|&(year, counts)| (counts[0] + counts[1], Reverse(year)));
    if let Some((year, [received, sent])) = busiest {
        items.push(format!(
            "Busiest year: {year}, with {}.",
            count(received + sent, "email", "emails")
        ));
    }
    let received: u64 = per_category.iter().map(|&(_, count)| count).sum();
    if let Some(&(category, emails)) = per_category.first()
        && emails > 0
    {
        items.push(format!(
            "Largest category: {}, with {}% of the received emails.",
            category.name(),
            emails * 100 / received
        ));
    }
    if let Some((address, sender)) = senders.first() {
        items.push(format!(
            "Top sender: {}, with {}.",
            escape(address),
            count(sender.emails, "email", "emails")
        ));
    }
    if let Some((address, recipient)) = recipients.first() {
        items.push(format!(
            "Top recipient: {}, with {}.",
            escape(address),
            count(recipient.emails, "email", "emails")
        ));
    }
    let mut lines = vec!["## Overview".to_owned(), String::new()];
    lines.extend(list(&items));
    lines
}

/// Returns the section with the email accounts and their emails.
///
/// # Arguments
///
/// - `index`: The index of all emails.
/// - `sources`: The email accounts of the config.
fn accounts(index: &Index, sources: &[Source]) -> Vec<String> {
    let mut lines = vec!["## Accounts".to_owned(), String::new()];
    lines.extend(table(
        &["Account", "Address", "Server", "Received", "Sent"],
        &[3, 4],
        sources.iter().map(|source| {
            let (mut received, mut sent) = (0, 0);
            for email in &index.emails {
                if email.sources.contains(&source.name) {
                    if email.sent {
                        sent += 1;
                    } else {
                        received += 1;
                    }
                }
            }
            vec![
                escape(&source.name),
                escape(&source.username),
                escape(&format!("{}:{}", source.host, source.port)),
                number(received),
                number(sent),
            ]
        }),
    ));
    lines.extend(paragraph(
        "An email in several accounts counts once in the other sections, \
         but for each account here. An email counts as sent if it came from \
         an address of an account, or is in a Sent folder.",
    ));
    lines
}

/// Returns the received and sent emails per year.
///
/// # Arguments
///
/// - `index`: The index of all emails.
fn count_years(index: &Index) -> BTreeMap<i64, [u64; 2]> {
    let mut counts: BTreeMap<i64, [u64; 2]> = BTreeMap::new();
    for email in &index.emails {
        if let Some(year) = email.year {
            counts.entry(year).or_default()[usize::from(email.sent)] += 1;
        }
    }
    counts
}

/// Returns the section with the emails per year, and its chart.
///
/// # Arguments
///
/// - `index`: The index of all emails.
/// - `counts`: The received and sent emails per year.
fn years(
    index: &Index,
    counts: &BTreeMap<i64, [u64; 2]>,
) -> (Vec<String>, (&'static str, String)) {
    // Every year from the first to the last, so that gaps show.
    let data: Vec<_> = match (counts.keys().next(), counts.keys().next_back()) {
        (Some(&first), Some(&last)) => (first..=last)
            .map(|year| {
                let [received, sent] =
                    counts.get(&year).copied().unwrap_or_default();
                (year.to_string(), vec![received, sent])
            })
            .collect(),
        _ => Vec::new(),
    };
    let dated: u64 = counts.values().flatten().sum();
    let undated = index.emails.len() as u64 - dated;
    let mut lines = vec![
        "## Emails per year".to_owned(),
        String::new(),
        format!("![Emails per year]({YEARS_CHART})"),
        String::new(),
    ];
    lines.extend(table(
        &["Year", "Received", "Sent"],
        &[1, 2],
        data.iter().map(|(year, counts)| {
            vec![year.clone(), number(counts[0]), number(counts[1])]
        }),
    ));
    if undated > 0 {
        lines.extend(paragraph(&format!(
            "Left out for lack of a usable date: {}.",
            count(undated, "email", "emails")
        )));
    }
    let chart = svg::columns("Emails per year", &["Received", "Sent"], &data);
    (lines, (YEARS_CHART, chart))
}

/// Returns the received emails per category, the most first.
///
/// Sent emails are by the owner, so their category says little.
///
/// # Arguments
///
/// - `index`: The index of all emails.
fn count_categories(index: &Index) -> Vec<(Category, u64)> {
    let mut counts: BTreeMap<Category, u64> = Category::ALL
        .iter()
        .map(|&category| (category, 0))
        .collect();
    for email in index.emails.iter().filter(|e| !e.sent) {
        *counts.entry(email.category).or_default() += 1;
    }
    let mut counts: Vec<_> = counts.into_iter().collect();
    // The sort is stable, so ties keep the order of the rules.
    counts.sort_by_key(|&(_, count)| Reverse(count));
    counts
}

/// Returns the section with the emails per category, and its chart.
///
/// # Arguments
///
/// - `counts`: The received emails per category, the most first.
fn categories(
    counts: &[(Category, u64)],
) -> (Vec<String>, (&'static str, String)) {
    let total = counts.iter().map(|&(_, count)| count).sum::<u64>().max(1);
    let mut lines = vec![
        "## Emails per category".to_owned(),
        String::new(),
        format!("![Emails per category]({CATEGORIES_CHART})"),
        String::new(),
    ];
    lines.extend(table(
        &["Category", "Emails", "Share", "Description"],
        &[1, 2],
        counts.iter().map(|&(category, count)| {
            vec![
                category.name().to_owned(),
                number(count),
                format!("{}%", count * 100 / total),
                category.description().to_owned(),
            ]
        }),
    ));
    lines.extend(paragraph(
        "Only received emails count. The categories come from simple rules \
         on the headers, so a few emails are in the wrong category.",
    ));
    let data: Vec<_> = counts
        .iter()
        .map(|&(category, count)| (category.name().to_owned(), count))
        .collect();
    let chart = svg::bars("Emails per category", &data);
    (lines, (CATEGORIES_CHART, chart))
}

/// Returns the section with the top senders, and its chart.
///
/// # Arguments
///
/// - `index`: The index of all emails.
/// - `senders`: The senders of received emails, the most emails first.
/// - `top`: The number of senders in the table.
fn top_senders(
    index: &Index,
    senders: &[(&str, Contact)],
    top: usize,
) -> (Vec<String>, (&'static str, String)) {
    let received = index.emails.iter().filter(|e| !e.sent).count() as u64;
    let without = received - senders.iter().map(|(_, s)| s.emails).sum::<u64>();
    let shown = &senders[..top.min(senders.len())];
    let share: u64 = shown.iter().map(|(_, s)| s.emails).sum::<u64>() * 100
        / received.max(1);

    let mut lines = vec![
        "## Top senders".to_owned(),
        String::new(),
        format!("![Top senders]({SENDERS_CHART})"),
        String::new(),
    ];
    lines.extend(paragraph(&format!(
        "{} in received emails. The top {} sent {share}% of all received \
         emails. The name is the one that the sender used most often.",
        count(senders.len() as u64, "sender address", "sender addresses"),
        number(shown.len() as u64),
    )));
    lines.extend(table(
        &["#", "Sender", "Name", "Emails"],
        &[0, 3],
        shown.iter().enumerate().map(|(rank, (address, sender))| {
            vec![
                (rank + 1).to_string(),
                escape(address),
                escape(sender.name()),
                number(sender.emails),
            ]
        }),
    ));
    if without > 0 {
        lines.extend(paragraph(&format!(
            "Left out for lack of a usable sender: {}.",
            count(without, "email", "emails")
        )));
    }
    let title = format!("Top {} senders", senders.len().min(CHART_SENDERS));
    (
        lines,
        (SENDERS_CHART, svg::bars(&title, &chart_data(senders))),
    )
}

/// Returns the section with the top recipients of sent emails, and its
/// chart.
///
/// # Arguments
///
/// - `recipients`: The recipients of sent emails, the most emails first.
/// - `top`: The number of recipients in the table.
fn top_recipients(
    recipients: &[(&str, Contact)],
    top: usize,
) -> (Vec<String>, (&'static str, String)) {
    let shown = &recipients[..top.min(recipients.len())];
    let mut lines = vec![
        "## Top recipients".to_owned(),
        String::new(),
        format!("![Top recipients]({RECIPIENTS_CHART})"),
        String::new(),
    ];
    lines.extend(paragraph(&format!(
        "{} in sent emails, from To, Cc, and Bcc. An email to several \
         recipients counts for each of them. Own addresses do not count.",
        count(
            recipients.len() as u64,
            "recipient address",
            "recipient addresses"
        ),
    )));
    lines.extend(table(
        &["#", "Recipient", "Emails"],
        &[0, 2],
        shown
            .iter()
            .enumerate()
            .map(|(rank, (address, recipient))| {
                vec![
                    (rank + 1).to_string(),
                    escape(address),
                    number(recipient.emails),
                ]
            }),
    ));
    let title =
        format!("Top {} recipients", recipients.len().min(CHART_SENDERS));
    (
        lines,
        (RECIPIENTS_CHART, svg::bars(&title, &chart_data(recipients))),
    )
}

/// Returns a section with the top contacts of each year, the newest first.
///
/// # Arguments
///
/// - `title`: The title of the section.
/// - `column`: The title of the column with the addresses.
/// - `contacts`: The contacts, the most emails first.
/// - `top`: The number of contacts per year.
/// - `names`: Whether to show the names of the contacts.
fn top_per_year(
    title: &str,
    column: &str,
    contacts: &[(&str, Contact)],
    top: usize,
    names: bool,
) -> Vec<String> {
    let mut header = vec!["Year", column];
    if names {
        header.push("Name");
    }
    header.push("Emails");
    let mut rows = Vec::new();
    for (year, ranked) in contacts::per_year(contacts).iter().rev() {
        for (rank, (address, contact, emails)) in
            ranked.iter().take(top).enumerate()
        {
            // The year only in its first row, so that the years stand out.
            let year = if rank == 0 {
                year.to_string()
            } else {
                String::new()
            };
            let mut row = vec![year, escape(address)];
            if names {
                row.push(escape(contact.name()));
            }
            row.push(number(*emails));
            rows.push(row);
        }
    }
    let mut lines = vec![format!("## {title}"), String::new()];
    lines.extend(paragraph(&format!(
        "The top {} of each year, the newest year first, with their emails \
         in that year.",
        number(top as u64)
    )));
    lines.extend(table(&header, &[header.len() - 1], rows));
    lines
}

/// Returns the section with all senders of received emails.
///
/// # Arguments
///
/// - `senders`: The senders of received emails, the most emails first.
fn all_senders(senders: &[(&str, Contact)]) -> Vec<String> {
    let mut lines = vec!["## All senders".to_owned(), String::new()];
    lines.extend(paragraph(
        "Every sender of received emails, the most emails first, e.g. to find \
         the ones to clean up. The category is the most common one of the \
         emails of the sender. First and Last are the years of the first and \
         the last email.",
    ));
    lines.extend(table(
        &["#", "Sender", "Name", "Category", "Emails", "First", "Last"],
        &[0, 4, 5, 6],
        senders.iter().enumerate().map(|(rank, (address, sender))| {
            let mut row = vec![
                (rank + 1).to_string(),
                escape(address),
                escape(sender.name()),
                sender
                    .category()
                    .map(Category::name)
                    .unwrap_or_default()
                    .into(),
                number(sender.emails),
            ];
            row.extend(first_and_last(sender));
            row
        }),
    ));
    lines
}

/// Returns the section with all recipients of sent emails.
///
/// # Arguments
///
/// - `recipients`: The recipients of sent emails, the most emails first.
fn all_recipients(recipients: &[(&str, Contact)]) -> Vec<String> {
    let mut lines = vec!["## All recipients".to_owned(), String::new()];
    lines.extend(paragraph(
        "Every recipient of sent emails, the most emails first. First and \
         Last are the years of the first and the last email.",
    ));
    lines.extend(table(
        &["#", "Recipient", "Emails", "First", "Last"],
        &[0, 2, 3, 4],
        recipients
            .iter()
            .enumerate()
            .map(|(rank, (address, recipient))| {
                let mut row = vec![
                    (rank + 1).to_string(),
                    escape(address),
                    number(recipient.emails),
                ];
                row.extend(first_and_last(recipient));
                row
            }),
    ));
    lines
}

/// Returns the cells with the years of the first and the last email of a
/// contact, empty if none of its emails has a year.
///
/// # Arguments
///
/// - `contact`: The contact.
fn first_and_last(contact: &Contact) -> [String; 2] {
    contact
        .period()
        .map(|(first, last)| [first.to_string(), last.to_string()])
        .unwrap_or_default()
}

/// Returns the data of a chart of the first contacts.
///
/// # Arguments
///
/// - `contacts`: The contacts, the most emails first.
fn chart_data(contacts: &[(&str, Contact)]) -> Vec<(String, u64)> {
    contacts
        .iter()
        .take(CHART_SENDERS)
        .map(|(address, contact)| ((*address).to_owned(), contact.emails))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::markdown::MAX_WIDTH;

    /// 2023-11-14 22:13:20 UTC
    const GENERATED: Timestamp = Timestamp::constant(1_700_000_000, 0);
    use crate::category::Category;
    use crate::index::Entry;
    use crate::test_util::TempDir;

    /// Returns an index entry of an email in source `work`.
    ///
    /// # Arguments
    ///
    /// - `year`: The year of the email.
    /// - `sender`: The address of the sender.
    /// - `category`: The category.
    fn entry(year: Option<i64>, sender: &str, category: Category) -> Entry {
        Entry {
            sources: vec!["work".into()],
            year,
            sender: Some(sender.into()),
            sender_name: None,
            category,
            sent: false,
            recipients: Vec::new(),
        }
    }

    /// Returns the content of a file of a report.
    ///
    /// # Arguments
    ///
    /// - `report`: The report.
    /// - `name`: The name of the file.
    fn file<'a>(report: &'a Report, name: &str) -> &'a str {
        report
            .files
            .iter()
            .find(|(file, _)| *file == name)
            .map(|(_, content)| content.as_str())
            .expect("report should have the file")
    }

    /// Returns the cells of the rows of a table, without padding.
    ///
    /// # Arguments
    ///
    /// - `markdown`: The report.
    /// - `first`: The title of the first column of the table.
    fn rows(markdown: &str, first: &str) -> Vec<Vec<String>> {
        let cells = |line: &str| -> Vec<String> {
            let inner =
                line.trim().trim_start_matches('|').trim_end_matches('|');
            inner
                .split('|')
                .map(|cell| cell.trim().to_owned())
                .collect()
        };
        markdown
            .lines()
            .skip_while(|line| {
                cells(line).first().map(String::as_str) != Some(first)
            })
            // The header and the separator.
            .skip(2)
            .take_while(|line| line.starts_with('|'))
            .map(cells)
            .collect()
    }

    /// Many emails make long numbers, which must still wrap at 80 columns.
    /// Tables may be wider.
    #[test]
    fn keeps_text_lines_within_80_columns() {
        // Many emails make long numbers in the paragraphs.
        let emails = (0..12_345)
            .map(|i| {
                entry(
                    Some(2024),
                    &format!("s{}@example.com", i % 9),
                    Category::Personal,
                )
            })
            .collect();
        let report = render(&Index { emails }, &[], GENERATED, 50);
        let markdown = file(&report, REPORT_FILE);
        for line in markdown.lines().filter(|line| !line.starts_with('|')) {
            assert!(line.chars().count() <= MAX_WIDTH, "{line}");
        }
    }

    /// Returns an email account.
    ///
    /// # Arguments
    ///
    /// - `name`: The name of the account.
    fn source(name: &str) -> Source {
        toml::from_str(&format!(
            r#"
            name = "{name}"
            host = "imap.example.com"
            username = "{name}@example.com"
            password = "secret"
            "#
        ))
        .expect("source should parse")
    }

    /// An email in two accounts counts for each, an account without emails
    /// shows zeros, and the password never shows.
    #[test]
    fn lists_the_accounts_with_their_emails() {
        let mut both = entry(Some(2024), "a@example.com", Category::Personal);
        both.sources.push("home".into());
        both.sent = true;
        let index = Index {
            emails: vec![
                entry(Some(2024), "a@example.com", Category::Personal),
                both,
            ],
        };
        let sources = [source("work"), source("home"), source("old")];
        let report = render(&index, &sources, GENERATED, 50);
        let markdown = file(&report, REPORT_FILE);
        assert_eq!(
            rows(markdown, "Account"),
            [
                ["work", "work@example.com", "imap.example.com:993", "1", "1"],
                ["home", "home@example.com", "imap.example.com:993", "0", "1"],
                ["old", "old@example.com", "imap.example.com:993", "0", "0"],
            ]
        );
        assert!(!markdown.contains("secret"), "{markdown}");
    }

    /// The overview names the largest of each section. Ties go to the
    /// earliest year, and the share of a category counts received emails
    /// only.
    #[test]
    fn sums_up_the_key_numbers() {
        // Boring setup: 2 received emails in 2021, 1 in 2023, and 1 sent
        // email in 2023 to b.
        let index = Index {
            emails: vec![
                entry(Some(2021), "a@example.com", Category::Newsletter),
                entry(Some(2021), "a@example.com", Category::Newsletter),
                entry(Some(2023), "c@example.com", Category::Personal),
                Entry {
                    sent: true,
                    recipients: vec!["b@example.com".into()],
                    ..entry(Some(2023), "work@example.com", Category::Personal)
                },
            ],
        };

        let report = render(&index, &[source("work")], GENERATED, 50);

        let markdown = file(&report, REPORT_FILE);
        assert!(
            markdown.contains(
                "## Overview\n\n\
                 - Period: 2021 to 2023.\n\
                 - Emails: 3 received and 1 sent.\n\
                 - Busiest year: 2021, with 2 emails.\n\
                 - Largest category: newsletter, with 66% of the received \
                 emails.\n\
                 - Top sender: a@example.com, with 2 emails.\n\
                 - Top recipient: b@example.com, with 1 email.\n\n"
            ),
            "{markdown}"
        );
    }

    /// Years without emails show as 0, so that gaps are visible. Emails without
    /// a year are left out with a note.
    #[test]
    fn counts_emails_per_year_with_gaps() {
        let mut sent = entry(Some(2023), "me@example.com", Category::Personal);
        sent.sent = true;
        let index = Index {
            emails: vec![
                entry(Some(2021), "a@example.com", Category::Personal),
                entry(Some(2023), "a@example.com", Category::Personal),
                sent,
                entry(None, "a@example.com", Category::Personal),
            ],
        };
        let report = render(&index, &[], GENERATED, 50);
        let markdown = file(&report, REPORT_FILE);
        assert!(
            markdown.contains("4 emails, each counted once"),
            "{markdown}"
        );
        assert!(
            markdown.contains(
                "Generated on 2023-11-14 22:13:20Z by\n[imap-statistics]\
                 (https://github.com/phip1611/imap-statistics)"
            ),
            "{markdown}"
        );
        assert_eq!(
            rows(markdown, "Year"),
            [["2021", "1", "0"], ["2022", "0", "0"], ["2023", "1", "1"]]
        );
        assert!(
            markdown.contains("Left out for lack of a usable date: 1 email."),
            "{markdown}"
        );
        let chart = file(&report, YEARS_CHART);
        assert!(chart.contains("<title>2023, Sent: 1</title>"), "{chart}");
    }

    /// Categories sort by their number of emails, ties in the order of the
    /// rules. Sent emails do not count.
    #[test]
    fn counts_emails_per_category_largest_first() {
        let index = Index {
            emails: vec![
                entry(Some(2024), "a@example.com", Category::Personal),
                entry(Some(2024), "n@example.com", Category::Newsletter),
                entry(Some(2024), "m@example.com", Category::Newsletter),
                entry(Some(2024), "s@example.com", Category::Social),
                // A sent email, which does not count.
                Entry {
                    sent: true,
                    ..entry(Some(2024), "me@example.com", Category::Personal)
                },
            ],
        };
        let report = render(&index, &[], GENERATED, 50);
        let markdown = file(&report, REPORT_FILE);
        assert_eq!(
            rows(markdown, "Category"),
            [
                [
                    "newsletter",
                    "2",
                    "50%",
                    "a newsletter, mailing list, or advertisement",
                ],
                ["social", "1", "25%", "from a social network or dating site"],
                [
                    "personal",
                    "1",
                    "25%",
                    "everything else, mostly from people"
                ],
                [
                    "shopping",
                    "0",
                    "0%",
                    "about an order, payment, or delivery"
                ],
                [
                    "notification",
                    "0",
                    "0%",
                    "sent by a program, e.g. a security code",
                ],
            ]
        );
        let chart = file(&report, CATEGORIES_CHART);
        assert!(chart.contains("<title>newsletter: 2</title>"), "{chart}");
    }

    /// Senders rank by their number of emails, ties by address, with their most
    /// used name. Sent emails and emails without sender do not count.
    #[test]
    fn ranks_the_top_senders() {
        // Boring setup: b has 3 emails, under two names, a and c have 2
        // and 1, and one email has no sender.
        let mut emails = vec![
            entry(Some(2024), "c@example.com", Category::Personal),
            entry(Some(2024), "a@example.com", Category::Personal),
            entry(Some(2024), "a@example.com", Category::Personal),
        ];
        for name in ["Bob", "Bobby", "Bob"] {
            let mut email =
                entry(Some(2024), "b@example.com", Category::Personal);
            email.sender_name = Some(name.into());
            emails.push(email);
        }
        let mut anonymous = entry(Some(2024), "", Category::Personal);
        anonymous.sender = None;
        emails.push(anonymous);
        // A sent email, which does not count.
        let mut sent = entry(Some(2024), "me@example.com", Category::Personal);
        sent.sent = true;
        emails.push(sent);
        let index = Index { emails };

        let report = render(&index, &[], GENERATED, 2);

        let markdown = file(&report, REPORT_FILE);
        // Line breaks of paragraphs do not matter here.
        assert!(
            markdown.replace('\n', " ").contains(
                "3 sender addresses in received emails. The top 2 sent 71% of \
                 all received emails."
            ),
            "{markdown}"
        );
        assert_eq!(
            rows(markdown, "#"),
            [
                ["1", "b@example.com", "Bob", "3"],
                ["2", "a@example.com", "", "2"]
            ]
        );
        assert!(
            markdown.contains("Left out for lack of a usable sender: 1 email."),
            "{markdown}"
        );
        let chart = file(&report, SENDERS_CHART);
        // The chart has more senders than the table.
        assert!(chart.contains("<title>Top 3 senders</title>"), "{chart}");
    }

    /// Each year ranks the senders by their emails in that year, not in
    /// total, and shows only the top ones. Emails without a year count in
    /// no year.
    #[test]
    fn ranks_the_top_senders_per_year() {
        // Boring setup: a has the most emails in total, 3 in 2023 and 1 in
        // 2024. b has 2 in 2024, and c has 1 in 2024 and 1 without a year.
        let emails = [
            (Some(2023), "a"),
            (Some(2023), "a"),
            (Some(2023), "a"),
            (Some(2024), "a"),
            (Some(2024), "b"),
            (Some(2024), "b"),
            (Some(2024), "c"),
            (None, "c"),
        ];
        let emails = emails
            .iter()
            .map(|&(year, sender)| {
                entry(
                    year,
                    &format!("{sender}@example.com"),
                    Category::Personal,
                )
            })
            .collect();

        let report = render(&Index { emails }, &[], GENERATED, 2);

        let markdown = file(&report, REPORT_FILE);
        let section = &markdown[markdown
            .find("## Top senders per year")
            .expect("report should have the section")..];
        assert_eq!(
            rows(section, "Year"),
            [
                // c ties with a in 2024, and is not in the top 2.
                ["2024", "b@example.com", "", "2"],
                ["", "a@example.com", "", "1"],
                ["2023", "a@example.com", "", "3"],
            ]
        );
    }

    /// The complete list shows every sender, with the most common category
    /// of its emails, and the years of its first and last email.
    #[test]
    fn lists_all_senders() {
        // Boring setup: a has a newsletter and a personal email in 2021 and
        // a newsletter in 2024. b has one email without a year.
        let emails = vec![
            entry(Some(2024), "a@example.com", Category::Newsletter),
            entry(Some(2021), "a@example.com", Category::Personal),
            entry(Some(2021), "a@example.com", Category::Newsletter),
            entry(None, "b@example.com", Category::Shopping),
        ];

        let report = render(&Index { emails }, &[], GENERATED, 1);

        let markdown = file(&report, REPORT_FILE);
        let section = &markdown[markdown
            .find("## All senders")
            .expect("report should have the section")..];
        assert_eq!(
            rows(section, "#"),
            [
                ["1", "a@example.com", "", "newsletter", "3", "2021", "2024"],
                ["2", "b@example.com", "", "shopping", "1", "", ""],
            ]
        );
    }

    /// Only recipients of sent emails count, once per email, and never an own
    /// address.
    #[test]
    fn ranks_the_top_recipients_of_sent_emails() {
        // Boring setup: two sent emails, one to a and b, one to a and the
        // own address, and a received email.
        let sent = |recipients: &[&str]| Entry {
            sent: true,
            recipients: recipients.iter().map(|&r| r.to_owned()).collect(),
            ..entry(Some(2024), "work@example.com", Category::Personal)
        };
        let index = Index {
            emails: vec![
                sent(&["a@example.com", "b@example.com"]),
                sent(&["a@example.com", "work@example.com"]),
                entry(Some(2024), "c@example.com", Category::Personal),
            ],
        };

        let report = render(&index, &[source("work")], GENERATED, 50);

        let markdown = file(&report, REPORT_FILE);
        let section = &markdown[markdown
            .find("## Top recipients")
            .expect("report should have the section")..];
        assert_eq!(
            rows(section, "#"),
            [["1", "a@example.com", "2"], ["2", "b@example.com", "1"]]
        );
        let chart = file(&report, RECIPIENTS_CHART);
        assert!(chart.contains("<title>a@example.com: 2</title>"), "{chart}");
        let section = &markdown[markdown
            .find("## All recipients")
            .expect("report should have the section")..];
        assert_eq!(
            rows(section, "#"),
            [
                ["1", "a@example.com", "2", "2024", "2024"],
                ["2", "b@example.com", "1", "2024", "2024"]
            ]
        );
    }

    #[test]
    fn writes_all_files() {
        let dir = TempDir::new("report-write");
        let index = Index {
            emails: vec![entry(
                Some(2024),
                "a@example.com",
                Category::Personal,
            )],
        };
        write(&index, &[], GENERATED, &dir.path().join("report"), 50)
            .expect("report should be written");
        for (name, _) in render(&index, &[], GENERATED, 50).files {
            assert!(dir.path().join("report").join(name).is_file(), "{name}");
        }
    }
}
