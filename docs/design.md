# Open Fitness Protocol (OFP) — design

Status: **first version built** (core, broadcast reader, one table, the `ofp` command). Everything
can still change until 1.0. Parts marked *unproven* have not been prototyped; the sections marked
*as built* describe what the code does today.

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

## The pieces and their names

| Name | What it is |
|---|---|
| **OFP** | The protocol and the project |
| **OFP Core** | The library: readers, the table engine and the output. No Bluetooth. Embedded in an app |
| **OFP Connect** | The standalone: Core plus its own radios and outputs — a stick, a desktop app or an Android app (later) |
| **OFP Registry** | The fields and tables in this repo |

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
- **Sample kinds.** `instantaneous` (power now; reads as zero when the source goes silent),
  `event` (one breath, one button press), `cumulative` (a running total such as energy), `state`
  (a setting that is neither a rate nor a total, such as the gear).
- **Superset of existing standards.** Every field in Heart Rate, Cycling Power, Cycling Speed and
  Cadence, Running Speed and Cadence and FTMS has a registry id from the start.
- **Values keep their context.** Power from a trainer, from a crank and from an estimate are not
  interchangeable. Every value travels with its source, a timestamp, a validity flag and, where it
  matters, where it was measured. OFP never silently merges two sources.

### 2. Readers

A reader is code in OFP Core that knows one *shape* of device communication. Readers are written
once, reviewed by maintainers, and are few:

| Reader | Shape |
|---|---|
| Broadcast | Values inside the BLE advertisement (manufacturer data or service data); the device never accepts a connection |
| Notify | Connect, subscribe to a characteristic, fixed layout |
| Flagged notify | The same, with optional values switched on by flag bits |
| Standard profiles | Heart Rate, Cycling Power, Speed and Cadence, Running Speed, FTMS data — layout already fixed by the Bluetooth standards, so no table is needed |

Because the standard profiles are readers too, an app can use OFP as its only way of reading
sensors. Existing apps do not have to: OFP can sit beside the parsers an app already has.

- **Generic when possible, specific when not.** A reader may be named after a single device when
  that device's shape is truly its own. It can be folded into a generic reader later, when a second
  device shows the same shape.
- **How a reader is proven generic:** a second, unrelated device is decoded by the same reader with
  only a new table.
- **What makes a device not generic:** values spread across several packet types that must be
  combined, checksums or rolling counters that must be validated, encrypted or obfuscated data.
- A new reader is requested with an issue (device, capture, how it sends data) and added to the
  core with a full code review. This should be rare.

### 3. Tables

A table teaches a reader about one device. It is data, not code:

- which device it matches (advertised name, company id, service),
- which reader it uses,
- which bytes at which scale become which field,
- conditions (use this packet only when byte 2 is 0; this value exists from firmware build X),
- bit masks and simple arithmetic,
- which value tells two identical devices apart (for example a bike number), so the rider can pick,
- after how long without a packet the values read as zero.

The table language stays clearly data: conditions, masks and arithmetic, but no loops and nothing
that amounts to a script. That keeps tables reviewable by non-programmers and safe to deliver to
phone apps.

Tables live in this repo. Every table ships with a real capture and the expected output; the
automated checks replay it.

#### The table language

*As built.* A table is a YAML file in `registry/tables/`. The full example is
`registry/tables/keiser-m3i.yaml`.

| Setting | Meaning |
|---|---|
| `id`, `name`, `version`, `status` | Identity. `id` is also the file name |
| `reader` | `broadcast` (the only reader so far) |
| `provenance`, `verified_on` | Where the knowledge comes from; the device and firmware it was checked on. Required |
| `match` | A list of rules; the first that fits wins. `manufacturer_id` (the Bluetooth company id at the start of the manufacturer data), `name_prefix`, `min_length` of the payload, and an optional `when` condition. A rule with no `manufacturer_id` reads a bare payload and must give `name_prefix` |
| `only_when` | Packets where this is false are ignored |
| `instance` | `label` and `value`: what tells two identical devices apart. It becomes part of the source id (`keiser-m3i:197`) |
| `stale_after_ms` | After this long without a packet, instantaneous fields read as zero |
| `fields` | Field key → expression, or `{ value, only_when }` |

