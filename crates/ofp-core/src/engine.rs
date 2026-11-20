//! The broadcast reader and the table engine.

use crate::bundle::{Bundle, FieldDef, FieldType, MatchRule, SampleKind, Table, Tier};
use crate::BUNDLE_VERSION;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
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
    pub now_ms: u64,
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
    },
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
}

/// What the host sets once, at the start.
#[derive(Clone, Debug, Default)]
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
    /// Bluetooth services the app reads by itself. `None` = the app did not say.
    pub handled_services: Option<Vec<String>>,
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
            if t.reader != "broadcast" {
                return Err(format!("table {}: reader '{}' is not in this core", t.id, t.reader));
            }
            if t.matches.is_empty() {
                return Err(format!("table {}: no match rule", t.id));
            }
            for m in &t.matches {
                if m.manufacturer_id.is_none() && m.name_prefix.is_none() {
                    return Err(format!("table {}: a match rule without a company id must name the device", t.id));
                }
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
            if !s.stale && !s.values.is_empty() {
                s.emitted_ms = now_ms;
                out.push(Message::Values { source_id: source_id.to_string(), t: now_ms, fields: s.values.clone(), out_of_range: Vec::new(), stale: false, repeat: true, rssi: s.rssi });
            }
        }
        Ok(out)
    }

    /// The rider stopped using it. The host forgets it too (takes it out of `remembered`).
    pub fn release_source(&mut self, source_id: &str) {
        if let Some(s) = self.sources.get_mut(source_id) {
            s.in_use = false;
        }
        self.settings.remembered.retain(|r| r != source_id);
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
        for (ti, table) in self.bundle.tables.iter().enumerate() {
            let Some(payload) = table.matches.iter().find_map(|m| match_rule(m, adv)) else { continue };
            if let Some(cond) = &table.only_when {
                if cond.eval(payload).unwrap_or(0.0) == 0.0 {
                    return out; // this device, but not a packet we read
                }
            }
            let instance = match &table.instance {
                Some(i) => match i.value.eval(payload) {
                    Some(v) => Some(v as i64),
                    None => return out,
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
                        return out;
                    }
                    let description = describe(&self.bundle, table, &source_id, instance);
                    out.push(description.clone());
                    self.sources.insert(
                        source_id.clone(),
                        Source { table: ti, last_ms: adv.now_ms, stale: false, in_use: false, description, values: BTreeMap::new(), emitted_ms: adv.now_ms, rssi: adv.rssi },
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

            let mut fields = BTreeMap::new();
            let mut out_of_range = Vec::new();
            for tf in &table.fields {
                if let Some(cond) = &tf.only_when {
                    if cond.eval(payload).unwrap_or(0.0) == 0.0 {
                        continue;
                    }
                }
                let (Some(v), Some(def)) = (tf.value.eval(payload), self.bundle.field(&tf.field)) else { continue };
                let v = round_to(v, def.decimals);
                if v.is_nan() || v < def.min || v > def.max {
                    out_of_range.push(tf.field.clone());
                } else {
                    fields.insert(tf.field.clone(), to_json(v, def));
                }
            }
            if let Some(s) = self.sources.get_mut(&source_id) {
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
            return out;
        }
        out
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
                let mut fields = BTreeMap::new();
                for tf in &table.fields {
                    if let Some(def) = self.bundle.field(&tf.field) {
                        if def.kind == SampleKind::Instantaneous {
                            fields.insert(tf.field.clone(), to_json(0.0, def));
                        }
                    }
                }
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
                matches: vec![MatchRule { manufacturer_id: Some(0x0102), name_prefix: None, min_length: 4, when: None }],
                only_when: Some(Expr::Bin(BinOp::Eq, Box::new(Expr::U8(0)), Box::new(Expr::Num(0.0)))),
                instance: Some(Instance { label: "bike".into(), value: Expr::U8(1) }),
                stale_after_ms: 3000,
                fields: vec![
                    TableField { field: "power".into(), value: Expr::U16le(2), only_when: None },
                    TableField { field: "energy".into(), value: Expr::U8(1), only_when: None },
                ],
            }],
        }
    }

    fn adv(data: &[u8], now_ms: u64) -> Advertisement<'_> {
        Advertisement { device_id: "d", name: None, rssi: Some(-50), manufacturer_data: data, now_ms }
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
