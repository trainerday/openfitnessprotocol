//! The broadcast reader and the table engine.

use crate::bundle::{Bundle, FieldDef, FieldType, MatchRule, RateSpec, SampleKind, Table, Tier, CONTROL_POINTS, MAX_FRAME_BYTES, MAX_LAYOUT_PARTS, MAX_WRITE_FRAMES, MIN_POLL_MS};
use crate::expr::{Cx, Expr, PartType};
use crate::BUNDLE_VERSION;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use serde::Serialize;
use serde_json::Value;

/// Most sources the core tracks at once (a room of bikes). Further ones are ignored.
const MAX_SOURCES: usize = 64;

/// One BLE advertisement, as the host's Bluetooth layer heard it.
#[derive(Clone, Copy, Debug)]
pub struct Advertisement<'a> {
    /// The host's id for the device. Only used when a table has no instance value.
    pub device_id: &'a str,
    pub name: Option<&'a str>,
    pub rssi: Option<i16>,
    /// Manufacturer data, raw: the 2-byte company id first (when the device sends one).
    pub manufacturer_data: &'a [u8],
    /// Service UUIDs the device advertises (text, any form).
    pub services: &'a [String],
    pub now_ms: u64,
}

/// A characteristic the host subscribes to for a source.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Subscription {
    pub service: String,
    pub characteristic: String,
}

/// A UUID in one form for comparing: lower case, no dashes, the Bluetooth base UUID shortened
/// (`0000180D-0000-1000-8000-00805F9B34FB` → `180d`).
pub fn normalize_uuid(uuid: &str) -> String {
    let u: String = uuid.chars().filter(|c| *c != '-').flat_map(|c| c.to_lowercase()).collect();
    if u.len() == 32 && u.starts_with("0000") && u.ends_with("00001000800000805f9b34fb") {
        u[4..8].to_string()
    } else {
        u
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct FieldDescription {
    pub key: String,
    pub id: u32,
    pub name: String,
    pub unit: String,
    #[serde(rename = "type")]
    pub ty: FieldType,
    pub decimals: u8,
    pub min: f64,
    pub max: f64,
    pub kind: SampleKind,
    pub tier: Tier,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    /// Sent once, before the first values of a source: what it is and which fields it sends.
    SourceAppeared {
        source_id: String,
        table: String,
        table_version: u32,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        instance_label: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        instance: Option<i64>,
        fields: Vec<FieldDescription>,
        /// The source is read over a connection: when the rider picks it, the core asks the host to
        /// connect and subscribe (`connect`). False for broadcast-only devices.
        #[serde(skip_serializing_if = "core::ops::Not::not")]
        connectable: bool,
    },
    /// For the host: connect to this device and subscribe to these characteristics, then pass
    /// their notifications in. Sent when a connectable source comes into use, and again while it
    /// is in use and not connected.
    Connect {
        source_id: String,
        device_id: String,
        subscribe: Vec<Subscription>,
        /// Written once, in order, after subscribing (the table's `start`). Empty for most devices.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        start: Vec<Write>,
    },
    /// For the host: write these fixed bytes to the connected device (a table's `poll`).
    Write { source_id: String, device_id: String, write: Write },
    /// For the host: the source is no longer in use, the connection is not needed for it.
    Disconnect { source_id: String, device_id: String },
    Values {
        source_id: String,
        t: u64,
        fields: BTreeMap<String, Value>,
        /// Fields the packet carried whose value was outside the field's plausible range.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        out_of_range: Vec<String>,
        /// True for the zero values sent when a source goes silent.
        #[serde(skip_serializing_if = "core::ops::Not::not")]
        stale: bool,
        /// True when these are the standing values sent again (`repeat_ms`), not a new packet.
        #[serde(skip_serializing_if = "core::ops::Not::not")]
        repeat: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        rssi: Option<i16>,
    },
    /// No packet for the table's `stale_after_ms`. Values resume without a new `source_appeared`.
    SourceStale { source_id: String },
    /// The source is now in use. `auto` when the core started using it by itself because the host
    /// had listed it as remembered and it was heard.
    SourceInUse {
        source_id: String,
        #[serde(skip_serializing_if = "core::ops::Not::not")]
        auto: bool,
    },
    /// A source in use stayed silent for the host's `lost_after_ms`. It is no longer in use; if it
    /// is remembered it is used again by itself when it is heard.
    SourceLost { source_id: String },
}

struct Source {
    table: usize,
    last_ms: u64,
    stale: bool,
    in_use: bool,
    /// What `source_appeared` said, kept so it can be said again.
    description: Message,
    /// The standing values (the last of each field).
    values: BTreeMap<String, Value>,
    /// When values last went out to the host (a packet or a repeat).
    emitted_ms: u64,
    rssi: Option<i16>,
    /// A source read over a connection.
    gatt: Option<Gatt>,
    /// One per field of the table: the running counters behind rate fields.
    rates: Vec<RateState>,
}

struct Gatt {
    device_id: String,
    connected: bool,
    /// When the host was last asked to connect.
    asked_ms: Option<u64>,
    /// The next poll frame, and when it is due.
    poll_index: usize,
    poll_due_ms: u64,
}

/// Fixed bytes for the host to write to one characteristic.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Write {
    pub service: String,
    pub characteristic: String,
    /// The bytes, as hex.
    pub data: String,
    pub with_response: bool,
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn writes_of(w: &crate::bundle::Writes) -> Vec<Write> {
    w.frames.iter().map(|f| Write { service: display_uuid(&w.service), characteristic: display_uuid(&w.characteristic), data: hex(f), with_response: w.with_response }).collect()
}

fn start_of(table: &Table) -> Vec<Write> {
    table.start.as_ref().map(writes_of).unwrap_or_default()
}

/// Ask the host again to connect a source in use at most this often.
const CONNECT_RETRY_MS: u64 = 5000;

/// What the host sets once, at the start.
#[derive(Clone, Debug)]
pub struct Settings {
    /// Wait at least this long before a source counts as silent, whatever its table says: for a
    /// radio that misses many advertisements.
    pub min_stale_ms: u64,
    /// While a source in use is not silent, send its standing values again when nothing went out
    /// for this long (0 = never). For hosts that treat "no event" as "no data".
    pub repeat_ms: u64,
    /// A source in use that is silent this long is lost (0 = never).
    pub lost_after_ms: u64,
    /// Sources the rider used before: they are used again as soon as they are heard.
    pub remembered: Vec<String>,
    /// Bluetooth services the app reads by itself. `None` = the app did not say. A connected-device
    /// table for one of these services is never offered: the app reads that service its own way.
    pub handled_services: Option<Vec<String>>,
    /// The rider's wheel, for speed from a wheel counter (Speed and Cadence sensors).
    pub wheel_circumference_mm: u32,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            min_stale_ms: 0,
            repeat_ms: 0,
            lost_after_ms: 0,
            remembered: Vec::new(),
            handled_services: None,
            // a 700x25c road tyre, the usual default in cycling apps
            wheel_circumference_mm: 2105,
        }
    }
}

