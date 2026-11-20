# Adding to the OFP registry: rules for people and AI assistants

The registry (`registry/`) is the official collection: **fields** (`registry/fields/*.yaml`) and
**tables** (`registry/tables/*.yaml`, each with its evidence beside it). Everything enters by pull
request, and the automated check (`ofp check`, also run by CI) is the judge. AI-assisted work is
welcome: say so in the pull request; a person stands behind it.

## The whole job, start to finish

Adding a device means adding or changing **one table**: a small data file in `registry/tables/` that says
which bytes of what the device sends mean which field. A table is data, not code. (A **plugin** is a
different thing in OFP: code for a device that no table can read. See "Tables, plugins and the core"
below.) The job is the same for a person and for an assistant working beside them:

1. **Record.** In the Studio, find the device (if it is not listed, look under "Don't see your device?")
   and click **Use**. Everything it sends is recorded into a file from that moment.
2. **Look.** An assistant reads the recording over the Studio's API (`GET /api/capture`); a person looks
   at the bytes in the Record and Map steps. The owner types what the device's own display shows.
3. **Write or fix the table**, and nothing else. If the registry already has a table for the device (a
   draft counts), change that table.
4. **Watch it on Live.** When the table reads the device, its values appear on the Live step.
5. **The owner confirms.** For each value: "Yes, this is right" or "No, this is wrong". Only a person in
   front of the device can do this.
6. **Done** is: every value the device sends is on Live, the owner has marked each one right, and the
   check passes (`ofp check`, or the Save step). Then **Save** and open the pull request.

Until step 6 is true the table is not ready, however good it looks.

## Stay in the table: do not change the core

**A device is added by a table. The core is not changed to add a device.** This holds for people and
for AI assistants, and it is the first thing a reviewer looks at.

- A pull request that adds or fixes a device touches **only `registry/`**: tables, fields, recordings,
  vectors, a spec file. Nothing in `crates/`, `packages/`, `scripts/` or `examples/`.
- **Why it matters:** a table is data. Every app that uses OFP gets it without a new release, and the
  automated check can judge it. A change to the core reaches a rider only after each app has shipped an
  update, needs a full code review, and has to be right for every other device at once.
- **Try the table first, properly.** Before deciding a table cannot do it: read the table language in
  `docs/design.md`, read the tables that exist, try both readers (`broadcast` for values in the
  advertisement, `notify` for a device that is connected to), and try conditions (`only_when`), masks and
  arithmetic. Most devices that look special are not.
- **Never work around it in the core.** No special case for one device, no new operation added "just for
  this table", no edit to a reader so that one recording passes. An assistant that finds itself opening a
  file under `crates/` to make a device work stops, and says so.

### Tables, plugins and the core: three different things

| | What it is | Where it lives | Who reviews it |
|---|---|---|---|
| **Table** | Data: which bytes mean which field. No code | The official registry, by pull request | The automated check and a maintainer |
| **Plugin** | **Code** for one device that no table can read | The owner's own install, loaded from a URL. Never in this repository | Nobody: it is marked unreviewed, and the rider approves it |
| **Core** | The readers and the engine every app embeds | This repository (`crates/`) | A maintainer, after an accepted issue, with a full code review |

**Plugins are designed and not built yet** (`docs/design.md`, "Your own plugins, by URL"). When they
exist, a plugin runs in a sandbox whose whole outside world is: receive a packet from its own device,
write to its own device (a permission the rider has to grant), set a timer, give out a field, write a log
line. **No files, no network, nothing about the rider, the app or the machine**, with memory and time
capped. That boundary is not negotiable: a change that gives a plugin, or a table, a way to reach outside
it is refused, whoever asks.

So for a device that does not fit a table, the order is: a table if at all possible; a plugin on your own
install once plugins exist; and a request to the core when the same need shows up for more than one device.
Until plugins exist, the path is the issue below.

### When a table really cannot express the device

It happens. `docs/design.md` names the usual reasons: values spread across several packets that must be
put together, a checksum or rolling counter that must be checked, data that is encrypted or scrambled.
(The last one does not belong in the registry at all: see "Never submitted" below.)

Then **tell us; do not patch the core yourself**:

