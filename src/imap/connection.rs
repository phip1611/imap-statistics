// SPDX-License-Identifier: MIT OR Apache-2.0

//! Connects to IMAP servers over TLS.

use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use rustls::crypto::ring;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

use crate::imap::Client;

/// Timeout to connect, per address of the server.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Timeout per read or write, not per command. Generous, as servers may
/// take a while to search large folders.
const IO_TIMEOUT: Duration = Duration::from_secs(120);

/// A TLS connection to an IMAP server.
pub type TlsStream = StreamOwned<ClientConnection, TcpStream>;

/// Connects to an IMAP server over TLS and reads its greeting.
///
/// The server's certificate must be valid for `host` and signed by a
/// root certificate that the OS trusts.
///
/// # Arguments
///
/// - `host`: Host name of the server, e.g. `imap.example.com`.
/// - `port`: Port of IMAP over TLS, usually 993.
pub fn connect(host: &str, port: u16) -> anyhow::Result<Client<TlsStream>> {
    let server_name = ServerName::try_from(host.to_owned())
        .with_context(|| format!("invalid host name {host:?}"))?;
    let tls = ClientConnection::new(Arc::new(tls_config()?), server_name)?;
    let tcp = connect_tcp(host, port)?;
    // The TLS handshake happens when the client reads the greeting.
    Client::new(StreamOwned::new(tls, tcp))
        .with_context(|| format!("failed to connect to {host}:{port}"))
}

/// Opens a TCP connection to the first address of the host that answers.
///
/// # Arguments
///
/// - `host`: Host name or IP address of the server.
/// - `port`: Port of the server.
fn connect_tcp(host: &str, port: u16) -> anyhow::Result<TcpStream> {
    let addresses = (host, port)
        .to_socket_addrs()
        .with_context(|| format!("failed to resolve {host}"))?;
    let mut last_error = None;
    for address in addresses {
        match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
            Ok(stream) => {
                stream.set_read_timeout(Some(IO_TIMEOUT))?;
                stream.set_write_timeout(Some(IO_TIMEOUT))?;
                return Ok(stream);
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.map_or_else(
        || anyhow!("{host} has no addresses"),
        |error| {
            anyhow::Error::new(error)
                .context(format!("failed to connect to {host}:{port}"))
        },
    ))
}

/// Returns the TLS configuration, with the root certificates of the OS.
fn tls_config() -> anyhow::Result<ClientConfig> {
    let native = rustls_native_certs::load_native_certs();
    let mut roots = RootCertStore::empty();
    // Certificates of the OS that rustls cannot parse are skipped.
    roots.add_parsable_certificates(native.certs);
    if roots.is_empty() {
        let errors: Vec<_> =
            native.errors.iter().map(ToString::to_string).collect();
        bail!(
            "found no root certificates in the OS: {}",
            errors.join(", ")
        );
    }
    let provider = Arc::new(ring::default_provider());
    Ok(ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Checks that the root certificates of the OS are found, on every OS
    /// that CI runs on.
    #[test]
    fn finds_root_certificates_of_the_os() {
        tls_config().expect("the OS should have root certificates");
    }
}
