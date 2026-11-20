export interface OfpField {
  key: string
  id: number
  name: string
  unit: string
  type: 'int' | 'decimal'
  decimals: number
  min: number
  max: number
  kind: 'instantaneous' | 'event' | 'cumulative' | 'state'
  tier: 'experimental' | 'stable'
}

/** A device OFP recognised. `id` is `<table>:<instance>`, e.g. `keiser-m3i:197`. */
export interface OfpDevice {
  id: string
  name: string
  table: string
  tableVersion: number
  instance?: number
  instanceLabel?: string
  fields: OfpField[]
  /** Read over a connection: when it comes into use, `on.connect` asks the app to connect and subscribe. */
  connectable: boolean
}

/** What the app's Bluetooth has to do for a connectable device. */
export interface OfpConnectRequest {
  id: string
  /** The app's own id for the device (what it passed as `deviceId` in the advertisement). */
  deviceId: string
  subscribe: Array<{ service: string; characteristic: string }>
}

export interface OfpBasic {
  power: number
  cadence: number
  speed: number
  heartRate: number
}

export interface OfpValues {
  id: string
  t: number
  /** Field key → value: every field the device sends, described in `OfpDevice.fields`. */
  fields: Record<string, number>
  /** The four values every training app has a place for (0 when the device has none). */
  basic: OfpBasic
  /** True for the zeros sent when the device went silent. */
  silent: boolean
  /** True when these are the standing values sent again, not a new packet from the device. */
  repeat: boolean
  rssi?: number
  outOfRange: string[]
}

export interface OfpEngine {
  create(bundleJson: string): { call(request: Record<string, unknown>): any[]; destroy(): void }
}

export interface OfpOptions {
  engine: OfpEngine
  /** The registry bundle (JSON text). */
  bundle: string
  /** Bluetooth services the app reads by itself (short UUIDs: '180d', '1818', '1826', …). */
  iRead?: string[]
  /** Sources the rider used before: used again as soon as they are heard. */
  remembered?: string[]
  on?: {
    device?: (device: OfpDevice) => void
    values?: (values: OfpValues) => void
    /** `auto`: OFP started using a remembered device by itself. */
    inUse?: (id: string, auto: boolean, device?: OfpDevice) => void
    /** A device in use stayed silent for `lostAfterMs`. It is used again by itself if remembered. */
    lost?: (id: string) => void
    silent?: (id: string) => void
    /** Connect to the device and subscribe to these characteristics; then call `connected` and pass their notifications to `notification`. */
    connect?: (request: OfpConnectRequest) => void
    /** The device is no longer in use: the connection is not needed for it. */
    disconnect?: (request: { id: string; deviceId: string }) => void
    /** Once a minute for a device in use: how well the radio hears it. */
    radio?: (stats: { id: string; packets: number; longestGapMs: number; rssi?: number }) => void
    error?: (error: unknown) => void
  }
  minStaleMs?: number
  repeatMs?: number
  lostAfterMs?: number
  /** Announce a device that keeps being heard but is not in use again this often (0 = never). */
  reofferMs?: number
  tickMs?: number
  now?: () => number
}

export interface OfpAdvertisement {
  deviceId: string
  name?: string | null
  rssi?: number | null
  /** Raw manufacturer data, company id first: bytes, or hex text. */
  manufacturerData?: Uint8Array | number[] | string | null
  /** Service UUIDs the device advertises (how devices that are connected to are recognised). */
  services?: string[]
}

export class Ofp {
  constructor(options: OfpOptions)
  readonly failed: boolean
  /** Bluetooth company ids the registry's tables match on. */
  readonly companyIds: number[]
  advertisement(a: OfpAdvertisement): void
  connected(deviceId: string): void
  disconnected(deviceId: string): void
  notification(deviceId: string, characteristic: string, bytes: Uint8Array | number[] | string): void
  use(id: string): boolean
  release(id: string): void
  isInUse(id: string): boolean
  /** Ids of the devices in use. */
  used(): string[]
  devices(heardWithinMs?: number): OfpDevice[]
  describe(id: string): OfpDevice | undefined
  /** 'in_use': scan continuously and do not stop; 'waiting': a slow scan is enough; null: nothing. */
  scanNeed(): 'in_use' | 'waiting' | null
  hasHeartRate(id: string): boolean
  rssi(id: string): number | undefined
  tick(): void
  destroy(): void
}

export function wasmEngine(wasmBytes: Uint8Array): OfpEngine
/** `exports`: a WebAssembly instance's exports, or the module `wasm2js` makes from the same file. */
export function moduleEngine(exports: any): OfpEngine
export function companyIdsOf(bundleJson: string): number[]
export function selfTest(engine: OfpEngine, bundle: string, rounds?: number): { ok: boolean; power?: number; cadence?: number; msPerPacket?: number; error?: string }
export function basic(fields: Record<string, number>): OfpBasic
export function toHex(bytes: Uint8Array | number[]): string
export function base64ToHex(base64: string): string
