import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { Ofp, wasmEngine, moduleEngine, basic, toHex, base64ToHex, selfTest } from '../index.js'
import * as rn from '../adapters/react-native-ble-manager.js'
import * as noble from '../adapters/noble.js'

const dist = (f) => fileURLToPath(new URL('../../../dist/' + f, import.meta.url))
const bundle = readFileSync(dist('registry.bundle.json'), 'utf8')
const asm = await import(dist('ofp_core.asm.js'))

// The same tests for both forms of the core: WebAssembly, and plain JavaScript (React Native).
const engines = { WebAssembly: () => wasmEngine(readFileSync(dist('ofp_core.wasm'))), 'plain JavaScript': () => moduleEngine(asm) }

/** A Keiser M3i advertisement as a scanner hands it over (company id 0x0102 first). */
function keiser({ power = 150, cadence = 82, hr = 0, type = 0, bike = 197 } = {}) {
  const p = new Uint8Array(19)
  p.set([0x02, 0x01, 0x06, 0x40, type, bike])
  p[6] = (cadence * 10) & 255; p[7] = (cadence * 10) >> 8
  p[8] = (hr * 10) & 255; p[9] = (hr * 10) >> 8
  p[10] = power & 255; p[11] = power >> 8
  p[18] = 11
  return p
}
const BIKE = 'keiser-m3i:197'

for (const [name, engine] of Object.entries(engines)) {
  const setup = (extra = {}) => {
    const seen = { devices: [], values: [], inUse: [], lost: [], silent: [], errors: [] }
    let now = 1_000_000
    const ofp = new Ofp({
      engine: engine(), bundle, tickMs: 0, now: () => now, iRead: ['180d', '1818', '1826'],
      on: { device: (d) => seen.devices.push(d), values: (v) => seen.values.push(v), inUse: (id, auto) => seen.inUse.push([id, auto]), lost: (id) => seen.lost.push(id), silent: (id) => seen.silent.push(id), error: (e) => seen.errors.push(e) },
      ...extra,
    })
    const hear = (o) => ofp.advertisement({ deviceId: 'uuid-1', name: 'M3', rssi: -58, manufacturerData: keiser(o) })
    const pass = (ms) => { for (let t = 0; t < ms; t += 1000) { now += 1000; ofp.tick() } }
    return { ofp, seen, hear, pass }
  }

  test(`${name}: a device is offered once, described, and silent until the rider picks it`, () => {
    const { ofp, seen, hear, pass } = setup()
    hear()
    hear({ power: 160 })
    assert.equal(seen.devices.length, 1)
    assert.deepEqual([seen.devices[0].id, seen.devices[0].name, seen.devices[0].table], [BIKE, 'Keiser M3i · bike 197', 'keiser-m3i'])
    assert.ok(seen.devices[0].fields.some((f) => f.key === 'gear' && f.kind === 'state'))
    assert.equal(seen.values.length, 2, 'every packet gives values; the app decides what to do with a device not in use')
    pass(3000)
    assert.equal(seen.values.length, 2, 'no repeats for a device nobody is using')
    assert.deepEqual(ofp.devices().map((d) => d.id), [BIKE])
    ofp.destroy()
  })

  test(`${name}: in use → values with the basic four, repeated through radio gaps, zero when silent, lost after a minute`, () => {
    const { ofp, seen, hear, pass } = setup()
    assert.equal(ofp.use(BIKE), false, 'cannot use what was never heard')
    hear({ power: 150, cadence: 82 })
    assert.equal(ofp.use(BIKE), true)
    assert.deepEqual(seen.inUse, [[BIKE, false]])
    const last = () => seen.values.at(-1)
    assert.deepEqual(last().basic, { power: 150, cadence: 82, speed: 0, heartRate: 0 })
    assert.equal(last().fields.gear, 11)
    const before = seen.values.length
    pass(5000)
    assert.ok(seen.values.length - before >= 4, 'standing values repeated every second')
    assert.ok(seen.values.slice(before).every((v) => v.basic.power === 150 && !v.silent))
    pass(6000) // 11 s without a packet
    assert.equal(last().silent, true)
    assert.equal(last().basic.power, 0)
    assert.deepEqual(seen.silent, [BIKE])
    assert.equal(ofp.isInUse(BIKE), true)
    hear({ power: 120 })
    assert.equal(last().basic.power, 120)
    pass(61_000)
    assert.deepEqual(seen.lost, [BIKE])
    assert.equal(ofp.isInUse(BIKE), false)
    ofp.destroy()
  })

  test(`${name}: a remembered device is used as soon as it is heard, and released devices stay released`, () => {
    const { ofp, seen, hear } = setup({ remembered: [BIKE] })
    hear({ power: 140 })
    assert.deepEqual(seen.inUse, [[BIKE, true]])
    assert.equal(ofp.isInUse(BIKE), true)
    ofp.release(BIKE)
    hear({ power: 141 })
    assert.equal(ofp.isInUse(BIKE), false)
    assert.equal(seen.inUse.length, 1)
    ofp.destroy()
  })

  test(`${name}: ignores what no table knows, the bike's averages packets, and its bike-0 wake-up packets`, () => {
    const { ofp, seen, hear } = setup()
    ofp.advertisement({ deviceId: 'x', name: 'iPhone', manufacturerData: new Uint8Array([0x4c, 0, 0x10, 5, 11, 28]) })
    ofp.advertisement({ deviceId: 'y', name: 'HRM', manufacturerData: null })
    hear({ type: 0xff })
    hear({ bike: 0 })
    assert.deepEqual([seen.devices.length, seen.values.length, seen.errors.length], [0, 0, 0])
    ofp.destroy()
  })

  test(`${name}: manufacturer data may come as hex text (from base64 on React Native)`, () => {
    const { ofp, seen } = setup()
    const b64 = Buffer.from(keiser({ power: 201 })).toString('base64')
    ofp.advertisement({ deviceId: 'uuid-1', name: 'M3', manufacturerData: base64ToHex(b64) })
    assert.equal(seen.values.at(-1).basic.power, 201)
    ofp.destroy()
  })

  test(`${name}: many calls do not leak or slow down`, () => {
    const { ofp, seen, hear } = setup()
    const t0 = Date.now()
    for (let i = 0; i < 20000; i++) hear({ power: 100 + (i % 200) })
    assert.equal(seen.values.length, 20000)
    assert.ok(Date.now() - t0 < 20000, `took ${Date.now() - t0} ms`)
    ofp.destroy()
  })

  test(`${name}: a bundle the core cannot read is refused when OFP is created`, () => {
    assert.throws(() => new Ofp({ engine: engine(), bundle: '{"version":99,"fields":[],"tables":[]}', tickMs: 0 }), /bundle/i)
  })
}

