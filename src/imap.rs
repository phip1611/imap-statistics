// SPDX-License-Identifier: MIT OR Apache-2.0

//! A minimal IMAP client that can only read (RFC 3501).
//!
//! The mailboxes must never change, not even the `\Seen` flag of an email.
//! The client therefore only has commands that cannot change a mailbox:
//! folders are opened with `EXAMINE` instead of `SELECT`, and headers are
//! fetched with `BODY.PEEK[HEADER]`, which unlike `BODY[HEADER]` does not
//! set `\Seen` (RFC 3501, section 6.4.5). As a second line of defense,
//! [`check_read_only`] refuses every other command before it is sent.
//!
//! The submodules read the responses of the server ([`response`]), parse
//! them (`parse`), send the commands (`client`), guard them
//! (`read_only`), and choose the folders (`folders`).

mod client;
mod folders;
mod parse;
mod read_only;
pub mod response;

pub use client::Client;
pub use folders::select_folders;
pub use read_only::check_read_only;

/// A folder as listed by the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    /// Name as the server sent it, in modified UTF-7 (RFC 3501, section
    /// 5.1.3).
    pub name: String,
    /// Attributes in lowercase, e.g. `\noselect` or `\trash`.
    pub attributes: Vec<String>,
}

/// The state of a folder, as reported when opening it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderStatus {
    /// Number of emails in the folder.
    pub exists: u32,
    /// Changes when the server assigns new UIDs to the emails of the
    /// folder, which makes all known UIDs of it invalid.
    pub uidvalidity: u32,
}

/// The header of an email, as fetched from the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedHeader {
    pub uid: u32,
    /// Arrival time on the server, e.g. `17-Jul-1996 02:44:25 -0700`.
    pub internaldate: Option<String>,
    /// The raw header, including the empty line that ends it.
    pub header: Vec<u8>,
}
