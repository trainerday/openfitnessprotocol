//! What the Studio does, without HTTP and without a radio.

use crate::solve::{self, Sample};
use ofp_capture::{header, hex, parse_note, unhex, write_line, DeviceInfo, GattService, Line, PacketKind};
use ofp_core::{Advertisement, Core, Message};
use ofp_registry::check::{self, check_lines};
use ofp_registry::registry::{self, Registry};
use ofp_registry::replay::values_of;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;
use tokio::sync::broadcast;

pub type Reply = Result<Value, String>;

#[derive(Clone, Debug)]
pub struct Config {
    /// The registry folder (fields/ and tables/).
    pub registry: PathBuf,
    /// Where captures being worked on are kept.
    pub captures: PathBuf,
}

/// One advertisement, as the radio task hands it over.
#[derive(Clone, Debug, Default)]
pub struct AdvIn {
    pub device_id: String,
    pub name: Option<String>,
    pub rssi: Option<i16>,
    /// Company id first.
    pub mfr: Vec<u8>,
    pub service_data: BTreeMap<String, Vec<u8>>,
    pub services: Vec<String>,
}

struct DeviceRow {
    name: Option<String>,
    companies: Vec<String>,
    services: Vec<String>,
    rssi: Option<i16>,
    count: u64,
    first: Instant,
    last: Instant,
    longest_gap_ms: u64,
    has_data: bool,
    /// The registry already has a table for it: the source's name.
    known: Option<String>,
}

struct Session {
    name: String,
    device_id: Option<String>,
    from_file: bool,
    lines: Vec<Line>,
    path: Option<PathBuf>,
    file: Option<std::fs::File>,
    recording: bool,
    connect: bool,
    segment_start: Option<Instant>,
    t_offset: u64,
    /// The table being written, once it compiles.
    draft_yaml: Option<String>,
    /// What was chosen in the page (or by an assistant) that the draft was written from.
    design: Option<ProposeReq>,
    /// The draft was edited as text after it was written from the design.
    draft_edited: bool,
    live_core: Option<Core>,
    live_source: Option<Value>,
    live_values: BTreeMap<String, f64>,
    live_source_id: Option<String>,
    /// Every source the live core offered for this device, by id (what `source` events describe).
    live_sources: BTreeMap<String, Value>,
    /// Fixed bytes a table asks to write to the connected device (its `start`, then its `poll`), for the
    /// radio to send: (characteristic, bytes, with response).
    pending_writes: Vec<(String, Vec<u8>, bool)>,
}

impl Session {
    fn now(&self) -> u64 {
        self.t_offset + self.segment_start.map(|s| s.elapsed().as_millis() as u64).unwrap_or(0)
    }

    fn company_id(&self) -> Option<&str> {
        match self.lines.first() {
            Some(Line::Header { device, .. }) => device.company_id.as_deref(),
            _ => None,
        }
    }

    fn stream_keys(&self) -> Vec<(String, usize)> {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for l in &self.lines {
            if let Some((key, _, _)) = packet_of(l) {
                *counts.entry(key).or_default() += 1;
            }
        }
        let mut v: Vec<(String, usize)> = counts.into_iter().collect();
        // advertisements first, then the busiest characteristic
        v.sort_by(|a, b| (b.0 == "adv").cmp(&(a.0 == "adv")).then(b.1.cmp(&a.1)));
        v
    }

    /// The packets of one stream as a table would read them (advertisements: after the company id).
    fn samples(&self, stream: &str) -> Vec<Sample> {
        let strip = if stream == "adv" && self.company_id().is_some() { 2 } else { 0 };
        self.lines
            .iter()
            .filter_map(packet_of)
            .filter(|(key, _, _)| key == stream)
            .filter_map(|(_, t, bytes)| (bytes.len() >= strip).then(|| Sample { t, payload: bytes[strip..].to_vec() }))
            .collect()
    }

    fn append(&mut self, line: Line) -> Result<(), String> {
        if let Some(f) = &mut self.file {
            write_line(f, &line).and_then(|_| f.flush()).map_err(|e| e.to_string())?;
        }
        self.lines.push(line);
        Ok(())
    }
}

/// (stream key, time, bytes) of a packet line.
fn packet_of(line: &Line) -> Option<(String, u64, Vec<u8>)> {
    match line {
        Line::Packet { t, kind: PacketKind::Adv, mfr: Some(m), .. } => Some(("adv".into(), *t, unhex(m).ok()?)),
        Line::Packet { t, kind: PacketKind::Notify, chr: Some(c), data: Some(d), .. } => Some((c.clone(), *t, unhex(d).ok()?)),
        _ => None,
    }
}

struct State {
    devices: BTreeMap<String, DeviceRow>,
    known_core: Option<Core>,
    session: Option<Session>,
    radio: String,
}

