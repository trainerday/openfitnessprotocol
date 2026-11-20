# iConsole+ / Toorx family bikes: protocol facts

Facts only, for the clean-room rule in `AGENTS.md`. No table has been written from this page yet.

**Where these facts were learned:** from studying the open-source project QZ (qdomyos-zwift,
GPL-3), as read on 2026-11-20. No code, comments or identifiers from it are reproduced here. The
consoles are made in China and sold under many brands; no maker's specification is known to this
project. **Nothing here has been observed on a device by this project yet.**

## The family

Many exercise-bike consoles speak one protocol over Bluetooth LE: the app writes short command
frames that start with `F0`, the console answers with notifications, and it keeps sending data only
while the app keeps writing a "poll" frame. Every frame seen has the same shape:

- byte 0: `F0`
- byte 1: a command (`A0`, `A1`, `A2`, `A3`, `A4`, `A5`, `A6`, `AF` are used at start; `A2` is the poll)
- bytes 2–3: two bytes that differ by console and stay the same in every frame for that console
  (they look like a model or console address; their meaning is not clear from the code)
- then 0 to 11 payload bytes
- last byte: the sum of all the bytes before it, modulo 256.

All frames below are fixed: the checksums are already worked out and are part of the bytes. Nothing
is computed per session, nothing answers a challenge.

Consoles differ in the two address bytes, in the start sequence and in a few packet details. This
spec covers **two variants**, the default one and the "3.0" one. The others are listed at the end
as not covered.

## Recognition

Names are compared in upper case unless marked case-sensitive. QZ recognises the family by name
only; the variant is then picked by name, by which service the bike offers, and by QZ settings.

**Variant A (address `01 01`)** is what QZ uses, by default, for:

- names starting `CARE11216+`
- names starting `IBIKING+` (when the user has not set QZ to an Enerfit/Hammer bike)
- names containing `RI009R`
- with QZ's "Toorx bike" option on: names starting `TOORX` (case-sensitive), `I-CONSOLE+`,
  `ICONSOLE+`, or containing `CR011R`.

**Variant B (address `23 D3`)** is what QZ uses for:

- names that are `TC` followed by exactly three digits (whole name 5 characters, e.g. `TC123`)
- any Variant A name when the user has turned on QZ's setting for "Toorx 3.0" consoles. So some
  Toorx/iConsole consoles with the same names speak Variant B; which ones cannot be told from the
  name. A table author may want two tables for the same names and let the data decide (a console
  that is sent the wrong start sequence is expected to stay silent, but that is not confirmed).

One more rule applies before Variant A: when the bike's Bluetooth address starts with `E8`, QZ
uses a third variant (address `23 01`, not covered here) instead. Apps on Apple platforms cannot see
the address, so this rule cannot be applied there. See "Not covered".

## Services and characteristics

A bike offers one of two transports. The variant does not depend on the transport: the same
frames are written either way.

| Transport | Service | Notify (data) | Write (commands) |
|---|---|---|---|
| Microchip transparent UART | `49535343-FE7D-4AE5-8FA9-9FAFD205E455` | `49535343-1E4D-4BD9-BA61-23C647249616` | `49535343-8841-43F4-A8D4-ECBE34729BB3` |
| `FFF0` | `0000FFF0-0000-1000-8000-00805F9B34FB` | `0000FFF1-0000-1000-8000-00805F9B34FB` | `0000FFF2-0000-1000-8000-00805F9B34FB` |

- QZ looks for the Microchip service first and falls back to `FFF0` when it is missing (Variant A).
  For `TC` names it uses the Microchip service only.
- On the Microchip service QZ also subscribes to `49535343-4C8A-39B3-2F49-511CFF073B7E` when the
  bike has it. QZ treats every notification from the service the same way, so which of the two
  characteristics carries the data packet is not clear from the code.
- Subscribe with `01 00` to the client configuration descriptor. The start frames are written after
  the subscription is confirmed.
- Writes go **without response** when the write characteristic allows it, otherwise with response.

## What to write

### Variant A (address `01 01`)

Start, once, in this order:

| # | Frame (hex) |
|---|---|
| 1 | `F0 A0 01 01 92` |
| 2 | `F0 A1 01 01 93` |
| 3 | `F0 A3 01 01 01 96` |
| 4 | `F0 A5 01 01 02 99` |
| 5 | `F0 A6 01 01 06 9E` |

Timing: after each frame QZ waits for the next notification from the bike, or 300 ms if none comes.
On bikes with the extra characteristic `49535343-4C8A-…` and on bikes using the `FFF0` service,
QZ also waits a further 400 ms after each frame; on Microchip-service bikes without that
characteristic it does not. So a gap of about 400–700 ms between frames matches what QZ does on
every Variant A bike.

Poll, repeated: `F0 A2 01 01 94`.

### Variant B (address `23 D3`)

Start, once, in this order (no extra delay beyond the wait for a reply or 300 ms):

| # | Frame (hex) |
|---|---|
| 1 | `F0 A0 01 01 92` |
| 2 | `F0 A5 23 D3 04 8F` |
| 3 | `F0 A0 23 D3 86` |
| 4 | `F0 A1 23 D3 87` |
| 5 | `F0 A3 23 D3 01 15 01 02 51 01 51 45` (12 bytes) |
| 6 | `F0 A4 23 D3 01 01 01 01 01 01 01 01 01 01 01 95` (16 bytes) |
| 7 | `F0 AF 23 D3 02 97` |

