Email statistics
================

This directory holds a report about the emails in your email accounts,
made by imap-statistics: https://github.com/phip1611/imap-statistics

imap-statistics only reads the emails. It never changes or deletes
anything in your accounts.

Files
-----

report.md        The report. Any text editor opens it. A Markdown viewer,
                 e.g. VS Code, also shows the charts.
report.pdf       The same report as PDF, to read or print. It is only there
                 if pandoc is installed, see below.
senders.csv      All senders of received emails, for spreadsheet programs
                 such as LibreOffice Calc or Excel.
recipients.csv   All recipients of sent emails, for spreadsheet programs.
*.svg            The charts of the report. Any web browser opens them.

How to clean up
---------------

1. Read the overview at the start of the report. It shows where most of
   your emails come from.
2. Open senders.csv in a spreadsheet program and sort it by "Emails". The
   senders at the top are the best candidates: newsletters, shops, and
   notifications often send thousands of emails.
3. "First" and "Last" are the years of the first and the last email of a
   sender. If the last email is years old, e.g. from a newsletter that you
   cancelled, you can often delete all emails of that sender.
4. In your email program or web mail, search for the address of the
   sender, select all results, and delete them. Unsubscribe from
   newsletters that you do not read, so that no new ones come.
5. Run imap-statistics again to see the result.

The categories come from simple rules, so a few emails are in the wrong
category. Check the emails before you delete them.

The PDF
-------

The PDF needs pandoc: https://pandoc.org
It looks best with typst installed as well: https://typst.app
Without typst, pandoc needs LaTeX. Install them, and run imap-statistics
again.