pub struct Studio {
    cfg: Config,
    state: Mutex<State>,
    pub events: broadcast::Sender<String>,
    started: Instant,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct Mapping {
    pub field: String,
    pub expr: String,
    #[serde(default)]
    pub only_when: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct ProposeReq {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub mappings: Vec<Mapping>,
    #[serde(default)]
    pub only_when: Option<String>,
    #[serde(default)]
    pub instance_label: Option<String>,
    #[serde(default)]
    pub instance_expr: Option<String>,
    #[serde(default)]
    pub stale_after_ms: Option<u64>,
    #[serde(default)]
    pub provenance: Option<String>,
    #[serde(default)]
    pub verified_on: Option<String>,
    /// The stream the table reads: `adv` (a broadcast table) or a characteristic (a notify table).
    #[serde(default)]
    pub stream: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct FieldReq {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub unit: String,
    /// `int` or `decimal`
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub decimals: u8,
    pub min: f64,
    pub max: f64,
    /// instantaneous, event, cumulative or state
    pub kind: String,
    pub description: String,
    /// Add it even though similar fields exist.
    #[serde(default)]
    pub confirm_new: bool,
}

fn slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    let s = s.trim_end_matches('-').to_string();
    if s.is_empty() || !s.as_bytes()[0].is_ascii_lowercase() {
        format!("device-{s}").trim_end_matches('-').to_string()
    } else {
        s
    }
}

fn stamp() -> String {
    ofp_capture::now_iso().replace([':', '-'], "").replace('T', "-").replace('Z', "")
}

/// At most `max` points, evenly taken.
fn thin(points: Vec<(u64, f64)>, max: usize) -> Vec<(u64, f64)> {
    if points.len() <= max {
        return points;
    }
    let step = points.len().div_ceil(max);
    points.into_iter().step_by(step).collect()
}

impl Studio {
    pub fn new(cfg: Config) -> Studio {
        let (events, _) = broadcast::channel(1024);
        let studio = Studio {
            cfg,
            state: Mutex::new(State { devices: BTreeMap::new(), known_core: None, session: None, radio: "off".into() }),
            events,
            started: Instant::now(),
        };
        studio.reload_known();
        studio
    }

    fn emit(&self, v: Value) {
        let _ = self.events.send(v.to_string());
    }

    fn registry(&self) -> Result<Registry, String> {
        registry::load(&self.cfg.registry).map_err(|e| e.join("\n"))
    }

    fn reload_known(&self) {
        let core = self.registry().ok().map(|r| r.core());
        let mut st = self.state.lock().unwrap();
        st.known_core = core;
        for d in st.devices.values_mut() {
            d.known = None;
        }
    }

    pub fn set_radio(&self, status: &str) {
        self.state.lock().unwrap().radio = status.to_string();
        self.emit(json!({ "type": "radio", "status": status }));
    }

    // ───────────── what the page reads ─────────────

    pub fn state_json(&self) -> Value {
        let registry = match self.registry() {
            Ok(r) => json!({
                "fields": r.bundle.fields.iter().map(|f| json!({
                    "key": f.key, "id": f.id, "name": f.name, "unit": f.unit, "type": f.ty, "decimals": f.decimals,
                    "min": f.min, "max": f.max, "kind": f.kind, "tier": f.tier,
                    "description": r.descriptions.get(&f.key), "tolerance": r.tolerance(&f.key),
                })).collect::<Vec<_>>(),
                "tables": r.bundle.tables.iter().map(|t| json!({ "id": t.id, "name": t.name, "version": t.version, "reader": t.reader, "draft": r.drafts.contains(&t.id) })).collect::<Vec<_>>(),
            }),
            Err(e) => json!({ "error": e, "fields": [], "tables": [] }),
        };
        let st = self.state.lock().unwrap();
        json!({ "version": env!("CARGO_PKG_VERSION"), "registry": registry, "radio": st.radio, "session": st.session.as_ref().map(session_json) })
    }

    pub fn devices_json(&self) -> Value {
        let st = self.state.lock().unwrap();
        let now = Instant::now();
        let mut rows: Vec<Value> = st
            .devices
            .iter()
            .filter(|(_, d)| now.duration_since(d.last).as_secs() < 60)
            .map(|(id, d)| {
                let secs = now.duration_since(d.first).as_secs_f64().max(1.0);
                json!({
                    "device_id": id, "name": d.name, "companies": d.companies, "services": d.services, "rssi": d.rssi,
                    "adverts": d.count, "per_min": (d.count as f64 * 60.0 / secs).round(),
                    "longest_gap_s": (d.longest_gap_ms as f64 / 100.0).round() / 10.0,
                    "silent_s": now.duration_since(d.last).as_secs(), "has_data": d.has_data, "known": d.known,
                })
            })
            .collect();
        // A steady order, so rows do not jump as signal strengths change: devices the registry knows,
        // then named ones by name, then the nameless. (A map keyed by device id is already steady.)
        rows.sort_by(|a, b| {
            let key = |v: &Value| (v["known"].is_null(), v["name"].is_null(), v["name"].as_str().unwrap_or("").to_lowercase(), v["device_id"].as_str().unwrap_or("").to_string());
            key(a).cmp(&key(b))
        });
        json!(rows)
    }

    /// Capture files that can be opened: the ones being worked on and the registry's.
    pub fn captures_json(&self) -> Value {
        let mut out = Vec::new();
        for (dir, place) in [(self.cfg.captures.clone(), "captures"), (self.cfg.registry.join("tables"), "registry")] {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            let mut files: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.to_string_lossy().ends_with(".capture.jsonl")).collect();
            // newest first
            files.sort_by_key(|f| std::cmp::Reverse(std::fs::metadata(f).and_then(|m| m.modified()).ok()));
            for f in files {
                let size = std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
                out.push(json!({ "path": f.display().to_string(), "name": f.file_name().and_then(|n| n.to_str()), "place": place, "kb": size.div_ceil(1024) }));
            }
        }
        json!(out)
    }

    // ───────────── session ─────────────

    pub fn open_device(&self, device_id: &str) -> Reply {
        let mut st = self.state.lock().unwrap();
        if st.session.as_ref().is_some_and(|s| s.recording) {
            return Err("Stop the recording first.".into());
        }
        let d = st.devices.get(device_id).ok_or("That device is no longer in range.")?;
        let name = d.name.clone().unwrap_or_else(|| "device".into());
        let info = DeviceInfo { name: d.name.clone(), company_id: d.companies.first().cloned(), services: d.services.clone() };
        let connect = !d.has_data;
        st.session = Some(Session {
            name,
            device_id: Some(device_id.to_string()),
            from_file: false,
            lines: vec![header(concat!("ofp studio ", env!("CARGO_PKG_VERSION")), info, vec![])],
            path: None,
            file: None,
            recording: false,
            connect,
            segment_start: None,
            t_offset: 0,
            draft_yaml: None,
            design: None,
            draft_edited: false,
            live_core: None,
            live_source: None,
            live_values: BTreeMap::new(),
            live_source_id: None,
            live_sources: BTreeMap::new(),
            pending_writes: Vec::new(),
        });
        let v = session_json(st.session.as_ref().unwrap());
        drop(st);
        self.emit(json!({ "type": "session" }));
        Ok(v)
    }

    pub fn open_capture(&self, path: &Path) -> Reply {
        let lines = ofp_capture::read_file(path)?;
        let name = match lines.first() {
            Some(Line::Header { device, .. }) => device.name.clone(),
            _ => None,
        }
        .unwrap_or_else(|| "device".into());
        let mut st = self.state.lock().unwrap();
        if st.session.as_ref().is_some_and(|s| s.recording) {
            return Err("Stop the recording first.".into());
        }
        st.session = Some(Session {
            name,
            device_id: None,
            from_file: true,
            lines,
            path: Some(path.to_path_buf()),
            file: None,
            recording: false,
            connect: false,
            segment_start: None,
            t_offset: 0,
            draft_yaml: None,
            design: None,
            draft_edited: false,
            live_core: None,
            live_source: None,
            live_values: BTreeMap::new(),
            live_source_id: None,
            live_sources: BTreeMap::new(),
            pending_writes: Vec::new(),
        });
        let v = session_json(st.session.as_ref().unwrap());
        drop(st);
        self.emit(json!({ "type": "session" }));
        Ok(v)
    }

    pub fn close(&self) -> Reply {
        let mut st = self.state.lock().unwrap();
        if st.session.as_ref().is_some_and(|s| s.recording) {
            return Err("Stop the recording first.".into());
        }
        st.session = None;
        drop(st);
        self.emit(json!({ "type": "session" }));
        Ok(json!({}))
    }

    /// Start recording. Returns the device to connect to when the device must be connected.
    pub fn record_start(&self, connect: Option<bool>) -> Result<(Value, Option<String>), String> {
        let registry = self.registry().ok();
        let mut st = self.state.lock().unwrap();
        let s = st.session.as_mut().ok_or("Choose a device first.")?;
        if s.from_file {
            return Err("This is a capture file. Choose the device in Devices to record more.".into());
        }
        if s.recording {
            return Err("Already recording.".into());
        }
        if let Some(c) = connect {
            s.connect = c;
        }
        // Every recording is its own file: starting again after a stop begins a new one (the table
        // being written is kept).
        if s.file.is_some() {
            s.lines.truncate(1);
            if let Some(Line::Header { started, gatt, .. }) = s.lines.first_mut() {
                *started = ofp_capture::now_iso();
                gatt.clear();
            }
            s.file = None;
            s.path = None;
            s.t_offset = 0;
        }
        if s.file.is_none() {
            std::fs::create_dir_all(&self.cfg.captures).map_err(|e| e.to_string())?;
            let path = self.cfg.captures.join(format!("{}-{}.capture.jsonl", ofp_capture::file_stem_for(&s.name), stamp()));
            let mut f = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            for l in &s.lines {
                write_line(&mut f, l).map_err(|e| e.to_string())?;
            }
            s.file = Some(f);
            s.path = Some(path);
        }
        s.recording = true;
        s.segment_start = Some(Instant::now());
        s.live_core = live_core(registry.as_ref(), s.draft_yaml.as_deref());
        s.live_source = None;
        s.live_source_id = None;
        s.live_sources.clear();
        s.live_values.clear();
        let connect_to = s.connect.then(|| s.device_id.clone()).flatten();
        let reply = session_json(s);
        drop(st);
        self.emit(json!({ "type": "recording", "on": true }));
        Ok((reply, connect_to))
    }

    pub fn record_stop(&self) -> Reply {
        let mut st = self.state.lock().unwrap();
        let s = st.session.as_mut().ok_or("Nothing is being recorded.")?;
        if s.recording {
            s.t_offset = s.now() + 1;
            s.segment_start = None;
            s.recording = false;
        }
        let reply = session_json(s);
        drop(st);
        self.emit(json!({ "type": "recording", "on": false }));
        Ok(reply)
    }

    pub fn is_recording_connected(&self) -> bool {
        self.state.lock().unwrap().session.as_ref().is_some_and(|s| s.recording && s.connect)
    }

    pub fn note(&self, text: &str) -> Reply {
        let text = text.trim();
        if text.is_empty() {
            return Err("The note is empty.".into());
        }
        let mut st = self.state.lock().unwrap();
        let s = st.session.as_mut().ok_or("Choose a device first.")?;
        if !s.recording {
            return Err("Notes are saved while recording: they say what the display showed at that moment.".into());
        }
        let line = Line::Note { t: s.now(), text: text.to_string(), values: parse_note(text) };
        let v = serde_json::to_value(&line).map_err(|e| e.to_string())?;
        s.append(line)?;
        drop(st);
        self.emit(v.clone());
        Ok(v)
    }

    pub fn capture_json(&self) -> Reply {
        let st = self.state.lock().unwrap();
        let s = st.session.as_ref().ok_or("Choose a device or open a capture first.")?;
        let strip = if s.company_id().is_some() { 2 } else { 0 };
        let mut packets = Vec::new();
        let (mut notes, mut confirms) = (Vec::new(), Vec::new());
        // what was read once after connecting, and what the tester wrote: [t, "read" | "write", characteristic, hex]
        let mut io = Vec::new();
        for l in &s.lines {
            match l {
                Line::Packet { t, kind: kind @ (PacketKind::Read | PacketKind::Write), chr: Some(c), data: Some(d), .. } => {
                    io.push(json!([t, if *kind == PacketKind::Read { "read" } else { "write" }, c, d]));
                }
                Line::Note { .. } => notes.push(serde_json::to_value(l).unwrap_or_default()),
                Line::Confirm { .. } => confirms.push(serde_json::to_value(l).unwrap_or_default()),
                _ => {
                    if let Some((key, t, bytes)) = packet_of(l) {
                        let cut = if key == "adv" { strip.min(bytes.len()) } else { 0 };
                        packets.push(json!([t, key, hex(&bytes[cut..])]));
                    }
                }
            }
        }
        let skipped = packets.len().saturating_sub(20_000);
        Ok(json!({
            "session": session_json(s),
            "header": s.lines.first(),
            "packets": packets[skipped..],
            "notes": notes,
            "confirms": confirms,
            "io": io,
        }))
    }

    /// The recording as a file to send to someone: (file name, text).
    pub fn capture_file(&self) -> Result<(String, String), String> {
        let st = self.state.lock().unwrap();
        let s = st.session.as_ref().ok_or("Choose a device or open a capture first.")?;
        let mut text = Vec::new();
        for l in &s.lines {
            write_line(&mut text, l).map_err(|e| e.to_string())?;
        }
        let name = s.path.as_ref().and_then(|p| p.file_name()).and_then(|n| n.to_str()).map(String::from).unwrap_or_else(|| format!("{}.capture.jsonl", ofp_capture::file_stem_for(&s.name)));
        Ok((name, String::from_utf8_lossy(&text).into_owned()))
    }

    /// A recording someone sent: checked, kept in the captures folder, and opened.
    pub fn open_uploaded(&self, name: &str, text: &str) -> Reply {
        ofp_capture::read(text.as_bytes()).map_err(|e| format!("That is not an OFP recording: {e}"))?;
        std::fs::create_dir_all(&self.cfg.captures).map_err(|e| e.to_string())?;
        let stem = ofp_capture::file_stem_for(name.trim_end_matches(".jsonl").trim_end_matches(".capture"));
        let path = self.cfg.captures.join(format!("{stem}-received-{}.capture.jsonl", stamp()));
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        self.open_capture(&path)
    }

    // ───────────── finding the bytes ─────────────

    fn default_stream(s: &Session) -> Result<String, String> {
        s.stream_keys().first().map(|(k, _)| k.clone()).ok_or_else(|| "Nothing has been recorded yet.".to_string())
    }

    pub fn series(&self, stream: Option<&str>, expr: &str) -> Reply {
        let st = self.state.lock().unwrap();
        let s = st.session.as_ref().ok_or("Choose a device or open a capture first.")?;
        let stream = match stream {
            Some(k) => k.to_string(),
            None => Self::default_stream(s)?,
        };
        let points = solve::series(&s.samples(&stream), expr)?;
        let (mut min, mut max) = (f64::INFINITY, f64::NEG_INFINITY);
        for (_, v) in &points {
            min = min.min(*v);
            max = max.max(*v);
        }
        Ok(json!({ "stream": stream, "count": points.len(), "min": if points.is_empty() { Value::Null } else { json!(min) }, "max": if points.is_empty() { Value::Null } else { json!(max) }, "points": thin(points, 2000) }))
    }

    pub fn solve(&self, stream: Option<&str>) -> Reply {
        let registry = self.registry().ok();
        let st = self.state.lock().unwrap();
        let s = st.session.as_ref().ok_or("Choose a device or open a capture first.")?;
        let stream = match stream {
            Some(k) => k.to_string(),
            None => Self::default_stream(s)?,
        };
        let samples = s.samples(&stream);
        let mut noted: BTreeMap<String, Vec<(u64, f64)>> = BTreeMap::new();
        for l in &s.lines {
            if let Line::Note { t, values, .. } = l {
                for (k, v) in values {
                    noted.entry(k.clone()).or_default().push((*t, *v));
                }
            }
        }
        if noted.is_empty() {
            return Err("There are no notes with values yet. While recording, type what the device's display shows, for example power=150 cadence=82.".into());
        }
        let mut fields = serde_json::Map::new();
        for (key, notes) in noted {
            let known = registry.as_ref().is_some_and(|r| r.bundle.field(&key).is_some());
            let tolerance = registry.as_ref().map(|r| r.tolerance(&key)).unwrap_or_default();
            let range = registry.as_ref().and_then(|r| r.bundle.field(&key)).map(|f| (f.min, f.max));
            let candidates = solve::solve(&samples, &notes, tolerance, range, 5);
            fields.insert(key, json!({ "known_field": known, "notes": notes.len(), "candidates": candidates }));
        }
        Ok(json!({ "stream": stream, "packets": samples.len(), "fields": fields }))
    }

    /// Before adding: does the registry already know this device, and which fields would be reused?
    /// "Extend, don't add": a table that already decodes the capture should be changed, not doubled.
    pub fn similar(&self) -> Reply {
        let registry = self.registry()?;
        let st = self.state.lock().unwrap();
        let s = st.session.as_ref().ok_or("Choose a device or open a capture first.")?;
        let r = check_lines(&registry, None, "this capture", &s.lines);
        let mut tables: BTreeMap<String, usize> = BTreeMap::new();
        for d in r.timeline.iter().filter(|d| !d.stale) {
            *tables.entry(d.source_id.split(':').next().unwrap_or("").to_string()).or_default() += 1;
        }
        let company = s.company_id().map(|c| c.to_string());
        let existing: Vec<Value> = tables
            .iter()
            .map(|(id, packets)| {
                let path = self.cfg.registry.join("tables").join(format!("{id}.yaml"));
                json!({ "table": id, "packets_decoded": packets, "of_adverts": r.adverts, "yaml": std::fs::read_to_string(&path).ok() })
            })
            .collect();
        let advice = if existing.is_empty() {
            "No table in the registry decodes this device. A new table is the right thing.".to_string()
        } else {
            format!("The registry already decodes this device with table '{}'. Extend that table (bump its version) instead of adding a second one.", tables.keys().next().unwrap())
        };
        Ok(json!({ "company_id": company, "existing_tables": existing, "advice": advice }))
    }

    // ───────────── the table ─────────────

    /// The table as YAML from what was chosen in the page; everything not given is proposed from the capture.
    pub fn propose(&self, req: ProposeReq) -> Reply {
        let st = self.state.lock().unwrap();
        let s = st.session.as_ref().ok_or("Choose a device or open a capture first.")?;
        propose_yaml(s, req)
    }

    /// Set what the table is made of. The table text is written from it and becomes the shared draft,
    /// so a person in the page and an assistant on the API work on the same table.
    pub fn set_design(&self, req: ProposeReq) -> Reply {
        let mut st = self.state.lock().unwrap();
        let s = st.session.as_mut().ok_or("Choose a device or open a capture first.")?;
        let v = propose_yaml(s, req.clone())?;
        s.draft_yaml = v["yaml"].as_str().map(String::from);
        s.design = Some(req);
        s.draft_edited = false;
        let out = json!({ "type": "draft", "yaml": s.draft_yaml, "design": s.design, "edited": false });
        drop(st);
        self.emit(out.clone());
        Ok(out)
    }
}

fn propose_yaml(s: &Session, req: ProposeReq) -> Reply {
    {
        // Which stream the table reads: the one asked for, else advertisements when there are any,
        // else the busiest characteristic.
        let keys = s.stream_keys();
        let stream = req.stream.clone().filter(|k| keys.iter().any(|(x, _)| x == k)).or_else(|| keys.first().map(|(k, _)| k.clone())).unwrap_or_else(|| "adv".to_string());
        let notify = stream != "adv";
        let samples = s.samples(&stream);
        let name = req.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| s.name.clone());
        let id = req.id.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| slug(&name));
        let min_length = samples.iter().map(|x| x.payload.len()).min().unwrap_or(1);
        let mut y = String::new();
        y.push_str(&format!("id: {id}\nname: {}\nreader: {}\nversion: 1\nstatus: experimental\n\n", yaml_text(&name), if notify { "notify" } else { "broadcast" }));
        y.push_str(&format!("provenance: {}\n", yaml_text(req.provenance.as_deref().filter(|p| !p.trim().is_empty()).unwrap_or("My own capture, compared with the device's own display."))));
        y.push_str(&format!("verified_on:\n  - {}\n\n", yaml_text(req.verified_on.as_deref().filter(|p| !p.trim().is_empty()).unwrap_or(&name))));
        if notify {
            // the service the characteristic belongs to, as found after connecting
            let service = match s.lines.first() {
                Some(Line::Header { gatt, .. }) => gatt.iter().find(|g| g.characteristics.iter().any(|c| c.uuid == stream)).map(|g| g.uuid.clone()),
                _ => None,
            }
            .unwrap_or_else(|| "FILL-IN-THE-SERVICE".to_string());
            y.push_str(&format!("# The device is connected to; this table reads one characteristic's notifications.\nmatch:\n  - name_prefix: {}\n\nnotify:\n  service: {}\n  characteristic: {}\n  min_length: {min_length}\n", yaml_text(&s.name), yaml_text(&service), yaml_text(&stream)));
        } else {
            y.push_str("# Positions count from the start of the payload, after the 2-byte company id.\nmatch:\n");
            match s.company_id() {
                Some(c) => y.push_str(&format!("  - manufacturer_id: 0x{c}\n    min_length: {min_length}\n")),
                None => y.push_str(&format!("  - name_prefix: {}\n    min_length: {min_length}\n", yaml_text(&s.name))),
            }
        }
        if let Some(w) = req.only_when.as_deref().filter(|w| !w.trim().is_empty()) {
            y.push_str(&format!("\nonly_when: {}\n", yaml_text(w)));
        }
        if let Some(e) = req.instance_expr.as_deref().filter(|w| !w.trim().is_empty()) {
            y.push_str(&format!("\ninstance:\n  label: {}\n  value: {}\n", yaml_text(req.instance_label.as_deref().filter(|l| !l.trim().is_empty()).unwrap_or("device")), yaml_text(e)));
        }
        y.push_str(&format!("\nstale_after_ms: {}\n\nfields:\n", req.stale_after_ms.unwrap_or(3000)));
        for m in &req.mappings {
            match m.only_when.as_deref().filter(|w| !w.trim().is_empty()) {
                Some(w) => y.push_str(&format!("  {}:\n    value: {}\n    only_when: {}\n", m.field, yaml_text(&m.expr), yaml_text(w))),
                None => y.push_str(&format!("  {}: {}\n", m.field, yaml_text(&m.expr))),
            }
        }
        if req.mappings.is_empty() {
            y.push_str("  # nothing mapped yet\n");
        }
        Ok(json!({ "yaml": y, "id": id }))
    }
}

