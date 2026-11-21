// OFP ↔ noble (@stoprocent/noble, @abandonware/noble): Node and Electron.
// noble hands over the manufacturer data raw, company id first: exactly what OFP wants.

/**
 * A noble peripheral (from the 'discover' event) as an OFP advertisement, or null when it carries nothing
 * OFP could match: no manufacturer data, no name and no services. The services are how a device that is
 * connected to (a `notify` table) is recognised; noble lists them without dashes.
 */
export function advertisementFrom(peripheral) {
  const adv = peripheral && peripheral.advertisement;
  if (!adv) return null;
  const data = adv.manufacturerData && adv.manufacturerData.length >= 2 ? adv.manufacturerData : null;
  const services = [...(adv.serviceUuids || []), ...(adv.overflowServiceUuids || [])];
  const name = adv.localName || null;
  if (!data && !name && !services.length) return null;
  const a = { deviceId: peripheral.id, name, rssi: peripheral.rssi, manufacturerData: data };
  if (services.length) a.services = services;
  return a;
}

/** noble must scan with repeats, or it reports each device once: `noble.startScanning([], true)`. */
export const SCAN_ARGS = [[], true];
