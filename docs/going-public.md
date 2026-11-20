# Making the repository public

Written 2026-11-20, while the repository was still private. What is ready, what to look at before the
switch, and what to set on GitHub right after it.

## Ready in the repository

| What | Where |
|---|---|
| The rules for adding a device, for people and AI assistants; "stay in the table, do not change the core"; what to do when a table cannot express a device | `AGENTS.md` (also served by the Studio at `/api/guide`) |
| How a pull request gets in | `CONTRIBUTING.md` |
| Issue forms: "A device I wish worked", "A table cannot express this device" | `.github/ISSUE_TEMPLATE/` |
| The pull request checklist | `.github/pull_request_template.md` |
| Who must approve | `.github/CODEOWNERS` (one maintainer today) |
| Check **check**: registry, recordings, tests | `.github/workflows/check.yml` |
| Check **scope**: fails a pull request that touches anything outside `registry/` unless a maintainer put the label "scope accepted" on it | `.github/workflows/scope.yml` |
| How to report a security problem; what a table and a plugin can never do | `SECURITY.md`; `AGENTS.md`, "What a table may never do" |
| The names: who holds the trademarks, what anyone may do with them | `TRADEMARKS.md`, `NOTICE` (a draft: not read by a lawyer) |
| The branch rules and labels, as one command | `scripts/github-rules.sh` (not run yet) |

## Look at these before the switch

- **The site** `openfitnessprotocol.org` is live since 2026-11-20 (`.com` and `www.` redirect to it): the
  pages and the downloads (`docs/releasing.md`). Its links to GitHub open once the repository is public.

- **Who can write.** Today six accounts can push: three admins and three with write access. After the
  branch rules, write access can no longer reach `main` without a code owner's approval, but **an admin
  can change or remove the rules**. Whoever should not be able to does not stay an admin.
- **The history becomes public too.** 23 commits when this was written, all by one author with a
  `@trainerday.com` address; a search of the files and the history found no keys, passwords or paths of
  anyone's computer. `docs/plan-v1.md` (it names TrainerDay internals) is kept out by a local git exclude:
  check it is still untracked (`git ls-files docs/plan-v1.md` prints nothing).
- **The recording that ships with the Keiser table** (`registry/tables/keiser-m3i.capture.jsonl`) is a real
  24-minute ride. Open it once: it holds power, cadence and the bike's number, and would hold a heart rate
  if a strap had been paired to the bike.
- **The checks cost nothing on a public repository** (GitHub Actions is free there).

## Right after the switch

1. Settings → General → make public.
2. `scripts/github-rules.sh` (as an admin). It creates the three labels and the rule for `main`: pull
   requests only, **check** and **scope** green, a code owner approves, no force-push. One user may go
   past the rule (default: whoever runs the script), because the only code owner cannot approve their own
   pull request.
3. Settings → Advanced Security → turn on **Private vulnerability reporting**, so a security problem is
   not reported in a public issue.
4. Settings → Actions → General → "Require approval for first-time contributors" (the default for public
   repositories; check it is on), so a stranger's pull request does not run workflows before someone looked.
5. Optional: the [DCO app](https://github.com/apps/dco) checks the sign-off line on every commit, which
   `AGENTS.md` asks for and nothing enforces today.

## How a change reaches main after that

- **A table** (only `registry/` touched): **scope** passes by itself, **check** replays the recording, a
  maintainer approves and merges.
- **Anything else**: **scope** fails and says why. The author opens "A table cannot express this device"
  (or explains the change); a maintainer who agrees adds the label "scope accepted", reviews the code and
  merges. The maintainers' own pull requests take the same label.
