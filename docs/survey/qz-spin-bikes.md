# Spin bikes and indoor bikes supported by QZ: survey

Date: 2026-11-20.

Facts learned from studying the open-source project QZ (qdomyos-zwift, GPL-3); no code, comments
or identifiers from it are reproduced here. Read from a shallow clone of its public repository on
2026-11-20. Nothing here has been observed on a device by this project. Where the code did not make
something clear, the table says so.

## The classes

- **A**: speaks a Bluetooth standard OFP will cover (FTMS Indoor Bike Data `2AD2`, Cycling Power
  Measurement `2A63`, Cycling Speed and Cadence `2A5B`), with nothing proprietary needed for the
  basic values. Writes, if any, are only for controlling resistance or ERG.
- **B**: listens only. Data is in the advertisement, or arrives as notifications after a plain
  subscribe, with nothing written to the device.
- **C**: a fixed start, init or poll command must be written to the device for data to flow. No
  secret is involved (some commands end in a simple checksum over their own fixed bytes).
- **D**: needs a secret, a challenge-response, pairing beyond standard Bluetooth, or encryption.

"Names" are the advertised Bluetooth names QZ matches, case-insensitive unless noted. "n" means a
digit; "(len N)" means QZ also requires the whole name to be N characters long. Several bikes are
only matched when the user has turned on a matching option in QZ; that is noted as "(with a QZ
option)".

Popularity is **rough**: general knowledge of how widely the bike is sold, plus how large and
detailed QZ's support for it is. QZ's issue tracker was not read (it is not in the clone).

## Bluetooth LE bikes

