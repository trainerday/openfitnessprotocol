# Tymewear VitalPro breathing sensor: protocol facts

Facts only, for the clean-room rule in `AGENTS.md`. The table `tymewear-vitalpro.yaml` was written
from this page.

**Where these facts were learned:** from studying the open-source readers
[k-breathe](https://github.com/gloscherrybomb/k-breathe) and
[Tyme4All](https://github.com/gloscherrybomb/Tyme4All) (both MIT), as read on 2026-11-19. Their
authors report the identifiers as taken from the maker's Android app and the packet layout as worked
out from captured traffic and compared with the maker's own activity files. The maker has published
no specification. **Nothing here has been observed on a sensor by this project yet**, and a firmware
update could change any of it.

## The device

- A chest strap with two separate Bluetooth devices: the breathing sensor (advertised name
  `TYME-` followed by four hexadecimal digits, or containing `VitalPro`) and a heart-rate pod
  (name starting `TymeHR`), which is an ordinary heart-rate monitor.
- The breathing sensor accepts connections. No command, pairing or handshake is needed: once
  notifications are switched on, data arrives.

## Services and characteristics

| What | UUID |
|---|---|
| Service | `40B50000-30B5-11E5-A151-FEFF819CDC90` |
| Data stream (notify): all breathing data arrives here | `40B50004-30B5-11E5-A151-FEFF819CDC90` |
| A second notifying characteristic, not the main stream | `40B50001-30B5-11E5-A151-FEFF819CDC90` |
| Sensor id (the four digits printed on the device) | `40B50007-30B5-11E5-A151-FEFF819CDC90` |
| Battery | standard `180F` / `2A19` |

## Packets on the data stream

The first byte says which kind of packet it is. All numbers are little-endian.

| First byte | Length | How often | What |
|---|---|---|---|
| `01` | 17 bytes | once per breath (every 2 to 4 s) | one breath, below |
| `02` | 13 bytes | about once a second | movement data, not breathing |
| `06` | 17 bytes | with each `01` | two raw sensor peaks; their difference is the `01` packet's raw tidal volume |

A `01` packet:

| Bytes | Meaning |
|---|---|
| 0 | `01` |
| 1–4 | time stamp, unsigned 32-bit, in ticks of 40 ms |
| 5–6 | breathing rate, tenths of a breath per minute (an owner's recording, 2026-11-24; was thought to be the inhale duration) |
| 7–8 | breathing rate again, tenths (per breath or smoothed: which is which is not known yet; was thought to be the exhale duration) |
| 9–10 | tidal volume, raw |
| 11–12 | the same value as 9–10 again |
| 13–14 | the sensor's own minute ventilation: equal to rate × tidal volume (an owner's recording, 2026-11-24) |
| 15–16 | the same value as 13–14 again |

## Turning the raw values into measurements

- **Breathing rate** (breaths per minute): 60 divided by the time between two consecutive `01`
  packets (time stamp difference × 0.040 s). When there is no previous packet, or that gives less
  than 4 breaths per minute (a packet was missed): 6000 divided by (inhale duration + exhale
  duration). That second form implies the durations are in hundredths of a second.
- **Tidal volume**: raw value × 0.01. This scale was found by matching the maker's files; it is an
  approximation, not a calibrated number of litres.
- **Minute ventilation**: breathing rate × tidal volume.
- **Inhale to exhale ratio**: inhale duration ÷ exhale duration.

## Corrected by a sensor (2026-11-24)

The first owner to record a sensor (TYME-A220) found that bytes 5–8 are not inhale and exhale
durations. They are the breathing rate in tenths of a breath per minute, which they checked against
the time between breath packets (one packet per breath). Read as durations, `6000 / (A + B)` gives
300 / rate, which on a ride (25–48 br/min) is a quarter to an eighth of the real rate. Bytes 13–14
equal rate × tidal volume. Still open: which of the two rate values the Tymewear app shows, and a
value from 86 to 154 sent about once a second (probably in the `02` packet).
