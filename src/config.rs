// SPDX-License-Identifier: MIT OR Apache-2.0

//! The config file: where to store the data, and which IMAP accounts to
//! read.

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::Deserialize;

/// Port of IMAP over TLS (RFC 8314, section 7).
const DEFAULT_PORT: u16 = 993;

/// The parsed and validated config file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Directory for the fetched headers, the index, and the report.
    ///
    /// In the file, a relative path is relative to the directory of the
    /// config file. After loading, it is relative to the current
    /// directory, or absolute.
    pub data_dir: PathBuf,
    /// The IMAP accounts, in the order of the file.
    #[serde(default, rename = "source")]
    pub sources: Vec<Source>,
}

/// An IMAP account to read the emails from.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Unique name, also the name of the source's directory in the data
    /// directory.
    pub name: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub username: String,
    pub password: Password,
}

/// A password, which [`fmt::Debug`] does not print.
#[derive(Deserialize)]
#[serde(transparent)]
pub struct Password(String);

impl Password {
    /// Returns the password, only to send it to the server.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Password(<hidden>)")
    }
}

const fn default_port() -> u16 {
    DEFAULT_PORT
}

impl Config {
    /// Reads and validates the config file at `path`.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = fs::read_to_string(path).with_context(|| {
            format!("failed to read config file {}", path.display())
        })?;
        let base = path.parent().unwrap_or_else(|| Path::new(""));
        Self::parse(&text, base)
            .with_context(|| format!("invalid config file {}", path.display()))
    }

    /// Parses and validates a config file, whose relative paths are
    /// relative to `base`.
    fn parse(text: &str, base: &Path) -> anyhow::Result<Self> {
        let mut config: Self = toml::from_str(text)?;
        config.validate()?;
        // An absolute data_dir replaces the base.
        config.data_dir = base.join(&config.data_dir);
        Ok(config)
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.sources.is_empty() {
            bail!("no [[source]] configured");
        }
        let mut names = HashSet::new();
        for source in &self.sources {
            if !is_valid_name(&source.name) {
                bail!(
                    "invalid source name {:?}: use only a-z, 0-9, '-', and '_'",
                    source.name
                );
            }
            if !names.insert(source.name.as_str()) {
                bail!("duplicate source name {:?}", source.name);
            }
            if source.password.expose().is_empty() {
                bail!("empty password for source {:?}", source.name);
            }
        }
        Ok(())
    }
}

/// Source names are directory names. Lowercase ASCII letters, digits, '-',
/// and '_' are valid on every OS, also on file systems that ignore case.
fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|b| {
            b.is_ascii_lowercase()
                || b.is_ascii_digit()
                || b == b'-'
                || b == b'_'
        })
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    const MINIMAL: &str = r#"
        data_dir = "data"

        [[source]]
        name = "work"
        host = "imap.example.com"
        username = "me@example.com"
        password = "secret"
    "#;

    /// Returns a config with two sources, `first` and `second` as their
    /// names.
    fn two_sources(first: &str, second: &str) -> String {
        format!(
            r#"
            data_dir = "data"

            [[source]]
            name = "{first}"
            host = "imap.example.com"
            username = "me@example.com"
            password = "secret"

            [[source]]
            name = "{second}"
            host = "imap.example.org"
            port = 1993
            username = "me@example.org"
            password = "secret"
            "#
        )
    }

    fn error(text: &str) -> String {
        Config::parse(text, Path::new(""))
            .expect_err("config should be invalid")
            .to_string()
    }

    #[test]
    fn parses_minimal_config_with_defaults() {
        let config = Config::parse(MINIMAL, Path::new("base"))
            .expect("minimal config should be valid");
        assert_eq!(config.data_dir, Path::new("base").join("data"));
        let [source] = config.sources.as_slice() else {
            panic!("expected one source, got {:?}", config.sources);
        };
        assert_eq!(source.name, "work");
        assert_eq!(source.host, "imap.example.com");
        assert_eq!(source.port, DEFAULT_PORT);
        assert_eq!(source.username, "me@example.com");
        assert_eq!(source.password.expose(), "secret");
    }

    #[test]
    fn parses_example_config() {
        let text = include_str!("../imap-statistics.example.toml");
        let config = Config::parse(text, Path::new(""))
            .expect("example config should be valid");
        assert_eq!(config.sources.len(), 2);
    }

    #[test]
    fn keeps_sources_in_order() {
        let config = Config::parse(&two_sources("b", "a"), Path::new(""))
            .expect("config should be valid");
        let names: Vec<_> =
            config.sources.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["b", "a"]);
        assert_eq!(config.sources[1].port, 1993);
    }

    #[test]
    fn keeps_absolute_data_dir() {
        let absolute = env::temp_dir().join("data");
        let text = MINIMAL.replace(
            r#"data_dir = "data""#,
            &format!("data_dir = {:?}", absolute.display().to_string()),
        );
        let config = Config::parse(&text, Path::new("base"))
            .expect("config should be valid");
        assert_eq!(config.data_dir, absolute);
    }

    #[test]
    fn rejects_duplicate_names() {
        assert_eq!(
            error(&two_sources("work", "work")),
            r#"duplicate source name "work""#
        );
    }

    #[test]
    fn rejects_names_that_are_no_portable_directory_names() {
        for name in ["", "Work", "a/b", r"a\b", "..", "a b", "ä"] {
            let message = error(&two_sources(name, "other"));
            assert!(
                message.starts_with("invalid source name"),
                "{name:?}: {message}"
            );
        }
    }

    #[test]
    fn rejects_empty_password() {
        assert_eq!(
            error(&MINIMAL.replace(r#""secret""#, r#""""#)),
            r#"empty password for source "work""#
        );
    }

    #[test]
    fn rejects_config_without_sources() {
        assert_eq!(error(r#"data_dir = "data""#), "no [[source]] configured");
    }

    #[test]
    fn rejects_unknown_fields() {
        let message = error(&MINIMAL.replace("username", "user"));
        assert!(message.contains("unknown field `user`"), "{message}");
    }

    #[test]
    fn debug_output_hides_the_password() {
        let config = Config::parse(MINIMAL, Path::new(""))
            .expect("minimal config should be valid");
        let debug = format!("{config:?}");
        assert!(debug.contains("Password(<hidden>)"), "{debug}");
        assert!(!debug.contains("secret"), "{debug}");
    }
}
