#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# Generates the demo report from made-up data, see README.md.

set -euo pipefail

cd "$(dirname "$0")/../.."
rm -rf examples/demo/data
cargo run --locked --example demo_data -- examples/demo/data
config=examples/demo/imap-statistics.toml
cargo run --locked -- --config "$config" index
cargo run --locked -- --config "$config" report --output examples/demo/report
