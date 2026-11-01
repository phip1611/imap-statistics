// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reads IMAP server responses and splits them into tokens (RFC 3501,
//! sections 7 and 9).
//!
//! Only the syntax that the read-only commands of this crate get back is
//! supported: atoms, numbers, quoted strings, literals, and parentheses.
//! Brackets stay part of atoms, so `BODY[HEADER]` is one token, and
//! `[UIDVALIDITY 42]` are the two tokens `[UIDVALIDITY` and `42]`.

use std::io::{BufRead, Read};
use std::str;

use anyhow::{Context, bail};

/// Longest accepted line, without its literals. `UID SEARCH` returns all
/// UIDs of a folder in one line, with up to 11 bytes per email, so this
/// is enough for more than a million emails per folder.
const MAX_LINE: u64 = 16 * 1024 * 1024;
/// Largest accepted literal. Email headers have a few kilobytes, rarely
/// more than a megabyte. The limit protects against broken servers.
const MAX_LITERAL: usize = 16 * 1024 * 1024;

/// A token of a server response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// An atom or a number, e.g. `OK`, `42`, `\Noselect`, or `NIL`.
    Atom(String),
    /// A quoted string, without the quotes and escapes.
    Quoted(String),
    /// A literal: raw bytes, announced by `{n}` at the end of a line.
    Literal(Vec<u8>),
    /// `(`
    Open,
    /// `)`
    Close,
}

/// Reads one response, including all literals in it.
///
/// # Arguments
///
/// - `reader`: The connection to the server, at the start of a response.
pub fn read_response(reader: &mut impl BufRead) -> anyhow::Result<Vec<Token>> {
    let mut tokens = Vec::new();
    loop {
        let line = read_line(reader)?;
        let Some(length) = tokenize(&line, &mut tokens) else {
            return Ok(tokens);
        };
        if length > MAX_LITERAL {
            bail!("server sent a literal of {length} bytes, more than allowed");
        }
        let mut literal = vec![0; length];
        reader
            .read_exact(&mut literal)
            .context("connection closed by the server in a literal")?;
        tokens.push(Token::Literal(literal));
    }
}