impl Studio {

    /// Compile the table text and replay this capture through it: the same judgement CI makes.
    pub fn check_table(&self, yaml: &str) -> Reply {
        let registry = self.registry()?;
        let draft = match registry.with_only_table(yaml) {
            Ok(d) => d,
            Err(problems) => return Ok(json!({ "ok": false, "compiled": false, "problems": problems })),
        };
        let mut st = self.state.lock().unwrap();
        let s = st.session.as_mut().ok_or("Choose a device or open a capture first.")?;
        let table = draft.bundle.tables[0].clone();
        let mut pending: Vec<Value> = Vec::new();
        let r = check_lines(&draft, Some(&table.id), "this capture", &s.lines);
        let mut series: BTreeMap<String, Vec<(u64, f64)>> = BTreeMap::new();
        let mut sources = BTreeSet::new();
        for d in r.timeline.iter().filter(|d| !d.stale) {
            sources.insert(d.source_id.clone());
            for (k, v) in &d.fields {
                series.entry(k.clone()).or_default().push((d.t, *v));
            }
        }
        let series: BTreeMap<String, Vec<(u64, f64)>> = series.into_iter().map(|(k, v)| (k, thin(v, 1500))).collect();
        if s.draft_yaml.as_deref() != Some(yaml) {
            s.draft_edited = s.design.is_some();
            s.draft_yaml = Some(yaml.to_string());
        }
        if s.recording {
            s.live_core = Some(draft.core());
            s.live_source = None;
            s.live_source_id = None;
            s.live_sources.clear();
            s.live_values.clear();
            if s.connect {
                let mut primed = Vec::new();
                prime_connected(s, &mut primed);
                pending.extend(primed);
            }
        }
        let out = json!({
            "ok": r.errors.is_empty(), "compiled": true, "problems": r.errors, "summary": r.summary,
            "table": table.id, "decoded": r.decoded, "adverts": r.adverts, "sources": sources, "series": series,
        });
        let event = json!({ "type": "checked", "yaml": yaml, "edited": s.draft_edited, "ok": out["ok"], "problems": out["problems"], "summary": out["summary"] });
        drop(st);
        self.emit(event);
        for v in pending {
            self.emit(v);
        }
        Ok(out)
    }

