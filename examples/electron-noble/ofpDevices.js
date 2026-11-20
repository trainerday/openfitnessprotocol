// OFP in an Electron (or Node) app that scans with noble.
//
// This is how the TrainerDay desktop app does it, with our own app's names taken out. Two kinds of
// lines: the ones marked SAME FOR EVERYONE you can copy; the ones marked YOUR APP call into your
// own code (`app.…`) and will look different in yours.
//
//   const ofpDevices = createOfpDevices({ noble, wasm, bundle, app })
//   // in your Connect / Disconnect handlers:
//   if (ofpDevices.isOfp(id)) ofpDevices.connect(id)
//
// In your app, import from the package: 'ofp' and 'ofp/adapters/noble'.
import { Ofp, wasmEngine } from '../../packages/ofp-js/index.js'
import { advertisementFrom, SCAN_ARGS } from '../../packages/ofp-js/adapters/noble.js'

const PREFIX = 'ofp:' // so the app can tell these ids from its Bluetooth ones

/**
 * @param noble   the app's noble instance
 * @param wasm    bytes of ofp_core.wasm
 * @param bundle  text of registry.bundle.json
 * @param app     YOUR APP: { servicesIRead, remembered, addToDeviceList, setConnected, setDisconnected,
 *                            readings, remember, forget, log }
 */
export function createOfpDevices({ noble, wasm, bundle, app }) {
  // ── 1. SAME FOR EVERYONE: start OFP, and say what the app already reads ──────────────────────
  const ofp = new Ofp({
    engine: wasmEngine(wasm),
    bundle,
    iRead: app.servicesIRead, // e.g. ['1826', '1818', '1816', '180d']
    remembered: app.remembered().filter((id) => id.startsWith(PREFIX)).map((id) => id.slice(PREFIX.length)),
    on: {
      // ── 3. Results out ──────────────────────────────────────────────────────────────────────
      device: (d) => app.addToDeviceList(describe(d)), // YOUR APP: whatever your device list takes
      values: (v) => {
        if (ofp.isInUse(v.id)) app.readings(PREFIX + v.id, v.basic, v.fields) // YOUR APP
      },
      inUse: (id, auto, d) => {
        app.remember(PREFIX + id) // YOUR APP: saved for the next launch
        app.setConnected(PREFIX + id, d ? d.name : id)
        keepScanning()
      },
      lost: (id) => app.setDisconnected(PREFIX + id), // silent for a minute; it returns by itself
      radio: (r) => app.log(`OFP ${r.id}: ${r.packets} packets/min, longest gap ${(r.longestGapMs / 1000).toFixed(1)} s`),
      error: (e) => app.log(`OFP turned itself off: ${e}`),
    },
  })

  function describe(d) {
    const can = d.fields.map((f) => f.key).filter((k) => ['power', 'cadence', 'speed'].includes(k))
    // A bike with no strap paired to it sends heart rate 0: offer that only once it was seen
    if (ofp.hasHeartRate(d.id)) can.push('heart_rate')
    return { id: PREFIX + d.id, name: d.name, rssi: ofp.rssi(d.id), can }
  }

  // ── 2. SAME FOR EVERYONE: every advertisement in, before any filtering of your own ───────────
  const onDiscover = (peripheral) => {
    const advertisement = advertisementFrom(peripheral)
    if (advertisement) ofp.advertisement(advertisement)
  }
  noble.on('discover', onDiscover)

  // ── The scan must stay on. YOUR APP decides how; this is the part that differs most. ─────────
  // A broadcast-only device's data IS the scan. Most apps stop scanning once their devices are
  // connected; while OFP needs the scan, start it again whenever it stops.
  function keepScanning() {
    if (ofp.scanNeed() && !app.isScanning()) noble.startScanning(...SCAN_ARGS)
  }
  const onScanStop = () => setTimeout(keepScanning, 1000)
  noble.on('scanStop', onScanStop)

  return {
    isOfp: (id) => typeof id === 'string' && id.startsWith(PREFIX),
    // ── 4. SAME FOR EVERYONE: the rider's choice ───────────────────────────────────────────────
    /** The rider pressed Connect. Nothing is sent to the device: "connect" means "use it". */
    connect: (id) => ofp.use(id.slice(PREFIX.length)),
    /** The rider pressed Disconnect. */
    disconnect(id) {
      ofp.release(id.slice(PREFIX.length))
      app.forget(id)
      app.setDisconnected(id)
    },
    /** For a device list that just opened. */
    devicesInRange: () => ofp.devices().map(describe),
    /** Your scanner may stop when this is null. */
    scanNeed: () => ofp.scanNeed(),
    destroy() {
      noble.removeListener('discover', onDiscover)
      noble.removeListener('scanStop', onScanStop)
      ofp.destroy()
    },
  }
}
