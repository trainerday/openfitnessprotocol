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
        #[serde(skip_serializing_if = "Option::is_none")]
        rssi: Option<i16>,
    },
    /// No packet for the table's `stale_after_ms`. Values resume without a new `source_appeared`.
    SourceStale { source_id: String },
}

struct Source {
    table: usize,
    last_ms: u64,
    stale: bool,
}

pub struct Core {
    bundle: Bundle,
    sources: BTreeMap<String, Source>,
    min_stale_ms: u64,
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
        Ok(Core { bundle, sources: BTreeMap::new(), min_stale_ms: 0 })
    }

    /// A host whose radio misses many advertisements can wait longer than a table says before a
    /// source counts as silent. The longer of the two times is used.
    pub fn set_min_stale_ms(&mut self, ms: u64) {
        self.min_stale_ms = ms;
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
                    self.sources.insert(source_id.clone(), Source { table: ti, last_ms: adv.now_ms, stale: false });
                    out.push(describe(&self.bundle, table, &source_id, instance));
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
            out.push(Message::Values { source_id, t: adv.now_ms, fields, out_of_range, stale: false, rssi: adv.rssi });
            return out;
        }
        out
    }

    /// Call about once a second. Sends zero values and `source_stale` for sources gone silent.
    pub fn tick(&mut self, now_ms: u64) -> Vec<Message> {
        let mut out = Vec::new();
        for (id, s) in self.sources.iter_mut() {
            let table = &self.bundle.tables[s.table];
            if s.stale || now_ms.saturating_sub(s.last_ms) < table.stale_after_ms.max(self.min_stale_ms) {
                continue;
            }
            s.stale = true;
            let mut fields = BTreeMap::new();
            for tf in &table.fields {
                if let Some(def) = self.bundle.field(&tf.field) {
                    if def.kind == SampleKind::Instantaneous {
                        fields.insert(tf.field.clone(), to_json(0.0, def));
                    }
                }
            }
            out.push(Message::Values { source_id: id.clone(), t: now_ms, fields, out_of_range: Vec::new(), stale: true, rssi: None });
            out.push(Message::SourceStale { source_id: id.clone() });
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
