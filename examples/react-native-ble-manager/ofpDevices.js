// OFP in a React Native app that scans with react-native-ble-manager (Android).
//
// This is how the TrainerDay mobile app does it, with our own app's names taken out. Two kinds of
// lines: the ones marked SAME FOR EVERYONE you can copy; the ones marked YOUR APP call into your
// own code (`app.…`) and will look different in yours.
//
// React Native's Hermes has no WebAssembly, so the core is the plain-JavaScript build
// (`ofp_core.asm.js`). iOS cannot filter a scan by company id; this example is for Android.
//
// In your app, import from the package: 'ofp' and 'ofp/adapters/react-native-ble-manager'.
import { Ofp, moduleEngine, selfTest } from '../../packages/ofp-js/index.js'
import { advertisementsFrom, onlyOfpCanRead, scanOptionsFor, MAX_SCAN_SECONDS } from '../../packages/ofp-js/adapters/react-native-ble-manager.js'

const PREFIX = 'ofp:'

/**
 * @param BleManager  react-native-ble-manager
 * @param core        the module `ofp_core.asm.js` (import * as core from …)
 * @param bundle      text of the registry bundle
 * @param lowLatency  BleScanMode.LowLatency
 * @param app         YOUR APP: { servicesIRead, remembered, addToDeviceList, setConnected, setDisconnected,
 *                                remember, forget, log }
 */
export function createOfpDevices({ BleManager, core, bundle, lowLatency, app }) {
  const engine = moduleEngine(core)
  // Worth one line at start: says whether the core runs on this phone before any device is near.
  app.log(`OFP self-test: ${JSON.stringify(selfTest(engine, bundle))}`)

  /** What your screens read the numbers from. YOUR APP: ours expects an object per connected device. */
  const live = new Map() // id → { power, cadence, speed, heartRate }

  // ── 1. SAME FOR EVERYONE: start OFP, and say what the app already reads ──────────────────────
  const ofp = new Ofp({
    engine,
    bundle,
    iRead: app.servicesIRead,
    remembered: app.remembered().filter((id) => id.startsWith(PREFIX)).map((id) => id.slice(PREFIX.length)),
    repeatMs: 0, // this app polls `readings(id)` every second; set it if yours waits for events
    reofferMs: 10000, // our device list drops what it has not seen for 30 s
    on: {
      // ── 3. Results out ──────────────────────────────────────────────────────────────────────
      // YOUR APP: we hand our device list a "scan result" that looks like a power meter, so the
      // app's own rules give it its icon and its power and cadence slots.
      device: (d) => {
        if (!ofp.isInUse(d.id)) app.addToDeviceList({ id: PREFIX + d.id, name: d.name, rssi: ofp.rssi(d.id), advertising: { serviceUUIDs: ['1818'], isConnectable: false } })
      },
      values: (v) => {
        if (live.has(PREFIX + v.id)) live.set(PREFIX + v.id, v.basic)
      },
      inUse: (id, auto, d) => {
        live.set(PREFIX + id, { power: 0, cadence: 0, speed: 0, heartRate: 0 })
        app.remember(PREFIX + id)
        app.setConnected(PREFIX + id, d ? d.name : id)
      },
      lost: (id) => {
        live.delete(PREFIX + id)
        app.setDisconnected(PREFIX + id)
      },
      radio: (r) => app.log(`OFP ${r.id}: ${r.packets} packets/min, longest gap ${(r.longestGapMs / 1000).toFixed(1)} s`),
      error: (e) => app.log(`OFP turned itself off: ${e}`),
    },
  })

  // ── 2. SAME FOR EVERYONE: every scan result in, before the app's own handling ────────────────
  /** Call first in your onDiscoverPeripheral listener. True = only OFP can read this one: skip your own handling. */
  function onPeripheral(peripheral) {
    const advertisements = advertisementsFrom(peripheral, ofp.companyIds)
    if (!advertisements.length) return false
    for (const a of advertisements) ofp.advertisement(a)
    return onlyOfpCanRead(peripheral, app.servicesIRead)
  }

  // ── The scan. SAME FOR EVERYONE on this library; where you call it is YOUR APP. ──────────────
  let scanning = null // the need the running scan was started for
  /** Spread into every BleManager.scan({...}) your app makes, so the devices get through the filter. */
  const scanOptions = () => scanOptionsFor(ofp, ofp.scanNeed(), lowLatency)
  /** A broadcast-only device's data IS the scan: every few seconds, put it back if OFP needs it. */
  const timer = setInterval(() => {
    const need = ofp.scanNeed()
    if (!need || app.isScanning()) {
      // a slow scan is running but a device is now in use: restart it listening continuously
      if (need === 'in_use' && scanning !== 'in_use' && app.isScanning()) BleManager.stopScan()
      return
    }
    scanning = need
    BleManager.scan({ serviceUUIDs: app.servicesIRead, seconds: MAX_SCAN_SECONDS, ...scanOptions() })
  }, 5000)

  return {
    isOfp: (id) => typeof id === 'string' && id.startsWith(PREFIX),
    onPeripheral,
    scanOptions,
    // ── 4. SAME FOR EVERYONE: the rider's choice ───────────────────────────────────────────────
    /** The rider tapped the device. Only a device being heard can be used; a silent one returns by itself. */
    connect(id) {
      const bare = id.slice(PREFIX.length)
      return ofp.isInUse(bare) || (ofp.devices(15000).some((d) => d.id === bare) && ofp.use(bare))
    },
    disconnect(id) {
      live.delete(id)
      ofp.release(id.slice(PREFIX.length))
      app.forget(id)
      app.setDisconnected(id)
    },
    /** YOUR APP: the numbers for a device in use (zero when it went silent). */
    readings: (id) => live.get(id),
    destroy() {
      clearInterval(timer)
      ofp.destroy()
    },
  }
}
