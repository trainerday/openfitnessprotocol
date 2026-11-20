# Changes

What changed in each release of OFP Studio, the `ofp` command and the JavaScript package, newest first.
Published at https://openfitnessprotocol.org/releases.

How to write it: one line per thing a user would notice, new things first, at most about seven; small
fixes are folded into a last "Bug fixes and improvements". Notes for the next release go under `## Next`;
`scripts/release.sh` turns that heading into the version and date.

## 0.1.0 (2026-11-20)

- OFP Studio as an app for the Mac, with its own window: no browser or terminal needed.
- Devices: the ones something can read come first, fitness devices nothing reads yet are listed below
  them, and everything else is one click away under "Don't see your device?".
- Live: what your device sends right now, grouped by what reads it, with "Yes, this is right" or "No,
  this is wrong" for each value. Stop and save gives you one file to send to whoever is helping.
- Recordings: every earlier recording in its own tab, and opening a file someone sent you.
- Devices you can try: the Keiser M3i (checked on a real ride), and first drafts for the CORE body
  temperature sensor and the Tymewear VitalPro breathing sensor, waiting for owners to try them.
- The Studio tells you when a newer version is available.
