# Nautilus bikes and Schwinn 170 / 270: protocol facts

Facts only, for the clean-room rule in `AGENTS.md`. No table has been written from this page yet.

**Where these facts were learned:** from studying the open-source project QZ (qdomyos-zwift,
GPL-3), as read on 2026-11-20. No code, comments or identifiers from it are reproduced here. Nautilus
Inc. (Nautilus, Schwinn, Bowflex) has published no specification that this project knows of.
**Nothing here has been observed on a device by this project yet.**

## One protocol, two ways QZ reads it

Both bikes use the same Nautilus-family characteristics (the `…-9803-11E3-…-0002A5D5C51B` UUIDs,
also used by QZ for Nautilus and Bowflex treadmills and ellipticals), the same 7-byte start frame,
and the same 14-byte and 20-byte packets for power, speed and resistance. They differ in where
cadence comes from, in heart rate, and in the speed unit. One spec covers both; the differences are
marked **Nautilus** and **Schwinn 170/270**. A table author will probably want two tables.

## Recognition

- **Nautilus:** advertised name starting `NAUTILUS B` (any case), e.g. the B628 and B616.
- **Schwinn 170/270:** advertised name starting `SCHWINN 170/270` (any case).
- QZ recognises both by name only, not by an advertised service.

## Services and characteristics

| What | UUID |
|---|---|
| Write (start frame) | `1717B3C0-9803-11E3-90E1-0002A5D5C51B` |
| Notify 1 | `A46A4A80-9803-11E3-8F3C-0002A5D5C51B` |
| Notify 2 | `6BE8F580-9803-11E3-AB03-0002A5D5C51B` |
| Second write characteristic (Schwinn 170/270 only) | `5EC4E520-9804-11E3-B4B9-0002A5D5C51B` |

The earlier survey (`docs/survey/qz-spin-bikes.md`) lists `1717B3C0-…` and `5EC4E520-…` as
services; in QZ they are **characteristics**.

**Nautilus.** The characteristics are looked up in one of three services, tried in this order:

1. `B1F93401-D4BD-4839-B7D7-B7434E9656CC`
2. `F755C9CF-E1FC-4ECD-8D90-F2D7EBF56B81` (QZ calls this one the B616; it matters for speed units)
3. `44F8D44F-7E03-4BAF-9CC1-BD5A9C7A076B`

QZ subscribes (`01 00`) to both notify characteristics. It handles notifications from both the
same way, telling packets apart only by length, so **which characteristic carries which packet is
not clear from the code**.

**Schwinn 170/270.** QZ does not name a service. It discovers every service and subscribes to every
characteristic that can notify (`01 00`) or, failing that, indicate (`02 00`). The Nautilus
characteristics above are expected to be among them, but which characteristic carries which packet
is not clear from the code.

## What to write

Start, once, after subscribing: **`07 01 D3 00 1F 05 01`** (7 bytes), with response.

- **Nautilus:** written once to `1717B3C0-…` after the subscriptions are confirmed. QZ writes the
  same frame again when the user presses start in QZ; whether that matters is not clear.
- **Schwinn 170/270:** written to `5EC4E520-…` and/or `1717B3C0-…` (whichever exist and are
  writable and cannot also notify, indicate or be read), during the same pass in which QZ
  subscribes, so it may arrive before every subscription is done. QZ also writes the same frame to
  **any** characteristic, in any service, that offers write-without-response and cannot notify,
  indicate or be read. That broad rule is not something to copy; whether the bike needs it is not
  clear from the code. Writing the frame to the two named characteristics is the expected minimum.

What the bytes mean is not clear from the code; they are the same for every bike and every session.

No poll. Nothing else is written to read data (QZ writes nothing for resistance on these bikes).

## The packets

Told apart by length only. Multi-byte numbers are **little-endian, unsigned**.

### 14-byte packet (both bikes)