/// Reads one line and strips its line ending, CRLF or LF.
///
/// # Arguments
///
/// - `reader`: The connection to the server, at the start of a line.
fn read_line(reader: &mut impl BufRead) -> anyhow::Result<Vec<u8>> {
    let mut line = Vec::new();
    reader.take(MAX_LINE).read_until(b'\n', &mut line)?;
    if line.pop() != Some(b'\n') {
        if line.len() as u64 >= MAX_LINE - 1 {
            bail!("server sent a line longer than allowed");
        }
        bail!("connection closed by the server");
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    Ok(line)
}

/// Appends the tokens of one line to `tokens`.
///
/// Returns the length of the literal that follows, if the line ends with
/// `{n}`. The response then continues after the literal.
///
/// # Arguments
///
/// - `line`: The line, without its line ending.
/// - `tokens`: The tokens of the response so far.
fn tokenize(line: &[u8], tokens: &mut Vec<Token>) -> Option<usize> {
    let mut rest = line;
    while let Some((&first, tail)) = rest.split_first() {
        match first {
            b' ' => rest = tail,
            b'(' => {
                tokens.push(Token::Open);
                rest = tail;
            }
            b')' => {
                tokens.push(Token::Close);
                rest = tail;
            }
            b'"' => {
                let (text, after) = quoted(tail);
                tokens.push(Token::Quoted(text));
                rest = after;
            }
            _ => {
                let end = rest
                    .iter()
                    .position(|b| matches!(b, b' ' | b'(' | b')'))
                    .unwrap_or(rest.len());
                let (atom, after) = rest.split_at(end);
                if after.is_empty()
                    && let Some(length) = literal_length(atom)
                {
                    return Some(length);
                }
                tokens.push(Token::Atom(String::from_utf8_lossy(atom).into()));
                rest = after;
            }
        }
    }
    None
}

/// Splits a quoted string into its unescaped text and what follows it.
///
/// Lenient: a missing closing quote ends the string at the end of the line.
///
/// # Arguments
///
/// - `rest`: The rest of the line, after the opening quote.
fn quoted(rest: &[u8]) -> (String, &[u8]) {
    let mut text = Vec::new();
    let mut bytes = rest.iter().enumerate();
    while let Some((index, &byte)) = bytes.next() {
        match byte {
            b'"' => {
                let text = String::from_utf8_lossy(&text).into();
                return (text, &rest[index + 1..]);
            }
            b'\\' => {
                if let Some((_, &escaped)) = bytes.next() {
                    text.push(escaped);
                }
            }
            _ => text.push(byte),
        }
    }
    (String::from_utf8_lossy(&text).into(), &[])
}

/// Returns `n` if an atom is `{n}`, which announces a literal.
///
/// # Arguments
///
/// - `atom`: The atom at the end of a line.
fn literal_length(atom: &[u8]) -> Option<usize> {
    let digits = atom.strip_prefix(b"{")?.strip_suffix(b"}")?;
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    str::from_utf8(digits).ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns an atom token.
    ///
    /// # Arguments
    ///
    /// - `text`: The text of the atom.
    fn atom(text: &str) -> Token {
        Token::Atom(text.to_owned())
    }

    /// Returns a quoted string token.
    ///
    /// # Arguments
    ///
    /// - `text`: The unescaped text.
    fn quoted(text: &str) -> Token {
        Token::Quoted(text.to_owned())
    }

    /// Reads one response.
    ///
    /// # Arguments
    ///
    /// - `input`: What the server sends.
    fn read(input: &[u8]) -> anyhow::Result<Vec<Token>> {
        read_response(&mut &input[..])
    }

    /// Returns the error message of an invalid response.
    ///
    /// # Arguments
    ///
    /// - `input`: What the server sends.
    fn error(input: &[u8]) -> String {
        read(input)
            .expect_err("response should be invalid")
            .to_string()
    }

    #[test]
    fn reads_tagged_status() {
        let tokens = read(b"A1 OK LOGIN completed\r\n")
            .expect("response should be valid");
        assert_eq!(
            tokens,
            [atom("A1"), atom("OK"), atom("LOGIN"), atom("completed")]
        );
    }

    #[test]
    fn keeps_brackets_in_atoms() {
        let tokens = read(b"* OK [UIDVALIDITY 42] UIDs valid\r\n")
            .expect("response should be valid");
        assert_eq!(
            tokens,
            [
                atom("*"),
                atom("OK"),
                atom("[UIDVALIDITY"),
                atom("42]"),
                atom("UIDs"),
                atom("valid")
            ]
        );
    }

    #[test]
    fn reads_parentheses_and_quoted_strings_with_escapes() {
        let tokens =
            read(b"* LIST (\\HasNoChildren \\Sent) \"/\" \"a \\\"b\\\\\"\r\n")
                .expect("response should be valid");
        assert_eq!(
            tokens,
            [
                atom("*"),
                atom("LIST"),
                Token::Open,
                atom("\\HasNoChildren"),
                atom("\\Sent"),
                Token::Close,
                quoted("/"),
                quoted("a \"b\\"),
            ]
        );
    }

    #[test]
    fn reads_literals_and_the_rest_of_the_line_after_them() {
        let input = b"* 1 FETCH (UID 7 BODY[HEADER] {5}\r\nab\r\nc)\r\n\
                      * 2 FETCH (UID 8)\r\n";
        let mut reader = &input[..];
        let first =
            read_response(&mut reader).expect("response should be valid");
        assert_eq!(
            first,
            [
                atom("*"),
                atom("1"),
                atom("FETCH"),
                Token::Open,
                atom("UID"),
                atom("7"),
                atom("BODY[HEADER]"),
                Token::Literal(b"ab\r\nc".to_vec()),
                Token::Close,
            ]
        );
        let second =
            read_response(&mut reader).expect("response should be valid");
        assert_eq!(second[1], atom("2"));
        assert!(reader.is_empty());
    }

    #[test]
    fn reads_several_literals_in_one_response() {
        let tokens =
            read(b"* X {1}\r\na {0}\r\n)\n").expect("response should be valid");
        assert_eq!(
            tokens,
            [
                atom("*"),
                atom("X"),
                Token::Literal(b"a".to_vec()),
                Token::Literal(Vec::new()),
                Token::Close
            ]
        );
    }

    #[test]
    fn reads_braces_that_end_no_line_as_atom() {
        let tokens =
            read(b"* OK {5} text\r\n").expect("response should be valid");
        assert_eq!(tokens[2], atom("{5}"));
    }

    #[test]
    fn accepts_unterminated_quoted_strings() {
        let tokens =
            read(b"* OK \"open\r\n").expect("response should be valid");
        assert_eq!(tokens[2], quoted("open"));
    }

    #[test]
    fn rejects_closed_connection() {
        assert_eq!(error(b""), "connection closed by the server");
        assert_eq!(error(b"* OK"), "connection closed by the server");
        assert_eq!(
            error(b"* 1 FETCH (BODY[HEADER] {5}\r\nab"),
            "connection closed by the server in a literal"
        );
    }

    #[test]
    fn rejects_too_long_line() {
        let mut input = vec![
            b'x';
            usize::try_from(MAX_LINE)
                .expect("MAX_LINE should fit into usize")
        ];
        input.extend_from_slice(b"\r\n");
        assert_eq!(error(&input), "server sent a line longer than allowed");
    }

    #[test]
    fn rejects_too_large_literal() {
        let input = format!("* X {{{}}}\r\n", MAX_LITERAL + 1);
        assert!(
            error(input.as_bytes()).contains("more than allowed"),
            "{input}"
        );
    }
}
