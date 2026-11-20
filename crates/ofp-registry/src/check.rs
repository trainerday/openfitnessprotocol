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
    manufacturer_data: String,
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
        let data = match unhex(&v.manufacturer_data) {
            Ok(d) => d,
            Err(e) => {
                errors.push(format!("{at}: {e}"));
                continue;
            }
        };
        let mut core = reg.core();
        let messages = core.on_advertisement(&Advertisement { device_id: "vector", name: v.device_name.as_deref(), rssi: None, manufacturer_data: &data, services: &[], now_ms: 0 });
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
        let dir = std::env::temp_dir().join(format!("ofp-check-{name}-{}", std::process::id()));
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
    fn the_core_draft_decodes_a_notification_built_from_the_makers_specification() {
        let reg = registry::load(&registry_dir()).unwrap();
        assert!(reg.drafts.contains("core-body-temperature-connected"));
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
        // the strap's heart-rate pod is not this table's
        let pod = connected_capture("TymeHR 55", "180d", "2a37", &["0048"], &[]);
        assert_eq!(check_lines(&reg, None, "test", &pod).decoded, 0);
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
