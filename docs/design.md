# Open Fitness Protocol (OFP) — design

Status: **draft, nothing built yet.** This records the design sessions of 2026-11-20. Everything
can still change until 1.0. Parts marked *unproven* have not been prototyped.

## The idea in one paragraph

OFP connects to fitness devices that an app does not support by itself and hands the app one
standard, GATT-style stream of named fields. To the app, OFP looks like one more sensor profile next
to Heart Rate and FTMS: support that profile once, and every device behind it arrives the same way.
New fields and new devices are added to this repo with a pull request, not by a committee.

```
 devices                    OFP                         app
 broadcast-only bike ─┐   ┌──────────────────────┐
 breathing sensor    ─┼──▶│ readers + tables     │──▶ one stream of named fields
 heart-rate strap    ─┘   │ → fields             │
                          └──────────────────────┘
```

## The core: four parts

Everything else in this document is optional and comes later.

### 1. Fields

A registry with one file per field: id, name, unit, type, scale, plausible range, sample kind, and
one paragraph saying what it means.

- **Append-only.** Once a field is stable, its id, unit and meaning never change and ids are never
  reused. An automated check rejects any pull request that edits a stable entry.
- **Two tiers.** `experimental` passes the format check and is merged (there is also a private-use
  id range that needs no pull request). `stable` needs a working implementation, a real capture, and
  a fixed number of days with no objection.
- **Sample kinds.** `instantaneous` (power now), `event` (one breath, one button press),
  `cumulative` (total crank revolutions).
- **Superset of existing standards.** Every field in Heart Rate, Cycling Power, Cycling Speed and
  Cadence, Running Speed and Cadence and FTMS has a registry id from the start.

### 2. Readers

A reader is code in the core that knows one *shape* of device communication. Readers are written
once, reviewed by maintainers, and are few:

| Reader | Shape |
|---|---|
| Broadcast | Values inside the BLE advertisement; the device never accepts a connection |
| Notify | Connect, subscribe to a characteristic, fixed layout |
| Flagged notify | The same, with optional values switched on by flag bits |
| Standard profiles | Heart Rate, Cycling Power, Speed and Cadence, Running Speed, FTMS data — layout already fixed by the Bluetooth standards, so no table is needed |

Because the standard profiles are readers too, an app can use OFP as its only way of reading
sensors. Existing apps do not have to: OFP can sit beside the parsers an app already has.

A new reader is requested with an issue (device, capture, how it sends data) and added to the core
with a full code review. This should be rare.

### 3. Tables

A table teaches a reader about one device. It is data, not code:

- which device it matches (advertised name, company id, service),
- which reader it uses,
- which bytes at which scale become which field,
- conditions (use this packet only when byte 2 is 0; this value exists from firmware build X),
- bit masks and simple arithmetic,
- which value tells two identical devices apart (for example a bike number), so the rider can pick,
- after how long without a packet the values read as zero.

Tables live in this repo. Every table ships with a real capture and the expected output; the
automated checks replay it.

### 4. Output

One GATT-style profile: the source describes its fields once (id, unit, type, name, sample kind,
tier, source identity), then sends compact values. An app can show and record a field it has never
heard of.

- Events are sent immediately; sampled values may be batched.
- When OFP runs inside the app, the app receives the same messages directly, with no radio in
  between. When OFP runs as a separate bridge, the same messages travel over BLE or the network.
  The app-side code is the same either way.

## Where OFP runs

- **The OFP library** — the core, embedded in an app. It contains **no Bluetooth**: the app passes
  in what its own Bluetooth layer hears and performs the writes the library asks for. This is how
  an app adds OFP without a second Bluetooth stack fighting its own.
- **An OFP bridge** (later) — the same library with its own radio and its own outputs: an ESP32
  stick, a desktop background app, or an Android app.

The small interface between the library and its host (advertisement seen, connect, subscribe,
read, write, disconnected) is part of the standard and is frozen like stable fields. Broadcast-only
devices put one real requirement on the host: continuous scanning with duplicates during the
session.

The core is written in **Rust** and compiled for desktop, iOS, Android and microcontrollers, so a
table behaves the same everywhere. The automated checks apply a microcontroller's memory and time
limits from the start, so everything written before the stick exists will run on it.

Phones:

- **iPhone** — the library inside each app, or a stick. A separate background app cannot reliably
  serve another app on the same phone. Several apps can share one sensor connection, so two apps
  that each embed the library do not fight over a device.
- **Android** — the library, or a standalone bridge app running as a foreground service that other
  apps reach over loopback.

## What a consuming app does

"Support it once" only holds if the app does something useful with a field it has never seen:

- **Show it** — a generic tile built from the description.
- **Record it** — as a stream next to power and heart rate.
- **Export it** — as a FIT developer field, so it survives into other tools.
- **Choose between sources** when two send the same field.
- **Keep experimental and unreviewed fields apart** from stable ones.

## Rules of the official registry

- **Tables only.** The official registry hosts tables, never code. A table cannot perform a
  handshake or decrypt anything.
- **No unlocks.** A table may decode what a device sends openly. Nothing in the registry may defeat
  authentication, encryption or pairing that the maker put there to restrict access. Edge cases
  (a fixed "start sending" command) are ruled on one by one and the rulings are kept in the repo.
