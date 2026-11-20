#!/bin/sh
# Publishes the JavaScript package @openfitnessprotocol/ofp to npm: packages/ofp-js plus what an app needs
# to run it, from dist/ (the core as WebAssembly and as plain JavaScript, the compiled device tables).
#   scripts/npm-publish.sh            build, assemble, publish (needs NPM_TOKEN)
#   scripts/npm-publish.sh --dry-run  assemble and show what would be published
#   scripts/npm-publish.sh --otp 123456   when the account asks for a two-factor code (a token made with
#                                     "bypass two-factor" needs none)
# The version is packages/ofp-js/package.json's (set by scripts/release.sh). The token is never written
# into the repository: a throwaway npmrc in a temporary folder.
set -e
cd "$(dirname "$0")/.."
DRY=""
OTP=""
[ "$1" = "--dry-run" ] && DRY="--dry-run"
[ "$1" = "--otp" ] && OTP="--otp=$2"
scripts/build.sh >/dev/null
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
cp -R packages/ofp-js/index.js packages/ofp-js/index.d.ts packages/ofp-js/adapters packages/ofp-js/package.json packages/ofp-js/README.md LICENSE NOTICE "$STAGE/"
mkdir -p "$STAGE/core"
cp dist/ofp_core.wasm dist/ofp_core.asm.js "$STAGE/core/"
cp dist/registry.bundle.json "$STAGE/"
# the tables as a JavaScript module too (bundlers such as Metro import it without JSON settings)
node -e 'const fs=require("fs");const t=fs.readFileSync(process.argv[1],"utf8").trim();fs.writeFileSync(process.argv[2],"// The compiled OFP device tables (registry.bundle.json) as text.\nexport default "+JSON.stringify(t)+"\n")' dist/registry.bundle.json "$STAGE/registry.js"
# types for the two entry points that are generated (TypeScript with noImplicitAny needs them)
printf '%s\n' '/** The OFP core as plain JavaScript, for runtimes with no WebAssembly: pass it to moduleEngine(). */' 'export {}' > "$STAGE/core/ofp_core.asm.d.ts"
printf '%s\n' '/** The compiled OFP device tables (registry.bundle.json) as text: pass it as Ofp({ bundle }). */' 'declare const bundle: string' 'export default bundle' > "$STAGE/registry.d.ts"
if [ -z "$DRY" ]; then
  [ -n "$NPM_TOKEN" ] || { echo "NPM_TOKEN is not set" >&2; exit 1; }
  printf '//registry.npmjs.org/:_authToken=%s\n' "$NPM_TOKEN" > "$STAGE/.npmrc.publish"
  (cd "$STAGE" && npm publish --access public --userconfig .npmrc.publish --ignore-scripts $OTP)
else
  (cd "$STAGE" && npm publish --access public --dry-run --ignore-scripts 2>&1 | grep -E "name:|version:|package size|unpacked size|total files|npm notice [0-9.]+[kKM]?B ")
fi