Poll, repeated: `F0 A2 23 D3 88`.

### Poll timing (both variants)

QZ writes the poll every 200 ms, but after each poll it waits for the bike's reply (or 300 ms)
before going on, so in practice one poll goes out per reply, about every 200–500 ms. Polling starts
only after the last start frame. A fixed interval of 250–500 ms is expected to work; this is not
confirmed. QZ writes nothing else for reading data (other frames it writes only set resistance and
are not described here).

All start and poll frames are within OFP's limits (at most 7 start frames, one poll frame, at most
16 bytes each).

## The data packet

QZ uses a notification only when it is **exactly 21 bytes**; anything else (for example replies to
the start frames) is ignored. QZ does not check the first bytes. For treadmills of the same family,
QZ treats a reply whose byte 1 is `B2` as the data reply; for bikes this is not checked and not
confirmed.

**Number encoding.** The numbers are not binary. Each byte holds a value from 0 to 99 **plus 1**
(so `01` means 0 and `64` means 99), and a two-byte number is high part × 100 + low part, high byte
first. Below, "pair (h, l)" means value = (byte h − 1) × 100 + (byte l − 1).

| Bytes | Meaning |
|---|---|
| 0–3 | not decoded (probably a header; see above) |
| 4 | elapsed seconds, 0–59, as byte − 1 |
| 5 | elapsed minutes, as byte − 1. QZ reads this time but does not use it |
| 6–7 | speed, pair (6, 7), in km/h × 10 (divide by 10) |
| 8–9 | cadence, pair (8, 9), rpm |
| 10–11 | not decoded (QZ computes distance from speed instead; these are probably distance) |
| 12–13 | energy, pair (12, 13), kcal |
| 14–15 | heart rate, pair (14, 15), bpm, from the console's own pulse sensor; 0 means none |
| 16–17 | power, pair (16, 17), in watts × 10 (divide by 10) |
| 18 | resistance level, as byte − 1 (0 if the result is below 0). QZ allows levels 1 to 32 |
| 19–20 | not decoded |

Values that exist: speed, cadence, power, energy, resistance level, heart rate, elapsed time.
Power is sent by the console (QZ does not need to estimate it for these two variants).

## Validity

- QZ reports power as 0 whenever cadence is 0.
- Heart rate 0 means the console has no reading.
- Rate: one data packet per poll, as far as the code shows.

## Example response (made up from the layout above, not captured)

```
F0 B2 01 01 06 03 03 36 01 56 01 01 01 2B 02 21 10 01 09 01 00
```

Bytes 0–3 and 19–20 are placeholders (not decoded; the `F0 B2` start is only a guess from the
treadmill code). It decodes to:

| Bytes | Raw | Value |
|---|---|---|
| 4, 5 | `06`, `03` | 5 s, 2 min → 2:05 elapsed |
| 6–7 | `03 36` | (3−1)×100 + (54−1) = 253 → 25.3 km/h |
| 8–9 | `01 56` | (1−1)×100 + (86−1) = 85 rpm |
| 12–13 | `01 2B` | (1−1)×100 + (43−1) = 42 kcal |
| 14–15 | `02 21` | (2−1)×100 + (33−1) = 132 bpm |
| 16–17 | `10 01` | (16−1)×100 + (1−1) = 1500 → 150.0 W |
| 18 | `09` | 9 − 1 = level 8 |

## Not covered (each has its own start sequence in QZ)

- **Bikes whose Bluetooth address starts `E8`**, and QZ's Enerfit SPX 9500 / Hammer Speed Bike S
  options: address `23 01`, a different 6-frame start, poll `F0 A2 23 01 B6`. Hammer (`IBIKING+`
  with the option) also uses a different packet layout (19 bytes, no power).
- `FITHIWAY` / `FIT HI WAY`, and Hertz XR-770 (QZ option): a 12-frame start mixing addresses
  `01 01` and `02 01`; FitHiWay then polls with the Variant B poll, Hertz with the Variant A one.
- `TUN ` (Tunturi): address `03 01`, 20- or 21-byte packets, speed not sent.
- `BODYSCULPTURE`: address `0D 01`.
- `VIRTUFIT`, `VIFHTR…`: address `1E 01`.
- `REEBOK`: address `32 01`.
- `ADIDAS` + 4 or more digits: address `5B 01`.
- `BIKZU_`: address `54 31`.
- `FAL-SPORTS`: address `1B 01`, speed not sent.
- `DKN MOTION`: address `02 01`, and one start frame that is not in the `F0` format.
- `I-CONSOIE+` (sic, Casall): address `3B 01`.
- Hop-Sport HS-090H, Toorx SRX 500, Taurua IC90, Fytter RI08, Asviva (QZ options): their own
  sequences; Fytter and Asviva use a different packet layout.
- JLL IC400 (QZ option): **its poll frame changes on every write** (a counter and a running
  checksum), so it would be stopped under OFP's fixed-bytes rule anyway. It also sends no power.
- `PASYOU-`: QZ gives it the Variant A start and poll, but its packet rules ask for a length of 21
  and 19 at the same time, so QZ never decodes its data. What its packet looks like is not clear
  from the code.

## Uncertain

- Which of the two Microchip notify characteristics carries the data.
- Bytes 0–3, 10–11 and 19–20 of the data packet.
- Whether a console stays silent or misbehaves when sent the other variant's start sequence.
- Whether the gaps between start frames matter, and how fast the poll may safely go.
