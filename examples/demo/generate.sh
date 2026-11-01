#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# Generates the demo report from made-up data, see README.md.

set -euo pipefail

cd "$(dirname "$0")/../.."
# A fixed time of the report, 2026-01-01 00:00:00 UTC, so that the report
# only changes when the code does.
export IMAP_STATISTICS_NOW=1767225600
rm -rf examples/demo/data
cargo run --locked --example demo_data -- examples/demo/data
config=examples/demo/imap-statistics.toml
cargo run --locked -- --config "$config" --offline \
    --output examples/demo/report
