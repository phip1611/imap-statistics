# Agent Instructions

Instructions for AI coding agents (and humans) working on this repository.
User docs are in [README.md](README.md).

## Principles

- **Read-only on the servers.** The tool must never change a mailbox: no
  flags (including `\Seen`), no moves, no deletes. Only send IMAP commands
  that cannot change a mailbox.
- **Privacy.** Logs and errors contain counts and error kinds, never
  subjects, addresses, or other message data. The repository is public:
  never commit real emails, real config files, or passwords. Tests use
  made-up data only.
- **Cross-platform.** Linux, macOS, and Windows. Use `std::path`, no shell
  commands, and no dependencies on system libraries such as OpenSSL.
- **Simple and maintainable.** Prefer readable code and few dependencies
  over clever or generic solutions.

## Checks

`nix develop`, or direnv with `.envrc`, provides rustup, and pandoc with
typst for the PDF of the report.

Run before every commit. Every commit of a series must pass them:

```sh
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --locked --all-features \
  --document-private-items --no-deps
cargo +1.95 build --locked --all-features    # MSRV, also in Cargo.toml and CI
typos
```

After a change of the report, also run `examples/demo/generate.sh` and
commit the changed demo report, which CI compares.

## Conventions

- Every `.rs` file starts with `// SPDX-License-Identifier: MIT OR Apache-2.0`.
- Line width is 80 (`rustfmt.toml`, `.editorconfig`).
- Every function has a doc comment, private ones and test helpers too: a
  summary of one line (two at most), optionally more text after an empty
  line, and then a `# Arguments` section that lists every argument but
  `self` as `` - `name`: description ``. Non-trivial tests start with 1-3
  lines that explain their context, e.g. why the case matters.
- Commit subjects are `<component>: <title>`, imperative, at most 72
  characters. Commits are atomic: dependency additions, renames, and moves
  go into their own commits.
- Dependencies have no default features, only the needed ones, and
  versions in `Cargo.toml` are `x.y`, not `x.y.z`.
- User-visible changes get a `CHANGELOG.md` entry and, if needed, a
  `README.md` update in the same commit.
