Release a new version of OFP Studio (the Mac app and the Windows installer), with its notes, the downloads on
openfitnessprotocol.com and the GitHub release.

**The runbook is [docs/releasing.md](../../docs/releasing.md). Read it before running this.** The work is done by
`scripts/release.sh`; this command decides the version, checks the notes, runs it, and checks the result.

## Input

Arguments: `$ARGUMENTS` — empty, `patch` / `minor` / `major`, or a version `x.y.z`. Add `mac` to skip Windows.

## 1. The version

- The last release is the newest entry in `site/releases.json` (none yet: the first release is the version in
  `Cargo.toml`, `0.1.0`).
- No argument: a **patch** bump (`0.1.3` → `0.1.4`). Minor and major bumps are rare and only on request. A
  given `x.y.z` is used as it is, and must be newer than the last release.
- Say the version you are going to release in one line.

## 2. What ships (always)

Run `git log <last tag>..HEAD --oneline` (all commits for the first release: say how many) and show the list.
Ask once: **"Have these been tried in the app?"** Stop if the answer is no. Skip only when the arguments say
`skip pre-flight`.

## 3. The notes

`CHANGELOG.md`, section `## Next`. What a user reads about a release is short:

- at most about **7 one-line bullets**, of what a user would notice, new things first, in plain words;
- small fixes, layout corrections, internal work, and details of something already named are folded into a
  last **"Bug fixes and improvements"**;
- nothing about the repository, CI, docs or refactoring unless a user sees it.

If `## Next` is missing or does not say what the commits above did, write it from the commits, show it, and
commit it (`docs: notes for <version>`) before going on. The script turns `## Next` into
`## <version> (<date>)`.

## 4. Release

```sh
[ -f .release.env ] && . ./.release.env     # the maintainers' settings (git-ignored): see docs/releasing.md
scripts/release.sh <version>                # prepare, mac, windows (when OFP_WINDOWS_SSH is set), publish
```

It takes about 15 minutes (two notarizations on the Mac, the Windows build). Run it in the background and wait
for it. When a step fails, fix the cause and run **that step** again (`scripts/release.sh mac`, `windows`,
`publish`): `prepare` has already committed, tagged and pushed, and must not run twice for one version.

- Windows: the build runs over ssh on the Windows machine and signs with Azure Trusted Signing. Signing fails
  with `Access is denied` when nobody is logged in there: ask for someone to log in, then `scripts/release.sh
  windows` and `publish`.
- `mac` in the arguments: leave `OFP_WINDOWS_SSH` unset for this run; the site then offers the Mac app only.

## 5. Check, then report

```sh
curl -s https://openfitnessprotocol.com/download/latest.json      # the new version, both files
curl -sI <each download url>                                        # 200, the size in releases.json
```

Open `https://openfitnessprotocol.com/download` and `/releases` and check the version and notes are there.

Report in a few lines: the version, the download page, which platforms, the GitHub release (or "the repository
is not public: none"), and anything that was skipped. Do not paste the notes back unless asked.