Expressions read bytes from the payload (positions count from its start, after the company id) and
combine them:

```text
u8[3]  i8[3]  u16le[4]  u16be[4]  i16le[4]  i16be[4]  u24le[4]  u32le[4]
12   0x7fff   1609.344
* /   then   + -   then   << >>   then   &   then   ^   then   |
== != < <= > >=   then   &&   then   ||   then   condition ? a : b
!x   -x   ( … )
```

Bit operators bind tighter than comparisons, so `u8[2] & 0x80 == 0` means what it reads like.
There are no variables, loops or functions. Expressions are parsed when the registry is compiled;
the core receives a small tree (at most 16 levels and 64 nodes) and only evaluates it. A read
outside the packet or a division by zero gives no value for that field.

The compiled form is the **bundle** (`registry.bundle.json`): all fields and tables in one JSON
file, which is what the core loads.

#### How a table is proven

*As built.* Evidence lives next to the table and `ofp check` replays all of it:

- **A capture** (`<table>.capture.jsonl`, see `capture-format.md`): everything the device sent
  during a real session, plus what the rider typed. A **note** with values (`power=150`) is what the
  device's own display showed at that moment; the decoded value must be within the field's
  tolerance of it. A **confirmation** is the rider's answer in `ofp live` to "does the display show
  the same?": a confirmed value must decode to exactly the same number from then on, and a value
  marked wrong fails the check until the table decodes what the display showed.
- **Vectors** (`<table>.vectors.yaml`): single packets with known answers, for rules the capture
  does not exercise (another unit, an older firmware, a packet that must be refused).

A table whose fields its author has confirmed is at the "Submitted" level below.

### 4. Output

One GATT-style profile: the source describes its fields once (id, unit, type, name, sample kind,
tier, source identity), then sends compact values. An app can show and record a field it has never
heard of.

- Events are sent immediately; sampled values may be batched.
- When OFP Core runs inside the app, the app receives the same messages directly, with no radio in
  between. When OFP Connect is used, the same messages travel over BLE or the network. The app-side
  code is the same either way.
- Where a Bluetooth standard already exists for a value, a Connect can also offer it in that
  standard form, so apps that know nothing about OFP still work.

### Inside an app: what the app wires up

*Decided 2026-11-20. Built for broadcast devices: all four hooks, in the core and in
`packages/ofp-js`. The list of services is accepted but not acted on yet (nothing to hold back
until the core reads connectable devices).*
The promise to an app developer: add the library and about twenty lines, and any device with a
table works in the app the day the table is merged. Because OFP has no radio, four things connect
it to the app:

1. **Initialize with what the app already reads**: the list of Bluetooth services (or single
   characteristics) the app handles itself. See "The app says what it already reads".
2. **The full scan in**: every advertisement the app's scanner hears, before the app's own
   filtering. Later, for devices that are connected to, also the notifications of the
   characteristics OFP asks for.
3. **Results out**: devices OFP recognises are announced so they land in the app's existing device
   list; their readings come back as events.
4. **The rider's choice in**: when the rider picks an OFP device, the app tells OFP "use this
   source" (and "stop using it"), instead of making a Bluetooth connection.

The bookkeeping around those hooks is in the library, not in each app: which sources are in use,
which are remembered, repeating the standing values while the radio has a gap, declaring a source
silent and later lost. *As built:* in the core, behind the host interface below, and wrapped for
JavaScript apps in `packages/ofp-js`. Saving the remembered ids between launches stays with the app.

**Readings come out in two forms.** Values that fit what every training app has (power, cadence,
heart rate, speed) can be delivered in the shape the app already uses. Everything else is a
**generic field event**: the source describes each field once (key, name, unit, type, range, kind),
then sends values. An app that shows and records that one event handles every future field; an app
that ignores it loses nothing.