/// The last event of a running counter (see `RateSpec`), per field of a source.
#[derive(Clone, Copy, Debug, Default)]
struct RateState {
    count: f64,
    time: f64,
    /// Counts per second between the last two events.
    rate: Option<f64>,
    /// When the counter last moved.
    moved_ms: u64,
    seen: bool,
}

pub struct Core {
    bundle: Bundle,
    sources: BTreeMap<String, Source>,
    settings: Settings,
}

impl Core {
    /// Load a compiled bundle (JSON).
    pub fn new(bundle_json: &str) -> Result<Core, String> {
        let bundle: Bundle = serde_json::from_str(bundle_json).map_err(|e| format!("bundle: {e}"))?;
        Core::from_bundle(bundle)
    }

    pub fn from_bundle(bundle: Bundle) -> Result<Core, String> {
        if bundle.version != BUNDLE_VERSION {
            return Err(format!("bundle version {} is not supported (this core reads {})", bundle.version, BUNDLE_VERSION));
        }
        for t in &bundle.tables {
            match t.reader.as_str() {
                "broadcast" => {
                    for m in &t.matches {
                        if m.manufacturer_id.is_none() && m.name_prefix.is_none() {
                            return Err(format!("table {}: a match rule without a company id must name the device", t.id));
                        }
                    }
                }
                "notify" => {
                    if t.notify.is_none() {
                        return Err(format!("table {}: a notify table says which characteristic it reads", t.id));
                    }
                    for m in &t.matches {
                        if m.manufacturer_id.is_none() && m.name_prefix.is_none() && m.service.is_none() {
                            return Err(format!("table {}: a match rule needs a name, a company id or an advertised service", t.id));
                        }
                    }
                }
                other => return Err(format!("table {}: reader '{other}' is not in this core", t.id)),
            }
            if t.matches.is_empty() {
                return Err(format!("table {}: no match rule", t.id));
            }
            let exprs = t
                .only_when
                .iter()
                .chain(t.instance.iter().map(|i| &i.value))
                .chain(t.matches.iter().filter_map(|m| m.when.as_ref()))
                .chain(t.fields.iter().map(|f| &f.value))
                .chain(t.fields.iter().filter_map(|f| f.only_when.as_ref()));
            for e in exprs {
                if !e.within_limits() {
                    return Err(format!("table {}: an expression is too large", t.id));
                }
            }
            for f in &t.fields {
                if bundle.field(&f.field).is_none() {
                    return Err(format!("table {}: unknown field '{}'", t.id, f.field));
                }
            }
            check_layout_and_rates(t)?;
        }
        Ok(Core { bundle, sources: BTreeMap::new(), settings: Settings::default() })
    }

