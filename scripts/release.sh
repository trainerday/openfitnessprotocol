#!/bin/sh
# Releases OFP Studio: version, builds, downloads, the site, the GitHub release. The runbook is
# docs/releasing.md; the /version command walks through it.
#
#   scripts/release.sh <x.y.z>            everything below, in order
#   scripts/release.sh prepare <x.y.z>    checks, version, CHANGELOG heading, tests, commit + tag, push
#   scripts/release.sh mac                the Mac app (universal), signed and notarized, into release/
#   scripts/release.sh windows            the Windows installer, built on OFP_WINDOWS_SSH, into release/
#   scripts/release.sh publish            upload release/, site/releases.json, the site, latest.json, GitHub
#
# Each step can be run again on its own after a failure. Environment:
#   Mac signing and notarizing: see scripts/app-build.sh
#   OFP_WINDOWS_SSH      ssh host of a Windows machine with a clone at OFP_WINDOWS_DIR (no Windows build without it)
#   OFP_WINDOWS_DIR      that clone, Windows path (default C:\Projects\openfitnessprotocol)
#   OFP_WINDOWS_SIGN     "<endpoint> <account> <profile>" for Azure Trusted Signing (unsigned without it)
#   Cloudflare: see site/deploy.sh. The bucket is ofp-releases.
set -e
cd "$(dirname "$0")/.."
ROOT=$(pwd)
# The maintainers' settings on this computer, when they keep them in the repository (git-ignored)
[ -f .release.env ] && . ./.release.env
SITE=https://openfitnessprotocol.com
BUCKET=ofp-releases
OUT=release

die() { echo "release: $*" >&2; exit 1; }
version() { sed -n 's/^version = "\([0-9.]*\)"/\1/p' Cargo.toml | head -1; }
newer() { [ "$(printf '%s\n%s\n' "$1" "$2" | sort -t. -k1,1n -k2,2n -k3,3n | tail -1)" = "$1" ] && [ "$1" != "$2" ]; }
cloudflare() {
  if [ -n "$CLOUDFLARE_GLOBAL_API_KEY" ] && [ -z "$CLOUDFLARE_API_KEY" ]; then export CLOUDFLARE_API_KEY="$CLOUDFLARE_GLOBAL_API_KEY"; fi
  if [ -n "$CLOUDFLARE_API_KEY" ] && [ -n "$CLOUDFLARE_EMAIL" ]; then unset CLOUDFLARE_API_TOKEN; fi
  [ -n "$CLOUDFLARE_ACCOUNT_ID" ] || die "CLOUDFLARE_ACCOUNT_ID is not set (see site/deploy.sh)"
}
put() { # put <file> <key> <content-type> [cache-control]
  npx wrangler r2 object put "$BUCKET/$2" --file "$1" --content-type "$3" ${4:+--cache-control "$4"} --remote >/dev/null
  echo "  uploaded $2"
}

