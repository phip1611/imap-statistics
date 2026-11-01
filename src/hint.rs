// SPDX-License-Identifier: MIT OR Apache-2.0

//! Hints for errors that users can fix themselves.

use std::io::{self, ErrorKind};

use crate::imap::CommandFailed;

/// Hint for a login that the server rejected.
const LOGIN: &str = "check the username and the password. Gmail and many \
    other providers need an app password if two-factor authentication is \
    on, and some need IMAP to be enabled in the settings first.";
/// Hint for a server certificate that is not trusted.
const CERTIFICATE: &str = "the TLS certificate of the server is not \
    trusted. Check the host name. If the server has a certificate of its \
    own CA, add that CA to the root certificates of the OS.";
/// Hint for a server that cannot be reached.
const CONNECTION: &str = "check the host and the port. The connection uses \
    TLS from the start, usually on port 993. Servers that only offer \
    STARTTLS on port 143 are not supported.";

/// Returns a hint on how to fix an error of a fetch, if there is one.
///
/// # Arguments
///
/// - `error`: The error, with all its causes.
#[must_use]
pub fn hint(error: &anyhow::Error) -> Option<&'static str> {
    error.chain().find_map(|cause| {
        if let Some(failed) = cause.downcast_ref::<CommandFailed>() {
            return (failed.command == "LOGIN").then_some(LOGIN);
        }
        let io_error = cause.downcast_ref::<io::Error>()?;
        if is_certificate_error(io_error) {
            return Some(CERTIFICATE);
        }
        matches!(
            io_error.kind(),
            ErrorKind::ConnectionRefused
                | ErrorKind::TimedOut
                | ErrorKind::HostUnreachable
                | ErrorKind::NetworkUnreachable
        )
        .then_some(CONNECTION)
    })
}

/// Checks whether an I/O error is a certificate that rustls rejected.
///
/// rustls reports TLS errors of a stream as I/O errors that wrap them.
///
/// # Arguments
///
/// - `error`: The I/O error.
fn is_certificate_error(error: &io::Error) -> bool {
    error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
        .is_some_and(|tls| matches!(tls, rustls::Error::InvalidCertificate(_)))
}

#[cfg(test)]
mod tests {
    use rustls::CertificateError;

    use super::*;

    /// Returns a refused command, wrapped like errors of the client.
    ///
    /// # Arguments
    ///
    /// - `command`: The name of the command.
    fn refused(command: &str) -> anyhow::Error {
        anyhow::Error::new(CommandFailed {
            command: command.into(),
            response: "NO [AUTHENTICATIONFAILED] Invalid credentials".into(),
        })
        .context("source failed")
    }

    /// Returns an I/O error, wrapped like errors of the connection.
    ///
    /// # Arguments
    ///
    /// - `error`: The I/O error.
    fn wrapped(error: io::Error) -> anyhow::Error {
        anyhow::Error::new(error).context("failed to connect")
    }

    #[test]
    fn hints_at_app_passwords_for_rejected_logins() {
        assert_eq!(hint(&refused("LOGIN")), Some(LOGIN));
        assert_eq!(hint(&refused("EXAMINE")), None);
    }

    /// rustls reports certificate errors wrapped in I/O errors, which the hint
    /// must find among the causes. Other TLS errors get no hint.
    #[test]
    fn hints_at_the_ca_for_untrusted_certificates() {
        let untrusted = io::Error::new(
            ErrorKind::InvalidData,
            rustls::Error::InvalidCertificate(CertificateError::UnknownIssuer),
        );
        assert_eq!(hint(&wrapped(untrusted)), Some(CERTIFICATE));
        let other =
            io::Error::new(ErrorKind::InvalidData, rustls::Error::DecryptError);
        assert_eq!(hint(&wrapped(other)), None);
    }

    #[test]
    fn hints_at_host_and_port_for_unreachable_servers() {
        for kind in [ErrorKind::ConnectionRefused, ErrorKind::TimedOut] {
            assert_eq!(hint(&wrapped(kind.into())), Some(CONNECTION));
        }
        assert_eq!(hint(&wrapped(ErrorKind::NotFound.into())), None);
    }
}
