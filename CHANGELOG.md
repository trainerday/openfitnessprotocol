# Changes

What changed in each release of OFP Studio, the `ofp` command and the JavaScript package, newest first.
Published at https://openfitnessprotocol.org/releases.

How to write it: one line per thing a user would notice, new things first, at most about seven; small
fixes are folded into a last "Bug fixes and improvements". Notes for the next release go under `## Next`;
`scripts/release.sh` turns that heading into the version and date.

## Next

- Tymewear VitalPro: breathing rate and minute ventilation are now read correctly. They were shown at a
  quarter to an eighth of the real values (found with the first real sensor).
- OFP Studio is safer to leave running: only its own window or page can use it, and recordings open by name.
- Recordings and tables sent by others can no longer hang or crash OFP Studio or the `ofp` command.
- The site and the package's notes say which devices the package reads today.
- Bug fixes and improvements.

## 0.2.3 (2026-11-21)

- CORE body temperature sensors, read from their advertisement (core temperature) or connected (also skin
  temperature, the heart rate the sensor receives, and the heat strain index). From the maker's published
  specification, waiting for owners to test them.
- Tymewear VitalPro breathing sensors: breathing rate, tidal volume and minute ventilation, waiting for
  owners to test them.
- For table writers: vectors can now be a notification, for tables of devices that are connected to.
- For app makers: the noble adapter passes the advertised services and names, so devices that are
  connected to are recognised.
- Bug fixes and improvements.

## 0.2.2 (2026-11-20)

- Every number can be marked right or wrong, RR intervals too.
- For app makers: the library is on npm, `npm install @openfitnessprotocol/ofp`, with TypeScript types for
  React Native's plain-JavaScript core and the device tables.

## 0.2.1 (2026-11-20)

- Heart rate straps: RR intervals (the time between beats, for HRV), one value per beat.

## 0.2.0 (2026-11-20)

- A simpler Studio: connect your Bluetooth device, check your numbers, and send the recording to us
  with one button. The full Studio is under Advanced.
- My recordings: every session you connected, to send or download, with the code of each one you sent.
- Reads the standard sensors: heart rate straps, power meters, speed and cadence sensors, running
  sensors, and FTMS bikes, rowers and treadmills. Reading only: OFP never controls a device.
- First drafts for more spin bikes, waiting for owners to test them: Domyos, older Yesoul, iConsole+
  consoles, Nautilus, Inspire IC, XCX and Chrono.
- A device that only sends after a fixed start command can now be read (never one with a lock).
- Bug fixes and improvements.

## 0.1.0 (2026-11-20)

- OFP Studio as an app for Mac and Windows, with its own window: no browser or terminal needed.
- Devices: the ones something can read come first, fitness devices nothing reads yet are listed below
  them, and everything else is one click away under "Don't see your device?".
- Live: what your device sends right now, grouped by what reads it, with "Yes, this is right" or "No,
  this is wrong" for each value. Stop and save gives you one file to send to whoever is helping.
- Recordings: every earlier recording in its own tab, and opening a file someone sent you.
- Devices you can try: the Keiser M3i (checked on a real ride), and first drafts for the CORE body
  temperature sensor and the Tymewear VitalPro breathing sensor, waiting for owners to try them.
- The Studio tells you when a newer version is available.
