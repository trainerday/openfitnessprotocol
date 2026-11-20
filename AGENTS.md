# Adding to the OFP registry — rules for people and AI assistants

The registry (`registry/`) is the official collection: **fields** (`registry/fields/*.yaml`) and
**tables** (`registry/tables/*.yaml`, each with its evidence beside it). Everything enters by pull
request, and the automated check (`ofp check`, also run by CI) is the judge. AI-assisted work is
welcome: say so in the pull request; a person stands behind it.

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