for (const [name, engine] of Object.entries(engines)) {
  test(`${name}: what the scan must do, radio statistics, heart rate seen, re-offering, self-test`, () => {
    const seen = { devices: [], radio: [] }
    let now = 5_000_000
    const ofp = new Ofp({ engine: engine(), bundle, tickMs: 0, now: () => now, reofferMs: 10_000, on: { device: (d) => seen.devices.push(d.id), radio: (r) => seen.radio.push(r) } })
    const hear = (o) => ofp.advertisement({ deviceId: 'u', name: 'M3', rssi: -61, manufacturerData: keiser(o) })
    assert.deepEqual(ofp.companyIds, [0x0102, 0x0645])
    assert.equal(ofp.scanNeed(), null)
    hear()
    // heard, not in use: offered again every 10 s while it keeps being heard
    for (let i = 0; i < 12; i++) { now += 1000; hear() }
    assert.deepEqual(seen.devices, [BIKE, BIKE])
    assert.equal(ofp.rssi(BIKE), -61)
    assert.equal(ofp.hasHeartRate(BIKE), false)
    ofp.use(BIKE)
    assert.equal(ofp.scanNeed(), 'in_use')
    // a minute in use, a packet every 2 s with one 9 s gap: the app is told how well the radio hears it
    for (let t = 0; t < 62_000; t += 2000) { now += t === 20_000 ? 9000 : 2000; hear({ hr: 140 }); ofp.tick() }
    assert.equal(seen.radio.length, 1)
    assert.equal(seen.radio[0].id, BIKE)
    assert.ok(seen.radio[0].packets >= 25 && seen.radio[0].longestGapMs === 9000, JSON.stringify(seen.radio[0]))
    assert.equal(ofp.hasHeartRate(BIKE), true)
    assert.equal(seen.devices.length, 2, 'a device in use is not offered again')
    // lost but remembered: a slow scan waits for it; released: nothing
    now += 80_000; ofp.tick(); ofp.tick()
    assert.equal(ofp.scanNeed(), 'waiting')
    ofp.release(BIKE)
    assert.equal(ofp.scanNeed(), null)
    ofp.destroy()
    assert.deepEqual({ ...selfTest(engine(), bundle, 20), msPerPacket: 0 }, { ok: true, power: 187, cadence: 84, msPerPacket: 0 })
  })
}

