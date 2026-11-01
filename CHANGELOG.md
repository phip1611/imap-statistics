# UNRELEASED

- Initial version of the `imap-statistics` CLI.
- `check-config` checks the config file and lists its sources.
- `fetch` fetches the headers of all new emails of all sources.
  For common errors, such as a rejected login, it shows how to fix them.
- `index` builds the index of all fetched emails, with their year, sender,
  and category.
- `report` generates a Markdown report with SVG charts: emails per account,
  per year, per category, and the top senders.
  If pandoc is installed, it also writes the report as PDF.
