# OFP Studio

A page on your own computer for adding a device and for testing one. Start it with:

```bash
ofp studio                 # opens http://127.0.0.1:4747
ofp studio --capture registry/tables/keiser-m3i.capture.jsonl   # start from a recording
ofp studio --no-open --port 4800
```

It listens on `127.0.0.1` only: the page can write into the registry folder, so it is not for a
network. It needs the registry folder (`--registry`, default `registry`) and keeps recordings in
`captures/` (git-ignored) until you save.

## The steps

| Step | What you do |
|---|---|
| **1 Devices** | Everything the radio hears, with how often. A badge on devices the registry already reads. Choose one, or open a capture file |
| **2 Record** | Record what the device sends (tick "Connect to it" for a device that sends nothing in its adverts). Watch the bytes; type what the device's own display shows (`power=150 cadence=82`) |
| **3 Map** | Click bytes to see their values over the recording, or **Find it for me**: every position, width and scale is tried against your notes. Choose the field (or draft a new one) and add it to the table |
| **4 Live** | What the device sends right now, grouped by what reads it (an OFP table, or a standard profile such as FTMS or Heart Rate). For each value: **Yes, this is right** or **No, this is wrong**, with one optional line for what you saw. No exact numbers are asked for: they move. Saved in the recording with the time. **Download the recording** gives one file to send to whoever is helping |
| **5 Save** | The table as text, the same check CI runs, then Save: the table and the capture go into `registry/tables/`. Nothing is committed or pushed; the page shows the git commands |
| **Tester** | Connect the way an app would. Standard profiles (FTMS, Cycling Power, Speed and Cadence, Heart Rate) are read out side by side by source; findings in plain words; FTMS trainer control; a log; a report to download |

## Testing a device for someone else

The person with the device and the person fixing the table do not have to be together:

1. The owner starts the Studio (`npm start`), picks the device, opens **Live**, presses Start, uses
   the device normally and answers right or wrong for each value.
2. They press **Download the recording** and send that one file.
3. The helper opens it in their own Studio (Devices → "A file someone sent you"). It holds everything
   the device sent, and for each "wrong": what was shown, what the owner saw, and when.

A "wrong" stays open in the checks until the same value is later marked right, or the table decodes
what the owner saw.

Before you add: a banner tells you when the registry already decodes the device (**extend that
table**), and a new field is refused once when a similar one exists (**reuse fields**).

Devices that must be connected to can be recorded, explored and tested today. Saving a *table* for
them needs a reader the core does not have yet.

## A person and an assistant on the same session

The session (device, recording, notes, the table being written) lives in the Studio, not in the
page, and every change is sent to the page as an event. So an AI assistant can work on the same
session over the API while you watch: what it finds or changes appears in your page as it happens.
"Work with an assistant" in the page gives the text to paste. The person stays the one who reads
the device (notes, confirmations) and presses Save.

## The API

JSON over HTTP on the same address. `POST` bodies are JSON; errors come back as
`{"error": "…"}` with status 400.

| Request | What it does |
|---|---|
| `GET /api/guide` | The rules for adding to the registry (`AGENTS.md`) |
| `GET /api/state` | Registry (fields with unit, range, tolerance, description; tables), radio status, the session |
| `GET /api/devices` | Devices in range |
| `GET /api/captures` | Capture files that can be opened |
| `POST /api/session` | `{"device_id": …}` or `{"capture": "<path>"}`; `{}` closes the session |
| `POST /api/record/start` | `{"connect": true}` to connect and record notifications; `{}` for adverts |
| `POST /api/record/stop` | |
| `POST /api/note` | `{"text": "power=150 cadence=82"}` (while recording) |
| `GET /api/capture` | `packets` as `[t, stream, hex]` (advertisements: after the company id), `notes`, `confirms`, `io` (reads and writes), `header` |
| `GET /api/similar` | Tables that already decode this capture, and the advice to extend or add |
| `POST /api/series` | `{"expr": "u16le[4] / 10", "stream": "adv"}` → that reading over the capture |
| `POST /api/solve` | Ranked readings for every noted value |
| `POST /api/table/design` | The table from parts: `name`, `id`, `mappings: [{field, expr, only_when}]`, `only_when`, `instance_expr`, `instance_label`, `stale_after_ms`, `provenance`, `verified_on`. Becomes the shared draft |
| `POST /api/table/propose` | The same, without changing the draft |
| `POST /api/table/check` | `{"yaml": …}` → compiles, replays the capture, compares with notes and confirmations: `ok`, `problems`, `series` per field |
| `POST /api/field` | Draft a new experimental field. Answers `similar` instead when one like it exists; `confirm_new: true` adds it anyway |
| `POST /api/confirm` | `{"field": "power"}` (right), `{"field": "power", "wrong": true}` or `{"field": "power", "wrong": true, "console": 70}` (wrong, and what the device showed); while recording |
| `GET /api/capture/file` | The recording as one file to send |
| `POST /api/capture/upload` | `{"name": …, "text": …}` → open a recording someone sent |
| `POST /api/save` | `{"yaml": …, "overwrite": false}` → writes table and capture, runs the full check, undoes itself if the registry no longer passes |
| `POST /api/write` | `{"chr": "2ad9", "hex": "00"}` → write to a characteristic of the connected device (the Tester's trainer commands) |
| `GET /api/events` | Server-sent events: `devices`, `packet`, `tx`, `note`, `confirm`, `source`, `values`, `recording`, `session`, `draft`, `checked`, `radio`, `connected` |

## How it is built

`crates/ofp-studio`: `studio.rs` is everything the Studio does, without HTTP or a radio (the tests
drive it directly); `server.rs` is the HTTP layer and the radio task; `solve.rs` is "find it for
me"; `web/` is the page (plain HTML, CSS and JavaScript, compiled into the binary). Decoding always
goes through `ofp-core`, and tables through the same compiler and checker as `ofp check`.
