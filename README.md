# imap-statistics

CLI to fetch the headers of all emails from IMAP accounts and generate a
Markdown report with statistics: emails per year, per category, and per
sender.

> **Status:** under development, nothing usable yet.

The tool only reads from the IMAP servers and never changes a mailbox,
not even the "seen" flag of a message. It runs on Linux, macOS, and
Windows.

## Install

```sh
cargo install --locked --git https://github.com/phip1611/imap-statistics
```

## Usage

1. Copy [`imap-statistics.example.toml`](imap-statistics.example.toml) to
   `imap-statistics.toml` and fill in your IMAP accounts. The file holds
   passwords: keep it private.
2. Check the config:

   ```sh
   imap-statistics check-config
   ```

3. Fetch the headers of all emails:

   ```sh
   imap-statistics fetch
   ```

   The first fetch takes a while for large mailboxes. Later fetches only
   fetch the headers of new emails. A fetch can stop at any time, e.g.
   with Ctrl+C, and the next one continues where it stopped.

`--config <path>` selects another config file.

### What is fetched

Only the headers of the emails (sender, date, subject, etc.), never their
content or attachments. They are stored in the data directory, one
directory per source, so that the statistics can be built again without
fetching.

All folders are fetched except the trash, spam, and folders that cannot
hold emails. If a server has a folder with all emails, as Gmail has with
"All Mail", only that folder is fetched, so that an email with several
labels counts once. Emails that were deleted on the server drop out with
the next fetch.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
