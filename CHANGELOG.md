# UNRELEASED

- Initial version of the `imap-statistics` CLI.
- `imap-statistics` fetches, indexes, and reports in one run. Every run
  continues where the last one stopped. `--offline` skips the fetch. If a
  source fails, the report still covers its emails of earlier runs, but
  the run fails at the end.
- `check-config` checks the config file and lists its sources.
- The fetch gets the headers of all new emails of all sources.
  It reports every step, and the time left for large folders.
  For common errors, such as a rejected login, it shows how to fix them.
- The index holds all fetched emails, with their year, sender, category,
  and whether they were received or sent. Emails from an own address (the
  username or `addresses` in the config) or in a Sent folder count as
  sent.
- The report is in Markdown with SVG charts: an overview of the key
  numbers, received and sent emails per account and per year, received
  emails per category, and the top senders and the top recipients of sent
  emails, in total and per year. Complete lists of all senders and
  recipients help to find the emails to clean up. They are also written as
  CSV files, for spreadsheets. `README.txt` and `LIESMICH.txt` (German)
  explain the files of the report, and how to clean up with them.
  It states when it was generated, in UTC.
  If pandoc is installed, it also writes the report as PDF.
