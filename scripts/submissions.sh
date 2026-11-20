#!/bin/sh
# Recordings people sent from OFP Studio ("Send to OFP"). They are kept privately in the R2 bucket
# ofp-submissions for 90 days; only maintainers with the Cloudflare credentials can read them.
#
#   scripts/submissions.sh list            the codes, newest first, with the device and comment
#   scripts/submissions.sh get <code>      the recording and its note into captures/submitted/<code>.*
#
# Needs CLOUDFLARE_ACCOUNT_ID and CLOUDFLARE_EMAIL + CLOUDFLARE_GLOBAL_API_KEY (or CLOUDFLARE_API_KEY), or
# CLOUDFLARE_API_TOKEN with R2 rights (as for site/deploy.sh; .release.env sets them for maintainers).
set -e
cd "$(dirname "$0")/.."
[ -f .release.env ] && . ./.release.env
BUCKET=ofp-submissions
[ -n "$CLOUDFLARE_ACCOUNT_ID" ] || { echo "CLOUDFLARE_ACCOUNT_ID is not set" >&2; exit 1; }
KEY=${CLOUDFLARE_API_KEY:-$CLOUDFLARE_GLOBAL_API_KEY}
api() {
  if [ -n "$KEY" ] && [ -n "$CLOUDFLARE_EMAIL" ]; then
    curl -sf -H "X-Auth-Email: $CLOUDFLARE_EMAIL" -H "X-Auth-Key: $KEY" "https://api.cloudflare.com/client/v4/accounts/$CLOUDFLARE_ACCOUNT_ID/r2/buckets/$BUCKET$1"
  else
    curl -sf -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" "https://api.cloudflare.com/client/v4/accounts/$CLOUDFLARE_ACCOUNT_ID/r2/buckets/$BUCKET$1"
  fi
}
keys() { # every object key (at most a few thousand: 90 days of submissions)
  cursor=""
  while :; do
    page=$(api "/objects?per_page=1000${cursor:+&cursor=$cursor}")
    echo "$page" | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{const j=JSON.parse(s);for(const o of j.result||[])console.log(o.key)})'
    cursor=$(echo "$page" | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{const j=JSON.parse(s);const r=j.result_info||{};console.log(r.is_truncated&&r.cursor||"")})')
    [ -n "$cursor" ] || break
  done
}
object() { # object <key>: its body on stdout
  api "/objects/$(node -e 'console.log(encodeURIComponent(process.argv[1]))' "$1")"
}

case "$1" in
  list)
    for k in $(keys | grep '\.json$' | sort -r); do
      object "$k" | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{const m=JSON.parse(s);console.log([m.code,m.received.slice(0,16).replace("T"," "),[m.make,m.model].filter(Boolean).join(" ")||m.device||"?",m.read_as||"",(m.comment||"").replace(/\s+/g," ").slice(0,80)].join("  |  "))})'
    done
    ;;
  get)
    CODE=$(echo "$2" | tr '[:lower:]' '[:upper:]')
    [ -n "$CODE" ] || { echo "usage: scripts/submissions.sh get OFP-XXXX-XX" >&2; exit 1; }
    BASE=$(keys | grep "/$CODE\.json\$" | sed 's/\.json$//' | head -1)
    [ -n "$BASE" ] || { echo "$CODE: not found (sent more than 90 days ago, or mistyped)" >&2; exit 1; }
    mkdir -p captures/submitted
    object "$BASE.json" > "captures/submitted/$CODE.json"
    object "$BASE.capture.jsonl" > "captures/submitted/$CODE.capture.jsonl"
    cat "captures/submitted/$CODE.json"
    echo "captures/submitted/$CODE.capture.jsonl ($(wc -l < "captures/submitted/$CODE.capture.jsonl" | tr -d ' ') lines). Look at it: ofp inspect captures/submitted/$CODE.capture.jsonl"
    ;;
  *) sed -n '2,10p' "$0"; exit 1 ;;
esac
