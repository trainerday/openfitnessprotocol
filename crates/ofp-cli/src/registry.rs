//! Reads the registry (fields and tables as YAML) and compiles it into the bundle the core loads.

use crate::exprparse;
use ofp_core::{Bundle, Expr, FieldDef, FieldType, Instance, MatchRule, SampleKind, Table, TableField, Tier, BUNDLE_VERSION};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};


#[derive(Deserialize, Clone, Copy, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct Tolerance {
    #[serde(default)]
    pub abs: f64,
    #[serde(default)]
    pub pct: f64,
}

impl Tolerance {
    /// How far a value read off a console by a person may be from the decoded value.
    pub fn allows(&self, decoded: f64, console: f64) -> bool {
        (decoded - console).abs() <= self.abs.max(console.abs() * self.pct / 100.0) + 1e-9
    }
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct FieldSrc {
    id: u32,
    key: String,
    name: String,
    unit: String,
    #[serde(rename = "type")]
    ty: FieldType,
    #[serde(default)]
    decimals: u8,
    min: f64,
    max: f64,
    kind: SampleKind,
    tier: Tier,
    #[serde(default)]
    tolerance: Tolerance,
    description: String,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct MatchSrc {
    #[serde(default)]
    manufacturer_id: Option<u16>,
    #[serde(default)]
    name_prefix: Option<String>,
    min_length: usize,
    #[serde(default)]
    when: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct InstanceSrc {
    label: String,
    value: String,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct TableSrc {
    id: String,
    name: String,
    reader: String,
    version: u32,
    status: String,
    provenance: String,
    #[serde(default)]
    verified_on: Vec<String>,
    #[serde(rename = "match")]
    matches: Vec<MatchSrc>,
    #[serde(default)]
    only_when: Option<String>,
    #[serde(default)]
    instance: Option<InstanceSrc>,
    stale_after_ms: u64,
    fields: serde_yaml_ng::Mapping,
}

/// A stable field as frozen in `registry/fields.lock.json`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LockedField {
    pub id: u32,
    pub key: String,
    pub unit: String,
    #[serde(rename = "type")]
    pub ty: FieldType,
    pub decimals: u8,
    pub kind: SampleKind,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Lock {
    /// Stable fields: id, key, unit, type and kind never change again.
    #[serde(default)]
    pub stable: Vec<LockedField>,
    /// Ids that were used once and are never given out again.
    #[serde(default)]
    pub retired_ids: Vec<u32>,
}

pub struct Registry {
    pub bundle: Bundle,
    pub tolerances: BTreeMap<String, Tolerance>,
}

fn key_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 40 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') && s.as_bytes()[0].is_ascii_lowercase()
}

fn id_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 60 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') && s.as_bytes()[0].is_ascii_lowercase()
}

fn yaml_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            n.ends_with(".yaml") && !n.ends_with(".vectors.yaml")
        })
        .collect();
    files.sort();
    Ok(files)
}

