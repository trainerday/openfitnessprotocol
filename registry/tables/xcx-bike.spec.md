# XCX bike: protocol facts

Facts only, for the clean-room rule in `AGENTS.md`. No table has been written from this page yet.

**Where these facts were learned:** from studying the open-source project QZ (qdomyos-zwift,
GPL-3), as read on 2026-11-20. No code, comments or identifiers from it are reproduced here. QZ's
notes say its decoding comes from captured traffic of one bike. The maker is not clear from the
code and has published no specification that this project knows of. **Nothing here has been
observed on a device by this project yet.**

## The device

- An indoor bike whose advertised name starts with `XCX-` (any case).
- It also offers the standard FTMS, Cycling Speed and Cadence and Cycling Power services, but
  according to QZ's notes the working data is only on the proprietary characteristic below; the
  FTMS copy reports distance wrongly.
- It accepts a connection. Nothing is written to it: once notifications are switched on (`01 00`
  to the client configuration descriptor), data arrives. A writable characteristic exists, but no
  command format for it is known.

## Services and characteristics

| What | UUID |
|---|---|
| Service | `0000FFF0-0000-1000-8000-00805F9B34FB` |
| Data (notify) | `0000FFF6-0000-1000-8000-00805F9B34FB` |
| Writable, format unknown, not used | `0000FFF5-0000-1000-8000-00805F9B34FB` |

## The data packet

A packet is used only when it is 20 or 23 bytes long and starts with `FA 05`. The 23-byte form
has three extra bytes at the end whose meaning is unknown; only the first 20 bytes are decoded.
All multi-byte numbers are little-endian and unsigned.

| Bytes | Meaning |
|---|---|
| 0–1 | `FA 05` (header) |
| 2 | a state value; meaning unknown |
| 3 | a second state value; meaning unknown |
| 4–5 | a timer; unit not clear |
| 6–7 | speed, km/h × 10 (divide by 10) |
| 8–10 | distance counter, 24-bit; divide by 10 to get kilometres (it moves in steps of 0.1 km) |
| 11–12 | energy, kcal |
| 13–14 | cadence, rpm |
| 15–16 | unknown |
| 17 | power, watts. Only this one byte is proven, so only values up to 255 W are known to be right; whether byte 16 or 18 is a high byte is not known |
| 18–19 | resistance level |

## Validity

- QZ reports power as 0 whenever cadence is 0.
- Rate: not clear from the code.
