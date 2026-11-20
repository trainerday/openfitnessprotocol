# Contributing to Open Fitness Protocol

Thank you. Most contributions are one of these, from easiest to hardest:

| You want to | Do this |
|---|---|
| Say that a device should work | Open an issue: **"A device I wish worked"** |
| Add a device you own, or fix its table | Record it in the Studio, write or fix its **table**, open a pull request that touches only `registry/` |
| Confirm someone else's table on your device | Run the Studio, open **Live**, mark each value right or wrong, send the recording (the pull request or issue for that table) |
| Report that a table cannot express a device | Open an issue: **"A table cannot express this device"** |
| Change the core | Only after such an issue has been accepted. Link it in the pull request |

**The rules are in [`AGENTS.md`](AGENTS.md)**, written for people and for AI assistants alike. Read "The
whole job, start to finish" and "Stay in the table" before you start. The short version:

- A device is added by a **table** (data), not by changing the core (code).
- A table ships with a **real recording** and the owner's confirmations. The automated check replays the
  recording through the table; that check is the judge.
- It is **your own work**, from your own recordings or the maker's public documentation. Nothing copied
  from other projects (see the clean-room rule).
- Sign your commits off: `git commit -s` (Developer Certificate of Origin). Everything here is Apache-2.0.
- AI-assisted work is welcome. Say so in the pull request; a person stands behind it.

A security problem is reported privately, not in an issue: [`SECURITY.md`](SECURITY.md). The names
"Open Fitness Protocol" and "OFP": [`TRADEMARKS.md`](TRADEMARKS.md).

## How a pull request gets in

1. Open it against `main`. Nobody pushes to `main` directly.
2. Two automated checks run: **check** (the registry, the recordings, the tests) and **scope** (does the
   pull request stay inside `registry/`?).
3. A maintainer reviews it. A pull request that stays inside `registry/` and passes the check is usually
   quick. One that touches anything else needs the issue described above and a full code review.
4. A maintainer merges it.

## Try it first

`npm start` opens the Studio in your browser; `npm run app` opens it in a window of its own. You need
[Rust](https://rustup.rs). `npm test` runs everything the automated check runs.
