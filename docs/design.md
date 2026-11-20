# Open Fitness Data (OFD) — design

Status: **draft, nothing built yet.** This is the design agreed in the first design session
(2026-11-20). Everything here can still change until 1.0. Parts marked *unproven* have not been
prototyped.

## What it is

An **OFD** is a small piece of software that sits between fitness devices and fitness apps. It
connects to sensors and input devices, turns what they send into a shared set of named fields, and
offers those fields to any app over Bluetooth Low Energy (BLE), WiFi, or a local connection on the
same computer.

```
 devices               an OFD                      consumers
 breathing sensor ─┐   ┌────────────────────┐ BLE   ┌─ training apps
 core temperature ─┼──▶│ plugins → fields   │ WiFi ─┼─ recording apps
 buttons, DIY     ─┘   └────────────────────┘ local └─ anything else
```

The same OFD runs in three places:

- **ESP32 stick** — a cheap always-on board. This is the smallest OFD and the reference target.
- **Computer** — a background app on Windows, macOS or Linux.
- **Phone** — the same core built into a consumer app as a library (a separate background app
  cannot reliably serve another app on the same phone).

## Why

- Standards such as FTMS only change when a committee agrees. Adding one field (torque per
  repetition, breathing rate, core temperature) can take years or never happen.
- New sensors therefore ship with proprietary protocols, and every app has to write its own
  connector for every device.
- OFD moves both decisions into a public git repo: a field or a device is added with a pull
  request, and the rules are enforced by automated checks instead of by meetings.

The goal for a consumer app: **implement OFD once, and never write another device connector.**

## Scope

| In scope now | Considered, built later | Out of scope |
|---|---|---|
| Sensor data flowing in (breathing, temperature, muscle oxygen, power, heart rate, …) | Extended control beyond what FTMS offers | Replacing trainer-control bridges such as QZ |
| Input events flowing in (buttons, shifters, steering) | | |
| Managing an OFD from inside a consumer app | | |

