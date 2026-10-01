#!/usr/bin/env bash
# The quality gate: one call, run locally, by the pre-commit hook and by CI.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --all --check
cargo clippy --workspace --exclude warpify-plugin --all-targets -- -D warnings
cargo clippy -p warpify-plugin --target wasm32-wasip1 --all-targets -- -D warnings
cargo test --workspace --exclude warpify-plugin
cargo build -p warpify-plugin --target wasm32-wasip1 --release