| Bike | Names it advertises | How data arrives | Writes needed? | Values | Class | Popularity (rough) |
|---|---|---|---|---|---|---|
| Generic FTMS bikes and trainers (a long list QZ recognises by name, e.g. Wattbike, Zwift Hub, Concept2 BikeErg (`PM5`), Yesoul `YS_C1_` / `YS_G1_` / `YS_M1P_` / `YS_A` / `YS_G1MPLUS` / `YS_G1MMAX`, Rogue Echo Bike V3 `ECHO_BIKE_`, Sole SB700 `DT-` (len ≥ 14), Joroto JH30 Pro `JOROTO-JH30PRO`, Merach `MERACH-MR667-` / `MERACH-667-`, Tunturi `TUNTURI E60-` / `F40-`, Kettler `KETTLERBLE`, Decathlon Van Rysel `RCR-`, Domyos `DOMYOS-BIKING-` and `Domyos-Bike` when it advertises FTMS, Saris `HAMMER `, Elite `SUITO` / `DIRETO X` / `NERO`, `STAGES BIKE`, Bodytone `DS25-` / `DU30-`, Cecotec, ION Fitness `FI… ION`, Magnus with FTMS, Toputure, Sportstech SX600 `ZV-` (len 9), and about a hundred more prefixes; plus any device the user picks as "FTMS bike") | Connect; notifications on FTMS Indoor Bike Data `2AD2` in service `1826` | No for data. QZ writes FTMS control-point commands only to set resistance, power or simulated grade | power, cadence, speed, distance, resistance level, energy, heart rate (when the flags include them) | A | high in total (dozens of brands) |
| Schwinn IC4 / Bowflex C6 / IC8 and related | `IC BIKE`, `C7-` (len ≠ 17), `C9/C10` | Connect; FTMS Indoor Bike Data `2AD2` | No for data | power, cadence, speed, distance, heart rate | A | high (very common home spin bike in North America and UK) |
| Wahoo KICKR family | `KICKR SNAP`, `KICKR BIKE`, `KICKR ROLLR`, `KICKR CORE`, `KICKR MOVE `, `WAHOO KICKR`, `HOI FRAME ` | Connect; standard Cycling Power `2A63` (`1818`); Wahoo's own control characteristic `A026E005-0A7D-4AB3-97FA-F1500F9FEB8B` for resistance | No for data. Control (ERG, sim, wheel size) goes through Wahoo's control characteristic, which QZ unlocks with a fixed command first | power, cadence (crank data), speed | A | high |
| Tacx Neo / Flux family, ThinkRider, Xplova Noza One, Decathlon Van Rysel HT | `TACX ` (not `TACX SATORI`), `TACX SMART BIKE`, `THINK X`, `THINK-`, `THINK_`, `53997-`, `NOZA-ONE `, `VANRYSEL-HT` | Connect; FTMS `2AD2` / Cycling Power `2A63` / CSC `2A5B`; Tacx FE-C-over-BLE characteristics `6E40FEC2-…` / `6E40FEC3-B5A3-F393-E0A9-E50E24DCCA9E` used for control | No for data | power, cadence, speed | A | high |
| Power-meter style bikes and trainers: Stages SC/SB20, Tacx Satori, Elite trainers, BH Racer S (without the classic option), Elite Misuro B+, Tour 600, Smart+, ThinkRider A, Favero Assioma, short names `KU`, `QD`, `RM`, `DR` (len 2), `DFC` (len 3) | `STAGES `, `TACX SATORI`, `RACER S`, `ELITETRAINER`, `MISURO B+`, `TOUR 600`, `SMART+ #`, `THINK A` (len 18), `ASSIOMA`, plus any power sensor the user picks | Connect; Cycling Power `2A63` (and FTMS `2AD2` when present). Elite's own service `347B0001-7635-408B-8918-8FF3949CE592` is used only for control | No for data. An Elite init on `347B0010…` / `347B0018…` is written only for trainer control | power, cadence | A | medium-high |
| Technogym bikes | `BIKE n…` (with a QZ option), `MYCYCLING` | Connect; Cycling Power `2A63` | No for data; writes only to set power or resistance | power, cadence | A | medium (gyms) |
| Renpho and similar FTMS bikes | `RQ` (len 5), `R-Q…` (len > 6), `SCH130`, `TOORX` (with a QZ option) | Connect; FTMS `2AD2` | QZ writes FTMS "request control", "start" and one further FTMS-format command on connect. Not clear from the code whether data flows without them; in standard FTMS it should | power, cadence, speed, resistance, heart rate | A | medium |
| Snode and similar | `TF-` (not `TF-T`), `FS-` (with a QZ option) | Connect; FTMS `2AD2` | No for data | power, cadence, speed, resistance | A | low-medium |
| CycleOps / Saris Phantom and Magnus without FTMS | `INDOORCYCLE`, `MAGNUS ` (without the FTMS service) | Connect; CSC `2A5B`, Cycling Power, FTMS `2AD2`; a CycleOps service `C0F4013A-A837-4165-BAB9-654EF70747C6` (control point `CA31A533-A858-4DC7-A650-FDEB6DAD4C14`) for control | No for data | power, cadence, speed | A | low-medium |
| NPE CABLE bridge and bikes behind it; generic `BIKE n` / `MDnnnnn` bikes | `>CABLE`, `MD` (len 7), `BIKE` (len 6, without the Technogym / Life Fitness options) | Connect; FTMS `2AD2`, CSC `2A5B`, Cycling Power, Heart Rate. Some send a resistance value on a Nordic-UART notify (`6E400003-B5A3-F393-E0A9-E50E24DCCA9E`, 20-byte packets starting `FF 1F`, byte 15) | No | power, cadence, speed, resistance, heart rate | A | low-medium |
| Bikes that are a CSC sensor | `JOROTO-BK-`, `BGYM` (len 8), any CSC sensor the user picks | Connect; CSC `2A5B` | No | cadence, speed | A | low-medium |
| Bkool smart trainers / bike | `BKOOLSMARTPRO`, `BKOOLFBIKE`, `BKOOLFITNESSBIKE` | Connect; CSC `2A5B`, Cycling Power `2A63`; Bkool service with `F03EE002-…` / `F03EE006-4910-473C-BE46-960948C2F59C` | QZ writes a fixed init and polls on Bkool's own characteristic. Not clear from the code whether the standard CSC/CPS data flows without them (it likely does) | power, cadence, speed | A (uncertain) | low-medium |
| Horizon GR7 / Johnson bikes | `JFIC…` | With FTMS: `2AD2`. Without FTMS: proprietary notify `FFF4` (write `FFF3`) | With FTMS: no. Without FTMS: a fixed 12-byte start command | power, cadence, speed, resistance | A, or C when the bike has no FTMS | low-medium |
| Keiser M3i / M3 plus | name starting `M3`; QZ also checks the advertisement parses and (optionally) the bike id | Advertisement only: manufacturer data; no connection | No | power, cadence, heart rate, gear, distance, time, energy | B (OFP already has a table, `keiser-m3i.yaml`) | medium-high (studios) |
| Inspire Fitness IC bikes | `IC` (len 8) | Connect; notify `7EE72AD1-6FD8-4CBD-B648-A2CDCFA4E7F4` in service `7EE7E1FA-6FD8-4CBD-B648-A2CDCFA4E7F4` | No | cadence, resistance level (power and speed are estimated by QZ, not sent) | B | low |
| "Chrono" bike (maker not clear from the code) | `CHRONO ` | Connect; notify `A026E01D-0A7D-4AB3-97FA-F1500F9FEB8B` in service `A026EE07-0A7D-4AB3-97FA-F1500F9FEB8B` | No | power, cadence, speed | B | low |
| XCX bike (maker not clear from the code) | `XCX-` | Connect; notify `FFF6` in service `FFF0`. It also advertises FTMS, CSC and Cycling Power, but QZ's notes say those do not carry usable data | No | power (≤ 255 W proven), cadence, speed, distance, energy, resistance | B | low |
| Echelon Connect Sport / EX bikes | `ECH…` (case-sensitive) | Connect; notify `0BF669F4-45F2-11E7-9598-0800200C9A66` (write `0BF669F2-…`) in service `0BF669F1-45F2-11E7-9598-0800200C9A66` | Yes: fixed init commands and a repeated poll. Newer firmware sends a challenge that must be answered before data flows | power (estimated from resistance and cadence), cadence, resistance | C on older firmware; D on firmware that sends the unlock challenge | high |
| Flywheel Home bike; Life Fitness IC5 / IC8 | `Flywheel…` (case-sensitive), `BIKE n` (len 6, with a QZ option) | Connect; Nordic UART service `6E400001-…`, notify `6E400003-B5A3-F393-E0A9-E50E24DCCA9E`, write `6E400002-…` | Yes: a fixed 5-byte start command (Flywheel) or two fixed longer frames (Life Fitness IC5) | power, cadence, resistance | C | medium (Flywheel bikes are no longer sold, Life Fitness is common in gyms) |
| ProForm / NordicTrack (iFit) Bluetooth bikes | `I_EB`, `I_SB`, or containing `_IFIT_BIKE` (case-sensitive prefixes) | Connect; iFit service `00001533-1412-EFDE-1523-785FEABCD123`, characteristics `…1534…` / `…1535…` | Yes: long model-specific fixed init sequences, then fixed poll frames sent continuously; data arrives as replies | power, cadence, speed, resistance, incline, heart rate | C | high (largest single code area in QZ for bikes) |
| Domyos bikes without FTMS (Decathlon) | `Domyos-Bike` (case-sensitive; not `DomyosBridge`) without the FTMS service | Connect; Microchip transparent-UART service `49535343-FE7D-4AE5-8FA9-9FAFD205E455`, notify `49535343-1E4D-…` / write `49535343-8841-…` | Yes: fixed init frames, then a repeated fixed poll; frames end in an additive checksum over their own bytes | power, cadence, speed, resistance, heart rate | C | high in Europe |
| Yesoul (older protocol) | `YESOUL` (case-sensitive), `YS_G1M_` | Connect; proprietary notify `FFF4` in service `FFF0` (write `FFF1`) | Yes: a fixed 5-byte start command | power, cadence, speed, resistance | C | medium-high (newer Yesouls use FTMS, row 1) |
| Sole bikes (LCB, LCR, R92) | `LCB`, `LCR`, `R92` | Connect; Microchip transparent-UART service `49535343-…` | Yes: fixed init frames and a repeated fixed poll | power, cadence, speed, resistance, heart rate | C | medium |
| Toorx / iConsole+ / FitHiWay family (many Chinese-made consoles) | `TUN `, `FITHIWAY`, `FIT HI WAY`, `BODYSCULPTURE`, `CARE11216+`, `BIKZU_`, `PASYOU-`, `VIRTUFIT`, `IBIKING+`, `TC` + 3 digits (len 5), containing `RI009R`, `ADIDAS` + 4 or more digits; with a QZ option also `TOORX`, `I-CONSOLE+`, `ICONSOLE+`, `VIFHTR2.1`, `REEBOK`, containing `CR011R`, `FAL-SPORTS`, `DKN MOTION` | Connect; service `FFF0` (`FFF1`/`FFF2`) or Microchip transparent-UART `49535343-…` | Yes: fixed init sequences that depend on the console, then a repeated fixed poll | power, cadence, speed, resistance, heart rate | C | medium-high in total |
| Sportstech bikes | `EW-BK`, `AF` (len 7) | Connect; service `FFF0` (`FFF1` notify / `FFF2` write) | Yes: four fixed init frames, then a repeated fixed poll | power, cadence, speed, resistance, heart rate | C | medium (Europe) |
| Sports Plus / Care Fitness / Cardiofit | `CARDIOFIT`, `HT` (len 10), containing `CARE` (len 11, not `CARE11216+`), some 13-character `CARE…` names | Connect; service `FFF0` | Yes: a fixed 5-byte init (repeated on some models), then a repeated poll | power, cadence, speed, resistance, heart rate | C | low-medium |
| Skandika Wiri / X-2000 | `BFCP`, `HT` (len 11 or 12) | Connect; service `FFF0` | Yes: a fixed 5-byte init, then a repeated poll | power, cadence, speed, resistance | C | low |
| Ultrasport X-Bike | `X-BIKE` | Connect; service `FFF0`, characteristic `FFF6` | Yes: two short fixed init frames repeated, then a poll | power, cadence, speed, resistance | C | low |
| Volava bike | `XQ` (len 12) | Connect; custom service `00010203-0405-0607-0809-0A0B0C0D1910`, characteristics `…2B10` / `…2B11` | Yes: a fixed user-profile frame and a fixed start frame | power, cadence, resistance | C | low |
| Keep bikes | `KEEP_BIKE_`, `KEEP_CC_` | Connect; service `00FF` (`FF01` / `FF02`) | Yes: a fixed init sequence whose frames contain fixed text strings that look like an identifier or token; newer firmware uses framed commands with a CRC-16 and protobuf-encoded fields | power, cadence, resistance | C (the fixed strings may be a device-specific token: unclear) | low-medium (China) |
| Kettler C12 | `ERGO C12`, `KETTLER C12` | Connect; Kettler service `638A1005-7BDE-3E25-FFC5-9DE9B2A0197A` / `638A100A-…` | Yes: a fixed setup sequence of about ten frames | power, cadence, speed | C | low |
| Nautilus bikes (B628 etc.) | `NAUTILUS B` | Connect; Nautilus service family `1717B3C0-9803-11E3-90E1-0002A5D5C51B` and related | Yes: one fixed 7-byte init | power, cadence, speed, resistance, heart rate | C | medium |
| Schwinn 170 / 270 | `SCHWINN 170/270` | Connect; Nautilus-family services `1717B3C0-…`, `5EC4E520-9804-11E3-B4B9-0002A5D5C51B` | Yes: a fixed init written when the service is ready | power, cadence, resistance, heart rate | C | medium |
| MCF bikes | `MCF-` | Connect; Nordic UART `6E400001-…` | Yes: six fixed 20-byte init frames, then a repeated fixed poll | power, cadence, speed, resistance | C | low |
| Apex bikes | `WLT8266BM` | Connect; service `FFF0` (`FFF1` / `FFF2`) | Yes: a fixed 10-byte init repeated three times, then a repeated fixed 10-byte poll | cadence, resistance (power is estimated by QZ from a table) | C | low |
| Pafers bikes | `PAFERS_` | Connect; service `72D70001-501F-46F7-95F9-23846EE1ABA3` | Yes: four fixed 4-byte init commands, then a repeated fixed poll | power, cadence, speed, resistance | C | low |
| PitPat bikes | `PITPAT-S` | Connect; proprietary service (UUIDs found at run time) | Yes: a fixed handshake on the legacy protocol and a repeated poll; frames end in an XOR checksum over their own bytes | power, cadence, speed | C | low-medium |
| MePanel bikes | `MEPANEL` | Connect; service `FFF0` | Yes: a fixed 4-byte init whose last byte is a checksum of the two before it | power, cadence, resistance | C | low |
| Lifespan bikes | `LIFESPAN` (with a QZ option) | Connect; proprietary service | Yes: a fixed 5-byte init, then nine fixed 5-byte query commands in rotation (one per value) | power, cadence, speed, resistance, distance | C | low-medium |
| Freebeat Boom | `FEAQ51A_` | Connect; service `FFF0` (`FFF1` / `FFF2`) | Yes: a fixed "query telemetry" command repeated | power, cadence, resistance | C | low |
| FitPlus / Merach / Osaka bikes without FTMS | `FS-` (with a QZ option), `X100-`, `H9110 OSAKA`, `MRK-` (not `MRK-T12-`) | Connect; service `FFF0`, or FTMS `2AD2` when the bike has it | Without FTMS: fixed init sequences and a repeated poll | power, cadence, speed, resistance | C (A when the bike has FTMS) | medium |
| Kinetic inRide / Smart Control | `SMART CONTROL` | Connect; Kinetic service `E9410200-B434-446B-B5CC-36592FC4C724` and related | Data is obfuscated / encrypted; needs decoding with device-specific values | power, cadence, speed | D | low-medium |