prepare() {
  V=$1
  echo "$V" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' || die "the version is x.y.z, got '$V'"
  [ "$(git rev-parse --abbrev-ref HEAD)" = main ] || die "not on main"
  [ -z "$(git status --porcelain)" ] || die "the working tree is not clean"
  git fetch -q origin main
  [ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || die "main is not the same as origin/main (pull or push first)"
  git rev-parse -q --verify "refs/tags/v$V" >/dev/null && die "the tag v$V exists already"
  LAST=$(node -e 'const r=require("./site/releases.json");console.log(r.map(x=>x.version).sort((a,b)=>a.localeCompare(b,undefined,{numeric:true})).pop()||"")')
  [ -z "$LAST" ] || newer "$V" "$LAST" || die "$V is not newer than the last release, $LAST"
  if grep -q "^## $V" CHANGELOG.md; then :
  elif grep -q '^## Next' CHANGELOG.md; then
    awk '/^## Next/{f=1;next} /^## /{f=0} f&&/^- /{n++} END{exit n?0:1}' CHANGELOG.md || die "CHANGELOG.md: '## Next' has no notes"
    sed -i '' "s/^## Next\$/## $V ($(date +%Y-%m-%d))/" CHANGELOG.md
  else die "CHANGELOG.md has neither '## Next' nor '## $V'"; fi
  sed -i '' "s/^version = \"[0-9.]*\"/version = \"$V\"/" Cargo.toml
  node -e 'const f="packages/ofp-js/package.json",p=require("./"+f);p.version=process.argv[1];require("fs").writeFileSync(f,JSON.stringify(p,null,2)+"\n")' "$V"
  cargo update -w -q
  echo "Testing"
  npm test >/dev/null 2>&1 || { npm test; die "tests failed"; }
  git add -A
  git commit -q -s -m "Release $V"
  git tag -a "v$V" -m "OFP Studio $V"
  git push -q origin main "v$V"
  echo "Released $V in git (commit and tag pushed)"
}

mac() {
  V=$(version)
  rm -rf target/universal-apple-darwin/release/bundle
  scripts/app-build.sh --target universal-apple-darwin
  APP="target/universal-apple-darwin/release/bundle/macos/OFP Studio.app"
  DMG=$(ls target/universal-apple-darwin/release/bundle/dmg/*.dmg | head -1)
  [ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$APP/Contents/Info.plist")" = "$V" ] || die "the app is not version $V"
  spctl -a -vv "$APP" 2>&1 | grep -q 'Notarized Developer ID' || die "the app is not notarized"
  spctl -a -t open --context context:primary-signature -vv "$DMG" 2>&1 | grep -q 'Notarized Developer ID' || die "the disk image is not notarized"
  mkdir -p $OUT
  cp "$DMG" "$OUT/OFP-Studio-$V-mac.dmg"
  echo "Mac: $OUT/OFP-Studio-$V-mac.dmg"
}

windows() {
  V=$(version)
  [ -n "$OFP_WINDOWS_SSH" ] || die "OFP_WINDOWS_SSH is not set: no Windows build"
  DIR=${OFP_WINDOWS_DIR:-'C:\Projects\openfitnessprotocol'}
  set -- $OFP_WINDOWS_SIGN
  SIGN=${1:+-Endpoint $1 -Account $2 -Profile $3}
  # One connection: fetch the tag, build, and say where the installer is
  ssh "$OFP_WINDOWS_SSH" "cd /d $DIR && git fetch -q --tags origin && git checkout -q v$V && powershell -NoProfile -ExecutionPolicy Bypass -File scripts\\app-build-windows.ps1 $SIGN"
  mkdir -p $OUT
  scp "$OFP_WINDOWS_SSH:$(echo "$DIR" | tr '\\' '/')/target/release/bundle/nsis/OFP Studio_${V}_x64-setup.exe" "$OUT/OFP-Studio-$V-windows-setup.exe"
  echo "Windows: $OUT/OFP-Studio-$V-windows-setup.exe"
}

publish() {
  V=$(version)
  cloudflare
  DATE=$(git log -1 --format=%cs "v$V" 2>/dev/null || date +%Y-%m-%d)
  [ -f "$OUT/OFP-Studio-$V-mac.dmg" ] || die "$OUT/OFP-Studio-$V-mac.dmg is missing (scripts/release.sh mac)"
  echo "Uploading $V"
  put "$OUT/OFP-Studio-$V-mac.dmg" "download/mac/OFP-Studio-$V-mac.dmg" application/x-apple-diskimage
  put "$OUT/OFP-Studio-$V-mac.dmg" "download/mac/OFP-Studio-latest-mac.dmg" application/x-apple-diskimage "public, max-age=60"
  if [ -f "$OUT/OFP-Studio-$V-windows-setup.exe" ]; then
    put "$OUT/OFP-Studio-$V-windows-setup.exe" "download/windows/OFP-Studio-$V-windows-setup.exe" application/x-msdownload
    put "$OUT/OFP-Studio-$V-windows-setup.exe" "download/windows/OFP-Studio-latest-windows-setup.exe" application/x-msdownload "public, max-age=60"
  else
    echo "  no Windows installer for $V: the site offers the Mac app only"
  fi
  # The list of releases (committed), the pages, and last the file running Studios check
  node scripts/release-record.mjs "$V" "$DATE" "$OUT" "$SITE"
  site/deploy.sh >/dev/null
  echo "  site deployed"
  put "$OUT/latest.json" download/latest.json application/json "public, max-age=60"
  if [ "$(gh repo view --json visibility -q .visibility 2>/dev/null)" = PUBLIC ]; then
    NOTES=$(node --input-type=module -e "import {readChangelog} from './site/build.mjs'; import {readFileSync} from 'fs'; const c=readChangelog(readFileSync('CHANGELOG.md','utf8')); console.log((c['$V']||{notes:[]}).notes.map(n=>'- '+n).join('\n'))")
    gh release view "v$V" >/dev/null 2>&1 || gh release create "v$V" "$OUT"/OFP-Studio-"$V"-* --title "OFP Studio $V" --notes "$NOTES

Downloads and every version: $SITE/download"
    echo "  GitHub release v$V"
  else
    echo "  the repository is not public: no GitHub release"
  fi
  if [ -n "$(git status --porcelain site/releases.json)" ]; then
    git add site/releases.json
    git commit -q -s -m "Published $V"
    git push -q origin main
  fi
  echo "Published $V: $SITE/download"
}

case "$1" in
  prepare) prepare "$2" ;;
  mac) mac ;;
  windows) windows ;;
  publish) publish ;;
  [0-9]*) prepare "$1"; mac; if [ -n "$OFP_WINDOWS_SSH" ]; then windows; fi; publish ;;
  *) sed -n '2,20p' "$0"; exit 1 ;;
esac