- **Verification levels.**

  | Level | What it takes |
  |---|---|
  | Submitted | The author's capture, replayed by the automated checks |
  | Confirmed | A second person with the same device adds a capture or signs off |
  | Field-proven | Apps report that real riders connected and got sensible data |

  The core checks every value against the field's plausible range. Apps may report anonymous,
  opt-in counts (table, version, device firmware, share of packets decoded, values in range — no
  rider, no values) by submitting periodic summaries to this repo. A table that starts failing
  drops a level and its author is told.

## Your own plugins, by URL (later)

People can do what they want on their own install.

- A rider gives OFP the URL of a plugin. OFP shows what it claims (name, author, device, fields,
  whether it sends commands), warns that it is **not reviewed**, and the rider confirms.
- A URL plugin may be **code**. It runs in a WebAssembly sandbox whose entire outside world is:
  receive a packet from its device, write to its device, set a timer, emit a field, write a log
  line. No files, no network, nothing about the rider or the app. Memory and time per call are
  capped. *Unproven on microcontrollers; app-store rules for downloaded code on iPhone need
  checking.*
- The rider attaches it to a device explicitly; it is pinned to the version they approved; its
  fields stay labelled unreviewed. Sending commands to the device is a declared permission, off by
  default.
- The project does not host or list such plugins.

This is also the development loop: write a table, host it anywhere, load it by URL, ride with it,
then submit the same file as a pull request.

URL plugins are the lab. When several of them need the same thing (for example values split across
packets), that pattern can be proposed as a new reader. Once it is in the core, those devices
become tables and can enter the official registry.

## Later, and optional

None of these changes the four core parts.

- **Bridges** — the ESP32 stick (flashed from a web page), the desktop app, the Android app.
- **The remote** — a small frozen set of requests (info, scan, connect / forget, install, status)
  in the same profile as the data, so a bridge with no screen is set up from inside whichever app
  the rider uses. The bridge owns the sensor connections, stores its own configuration, and lets
  one app configure at a time.
- **Transports** — BLE; GATT tunnelled over TCP with mDNS discovery (documented here, compatible
  with the "direct connect" model trainers use); loopback on the same computer; WebSocket, which
  also lets browser apps and simple scripts connect.
- **Local push** — anything that is not BLE (a script, a camera tool, a DIY board) sends fields in
  over loopback or WebSocket.
- **Cloud relay** — the same messages through a server. The protocol can describe it so anyone can
  run one; the project does not host one.
- **Inputs** — buttons, shifters and steering are event fields. OFP mirrors OpenBikeControl's
  inputs and can speak it outward instead of defining a competing vocabulary.
- **Existing standards outward** — a bridge can also expose plain Heart Rate / Cycling Power / FTMS
  so apps that know nothing about OFP still work.
- **Control** — telling a trainer what to do is out of scope for now. Reserved at almost no cost: a
  version byte in the description, a read/write flag on every field, a reserved control
  characteristic. Until it exists, an OFP-only app can read a trainer but not control it.

## First version

Small enough to hold in your head:

- the library (Rust core, no Bluetooth),
- the broadcast reader,
- one table: the Keiser M3i, whose broadcast needs conditions, a bit mask, arithmetic, a bike-number
  picker and a staleness time,
- about ten fields,
- the output format,
- running inside one desktop app, checked against packets with known answers and against an
  existing hardware bridge for the same bike.

Next: a device that connects and notifies (the notify reader), then the standard-profile readers.

The repo becomes public when the first version runs end to end, a newcomer can add a field and a
table by following the README, and the license, governance and contributing documents exist.

## How the repo governs

- The rules live here and are enforced by automated checks: format, append-only, capture replay,
  resource limits.
- Stable changes use lazy consensus: open for a fixed number of days, approved by maintainers,
  merged if nobody objects.
- Created and maintained by TrainerDay, open to everyone. Maintainers are added from people whose
  tables, fields and readers have been merged.
- Anything calling itself OFP-compatible passes the conformance tests in this repo.
- License: undecided. Working assumption: a permissive license with a patent grant for code and an
  open documentation license for the specification.

## Related work

- **Heart Rate, Cycling Power, Speed and Cadence, Running Speed, FTMS** — the Bluetooth standards
  OFP reads and imports as fields.
- **QZ (qdomyos-zwift)** — a bridge that makes many proprietary machines appear as standard
  trainers, including control. OFP does not try to replace it.
- **OpenBikeControl** — an open protocol for buttons and other inputs.
- **FIT developer fields and USB HID descriptors** — the models for self-describing data.

## Open questions

- The table language: exact syntax for conditions, masks and arithmetic.
- The description and value layout, and how long descriptions are split over BLE.
- How official tables are signed and delivered to apps (a signed index on static hosting is the
  working idea), and offline behaviour.
- How long "no objection" lasts and how many approvals a stable change needs.
- WebAssembly on a microcontroller next to two BLE roles on one radio.
- App-store rules for URL-loaded code on iPhone.
- Whether offering a URL loader carries legal exposure when people load unlock plugins (a question
  for a lawyer).
- License.
- What to call the pieces: "library" and "bridge" are working names.
