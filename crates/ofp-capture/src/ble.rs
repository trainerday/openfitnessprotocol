//! Bluetooth for the capture tool: hear advertisements, and connect to record notifications.
//!
//! This is the tool's own radio. OFP Core has none: an app passes in what its Bluetooth layer hears.

use crate::format::{GattCharacteristic, GattService};
use btleplug::api::{Central, CentralEvent, CharPropFlags, Manager as _, Peripheral as _, ScanFilter, ValueNotification};
use btleplug::platform::{Adapter, Manager, Peripheral, PeripheralId};
use futures::stream::{Stream, StreamExt};
use std::collections::{BTreeMap, HashMap};
use std::pin::Pin;

/// One advertisement.
#[derive(Clone, Debug)]
pub struct Adv {
    /// The host's id for the device (on macOS a per-computer UUID, not the radio address).
    pub device_id: String,
    pub name: Option<String>,
    pub rssi: Option<i16>,
    /// Manufacturer data, raw: company id (2 bytes, little endian) first. Empty when there is none.
    pub mfr: Vec<u8>,
    /// Service data, UUID → bytes.
    pub service_data: BTreeMap<String, Vec<u8>>,
    /// Advertised service UUIDs.
    pub services: Vec<String>,
}

#[derive(Default, Clone)]
struct Known {
    name: Option<String>,
    rssi: Option<i16>,
    services: Vec<String>,
}

pub struct Scanner {
    adapter: Adapter,
    events: Pin<Box<dyn Stream<Item = CentralEvent> + Send>>,
    known: HashMap<PeripheralId, Known>,
    pending: Vec<Adv>,
}

fn err<E: std::fmt::Display>(what: &'static str) -> impl Fn(E) -> String {
    move |e| format!("{what}: {e}")
}

/// Short form for the standard 16-bit UUIDs (`180d`), the full UUID otherwise.
pub fn short_uuid(u: &uuid::Uuid) -> String {
    let s = u.to_string();
    if s.starts_with("0000") && s.ends_with("-0000-1000-8000-00805f9b34fb") {
        s[4..8].to_string()
    } else {
        s
    }
}

impl Scanner {
    /// Start scanning: every advertisement of every device, repeats included.
    pub async fn start() -> Result<Scanner, String> {
        let manager = Manager::new().await.map_err(err("Bluetooth"))?;
        let adapter = manager.adapters().await.map_err(err("Bluetooth"))?.into_iter().next().ok_or("this computer has no Bluetooth adapter")?;
        let events = adapter.events().await.map_err(err("Bluetooth"))?;
        adapter.start_scan(ScanFilter::default()).await.map_err(err("could not start scanning (is Bluetooth on, and is this program allowed to use it?)"))?;
        Ok(Scanner { adapter, events, known: HashMap::new(), pending: Vec::new() })
    }

    async fn refresh(&mut self, id: &PeripheralId) -> Known {
        let mut k = self.known.get(id).cloned().unwrap_or_default();
        if let Ok(p) = self.adapter.peripheral(id).await {
            if let Ok(Some(props)) = p.properties().await {
                if let Some(n) = props.local_name.or(props.advertisement_name) {
                    k.name = Some(n);
                }
                if props.rssi.is_some() {
                    k.rssi = props.rssi;
                }
                if !props.services.is_empty() {
                    k.services = props.services.iter().map(short_uuid).collect();
                }
            }
        }
        self.known.insert(id.clone(), k.clone());
        k
    }

    /// The next advertisement that carries data. `None` when Bluetooth stops.
    pub async fn next(&mut self) -> Option<Adv> {
        loop {
            if let Some(a) = self.pending.pop() {
                return Some(a);
            }
            match self.events.next().await? {
                CentralEvent::ManufacturerDataAdvertisement { id, manufacturer_data } => {
                    let k = self.refresh(&id).await;
                    for (company, data) in manufacturer_data {
                        let mut mfr = company.to_le_bytes().to_vec();
                        mfr.extend_from_slice(&data);
                        self.pending.push(Adv { device_id: id.to_string(), name: k.name.clone(), rssi: k.rssi, mfr, service_data: BTreeMap::new(), services: k.services.clone() });
                    }
                }
                CentralEvent::ServiceDataAdvertisement { id, service_data } => {
                    let k = self.refresh(&id).await;
                    let service_data = service_data.iter().map(|(u, d)| (short_uuid(u), d.clone())).collect();
                    self.pending.push(Adv { device_id: id.to_string(), name: k.name, rssi: k.rssi, mfr: Vec::new(), service_data, services: k.services });
                }
                CentralEvent::ServicesAdvertisement { id, services } => {
                    // Devices that advertise only a name and services (most connectable sensors).
                    let k = self.refresh(&id).await;
                    let services: Vec<String> = services.iter().map(short_uuid).collect();
                    self.pending.push(Adv { device_id: id.to_string(), name: k.name, rssi: k.rssi, mfr: Vec::new(), service_data: BTreeMap::new(), services });
                }
                CentralEvent::RssiUpdate { id, rssi } => {
                    self.known.entry(id).or_default().rssi = Some(rssi);
                }
                _ => {}
            }
        }
    }