## Not Bluetooth LE (outside OFP's scope today)

- Peloton Bike: QZ reads it from the screen (optical reading), not Bluetooth.
- ProForm / NordicTrack Wi-Fi and Telnet bikes, NordicTrack iFit via ADB, Wahoo DirCon: network.
- Computrainer, Kettler USB, Daum, Freebeat (serial variant): serial or USB.
- ANT+ bikes (desktop and Android): ANT+.
- BH (`BH DUALKIT`, `BH-`) and iConsole (`ICONSOLE+` with a QZ option) and Kettler classic
  (`RACER S` with a QZ option): classic Bluetooth serial port profile, not BLE.

## Counts

| Class | Rows |
|---|---|
| A | 13 (the first row alone covers about 150 name prefixes) |
| B | 4 (Keiser M3i, Inspire, Chrono, XCX) |
| C | 24 (Echelon counted here; Horizon GR7 and FitPlus without FTMS are also C, but Horizon is counted under A) |
| D | 1 (Kinetic), plus Echelon on newer firmware |
| Not BLE | 6 groups |

## Class-C bikes: what kind of command

- Echelon (older firmware): three fixed short commands (4 to 5 bytes) at start, then a repeated
  fixed poll whose last byte is a sum of the others.
- Flywheel / Life Fitness IC5: a fixed 5-byte start command on the write characteristic (Life
  Fitness: two fixed frames of 15 to 19 bytes).