    /// Draft a new experimental field into registry/fields.
    pub fn add_field(&self, f: FieldReq) -> Reply {
        let registry = self.registry()?;
        let key = f.key.trim();
        if registry.bundle.field(key).is_some() {
            return Err(format!("A field '{key}' already exists."));
        }
        if !matches!(f.ty.as_str(), "int" | "decimal") || !matches!(f.kind.as_str(), "instantaneous" | "event" | "cumulative" | "state") {
            return Err("Choose a number type and a kind.".into());
        }
        // Reuse before adding: a field with the same name, or the same unit and kind, is offered first.
        if !f.confirm_new {
            let words = |t: &str| -> BTreeSet<String> { t.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 2).map(String::from).collect() };
            let mine = &words(&f.name) | &words(key);
            let alike: Vec<Value> = registry
                .bundle
                .fields
                .iter()
                .filter(|x| {
                    let theirs = &words(&x.name) | &words(&x.key);
                    !mine.is_disjoint(&theirs) || (!f.unit.trim().is_empty() && x.unit == f.unit.trim() && format!("{:?}", x.kind).to_lowercase() == f.kind)
                })
                .map(|x| json!({ "key": x.key, "name": x.name, "unit": x.unit, "description": registry.descriptions.get(&x.key) }))
                .collect();
            if !alike.is_empty() {
                return Ok(json!({ "similar": alike, "advice": "These fields already exist. Use one of them if it means the same thing; send confirm_new: true to add yours anyway." }));
            }
        }
        let id = registry.bundle.fields.iter().map(|x| x.id).max().unwrap_or(0) + 1;
        let decimals = if f.ty == "int" { 0 } else { f.decimals.clamp(1, 4) };
        let mut y = format!("id: {id}\nkey: {key}\nname: {}\nunit: {}\ntype: {}\n", yaml_text(f.name.trim()), yaml_text(f.unit.trim()), f.ty);
        if decimals > 0 {
            y.push_str(&format!("decimals: {decimals}\n"));
        }
        y.push_str(&format!("min: {}\nmax: {}\nkind: {}\ntier: experimental\ndescription: {}\n", f.min, f.max, f.kind, yaml_text(f.description.trim())));
        let path = self.cfg.registry.join("fields").join(format!("{key}.yaml"));
        if path.exists() {
            return Err(format!("{} already exists.", path.display()));
        }
        std::fs::write(&path, &y).map_err(|e| format!("{}: {e}", path.display()))?;
        if let Err(problems) = registry::load(&self.cfg.registry) {
            let _ = std::fs::remove_file(&path);
            return Err(problems.join("\n"));
        }
        self.reload_known();
        Ok(json!({ "key": key, "id": id, "file": path.display().to_string() }))
    }

    /// The rider's verdict on a value being shown: right, or wrong (`wrong`, with what the device's own
    /// display showed when they know it).
    pub fn confirm(&self, field: &str, console: Option<f64>, wrong: bool) -> Reply {
        let mut st = self.state.lock().unwrap();
        let s = st.session.as_mut().ok_or("Choose a device first.")?;
        if !s.recording {
            return Err("Confirming is done while recording, with the device in front of you.".into());
        }
        let shown = *s.live_values.get(field).ok_or_else(|| format!("No value for '{field}' has been decoded yet."))?;
        let source_id = s.live_source_id.clone().unwrap_or_default();
        let ok = !wrong && console.is_none_or(|c| (c - shown).abs() < 1e-9);
        let line = Line::Confirm { t: s.now(), source_id, field: field.to_string(), shown, console: if ok { None } else { console }, ok };
        let v = serde_json::to_value(&line).map_err(|e| e.to_string())?;
        s.append(line)?;
        drop(st);
        self.emit(v.clone());
        Ok(v)
    }

    /// Write the table and this capture into the registry, if the whole registry still checks.
    pub fn save(&self, yaml: &str, overwrite: bool) -> Reply {
        if self.state.lock().unwrap().session.as_ref().is_some_and(|s| s.recording) {
            return Err("Stop the recording first.".into());
        }
        let registry = self.registry()?;
        let draft = match registry.with_only_table(yaml) {
            Ok(d) => d,
            Err(problems) => return Ok(json!({ "ok": false, "problems": problems })),
        };
        let id = draft.bundle.tables[0].id.clone();
        let lines = {
            let st = self.state.lock().unwrap();
            let s = st.session.as_ref().ok_or("Choose a device or open a capture first.")?;
            if s.recording {
                return Err("Stop the recording first.".into());
            }
            s.lines.clone()
        };
        let r = check_lines(&draft, Some(&id), "this capture", &lines);
        if !r.errors.is_empty() {
            return Ok(json!({ "ok": false, "problems": r.errors }));
        }
        let tables = self.cfg.registry.join("tables");
        let table_path = tables.join(format!("{id}.yaml"));
        let previous = std::fs::read_to_string(&table_path).ok();
        if previous.is_some() && !overwrite {
            return Ok(json!({ "ok": false, "exists": true, "problems": [format!("A table '{id}' already exists. Saving replaces it and adds this capture beside the ones it has.")] }));
        }
        let first = tables.join(format!("{id}.capture.jsonl"));
        let capture_path = if first.exists() { tables.join(format!("{id}.{}.capture.jsonl", stamp())) } else { first };
        let mut text = Vec::new();
        for l in &lines {
            write_line(&mut text, l).map_err(|e| e.to_string())?;
        }
        std::fs::write(&table_path, yaml).map_err(|e| format!("{}: {e}", table_path.display()))?;
        std::fs::File::create(&capture_path).and_then(|mut f| f.write_all(&text)).map_err(|e| format!("{}: {e}", capture_path.display()))?;
        let report = check::run(&self.cfg.registry, false);
        if !report.errors.is_empty() {
            // Leave the registry as it was.
            let _ = std::fs::remove_file(&capture_path);
            match previous {
                Some(p) => {
                    let _ = std::fs::write(&table_path, p);
                }
                None => {
                    let _ = std::fs::remove_file(&table_path);
                }
            }
            return Ok(json!({ "ok": false, "problems": report.errors }));
        }
        self.reload_known();
        let rel = |p: &Path| p.strip_prefix(self.cfg.registry.parent().unwrap_or(Path::new(""))).unwrap_or(p).display().to_string();
        Ok(json!({
            "ok": true, "table": rel(&table_path), "capture": rel(&capture_path), "report": report.lines,
            "next": [
                format!("git checkout -b add-{id}"),
                format!("git add {} {}", rel(&table_path), rel(&capture_path)),
                format!("git commit -s -m \"Add {id}\""),
                "git push  (then open a pull request)".to_string(),
            ],
        }))
    }

    // ───────────── from the radio ─────────────

    pub fn on_adv(&self, adv: AdvIn) {
        let now = Instant::now();
        let now_ms = now.duration_since(self.started).as_millis() as u64;
        let mut out = Vec::new();
        {
            let mut guard = self.state.lock().unwrap();
            let st = &mut *guard;
            let d = st.devices.entry(adv.device_id.clone()).or_insert_with(|| DeviceRow {
                name: None, companies: vec![], services: vec![], rssi: None, count: 0, first: now, last: now, longest_gap_ms: 0, has_data: false, known: None,
            });
            if d.count > 0 {
                d.longest_gap_ms = d.longest_gap_ms.max(now.duration_since(d.last).as_millis() as u64);
            }
            d.count += 1;
            d.last = now;
            if adv.name.is_some() {
                d.name = adv.name.clone();
            }
            if adv.rssi.is_some() {
                d.rssi = adv.rssi;
            }
            if adv.mfr.len() >= 2 {
                let c = format!("{:02x}{:02x}", adv.mfr[1], adv.mfr[0]);
                if !d.companies.contains(&c) {
                    d.companies.push(c);
                }
                // more than a company id: the advert carries data
                if adv.mfr.len() > 4 {
                    d.has_data = true;
                }
            }
            for u in adv.services.iter().chain(adv.service_data.keys()) {
                if !d.services.contains(u) {
                    d.services.push(u.clone());
                }
            }
            if d.known.is_none() {
                if let Some(core) = &mut st.known_core {
                    for m in core.on_advertisement(&Advertisement { device_id: &adv.device_id, name: adv.name.as_deref(), rssi: adv.rssi, manufacturer_data: &adv.mfr, services: &d.services, now_ms }) {
                        if let Message::SourceAppeared { name, .. } = m {
                            d.known = Some(name);
                        }
                    }
                }
            }
            if let Some(s) = st.session.as_mut() {
                if s.recording && !s.connect && s.device_id.as_deref() == Some(adv.device_id.as_str()) && (!adv.mfr.is_empty() || !adv.service_data.is_empty()) {
                    let t = s.now();
                    let line = Line::adv(t, adv.name.clone(), adv.rssi, &adv.mfr, adv.service_data.iter().map(|(k, v)| (k.clone(), hex(v))).collect());
                    let strip = if s.company_id().is_some() { 2.min(adv.mfr.len()) } else { 0 };
                    if !adv.mfr.is_empty() {
                        out.push(json!({ "type": "packet", "t": t, "stream": "adv", "hex": hex(&adv.mfr[strip..]), "rssi": adv.rssi }));
                    }
                    let _ = s.append(line);
                    if let Some(core) = &mut s.live_core {
                        let messages = core.on_advertisement(&Advertisement { device_id: &adv.device_id, name: adv.name.as_deref(), rssi: adv.rssi, manufacturer_data: &adv.mfr, services: &adv.services, now_ms: t });
                        live_messages(s, messages, &mut out);
                    }
                }
            }
        }
        for v in out {
            self.emit(v);
        }
    }

    /// The device was connected: what it offers, and what could be read.
    pub fn on_connected(&self, gatt: Vec<GattService>, reads: Vec<(String, Vec<u8>)>, subscribed: Vec<String>) {
        let mut out = Vec::new();
        let mut st = self.state.lock().unwrap();
        if let Some(s) = st.session.as_mut() {
            if let Some(Line::Header { gatt: g, .. }) = s.lines.first_mut() {
                *g = gatt.clone();
            }
            // the header changed: write the file again from the start
            if let Some(path) = s.path.clone() {
                if let Ok(mut f) = std::fs::File::create(&path) {
                    for l in &s.lines {
                        let _ = write_line(&mut f, l);
                    }
                    s.file = Some(f);
                }
            }
            let t = s.now();
            for (chr, value) in reads {
                let _ = s.append(Line::Packet { t, kind: PacketKind::Read, name: None, rssi: None, mfr: None, service_data: BTreeMap::new(), chr: Some(chr), data: Some(hex(&value)) });
            }
            prime_connected(s, &mut out);
        }
        drop(st);
        self.emit(json!({ "type": "connected", "gatt": gatt, "subscribed": subscribed }));
        for v in out {
            self.emit(v);
        }
    }

    pub fn on_notify(&self, chr: &str, value: &[u8]) {
        let mut st = self.state.lock().unwrap();
        let Some(s) = st.session.as_mut() else { return };
        if !s.recording {
            return;
        }
        let t = s.now();
        let _ = s.append(Line::Packet { t, kind: PacketKind::Notify, name: None, rssi: None, mfr: None, service_data: BTreeMap::new(), chr: Some(chr.to_string()), data: Some(hex(value)) });
        let mut out = vec![json!({ "type": "packet", "t": t, "stream": chr, "hex": hex(value) })];
        if let Some(core) = &mut s.live_core {
            let messages = core.on_notification(LIVE_DEVICE, chr, value, t);
            live_messages(s, messages, &mut out);
        }
        drop(st);
        for v in out {
            self.emit(v);
        }
    }

    /// Whether a write may go out now: only to a device this session is connected to.
    pub fn can_write(&self) -> Result<(), String> {
        match self.state.lock().unwrap().session.as_ref() {
            Some(s) if s.recording && s.connect => Ok(()),
            _ => Err("Connect to the device first (Tester → Connect).".into()),
        }
    }

    /// Bytes were written to the device (or the write failed): it goes into the capture and the log.
    pub fn on_write(&self, chr: &str, value: &[u8], error: Option<String>) {
        let mut st = self.state.lock().unwrap();
        let Some(s) = st.session.as_mut() else { return };
        let t = s.now();
        if error.is_none() {
            let _ = s.append(Line::Packet { t, kind: PacketKind::Write, name: None, rssi: None, mfr: None, service_data: BTreeMap::new(), chr: Some(chr.to_string()), data: Some(hex(value)) });
        }
        drop(st);
        self.emit(json!({ "type": "tx", "t": t, "chr": chr, "hex": hex(value), "error": error }));
    }

    /// Once a second: silence is noticed here, and the device list goes out.
    /// Often (every 200 ms): the polls a connected device's table asks for. The radio sends what this returns.
    pub fn poll_tick(&self) -> Vec<(String, Vec<u8>, bool)> {
        let mut out = Vec::new();
        let writes = {
            let mut st = self.state.lock().unwrap();
            let Some(s) = st.session.as_mut() else { return Vec::new() };
            if s.recording {
                let t = s.now();
                if let Some(core) = &mut s.live_core {
                    let messages = core.tick(t);
                    for m in &messages {
                        if let Message::Write { write, .. } = m {
                            let w = write_of(write);
                            s.pending_writes.push(w);
                        }
                    }
                    live_messages(s, messages, &mut out);
                }
            }
            std::mem::take(&mut s.pending_writes)
        };
        for v in out {
            self.emit(v);
        }
        writes
    }

    pub fn tick(&self) {
        let mut out = Vec::new();
        {
            let mut st = self.state.lock().unwrap();
            if let Some(s) = st.session.as_mut() {
                if s.recording {
                    let t = s.now();
                    if let Some(core) = &mut s.live_core {
                        let messages = core.tick(t);
                        live_messages(s, messages, &mut out);
                    }
                }
            }
        }
        for v in out {
            self.emit(v);
        }
        if self.events.receiver_count() > 0 {
            self.emit(json!({ "type": "devices", "devices": self.devices_json() }));
        }
    }
}

