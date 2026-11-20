# "Chrono" bike: protocol facts

Facts only, for the clean-room rule in `AGENTS.md`. No table has been written from this page yet.

**Where these facts were learned:** from studying the open-source project QZ (qdomyos-zwift,
GPL-3), as read on 2026-11-20. No code, comments or identifiers from it are reproduced here. The
maker and model are not clear from the code. **Nothing here has been observed on a device by this
project yet.**

## The device

- An indoor bike whose advertised name starts with `CHRONO ` (any case, with the space).
- It accepts a connection. Nothing is written to it: once notifications are switched on (`01 00`
  to the client configuration descriptor), data arrives.
- The UUIDs below share the base `xxxxxxxx-0A7D-4AB3-97FA-F1500F9FEB8B`, the same base Wahoo uses
  for its own services. Whether the bike is Wahoo-related is not clear.

## Services and characteristics

| What | UUID |
|---|---|
| Service | `A026EE07-0A7D-4AB3-97FA-F1500F9FEB8B` |
| Data (notify) | `A026E01D-0A7D-4AB3-97FA-F1500F9FEB8B` |

## The data packet

Only packets exactly 19 bytes long are used; any other length is ignored. Multi-byte numbers are
little-endian and unsigned.

| Bytes | Meaning |
|---|---|
| 0–5 | not used; meaning unknown |
| 6–7 | speed, km/h × 100 (divide by 100) |
| 8 | cadence in half revolutions per minute (divide by 2 for rpm) |
| 9–16 | not used; meaning unknown |
| 17–18 | power, watts |

## Timing

- QZ treats about 3 s without a packet as a lost connection, which suggests packets normally
  arrive more often than that. The exact rate is not clear from the code.
- Resistance is not in the packet (QZ estimates a resistance figure from power and cadence; that is
  QZ's model, not reproduced here).
