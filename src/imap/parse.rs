// SPDX-License-Identifier: MIT OR Apache-2.0

//! Turns the tokens of server responses into folders, numbers, and
//! headers.

use anyhow::{Context, bail};

use crate::imap::response::Token;
use crate::imap::{FetchedHeader, Folder};

/// Error messages quote at most this many characters of a response.
const MAX_DESCRIPTION: usize = 200;

/// Checks whether a response is untagged and of the given kind.
///
/// # Arguments
///
/// - `tokens`: The tokens of the response.
/// - `kind`: The kind, e.g. `OK` or `LIST`, in any case.
pub(super) fn is_untagged(tokens: &[Token], kind: &str) -> bool {
    tokens.first() == Some(&Token::Atom("*".into()))
        && is_status(&tokens[1..], kind)
}

/// Checks whether tokens start with the given status or kind.
///
/// # Arguments
///
/// - `tokens`: The tokens of a response after its tag.
/// - `status`: The status, e.g. `OK`, in any case.
pub(super) fn is_status(tokens: &[Token], status: &str) -> bool {
    matches!(
        tokens.first(),
        Some(Token::Atom(atom)) if atom.eq_ignore_ascii_case(status)
    )
}

/// Parses `* <n> EXISTS`, the number of emails in a folder.
///
/// Returns `None` for other responses.
///
/// # Arguments
///
/// - `tokens`: The tokens of an untagged response.
pub(super) fn parse_exists(tokens: &[Token]) -> anyhow::Result<Option<u32>> {
    match tokens {
        [Token::Atom(star), Token::Atom(count), Token::Atom(kind), ..]
            if star == "*" && kind.eq_ignore_ascii_case("EXISTS") =>
        {
            parse_number(count).map(Some)
        }
        _ => Ok(None),
    }
}

/// Parses `* OK [UIDVALIDITY <n>]`, the UIDVALIDITY of a folder.
///
/// Returns `None` for other responses.
///
/// # Arguments
///
/// - `tokens`: The tokens of an untagged response.
pub(super) fn parse_uidvalidity(
    tokens: &[Token],
) -> anyhow::Result<Option<u32>> {
    match tokens {
        [
            Token::Atom(star),
            Token::Atom(ok),
            Token::Atom(code),
            Token::Atom(value),
            ..,
        ] if star == "*"
            && ok.eq_ignore_ascii_case("OK")
            && code.eq_ignore_ascii_case("[UIDVALIDITY") =>
        {
            parse_number(value.strip_suffix(']').unwrap_or(value)).map(Some)
        }
        _ => Ok(None),
    }
}

/// Parses `* SEARCH <uid> ...`, the UIDs of a search.
///
/// # Arguments
///
/// - `tokens`: The tokens of the response.
pub(super) fn parse_search(tokens: &[Token]) -> anyhow::Result<Vec<u32>> {
    tokens
        .iter()
        .skip(2)
        .map(|token| match token {
            Token::Atom(uid) => parse_number(uid),
            _ => bail!("malformed SEARCH response"),
        })
        .collect()
}

/// Parses `* LIST (<attributes>) <delimiter> <name>`.
///
/// # Arguments
///
/// - `tokens`: The tokens of the response.
pub(super) fn parse_list(tokens: Vec<Token>) -> anyhow::Result<Folder> {
    let mut rest = tokens.into_iter().skip(2);
    if rest.next() != Some(Token::Open) {
        bail!("malformed LIST response");
    }
    let mut attributes = Vec::new();
    loop {
        match rest.next() {
            Some(Token::Close) => break,
            Some(Token::Atom(attribute)) => {
                attributes.push(attribute.to_ascii_lowercase());
            }
            _ => bail!("malformed LIST response"),
        }
    }
    let _delimiter = rest.next();
    let name = match rest.next() {
        Some(Token::Atom(name) | Token::Quoted(name)) => name,
        Some(Token::Literal(name)) => {
            String::from_utf8(name).context("folder name is no UTF-8")?
        }
        _ => bail!("malformed LIST response"),
    };
    Ok(Folder { name, attributes })
}

