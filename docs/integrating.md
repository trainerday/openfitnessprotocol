# Adding OFP to an app

OFP has no Bluetooth of its own: it listens through your app's. So there are four things to wire
up, and one requirement.

1. **Initialize, and say what you already read**: the Bluetooth services your app handles itself.
2. **Give it the full scan**: every advertisement, before your own filtering.
3. **Hook into the results**: recognised devices go into your device list; readings come back as
   events (`basic` = power, cadence, speed, heart rate; `fields` = everything, described once).
4. **Say when the rider picks one**: Connect means "use it", Disconnect means "release it".

**The requirement:** a broadcast-only device's data *is* the scan, so your app keeps scanning, with
repeats, while one is in use. `ofp.scanNeed()` says when (`'in_use'`, `'waiting'`, or nothing).

## Two worked examples

Both are how TrainerDay's own apps do it, with our internals taken out. Lines marked
`SAME FOR EVERYONE` can be copied; lines marked `YOUR APP` call into your code and will differ.

| App | Example | The core |
|---|---|---|
| Electron or Node, scanning with noble | [`examples/electron-noble/ofpDevices.js`](../examples/electron-noble/ofpDevices.js) | WebAssembly |
| React Native (Android), scanning with react-native-ble-manager | [`examples/react-native-ble-manager/ofpDevices.js`](../examples/react-native-ble-manager/ofpDevices.js) | plain JavaScript (Hermes has no WebAssembly) |

They are tested (`examples/test/`) against fake Bluetooth libraries and the real core.

## What it cost us

The four hooks are a few lines each. What took more in our apps, and may or may not in yours:

- **Keeping the scan on.** Both our apps stop scanning once their devices are connected. An app
  whose scanner already runs all the time has nothing to do here.
- **Looking like a device the app expects.** Our mobile app's screens read a dozen things off a
  connected-device object, so an OFP device needs a stand-in with the same properties. An app whose
  devices are just "a source of readings" needs none.
- **Getting through a filtered scan** (phone): our scan is limited to fitness services; on Android a
  company-id filter is added beside them. iOS cannot filter by company id: there the scan is opened to
  every device, with repeats, when it could matter (the rider is looking for devices, or an OFP device is
  paired or in use), and the app drops the results its own filter would not have let through
  (`scanOptionsFor(..., 'ios')` and `appCanRead` in the react-native-ble-manager adapter). iOS hands
  over nothing from such a scan in the background, so a broadcast-only device is read in the foreground only.

In our apps the wiring is one file each, about 250 and 300 lines with comments.

## Asking an assistant to do it

```text
Add Open Fitness Protocol (OFP) to this app. Read docs/integrating.md in the OFP repo and the
example that matches this app's Bluetooth library (examples/…/ofpDevices.js), then:

1. Find where this app scans, where a scan result is handled, how a device gets into the device
   list, how connect and disconnect are handled, and where readings reach the screens.
2. Wire the four hooks in one new file. Keep everything OFP-specific in it; touch the app's own
   files only for the hook calls.
3. Pass the services this app already reads to `iRead`. OFP then leaves those to the app. An empty
   list means OFP reads the standard sensors too (heart rate, power, speed and cadence, running,
   FTMS bikes, rowers and treadmills). A device with several services can be asked for by several
   sources: connect it once and subscribe to everything they ask for.
   Some devices send only after the app writes fixed bytes their table declares: `connect` brings them as
   `start` (write once, in order, after subscribing), and the `write` hook asks for a poll now and then
   (call `tick()` about every 200 ms while such a device is in use). An app that does not wire `write`
   still reads every other device.
4. Find out when this app stops scanning, and keep the scan running while `ofp.scanNeed()` says so.
5. Tell me what an OFP device has to look like for this app's device list and screens, before
   writing a stand-in for it.
6. Add a test with a fake scan result for a Keiser M3i (see the examples' tests), and log the
   self-test at start.
Do not change how the app handles the devices it already supports.
```