/// The device id a connected session's device has inside the live core.
const LIVE_DEVICE: &str = "live";

fn write_of(w: &ofp_core::Write) -> (String, Vec<u8>, bool) {
    let bytes = (0..w.data.len() / 2).filter_map(|i| u8::from_str_radix(&w.data[i * 2..i * 2 + 2], 16).ok()).collect();
    (w.characteristic.clone(), bytes, w.with_response)
}

/// A connected session: tell the live core the device is there (its name and services), use every
/// table that knows it, and say it is connected. From then on its notifications are decoded.
fn prime_connected(s: &mut Session, out: &mut Vec<Value>) {
    let (name, services) = match s.lines.first() {
        Some(Line::Header { device, gatt, .. }) => {
            let mut services = device.services.clone();
            services.extend(gatt.iter().map(|g| g.uuid.clone()));
            (device.name.clone(), services)
        }
        _ => (None, Vec::new()),
    };
    let t = s.now();
    let mut s_writes = Vec::new();
    let Some(core) = &mut s.live_core else { return };
    let appeared = core.on_advertisement(&Advertisement { device_id: LIVE_DEVICE, name: name.as_deref(), rssi: None, manufacturer_data: &[], services: &services, now_ms: t });
    for m in &appeared {
        if let Message::SourceAppeared { source_id, connectable: true, .. } = m {
            // a table that must start the device's data: its fixed start frames, for the radio to write
        for m in core.use_source(source_id, t).unwrap_or_default() {
            if let Message::Connect { start, .. } = m {
                for w in start {
                    s_writes.push(write_of(&w));
                }
            }
        }
        }
    }
    core.on_connected(LIVE_DEVICE, t);
    s.pending_writes.extend(s_writes);
    live_messages(s, appeared, out);
}

