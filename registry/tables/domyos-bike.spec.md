# Domyos bike without FTMS (Decathlon): protocol facts

Facts only, for the clean-room rule in `AGENTS.md`. No table has been written from this page yet.

**Where these facts were learned:** from studying the open-source project QZ (qdomyos-zwift,
GPL-3), as read on 2026-11-20. No code, comments or identifiers from it are reproduced here.
Decathlon has published no specification for this protocol that this project knows of.
**Nothing here has been observed on a device by this project yet.**

## The device

- A Decathlon Domyos exercise bike whose advertised name starts with `Domyos-Bike` (exact case).
- Only when the bike does **not** advertise the FTMS service (`1826`). A `Domyos-Bike` that
  advertises FTMS is an FTMS bike and is read by OFP's standard FTMS table.
- After connecting, the bike must have the Microchip transparent-UART service below. If it has
  FTMS instead, QZ switches to FTMS.
- QZ tells two console makers apart by the Bluetooth address (one family's addresses start with
  `57`), but it sends the same start sequence to both. A table cannot see the address on every
  platform and does not need to.
- It accepts a connection. Fixed frames are written to start the data and one fixed frame is
  repeated to keep it coming (below).

## Services and characteristics

| What | UUID |
|---|---|
| Service | `49535343-FE7D-4AE5-8FA9-9FAFD205E455` |
| Data (notify) | `49535343-1E4D-4BD9-BA61-23C647249616` |
| Write | `49535343-8841-43F4-A8D4-ECBE34729BB3` |

## Frame format (what is written)

Every frame starts with `F0`, then a command byte, then any payload, and ends with a checksum
byte: the low 8 bits of the sum of all bytes before it. A frame longer than 20 bytes is written as
two writes: the first 20 bytes, then the rest. All frames below are complete and fixed; their
checksums are already included.

## What to write

