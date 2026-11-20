#!/usr/bin/env bash
# Build what an app embeds: the core as WebAssembly and the compiled registry.
#   dist/ofp_core.wasm          the core (C interface, see crates/ofp-ffi)
#   dist/registry.bundle.json   fields and tables, compiled
set -euo pipefail
cd "$(dirname "$0")/.."

cargo run --quiet -p ofp-cli --no-default-features -- check
cargo build --quiet --release -p ofp-ffi --target wasm32-unknown-unknown
mkdir -p dist
cp target/wasm32-unknown-unknown/release/ofp_ffi.wasm dist/ofp_core.wasm
cargo run --quiet -p ofp-cli --no-default-features -- compile -o dist/registry.bundle.json
ls -l dist