fn live_core(registry: Option<&Registry>, draft: Option<&str>) -> Option<Core> {
    let registry = registry?;
    match draft.and_then(|y| registry.with_only_table(y).ok()) {
        Some(d) => Some(d.core()),
        None => Some(registry.core()),
    }
}

fn live_messages(s: &mut Session, messages: Vec<Message>, out: &mut Vec<Value>) {
    for m in messages {
        match &m {
            Message::SourceAppeared { source_id, .. } => {
                s.live_sources.insert(source_id.clone(), serde_json::to_value(&m).unwrap_or_default());
                // One source is shown as the device's table. A device's own table comes before a standard one
                // (a bike with its own service and FTMS shows its own); the standard ones are shown by the
                // Tester's readouts beside it.
                let standard = |id: &str| id.starts_with("std-");
                let replace = match &s.live_source_id {
                    None => true,
                    Some(cur) => standard(cur) && !standard(source_id),
                };
                if replace {
                    s.live_source_id = Some(source_id.clone());
                    let v = serde_json::to_value(&m).unwrap_or_default();
                    s.live_source = Some(v.clone());
                    out.push(json!({ "type": "source", "source": v }));
                }
            }
            // A device that matches several tables (an FTMS machine: bike, rower and treadmill data; a trainer
            // with Speed and Cadence and Cycling Power) shows the one that actually sends values, while the one
            // shown has sent none.
            Message::Values { source_id, fields, .. } if s.live_source_id.as_ref() != Some(source_id) && s.live_values.is_empty() && !fields.is_empty() => {
                if let Some(v) = s.live_sources.get(source_id).cloned() {
                    s.live_source_id = Some(source_id.clone());
                    s.live_source = Some(v.clone());
                    out.push(json!({ "type": "source", "source": v }));
                    s.live_values.extend(values_of(fields));
                    out.push(json!({ "type": "values", "t": 0, "fields": fields, "out_of_range": [], "stale": false }));
                }
            }
            Message::Values { source_id, fields, out_of_range, stale, t, .. } if s.live_source_id.as_ref() == Some(source_id) => {
                s.live_values.extend(values_of(fields));
                for k in out_of_range {
                    s.live_values.remove(k);
                }
                out.push(json!({ "type": "values", "t": t, "fields": fields, "out_of_range": out_of_range, "stale": stale }));
            }
            _ => {}
        }
    }
}

fn session_json(s: &Session) -> Value {
    let (mut notes, mut confirms) = (0, 0);
    for l in &s.lines {
        match l {
            Line::Note { .. } => notes += 1,
            Line::Confirm { .. } => confirms += 1,
            _ => {}
        }
    }
    json!({
        "name": s.name, "device_id": s.device_id, "from_file": s.from_file, "recording": s.recording, "connect": s.connect,
        "file": s.path.as_ref().map(|p| p.display().to_string()), "company_id": s.company_id(),
        "streams": s.stream_keys().into_iter().map(|(k, n)| json!({ "key": k, "packets": n })).collect::<Vec<_>>(),
        "notes": notes, "confirms": confirms, "duration_ms": s.lines.last().map(|l| l.t()).unwrap_or(0),
        "draft": s.draft_yaml, "design": s.design, "draft_edited": s.draft_edited, "live": { "source": s.live_source, "values": s.live_values },
    })
}

