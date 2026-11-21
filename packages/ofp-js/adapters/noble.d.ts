import type { OfpAdvertisement } from '../index'

/** A noble peripheral (from the 'discover' event) as an OFP advertisement, or null when it carries no manufacturer data, name or services. */
export function advertisementFrom(peripheral: {
  id: string
  rssi?: number
  advertisement?: { localName?: string; manufacturerData?: Uint8Array | null; serviceUuids?: string[]; overflowServiceUuids?: string[] }
}): OfpAdvertisement | null
/** Arguments for `noble.startScanning`: no service filter, with repeats. */
export const SCAN_ARGS: [string[], boolean]