**One requirement cannot be hidden**: a broadcast-only device's data is the scan, so the app keeps
scanning, with repeats, while such a device is in use.

### Inside an app: a virtual Bluetooth device

*Decided 2026-11-20, not built.* The heavier route to the same end, for an app that wants no
hooks at all: OFP wraps the Bluetooth library the app uses, so the four hooks above happen inside
the wrapper.

- To the app, each OFP source looks like one more Bluetooth device, reached through the Bluetooth
  library the app already uses: it shows up in a scan, can be connected and subscribed to.
- That virtual device offers the **standard services** for what it has (FTMS Indoor Bike Data,
  Cycling Power, Heart Rate, byte for byte as the standards define them), so the parsers an app
  already has work unchanged. A broadcast-only bike simply appears as an FTMS bike.
- It also offers **one OFP service**: a description (read) and values (notify) for everything the
  standards have no place for, such as a gear. An app that wants those reads one new service.
- It is delivered as a thin wrapper around each common Bluetooth library. Real devices pass through
  untouched.
- An OFP Connect sends the same services over the air, so the app's code is the same either way.

#### A device that has a standard service and something of its own

*Decided 2026-11-20.* Many sensors offer a standard service for the usual value and a service of
their own for more (a heart-rate strap that also sends ECG, a trainer with an extension). OFP does
not merge into the device the app already shows. Its part appears as a **second device named
"<device> Extras"** (for example "Polar H10 Extras"):

- The app keeps reading the standard service exactly as it does today. Nothing about that changes.
- The Extras device carries only what the table decodes from the device's own service: normally
  the fields the standards have no place for.
- A device OFP is the only way to read keeps its plain name (a broadcast-only bike is just that
  bike).
- Both are the same physical device, so the host uses one connection for both.
- If a table does decode a value the standard service also carries, the rider chooses the source
  in the app, the way they choose between the ANT+ and the Bluetooth version of a sensor today.

#### The app says what it already reads

*Planned 2026-11-20, not built.* When an app starts OFP it passes the list of Bluetooth services
(or single characteristics) it reads by itself, for example Heart Rate, Cycling Power and FTMS.
OFP then offers only what is **not** on that list:

- a device's own service that a table decodes → "<device> Extras";
- a standard service the app has no code for (say Running Speed and Cadence, or treadmill data) →
  also offered through OFP, read by OFP's standard-profile readers, with no table needed;
- everything on the list → left to the app, and OFP stays silent about it.

So the same device can be all-direct in one app and partly OFP in another, and nobody maintains a
list of "OFP devices": it follows from what the app said and what the device has. An app that
passes an empty list uses OFP as its only way of reading sensors.

Still open: the OFP service's UUIDs and byte layout, which standard service a source offers when
several would fit, and what OFP assumes when an app passes no list at all. Until this exists, a host uses the JSON messages of the host interface below.

## Where OFP runs

- **OFP Core** contains **no Bluetooth**: the app passes in what its own Bluetooth layer hears and
  performs the writes Core asks for. This is how an app adds OFP without a second Bluetooth stack
  fighting its own. Ready-made Bluetooth glue for common platforms can be contributed to this repo
  for apps that have none.
- **OFP Connect** (later) is Core with its own radio and its own outputs.

The small interface between Core and its host (advertisement seen, connect, subscribe, read, write,
disconnected) is part of the standard and is frozen like stable fields. It does not assume a single
radio. Broadcast-only devices put one real requirement on the host: continuous scanning with
duplicates during the session.

#### The host interface

*As built, for broadcast devices.* The core is a Rust library (`crates/ofp-core`, `no_std`). Hosts
that are not written in Rust use five plain C functions (`crates/ofp-ffi`), the same for
WebAssembly and native builds; requests and replies are JSON:

