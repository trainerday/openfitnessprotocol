# Inspire Fitness IC bike: protocol facts

Facts only, for the clean-room rule in `AGENTS.md`. No table has been written from this page yet.

**Where these facts were learned:** from studying the open-source project QZ (qdomyos-zwift,
GPL-3), as read on 2026-11-20. No code, comments or identifiers from it are reproduced here. The
maker has published no specification that this project knows of. **Nothing here has been observed
on a device by this project yet.**

## The device

- An indoor spin bike (Inspire Fitness IC series). QZ recognises it by an advertised name that
  starts with `IC` (any case) and is exactly 8 characters long. That rule is loose: other devices
  could match it, so a table should also check the service below.
- It accepts a connection. Nothing is written to it: once notifications are switched on (the usual
  write of `01 00` to the characteristic's client configuration descriptor), data arrives.

## Services and characteristics

| What | UUID |
|---|---|
| Service | `7EE7E1FA-6FD8-4CBD-B648-A2CDCFA4E7F4` |
| Data (notify) | `7EE72AD1-6FD8-4CBD-B648-A2CDCFA4E7F4` |

## The data packet

Only packets exactly 8 bytes long are used; any other length is ignored.

| Byte | Meaning |
|---|---|
| 0–2 | not used; meaning unknown |
| 3 | cadence, unsigned 8-bit, revolutions per minute |
| 4–5 | not used; meaning unknown |
| 6 | resistance level, 8-bit (the bike has levels 1 to 40) |
| 7 | not used; meaning unknown |

## What is not in the packet

- **Power** is not sent. QZ estimates it from cadence and resistance with its own fitted formulas;
  those are QZ's model, not a fact about the bike, and are not reproduced here. A table for this
  bike would give cadence and resistance only, unless the project makes its own power model from
  its own measurements.
- **Speed** is not sent either; QZ derives it from cadence (or from its power estimate).
- QZ ignores a gap of more than 3 s between packets when adding up distance and energy, which
  suggests packets normally arrive well within that.
