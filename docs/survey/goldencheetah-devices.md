# Bluetooth LE devices read by GoldenCheetah: survey

Date: 2026-11-20.

Facts learned from studying the open-source project GoldenCheetah (GPL); no code, comments or
identifiers from it are reproduced here. Read from a shallow clone of its public repository on
2026-11-20. Nothing here has been observed on a device by this project. Where the code did not make
something clear, this page says so.

The question: does GoldenCheetah read any Bluetooth LE device or sensor type that neither the QZ
survey (`qz-spin-bikes.md`) nor OFP's existing tables cover? Only reading matters here, not
trainer control.

## The classes

- **B**: listens only. Data arrives in the advertisement, or as notifications after a plain
  subscribe, with nothing written to the device.
- **C**: a fixed command (no secret) must be written for data to flow.
- **D**: needs a secret, a challenge-response, encryption or obfuscation keyed to the device.
- **S**: a Bluetooth SIG standard service OFP has no table for yet.

## How GoldenCheetah finds Bluetooth LE devices

It does not match advertised names, advertised services or company ids. It scans, connects to
every Bluetooth LE device found (or only to the addresses the user saved for the device profile),
discovers the services, and uses the ones it knows. So "how it is recognised" below is always a
service found after connecting.

## New items

Only one Bluetooth LE device that GoldenCheetah reads is not already covered.

| Device or sensor | How it is recognised | How data arrives | Writes needed? | Values | Class | Also in QZ? |
|---|---|---|---|---|---|---|
| VO2 Master Pro (portable gas-exchange / VO2 analyser) | Service `00001523-1212-EFDE-1523-785FEABCD123` found after connecting (no name check). That UUID is also the base of a widely copied chip-maker example service, so a table should check the name as well | Connect; notifications on three characteristics in that service (ventilation `…1527…`, gas exchange `…1528…`; older firmware sends everything on one, `…1524…`). Status replies come as notifications on `…1526…` | GoldenCheetah writes three settings on connect (idle timeout, mouthpiece size, volume correction mode) and a "calibrate" state on the user's request, on `…1525…`. All are plain two-byte frames, no secret. Not clear from the code whether data flows without them; they look like settings, not an unlock | breathing rate, tidal volume, minute ventilation, FeO2, FeCO2, VO2, VCO2 (newer firmware), RER (derived); older firmware also a temperature and a humidity byte; status: calibration progress, error codes, breath state | B (C if the device turns out to need the state write before it records) | Not in the QZ survey (which covered bikes only) |

Spec written: `registry/tables/vo2master-pro.spec.md`.

No muscle-oxygen, core-temperature, running or other sensor type is read over Bluetooth LE by
GoldenCheetah; those it reads over ANT+ only (list below). No new SIG standard service (class S)
is read either.

## Already covered

GoldenCheetah also reads, over Bluetooth LE: Heart Rate `2A37` (8- and 16-bit), Cycling Power
`2A63` (power, wheel and crank revolutions), Cycling Speed and Cadence `2A5B`, FTMS Indoor Bike
Data `2AD2` (power, cadence, speed), all in OFP's standard tables; and the Kinetic inRide (service
`E9410100-B434-446B-B5CC-36592FC4C724`, an older service than the one in the QZ survey) and Kinetic
Smart Control (`E9410200-…`), whose data is obfuscated (class D, as in the QZ survey; on the inRide
the power is not sent but computed by the app from speed). The Tacx FE-C-over-BLE characteristic,
Wahoo's control characteristic and the FTMS control point, feature, range and status
characteristics are used only for trainer control and calibration.

## Not Bluetooth LE (context only)

- **ANT+** (a local USB stick, or a Quarq ANT+ bridge over the network): heart rate, power,
  speed, cadence, speed and cadence, fitness equipment (FE-C), foot pod, **Moxy muscle oxygen**,
  **CORE body temperature**, ANT+ environment temperature (Tempe), Tacx Vortex trainer, and ANT+
  remote controls.
- **USB**: Tacx Fortius and Tacx iMagic trainers, Monark ergometers.
- **Serial**: CompuTrainer, Kettler and Kettler Racer ergometers, Ergofit, Daum.

## Uncertain

- VO2 Master Pro: whether notifications flow with no write at all (for example when recording is
  started on the device itself), and how often each characteristic notifies.
- VO2 Master Pro: the scaling of FeCO2 (GoldenCheetah reads it but never uses it) and the units of
  the older firmware's temperature and humidity bytes (read and discarded).
- VO2 Master Pro: the third, environment characteristic (`…1529…`) is known to GoldenCheetah but
  not subscribed or decoded; its content is not clear from the code.
