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