| Request | When |
|---|---|
| `{"op":"advertisement","device_id":…,"name":…,"rssi":…,"manufacturer_data":"<hex, company id first>","now_ms":…}` | For every advertisement the host hears, from any device |
| `{"op":"tick","now_ms":…}` | About once a second |
| `{"op":"configure", …}` | Once, at the start. `handled_services`: what the app reads itself. `remembered`: sources the rider used before. `min_stale_ms`: wait at least this long before a source counts as silent (for a radio that misses many advertisements). `repeat_ms`: send a source in use's standing values again when nothing went out for this long. `lost_after_ms`: give up a silent source in use after this long |
| `{"op":"use_source","source_id":…,"now_ms":…}` | The rider picked this source. It is also remembered |
| `{"op":"release_source","source_id":…}` | The rider let it go (and it is forgotten) |
| `{"op":"sources","now_ms":…}` | The sources in range and in use, for a device list that just opened |

| Message in the reply | Meaning |
|---|---|
| `source_appeared` | Sent once per source, before its first values: source id, name, and the description of every field it sends (id, name, unit, type, range, kind, tier) |
| `values` | `source_id`, time, field key → value. Values outside a field's plausible range are left out and listed in `out_of_range`. `stale: true` marks the zeros sent when a source goes silent; `repeat: true` marks standing values sent again |
| `source_stale` | No packet for the stale time. Values resume later without a new `source_appeared` |
| `source_in_use` | The source is now in use; `auto: true` when the core did it because the source was remembered and was heard |
| `source_lost` | A source in use stayed silent for `lost_after_ms`. If it is remembered it is used again by itself when it is heard |

Connecting, subscribing and writing are not in this version; they arrive with the notify readers.

**What a host's radio really hears** (measured 2026-11-20, a Keiser M3i two metres from a Mac, 24
minutes of riding, signal around -58 dBm): the computer's built-in radio delivered about 30 of the
bike's advertisements per minute, roughly a third of what the bike sends, with gaps of 5–10 s about
twice a minute and one of 40 s. Other broadcasting devices in the room were heard at a similar share.
The operating system shares that radio with the keyboard, the mouse and WiFi and does not let an app
change how much of the time it listens. So:

- a table's `stale_after_ms` describes the device; a host sets `min_stale_ms` for its radio (the
  desktop app uses 10 s) and repeats the standing values in between;
- a dedicated radio (a USB adapter, an OFP Connect stick, a phone in the foreground) is the way to
  hear every packet. *The phone numbers are not measured yet.*

Core is written in **Rust** and compiled for desktop, iOS, Android and microcontrollers, so a table
behaves the same everywhere. The automated checks apply a microcontroller's memory and time limits
from the start, so everything written before the stick exists will run on it.

### Phones

- **iPhone: readers and tables only.** An iPhone app does not load code from a URL (app-store rules
  on downloaded code). A device that needs code reaches an iPhone through an OFP Connect, which
  runs the plugin and sends ordinary fields. A new reader reaches iPhone with an app update; new
  tables arrive live, because they are data. Several apps can share one sensor connection, so two
  apps that each embed Core do not fight over a device.
- **Android:** Core inside the app, or a standalone Connect app running as a foreground service
  that other apps reach over loopback.

## What a consuming app does

"Support it once" only holds if the app does something useful with a field it has never seen:

- **Show it** — a generic tile built from the description.
- **Record it** — as a stream next to power and heart rate.
- **Export it** — as a FIT developer field, so it survives into other tools.
- **Choose between sources** when two send the same field.
- **Keep experimental and unreviewed fields apart** from stable ones.

## Rules of the official registry

- **Tables only.** The registry hosts tables, never code. A table cannot perform a handshake or
  decrypt anything.
- **No unlocks.** A table may decode what a device sends openly. Nothing in the registry may defeat
  authentication, encryption or pairing that the maker put there to restrict access. Edge cases
  (a fixed "start sending" command) are ruled on one by one and the rulings are kept in the repo.
- **Own work only.** A table comes from the contributor's own captures or from the maker's public
  documentation. What a device's bytes mean may be learned from anywhere, including other
  projects' source code, but nothing is copied from projects under incompatible licenses (for
  example GPL): no code, comments, names or structure. Each table notes where its knowledge came
  from and which firmware it was verified on.
  When the knowledge came from reading another project's source, the work is split in two
  (`AGENTS.md`, "the clean-room rule"): a facts-only spec written first, and the table written from
  that spec in a separate session that never saw the other code. The spec ships with the table.