| | |
|---|---|
| Characteristic | the write characteristic `49535343-8841-…` |
| Write type | with response (QZ uses its Bluetooth library's default write, which is with response). Whether a write without response also works is not clear from the code |
| When | after notifications are switched on (`01 00` to the data characteristic's client configuration descriptor). QZ subscribes first, then starts writing |

### Start frames, in order

| # | Hex | Note |
|---|---|---|
| 1 | `F0 C8 01 B9` | |
| 2 | `F0 C9 B9` | |
| 3 | `F0 A3 93` | |
| 4 | `F0 A4 94` | |
| 5 | `F0 A5 95` | |
| 6 | `F0 AB 9B` | |
| 7 | `F0 C4 03 B7` | |
| 8 | `F0 AD FF FF FF FF FF FF FF FF FF FF FF FF FF FF FF FF 01 FF` | first 20 bytes of a 23-byte frame |
| 9 | `FF FF 8B` | its last 3 bytes |
| 10 | `F0 CB 02 00 08 FF FF FF FF FF FF FF FF FF FF FF FF FF 01 00` | first 20 bytes of a 27-byte frame (a console display frame) |
| 11 | `00 01 FF FF FF FF B6` | its last 7 bytes |
| 12 | `F0 AD FF FF 00 05 FF FF FF FF FF FF FF 00 00 FF FF FF 01 FF` | first 20 bytes of a 23-byte frame |
| 13 | `FF FF 94` | its last 3 bytes |

Two more frames are sent by QZ only later, when the ride is started (from QZ, or when the bike
reports its start button, see byte 22 below). Whether data flows without them is not clear from
the code; QZ receives data before sending them, so they look optional:

| # | Hex | Note |
|---|---|---|
| 14 | `F0 CB FF FF FF FF FF FF FF FF FF FF FF FF 01 00 14 01 FF FF` | first 20 bytes of a 27-byte frame |
| 15 | `FF FF FF FF FF FF BD` | its last 7 bytes |

Timing: the bike answers each start frame (each pair, for the split ones) with a notification.
QZ waits for that answer, up to 300 ms, before the next frame, and **writes the same frame again
if no answer came**. An OFP `start` list writes each frame once; a gap of about 300 ms between
frames matches QZ's wait. Whether the bike ever needs a frame repeated is not clear from the code.

Command `AD` (frames 8 and 12) is the same command QZ uses to set the resistance level (the level
goes at byte 10 of the 23-byte frame). In both start frames byte 10 is `FF`, so they set no level.
Reviewers should still check this against the rule that nothing is written to a trainer control.

### Poll frame

| | |
|---|---|
| Frame | `F0 AC 9C` (one frame) |
| Interval | 300 ms |

QZ also replaces about every fourth poll with a console display frame (command `CB`) that carries
elapsed time, speed, heart rate, cadence and energy, so its bytes are **computed** each time.
That frame is not taken here. Whether the bike keeps sending data with only the fixed poll is not
clear from the code; a recording should check it. QZ also skips the poll while it is waiting for
the second half of a split data packet.

## The data packet

The data packet is **26 bytes**. It arrives either in one notification of 26 bytes, or split
into a 20-byte notification followed by a 6-byte one. When split, the 20-byte piece starts with
`F0 BC`. QZ decodes any 26-byte packet without checking its header, so `F0 BC` as the header is
likely but not proven.

All the values below except byte 22 are within the first 20 bytes, so they can be read from the
20-byte piece alone when the packet is split.

Multi-byte numbers are **big-endian** (high byte first) and unsigned.

| Bytes | Meaning |
|---|---|
| 0–1 | `F0 BC` (header; see above) |
| 2–5 | not used; meaning unknown |
| 6–7 | speed, km/h × 10 (divide by 10) |
| 8 | not used; meaning unknown |
| 9 | cadence, rpm |
| 10–11 | energy, kcal. When byte 10 is `FF` the value is not valid (the bike sends `FF FF` at start-up) |
| 12–13 | a distance counter, divided by 10. QZ only logs it and integrates speed for its own distance, so its unit (probably km) is not proven |
| 14 | resistance level. A value below 1 is not valid |
| 15–17 | not used; meaning unknown |
| 18 | heart rate, bpm, from the bike's grips or receiver. 0 means none |
| 19–21 | not used; meaning unknown |
| 22 | console button: `06` = start pressed, `07` = stop pressed |
| 23–24 | not used; meaning unknown |
| 25 | last byte; probably the same additive checksum as the written frames. QZ does not check it |

The answers to the start frames are shorter or longer than 20 bytes and are not 26 bytes long;
they carry no values QZ reads.

## Values

| Value | Sent? |
|---|---|
| speed | yes, bytes 6–7 |
| cadence | yes, byte 9 |
| resistance level | yes, byte 14 |
| energy (kcal) | yes, bytes 10–11 |
| heart rate | yes, byte 18 (0 when none) |
| distance | a counter at bytes 12–13, unit not proven |
| power | **power is not sent.** QZ estimates it with its own model; that model is not taken |

## Example

Constructed from the byte positions above, not captured from a bike:

```
F0 BC 00 00 00 00 00 FA 00 5A 00 2D 00 64 08 00 00 00 8C 00 00 00 00 00 00 25
```

or, split: `F0 BC 00 00 00 00 00 FA 00 5A 00 2D 00 64 08 00 00 00 8C 00` then
`00 00 00 00 00 25`.

| Field | Bytes | Value |
|---|---|---|
| speed | `00 FA` | 250 / 10 = 25.0 km/h |
| cadence | `5A` | 90 rpm |
| energy | `00 2D` | 45 kcal |
| distance counter | `00 64` | 100 / 10 = 10.0 (probably km) |
| resistance | `08` | 8 |
| heart rate | `8C` | 140 bpm |
| button | `00` | none |

## Not clear from the code

- Whether the fixed poll alone keeps data coming, without the computed display frames.
- Whether frames 14 and 15 are needed.
- Whether a start frame ever has to be sent twice.
- The header of the data packet when it arrives in one piece, the meaning of the unused bytes,
  and the unit of the distance counter.