| Bytes | Meaning |
|---|---|
| 0 | not decoded |
| 1 | **Nautilus:** cadence, rpm (one byte). **Schwinn 170/270:** QZ does not read this byte |
| 2 | not decoded |
| 3–4 | speed × 100 (divide by 100). Unit: see below |
| 5 | not decoded |
| 6–7 | power, watts × 100 (divide by 100) |
| 8–11 | not decoded |
| 12–13 | probably distance; QZ has an unused reading of it (big-endian, ÷ 10, unit unclear) and computes distance from speed instead |

Speed unit:

- **Nautilus, B616 service (`F755C9CF-…`):** km/h, except mph when the bike's console is set to
  miles. QZ follows its own app setting for this, as the bike's setting cannot be read; how a table
  could know is not clear.
- **Nautilus, other services (B628 and others):** always mph.
- **Schwinn 170/270:** QZ uses the value as km/h with no conversion. Whether that is right is not
  clear from the code.

Nautilus cadence: QZ reads byte 1 as a signed byte, which only matters above 127 rpm; it is
expected to be unsigned.

### 20-byte packet (both bikes)

| Bytes | Meaning |
|---|---|
| 16 | **Nautilus:** heart rate, bpm, from the bike's own sensor; 0 means none. **Schwinn 170/270:** not read |
| 18 | resistance level (one byte) |
| others | not decoded |

### 17-byte packet (Schwinn 170/270 only)

| Bytes | Meaning |
|---|---|
| 4–5 | a running crank-revolution counter, 16 bits |
| others | not decoded |

Cadence on the Schwinn 170/270 is **not sent as a value**: QZ works it out from how fast this
counter grows, using the time between packets **as received by the phone**. The packet carries no
event time that QZ uses. QZ only updates cadence when the counter has changed, and drops results of
255 rpm or more. OFP's `rate` needs a time field in the packet, so as far as this spec can tell a
table cannot give Schwinn 170/270 cadence; power, speed and resistance do not depend on it. Whether
some other byte of the 17-byte packet is an event time is not clear from the code.

## Values that exist

| Value | Nautilus | Schwinn 170/270 |
|---|---|---|
| power | yes (14-byte, bytes 6–7) | yes (14-byte, bytes 6–7) |
| speed | yes (14-byte, bytes 3–4; mph or km/h) | yes (14-byte, bytes 3–4; unit unclear) |
| cadence | yes (14-byte, byte 1) | only as a rate from a counter (17-byte, bytes 4–5), see above |
| resistance level | yes (20-byte, byte 18) | yes (20-byte, byte 18) |
| heart rate | yes (20-byte, byte 16) | not read by QZ |

## Validity

- QZ reports power as 0 whenever cadence is 0 (both bikes).
- Heart rate 0 means no reading.
- Rate: not clear from the code.

## Example responses (made up from the layout above, not captured)

Nautilus B628, 14-byte packet:

```
00 50 00 0E 06 00 39 30 00 00 00 00 00 00
```

| Bytes | Raw | Value |
|---|---|---|
| 1 | `50` | 80 rpm |
| 3–4 | `0E 06` | 0x060E = 1550 → 15.50 mph (24.9 km/h) |
| 6–7 | `39 30` | 0x3039 = 12345 → 123.45 W |

Nautilus, 20-byte packet:

```
00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 8C 00 0C 00
```

| Bytes | Raw | Value |
|---|---|---|
| 16 | `8C` | 140 bpm |
| 18 | `0C` | resistance level 12 |

Schwinn 170/270, two 17-byte packets received 750 ms apart:

```
00 00 00 00 F4 01 00 00 00 00 00 00 00 00 00 00 00
00 00 00 00 F5 01 00 00 00 00 00 00 00 00 00 00 00
```

The counter goes from 500 (`F4 01`) to 501 (`F5 01`): one revolution in 0.75 s → 80 rpm.

Unmarked bytes are placeholders.

## Uncertain

- Which notify characteristic carries which packet, on both bikes.
- What the start frame's bytes mean, and whether the Schwinn needs it on more than the two named
  characteristics.
- The Schwinn 170/270 speed unit, and whether its 17-byte packet has a usable event time.
- Whether the Schwinn 170/270's 14-byte byte 1 is cadence as on the Nautilus (QZ does not read it).
