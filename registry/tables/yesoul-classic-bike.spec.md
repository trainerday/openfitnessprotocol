# Yesoul bike, older protocol: protocol facts

Facts only, for the clean-room rule in `AGENTS.md`. No table has been written from this page yet.

**Where these facts were learned:** from studying the open-source project QZ (qdomyos-zwift,
GPL-3), as read on 2026-11-20. No code, comments or identifiers from it are reproduced here.
Yesoul has published no specification for this protocol that this project knows of. **Nothing
here has been observed on a device by this project yet.**

## The device

- A Yesoul indoor bike from before Yesoul moved to the standard FTMS service. Newer Yesoul bikes
  (`YS_C1_`, `YS_G1_`, `YS_M1P_`, `YS_A…`, `YS_G1MPLUS`, `YS_G1MMAX`) speak FTMS and are read by
  OFP's standard FTMS table, not by this one.
- Recognised by its advertised name:
  - starts with `YESOUL` (upper case, exact case), or
  - starts with `YS_G1M_` (any case). QZ checks the FTMS prefixes above first, so `YS_G1MPLUS…`
    and `YS_G1MMAX…` never reach this protocol.
- On Android 13 some of these bikes advertise no name. QZ then recognises the bike by its
  manufacturer data: company id `637` (`0x027D`) whose data starts with `01 05 00 FF FF`.
- If a bike named `YESOUL` has no `FFF0` service but does have FTMS (`1826`), QZ treats it as an
  FTMS device instead. So a table for this protocol should require the `FFF0` service.
- It accepts a connection. One fixed frame is written to start the data (below).

## Services and characteristics

| What | UUID |
|---|---|
| Service | `0000FFF0-0000-1000-8000-00805F9B34FB` |
| Data (notify) | `0000FFF4-0000-1000-8000-00805F9B34FB` |
| Write | `0000FFF1-0000-1000-8000-00805F9B34FB` |

## What to write

| | |
|---|---|
| Characteristic | `FFF1` |
| Write type | with response (QZ uses its Bluetooth library's default write, which is with response). Whether `FFF1` also accepts a write without response is not clear from the code |
| Start frames | one frame: `F5 20 20 40 F6` |
| Poll frames | none. Nothing is written after the start frame; the bike keeps sending |

Order: QZ writes the start frame **before** it switches notifications on, waits up to 300 ms for
a notification, then writes `01 00` to the data characteristic's client configuration descriptor.
QZ treats "start frame first, then subscribe" as what this bike needs. Whether the bike also
starts when the frame is written after subscribing (the order an OFP `start` list uses) is not
clear from the code; a recording should check it.

The frame has no checksum, counter or value in it; it is the same for every bike and session.

## The data packet

A notification on `FFF4` is used only when it is exactly **12 bytes** long. Other lengths are
ignored. No header byte is checked. Multi-byte numbers are **big-endian** (high byte first) and
unsigned.

| Bytes | Meaning |
|---|---|
| 0–1 | not used; meaning unknown |
| 2–3 | a distance counter, divided by 100. QZ only logs it and computes its own distance, so its unit (probably km) is not proven |
| 4 | resistance level |
| 5 | not used; meaning unknown |
| 6 | cadence, rpm |
| 7–8 | power, watts |
| 9–11 | not used; meaning unknown |

## Values

| Value | Sent? |
|---|---|
| power | yes, bytes 7–8 |
| cadence | yes, byte 6 |
| resistance level | yes, byte 4 |
| distance | a counter at bytes 2–3, unit not proven (see above) |
| speed | **not sent**. QZ estimates it from cadence |
| energy (kcal) | **not sent**. QZ estimates it from power |
| heart rate | not sent on this characteristic |

## Validity

- QZ reports power as 0 whenever cadence is 0.
- Rate: not clear from the code.

## Example

Constructed from the byte positions above, not captured from a bike:

```
00 00 03 E8 0A 00 55 00 B4 00 00 00
```

| Field | Bytes | Value |
|---|---|---|
| distance counter | `03 E8` | 1000 / 100 = 10.00 (probably km) |
| resistance | `0A` | 10 |
| cadence | `55` | 85 rpm |
| power | `00 B4` | 180 W |

## Not clear from the code

- Whether the start frame must come before subscribing.
- Whether the start frame needs to be repeated after a reconnect (QZ writes it again on every
  connection).
- The meaning of bytes 0–1, 5 and 9–11, and the unit of the distance counter.
- How often the bike sends.
