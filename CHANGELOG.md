# UNRELEASED

- Initial version of the `imap-statistics` CLI.
- `check-config` checks the config file and lists its sources.
- `fetch` fetches the headers of all new emails of all sources.
- `index` builds the index of all fetched emails, with their year, sender,
  and category.
- `report` generates a Markdown report with SVG charts: emails per year,
  per category, and the top senders.
