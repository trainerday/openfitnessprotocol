//! `ofp check`: everything a pull request must pass.

use crate::registry::{self, Lock, Registry};
use crate::replay::{replay, values_of, Decoded};
use ofp_capture::{unhex, Line};
use ofp_core::{Advertisement, Message, Tier};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Expected {
    source_id: String,
    #[serde(default)]
    fields: BTreeMap<String, f64>,
    /// Fields that must not be in the values.
    #[serde(default)]
    absent: Vec<String>,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum Expect {
    /// `expect: none` — the packet must produce no values.
    Word(String),
    Values(Expected),
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Vector {
    name: String,
    #[serde(default)]
    device_name: Option<String>,
    /// An advertisement's manufacturer data (broadcast tables).
    #[serde(default)]
    manufacturer_data: String,
    /// One notification from the table's characteristic (notify tables): the device is announced,
    /// used and connected first, as an app does.
    #[serde(default)]
    notification: Option<String>,
    expect: Expect,
}

pub struct Report {
    pub errors: Vec<String>,
    pub lines: Vec<String>,
}

fn table_files(dir: &Path, table: &str, suffix: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir.join("tables"))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            n.ends_with(suffix) && (n == format!("{table}{suffix}") || n.starts_with(&format!("{table}.")))
        })
        .collect();
    v.sort();
    v
}

fn check_vectors(reg: &Registry, table: &str, path: &Path, errors: &mut Vec<String>) -> usize {
    let shown = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
    let vectors: Vec<Vector> = match std::fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|t| serde_yaml_ng::from_str(&t).map_err(|e| e.to_string())) {
        Ok(v) => v,
        Err(e) => {
            errors.push(format!("{shown}: {e}"));
            return 0;
        }
    };
    for v in &vectors {
        let at = format!("{shown}: '{}'", v.name);
        let data = match unhex(v.notification.as_deref().unwrap_or(&v.manufacturer_data)) {
            Ok(d) => d,
            Err(e) => {
                errors.push(format!("{at}: {e}"));
                continue;
            }
        };
        let mut core = reg.core();
        let messages = match &v.notification {
            None => core.on_advertisement(&Advertisement { device_id: "vector", name: v.device_name.as_deref(), rssi: None, manufacturer_data: &data, services: &[], now_ms: 0 }),
            Some(_) => {
                let Some(spec) = reg.bundle.tables.iter().find(|t| t.id == table).and_then(|t| t.notify.clone()) else {
                    errors.push(format!("{at}: a notification vector needs a notify table"));
                    continue;
                };
                let services = vec![spec.service.clone()];
                core.on_advertisement(&Advertisement { device_id: "vector", name: v.device_name.as_deref(), rssi: None, manufacturer_data: &[], services: &services, now_ms: 0 });
                if let Err(e) = core.use_source(&format!("{table}:vector"), 0) {
                    errors.push(format!("{at}: {e}"));
                    continue;
                }
                core.on_connected("vector", 0);
                core.on_notification("vector", &spec.characteristic, &data, 1000)
            }
        };
        let values = messages.iter().find_map(|m| match m {
            Message::Values { source_id, fields, .. } => Some((source_id.clone(), values_of(fields))),
            _ => None,
        });
        match (&v.expect, values) {
            (Expect::Word(w), got) => {
                if w != "none" {
                    errors.push(format!("{at}: expect must be 'none' or a list of values"));
                } else if let Some((id, _)) = got {
                    errors.push(format!("{at}: expected no values, but {id} decoded it"));
                }
            }
            (Expect::Values(_), None) => errors.push(format!("{at}: nothing was decoded")),
            (Expect::Values(exp), Some((id, got))) => {
                if !id.starts_with(&format!("{table}:")) {
                    errors.push(format!("{at}: decoded by {id}, not by table {table}"));
                }
                if id != exp.source_id {
                    errors.push(format!("{at}: source is {id}, expected {}", exp.source_id));
                }
                for (k, want) in &exp.fields {
                    match got.get(k) {
                        Some(have) if (have - want).abs() < 1e-6 => {}
                        Some(have) => errors.push(format!("{at}: {k} is {have}, expected {want}")),
                        None => errors.push(format!("{at}: {k} is missing, expected {want}")),
                    }
                }
                for k in &exp.absent {
                    if let Some(have) = got.get(k) {
                        errors.push(format!("{at}: {k} should be absent but is {have}"));
                    }
                }
            }
        }
    }
    vectors.len()
}

/// Decoded values of one field around a moment: a person reads a console, then types.
fn around<'a>(timeline: &'a [Decoded], field: &'a str, t: u64) -> impl Iterator<Item = f64> + 'a {
    timeline.iter().filter(move |d| !d.stale && d.t + 8000 >= t && d.t <= t + 2000).filter_map(move |d| d.fields.get(field).copied())
}

pub struct CaptureResult {
    pub summary: String,
    pub errors: Vec<String>,
    /// Everything the table decoded, in order.
    pub timeline: Vec<Decoded>,
    pub decoded: usize,
    pub adverts: usize,
}

fn failed(error: String) -> CaptureResult {
    CaptureResult { summary: String::new(), errors: vec![error], timeline: Vec::new(), decoded: 0, adverts: 0 }
}