- **Verification levels.**

  | Level | What it takes |
  |---|---|
  | Submitted | The author's capture, replayed by the automated checks |
  | Confirmed | A second person with the same device adds a capture or signs off |
  | Field-proven | Apps report that real riders connected and got sensible data |
  | Maintained | The above, plus a named person who answers when a firmware update breaks it |

  The core checks every value against the field's plausible range. Apps may report anonymous,
  opt-in counts (table, version, device firmware, share of packets decoded, values in range — no
  rider, no values) by submitting periodic summaries to this repo. A table that starts failing
  drops a level and its author is told. A device with a single owner can stay at "submitted" with a
  mild note.

## Your own plugins, by URL (later)

People can do what they want on their own install.

- A rider gives OFP the URL of a plugin. OFP shows what it claims (name, author, device, fields,
  whether it sends commands), warns that it is **not reviewed**, and the rider confirms.
- A URL plugin may be **code**. It runs in a WebAssembly sandbox whose entire outside world is:
  receive a packet from its device, write to its device, set a timer, emit a field, write a log
  line. No files, no network, nothing about the rider or the app. Memory and time per call are
  capped. *Unproven on microcontrollers.*
- The rider attaches it to a device explicitly; it is pinned to the exact content they approved, so
  a changed file at the same URL asks again; its fields stay labelled unreviewed. Sending commands
  to the device is a declared permission, off by default.
- **Where it works:** in desktop and Android apps, and on any OFP Connect. Not inside iPhone apps.
- The project does not host or list such plugins. What people load is their own choice and
  responsibility.

This is also the development loop: write a table, host it anywhere, load it by URL, ride with it,
then submit the same file as a pull request.

URL plugins are the lab. When several of them need the same thing (for example values split across
packets), that pattern can be proposed as a new reader. Once it is in the core, those devices
become tables, can enter the official registry, and start working on iPhone.

## Later, and optional

None of these changes the four core parts.

- **OFP Connect** — the ESP32 stick (flashed from a web page), the desktop app, the Android app.
- **The remote** — a small frozen set of requests (info, scan, connect / forget, install, status)
  in the same profile as the data, so a Connect with no screen is set up from inside whichever app
  the rider uses, including handing it the WiFi name and password. The Connect owns the sensor
  connections, stores its own configuration, and lets one app configure at a time.
- **Transports** — BLE; GATT tunnelled over TCP with mDNS discovery (documented here, compatible
  with the "direct connect" model trainers use); loopback on the same computer; WebSocket, which
  also lets browser apps and simple scripts connect.
- **Many devices, many apps** — over the network one connection carries many devices, each a
  separate source, to several apps at once. The stream says when a source appears and disappears,
  and an app can subscribe to only the sources it wants. Source identity comes from the device, not
  from the Connect that heard it, so two Connects hearing the same device do not create two
  sources. Broadcast devices have no connection limit, so one Connect can hear a whole room of
  bikes.
- **More radios and more outputs** — a Connect may use several Bluetooth adapters (more sensor
  connections, or one radio that only listens and one that only serves apps), and may offer several
  outputs: OFP, or separate standard devices such as a bike and a heart-rate strap, each with its
  own address on chips that support that.
- **Local push** — anything that is not BLE (a script, a camera tool, a DIY board) sends fields in
  over loopback or WebSocket.
- **Cloud relay** — the same messages through a server. The protocol can describe it so anyone can
  run one; the project does not host one.
- **Inputs** — buttons, shifters and steering are event fields. OFP mirrors OpenBikeControl's
  inputs and can speak it outward instead of defining a competing vocabulary.
- **Control** — telling a trainer what to do is out of scope for now. Reserved at almost no cost: a
  version byte in the description, a read/write flag on every field, a reserved control
  characteristic. Until it exists, an OFP-only app can read a trainer but not control it.

