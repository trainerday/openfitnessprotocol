// OFP ↔ noble (@stoprocent/noble, @abandonware/noble): Node and Electron.
// noble hands over the manufacturer data raw, company id first: exactly what OFP wants.

/** A noble peripheral (from the 'discover' event) as an OFP advertisement, or null when it carries no data. */
export function advertisementFrom(peripheral) {
  const adv = peripheral && peripheral.advertisement;
  const data = adv && adv.manufacturerData;
  if (!data || data.length < 2) return null;
  return { deviceId: peripheral.id, name: adv.localName || null, rssi: peripheral.rssi, manufacturerData: data };
}

/** noble must scan with repeats, or it reports each device once: `noble.startScanning([], true)`. */
export const SCAN_ARGS = [[], true];
