import type { OfpAdvertisement } from '../index'

interface ScannedPeripheral {
  id: string
  name?: string
  rssi?: number
  advertising?: {
    localName?: string
    serviceUUIDs?: string[]
    overflowServiceUUIDs?: string[]
    manufacturerData?: Record<string, { bytes?: number[] } | undefined>
  }
}

/** The OFP advertisements in one scan result: one per company id the registry knows. */
export function advertisementsFrom(peripheral: ScannedPeripheral, companyIds: number[]): OfpAdvertisement[]
/**
 * Extra options for `BleManager.scan({...})`, to be spread after the app's own.
 * Android: the company-id filter, and continuous listening while a device is in use.
 * iOS (`platform` 'ios'): an open scan with repeats; drop the results `appCanRead` says no to.
 */
export function scanOptionsFor(
  ofp: { companyIds: number[] },
  need: 'in_use' | 'waiting' | null,
  lowLatencyMode?: number,
  platform?: 'ios' | 'android' | string,
): { manufacturerData?: { manufacturerId: number }; scanMode?: number; serviceUUIDs?: string[]; allowDuplicates?: boolean }
/** Android degrades a scan that runs longer than 30 minutes: restart before this. */
export const MAX_SCAN_SECONDS: number
/** True when the peripheral advertises one of the services the app reads itself (what the app's filtered scan lets through). */
export function appCanRead(peripheral: ScannedPeripheral, servicesTheAppReads: string[]): boolean
/** True when none of the peripheral's advertised services is one the app reads itself. */
export function onlyOfpCanRead(peripheral: ScannedPeripheral, servicesTheAppReads: string[]): boolean
