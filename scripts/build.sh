#!/usr/bin/env bash
# Build what an app embeds: the core as WebAssembly and the compiled registry.
#   dist/ofp_core.wasm          the core (C interface, see crates/ofp-ffi)
#   dist/ofp_core.asm.js        the same core as plain JavaScript, for runtimes with no WebAssembly
#                               (React Native's Hermes); needs binaryen's wasm2js
#   dist/registry.bundle.json   fields and tables, compiled
#   dist/ofp.js, dist/ofp.d.ts  the JavaScript package (packages/ofp-js): the hooks an app wires up
set -euo pipefail
cd "$(dirname "$0")/.."

cargo run --quiet -p ofp-cli --no-default-features -- check
cargo build --quiet --release -p ofp-ffi --target wasm32-unknown-unknown
mkdir -p dist
cp target/wasm32-unknown-unknown/release/ofp_ffi.wasm dist/ofp_core.wasm
cargo run --quiet -p ofp-cli --no-default-features -- compile -o dist/registry.bundle.json
if command -v wasm2js >/dev/null; then
  wasm2js -O dist/ofp_core.wasm -o dist/ofp_core.asm.js
else
  echo "wasm2js not found (binaryen, version 133 or newer): dist/ofp_core.asm.js not rebuilt" >&2
fi
# the package, flattened: ofp.js beside adapters/
cp packages/ofp-js/index.js dist/ofp.js
cp packages/ofp-js/index.d.ts dist/ofp.d.ts
mkdir -p dist/adapters
cp packages/ofp-js/adapters/*.js dist/adapters/
# in dist the package's index is named ofp
for f in packages/ofp-js/adapters/*.d.ts; do sed "s|'../index'|'../ofp'|" "$f" > "dist/adapters/$(basename "$f")"; done
ls -l dist