    /// A host whose radio misses many advertisements can wait longer than a table says before a
    /// source counts as silent. The longer of the two times is used.
    pub fn set_min_stale_ms(&mut self, ms: u64) {
        self.settings.min_stale_ms = ms;
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    /// The rider chose this source. Its values are from now on kept alive between packets
    /// (`repeat_ms`) and its silence leads to `source_lost`. Nothing is sent to the device.
    pub fn use_source(&mut self, source_id: &str, now_ms: u64) -> Result<Vec<Message>, String> {
        let s = self.sources.get_mut(source_id).ok_or_else(|| format!("no source '{source_id}' has been heard"))?;
        let mut out = Vec::new();
        // The rider's choice is remembered: after being lost, the source is used again when heard.
        if !self.settings.remembered.iter().any(|r| r == source_id) {
            self.settings.remembered.push(source_id.to_string());
        }
        if !s.in_use {
            s.in_use = true;
            out.push(Message::SourceInUse { source_id: source_id.to_string(), auto: false });
            if let Some(g) = &mut s.gatt {
                if !g.connected {
                    g.asked_ms = Some(now_ms);
                    out.push(Message::Connect { source_id: source_id.to_string(), device_id: g.device_id.clone(), subscribe: subscriptions(&self.bundle.tables[s.table]), start: start_of(&self.bundle.tables[s.table]) });
                }
            }
            if !s.stale && !s.values.is_empty() {
                s.emitted_ms = now_ms;
                out.push(Message::Values { source_id: source_id.to_string(), t: now_ms, fields: s.values.clone(), out_of_range: Vec::new(), stale: false, repeat: true, rssi: s.rssi });
            }
        }
        Ok(out)
    }

    /// The rider stopped using it. The host forgets it too (takes it out of `remembered`).
    pub fn release_source(&mut self, source_id: &str) -> Vec<Message> {
        let mut out = Vec::new();
        if let Some(s) = self.sources.get_mut(source_id) {
            if s.in_use {
                if let Some(g) = &s.gatt {
                    out.push(Message::Disconnect { source_id: source_id.to_string(), device_id: g.device_id.clone() });
                }
            }
            s.in_use = false;
        }
        self.settings.remembered.retain(|r| r != source_id);
        out
    }

    /// The host connected the device and subscribed, as a `connect` message asked.
    pub fn on_connected(&mut self, device_id: &str, _now_ms: u64) {
        for s in self.sources.values_mut() {
            if let Some(g) = &mut s.gatt {
                if g.device_id == device_id {
                    if !g.connected {
                        // polls start right after connecting (the start frames are the host's, with Connect)
                        g.poll_index = 0;
                        g.poll_due_ms = _now_ms;
                    }
                    g.connected = true;
                }
            }
        }
    }

    /// The connection to the device ended (or could not be made). Its sources read zero at once.
    pub fn on_disconnected(&mut self, device_id: &str, now_ms: u64) -> Vec<Message> {
        let mut out = Vec::new();
        for (id, s) in self.sources.iter_mut() {
            let Some(g) = &mut s.gatt else { continue };
            if g.device_id != device_id {
                continue;
            }
            g.connected = false;
            if !s.stale {
                s.stale = true;
                let fields = zeros(&self.bundle, &self.bundle.tables[s.table]);
                for (k, v) in &fields {
                    s.values.insert(k.clone(), v.clone());
                }
                s.emitted_ms = now_ms;
                out.push(Message::Values { source_id: id.clone(), t: now_ms, fields, out_of_range: Vec::new(), stale: true, repeat: false, rssi: None });
                out.push(Message::SourceStale { source_id: id.clone() });
            }
        }
        out
    }

    /// One notification from a characteristic the core asked the host to subscribe to.
    pub fn on_notification(&mut self, device_id: &str, characteristic: &str, data: &[u8], now_ms: u64) -> Vec<Message> {
        let mut out = Vec::new();
        let chr = normalize_uuid(characteristic);
        for (id, s) in self.sources.iter_mut() {
            let Some(g) = &mut s.gatt else { continue };
            if g.device_id != device_id {
                continue;
            }
            let table = &self.bundle.tables[s.table];
            let Some(spec) = &table.notify else { continue };
            if normalize_uuid(&spec.characteristic) != chr || data.len() < spec.min_length {
                continue;
            }
            g.connected = true; // data is the proof
            if let Some(cond) = &table.only_when {
                if cond.eval(data).unwrap_or(0.0) == 0.0 {
                    continue;
                }
            }
            let (fields, out_of_range) = decode(&self.bundle, table, data, &mut s.rates, now_ms, &self.settings);
            s.last_ms = now_ms;
            s.stale = false;
            for (k, v) in &fields {
                s.values.insert(k.clone(), v.clone());
            }
            for k in &out_of_range {
                s.values.remove(k);
            }
            s.emitted_ms = now_ms;
            out.push(Message::Values { source_id: id.clone(), t: now_ms, fields, out_of_range, stale: false, repeat: false, rssi: None });
        }
        out
    }

    pub fn is_in_use(&self, source_id: &str) -> bool {
        self.sources.get(source_id).is_some_and(|s| s.in_use)
    }

    /// The sources heard within `heard_within_ms` (and every source in use): what a device list
    /// that just opened should show. Each as its `source_appeared` message.
    pub fn sources(&self, now_ms: u64, heard_within_ms: u64) -> Vec<Message> {
        self.sources.values().filter(|s| s.in_use || now_ms.saturating_sub(s.last_ms) <= heard_within_ms).map(|s| s.description.clone()).collect()
    }

    pub fn bundle(&self) -> &Bundle {
        &self.bundle
    }

    /// Pass in one advertisement. Returns nothing when no table knows the device.
    pub fn on_advertisement(&mut self, adv: &Advertisement) -> Vec<Message> {
        let mut out = Vec::new();
        self.broadcast(adv, &mut out);
        self.connectable(adv, &mut out);
        out
    }

    /// Devices that are connected to: the advertisement only says the device is there.
    fn connectable(&mut self, adv: &Advertisement, out: &mut Vec<Message>) {
        for (ti, table) in self.bundle.tables.iter().enumerate() {
            if table.reader != "notify" || !table.matches.iter().any(|m| match_connectable(m, adv)) {
                continue;
            }
            // the app reads this service its own way
            if let (Some(handled), Some(n)) = (&self.settings.handled_services, &table.notify) {
                let service = normalize_uuid(&n.service);
                if handled.iter().any(|h| normalize_uuid(h) == service) {
                    continue;
                }
            }
            let source_id = format!("{}:{}", table.id, adv.device_id);
            if !self.sources.contains_key(&source_id) {
                if self.sources.len() >= MAX_SOURCES {
                    return;
                }
                let mut description = describe(&self.bundle, table, &source_id, None);
                if let Message::SourceAppeared { name, connectable, .. } = &mut description {
                    *connectable = true;
                    // the device's own name tells two of the same kind apart
                    if let Some(n) = adv.name.map(str::trim).filter(|n| !n.is_empty() && !name.contains(n)) {
                        *name = format!("{name} · {n}");
                    }
                }
                out.push(description.clone());
                self.sources.insert(
                    source_id.clone(),
                    Source {
                        table: ti,
                        last_ms: adv.now_ms,
                        // nothing has been read yet: there is nothing to declare silent
                        stale: true,
                        in_use: false,
                        description,
                        values: BTreeMap::new(),
                        emitted_ms: adv.now_ms,
                        rssi: adv.rssi,
                        gatt: Some(Gatt { device_id: adv.device_id.to_string(), connected: false, asked_ms: None, poll_index: 0, poll_due_ms: 0 }),
                        rates: vec![RateState::default(); table.fields.len()],
                    },
                );
            }
            let remembered = self.settings.remembered.iter().any(|r| *r == source_id);
            let Some(s) = self.sources.get_mut(&source_id) else { continue };
            if adv.rssi.is_some() {
                s.rssi = adv.rssi;
            }
            if remembered && !s.in_use {
                s.in_use = true;
                // being heard again counts as a fresh start for the "lost" clock
                s.last_ms = adv.now_ms;
                out.push(Message::SourceInUse { source_id: source_id.clone(), auto: true });
            }
            let Some(g) = &mut s.gatt else { continue };
            if s.in_use && !g.connected && g.asked_ms.is_none_or(|t| adv.now_ms.saturating_sub(t) >= CONNECT_RETRY_MS) {
                g.asked_ms = Some(adv.now_ms);
                out.push(Message::Connect { source_id: source_id.clone(), device_id: g.device_id.clone(), subscribe: subscriptions(table), start: start_of(table) });
            }
        }
    }

    /// Broadcast-only devices: the values are in the advertisement itself.
    fn broadcast(&mut self, adv: &Advertisement, out: &mut Vec<Message>) {
        for (ti, table) in self.bundle.tables.iter().enumerate() {
            if table.reader != "broadcast" {
                continue;
            }
            let Some(payload) = table.matches.iter().find_map(|m| match_rule(m, adv)) else { continue };
            if let Some(cond) = &table.only_when {
                if cond.eval(payload).unwrap_or(0.0) == 0.0 {
                    return; // this device, but not a packet we read
                }
            }
            let instance = match &table.instance {
                Some(i) => match i.value.eval(payload) {
                    Some(v) => Some(v as i64),
                    None => return,
                },
                None => None,
            };
            let source_id = match instance {
                Some(i) => format!("{}:{}", table.id, i),
                None => format!("{}:{}", table.id, adv.device_id),
            };

            match self.sources.get_mut(&source_id) {
                Some(s) => {
                    s.last_ms = adv.now_ms;
                    s.stale = false;
                }
                None => {
                    if self.sources.len() >= MAX_SOURCES {
                        return;
                    }
                    let description = describe(&self.bundle, table, &source_id, instance);
                    out.push(description.clone());
                    self.sources.insert(
                        source_id.clone(),
                        Source {
                            table: ti,
                            last_ms: adv.now_ms,
                            stale: false,
                            in_use: false,
                            description,
                            values: BTreeMap::new(),
                            emitted_ms: adv.now_ms,
                            rssi: adv.rssi,
                            gatt: None,
                            rates: vec![RateState::default(); table.fields.len()],
                        },
                    );
                }
            }
            // A remembered source is used again as soon as it is heard.
            let remembered = self.settings.remembered.iter().any(|r| *r == source_id);
            if let Some(s) = self.sources.get_mut(&source_id) {
                if remembered && !s.in_use {
                    s.in_use = true;
                    out.push(Message::SourceInUse { source_id: source_id.clone(), auto: true });
                }
            }

            let Some(s) = self.sources.get_mut(&source_id) else { return };
            let (fields, out_of_range) = decode(&self.bundle, table, payload, &mut s.rates, adv.now_ms, &self.settings);
            {
                for (k, v) in &fields {
                    s.values.insert(k.clone(), v.clone());
                }
                for k in &out_of_range {
                    s.values.remove(k);
                }
                s.emitted_ms = adv.now_ms;
                if adv.rssi.is_some() {
                    s.rssi = adv.rssi;
                }
            }
            out.push(Message::Values { source_id, t: adv.now_ms, fields, out_of_range, stale: false, repeat: false, rssi: adv.rssi });
            return;
        }
    }


    /// Call about once a second. Silent sources read zero (`source_stale`); sources in use have
    /// their standing values repeated (`repeat_ms`) and are given up after `lost_after_ms`.
    pub fn tick(&mut self, now_ms: u64) -> Vec<Message> {
        let mut out = Vec::new();
        let settings = &self.settings;
        for (id, s) in self.sources.iter_mut() {
            let table = &self.bundle.tables[s.table];
            let silent_for = now_ms.saturating_sub(s.last_ms);
            if !s.stale && silent_for >= table.stale_after_ms.max(settings.min_stale_ms) {
                s.stale = true;
                let fields = zeros(&self.bundle, table);
                for (k, v) in &fields {
                    s.values.insert(k.clone(), v.clone());
                }
                s.emitted_ms = now_ms;
                out.push(Message::Values { source_id: id.clone(), t: now_ms, fields, out_of_range: Vec::new(), stale: true, repeat: false, rssi: None });
                out.push(Message::SourceStale { source_id: id.clone() });
            }
            if !s.in_use {
                continue;
            }
            // devices that answer only when asked: the next fixed frame, when it is due
            if let (Some(poll), Some(g)) = (&table.poll, &mut s.gatt) {
                if g.connected && !poll.frames.is_empty() && now_ms >= g.poll_due_ms {
                    let frame = &poll.frames[g.poll_index % poll.frames.len()];
                    out.push(Message::Write {
                        source_id: id.clone(),
                        device_id: g.device_id.clone(),
                        write: Write { service: display_uuid(&poll.service), characteristic: display_uuid(&poll.characteristic), data: hex(frame), with_response: poll.with_response },
                    });
                    g.poll_index = (g.poll_index + 1) % poll.frames.len();
                    g.poll_due_ms = now_ms + poll.every_ms;
                }
            }
            if s.stale {
                if settings.lost_after_ms > 0 && silent_for >= settings.lost_after_ms {
                    s.in_use = false;
                    out.push(Message::SourceLost { source_id: id.clone() });
                }
            } else if settings.repeat_ms > 0 && now_ms.saturating_sub(s.emitted_ms) >= settings.repeat_ms && !s.values.is_empty() {
                s.emitted_ms = now_ms;
                out.push(Message::Values { source_id: id.clone(), t: now_ms, fields: s.values.clone(), out_of_range: Vec::new(), stale: false, repeat: true, rssi: s.rssi });
            }
        }
        out
    }
}

/// A UUID as a Bluetooth library takes it: `180d`, or lower case with dashes.
fn display_uuid(uuid: &str) -> String {
    let u = normalize_uuid(uuid);
    if u.len() == 32 {
        format!("{}-{}-{}-{}-{}", &u[0..8], &u[8..12], &u[12..16], &u[16..20], &u[20..32])
    } else {
        u
    }
}

/// A table's fields from one payload: the values in range, and the keys of those that were not.
/// A layout and rates can only read what exists, within the same limits as any expression.
fn check_layout_and_rates(t: &Table) -> Result<(), String> {
    let parts = t.layout.as_ref().map_or(0, |l| l.parts.len());
    if parts > MAX_LAYOUT_PARTS {
        return Err(format!("table {}: a layout has at most {MAX_LAYOUT_PARTS} parts", t.id));
    }
    let reads_ok = |e: &Expr, rate_allowed: bool| {
        let (part, rate) = e.uses();
        e.within_limits() && part.is_none_or(|p| p < parts) && (rate_allowed || !rate)
    };
    for (what, w) in [("start", &t.start), ("poll", &t.poll)] {
        let Some(w) = w else { continue };
        if t.reader != "notify" {
            return Err(format!("table {}: {what} goes with reader: notify", t.id));
        }
        let chr = normalize_uuid(&w.characteristic);
        if CONTROL_POINTS.iter().any(|c| normalize_uuid(c) == chr) {
            return Err(format!("table {}: {what} writes to a control point ({chr}): that is control, which OFP does not do", t.id));
        }
        if w.frames.is_empty() || w.frames.len() > MAX_WRITE_FRAMES || w.frames.iter().any(|f| f.is_empty() || f.len() > MAX_FRAME_BYTES) {
            return Err(format!("table {}: {what}: 1 to {MAX_WRITE_FRAMES} frames of 1 to {MAX_FRAME_BYTES} bytes", t.id));
        }
        if what == "poll" && w.every_ms < MIN_POLL_MS {
            return Err(format!("table {}: poll every_ms must be at least {MIN_POLL_MS}", t.id));
        }
    }
    if let Some(l) = &t.layout {
        for p in &l.parts {
            // where a part sits cannot depend on other parts: flags are read at fixed positions
            if p.present_when.as_ref().is_some_and(|c| !c.within_limits() || c.uses() != (None, false)) {
                return Err(format!("table {}: layout part {}: present_when reads fixed positions only", t.id, p.name));
            }
        }
    }
    let plain = t.only_when.iter().chain(t.instance.iter().map(|i| &i.value)).chain(t.matches.iter().filter_map(|m| m.when.as_ref()));
    for e in plain {
        if e.uses() != (None, false) {
            return Err(format!("table {}: match rules, only_when and instance read fixed positions only", t.id));
        }
    }
    for f in &t.fields {
        let has_rate = f.rate.is_some();
        if !reads_ok(&f.value, has_rate) || !f.only_when.as_ref().is_none_or(|w| reads_ok(w, has_rate)) {
            return Err(format!("table {}: field {} reads a part that does not exist, or a rate it does not have", t.id, f.field));
        }
        if let Some(r) = &f.rate {
            if !reads_ok(&r.count, false) || !reads_ok(&r.time, false) || !(r.count_wrap > 0.0 && r.time_wrap > 0.0 && r.time_unit > 0.0) || r.stop_ms > 60_000 {
                return Err(format!("table {}: field {}: the rate's counter, time, wraps (above 0) or stop time (at most 60 s) are not right", t.id, f.field));
            }
        }
    }
    Ok(())
}

/// Where each part of a table's layout sits in this packet (`None`: not in it).
fn place_parts(table: &Table, payload: &[u8]) -> Vec<(PartType, Option<usize>)> {
    let Some(layout) = &table.layout else { return Vec::new() };
    let mut at = layout.start;
    layout
        .parts
        .iter()
        .map(|part| {
            let present = part.present_when.as_ref().is_none_or(|c| c.eval(payload).unwrap_or(0.0) != 0.0);
            if present {
                let here = at;
                at += part.ty.width();
                (part.ty, Some(here))
            } else {
                (part.ty, None)
            }
        })
        .collect()
}

/// How far a counter that starts again from 0 at `wrap` moved forward (no `rem_euclid` without std).
fn forward(d: f64, wrap: f64) -> f64 {
    let r = d % wrap;
    if r < 0.0 {
        r + wrap
    } else {
        r
    }
}

/// The newest rate of a running counter, after this packet. `None` until two events were seen.
fn advance_rate(spec: &RateSpec, st: &mut RateState, payload: &[u8], cx: &Cx, now_ms: u64) -> Option<f64> {
    // this packet has no counter (its flag is off): no value from it
    let (Some(count), Some(time)) = (spec.count.eval_in(payload, cx), spec.time.eval_in(payload, cx)) else { return None };
    if !st.seen {
        *st = RateState { count, time, rate: None, moved_ms: now_ms, seen: true };
        return None;
    }
    let dt = forward(time - st.time, spec.time_wrap) * spec.time_unit;
    if dt > 0.0 {
        let dc = forward(count - st.count, spec.count_wrap);
        st.rate = Some(dc / dt);
        st.count = count;
        st.time = time;
        st.moved_ms = now_ms;
    } else if now_ms.saturating_sub(st.moved_ms) >= spec.stop_ms {
        // the same event again and again: nothing is turning
        st.rate = Some(0.0);
    }
    st.rate
}

fn decode(bundle: &Bundle, table: &Table, payload: &[u8], rates: &mut [RateState], now_ms: u64, settings: &Settings) -> (BTreeMap<String, Value>, Vec<String>) {
    let mut fields = BTreeMap::new();
    let mut out_of_range = Vec::new();
    let parts = place_parts(table, payload);
    let wheel_m = Some(settings.wheel_circumference_mm as f64 / 1000.0);
    for (i, tf) in table.fields.iter().enumerate() {
        let mut cx = Cx { parts: &parts, rate: None, wheel_m };
        if let (Some(spec), Some(st)) = (&tf.rate, rates.get_mut(i)) {
            cx.rate = advance_rate(spec, st, payload, &cx, now_ms);
            if cx.rate.is_none() {
                continue;
            }
        }
        if let Some(cond) = &tf.only_when {
            if cond.eval_in(payload, &cx).unwrap_or(0.0) == 0.0 {
                continue;
            }
        }
        let (Some(v), Some(def)) = (tf.value.eval_in(payload, &cx), bundle.field(&tf.field)) else { continue };
        let v = round_to(v, def.decimals);
        if v.is_nan() || v < def.min || v > def.max {
            out_of_range.push(tf.field.clone());
        } else {
            fields.insert(tf.field.clone(), to_json(v, def));
        }
    }
    (fields, out_of_range)
}

/// Zero for every instantaneous field of a table: what a silent source reads.
fn zeros(bundle: &Bundle, table: &Table) -> BTreeMap<String, Value> {
    let mut fields = BTreeMap::new();
    for tf in &table.fields {
        if let Some(def) = bundle.field(&tf.field) {
            if def.kind == SampleKind::Instantaneous {
                fields.insert(tf.field.clone(), to_json(0.0, def));
            }
        }
    }
    fields
}

fn subscriptions(table: &Table) -> Vec<Subscription> {
    table.notify.iter().map(|n| Subscription { service: display_uuid(&n.service), characteristic: display_uuid(&n.characteristic) }).collect()
}

/// Does this advertisement come from a device a `notify` table reads? Every criterion the rule
/// gives must hold.
fn match_connectable(m: &MatchRule, adv: &Advertisement) -> bool {
    if let Some(prefix) = &m.name_prefix {
        let Some(name) = adv.name.map(str::trim) else { return false };
        if name.len() < prefix.len() || !name.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes()) {
            return false;
        }
    }
    if let Some(id) = m.manufacturer_id {
        let d = adv.manufacturer_data;
        if d.len() < 2 || u16::from_le_bytes([d[0], d[1]]) != id {
            return false;
        }
    }
    if let Some(service) = &m.service {
        let want = normalize_uuid(service);
        if !adv.services.iter().any(|s| normalize_uuid(s) == want) {
            return false;
        }
    }
    true
}