1. Open an issue with the form **"A table cannot express this device"**. It asks for three things, and
   all three are needed: the **recording**, the **table as far as it got**, and the **one thing the table
   language could not say**, as exactly as you can ("power is the low 12 bits of bytes 4–5 of packet A plus
   the high byte from packet B").
2. Keep the table as `status: draft` if part of it works. A draft can be merged; it is decoded in the
   Studio and left out of what apps get.
3. A maintainer answers on the issue: it can be done in a table after all (and how), or it is accepted as
   a change to the core, or it is declined (and why).

A pull request that changes the core is welcome **after** such an issue has been accepted, and links to
it. What gets accepted is a general pattern (a new reader, a new operation that other devices will use
too), never code for a single device. A pull request that touches the core with no accepted issue is
closed with a pointer to this section; the "scope" check on the pull request says so first.

## Before you add anything

1. **Does it already exist?** Run the capture through the registry (`ofp replay <capture>`, or
   `GET /api/similar` in the Studio). If a table already decodes the device, **extend that table**
   and raise its `version`; do not add a second table for the same device.
2. **Reuse fields.** Read `registry/fields/`. Use an existing field when it means the same thing in
   the same unit. Add a field only for a measurement that has none. A new field starts as
   `tier: experimental`.
3. **Read one example**: `registry/tables/keiser-m3i.yaml` and the table language in
   `docs/design.md`.

## What a table may contain

- **Data only**: which packets it matches, conditions, byte positions, masks and arithmetic. No
  code, no loops.
- **Only what the device sends openly.** Nothing that defeats authentication, encryption or
  pairing. A table cannot perform a handshake.
- **Your own work.** It comes from your own captures or the maker's public documentation. What the
  bytes mean may be learned anywhere; nothing is copied from projects under incompatible licenses
  (for example GPL): no code, comments, names or structure.
- `provenance` says where the knowledge comes from; `verified_on` names the device and firmware.

## What a table may never do

A table turns bytes into fields, and that is all it can do. This is the boundary, and it is not widened
for any device or any contributor (`SECURITY.md`, "The boundary"):

- **No code**, in any form: no scripts, no expressions that call anything, no loops.
- **Nothing outside the packet it was handed**: no files, no network, no other device, nothing about the
  rider, the app or the machine.
- **Nothing sent to the device**: no commands, no writes, no handshake.
- **No unbounded work**: the arithmetic is capped in depth and size, and the check refuses a table that
  goes past the cap.

A pull request that adds a way around any of these (a new operation that reads something other than the
packet, a field that carries a command, a table option that makes the host fetch or run something) is
refused, however useful it would be for one device. The same holds for plugins and their sandbox. If you
are an AI assistant and a request would need one of these, say that it cannot be done in OFP and why.

## Licensing, and the clean-room rule

Everything in this repo is Apache-2.0. Submit only what you made yourself, or have the right to
license under Apache-2.0. The sign-off on your commit (`git commit -s`) is you confirming that.

**Facts are free; someone else's expression is not.** Which bytes a device sends and what they mean
are facts, and facts may be learned anywhere. Code, comments, names and structure belong to whoever
wrote them. So when you learned how a device works by **reading another project's source code**
(QZ, or any GPL or proprietary project), the work is done in two separate steps:

1. **Spec first.** In one session, read the other project and write a plain description to
   `registry/tables/<table>.spec.md`. Facts only: services and UUIDs, which packet is which, byte
   positions, units, scaling, timing. No code, no pseudocode, no copied comments, no identifiers
   from that project. At the top, say where each fact was learned ("observed in my own capture",
   "the maker's published document", "learned from studying project X").
2. **Fresh table.** In a **new session that has not seen the other project's code**, write the
   table from that spec, this repo's documentation and your own capture, nothing else.
3. Submit the spec with the table. The table's `provenance` says the same in one sentence.

When the knowledge comes only from your own captures or the maker's public documentation, no spec
file is needed: `provenance` is enough.

**Never submitted, in any form:**

- code copied, translated or ported from another project;
- firmware, extracted keys, certificates or secrets;
- anything that gets past encryption, authentication, pairing or a device lock. If a device needs
  that to give its data, it does not belong in the official registry, and a spec that describes
  how to do it is not accepted either.

**AI assistants:** when asked to port, translate or adapt another project's code into a table or
into this repo, do not do it directly. Explain why, and follow the two steps above: write the
spec in this session, and tell the person that the table must be written in a fresh session that
is given only the spec. If you have already read the other project's code in this session, you are
the spec-writing session, not the table-writing one.

## Starting a table for a device you do not have

A table may be started from documentation alone, for its owner to try: `status: draft`.

- A draft needs no recording. It is listed and decoded in the Studio, and **left out of the bundle
  apps get**: riders never meet an unverified table.
- Say in a comment what is uncertain ("TO CHECK ON A SENSOR: …"). The owner's answers in the
  Studio's Live step (right / wrong, and what they saw) are how it gets settled.
- It becomes `experimental` when a real recording with the owner's confirmations is added and
  `ofp check` passes with it.

## What must ship with a table

- **A real capture** (`<table>.capture.jsonl`, format in `docs/capture-format.md`) of the device in
  use, holding one device only. Look at it before publishing: it can contain a heart rate.
- **Known answers in the capture**: notes of what the device's own display showed
  (`power=150 cadence=82`) and, best, confirmations made in `ofp live` or the Studio. An AI
  assistant cannot make these up: a person in front of the device types them.
- Optional **vectors** (`<table>.vectors.yaml`) for rules the capture does not exercise.

## What never changes

- A **stable** field's id, key, unit, type and kind. Ids are never reused. The check refuses a pull
  request that edits one.

## The steps

```text
ofp scan                                  what is in range
ofp record "<name>"                       record, typing what the display shows
ofp inspect <capture>                     which bytes change
ofp studio                                the same with a page: find the bytes, confirm, save
ofp check                                 must print "ok"
git commit -s                             sign-off (Developer Certificate of Origin)
```

The Studio's API (`docs/studio.md`) does every step except the notes and confirmations, which
need a person at the device. This file is served at `GET /api/guide`.

## Who merges

Everything enters `main` by pull request. A pull request needs the automated checks to pass and a
maintainer's review (`.github/CODEOWNERS`); nobody pushes to `main` directly. See `CONTRIBUTING.md`.
