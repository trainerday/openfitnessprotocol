// OFP ↔ react-native-ble-manager (React Native).
//
// Two things differ from what OFP wants:
//  - the manufacturer data arrives keyed by company id ("0102") WITHOUT the id in the bytes (the same on
//    Android and iOS);
//  - apps usually scan filtered to fitness services, which a broadcast-only device does not advertise.
//    Android lets such a device through by its company id. iOS has no such filter: there the scan is opened
//    to every device and the app's own filter is applied to the results (`appCanRead`).

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
 * well enough. Spread them AFTER the app's own options: on iOS they replace two of them.
 *
 * Android: its filters are alternatives (a device passes if it matches any), and the library takes one
 * company id, so that id is added beside the app's services.
 *
 * iOS (`platform` 'ios'): there is no company-id filter, so the scan is opened to every device (an empty
 * service list) and repeats are asked for (without them iOS reports a device once, and for a broadcast-only
 * device the repeats are the data). The app then has to drop what it would not have been given by its own
 * filter: `appCanRead`. iOS only delivers this while the app is in the foreground.
 *
 * @param need  `ofp.scanNeed()`
 * @param lowLatencyMode  the library's `BleScanMode.LowLatency`
 * @param platform  'ios' | 'android' (React Native's `Platform.OS`); Android when left out
 */
export function scanOptionsFor(ofp, need, lowLatencyMode, platform) {
  if (platform === 'ios') return { serviceUUIDs: [], allowDuplicates: true };
  const options = {};
  if (ofp.companyIds.length) options.manufacturerData = { manufacturerId: ofp.companyIds[0] };
  // While a broadcast device is in use its adverts are its data: listen all the time.
  if (need === 'in_use' && lowLatencyMode !== undefined) options.scanMode = lowLatencyMode;
  return options;
}

/** Android turns a scan that runs longer than 30 minutes into one that hears almost nothing: restart before. */
export const MAX_SCAN_SECONDS = 25 * 60;

// A 16-bit service comes as "1818" or in its long form "00001818-0000-1000-8000-00805f9b34fb"
function shortUuid(uuid) {
  const u = String(uuid).toLowerCase();
  const m = /^0000([0-9a-f]{4})-0000-1000-8000-00805f9b34fb$/.exec(u);
  return m ? m[1] : u;
}

/**
 * True when the peripheral advertises one of the services the app reads itself: what a scan filtered to
 * those services would have let through. On an open scan (iOS) everything else that OFP does not read is
 * a phone, a headset, a television: drop it.
 */
export function appCanRead(peripheral, servicesTheAppReads) {
  const mine = servicesTheAppReads.map(shortUuid);
  const adv = (peripheral && peripheral.advertising) || {};
  const theirs = [...(adv.serviceUUIDs || []), ...(adv.overflowServiceUUIDs || [])].map(shortUuid);
  return theirs.some((u) => mine.includes(u));
}

/** True when none of the peripheral's advertised services is one the app reads itself: only OFP can read it. */
export function onlyOfpCanRead(peripheral, servicesTheAppReads) {
  return !appCanRead(peripheral, servicesTheAppReads);
}