/// The payload a rule reads from this advertisement, or `None` when the rule does not match.
fn match_rule<'a>(m: &MatchRule, adv: &Advertisement<'a>) -> Option<&'a [u8]> {
    if let Some(prefix) = &m.name_prefix {
        let name = adv.name?.trim();
        if name.len() < prefix.len() || !name.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes()) {
            return None;
        }
    }
    let data = adv.manufacturer_data;
    let payload = match m.manufacturer_id {
        Some(id) => {
            if data.len() < 2 || u16::from_le_bytes([data[0], data[1]]) != id {
                return None;
            }
            &data[2..]
        }
        None => data,
    };
    if payload.len() < m.min_length {
        return None;
    }
    if let Some(cond) = &m.when {
        if cond.eval(payload).unwrap_or(0.0) == 0.0 {
            return None;
        }
    }
    Some(payload)
}

fn describe(bundle: &Bundle, table: &Table, source_id: &str, instance: Option<i64>) -> Message {
    let label = table.instance.as_ref().map(|i| i.label.clone());
    let name = match (&label, instance) {
        (Some(l), Some(i)) => format!("{} · {} {}", table.name, l, i),
        _ => table.name.clone(),
    };
    let fields = table
        .fields
        .iter()
        .filter_map(|tf| bundle.field(&tf.field))
        .map(|d: &FieldDef| FieldDescription {
            key: d.key.clone(),
            id: d.id,
            name: d.name.clone(),
            unit: d.unit.clone(),
            ty: d.ty,
            decimals: d.decimals,
            min: d.min,
            max: d.max,
            kind: d.kind,
            tier: d.tier,
        })
        .collect();
    Message::SourceAppeared {
        source_id: source_id.to_string(),
        table: table.id.clone(),
        table_version: table.version,
        name,
        instance_label: label,
        instance,
        fields,
        connectable: false,
    }
}

