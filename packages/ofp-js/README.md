# @openfitnessprotocol/ofp

Open Fitness Protocol for JavaScript apps: read fitness devices from your app's own Bluetooth (heart rate
straps, power meters, speed and cadence sensors, FTMS bikes, rowers and treadmills, and devices the
community adds as tables). Reading only: OFP never controls a device. Apache-2.0, no dependencies.
Project and device list: https://openfitnessprotocol.org

```sh
npm install @openfitnessprotocol/ofp
```

| Import | What it is |
|---|---|
| `@openfitnessprotocol/ofp` | `Ofp` (the four hooks), `wasmEngine`, `moduleEngine`, helpers |
| `@openfitnessprotocol/ofp/core/ofp_core.wasm` | the core as WebAssembly (Electron, browsers, Node) |
| `@openfitnessprotocol/ofp/core/js` | the same core as plain JavaScript, for runtimes without WebAssembly (React Native's Hermes): `moduleEngine(require(...))` |
| `@openfitnessprotocol/ofp/registry` | the device tables, as text (`export default`); `.../registry.bundle.json` is the same file |
| `@openfitnessprotocol/ofp/adapters/noble`, `.../react-native-ble-manager` | helpers for those Bluetooth libraries |

New devices come with a new version of this package: no code of yours changes.

## Usage

Open Fitness Protocol for JavaScript apps: the OFP core and the four hooks an app wires up. No
dependencies.

```js
import { Ofp, wasmEngine } from './ofp.js'

const ofp = new Ofp({
  engine: wasmEngine(wasmBytes),          // or moduleEngine(...) where there is no WebAssembly
  bundle: registryBundleJson,             // the compiled registry (text)
  iRead: ['180d', '1818', '1816', '1826'], // 1. the Bluetooth services your app reads by itself
  remembered: savedIds,                   //    devices the rider used before
  on: {
    device: (d) => addToDeviceList(d),    // 3. a device OFP recognised: { id, name, fields, … }
    values: (v) => show(v.basic, v.fields), //  its readings: basic = { power, cadence, speed, heartRate }
    inUse: (id, auto) => markConnected(id),
    lost: (id) => markDisconnected(id),   //    silent for a minute; comes back by itself if remembered
  },
})

scanner.on('advertisement', (a) =>        // 2. every advertisement, before your own filtering
  ofp.advertisement({ deviceId: a.id, name: a.name, rssi: a.rssi, manufacturerData: a.manufacturerData }))

connectButton.onclick = () => ofp.use(id)       // 4. the rider picked it
disconnectButton.onclick = () => ofp.release(id)
```

- `manufacturerData` is the raw manufacturer data **with the 2-byte company id first**, as bytes or
  hex. Some Bluetooth libraries hand it over without the id (React Native ones, keyed by id): put it
  back in front, low byte first. `base64ToHex` and `toHex` are exported for convenience.
- `values.fields` has every field the device sends, by key; `device.fields` describes each one
  (name, unit, type, range, kind), so an app can show a field it has never heard of.
- **Radio gaps.** A phone's or computer's radio misses advertisements for seconds at a time. OFP
  keeps the last values for `minStaleMs` (default 10 s) before a device reads zero, and can repeat
  them every `repeatMs` (default 1 s) for apps that treat "no event" as "no data"; set `repeatMs: 0`
  if your app polls.
- **The scan must keep running** while a broadcast-only device is in use: its advertisements are
  its data. That part is your app's.
- A failure inside OFP turns it off (`on.error`); it never throws into your scanner.
- Helpers the two reference integrations use: `ofp.scanNeed()`, `on.radio` (how well the radio hears a
  device, once a minute), `reofferMs`, `ofp.hasHeartRate(id)`, `selfTest(engine, bundle)`, and the
  adapters in `adapters/` for noble and react-native-ble-manager.

## The two forms of the core

Both are built by `scripts/build.sh` into `dist/`, with the registry bundle and this package
(`dist/ofp.js`, `dist/ofp.d.ts`):

| Runtime | Core | Engine |
|---|---|---|
| Node, Electron, browsers | `ofp_core.wasm` | `wasmEngine(bytes)` |
| React Native (Hermes has no WebAssembly) | `ofp_core.asm.js`, the same core as plain JavaScript | `moduleEngine(require('./ofp_core.asm.js'))` |

`npm test` runs the same tests against both.