    pub async fn stop(&mut self) {
        let _ = self.adapter.stop_scan().await;
    }

    /// The device behind an advertisement, for connecting.
    pub async fn peripheral(&mut self, device_id: &str) -> Result<Peripheral, String> {
        for p in self.adapter.peripherals().await.map_err(err("Bluetooth"))? {
            if p.id().to_string() == device_id {
                return Ok(p);
            }
        }
        Err("the device is no longer known".into())
    }
}

fn props(flags: CharPropFlags) -> Vec<String> {
    let mut v = Vec::new();
    for (flag, name) in [
        (CharPropFlags::READ, "read"),
        (CharPropFlags::WRITE, "write"),
        (CharPropFlags::WRITE_WITHOUT_RESPONSE, "write_without_response"),
        (CharPropFlags::NOTIFY, "notify"),
        (CharPropFlags::INDICATE, "indicate"),
    ] {
        if flags.contains(flag) {
            v.push(name.to_string());
        }
    }
    v
}

pub struct Connection {
    pub peripheral: Peripheral,
    pub gatt: Vec<GattService>,
    /// (characteristic UUID, value) for everything that could be read after connecting.
    pub reads: Vec<(String, Vec<u8>)>,
    /// Characteristics the tool subscribed to.
    pub subscribed: Vec<String>,
    pub notifications: Pin<Box<dyn Stream<Item = ValueNotification> + Send>>,
}

/// Connect, list services and characteristics, read what is readable, subscribe to everything that
/// notifies. Nothing is written to the device here; [`write`] does that, only when asked.
pub async fn connect(peripheral: Peripheral) -> Result<Connection, String> {
    peripheral.connect().await.map_err(err("could not connect"))?;
    // Connected from here: a step that fails disconnects, or the device stays connected (and stops advertising)
    match connected(peripheral.clone()).await {
        Ok(c) => Ok(c),
        Err(e) => {
            disconnect(&peripheral).await;
            Err(e)
        }
    }
}

async fn connected(peripheral: Peripheral) -> Result<Connection, String> {
    peripheral.discover_services().await.map_err(err("could not list the services"))?;
    let mut gatt = Vec::new();
    let mut reads = Vec::new();
    let mut subscribed = Vec::new();
    let notifications = peripheral.notifications().await.map_err(err("notifications"))?;
    for service in peripheral.services() {
        let mut characteristics = Vec::new();
        for c in &service.characteristics {
            characteristics.push(GattCharacteristic { uuid: short_uuid(&c.uuid), properties: props(c.properties) });
            if c.properties.contains(CharPropFlags::READ) {
                if let Ok(v) = peripheral.read(c).await {
                    reads.push((short_uuid(&c.uuid), v));
                }
            }
            if c.properties.intersects(CharPropFlags::NOTIFY | CharPropFlags::INDICATE) && peripheral.subscribe(c).await.is_ok() {
                subscribed.push(short_uuid(&c.uuid));
            }
        }
        gatt.push(GattService { uuid: short_uuid(&service.uuid), characteristics });
    }
    Ok(Connection { peripheral, gatt, reads, subscribed, notifications })
}

pub async fn disconnect(peripheral: &Peripheral) {
    let _ = peripheral.disconnect().await;
}

/// Write bytes to one characteristic (by short or full UUID). With a response when the characteristic
/// supports it, unless `with_response` says otherwise.
pub async fn write(peripheral: &Peripheral, chr: &str, data: &[u8], with_response: Option<bool>) -> Result<(), String> {
    use btleplug::api::WriteType;
    let c = peripheral.characteristics().into_iter().find(|c| short_uuid(&c.uuid) == chr || c.uuid.to_string() == chr).ok_or_else(|| format!("the device has no characteristic {chr}"))?;
    let can_respond = c.properties.contains(CharPropFlags::WRITE);
    let can_fire = c.properties.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE);
    if !can_respond && !can_fire {
        return Err(format!("characteristic {chr} cannot be written"));
    }
    let kind = match with_response {
        Some(true) if can_respond => WriteType::WithResponse,
        Some(false) if can_fire => WriteType::WithoutResponse,
        _ if can_respond => WriteType::WithResponse,
        _ => WriteType::WithoutResponse,
    };
    peripheral.write(&c, data, kind).await.map_err(err("write failed"))
}
