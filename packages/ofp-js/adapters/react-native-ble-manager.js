// OFP ↔ react-native-ble-manager (React Native).
//
// Two things differ from what OFP wants:
//  - the manufacturer data arrives keyed by company id ("0102") WITHOUT the id in the bytes;
//  - apps usually scan filtered to fitness services, which a broadcast-only device does not advertise.

/**
 * The OFP advertisements in one scan result (`onDiscoverPeripheral`): one per company id the
 * registry knows. Empty when the peripheral is nothing OFP could read.
 */
export function advertisementsFrom(peripheral, companyIds) {
  const adv = peripheral && peripheral.advertising;
  const md = adv && adv.manufacturerData;
  if (!md) return [];
  const out = [];
  for (const key of Object.keys(md)) {
    const companyId = parseInt(key, 16);
    if (!companyIds.includes(companyId)) continue;
    const bytes = (md[key] && md[key].bytes) || [];
    out.push({
      deviceId: peripheral.id,
      name: adv.localName || peripheral.name || null,
      rssi: peripheral.rssi,
      // put the company id back in front, low byte first
      manufacturerData: [companyId & 0xff, (companyId >> 8) & 0xff, ...bytes],
    });
  }
  return out;
}

/**
 * Extra options for `BleManager.scan({...})` so OFP's devices get through a filtered scan and are heard
 * well enough. Android only: its filters are alternatives (a device passes if it matches any), and the
 * library takes one company id. iOS cannot filter by company id.
 *
 * @param need  `ofp.scanNeed()`
 * @param lowLatencyMode  the library's `BleScanMode.LowLatency`
 */
export function scanOptionsFor(ofp, need, lowLatencyMode) {
  const options = {};
  if (ofp.companyIds.length) options.manufacturerData = { manufacturerId: ofp.companyIds[0] };
  // While a broadcast device is in use its adverts are its data: listen all the time.
  if (need === 'in_use' && lowLatencyMode !== undefined) options.scanMode = lowLatencyMode;
  return options;
}

/** Android turns a scan that runs longer than 30 minutes into one that hears almost nothing: restart before. */
export const MAX_SCAN_SECONDS = 25 * 60;

/** True when none of the peripheral's advertised services is one the app reads itself: only OFP can read it. */
export function onlyOfpCanRead(peripheral, servicesTheAppReads) {
  const mine = servicesTheAppReads.map((u) => u.toLowerCase());
  const theirs = ((peripheral.advertising && peripheral.advertising.serviceUUIDs) || []).map((u) => u.toLowerCase());
  return !theirs.some((u) => mine.includes(u));
}