/// Replay one capture file and judge it against the notes and confirmations inside it.
pub fn check_capture(reg: &Registry, table: Option<&str>, path: &Path) -> CaptureResult {
    let shown = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
    match ofp_capture::read_file(path) {
        Ok(lines) => check_lines(reg, table, &shown, &lines),
        Err(e) => failed(e),
    }
}

/// The same for a capture already in memory. `shown` is its name in messages.
pub fn check_lines(reg: &Registry, table: Option<&str>, shown: &str, lines: &[Line]) -> CaptureResult {
    let mut errors = Vec::new();
    let mut core = reg.core();
    let c = match replay(&mut core, lines) {
        Ok(c) => c,
        Err(e) => return failed(format!("{shown}: {e}")),
    };
    if c.stats.decoded == 0 {
        errors.push(format!("{shown}: no packet was decoded"));
    }
    if let Some(table) = table {
        for id in c.stats.sources.keys() {
            if !id.starts_with(&format!("{table}:")) {
                errors.push(format!("{shown}: decoded by {id}, not by table {table}"));
            }
        }
    }
    let (mut notes, mut confirmed, mut refuted) = (0, 0, 0);
    for line in lines {
        match line {
            Line::Note { t, values, .. } => {
                for (key, console) in values {
                    if reg.bundle.field(key).is_none() {
                        continue; // free information, not a field
                    }
                    notes += 1;
                    let tol = reg.tolerance(key);
                    let seen: Vec<f64> = around(&c.timeline, key, *t).collect();
                    if seen.is_empty() {
                        errors.push(format!("{shown}: at {:.0} s the rider noted {key}={console}, but nothing was decoded then", *t as f64 / 1000.0));
                    } else if !seen.iter().any(|d| tol.allows(*d, *console)) {
                        let nearest = seen.iter().copied().min_by(|a, b| (a - console).abs().total_cmp(&(b - console).abs())).unwrap();
                        errors.push(format!("{shown}: at {:.0} s the rider noted {key}={console}, the table decodes {nearest}", *t as f64 / 1000.0));
                    }
                }
            }
            Line::Confirm { t, source_id, field, shown: value, console, ok } => {
                if *ok {
                    confirmed += 1;
                    let now = c.timeline.iter().rev().find(|d| d.t <= *t && d.source_id == *source_id && d.fields.contains_key(field)).and_then(|d| d.fields.get(field));
                    match now {
                        Some(d) if (d - value).abs() < 1e-6 => {}
                        Some(d) => errors.push(format!("{shown}: the rider confirmed {field}={value} at {:.0} s, the table now decodes {d}", *t as f64 / 1000.0)),
                        None => errors.push(format!("{shown}: the rider confirmed {field}={value} at {:.0} s, the table now decodes nothing", *t as f64 / 1000.0)),
                    }
                } else {
                    refuted += 1;
                    // A "wrong" stays open until the rider later says the same value is right (they looked
                    // again after the table was changed), or the table now decodes what the display showed.
                    let later_right = lines.iter().any(|l| matches!(l, Line::Confirm { t: t2, source_id: s2, field: f2, ok: true, .. } if t2 > t && f2 == field && s2.split(':').next() == source_id.split(':').next()));
                    let tol = reg.tolerance(field);
                    let fixed = console.is_some_and(|console| around(&c.timeline, field, *t).any(|d| tol.allows(d, console)));
                    if !later_right && !fixed {
                        let console = console.map(|c| format!("the console showed {c}")).unwrap_or_else(|| "no console value was given".into());
                        errors.push(format!("{shown}: the rider marked {field} wrong at {:.0} s (OFP showed {value}, {console})", *t as f64 / 1000.0));
                    }
                }
            }
            _ => {}
        }
    }
    let s = &c.stats;
    let packets = s.adverts + s.notifications;
    let mins = (s.duration_ms as f64 / 60000.0).max(1.0 / 60.0);
    let mut summary = format!(
        "{shown}: {} packets in {:.1} min ({:.0}/min, longest gap {:.1} s), {} decoded ({:.0} %), {} noted values, {} confirmed, {} marked wrong",
        packets,
        mins,
        packets as f64 / mins,
        s.longest_gap_ms as f64 / 1000.0,
        s.decoded,
        if packets > 0 { 100.0 * s.decoded as f64 / packets as f64 } else { 0.0 },
        notes,
        confirmed,
        refuted
    );
    for (k, n) in &s.out_of_range {
        summary.push_str(&format!("; {k} out of range {n}x"));
    }
    let (decoded, adverts) = (c.stats.decoded, c.stats.adverts + c.stats.notifications);
    CaptureResult { summary, errors, timeline: c.timeline, decoded, adverts }
}