"Data" means everything coming *from* devices, whether a number every second or a button press now
and then. "Control" means telling a trainer what to do; it is not designed here, but the wire
format reserves room for it (see [Reserved for control](#reserved-for-control)).

## Three layers that move at different speeds

Stability comes from keeping these apart.

### 1. Field registry — moves fast

One file per field: id, name, unit, type, scale, sample kind, and one paragraph saying what it
means.

- **Append-only.** Once a field is stable, its id, unit and meaning never change, and ids are
  never reused. An automated check rejects any pull request that edits a stable entry.
- **Two tiers.**
  - `experimental` — passes the format check and is merged. There is also a private-use id range
    that needs no pull request at all.
  - `stable` — needs one working implementation, one real packet capture, and a fixed number of
    days with no objection.
- **Superset of existing standards.** Every field in FTMS, Cycling Power, Heart Rate and Running
  Speed and Cadence gets a registry id on day one.
- **Sample kinds.** `instantaneous` (power now), `event` (one breath, one button press) and
  `cumulative` (total crank revolutions).

Devices can ship a new field first and standardize it afterwards.

### 2. Wire format — moves almost never

- **Self-describing.** A source first publishes a descriptor (field id, unit, type, name, sample
  kind, tier, source identity), then sends compact packets. A consumer can show and record a field
  it has never heard of.
- **Defined once as a GATT service**, carried three ways: BLE, GATT tunnelled over TCP with mDNS
  discovery (documented here, and kept compatible with the "direct connect" model trainers already
  use), and loopback for a consumer on the same computer.
- **Events are sent immediately**; sampled data may be batched.
- **Existing standards alongside.** Where it makes sense an OFD also exposes plain FTMS / Heart
  Rate / Cycling Power, and OpenBikeControl for buttons, so apps that know nothing about OFD still
  work.

### 3. Plugins — where contributors work

A plugin teaches an OFD to understand one device. The plugin format is whatever runs comfortably on
the ESP32; anything that runs there runs everywhere.

- **Declarative plugins** (the normal case): a data file saying which advertisement to match,
  which characteristic to read, and which bytes at which scale become which field, with formulas
  and a few built-in helpers (delta, rate, rolling average). No code runs.
- **Code plugins** (handshakes, checksums, encryption): a sandboxed WebAssembly module with a tiny
  fixed API — called on connect, on packet and on timer; may write to its own device and emit
  fields; nothing else. Memory and time per packet are capped. *Unproven on the ESP32.*
- **Every plugin ships with a real capture** and the expected output. The automated checks replay
  it in the same core, under the ESP32's limits.
- **Each plugin declares the advertisement it matches**, so an OFD can say "supported" before it
  connects to anything.

Three ways a data source can reach consumers:

1. **Native** — the device's own firmware is an OFD. No plugin needed.
2. **Plugin** — an OFD translates an existing proprietary device.
3. **Local push** — anything that is not BLE (a script, a camera tool, a DIY sensor on USB) sends
   fields to an OFD's local or WiFi endpoint.

## What a consumer has to do

"No more connectors" is only true if a consumer does something useful with a field it has never
seen. The consumer contract:

- **Show it** — a generic tile built from the descriptor.
- **Record it** — as a stream next to power and heart rate.
- **Export it** — as a FIT developer field, so it survives into other tools.
- **Choose between sources** when two send the same field.
- **Treat experimental fields as extra data**, separate from stable ones.

## Managing an OFD from a consumer (the OFD remote)

A stick has no screen, so setup happens inside whichever app the rider already uses. The remote is
a small fixed set of requests in the same service as the data, identical over every transport:

| Request | What it does |
|---|---|
| Info | Version, how many sensors it can hold, which plugins it has |
| Scan | Nearby devices, each marked supported / unknown |
| Connect / forget | Add or remove a device |
| Install plugin | The consumer downloads the plugin and pushes it to the OFD (a stick has no internet) |
| Status | Per source: connected, battery, signal, time of last packet |

What the rider sees:

1. In the app's devices screen, "Other sensors". The app finds any OFD by itself.
2. The OFD lists what it hears nearby.
3. The rider taps a device. Its data appears.
4. It is remembered, and it keeps working for other apps even when the first app is closed.

The rider never needs the words plugin, registry or field.

Rules that keep this safe and stable:

- The OFD owns the sensor connection, so several apps can share one sensor.
- Configuration is stored on the OFD; a consumer is needed to change it, not to run it.
- An OFD accepts only plugins signed by a registry release. Experimental plugins need an explicit
  "allow unreviewed" switch.
- One consumer configures at a time; others can read status.
- Remote requests are frozen like stable fields: new ones may be added, existing ones never change.

## Reserved for control

Not built, but kept open at almost no cost:

- a version byte in the descriptor,
- a read/write flag on every field,
- a reserved, unused control characteristic in the service.

## Implementation

- **One portable core in Rust**: the plugin interpreter and the packet codec, compiled for the
  ESP32, desktop, iOS and Android. One core is what makes a plugin behave the same everywhere.
- **ESP32 firmware**: the reference OFD. Flashed from a web page over USB.
- **Desktop app**: a shell around the same core.
- **Conformance tests**: anything calling itself an OFD passes the same tests in this repo.

Known limits of the stick: it holds only a handful of sensor connections while also serving a
consumer on the same radio, and it stores only the plugins the rider has turned on.

## How the repo governs

- The rules live in this repo and are enforced by automated checks: format, append-only, capture
  replay, resource limits.
- Stable changes use lazy consensus — open for a fixed number of days, approved by maintainers,
  merged if nobody objects.
- Created and maintained by TrainerDay, open to everyone. Maintainers are added from people whose
  plugins and fields have been merged.
- License: still to decide. The working assumption is a permissive license with a patent grant for
  code, and an open documentation license for the specification.

## Related work

- **FTMS, Cycling Power, Heart Rate, Running Speed and Cadence** — the Bluetooth standards OFD
  imports as fields and keeps emitting.
- **QZ (qdomyos-zwift)** — a bridge app that makes many proprietary machines appear as standard
  trainers. It covers control; OFD does not try to.
- **OpenBikeControl** — an open protocol for buttons and other inputs. OFD mirrors its inputs and
  speaks it outward instead of defining a competing one.
- **FIT developer fields and USB HID descriptors** — the models for self-describing data.

## First proof

Small enough to build, wide enough to test every part of the design:

- about twenty registry fields,
- one declarative plugin (a breathing sensor) and one for core temperature,
- one code plugin (a button controller), to test the WebAssembly tier,
- one local-push example,
- running on an ESP32 stick, shown in one consumer app as generic tiles, and written to the FIT
  file.

The repo becomes public when this runs end to end, a newcomer can add a field and a plugin by
following the README, and the license, governance and contributing documents are in place.

## Open questions

- Is WebAssembly practical on the ESP32 next to two BLE roles on one radio, or does the code tier
  need something smaller?
- Exact descriptor and packet layout, and how large descriptors are split over BLE.
- How plugin signing and registry releases work in practice.
- How many days "no objection" lasts, and how many maintainer approvals a stable change needs.
- License choice.
- Whether the name "Open Fitness Data" is distinctive enough for a compatibility badge.