/// Parses `* <n> FETCH (<item> <value> ...)` into a header.
///
/// Returns `None` if the response has no UID or no header, e.g. an update
/// of the flags of an email.
///
/// # Arguments
///
/// - `tokens`: The tokens of the response.
pub(super) fn parse_fetch(
    tokens: Vec<Token>,
) -> anyhow::Result<Option<FetchedHeader>> {
    let mut items = tokens.into_iter().skip(3);
    if items.next() != Some(Token::Open) {
        bail!("malformed FETCH response");
    }
    let mut uid = None;
    let mut internaldate = None;
    let mut header = None;
    loop {
        let key = match items.next() {
            Some(Token::Close) => break,
            Some(Token::Atom(key)) => key.to_ascii_uppercase(),
            _ => bail!("malformed FETCH response"),
        };
        match (key.as_str(), items.next()) {
            ("UID", Some(Token::Atom(value))) => {
                uid = Some(parse_number(&value)?);
            }
            ("INTERNALDATE", Some(Token::Quoted(value))) => {
                internaldate = Some(value);
            }
            ("BODY[HEADER]", Some(Token::Literal(value))) => {
                header = Some(value);
            }
            ("BODY[HEADER]", Some(Token::Quoted(value))) => {
                header = Some(value.into_bytes());
            }
            // A list, e.g. of FLAGS, which the server may add.
            (_, Some(Token::Open)) => skip_list(&mut items)?,
            (_, Some(_)) => {}
            (_, None) => bail!("malformed FETCH response"),
        }
    }
    Ok(uid.zip(header).map(|(uid, header)| FetchedHeader {
        uid,
        internaldate,
        header,
    }))
}

/// Describes a status response for an error message.
///
/// Literals are left out, as they could contain data of an email.
///
/// # Arguments
///
/// - `tokens`: The tokens to describe.
pub(super) fn describe(tokens: &[Token]) -> String {
    let words: Vec<_> = tokens
        .iter()
        .map(|token| match token {
            Token::Atom(text) => text.clone(),
            Token::Quoted(text) => format!("{text:?}"),
            Token::Literal(bytes) => format!("<{} bytes>", bytes.len()),
            Token::Open => "(".into(),
            Token::Close => ")".into(),
        })
        .collect();
    words.join(" ").chars().take(MAX_DESCRIPTION).collect()
}

/// Parses a number that the server sent, e.g. a UID.
///
/// # Arguments
///
/// - `text`: The digits.
fn parse_number(text: &str) -> anyhow::Result<u32> {
    text.parse()
        .with_context(|| format!("invalid number {text:?} from the server"))
}

/// Skips the rest of a parenthesized list, nested lists included.
///
/// # Arguments
///
/// - `tokens`: The tokens after the `(` of the list.
fn skip_list(tokens: &mut impl Iterator<Item = Token>) -> anyhow::Result<()> {
    let mut depth = 1;
    while depth > 0 {
        match tokens.next() {
            Some(Token::Open) => depth += 1,
            Some(Token::Close) => depth -= 1,
            Some(_) => {}
            None => bail!("unbalanced parentheses in a response"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imap::response::read_response;

    /// Reads the tokens of one response.
    ///
    /// # Arguments
    ///
    /// - `response`: The response, with its line ending.
    fn tokens(response: &[u8]) -> Vec<Token> {
        read_response(&mut &response[..]).expect("response should be valid")
    }

    #[test]
    fn parses_folder_names_in_all_forms() {
        for (response, name) in [
            (&b"* LIST () \"/\" INBOX\r\n"[..], "INBOX"),
            (b"* LIST () \"/\" \"Sent Items\"\r\n", "Sent Items"),
            (b"* LIST () NIL {4}\r\nWork\r\n", "Work"),
        ] {
            let folder =
                parse_list(tokens(response)).expect("LIST should parse");
            assert_eq!(folder.name, name);
        }
    }

    #[test]
    fn rejects_malformed_list() {
        let error = parse_list(tokens(b"* LIST INBOX\r\n"))
            .expect_err("LIST should be invalid");
        assert_eq!(error.to_string(), "malformed LIST response");
    }

    #[test]
    fn parses_exists_and_uidvalidity() {
        let exists = tokens(b"* 172 EXISTS\r\n");
        let uidvalidity = tokens(b"* OK [UIDVALIDITY 42] UIDs valid\r\n");
        let other = tokens(b"* OK [UIDNEXT 7] next\r\n");
        let parsed = |parse: fn(&[Token]) -> anyhow::Result<Option<u32>>| {
            [&exists, &uidvalidity, &other]
                .map(|t| parse(t).expect("response should parse"))
        };
        assert_eq!(parsed(parse_exists), [Some(172), None, None]);
        assert_eq!(parsed(parse_uidvalidity), [None, Some(42), None]);
    }

    #[test]
    fn parses_search() {
        let uids = parse_search(&tokens(b"* SEARCH 3 17\r\n"))
            .expect("SEARCH should parse");
        assert_eq!(uids, [3, 17]);
        parse_search(&tokens(b"* SEARCH (x)\r\n"))
            .expect_err("SEARCH should be invalid");
    }

    #[test]
    fn ignores_fetch_responses_without_header() {
        let header =
            parse_fetch(tokens(b"* 3 FETCH (UID 10 FLAGS (\\Seen))\r\n"))
                .expect("FETCH should parse");
        assert_eq!(header, None);
    }

    #[test]
    fn describes_responses_without_literals() {
        let tokens =
            [Token::Atom("NO".into()), Token::Literal(b"secret".to_vec())];
        assert_eq!(describe(&tokens), "NO <6 bytes>");
    }
}