pub fn run(dir: &Path, accept_stable: bool) -> Report {
    let mut lines = Vec::new();
    let reg = match registry::load(dir) {
        Ok(r) => r,
        Err(errors) => {
            // Accepting new stable fields is the one error a maintainer can clear here.
            if accept_stable && errors.iter().all(|e| e.contains("is not in fields.lock.json")) {
                match accept(dir) {
                    Ok(n) => return Report { errors: vec![], lines: vec![format!("{n} stable field(s) written to fields.lock.json — run `ofp check` again")] },
                    Err(e) => return Report { errors: vec![e], lines },
                }
            }
            return Report { errors, lines };
        }
    };
    let mut errors = Vec::new();
    lines.push(format!("{} fields, {} tables", reg.bundle.fields.len(), reg.bundle.tables.len()));
    let tables: Vec<String> = reg.bundle.tables.iter().map(|t| t.id.clone()).collect();
    for table in &tables {
        let mut evidence = 0;
        for path in table_files(dir, table, ".vectors.yaml") {
            let n = check_vectors(&reg, table, &path, &mut errors);
            lines.push(format!("{}: {n} vectors", path.file_name().and_then(|n| n.to_str()).unwrap_or("")));
            evidence += n;
        }
        let captures = table_files(dir, table, ".capture.jsonl");
        for path in &captures {
            let r = check_capture(&reg, Some(table), path);
            if !r.summary.is_empty() {
                lines.push(r.summary);
            }
            errors.extend(r.errors);
            evidence += 1;
        }
        if reg.drafts.contains(table) {
            lines.push(format!("draft: table {table} is not verified by its owner yet (it is left out of the bundle apps get)"));
        } else if evidence == 0 {
            errors.push(format!("table {table}: no capture and no vectors — every table ships with evidence (or is marked `status: draft`)"));
        } else if captures.is_empty() {
            lines.push(format!("note: table {table} has no real capture yet"));
        }
    }
    Report { errors, lines }
}

