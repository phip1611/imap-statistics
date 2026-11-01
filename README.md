# imap-statistics

CLI to fetch the headers of all emails from IMAP accounts and generate a
Markdown report with statistics: received and sent emails per account and per
year, received emails per category and sender, and the recipients of sent
emails. See the [demo report](examples/demo/report/report.md) of
made-up emails.

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

3. Fetch the headers of all emails, index them, and generate the report:

   ```sh
   imap-statistics
   ```

   The first run takes a while for large mailboxes. Later runs only fetch
   the headers of new emails. A run can stop at any time, e.g. with
   Ctrl+C, and the next one continues where it stopped. `--offline` skips
   the fetch and uses the emails of earlier runs. If a source fails, the
   report still covers its emails of earlier runs, but the run fails at
   the end.

   The report is written to `report/report.md` in the data directory, with
   SVG charts and CSV files of all senders and recipients next to it.
   `README.txt` and `LIESMICH.txt` (German) explain these files, and how to
   clean up with them. `--output <dir>` writes the report elsewhere, and
   `--top <n>` sets the number of senders and recipients in each list of
   top ones (default: 5).

   The report states when it was generated, in UTC. If set,
   `SOURCE_DATE_EPOCH` (seconds since 1970) overrides that time, e.g. for
   reproducible reports.

   If [pandoc](https://pandoc.org) is installed, the run also converts the
   report into `report.pdf`. With [typst](https://typst.app) installed as
   well, pandoc uses it, otherwise pandoc's default, which needs LaTeX and
   `rsvg-convert` for the charts. Without pandoc, there is no PDF, only a
   warning.

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

### The index

The index is in `index.json` in the data directory: one entry per email
with its sources, year, sender, and category. Copies of an email in
several folders or sources count once. The year is in UTC, from the
`Date` header, or from the arrival on the server if `Date` is missing or
implausible.

The categories come from simple rules on the headers, never the content,
so a few emails end up in the wrong category. Each category has a list of
over 100 sender domains, sender addresses, and subject words, in English,
German, Polish, French, Spanish, Italian, and Dutch, in
[`src/category/rules.rs`](src/category/rules.rs). The first matching rule
wins:

1. `social`: from a social network or dating site, e.g.
   `facebookmail.com`.
2. `newsletter`: from a mailing list (`List-Id`, `List-Unsubscribe`, or
   `Precedence: bulk`), or advertising, e.g. "Black Friday".
3. `shopping`: about an order, invoice, payment, or delivery, e.g. from a
   parcel service.
4. `notification`: sent by a program (`Auto-Submitted`, or an address such
   as `noreply@`), e.g. a security code or a bounce.
5. `personal`: everything else.

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
