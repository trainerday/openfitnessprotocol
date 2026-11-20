// The examples are real code: they run here against fake Bluetooth libraries and the real core.
import test from 'node:test'
import assert from 'node:assert/strict'
import { EventEmitter } from 'node:events'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { createOfpDevices as electron } from '../electron-noble/ofpDevices.js'
import { createOfpDevices as reactNative } from '../react-native-ble-manager/ofpDevices.js'

const dist = (f) => fileURLToPath(new URL('../../dist/' + f, import.meta.url))
const bundle = readFileSync(dist('registry.bundle.json'), 'utf8')
const BIKE = 'ofp:keiser-m3i:197'

function keiser(power = 150, cadence = 82) {
  const p = new Uint8Array(19)
  p.set([0x02, 0x01, 0x06, 0x40, 0x00, 197])
  p[6] = (cadence * 10) & 255; p[7] = (cadence * 10) >> 8
  p[10] = power & 255; p[11] = power >> 8
  p[18] = 11
  return p
}

function fakeApp(extra = {}) {
  const log = { list: [], connected: [], disconnected: [], readings: [], remembered: [], lines: [] }
  return {
    log,
    app: {
      servicesIRead: ['1826', '1818', '1816', '180d'],
      remembered: () => log.remembered,
      addToDeviceList: (d) => log.list.push(d),
      setConnected: (id, name) => log.connected.push([id, name]),
      setDisconnected: (id) => log.disconnected.push(id),
      readings: (id, basic) => log.readings.push([id, basic]),
      remember: (id) => { if (!log.remembered.includes(id)) log.remembered.push(id) },
      forget: (id) => { log.remembered = log.remembered.filter((x) => x !== id) },
      isScanning: () => false,
      log: (line) => log.lines.push(line),
      ...extra,
    },
  }
}

test('Electron + noble: the bike is listed, used on Connect, read, and the scan is kept on', () => {
  const noble = Object.assign(new EventEmitter(), { scans: 0, startScanning() { this.scans++ } })
  const { app, log } = fakeApp()
  const ofp = electron({ noble, wasm: readFileSync(dist('ofp_core.wasm')), bundle, app })
  noble.emit('discover', { id: 'p', rssi: -58, advertisement: { localName: 'HRM', serviceUuids: ['180d'] } })
  noble.emit('discover', { id: 'q', rssi: -58, advertisement: { manufacturerData: Buffer.from(keiser()) } }) // no name
  assert.deepEqual(log.list, [{ id: BIKE, name: 'Keiser M3i · bike 197', rssi: undefined, can: ['power', 'cadence'] }])
  assert.equal(ofp.isOfp(BIKE), true)
  assert.equal(ofp.isOfp('aa:bb'), false)
  assert.equal(ofp.scanNeed(), null)
  assert.equal(ofp.connect(BIKE), true)
  assert.deepEqual(log.connected, [[BIKE, 'Keiser M3i · bike 197']])
  assert.deepEqual(log.remembered, [BIKE])
  assert.equal(noble.scans, 1, 'the scan is started for the bike')
  noble.emit('discover', { id: 'q', rssi: -58, advertisement: { manufacturerData: Buffer.from(keiser(201, 90)) } })
  assert.deepEqual(log.readings.at(-1), [BIKE, { power: 201, cadence: 90, speed: 0, heartRate: 0 }])
  assert.equal(ofp.scanNeed(), 'in_use')
  ofp.disconnect(BIKE)
  assert.deepEqual(log.disconnected, [BIKE])
  assert.deepEqual(log.remembered, [])
  assert.equal(ofp.scanNeed(), null)
  ofp.destroy()
})

test('React Native + react-native-ble-manager: self-test, filter, found list, readings by polling', async () => {
  const BleManager = { scans: [], stops: 0, scan(o) { this.scans.push(o) }, stopScan() { this.stops++ } }
  const { app, log } = fakeApp()
  const core = await import(dist('ofp_core.asm.js'))
  const ofp = reactNative({ BleManager, core, bundle, lowLatency: 2, app })
  assert.match(log.lines[0], /self-test: \{"ok":true,"power":187,"cadence":84/)
  // the scan filter lets the Keiser's company id through
  assert.deepEqual(ofp.scanOptions(), { manufacturerData: { manufacturerId: 0x0102 } })
  const scanResult = (power, cadence) => ({ id: 'AA:BB', name: 'M3', rssi: -60, advertising: { localName: 'M3', serviceUUIDs: [], manufacturerData: { '0102': { bytes: [...keiser(power, cadence)].slice(2) } } } })
  assert.equal(ofp.onPeripheral({ id: 'x', name: 'KICKR', advertising: { serviceUUIDs: ['1826'] } }), false, 'the app handles its own devices')
  assert.equal(ofp.onPeripheral(scanResult(150, 82)), true, 'the raw M3 is kept out of the app\'s list')
  assert.equal(log.list.length, 1)
  assert.deepEqual([log.list[0].id, log.list[0].advertising.serviceUUIDs], [BIKE, ['1818']])
  assert.equal(ofp.readings(BIKE), undefined)
  assert.equal(ofp.connect(BIKE), true)
  ofp.onPeripheral(scanResult(201, 90))
  assert.deepEqual(ofp.readings(BIKE), { power: 201, cadence: 90, speed: 0, heartRate: 0 })
  assert.deepEqual(ofp.scanOptions(), { manufacturerData: { manufacturerId: 0x0102 }, scanMode: 2 }, 'continuous listening while in use')
  ofp.disconnect(BIKE)
  assert.equal(ofp.readings(BIKE), undefined)
  assert.equal(ofp.connect('ofp:keiser-m3i:5'), false, 'a bike that was never heard')
  ofp.destroy()
})
