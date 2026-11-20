#!/bin/sh
# Builds and deploys openfitnessprotocol.org (the Cloudflare Worker "ofp-site", its pages and its routes).
# Needs, in the environment:
#   CLOUDFLARE_ACCOUNT_ID
#   and either CLOUDFLARE_API_TOKEN, or CLOUDFLARE_EMAIL with CLOUDFLARE_GLOBAL_API_KEY (or CLOUDFLARE_API_KEY).
#   A token needs Workers, Workers Routes / custom domains and R2 rights on both zones; the global key always has them.
set -e
cd "$(dirname "$0")"
if [ -n "$CLOUDFLARE_GLOBAL_API_KEY" ] && [ -z "$CLOUDFLARE_API_KEY" ]; then
  export CLOUDFLARE_API_KEY="$CLOUDFLARE_GLOBAL_API_KEY"
fi
if [ -n "$CLOUDFLARE_API_KEY" ] && [ -n "$CLOUDFLARE_EMAIL" ]; then
  unset CLOUDFLARE_API_TOKEN   # wrangler uses one or the other
fi
node build.mjs
exec npx wrangler deploy "$@"