test('adapter: react-native-ble-manager', () => {
  const peripheral = { id: 'AA:BB', name: 'M3', rssi: -60, advertising: { localName: 'M3', serviceUUIDs: [], manufacturerData: { '0102': { bytes: [...keiser({ power: 222 })].slice(2) }, '004c': { bytes: [1, 2] } } } }
  const adverts = rn.advertisementsFrom(peripheral, [0x0102, 0x0645])
  assert.equal(adverts.length, 1)
  assert.deepEqual(adverts[0].manufacturerData, [...keiser({ power: 222 })])
  assert.deepEqual(rn.advertisementsFrom({ id: 'x', advertising: {} }, [0x0102]), [])
  assert.equal(rn.onlyOfpCanRead(peripheral, ['1826', '180D']), true)
  assert.equal(rn.onlyOfpCanRead({ advertising: { serviceUUIDs: ['180d'] } }, ['1826', '180D']), false)
  const ofp = { companyIds: [0x0102, 0x0645] }
  assert.deepEqual(rn.scanOptionsFor(ofp, 'in_use', 2), { manufacturerData: { manufacturerId: 0x0102 }, scanMode: 2 })
  assert.deepEqual(rn.scanOptionsFor(ofp, 'waiting', 2), { manufacturerData: { manufacturerId: 0x0102 } })
  assert.deepEqual(rn.scanOptionsFor({ companyIds: [] }, null, 2), {})
})

test('adapter: noble', () => {
  const data = Buffer.from(keiser())
  assert.deepEqual(noble.advertisementFrom({ id: 'p1', rssi: -55, advertisement: { localName: 'M3', manufacturerData: data } }), { deviceId: 'p1', name: 'M3', rssi: -55, manufacturerData: data })
  assert.equal(noble.advertisementFrom({ id: 'p2', advertisement: { localName: 'HRM' } }), null)
})

test('an error inside the core turns OFP off and tells the app, without throwing into its scanner', () => {
  const calls = { n: 0 }
  const engine = { create: () => ({ call: (r) => { if (r.op === 'advertisement' && ++calls.n > 1) throw new Error('boom'); return r.op === 'advertisement' ? [{ type: 'source_appeared', source_id: 'a:1', name: 'A', table: 'a', table_version: 1, fields: [] }, { type: 'source_in_use', source_id: 'a:1' }] : [] }, destroy() {} }) }
  const seen = { lost: [], errors: [] }
  const ofp = new Ofp({ engine, bundle: '{}', tickMs: 0, on: { lost: (id) => seen.lost.push(id), error: (e) => seen.errors.push(e) } })
  const adv = { deviceId: 'd', manufacturerData: new Uint8Array([1, 2, 3]) }
  ofp.advertisement(adv)
  assert.doesNotThrow(() => ofp.advertisement(adv))
  assert.equal(ofp.failed, true)
  assert.deepEqual(seen.lost, ['a:1'])
  assert.equal(seen.errors.length, 1)
  assert.doesNotThrow(() => { ofp.advertisement(adv); ofp.tick(); ofp.use('a:1'); ofp.release('a:1') })
  assert.deepEqual(ofp.devices(), [])
})

test('helpers', () => {
  assert.equal(toHex(new Uint8Array([0, 1, 255, 16])), '0001ff10')
  assert.equal(base64ToHex('AgEGQADF'), '0201064000c5')
  assert.deepEqual(basic({ power: 187, heart_rate: 142 }), { power: 187, cadence: 0, speed: 0, heartRate: 142 })
})
