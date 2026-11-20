# Releasing OFP Studio

One version for everything released from this repository: OFP Studio (Mac and Windows), the `ofp` command and
the JavaScript package (`packages/ofp-js`). The number lives in `Cargo.toml` (`[workspace.package] version`);
the release copies it into `packages/ofp-js/package.json`. The table format has its own versions.

`/version` (`.claude/commands/version.md`) walks an assistant through a release. The work is
`scripts/release.sh`.

## Where things are

| What | Where |
|---|---|
| The notes | `CHANGELOG.md`. Notes for the next release go under `## Next` |
| The version history | `site/releases.json` (committed: version, date, and for each platform the URL, size, SHA-256) |
| The site | `site/`: `build.mjs` makes the pages from the two files above, `worker.js` serves them, `deploy.sh` deploys. Cloudflare Worker `ofp-site` |
| The downloads | Cloudflare R2 bucket `ofp-releases`, served at `openfitnessprotocol.org/download/…` |
| What a running Studio checks | `openfitnessprotocol.org/download/latest.json`. A newer version shows a banner with a Download button |

```
download/mac/OFP-Studio-<version>-mac.dmg                    every version, kept
download/mac/OFP-Studio-latest-mac.dmg                       the newest (cached one minute)
download/windows/OFP-Studio-<version>-windows-setup.exe
download/windows/OFP-Studio-latest-windows-setup.exe
download/latest.json                                         written last
```

`openfitnessprotocol.com` and both `www.` names redirect to `openfitnessprotocol.org`.

## The notes

What a user reads about a release is short: at most about seven one-line bullets of what they would notice, new
things first; small fixes and internal work go into a last "Bug fixes and improvements". The detail is in the
commits.

## Releasing

```sh
. ./.release.env               # settings, see below
scripts/release.sh 0.1.1       # all four steps
```

| Step | What it does |
|---|---|
| `prepare <x.y.z>` | Refuses unless on `main`, clean, equal to `origin/main`, the version newer than the last release, and `CHANGELOG.md` has notes under `## Next`. Then: `## Next` becomes `## x.y.z (date)`, the version is set, `npm test`, commit "Release x.y.z" (signed off), tag `vx.y.z`, push both |
| `mac` | The universal Mac app (Apple Silicon and Intel), signed and notarized, and its disk image notarized (`scripts/app-build.sh`). Checks the version inside the app and that Gatekeeper accepts both. Copies the disk image to `release/` |
| `windows` | Over ssh on the Windows machine: fetch, check out the tag, `scripts\app-build-windows.ps1` (NSIS installer, signed with Azure Trusted Signing). Copies the installer to `release/`. Skipped when `OFP_WINDOWS_SSH` is not set |
| `publish` | Publishes the JavaScript package `@openfitnessprotocol/ofp` at the same version (when `NPM_TOKEN` is set; `scripts/npm-publish.sh`). Uploads the files and the "latest" copies, records the release in `site/releases.json` and writes `latest.json`, deploys the site, uploads `latest.json` last, creates the GitHub release (when the repository is public), commits "Published x.y.z" |

A failed step is run again on its own. `prepare` is the only one that must not run twice for one version.

### Settings (`.release.env`, git-ignored)

```sh
OFP_WINDOWS_SSH=<ssh host of the Windows machine>
OFP_WINDOWS_DIR='C:\Projects\openfitnessprotocol'                 # the default
OFP_WINDOWS_SIGN="<endpoint> <signing account> <certificate profile>"
CLOUDFLARE_ACCOUNT_ID=…   and CLOUDFLARE_API_TOKEN, or CLOUDFLARE_EMAIL + CLOUDFLARE_GLOBAL_API_KEY
NPM_TOKEN=…               an npm token for @openfitnessprotocol that bypasses two-factor (they expire: 90 days)
```

Mac signing and notarizing: a Developer ID certificate in the keychain and an App Store Connect API key
(`scripts/app-build.sh` names the variables). Without them the Mac app is built unsigned; a release should not
be.

### The Windows machine

Once: Visual Studio Build Tools with the C++ workload, Rust (`rustup`, toolchain `stable-x86_64-pc-windows-msvc`),
Node, git with access to the repository, a clone at `OFP_WINDOWS_DIR`. For signing: .NET 8, the PowerShell module
`TrustedSigning`, and `AZURE_TENANT_ID` / `AZURE_CLIENT_ID` / `AZURE_CLIENT_SECRET` for the signing service
principal. Signing needs someone logged in on the machine (`Access is denied` otherwise).

## Pulling a bad release

Running Studios only learn of a version through `latest.json`. To withdraw one: upload the previous version's
`latest.json` and its disk image as the "latest" copies again, remove the entry from `site/releases.json`,
`site/deploy.sh`, and commit. The files of the bad version can stay; nothing links to them. Then release a fixed
version with a higher number: a version number is never reused.

## The site alone

`site/deploy.sh` rebuilds and deploys the pages (for a change to the text, or a fixed note in `CHANGELOG.md`)
without a release.
