# The capture file

A capture is what a device really sent during a session, with what the rider wrote down. Every
table in the registry ships with one, and the automated checks replay it.

- One JSON object per line. File name: `<anything>.capture.jsonl`.
- Text on purpose: it shows up readably in a pull request, and a person or an AI assistant can read
  it.
- `t` is milliseconds since the recording started.
- A capture holds **one device**. Nothing about other devices nearby is recorded.

Make one with `ofp record "<device name>"` (raw, needs no table) or `ofp live` (when a table already
knows the device). Read one with `ofp inspect <file>` and `ofp replay <file>`.

## Lines

### `header` (always the first line)

```json
{"type":"header","format":"ofp-capture","version":1,"tool":"ofp 0.1.0","host":"macos",
 "started":"2026-11-20T10:50:28Z","device":{"name":"M3","company_id":"0102"}}
```

| Key | Meaning |
|---|---|
| `format`, `version` | `ofp-capture`, `1` |
| `tool`, `host` | What recorded it, and on which operating system |
| `started` | UTC time the recording began |
| `device.name` | The advertised name |
| `device.company_id` | Bluetooth company id of the manufacturer data, four hex digits |
| `device.services` | Advertised service UUIDs |
| `gatt` | For a connected device: the services and characteristics found, with their properties |

### `packet`

An advertisement:

```json
{"type":"packet","t":24188,"kind":"adv","name":"M3","rssi":-67,"mfr":"0201064000c5…"}
```

A notification, or a value read once after connecting:

```json
{"type":"packet","t":5120,"kind":"notify","chr":"2a37","data":"1648"}
```

| Key | Meaning |
|---|---|
| `kind` | `adv`, `notify` or `read` |
| `mfr` | Advertisement: manufacturer data in hex, the 2-byte company id first |
| `service_data` | Advertisement: service UUID → hex |
| `chr`, `data` | Notify / read: the characteristic UUID and the bytes in hex |
| `name`, `rssi` | As heard |

Standard 16-bit UUIDs are written short (`2a37`), others in full.

### `note`

What the rider typed while recording.

```json
{"type":"note","t":61000,"text":"power=150 cadence=82 steady","values":{"power":150,"cadence":82}}
```

`values` holds every `key=number` in the text. When the key is a registry field, it is what the
device's **own display** showed at that moment, and `ofp check` requires the table to decode a value
within the field's tolerance around that time (people read a display and then type, so the check
looks from 8 s before to 2 s after). Other keys are free information.

### `confirm`

The rider's answer in `ofp live` to "does the device's display show the same?".

```json
{"type":"confirm","t":90000,"source_id":"keiser-m3i:197","field":"gear","shown":11,"ok":true}
{"type":"confirm","t":95000,"source_id":"keiser-m3i:197","field":"power","shown":148,"console":160,"ok":false}
```

- `ok: true` — the table must decode exactly `shown` at that moment, now and after any later change
  to the table.
- `ok: false` — the check fails until the table decodes a value within tolerance of `console`.

## Privacy

A capture can contain a heart rate, a bike number and the device's advertised name. It contains no
account, no location and no radio address. Look at a capture before you publish it.