fn round_to(v: f64, decimals: u8) -> f64 {
    let m = [1.0, 10.0, 100.0, 1000.0, 10000.0][(decimals as usize).min(4)];
    let scaled = v * m;
    if !(scaled.abs() < 9.0e15) {
        return f64::NAN;
    }
    let r = if scaled >= 0.0 { (scaled + 0.5) as i64 } else { (scaled - 0.5) as i64 };
    r as f64 / m
}

fn to_json(v: f64, def: &FieldDef) -> Value {
    if def.ty == FieldType::Int || def.decimals == 0 {
        Value::from(v as i64)
    } else {
        serde_json::Number::from_f64(v).map(Value::Number).unwrap_or(Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{BinOp, Expr};
    use crate::bundle::{Instance, TableField};
    use alloc::boxed::Box;
    use alloc::vec;

    fn field(id: u32, key: &str, kind: SampleKind, max: f64) -> FieldDef {
        FieldDef { id, key: key.into(), name: key.into(), unit: "".into(), ty: FieldType::Int, decimals: 0, min: 0.0, max, kind, tier: Tier::Experimental }
    }

    fn bundle() -> Bundle {
        Bundle {
            version: BUNDLE_VERSION,
            fields: vec![field(1, "power", SampleKind::Instantaneous, 3000.0), field(2, "energy", SampleKind::Cumulative, 20000.0)],
            tables: vec![Table {
                id: "bike".into(),
                name: "Bike".into(),
                reader: "broadcast".into(),
                version: 1,
                matches: vec![MatchRule { manufacturer_id: Some(0x0102), name_prefix: None, service: None, min_length: 4, when: None }],
                notify: None,
                only_when: Some(Expr::Bin(BinOp::Eq, Box::new(Expr::U8(0)), Box::new(Expr::Num(0.0)))),
                instance: Some(Instance { label: "bike".into(), value: Expr::U8(1) }),
                stale_after_ms: 3000,
                layout: None,
            start: None,
            poll: None,
                fields: vec![
                    TableField { field: "power".into(), value: Expr::U16le(2), only_when: None, rate: None },
                    TableField { field: "energy".into(), value: Expr::U8(1), only_when: None, rate: None },
                ],
            }],
        }
    }

    fn adv(data: &[u8], now_ms: u64) -> Advertisement<'_> {
        Advertisement { device_id: "d", name: None, rssi: Some(-50), manufacturer_data: data, services: &[], now_ms }
    }

    fn values(m: &Message) -> (&str, &BTreeMap<String, Value>, bool) {
        match m {
            Message::Values { source_id, fields, stale, .. } => (source_id, fields, *stale),
            other => panic!("not values: {other:?}"),
        }
    }

    #[test]
    fn appears_once_then_values() {
        let mut core = Core::from_bundle(bundle()).unwrap();
        let first = core.on_advertisement(&adv(&[2, 1, 0, 7, 187, 0], 0));
        assert_eq!(first.len(), 2);
        assert!(matches!(&first[0], Message::SourceAppeared { source_id, name, instance: Some(7), .. } if source_id == "bike:7" && name == "Bike · bike 7"));
        let (id, fields, stale) = values(&first[1]);
        assert_eq!((id, stale), ("bike:7", false));
        assert_eq!(fields["power"], Value::from(187));
        let again = core.on_advertisement(&adv(&[2, 1, 0, 7, 200, 0], 300));
        assert_eq!(again.len(), 1);
    }

    #[test]
    fn two_bikes_are_two_sources() {
        let mut core = Core::from_bundle(bundle()).unwrap();
        core.on_advertisement(&adv(&[2, 1, 0, 7, 187, 0], 0));
        let other = core.on_advertisement(&adv(&[2, 1, 0, 8, 100, 0], 10));
        assert!(matches!(&other[0], Message::SourceAppeared { source_id, .. } if source_id == "bike:8"));
    }

    #[test]
    fn ignores_other_makers_short_packets_and_filtered_packets() {
        let mut core = Core::from_bundle(bundle()).unwrap();
        assert!(core.on_advertisement(&adv(&[0x4c, 0, 0, 7, 187, 0], 0)).is_empty());
        assert!(core.on_advertisement(&adv(&[2, 1, 0, 7], 0)).is_empty());
        assert!(core.on_advertisement(&adv(&[2, 1, 0xff, 7, 187, 0], 0)).is_empty());
        assert!(core.on_advertisement(&adv(&[], 0)).is_empty());
    }

    #[test]
    fn out_of_range_values_are_not_passed_on() {
        let mut core = Core::from_bundle(bundle()).unwrap();
        let m = core.on_advertisement(&adv(&[2, 1, 0, 7, 0xff, 0xff], 0));
        match &m[1] {
            Message::Values { fields, out_of_range, .. } => {
                assert!(!fields.contains_key("power"));
                assert_eq!(out_of_range, &vec![String::from("power")]);
                assert_eq!(fields["energy"], Value::from(7));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn silence_zeroes_instantaneous_values_once_and_resumes() {
        let mut core = Core::from_bundle(bundle()).unwrap();
        core.on_advertisement(&adv(&[2, 1, 0, 7, 187, 0], 0));
        assert!(core.tick(2999).is_empty());
        let m = core.tick(3000);
        assert_eq!(m.len(), 2);
        let (_, fields, stale) = values(&m[0]);
        assert!(stale);
        assert_eq!(fields["power"], Value::from(0));
        assert!(!fields.contains_key("energy"), "a running total keeps its last value");
        assert!(matches!(&m[1], Message::SourceStale { source_id } if source_id == "bike:7"));
        assert!(core.tick(4000).is_empty());
        let back = core.on_advertisement(&adv(&[2, 1, 0, 7, 150, 0], 9000));
        assert_eq!(back.len(), 1, "no second source_appeared");
        assert!(core.tick(9500).is_empty());
    }

    #[test]
    fn a_host_can_wait_longer() {
        let mut core = Core::from_bundle(bundle()).unwrap();
        core.set_min_stale_ms(8000);
        core.on_advertisement(&adv(&[2, 1, 0, 7, 187, 0], 0));
        assert!(core.tick(7999).is_empty());
        assert_eq!(core.tick(8000).len(), 2);
    }

    fn kinds(m: &[Message]) -> Vec<&'static str> {
        m.iter()
            .map(|m| match m {
                Message::SourceAppeared { .. } => "appeared",
                Message::Values { stale: true, .. } => "zeros",
                Message::Values { .. } => "values",
                Message::SourceStale { .. } => "stale",
                Message::SourceInUse { auto: true, .. } => "in_use_auto",
                Message::SourceInUse { .. } => "in_use",
                Message::SourceLost { .. } => "lost",
                Message::Connect { .. } => "connect",
                Message::Disconnect { .. } => "disconnect",
                Message::Write { .. } => "write",
            })
            .collect()
    }

    #[test]
    fn a_source_in_use_repeats_its_values_and_is_lost_after_long_silence() {
        let mut core = Core::from_bundle(bundle()).unwrap();
        *core.settings_mut() = Settings { min_stale_ms: 10_000, repeat_ms: 1000, lost_after_ms: 60_000, ..Default::default() };
        assert!(core.use_source("bike:7", 0).is_err(), "not heard yet");
        core.on_advertisement(&adv(&[2, 1, 0, 7, 187, 0], 0));
        // heard but not in use: no repeats
        assert!(core.tick(1500).is_empty());
        assert_eq!(kinds(&core.use_source("bike:7", 2000).unwrap()), ["in_use", "values"]);
        assert!(core.use_source("bike:7", 2100).unwrap().is_empty(), "already in use");
        assert!(core.is_in_use("bike:7"));
        // the radio misses packets for a while: the standing values go out every second
        assert!(core.tick(2500).is_empty());
        let repeat = core.tick(3000);
        assert_eq!(kinds(&repeat), ["values"]);
        assert_eq!(values(&repeat[0]).1["power"], Value::from(187));
        assert_eq!(kinds(&core.tick(4000)), ["values"]);
        // a real packet resets the clock
        core.on_advertisement(&adv(&[2, 1, 0, 7, 150, 0], 4500));
        assert!(core.tick(5000).is_empty());
        // silence: zeros once at the stale time, no repeats after, lost at a minute
        assert_eq!(kinds(&core.tick(14_500)), ["zeros", "stale"]);
        assert!(core.tick(15_500).is_empty());
        assert_eq!(kinds(&core.tick(64_500)), ["lost"]);
        assert!(!core.is_in_use("bike:7"));
        assert!(core.tick(65_500).is_empty());
    }

    #[test]
    fn a_remembered_source_is_used_as_soon_as_it_is_heard_and_returns_after_being_lost() {
        let mut core = Core::from_bundle(bundle()).unwrap();
        *core.settings_mut() = Settings { lost_after_ms: 60_000, remembered: vec!["bike:7".into()], ..Default::default() };
        assert_eq!(kinds(&core.on_advertisement(&adv(&[2, 1, 0, 7, 187, 0], 0))), ["appeared", "in_use_auto", "values"]);
        assert_eq!(kinds(&core.on_advertisement(&adv(&[2, 1, 0, 8, 100, 0], 10))), ["appeared", "values"], "another bike is not ours");
        assert_eq!(kinds(&core.tick(3100)), ["zeros", "stale", "zeros", "stale"]);
        assert_eq!(kinds(&core.tick(60_000)), ["lost"]);
        assert_eq!(kinds(&core.on_advertisement(&adv(&[2, 1, 0, 7, 120, 0], 90_000))), ["in_use_auto", "values"]);
        // the rider disconnects it: forgotten, and not used again by itself
        core.release_source("bike:7");
        assert!(!core.is_in_use("bike:7"));
        assert_eq!(kinds(&core.on_advertisement(&adv(&[2, 1, 0, 7, 120, 0], 91_000))), ["values"]);
    }

    #[test]
    fn the_device_list_gets_what_is_in_range() {
        let mut core = Core::from_bundle(bundle()).unwrap();
        core.on_advertisement(&adv(&[2, 1, 0, 7, 187, 0], 0));
        core.on_advertisement(&adv(&[2, 1, 0, 8, 100, 0], 40_000));
        core.use_source("bike:7", 100).unwrap();
        // bike 7 was heard long ago but is in use; bike 8 was just heard
        assert_eq!(core.sources(45_000, 30_000).len(), 2);
        core.release_source("bike:7");
        let recent = core.sources(45_000, 30_000);
        assert!(matches!(&recent[..], [Message::SourceAppeared { source_id, .. }] if source_id == "bike:8"));
    }

    /// A sensor that is connected to: its custom service notifies temperature x 100.
    fn sensor_bundle() -> Bundle {
        let mut b = bundle();
        b.fields.push(FieldDef { id: 3, key: "temp".into(), name: "Temperature".into(), unit: "C".into(), ty: FieldType::Decimal, decimals: 2, min: 20.0, max: 45.0, kind: SampleKind::Instantaneous, tier: Tier::Experimental });
        b.tables.push(Table {
            id: "sensor".into(),
            name: "Sensor".into(),
            reader: "notify".into(),
            version: 1,
            matches: vec![MatchRule { manufacturer_id: None, name_prefix: Some("CORE".into()), service: Some("00002100-5B1E-4347-B07C-97B514DAE121".into()), min_length: 0, when: None }],
            notify: Some(crate::bundle::NotifySpec { service: "00002100-5b1e-4347-b07c-97b514dae121".into(), characteristic: "00002101-5B1E-4347-B07C-97B514DAE121".into(), min_length: 3 }),
            only_when: Some(Expr::Bin(BinOp::Ne, Box::new(Expr::I16le(1)), Box::new(Expr::Num(32767.0)))),
            instance: None,
            stale_after_ms: 5000,
            layout: None,
            start: None,
            poll: None,
            fields: vec![TableField { field: "temp".into(), value: Expr::Bin(BinOp::Div, Box::new(Expr::I16le(1)), Box::new(Expr::Num(100.0))), only_when: None, rate: None }],
        });
        b
    }

    fn sensor_adv<'a>(services: &'a [String], name: &'a str, now_ms: u64) -> Advertisement<'a> {
        Advertisement { device_id: "dev9", name: Some(name), rssi: Some(-60), manufacturer_data: &[], services, now_ms }
    }

    const CHR: &str = "00002101-5b1e-4347-b07c-97b514dae121";

    #[test]
    fn a_connectable_sensor_from_advert_to_values() {
        let mut core = Core::from_bundle(sensor_bundle()).unwrap();
        let svc = vec![String::from("00002100-5b1e-4347-b07c-97b514dae121")];
        // the name alone, or the service alone, is not this rule
        assert!(core.on_advertisement(&sensor_adv(&[], "CORE 1234", 0)).is_empty());
        assert!(core.on_advertisement(&sensor_adv(&svc, "Other", 0)).is_empty());
        let m = core.on_advertisement(&sensor_adv(&svc, "CORE 1234", 0));
        assert!(matches!(&m[..], [Message::SourceAppeared { source_id, name, connectable: true, .. }] if source_id == "sensor:dev9" && name == "Sensor · CORE 1234"));
        // heard, not picked: no connection is asked for, and nothing goes "silent"
        assert!(core.on_advertisement(&sensor_adv(&svc, "CORE 1234", 1000)).is_empty());
        assert!(core.tick(20_000).is_empty());
        // the rider picks it: the host is asked to connect and subscribe
        let m = core.use_source("sensor:dev9", 21_000).unwrap();
        assert_eq!(kinds(&m), ["in_use", "connect"]);
        assert!(matches!(&m[1], Message::Connect { device_id, subscribe, .. } if device_id == "dev9" && subscribe[0].characteristic == CHR));
        core.on_connected("dev9", 21_500);
        // 37.25 C, then "no reading" (0x7FFF), then a short packet, then another characteristic
        let m = core.on_notification("dev9", "00002101-5B1E-4347-B07C-97B514DAE121", &[0, 0x8d, 0x0e], 22_000);
        assert_eq!(values(&m[0]).1["temp"], serde_json::json!(37.25));
        assert!(core.on_notification("dev9", CHR, &[0, 0xff, 0x7f], 22_500).is_empty());
        assert!(core.on_notification("dev9", CHR, &[0, 0x8d], 22_600).is_empty());
        assert!(core.on_notification("dev9", "2a19", &[80], 22_700).is_empty());
        assert!(core.on_notification("other", CHR, &[0, 0x8d, 0x0e], 22_800).is_empty());
        // out of the plausible range: not passed on
        assert!(matches!(&core.on_notification("dev9", CHR, &[0, 0x10, 0x27], 23_000)[0], Message::Values { out_of_range, .. } if out_of_range == &vec![String::from("temp")]));
    }

    /// A bike that sends only when asked: one start frame after subscribing, two poll frames in turn.
    fn polled_bundle(poll_every: u64, chr: &str) -> Bundle {
        let mut b = sensor_bundle();
        let t = b.tables.last_mut().unwrap();
        t.start = Some(crate::bundle::Writes { service: "fff0".into(), characteristic: chr.into(), frames: vec![vec![0xf0, 0xa0, 0x01]], with_response: false, every_ms: 0 });
        t.poll = Some(crate::bundle::Writes { service: "fff0".into(), characteristic: chr.into(), frames: vec![vec![0xf0, 0xa1], vec![0xf0, 0xa2]], with_response: false, every_ms: poll_every });
        b
    }

    #[test]
    fn start_frames_come_with_connect_and_polls_only_while_connected_and_in_use() {
        let mut core = Core::from_bundle(polled_bundle(500, "fff2")).unwrap();
        let svc = vec![String::from("00002100-5b1e-4347-b07c-97b514dae121")];
        core.on_advertisement(&sensor_adv(&svc, "CORE", 0));
        // not in use: nothing is written
        assert!(core.tick(1000).is_empty());
        let m = core.use_source("sensor:dev9", 1000).unwrap();
        assert!(matches!(&m[1], Message::Connect { start, .. } if start.len() == 1 && start[0].data == "f0a001" && start[0].characteristic == "fff2"));
        // asked to connect, not connected yet: no polls
        assert!(!kinds(&core.tick(1200)).contains(&"write"));
        core.on_connected("dev9", 1500);
        let polls = |m: Vec<Message>| m.into_iter().filter_map(|m| match m { Message::Write { write, .. } => Some(write.data), _ => None }).collect::<Vec<_>>();
        assert_eq!(polls(core.tick(1500)), ["f0a1"]);
        assert!(polls(core.tick(1800)).is_empty(), "every 500 ms");
        assert_eq!(polls(core.tick(2000)), ["f0a2"]);
        assert_eq!(polls(core.tick(2500)), ["f0a1"], "in turn");
        core.release_source("sensor:dev9");
        assert!(polls(core.tick(3500)).is_empty(), "released: no more");
    }

    #[test]
    fn a_table_cannot_write_to_a_control_point_or_too_much_or_too_often() {
        let msg = |b: Bundle| Core::from_bundle(b).err().unwrap_or_default();
        assert!(msg(polled_bundle(500, "2AD9")).contains("control point"));
        assert!(msg(polled_bundle(500, "a026e005-0a7d-4ab3-97fa-f1500f9feb8b")).contains("control point"));
        assert!(msg(polled_bundle(100, "fff2")).contains("at least 200"));
        let mut b = polled_bundle(500, "fff2");
        b.tables.last_mut().unwrap().start.as_mut().unwrap().frames = vec![vec![1]; 17];
        assert!(msg(b).contains("frames"));
        let mut b = polled_bundle(500, "fff2");
        b.tables.last_mut().unwrap().poll.as_mut().unwrap().frames = vec![vec![0; 65]];
        assert!(msg(b).contains("frames"));
    }

    #[test]
    fn a_dropped_connection_reads_zero_and_is_asked_for_again_when_the_device_is_heard() {
        let mut core = Core::from_bundle(sensor_bundle()).unwrap();
        *core.settings_mut() = Settings { lost_after_ms: 60_000, ..Default::default() };
        let svc = vec![String::from("00002100-5b1e-4347-b07c-97b514dae121")];
        core.on_advertisement(&sensor_adv(&svc, "CORE", 0));
        core.use_source("sensor:dev9", 100).unwrap();
        core.on_connected("dev9", 200);
        core.on_notification("dev9", CHR, &[0, 0x8d, 0x0e], 1000);
        assert_eq!(kinds(&core.on_disconnected("dev9", 2000)), ["zeros", "stale"]);
        assert!(core.on_disconnected("dev9", 2100).is_empty(), "once");
        // adverts return: connect again, but not on every advert
        assert_eq!(kinds(&core.on_advertisement(&sensor_adv(&svc, "CORE", 6000))), ["connect"]);
        assert!(core.on_advertisement(&sensor_adv(&svc, "CORE", 7000)).is_empty());
        assert_eq!(kinds(&core.on_advertisement(&sensor_adv(&svc, "CORE", 11_500))), ["connect"]);
        // it never comes back: lost, and the host is told to let go when the rider releases it
        assert_eq!(kinds(&core.tick(61_500)), ["lost"]);
        core.use_source("sensor:dev9", 62_000).unwrap();
        assert_eq!(kinds(&core.release_source("sensor:dev9")), ["disconnect"]);
        assert!(core.release_source("sensor:dev9").is_empty());
    }

    #[test]
    fn a_remembered_sensor_is_connected_as_soon_as_it_is_heard() {
        let mut core = Core::from_bundle(sensor_bundle()).unwrap();
        *core.settings_mut() = Settings { remembered: vec!["sensor:dev9".into()], ..Default::default() };
        let svc = vec![String::from("00002100-5b1e-4347-b07c-97b514dae121")];
        assert_eq!(kinds(&core.on_advertisement(&sensor_adv(&svc, "CORE", 0))), ["appeared", "in_use_auto", "connect"]);
    }

    #[test]
    fn uuids_compare_in_any_form() {
        assert_eq!(normalize_uuid("0000180D-0000-1000-8000-00805F9B34FB"), "180d");
        assert_eq!(normalize_uuid("180D"), "180d");
        assert_eq!(normalize_uuid("00002101-5B1E-4347-B07C-97B514DAE121"), "000021015b1e4347b07c97b514dae121");
    }

    #[test]
    fn refuses_bad_bundles() {
        let mut b = bundle();
        b.version = 99;
        assert!(Core::from_bundle(b).is_err());
        let mut b = bundle();
        b.tables[0].reader = "notify".into();
        assert!(Core::from_bundle(b).is_err());
        let mut b = bundle();
        b.tables[0].fields[0].field = "nope".into();
        assert!(Core::from_bundle(b).is_err());
        assert!(Core::new("not json").is_err());
    }

    #[test]
    fn rounding() {
        assert_eq!(round_to(5.1000000001, 1), 5.1);
        assert_eq!(round_to(1609.344, 0), 1609.0);
        assert_eq!(round_to(84.25, 1), 84.3);
        assert!(round_to(f64::INFINITY, 0).is_nan());
    }
}