## First version

Small enough to hold in your head:

- OFP Core (Rust, no Bluetooth),
- the broadcast reader,
- one table: the Keiser M3i, whose broadcast needs conditions, a bit mask, arithmetic, a bike-number
  picker and a staleness time,
- about ten fields,
- the output format,
- running inside one desktop app, checked against packets with known answers and against an
  existing hardware bridge for the same bike.

Next: a second, unrelated broadcast device (to prove the reader is generic), then a device that
connects and notifies, then the standard-profile readers.

The repo becomes public when the first version runs end to end, a newcomer can add a field and a
table by following the README, and the license, governance and contributing documents exist.

## How the repo governs

- The rules live here and are enforced by automated checks: format, append-only, capture replay,
  resource limits.
- Stable changes use lazy consensus: open for a fixed number of days, approved by maintainers,
  merged if nobody objects.
- Created and maintained by TrainerDay, open to everyone. Maintainers are added from people whose
  tables, fields and readers have been merged.
- **Handover is the intent.** TrainerDay does not want to own OFP forever. Once it has contributors
  and other apps using it, stewardship, including the name, is meant to pass to the community or to
  a neutral body. The license and the sign-off rule already make sure nobody, TrainerDay included,
  can take it closed.
- Anything calling itself OFP-compatible passes the conformance tests in this repo.
- **AI-assisted contributions are welcome.** Say so in the pull request; a human is responsible for
  it; the capture replay judges it like any other.

### License and money

- **Apache-2.0 for everything** in this repo — code, tables and documents. Any app, open or closed,
  can embed OFP, and contributions carry a patent grant.
- **Sign-off, not a contributor agreement.** Contributors add a "Signed-off-by" line (Developer
  Certificate of Origin).
- **No fees, ever.** No paid tiers, no certification fees. Nobody makes money from the protocol.
- **The name is protected** only so that "OFP compatible" means "passes the conformance tests".

## Related work

Projects this design learns from, and how it relates to them:

- **Heart Rate, Cycling Power, Speed and Cadence, Running Speed, FTMS** — the Bluetooth standards
  OFP reads and imports as fields.
- **QZ (qdomyos-zwift)** — the project that showed how much riders want their equipment to work
  everywhere. It bridges many machines to standard trainers, including control, which OFP does not
  do.
- **OpenBikeControl** — an open protocol for buttons and other inputs, which grew out of a working
  app first.
- **Buttplug.io / Intiface** — a Rust device library with a device configuration file, a
  user-supplied one, and a standalone app serving other apps.
- **Theengs Decoder** — data-file decoders for broadcast-only BLE devices, tested with captured
  advertisements.
- **BTHome** — an open broadcast format that devices emit natively.
- **OpenTelemetry semantic conventions** — a registry with stability tracked per field.
- **Zigbee2MQTT external converters** — user-loaded device support next to a reviewed set.
- **Kaitai Struct, CAN DBC files** — conditions, bit fields and scale factors written as data.
- **FIT developer fields, USB HID descriptors, GoldenCheetah XDATA** — self-describing and
  extensible data series.

Lessons taken from them: ship a working thing first and let the protocol follow; test every device
definition against captured traffic; keep the data format constrained and give code a separate,
deliberate escape hatch; write down change rules and patent terms early.

## Open questions

- The over-the-air form of the output (the description and value layout; today the messages exist
  only as JSON inside the host).
- How long descriptions are split over BLE.
- How official tables are signed and delivered to apps (a signed index on static hosting is the
  working idea), and offline behaviour.
- How long "no objection" lasts and how many approvals a stable change needs.
- WebAssembly on a microcontroller next to two BLE roles on one radio, and which runtime.
- Which microcontroller is the reference for the stick.
- Whether offering a URL loader carries legal exposure when people load unlock plugins (a question
  for a lawyer).
- Whether a heavier specification license is needed later, if a hardware maker asks for one.
- To confirm at the source: the current state of the ANT+ and Bluetooth standards processes for new
  sensor types.