- ProForm / NordicTrack iFit: long fixed init sequences that differ by model, then a fixed poll
  frame sent continuously; data comes back as replies.
- Domyos (non-FTMS): fixed init frames, then a repeated fixed poll; additive checksum in the frame.
- Yesoul (older protocol): a fixed 5-byte start command.
- Horizon GR7 without FTMS: a fixed 12-byte start command.
- Sole: fixed init frames and a repeated fixed poll.
- Toorx / iConsole+ family: fixed init sequences that depend on the console, then a repeated poll.
- Sportstech: four fixed init frames (4 to 17 bytes), then a repeated poll.
- Sports Plus / Care: a fixed 5-byte init (sent several times on some models), then a poll.
- Skandika Wiri: a fixed 5-byte init, then a poll.
- Ultrasport X-Bike: two short fixed init frames, repeated, then a poll.
- Volava: a fixed user-profile frame and a fixed start frame.
- Keep: a fixed init sequence containing fixed text strings (possibly an identifier); newer
  firmware uses CRC-16 framed commands.
- Kettler C12: a fixed setup sequence of about ten short frames.
- Nautilus: one fixed 7-byte init.
- Schwinn 170/270: one fixed init when the service is ready.
- MCF: six fixed 20-byte init frames, then a repeated fixed 20-byte poll.
- Apex: a fixed 10-byte init sent three times, then a fixed 10-byte poll.
- Pafers: four fixed 4-byte init commands, then a fixed 13-byte poll.
- PitPat: a fixed handshake frame, then a repeated poll; XOR checksum in the frame.
- MePanel: a fixed 4-byte init with a checksum over its own bytes.
- Lifespan: a fixed 5-byte init, then nine fixed 5-byte queries in rotation.
- Freebeat Boom: a fixed query command repeated.
- FitPlus / Merach without FTMS: fixed init sequences, then a repeated poll.

Most class-C bikes would be readable if OFP allowed a table to declare (1) a fixed list of frames
to write once after subscribing and (2) a fixed frame to repeat at a set interval. A few compute a
simple checksum, but in every case listed the frames themselves are constant, so the checksum can
be written as part of the fixed bytes.

## Uncertain

- Renpho and Bkool: whether the standard data flows without the writes QZ sends on connect.
- Keep: whether the fixed strings in its init are the same for every bike (a fixed command) or
  per device (a token, which would make it D).
- Chrono and XCX: the maker is not clear from the code.
- Popularity is rough throughout; QZ's issue tracker was not read.