fn scalar_to_expr(v: &serde_yaml_ng::Value) -> Option<String> {
    match v {
        serde_yaml_ng::Value::String(s) => Some(s.clone()),
        serde_yaml_ng::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Load the registry. Problems are collected, not stopped at: a contributor sees them all at once.
pub fn load(dir: &Path) -> Result<Registry, Vec<String>> {
    let mut errors = Vec::new();
    let mut fields: Vec<FieldDef> = Vec::new();
    let mut tolerances = BTreeMap::new();

    match yaml_files(&dir.join("fields")) {
        Err(e) => errors.push(e),
        Ok(files) => {
            for path in files {
                let shown = path.strip_prefix(dir).unwrap_or(&path).display().to_string();
                let text = match std::fs::read_to_string(&path) {
                    Ok(t) => t,
                    Err(e) => {
                        errors.push(format!("{shown}: {e}"));
                        continue;
                    }
                };
                let f: FieldSrc = match serde_yaml_ng::from_str(&text) {
                    Ok(f) => f,
                    Err(e) => {
                        errors.push(format!("{shown}: {e}"));
                        continue;
                    }
                };
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                if !key_ok(&f.key) {
                    errors.push(format!("{shown}: key '{}' must be lower case letters, digits and _", f.key));
                }
                if stem != f.key {
                    errors.push(format!("{shown}: the file must be named {}.yaml", f.key));
                }
                if f.id == 0 {
                    errors.push(format!("{shown}: id 0 is not allowed"));
                }
                if f.min > f.max {
                    errors.push(format!("{shown}: min is above max"));
                }
                if f.ty == FieldType::Int && f.decimals != 0 {
                    errors.push(format!("{shown}: an int field has no decimals"));
                }
                if f.decimals > 4 {
                    errors.push(format!("{shown}: at most 4 decimals"));
                }
                if f.name.trim().is_empty() || f.description.trim().is_empty() {
                    errors.push(format!("{shown}: name and description are required"));
                }
                if let Some(other) = fields.iter().find(|o| o.id == f.id) {
                    errors.push(format!("{shown}: id {} is already used by '{}'", f.id, other.key));
                }
                tolerances.insert(f.key.clone(), f.tolerance);
                fields.push(FieldDef { id: f.id, key: f.key, name: f.name, unit: f.unit, ty: f.ty, decimals: f.decimals, min: f.min, max: f.max, kind: f.kind, tier: f.tier });
            }
        }
    }
    fields.sort_by_key(|f| f.id);

    // Append-only: stable fields never change, ids are never reused.
    let lock_path = dir.join("fields.lock.json");
    let lock: Lock = match std::fs::read_to_string(&lock_path) {
        Ok(t) => serde_json::from_str(&t).unwrap_or_else(|e| {
            errors.push(format!("fields.lock.json: {e}"));
            Lock::default()
        }),
        Err(_) => Lock::default(),
    };
    for locked in &lock.stable {
        match fields.iter().find(|f| f.id == locked.id) {
            None => errors.push(format!("stable field '{}' (id {}) was removed: stable fields are append-only", locked.key, locked.id)),
            Some(f) => {
                if locked_form(f) != *locked {
                    errors.push(format!("stable field '{}' (id {}) was changed: id, key, unit, type, decimals and kind are frozen", locked.key, locked.id));
                }
                if f.tier != Tier::Stable {
                    errors.push(format!("field '{}' was stable and cannot go back to experimental", f.key));
                }
            }
        }
    }
    for f in &fields {
        if lock.retired_ids.contains(&f.id) {
            errors.push(format!("field '{}': id {} was retired and cannot be used again", f.key, f.id));
        }
        if f.tier == Tier::Stable && !lock.stable.iter().any(|l| l.id == f.id) {
            errors.push(format!("field '{}' is marked stable but is not in fields.lock.json (a maintainer runs `ofp check --accept-stable`)", f.key));
        }
    }

    let known: BTreeSet<String> = fields.iter().map(|f| f.key.clone()).collect();
    let mut tables: Vec<Table> = Vec::new();
    match yaml_files(&dir.join("tables")) {
        Err(e) => errors.push(e),
        Ok(files) => {
            for path in files {
                let shown = path.strip_prefix(dir).unwrap_or(&path).display().to_string();
                let text = match std::fs::read_to_string(&path) {
                    Ok(t) => t,
                    Err(e) => {
                        errors.push(format!("{shown}: {e}"));
                        continue;
                    }
                };
                let t: TableSrc = match serde_yaml_ng::from_str(&text) {
                    Ok(t) => t,
                    Err(e) => {
                        errors.push(format!("{shown}: {e}"));
                        continue;
                    }
                };
                let before = errors.len();
                let expr = |what: &str, src: &str, errors: &mut Vec<String>| -> Option<Expr> {
                    match exprparse::parse(src) {
                        Ok(e) => Some(e),
                        Err(e) => {
                            errors.push(format!("{shown}: {what}: {e}"));
                            None
                        }
                    }
                };
                if !id_ok(&t.id) {
                    errors.push(format!("{shown}: id '{}' must be lower case letters, digits and -", t.id));
                }
                if path.file_stem().and_then(|s| s.to_str()) != Some(t.id.as_str()) {
                    errors.push(format!("{shown}: the file must be named {}.yaml", t.id));
                }
                if t.reader != "broadcast" {
                    errors.push(format!("{shown}: reader '{}' does not exist (readers: broadcast)", t.reader));
                }
                if t.status != "experimental" && t.status != "stable" {
                    errors.push(format!("{shown}: status must be experimental or stable"));
                }
                if t.provenance.trim().is_empty() {
                    errors.push(format!("{shown}: provenance is required: say where the knowledge of this device comes from"));
                }
                if t.verified_on.is_empty() {
                    errors.push(format!("{shown}: verified_on is required: the device and firmware this was checked on"));
                }
                if t.version == 0 {
                    errors.push(format!("{shown}: version starts at 1"));
                }
                if !(500..=60_000).contains(&t.stale_after_ms) {
                    errors.push(format!("{shown}: stale_after_ms must be between 500 and 60000"));
                }
                if t.matches.is_empty() {
                    errors.push(format!("{shown}: at least one match rule is required"));
                }
                let mut matches = Vec::new();
                for (i, m) in t.matches.iter().enumerate() {
                    if m.manufacturer_id.is_none() && m.name_prefix.as_deref().map_or(true, |n| n.trim().is_empty()) {
                        errors.push(format!("{shown}: match rule {}: a rule without manufacturer_id needs name_prefix", i + 1));
                    }
                    let when = m.when.as_deref().and_then(|w| expr(&format!("match rule {} when", i + 1), w, &mut errors));
                    matches.push(MatchRule { manufacturer_id: m.manufacturer_id, name_prefix: m.name_prefix.clone(), min_length: m.min_length, when });
                }
                let only_when = t.only_when.as_deref().and_then(|w| expr("only_when", w, &mut errors));
                let instance = t.instance.as_ref().and_then(|i| expr("instance", &i.value, &mut errors).map(|value| Instance { label: i.label.clone(), value }));
                let mut tfields = Vec::new();
                if t.fields.is_empty() {
                    errors.push(format!("{shown}: no fields"));
                }
                for (k, v) in &t.fields {
                    let Some(key) = k.as_str() else {
                        errors.push(format!("{shown}: a field name is not text"));
                        continue;
                    };
                    if !known.contains(key) {
                        errors.push(format!("{shown}: field '{key}' is not in registry/fields"));
                        continue;
                    }
                    let (value_src, when_src) = match v {
                        serde_yaml_ng::Value::Mapping(m) => {
                            let extra: Vec<_> = m.keys().filter_map(|k| k.as_str()).filter(|k| *k != "value" && *k != "only_when").collect();
                            if !extra.is_empty() {
                                errors.push(format!("{shown}: field '{key}': unknown setting {}", extra.join(", ")));
                            }
                            (m.get("value").and_then(scalar_to_expr), m.get("only_when").and_then(scalar_to_expr))
                        }
                        other => (scalar_to_expr(other), None),
                    };
                    let Some(value_src) = value_src else {
                        errors.push(format!("{shown}: field '{key}' has no value"));
                        continue;
                    };
                    let value = expr(&format!("field '{key}'"), &value_src, &mut errors);
                    let only_when = when_src.and_then(|w| expr(&format!("field '{key}' only_when"), &w, &mut errors));
                    if let Some(value) = value {
                        tfields.push(TableField { field: key.to_string(), value, only_when });
                    }
                }
                if tables.iter().any(|o| o.id == t.id) {
                    errors.push(format!("{shown}: table id '{}' is used twice", t.id));
                }
                if errors.len() == before {
                    tables.push(Table { id: t.id, name: t.name, reader: t.reader, version: t.version, matches, only_when, instance, stale_after_ms: t.stale_after_ms, fields: tfields });
                }
            }
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }
    let bundle = Bundle { version: BUNDLE_VERSION, fields, tables };
    if let Err(e) = ofp_core::Core::from_bundle(bundle.clone()) {
        return Err(vec![e]);
    }
    Ok(Registry { bundle, tolerances })
}

pub fn locked_form(f: &FieldDef) -> LockedField {
    LockedField { id: f.id, key: f.key.clone(), unit: f.unit.clone(), ty: f.ty, decimals: f.decimals, kind: f.kind }
}

impl Registry {
    pub fn bundle_json(&self) -> String {
        serde_json::to_string(&self.bundle).expect("bundle serializes")
    }

    pub fn core(&self) -> ofp_core::Core {
        ofp_core::Core::from_bundle(self.bundle.clone()).expect("bundle was validated on load")
    }

    pub fn tolerance(&self, key: &str) -> Tolerance {
        self.tolerances.get(key).copied().unwrap_or_default()
    }
}