/// A string as a YAML double-quoted scalar.
fn yaml_text(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', " "))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Studio on a copy of the real registry, without the Keiser table (so the device is "new").
    fn studio(name: &str, keep_table: bool) -> (Studio, PathBuf, PathBuf) {
        let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../registry");
        let dir = std::env::temp_dir().join(format!("ofp-studio-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for sub in ["fields", "tables"] {
            std::fs::create_dir_all(dir.join("registry").join(sub)).unwrap();
        }
        for e in std::fs::read_dir(real.join("fields")).unwrap() {
            let p = e.unwrap().path();
            std::fs::copy(&p, dir.join("registry/fields").join(p.file_name().unwrap())).unwrap();
        }
        if keep_table {
            for e in std::fs::read_dir(real.join("tables")).unwrap() {
                let p = e.unwrap().path();
                std::fs::copy(&p, dir.join("registry/tables").join(p.file_name().unwrap())).unwrap();
            }
        }
        let capture = dir.join("ride.capture.jsonl");
        std::fs::copy(real.join("tables/keiser-m3i.capture.jsonl"), &capture).unwrap();
        (Studio::new(Config { registry: dir.join("registry"), captures: dir.join("captures") }), dir, capture)
    }

    fn keiser(power: u16, cadence10: u16, bike: u8) -> Vec<u8> {
        let mut p = vec![0x02, 0x01, 0x06, 0x40, 0x00, bike];
        p.extend_from_slice(&cadence10.to_le_bytes());
        p.extend_from_slice(&[0, 0]);
        p.extend_from_slice(&power.to_le_bytes());
        p.extend_from_slice(&[0, 0, 0, 5, 0, 0x80, 11]);
        p
    }

    fn adv(mfr: Vec<u8>) -> AdvIn {
        AdvIn { device_id: "dev-1".into(), name: Some("M3".into()), rssi: Some(-58), mfr, ..Default::default() }
    }

    const TABLE: &str = r#"{"name":"Keiser M3i","id":"keiser-m3i","only_when":"u8[2] == 0 && u8[3] != 0","instance_label":"bike","instance_expr":"u8[3]",
        "mappings":[{"field":"power","expr":"u16le[8]"},{"field":"cadence","expr":"u16le[4] / 10"},{"field":"gear","expr":"u8[16]"}]}"#;

    #[test]
    fn from_a_capture_file_to_a_saved_table() {
        let (s, dir, capture) = studio("file", false);
        s.open_capture(&capture).unwrap();
        // nothing in this registry reads the device yet
        let similar = s.similar().unwrap();
        assert_eq!(similar["existing_tables"].as_array().unwrap().len(), 0);
        // a reading over the capture
        let series = s.series(None, "u16le[8]").unwrap();
        assert_eq!(series["max"], 346.0);
        assert!(s.series(None, "u16le[8").is_err());
        // the design becomes the shared draft
        let draft = s.set_design(serde_json::from_str(TABLE).unwrap()).unwrap();
        let yaml = draft["yaml"].as_str().unwrap().to_string();
        assert!(yaml.contains("manufacturer_id: 0x0102") && yaml.contains("min_length: 17") && yaml.contains("power: \"u16le[8]\""), "{yaml}");
        assert_eq!(s.state_json()["session"]["draft"], yaml);
        // The page shows the version and compares it with the newest release
        assert_eq!(s.state_json()["version"], env!("CARGO_PKG_VERSION"));
        // the check CI would make
        let checked = s.check_table(&yaml).unwrap();
        assert_eq!(checked["ok"], true, "{checked}");
        assert_eq!(checked["decoded"], 621);
        assert_eq!(checked["sources"][0], "keiser-m3i:197");
        assert!(checked["series"]["power"].as_array().unwrap().len() > 100);
        // a table that does not compile says why, and one that decodes nothing does not pass
        assert_eq!(s.check_table(&yaml.replace("u16le[8]", "u16le[8")).unwrap()["compiled"], false);
        assert_eq!(s.check_table(&yaml.replace("0x0102", "0x9999")).unwrap()["ok"], false);
        // save: both files written, and the whole registry checks
        let saved = s.save(&yaml, false).unwrap();
        assert_eq!(saved["ok"], true, "{saved}");
        assert!(dir.join("registry/tables/keiser-m3i.yaml").exists() && dir.join("registry/tables/keiser-m3i.capture.jsonl").exists());
        assert!(check::run(&dir.join("registry"), false).errors.is_empty());
        // now the registry knows the device: extend, do not add
        let similar = s.similar().unwrap();
        assert_eq!(similar["existing_tables"][0]["table"], "keiser-m3i");
        assert!(similar["advice"].as_str().unwrap().contains("Extend"));
        // saving again asks before replacing, and a second capture gets its own file
        let again = s.save(&yaml, false).unwrap();
        assert_eq!((again["ok"].as_bool(), again["exists"].as_bool()), (Some(false), Some(true)));
        assert_eq!(s.save(&yaml, true).unwrap()["ok"], true);
        let captures = std::fs::read_dir(dir.join("registry/tables")).unwrap().filter(|e| e.as_ref().unwrap().path().to_string_lossy().ends_with(".capture.jsonl")).count();
        assert_eq!(captures, 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_save_that_breaks_the_registry_is_undone() {
        let (s, dir, capture) = studio("undo", true);
        s.open_capture(&capture).unwrap();
        let existing = std::fs::read_to_string(dir.join("registry/tables/keiser-m3i.yaml")).unwrap();
        // a table that passes this capture but breaks the registry's own vectors (power read from the wrong scale for them)
        let broken = existing.replace("gear:\n    value: u8[16]\n    only_when: u8[1] >= 0x21", "gear: u8[16]");
        assert_ne!(broken, existing);
        let r = s.save(&broken, true).unwrap();
        assert_eq!(r["ok"], false, "{r}");
        assert_eq!(std::fs::read_to_string(dir.join("registry/tables/keiser-m3i.yaml")).unwrap(), existing);
        assert!(check::run(&dir.join("registry"), false).errors.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn recording_notes_live_values_and_confirmations() {
        let (s, dir, _) = studio("live", false);
        let mut events = s.events.subscribe();
        s.on_adv(adv(keiser(150, 820, 197)));
        let devices = s.devices_json();
        assert_eq!(devices[0]["name"], "M3");
        assert_eq!(devices[0]["has_data"], true);
        assert!(devices[0]["known"].is_null());
        s.open_device("dev-1").unwrap();
        assert!(s.note("power=150").is_err(), "notes need a recording");
        s.set_design(serde_json::from_str(TABLE).unwrap()).unwrap();
        let (_, connect_to) = s.record_start(None).unwrap();
        assert_eq!(connect_to, None, "a broadcast device is not connected to");
        s.on_adv(adv(keiser(150, 820, 197)));
        s.on_adv(AdvIn { device_id: "other".into(), mfr: keiser(999, 0, 3), ..Default::default() });
        s.note("power=151 cadence=82 felt steady").unwrap();
        s.on_adv(adv(keiser(152, 825, 197)));
        // live values come from the table being written
        let st = s.state_json();
        assert_eq!(st["session"]["live"]["source"]["source_id"], "keiser-m3i:197");
        assert_eq!(st["session"]["live"]["values"]["power"], 152.0);
        // confirmations: one right, one wrong
        assert_eq!(s.confirm("power", None, false).unwrap()["ok"], true);
        assert_eq!(s.confirm("gear", Some(12.0), false).unwrap()["ok"], false);
        assert!(s.confirm("heart_rate", None, false).is_err());
        assert!(s.save("x", false).is_err(), "not while recording");
        s.record_stop().unwrap();
        let first_file = s.state_json()["session"]["file"].as_str().unwrap().to_string();
        // the capture holds only the chosen device, the note and both confirmations
        let c = s.capture_json().unwrap();
        assert_eq!(c["packets"].as_array().unwrap().len(), 2);
        assert_eq!(c["packets"][0][2].as_str().unwrap().len(), 34, "payload after the company id");
        assert_eq!((c["notes"].as_array().unwrap().len(), c["confirms"].as_array().unwrap().len()), (1, 2));
        // the file on disk is a capture the checker reads
        let file = PathBuf::from(c["session"]["file"].as_str().unwrap());
        let lines = ofp_capture::read_file(&file).unwrap();
        assert_eq!(lines.len(), 6);
        // the rider said gear is wrong: the check fails until the table decodes 12
        let yaml = s.state_json()["session"]["draft"].as_str().unwrap().to_string();
        let checked = s.check_table(&yaml).unwrap();
        assert_eq!(checked["ok"], false);
        assert!(checked["problems"][0].as_str().unwrap().contains("marked gear wrong"), "{checked}");
        assert_eq!(s.check_table(&yaml.replace("u8[16]", "u8[16] + 1")).unwrap()["ok"], true);
        // the solver works from the note
        let solved = s.solve(None).unwrap();
        assert_eq!(solved["fields"]["power"]["candidates"][0]["expr"], "u8[8]", "two packets under 256 W: one byte is all they show");
        // starting again is a new recording in a new file; the first one stays as it was
        std::thread::sleep(std::time::Duration::from_millis(1100));
        s.record_start(None).unwrap();
        s.on_adv(adv(keiser(160, 830, 197)));
        s.record_stop().unwrap();
        let again = s.capture_json().unwrap();
        assert_eq!(again["packets"].as_array().unwrap().len(), 1);
        assert_ne!(again["session"]["file"].as_str().unwrap(), first_file);
        assert_eq!(ofp_capture::read_file(Path::new(&first_file)).unwrap().len(), 6);
        assert_eq!(s.captures_json().as_array().unwrap().len(), 2, "both are in the history");
        // everything the page needs was announced
        let mut kinds = BTreeSet::new();
        while let Ok(e) = events.try_recv() {
            kinds.insert(serde_json::from_str::<Value>(&e).unwrap()["type"].as_str().unwrap().to_string());
        }
        for k in ["session", "draft", "recording", "packet", "source", "values", "note", "confirm", "checked"] {
            assert!(kinds.contains(k), "no '{k}' event in {kinds:?}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_bike_that_needs_start_and_poll_frames_gets_them_written_after_connecting() {
        use ofp_capture::{GattCharacteristic, GattService};
        let (s, dir, _) = studio("polled", true);
        let yaml = "id: polled-bike\nname: Polled bike\nreader: notify\nversion: 1\nstatus: draft\nprovenance: test\nverified_on: [test]\nmatch:\n  - name_prefix: POLLTEST\nnotify:\n  service: fff0\n  characteristic: fff4\nstart:\n  service: fff0\n  characteristic: fff1\n  frames: [\"f0 a0 01\"]\npoll:\n  service: fff0\n  characteristic: fff1\n  frames: [\"f0 a1\", \"f0 a2\"]\n  every_ms: 500\nstale_after_ms: 5000\nfields:\n  power: u8[2]\n";
        std::fs::write(dir.join("registry/tables/polled-bike.yaml"), yaml).unwrap();
        s.reload_known();
        s.on_adv(AdvIn { device_id: "dev-p".into(), name: Some("POLLTEST 7".into()), rssi: Some(-50), ..Default::default() });
        s.open_device("dev-p").unwrap();
        s.record_start(None).unwrap();
        // nothing to write before the connection
        assert!(s.poll_tick().is_empty());
        let chr = |u: &str| GattCharacteristic { uuid: u.into(), properties: vec!["notify".into(), "write".into()] };
        s.on_connected(vec![GattService { uuid: "fff0".into(), characteristics: vec![chr("fff4"), chr("fff1")] }], vec![], vec!["fff4".into()]);
        let w = s.poll_tick();
        assert_eq!(w.first().map(|x| x.1.clone()), Some(vec![0xf0, 0xa0, 0x01]), "the start frame first: {w:?}");
        assert!(w.iter().any(|x| x.1 == vec![0xf0, 0xa1]), "then the first poll: {w:?}");
        assert!(w.iter().all(|x| x.0 == "fff1"));
        // the bike answers, and its answer is read
        s.on_notify("fff4", &[0, 0, 187]);
        assert_eq!(s.state_json()["session"]["live"]["values"]["power"], 187.0);
        s.record_stop().unwrap();
        assert!(s.poll_tick().is_empty(), "stopped: no more polls");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_connected_sensor_is_decoded_live_by_a_draft_and_a_notify_table_can_be_written_for_it() {
        use ofp_capture::{GattCharacteristic, GattService};
        let (s, dir, _) = studio("notify", true);
        const SERVICE: &str = "40b50000-30b5-11e5-a151-feff819cdc90";
        const CHR: &str = "40b50004-30b5-11e5-a151-feff819cdc90";
        // heard: a name, no data in the advert → the Studio will connect to it. The registry's draft knows it.
        s.on_adv(AdvIn { device_id: "dev-t".into(), name: Some("TYME-1A2B".into()), rssi: Some(-55), ..Default::default() });
        let listed = s.devices_json();
        assert_eq!(listed[0]["has_data"], false);
        assert!(listed[0]["known"].as_str().unwrap().starts_with("Tymewear VitalPro"), "{listed}");
        s.open_device("dev-t").unwrap();
        let (_, connect_to) = s.record_start(None).unwrap();
        assert_eq!(connect_to.as_deref(), Some("dev-t"));
        s.on_connected(vec![GattService { uuid: SERVICE.into(), characteristics: vec![GattCharacteristic { uuid: CHR.into(), properties: vec!["notify".into()] }] }], vec![], vec![CHR.into()]);
        // one breath: 15 breaths/min, 1.80 L, 27 L/min
        let breath = ofp_capture::unhex("01 10000000 9600 fa00 b400 b400 0000 0000").unwrap();
        s.on_notify(CHR, &breath);
        let st = s.state_json();
        assert_eq!(st["session"]["live"]["values"]["breathing_rate"], 15.0, "{}", st["session"]["live"]);
        assert_eq!(st["session"]["live"]["values"]["minute_ventilation"], 27.0);
        // the owner says the volume is wrong and what they saw
        assert_eq!(s.confirm("tidal_volume", Some(0.9), true).unwrap()["ok"], false);
        s.record_stop().unwrap();
        // a table of one's own for this stream: proposed as a notify table, checked against the recording
        let draft = s.set_design(serde_json::from_str(&format!(r#"{{"name":"My breathing sensor","stream":"{CHR}","only_when":"u8[0] == 1","mappings":[{{"field":"tidal_volume","expr":"u16le[9] / 200"}}]}}"#)).unwrap()).unwrap();
        let yaml = draft["yaml"].as_str().unwrap();
        assert!(yaml.contains("reader: notify") && yaml.contains(&format!("characteristic: \"{CHR}\"")) && yaml.contains(&format!("service: \"{SERVICE}\"")), "{yaml}");
        // 180 / 200 = 0.9: what the owner saw, so their "wrong" is answered
        let checked = s.check_table(yaml).unwrap();
        assert_eq!(checked["ok"], true, "{checked}");
        assert_eq!(checked["series"]["tidal_volume"][0][1], 0.9);
        // with the old scale the same recording does not pass
        assert_eq!(s.check_table(&yaml.replace("/ 200", "/ 100")).unwrap()["ok"], false);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_new_field_is_drafted_only_when_nothing_like_it_exists() {
        let (s, dir, _) = studio("field", false);
        let req = |key: &str, name: &str, unit: &str, confirm: bool| FieldReq { key: key.into(), name: name.into(), unit: unit.into(), ty: "decimal".into(), decimals: 1, min: 30.0, max: 45.0, kind: "instantaneous".into(), description: "Core body temperature right now.".into(), confirm_new: confirm };
        // "pedal power" is too close to power: offered, not added
        let alike = s.add_field(req("pedal_power", "Pedal power", "W", false)).unwrap();
        assert_eq!(alike["similar"][0]["key"], "power");
        assert!(!dir.join("registry/fields/pedal_power.yaml").exists());
        // "body temperature" now has close relatives too: the core and skin temperature fields are offered
        let alike = s.add_field(req("body_temperature", "Body temperature", "°C", false)).unwrap();
        assert!(alike["similar"].as_array().unwrap().iter().any(|f| f["key"] == "core_temperature"), "{alike}");
        // a jump height is new (a rep count now has a relative: the rower's stroke count)
        let added = s.add_field(req("jump_height", "Jump height", "cm", false)).unwrap();
        assert_eq!(added["key"], "jump_height");
        assert!(s.state_json()["registry"]["fields"].as_array().unwrap().iter().any(|f| f["key"] == "jump_height" && f["tier"] == "experimental"));
        assert!(s.add_field(req("jump_height", "Jump height", "cm", true)).is_err(), "no duplicates");
        assert!(s.add_field(req("Bad Key", "Something else entirely", "zz", true)).is_err());
        assert!(!dir.join("registry/fields/Bad Key.yaml").exists());
        assert!(check::run(&dir.join("registry"), false).errors.iter().all(|e| !e.contains("fields/")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_device_the_registry_knows_is_marked_in_the_list() {
        let (s, dir, _) = studio("known", true);
        s.on_adv(adv(keiser(150, 820, 197)));
        assert_eq!(s.devices_json()[0]["known"], "Keiser M3i · bike 197");
        let _ = std::fs::remove_dir_all(dir);
    }
}
