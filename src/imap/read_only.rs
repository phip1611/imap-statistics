// SPDX-License-Identifier: MIT OR Apache-2.0

//! The allowlist of the commands that cannot change a mailbox.

use anyhow::bail;

/// What `UID FETCH` fetches of every email. `BODY.PEEK[HEADER]`, unlike
/// `BODY[HEADER]`, does not set `\Seen`.
pub(super) const FETCH_ITEMS: &str = "(UID INTERNALDATE BODY.PEEK[HEADER])";

/// Refuses every command that could change a mailbox.
///
/// All commands of [`Client`](super::Client) pass. This is a second line
/// of defense against bugs, e.g. a fetch item that sets `\Seen`.
///
/// # Arguments
///
/// - `command`: The command, without tag and line ending.
pub fn check_read_only(command: &str) -> anyhow::Result<()> {
    let name = command_name(command);
    let allowed = !command.contains(['\r', '\n'])
        && match name {
            "LOGIN" | "LIST" | "EXAMINE" | "LOGOUT" => true,
            "UID SEARCH" => command == "UID SEARCH ALL",
            "UID FETCH" => command
                .strip_prefix("UID FETCH ")
                .and_then(|rest| rest.strip_suffix(FETCH_ITEMS))
                .and_then(|set| set.strip_suffix(' '))
                .is_some_and(|set| {
                    !set.is_empty()
                        && set.bytes().all(|b| b.is_ascii_digit() || b == b',')
                }),
            _ => false,
        };
    if !allowed {
        bail!("refusing IMAP command {name}, it could change a mailbox");
    }
    Ok(())
}

/// Returns the name of a command, e.g. `LOGIN` or `UID FETCH`.
///
/// # Arguments
///
/// - `command`: The command, without tag and line ending.
pub(super) fn command_name(command: &str) -> &str {
    let words = if command.starts_with("UID ") { 2 } else { 1 };
    let end = command
        .match_indices(' ')
        .nth(words - 1)
        .map_or(command.len(), |(index, _)| index);
    &command[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_the_commands_of_the_client() {
        for command in [
            r#"LOGIN "a" "b""#,
            r#"LIST "" "*""#,
            r#"EXAMINE "INBOX""#,
            "UID SEARCH ALL",
            "UID FETCH 1,2 (UID INTERNALDATE BODY.PEEK[HEADER])",
            "LOGOUT",
        ] {
            check_read_only(command).expect(command);
        }
    }

    #[test]
    fn refuses_commands_that_could_change_a_mailbox() {
        for command in [
            r#"SELECT "INBOX""#,
            r"STORE 1 +FLAGS (\Deleted)",
            r"UID STORE 1 +FLAGS (\Seen)",
            "UID FETCH 1 (BODY[HEADER])",
            "UID FETCH 1 BODY[] (UID INTERNALDATE BODY.PEEK[HEADER])",
            "UID FETCH 1 (RFC822)",
            r#"UID COPY 1 "Trash""#,
            r#"UID MOVE 1 "Trash""#,
            "EXPUNGE",
            "UID SEARCH UNSEEN",
            "LOGIN \"a\" \"b\"\r\nA2 EXPUNGE",
        ] {
            let error = check_read_only(command)
                .expect_err("command should be refused");
            assert!(
                error.to_string().starts_with("refusing IMAP command"),
                "{command}: {error}"
            );
        }
    }

    #[test]
    fn names_commands() {
        assert_eq!(command_name("LOGIN \"a\" \"b\""), "LOGIN");
        assert_eq!(command_name("UID FETCH 1 (UID)"), "UID FETCH");
        assert_eq!(command_name("LOGOUT"), "LOGOUT");
    }
}
