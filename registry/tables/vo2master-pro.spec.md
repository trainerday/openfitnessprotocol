# VO2 Master Pro (gas-exchange analyser): protocol facts

Facts only, for the clean-room rule in `AGENTS.md`. No table has been written from this page yet.

**Where these facts were learned:** from studying the open-source project GoldenCheetah (GPL), as
read on 2026-11-20. No code, comments or identifiers from it are reproduced here. GoldenCheetah
refers to the maker's documentation for one rule (the ventilation-only row, below); that document
was not read by this project. **Nothing here has been observed on a device by this project yet**,
and a firmware update could change any of it.

## The device

- A portable mask-worn analyser that measures breathing and, from the exhaled oxygen (and on newer
  firmware carbon dioxide), oxygen uptake.
- It accepts a connection. GoldenCheetah does not check an advertised name: it connects and looks
  for the service below. The service UUID uses the same base as a widely copied chip-maker example
  service (`…-1212-EFDE-1523-785FEABCD123`), so other devices may carry it too; a table should also
  check the advertised name (not known from this source; take it from a capture).
- Reading: notifications are switched on with the usual `01 00` written to each data
  characteristic's client configuration descriptor. See "Writes" for what else GoldenCheetah sends.

## Services and characteristics

| What | UUID |
|---|---|
| Service | `00001523-1212-EFDE-1523-785FEABCD123` |
| Combined data, older firmware (notify) | `00001524-1212-EFDE-1523-785FEABCD123` |
| Command in (write) | `00001525-1212-EFDE-1523-785FEABCD123` |
| Command out / status (notify) | `00001526-1212-EFDE-1523-785FEABCD123` |
| Ventilation data, newer firmware (notify) | `00001527-1212-EFDE-1523-785FEABCD123` |
| Gas-exchange data, newer firmware (notify) | `00001528-1212-EFDE-1523-785FEABCD123` |
| Environment data, newer firmware (notify) | `00001529-1212-EFDE-1523-785FEABCD123` (content not clear from the source; not decoded) |

Newer firmware uses the three separate characteristics `1527`, `1528`, `1529`, and updates them in
that order. Older firmware sends everything on `1524` and has no VCO2. All numbers are
little-endian, unsigned.

## Ventilation packet (`1527`)

| Bytes | Meaning |
|---|---|
| 0–1 | breathing rate, unsigned 16-bit, × 0.01 → breaths per minute |
| 2–3 | tidal volume, unsigned 16-bit, × 0.01 → litres |
| 4–5 | minute ventilation, unsigned 16-bit, × 0.01 → litres per minute |

Whether the packet is longer than 6 bytes is not clear; nothing after byte 5 is read.

## Gas-exchange packet (`1528`)

| Bytes | Meaning |
|---|---|
| 0–1 | FeO2 (fraction of oxygen in exhaled air), unsigned 16-bit, × 0.01 → percent |
| 2–3 | FeCO2, unsigned 16-bit (read but not used by the source; scaling not clear, probably × 0.01 → percent like FeO2) |
| 4–5 | VO2, unsigned 16-bit, millilitres per minute (no scaling) |
| 6–7 | VCO2, unsigned 16-bit, millilitres per minute (no scaling) |

## Combined packet, older firmware (`1524`)

| Bytes | Meaning |
|---|---|
| 0–1 | breathing rate, unsigned 16-bit, × 0.01 → breaths per minute |
| 2 | temperature, 8-bit (read and discarded; unit not clear) |
| 3 | humidity, 8-bit (read and discarded; unit not clear) |
| 4–5 | minute ventilation, unsigned 16-bit, × 0.01 → litres per minute |
| 6–7 | FeO2, unsigned 16-bit, × 0.01 → percent |
| 8–9 | VO2, unsigned 16-bit, millilitres per minute |

No tidal volume and no VCO2 in this packet.

## Validity

- **Ventilation-only row**: when the raw FeO2 is exactly 2200 (22.00 %) and the raw VO2 is exactly
  22, the packet carries no gas-exchange result (only ventilation is valid). Applies to both the
  gas-exchange packet and the older combined packet. GoldenCheetah drops such packets entirely so
  as not to log a zero VO2.
- **RER** (respiratory exchange ratio) is not sent; it is VCO2 ÷ VO2, used only when both are above
  zero.
- VO2 here is absolute (mL/min). Per kilogram needs the rider's weight, which a table cannot know.

## Status messages (`1526`)

Each notification is exactly 2 bytes: a message code, then one value byte. Listening to these is
optional for reading; they are useful for showing calibration and errors.

| Code | Meaning of the value byte |
|---|---|
| 0 | the device did not recognise a command |
| 2 | current state: 1 calibrating, 2 recording (0 is an old, unused state) |
| 4 | mouthpiece (venturi) size: 0 small, 1 medium, 2 large, 3 extra large, 4 resting |
| 5 | calibration progress, 0–100 percent |
| 6 | error code: 0 and up fatal, 50 and up warnings (for example a mask leak, or the mouthpiece too small or too big), 100 and up reasons for an oxygen recalibration, 120 and up diagnostics |
| 8 | volume correction mode: 0 STPD, 1 BTPS |
| 9 | oxygen cell age, 0–100 percent |
| 12 | idle-timeout setting, 0 or 1 |
| 14 | automatic recalibration setting |
| 15 | breath state changed (value meaning not clear from the source) |

The odd codes 1, 3, 7, 10, 11, 13 are the matching "set" commands; their replies carry nothing the
source uses.

## Writes

Nothing in this section is part of reading, and an OFP table never writes. Recorded so that a
reader knows what another app may have set.

- Commands go to `1525` as two bytes: the code from the table above and a value. A "get" sends the
  get code and `00`.
- On connect, GoldenCheetah sets the idle timeout (code 11, value 1), the mouthpiece size to medium
  (code 3, value 1) and the volume correction to STPD (code 7, value 0). Which idle-timeout value
  keeps the device awake is not clear from the source.
- When the user asks for calibration it sends code 1 with value 1 (state: calibrate).
- No secret, challenge or encryption is involved.
- Not clear from the source: whether the device streams data with none of these writes (for
  example when recording is started on the device). If it needs the state write, the device is a
  class C device for OFP, not class B.
- The mouthpiece size and the volume correction mode change how the device computes its values, so
  the same breath gives different numbers under different settings.

## Timing

Not clear from the source. The values are breath-based, so packets probably come about once per
breath or on a fixed reporting interval; take it from a capture.