/// Freeze fields newly marked stable into the lock file.
fn accept(dir: &Path) -> Result<usize, String> {
    // Load without the lock check by reading the field files again through the bundle of a
    // registry whose only complaint was the missing lock entries.
    let lock_path = dir.join("fields.lock.json");
    let mut lock: Lock = std::fs::read_to_string(&lock_path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let mut added = 0;
    for entry in std::fs::read_dir(dir.join("fields")).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let f: ofp_core::FieldDef = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&text)
            .map_err(|e| e.to_string())
            .and_then(|mut v| {
                if let Some(m) = v.as_mapping_mut() {
                    m.remove("tolerance");
                    m.remove("description");
                }
                serde_yaml_ng::from_value(v).map_err(|e| e.to_string())
            })?;
        if f.tier == Tier::Stable && !lock.stable.iter().any(|l| l.id == f.id) {
            lock.stable.push(registry::locked_form(&f));
            added += 1;
        }
    }
    lock.stable.sort_by_key(|l| l.id);
    std::fs::write(&lock_path, serde_json::to_string_pretty(&lock).map_err(|e| e.to_string())? + "\n").map_err(|e| e.to_string())?;
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../registry")
    }

    /// A copy of the real registry with one text replaced in one file.
    fn altered(name: &str, file: &str, from: &str, to: &str) -> PathBuf {
        // One folder per call: tests run side by side, and two of them asking for the same name used to
        // share a folder (one deleting it while the other read it).
        static CALL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let call = CALL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("ofp-check-{name}-{}-{call}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for sub in ["fields", "tables"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
            for e in std::fs::read_dir(registry_dir().join(sub)).unwrap() {
                let p = e.unwrap().path();
                std::fs::copy(&p, dir.join(sub).join(p.file_name().unwrap())).unwrap();
            }
        }
        if let Ok(lock) = std::fs::read(registry_dir().join("fields.lock.json")) {
            std::fs::write(dir.join("fields.lock.json"), lock).unwrap();
        }
        let path = dir.join(file);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(from), "{file} does not contain {from}");
        std::fs::write(&path, text.replace(from, to)).unwrap();
        dir
    }

    fn problems(dir: &Path) -> Vec<String> {
        let r = run(dir, false);
        let _ = std::fs::remove_dir_all(dir);
        r.errors
    }

    #[test]
    fn the_registry_passes() {
        let r = run(&registry_dir(), false);
        assert!(r.errors.is_empty(), "{:#?}", r.errors);
    }

    #[test]
    fn a_wrong_byte_position_fails() {
        let e = problems(&altered("byte", "tables/keiser-m3i.yaml", "power: u16le[8]", "power: u16le[10]"));
        assert!(e.iter().any(|e| e.contains("power is 42, expected 187")), "{e:#?}");
    }

    #[test]
    fn a_wrong_scale_fails() {
        let e = problems(&altered("scale", "tables/keiser-m3i.yaml", "cadence: u16le[4] / 10", "cadence: u16le[4]"));
        assert!(e.iter().any(|e| e.contains("cadence")), "{e:#?}");
    }

    #[test]
    fn a_syntax_error_is_reported_with_the_file() {
        let e = problems(&altered("syntax", "tables/keiser-m3i.yaml", "power: u16le[8]", "power: u16le[8"));
        assert!(e.iter().any(|e| e.contains("keiser-m3i.yaml") && e.contains("field 'power'")), "{e:#?}");
    }

    #[test]
    fn an_unknown_field_fails() {
        let e = problems(&altered("field", "tables/keiser-m3i.yaml", "power: u16le[8]", "watts: u16le[8]"));
        assert!(e.iter().any(|e| e.contains("'watts' is not in registry/fields")), "{e:#?}");
    }

    #[test]
    fn a_duplicate_field_id_fails() {
        let e = problems(&altered("dup", "fields/cadence.yaml", "id: 2", "id: 1"));
        assert!(e.iter().any(|e| e.contains("id 1 is already used")), "{e:#?}");
    }

    #[test]
    fn a_table_without_provenance_fails() {
        let e = problems(&altered("prov", "tables/keiser-m3i.yaml", "verified_on:", "verified_elsewhere:"));
        assert!(!e.is_empty());
    }

    /// A capture of the shared vector (187 W, 84 rpm, gear 14) with what a rider typed.
    fn capture_with(lines: &[&str]) -> (PathBuf, PathBuf) {
        let dir = altered(&format!("cap{}", lines.concat().len()), "tables/keiser-m3i.yaml", "reader: broadcast", "reader: broadcast");
        let path = dir.join("tables/keiser-m3i.rider.capture.jsonl");
        let packet = |t: u64| format!(r#"{{"type":"packet","t":{t},"kind":"adv","name":"M3","mfr":"020106300007480 38c05bb002a000c2233800e"}}"#).replace(' ', "");
        let mut text = String::from(r#"{"type":"header","format":"ofp-capture","version":1,"tool":"test","host":"test","started":"2026-01-01T00:00:00Z","device":{"name":"M3","company_id":"0102"}}"#);
        text.push('\n');
        for t in [0u64, 400, 800, 1200, 1600, 2000] {
            text.push_str(&packet(t));
            text.push('\n');
        }
        for l in lines {
            text.push_str(l);
            text.push('\n');
        }
        std::fs::write(&path, text).unwrap();
        (dir, path)
    }

    #[test]
    fn a_riders_notes_and_confirmations_are_held_to() {
        // what the console showed, read by a person: close enough passes
        let (dir, _) = capture_with(&[
            r#"{"type":"note","t":2100,"text":"power=190 cadence=83 bike=7","values":{"power":190,"cadence":83,"bike":7}}"#,
            r#"{"type":"confirm","t":2200,"source_id":"keiser-m3i:7","field":"gear","shown":14,"ok":true}"#,
        ]);
        assert_eq!(problems(&dir), Vec::<String>::new());

        // a note far from what the table decodes fails
        let (dir, _) = capture_with(&[r#"{"type":"note","t":2100,"text":"power=250","values":{"power":250}}"#]);
        let e = problems(&dir);
        assert!(e.iter().any(|e| e.contains("noted power=250") && e.contains("decodes 187")), "{e:#?}");

        // "no, this is wrong" with nothing else fails; the same rider saying "yes" later clears it
        let (dir, _) = capture_with(&[r#"{"type":"confirm","t":1300,"source_id":"keiser-m3i:7","field":"power","shown":187,"ok":false}"#]);
        assert!(problems(&dir).iter().any(|e| e.contains("marked power wrong")));
        let (dir, _) = capture_with(&[
            r#"{"type":"confirm","t":1300,"source_id":"keiser-m3i:7","field":"power","shown":187,"ok":false}"#,
            r#"{"type":"confirm","t":2100,"source_id":"keiser-m3i:7","field":"power","shown":187,"ok":true}"#,
        ]);
        assert_eq!(problems(&dir), Vec::<String>::new());

        // a value the rider marked wrong fails until the table decodes what the console showed
        let (dir, _) = capture_with(&[r#"{"type":"confirm","t":2200,"source_id":"keiser-m3i:7","field":"power","shown":187,"console":230,"ok":false}"#]);
        let e = problems(&dir);
        assert!(e.iter().any(|e| e.contains("marked power wrong") && e.contains("console showed 230")), "{e:#?}");
    }

    #[test]
    fn a_confirmed_value_must_keep_decoding_the_same() {
        let (dir, _) = capture_with(&[r#"{"type":"confirm","t":2200,"source_id":"keiser-m3i:7","field":"cadence","shown":84,"ok":true}"#]);
        assert!(run(&dir, false).errors.is_empty());
        // someone later changes the scale
        let table = dir.join("tables/keiser-m3i.yaml");
        std::fs::write(&table, std::fs::read_to_string(&table).unwrap().replace("cadence: u16le[4] / 10", "cadence: u16le[4] / 5")).unwrap();
        let e = problems(&dir);
        assert!(e.iter().any(|e| e.contains("rider confirmed cadence=84") && e.contains("now decodes 168")), "{e:#?}");
    }

    /// A recording of a connected device, as the Studio writes it: header with the services found, then notifications.
    fn connected_capture(name: &str, service: &str, chr: &str, packets: &[&str], extra: &[&str]) -> Vec<Line> {
        let mut text = format!(
            r#"{{"type":"header","format":"ofp-capture","version":1,"tool":"test","host":"test","started":"2026-01-01T00:00:00Z","device":{{"name":"{name}"}},"gatt":[{{"uuid":"{service}","characteristics":[{{"uuid":"{chr}","properties":["notify"]}}]}}]}}"#
        );
        text.push('\n');
        for (i, p) in packets.iter().enumerate() {
            text.push_str(&format!(r#"{{"type":"packet","t":{},"kind":"notify","chr":"{chr}","data":"{}"}}"#, 1000 + i * 1000, p.replace(' ', "")));
            text.push('\n');
        }
        for l in extra {
            text.push_str(l);
            text.push('\n');
        }
        ofp_capture::read(text.as_bytes()).unwrap()
    }

    fn last_values(reg: &Registry, lines: &[Line]) -> (String, std::collections::BTreeMap<String, f64>) {
        let r = check_lines(reg, None, "test", lines);
        let d = r.timeline.iter().rev().find(|d| !d.stale).expect("something decoded");
        (d.source_id.clone(), d.fields.clone())
    }

    #[test]
    fn the_core_table_decodes_a_notification_built_from_the_makers_specification() {
        let reg = registry::load(&registry_dir()).unwrap();
        assert!(!reg.drafts.contains("core-body-temperature-connected"));
        // flags: skin, reserved, quality, heart rate and heat strain valid; core 37.25, skin 33.10, HR 142, HSI 2.3
        let lines = connected_capture("CORE 1234", "00002100-5b1e-4347-b07c-97b514dae121", "00002101-5b1e-4347-b07c-97b514dae121", &["37 8d0e ee0c 0000 24 8e 17"], &[]);
        let (source, v) = last_values(&reg, &lines);
        assert!(source.starts_with("core-body-temperature-connected:"));
        assert_eq!((v["core_temperature"], v["skin_temperature"], v["heart_rate"], v["heat_strain_index"]), (37.25, 33.1, 142.0, 2.3));
        // Fahrenheit (bit 3): 99.05 F is 37.25 C. No reading (0x7FFF) and invalid fields are left out.
        let (_, v) = last_values(&reg, &connected_capture("CORE", "00002100-5b1e-4347-b07c-97b514dae121", "00002101-5b1e-4347-b07c-97b514dae121", &["08 b126 0000 0000 00 00 00"], &[]));
        assert_eq!(v["core_temperature"], 37.25);
        assert!(!v.contains_key("skin_temperature") && !v.contains_key("heart_rate"));
        let r = check_lines(&reg, None, "test", &connected_capture("CORE", "00002100-5b1e-4347-b07c-97b514dae121", "00002101-5b1e-4347-b07c-97b514dae121", &["00 ff7f 0000 0000 00 00 00"], &[]));
        assert!(r.timeline.iter().all(|d| !d.fields.contains_key("core_temperature")));
    }

    /// A standard table, fed notifications 1 s apart.
    fn standard(reg: &Registry, service: &str, chr: &str, packets: &[&str]) -> std::collections::BTreeMap<String, f64> {
        last_values(reg, &connected_capture("Sensor", service, chr, packets, &[])).1
    }

    #[test]
    fn standard_heart_rate_8_and_16_bits() {
        let reg = registry::load(&registry_dir()).unwrap();
        assert_eq!(standard(&reg, "180d", "2a37", &["00 8c"])["heart_rate"], 140.0);
        // 16 bits (flag bit 0): 0x00b4 = 180
        assert_eq!(standard(&reg, "180d", "2a37", &["01 b400"])["heart_rate"], 180.0);
    }

    #[test]
    fn standard_cycling_power_finds_the_crank_after_the_optional_parts() {
        let reg = registry::load(&registry_dir()).unwrap();
        // crank only (bit 5): 250 W; crank 10 revolutions at 1024 ticks, then 11 at 2048 → 1 per second = 60 rpm
        let v = standard(&reg, "1818", "2a63", &["2000 fa00 0a00 0004", "2000 fa00 0b00 0008"]);
        assert_eq!((v["power"], v["cadence"]), (250.0, 60.0));
        // balance (bit 0), torque (bit 2), wheel (bit 4) and crank (bit 5): the crank moves 1 + 2 + 6 bytes on.
        // wheel: 2 turns in 1 s (2048 ticks of 1/2048 s) with a 2105 mm wheel = 15.2 km/h; crank: 90 rpm
        let v = standard(&reg, "1818", "2a63", &["3500 c800 32 1027 64000000 0008 1400 0004", "3500 c800 32 1027 66000000 0010 1500 aa06"]);
        assert_eq!((v["power"], v["speed"]), (200.0, 15.2));
        assert!((v["cadence"] - 90.0).abs() < 0.2, "{v:?}");
        // a packet without crank data says nothing about the cadence
        let v = standard(&reg, "1818", "2a63", &["0000 6400"]);
        assert_eq!(v["power"], 100.0);
        assert!(!v.contains_key("cadence"));
    }

    #[test]
    fn standard_speed_and_cadence_counts_across_the_wrap_and_stops() {
        let reg = registry::load(&registry_dir()).unwrap();
        // crank: 65535 → 1 (two revolutions) while its time goes 65000 → 488 (1024 ticks = 1 s): 120 rpm
        let v = standard(&reg, "1816", "2a5b", &["02 ffff e8fd", "02 0100 e801"]);
        assert_eq!(v["cadence"], 120.0);
        // the same crank event again and again: still 120 at first, 0 once 3 s have passed
        let lines = connected_capture("Sensor", "1816", "2a5b", &["02 0a00 0004", "02 0b00 0008", "02 0b00 0008", "02 0b00 0008", "02 0b00 0008", "02 0b00 0008"], &[]);
        let r = check_lines(&reg, None, "test", &lines);
        let cadence: Vec<f64> = r.timeline.iter().filter_map(|d| d.fields.get("cadence").copied()).collect();
        assert_eq!(cadence.first(), Some(&60.0));
        assert_eq!(cadence.last(), Some(&0.0), "{cadence:?}");
        // wheel only: 3 turns in 1 s, 2105 mm: 22.7 km/h
        let v = standard(&reg, "1816", "2a5b", &["01 0a000000 0004", "01 0d000000 0008"]);
        assert_eq!(v["speed"], 22.7);
    }

    #[test]
    fn standard_ftms_bike_reads_the_parts_its_flags_say_are_there() {
        let reg = registry::load(&registry_dir()).unwrap();
        // flags 0x0244: speed (bit 0 clear), cadence, power, heart rate.
        // 32.00 km/h, 90 rpm (180 halves), 250 W, 140 bpm
        let v = standard(&reg, "1826", "2ad2", &["4402 800c b400 fa00 8c"]);
        assert_eq!((v["speed"], v["cadence"], v["power"], v["heart_rate"]), (32.0, 90.0, 250.0, 140.0));
        // everything up to the elapsed time (flags 0x0bfe with bit 0 clear... speed, avg speed, cadence, avg
        // cadence, distance 1234 m, resistance 3.5, power 180, avg power, energy 55 kcal, heart rate 0 = no strap,
        // MET, elapsed 600 s)
        let v = standard(&reg, "1826", "2ad2", &["fe0f e803 e803 a000 a000 d20400 2300 b400 b400 3700 ffff ff 00 0a 5802"]);
        assert_eq!((v["speed"], v["cadence"], v["distance"], v["resistance"], v["power"], v["energy"], v["elapsed_time"]), (10.0, 80.0, 1234.0, 3.5, 180.0, 55.0, 600.0));
        assert!(!v.contains_key("heart_rate"), "0 = no strap");
        // "more data" (bit 0 set): no speed in this packet
        let v = standard(&reg, "1826", "2ad2", &["4100 fa00"]);
        assert_eq!(v["power"], 250.0);
        assert!(!v.contains_key("speed"));
    }

    #[test]
    fn standard_tables_are_left_to_an_app_that_reads_the_service_itself() {
        let reg = registry::load(&registry_dir()).unwrap();
        let mut core = ofp_core::Core::from_bundle(reg.bundle.clone()).unwrap();
        core.settings_mut().handled_services = Some(vec!["180D".into(), "00001826-0000-1000-8000-00805f9b34fb".into()]);
        let services = vec![String::from("180d"), String::from("1826"), String::from("1818")];
        let adv = ofp_core::Advertisement { device_id: "dev1", name: Some("KICKR"), rssi: Some(-50), manufacturer_data: &[], services: &services, now_ms: 0 };
        let offered: Vec<String> = core.on_advertisement(&adv).into_iter().filter_map(|m| match m { ofp_core::Message::SourceAppeared { source_id, .. } => Some(source_id), _ => None }).collect();
        assert!(offered.iter().all(|s| !s.starts_with("std-heart-rate") && !s.starts_with("std-ftms")), "{offered:?}");
        assert!(offered.iter().any(|s| s.starts_with("std-cycling-power")), "{offered:?}");
    }

    #[test]
    fn a_table_that_writes_to_a_control_point_is_refused() {
        let base = "id: x\nname: X\nreader: notify\nversion: 1\nstatus: draft\nprovenance: test\nverified_on: [test]\nmatch:\n  - name_prefix: X\nnotify:\n  service: fff0\n  characteristic: fff4\nstale_after_ms: 5000\nfields:\n  power: u8[0]\n";
        let dir = altered("writes", "tables/keiser-m3i.yaml", "reader: broadcast", "reader: broadcast");
        for (extra, ok) in [
            ("start:\n  service: fff0\n  characteristic: fff1\n  frames: [\"f0 a0 01\"]\npoll:\n  service: fff0\n  characteristic: fff1\n  frames: [\"f0 a1\", \"f0 a2\"]\n  every_ms: 500\n", true),
            ("start:\n  service: \"1826\"\n  characteristic: 2ad9\n  frames: [\"00\"]\n", false),
            ("poll:\n  service: fff0\n  characteristic: fff1\n  frames: [\"f0\"]\n  every_ms: 50\n", false),
            ("start:\n  service: fff0\n  characteristic: fff1\n  frames: [\"not hex\"]\n", false),
        ] {
            std::fs::write(dir.join("tables/x.yaml"), format!("{base}{extra}")).unwrap();
            let errors = run(&dir, false).errors;
            assert_eq!(errors.is_empty(), ok, "{extra}: {errors:?}");
        }
    }

    #[test]
    fn the_start_command_bikes_decode_their_specs_examples() {
        let reg = registry::load(&registry_dir()).unwrap();
        // older Yesoul: resistance 10, 85 rpm, 180 W
        let (source, v) = last_values(&reg, &connected_capture("YESOUL 12", "fff0", "fff4", &["0000 03e8 0a 00 55 00b4 000000"], &[]));
        assert!(source.starts_with("yesoul-classic-bike:"), "{source}");
        assert_eq!((v["resistance"], v["cadence"], v["power"]), (10.0, 85.0, 180.0));
        // Domyos, the 20-byte first half: 25.0 km/h, 90 rpm, 45 kcal, resistance 8, 140 bpm
        let svc = "49535343-fe7d-4ae5-8fa9-9fafd205e455";
        let chr = "49535343-1e4d-4bd9-ba61-23c647249616";
        let (source, v) = last_values(&reg, &connected_capture("Domyos-Bike-1234", svc, chr, &["f0bc 00000000 00fa 00 5a 002d 0064 08 000000 8c 00"], &[]));
        assert!(source.starts_with("domyos-bike:"), "{source}");
        assert_eq!((v["speed"], v["cadence"], v["energy"], v["resistance"], v["heart_rate"]), (25.0, 90.0, 45.0, 8.0, 140.0));
        // the answers to the start frames are not data
        assert_eq!(check_lines(&reg, None, "test", &connected_capture("Domyos-Bike-1234", svc, chr, &["f0c8 01 b9 0000000000000000000000000000"], &[])).decoded, 0);
        // iConsole+: each byte is 0-99 plus 1, pairs are high x 100 + low (the spec's example)
        let svc = "49535343-fe7d-4ae5-8fa9-9fafd205e455";
        let (source, v) = last_values(&reg, &connected_capture("IBIKING+ 22", svc, chr, &["f0b2 0101 06 03 0336 0156 0101 012b 0221 1001 09 0100"], &[]));
        assert!(source.starts_with("iconsole-bike:"), "{source}");
        assert_eq!((v["speed"], v["cadence"], v["energy"], v["heart_rate"], v["power"], v["resistance"]), (25.3, 85.0, 42.0, 132.0, 150.0, 8.0));
        // Nautilus: 85 rpm, 15.00 mph = 24.1 km/h, 180.00 W
        let (source, v) = last_values(&reg, &connected_capture("NAUTILUS B628", "b1f93401-d4bd-4839-b7d7-b7434e9656cc", "a46a4a80-9803-11e3-8f3c-0002a5d5c51b", &["00 55 00 dc05 00 5046 00000000 0000"], &[]));
        assert!(source.starts_with("nautilus-bike:"), "{source}");
        assert_eq!((v["cadence"], v["speed"], v["power"]), (85.0, 24.1, 180.0));
        // both declare their writes, none to a control point
        for id in ["yesoul-classic-bike", "domyos-bike"] {
            let t = reg.bundle.tables.iter().find(|t| t.id == id).unwrap();
            assert!(t.start.is_some(), "{id}");
        }
    }

    #[test]
    fn heart_rate_straps_send_rr_intervals_once_each() {
        use ofp_core::{Advertisement, Core, Message, Settings};
        let reg = registry::load(&registry_dir()).unwrap();
        let mut core = Core::from_bundle(reg.bundle.clone()).unwrap();
        *core.settings_mut() = Settings { repeat_ms: 1000, ..Default::default() };
        let services = vec![String::from("180d")];
        core.on_advertisement(&Advertisement { device_id: "strap", name: Some("HRM-Pro"), rssi: Some(-50), manufacturer_data: &[], services: &services, now_ms: 0 });
        core.use_source("std-heart-rate:strap", 0).unwrap();
        core.on_connected("strap", 0);
        let packet = |hex: &str, t: u64, core: &mut Core| -> serde_json::Map<String, serde_json::Value> {
            let bytes = ofp_capture::unhex(hex).unwrap();
            match core.on_notification("strap", "2a37", &bytes, t).into_iter().next() {
                Some(Message::Values { fields, .. }) => fields.into_iter().collect(),
                _ => serde_json::Map::new(),
            }
        };
        // 8-bit heart rate 72, no RR
        let v = packet("00 48", 1000, &mut core);
        assert_eq!(v["heart_rate"], 72.0);
        assert!(!v.contains_key("rr_interval"));
        // RR present (bit 4): one value, 832/1024 s = 812.5 ms
        let v = packet("10 48 4003", 2000, &mut core);
        assert_eq!(v["rr_interval"], serde_json::json!([812.5]));
        // energy (bit 3) before three RR values: 16-bit heart rate 150 (bit 0)
        let v = packet("19 9600 2c01 9001 a401 b801", 3000, &mut core);
        assert_eq!(v["heart_rate"], 150.0);
        assert_eq!(v["rr_interval"], serde_json::json!([390.6, 410.2, 429.7]));
        // a repeat of the standing values brings the heart rate again, never the RR
        let repeats: Vec<_> = core.tick(4500).into_iter().filter_map(|m| match m { Message::Values { repeat: true, fields, .. } => Some(fields), _ => None }).collect();
        assert!(!repeats.is_empty() && repeats.iter().all(|f| f.contains_key("heart_rate") && !f.contains_key("rr_interval")), "{repeats:?}");
        // gone silent: zeros for the heart rate, no list
        let zeros: Vec<_> = core.tick(20_000).into_iter().filter_map(|m| match m { Message::Values { stale: true, fields, .. } => Some(fields), _ => None }).collect();
        assert!(zeros.iter().all(|f| !f.contains_key("rr_interval")));
        // 17 values: at most 16 are taken
        let many = format!("10 48 {}", "0004 ".repeat(17));
        assert_eq!(packet(&many, 21_000, &mut core)["rr_interval"].as_array().unwrap().len(), 16);
    }

    #[test]
    fn a_repeating_part_must_be_last_and_read_by_a_list_field() {
        let dir = altered("lists", "tables/keiser-m3i.yaml", "reader: broadcast", "reader: broadcast");
        let base = "id: x\nname: X\nreader: notify\nversion: 1\nstatus: draft\nprovenance: test\nverified_on: [test]\nmatch:\n  - name_prefix: X\nnotify:\n  service: fff0\n  characteristic: fff4\nstale_after_ms: 5000\n";
        for (rest, ok) in [
            ("layout:\n  start: 1\n  parts:\n    - { name: hr, type: u8 }\n    - { name: rr, type: u16le, repeat: true }\nfields:\n  heart_rate: hr\n  rr_interval: rr\n", true),
            ("layout:\n  start: 1\n  parts:\n    - { name: rr, type: u16le, repeat: true }\n    - { name: hr, type: u8 }\nfields:\n  heart_rate: hr\n  rr_interval: rr\n", false),
            ("layout:\n  start: 1\n  parts:\n    - { name: rr, type: u16le, repeat: true }\nfields:\n  heart_rate: rr\n", false),
            ("fields:\n  rr_interval: u16le[1]\n", false),
        ] {
            std::fs::write(dir.join("tables/x.yaml"), format!("{base}{rest}")).unwrap();
            let errors = run(&dir, false).errors;
            assert_eq!(errors.is_empty(), ok, "{rest}: {errors:?}");
        }
    }

    #[test]
    fn the_tymewear_draft_decodes_a_breath_and_skips_the_other_packets() {
        let reg = registry::load(&registry_dir()).unwrap();
        // a breath: inhale 1.50 s, exhale 2.50 s, raw tidal volume 180 → 15 breaths/min, 1.80 L, 27 L/min
        let breath = "01 10000000 9600 fa00 b400 b400 0000 0000";
        let movement = "02 10000000 0000 0000 0000 0000";
        let lines = connected_capture("TYME-1A2B", "40b50000-30b5-11e5-a151-feff819cdc90", "40b50004-30b5-11e5-a151-feff819cdc90", &[movement, breath, movement], &[]);
        let r = check_lines(&reg, None, "test", &lines);
        assert_eq!(r.decoded, 1, "only the breath packet");
        let (source, v) = last_values(&reg, &lines);
        assert!(source.starts_with("tymewear-vitalpro:"));
        assert_eq!((v["breathing_rate"], v["tidal_volume"], v["minute_ventilation"]), (15.0, 1.8, 27.0));
        // the strap's heart-rate pod is not this table's: the standard Heart Rate table reads it
        let pod = connected_capture("TymeHR 55", "180d", "2a37", &["0048"], &[]);
        let (source, v) = last_values(&reg, &pod);
        assert!(source.starts_with("std-heart-rate:"), "{source}");
        assert_eq!(v["heart_rate"], 72.0);
    }

    #[test]
    fn an_owners_verdict_on_a_connected_sensor_is_held_to_like_any_other() {
        let reg = registry::load(&registry_dir()).unwrap();
        let wrong = r#"{"type":"confirm","t":1500,"source_id":"tymewear-vitalpro:live","field":"tidal_volume","shown":1.8,"console":0.9,"ok":false}"#;
        let lines = connected_capture("TYME-1A2B", "40b50000-30b5-11e5-a151-feff819cdc90", "40b50004-30b5-11e5-a151-feff819cdc90", &["01 10000000 9600 fa00 b400 b400 0000 0000"], &[wrong]);
        let r = check_lines(&reg, None, "test", &lines);
        assert!(r.errors.iter().any(|e| e.contains("marked tidal_volume wrong") && e.contains("console showed 0.9")), "{:#?}", r.errors);
    }

    #[test]
    fn drafts_are_left_out_of_the_bundle_apps_get() {
        let reg = registry::load(&registry_dir()).unwrap();
        let bundle: ofp_core::Bundle = serde_json::from_str(&reg.bundle_json()).unwrap();
        assert!(bundle.tables.iter().any(|t| t.id == "keiser-m3i"));
        assert!(bundle.tables.iter().all(|t| !reg.drafts.contains(&t.id)));
        assert!(reg.bundle.tables.len() > bundle.tables.len(), "the Studio still loads them");
    }

    #[test]
    fn stable_fields_are_append_only() {
        // Mark power stable and accept it, then change its unit.
        let dir = altered("stable", "fields/power.yaml", "tier: experimental", "tier: stable");
        assert!(run(&dir, false).errors.iter().any(|e| e.contains("is not in fields.lock.json")));
        assert!(run(&dir, true).errors.is_empty());
        assert!(run(&dir, false).errors.is_empty());
        let path = dir.join("fields/power.yaml");
        std::fs::write(&path, std::fs::read_to_string(&path).unwrap().replace("unit: W", "unit: kW")).unwrap();
        let e = problems(&dir);
        assert!(e.iter().any(|e| e.contains("stable field 'power' (id 1) was changed")), "{e:#?}");
    }
}
