#!/bin/sh
# Sets the rules for the main branch on GitHub, and the labels the forms and the scope check use.
# Run it ONCE, AFTER the repository is public (GitHub does not offer branch rules on a private repository
# of a free organisation), as a repository admin:   scripts/github-rules.sh
#
# What it sets on main:
#   - nobody pushes to main: every change is a pull request
#   - a pull request needs: the "check" and "scope" checks green, and an approving review from a code
#     owner (.github/CODEOWNERS)
#   - no force-push, no deleting the branch
#   - one person may go past the rule: the GitHub user in BYPASS_USER (default: whoever runs this). Without
#     that, the only code owner could never merge their own pull request.
#
# It has not been run yet (the repository was private when it was written): read GitHub's answer when you
# run it. The request follows https://docs.github.com/en/rest/repos/rules ("Create a repository ruleset").
set -e
REPO=${REPO:-trainerday/openfitnessprotocol}
BYPASS_USER=${BYPASS_USER:-$(gh api user -q .login)}
BYPASS_ID=$(gh api "users/$BYPASS_USER" -q .id)

if [ "$(gh api "repos/$REPO" -q .visibility)" != "public" ]; then
  echo "$REPO is not public yet: GitHub will refuse branch rules. Make it public first." >&2
  exit 1
fi

for label in "scope accepted|0e8a16|A maintainer agreed that this pull request may touch more than registry/" \
             "core request|d93f0b|A table could not express a device: a change to the core is asked for" \
             "device request|1d76db|A device someone wishes worked"; do
  name=${label%%|*}; rest=${label#*|}; color=${rest%%|*}; text=${rest#*|}
  gh label create "$name" --repo "$REPO" --color "$color" --description "$text" --force
done

echo "Rules for main; $BYPASS_USER (id $BYPASS_ID) may bypass."
gh api -X POST "repos/$REPO/rulesets" --input - <<JSON
{
  "name": "main: pull requests only, checks green, code owner approves",
  "target": "branch",
  "enforcement": "active",
  "bypass_actors": [{ "actor_id": $BYPASS_ID, "actor_type": "User", "bypass_mode": "always" }],
  "conditions": { "ref_name": { "include": ["~DEFAULT_BRANCH"], "exclude": [] } },
  "rules": [
    { "type": "deletion" },
    { "type": "non_fast_forward" },
    { "type": "pull_request", "parameters": {
        "required_approving_review_count": 1,
        "dismiss_stale_reviews_on_push": true,
        "require_code_owner_review": true,
        "require_last_push_approval": false,
        "required_review_thread_resolution": false } },
    { "type": "required_status_checks", "parameters": {
        "strict_required_status_checks_policy": false,
        "required_status_checks": [{ "context": "check" }, { "context": "scope" }] } }
  ]
}
JSON
echo "Done. See: https://github.com/$REPO/settings/rules"
