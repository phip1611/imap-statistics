# UNRELEASED

- Initial version of the `imap-statistics` CLI.
- `check-config` checks the config file and lists its sources.
- `fetch` fetches the headers of all new emails of all sources.
  It reports every step, and the time left for large folders.
  For common errors, such as a rejected login, it shows how to fix them.
- `index` builds the index of all fetched emails, with their year, sender,
  category, and whether they were received or sent. Emails from an own
  address (the username or `addresses` in the config) or in a Sent folder
  count as sent.
- `report` generates a Markdown report with SVG charts: an overview of the
  key numbers, received and sent emails per account and per year, received
  emails per category, and the top senders and the top recipients of sent
  emails, in total and per year.
  It states when it was generated, in UTC.
  If pandoc is installed, it also writes the report as PDF.
