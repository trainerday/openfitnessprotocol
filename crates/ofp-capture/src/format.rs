//! The capture file: one JSON object per line (`*.capture.jsonl`).
//!
//! A header, then packets, the rider's notes and confirmations, each with `t` = milliseconds since
//! the recording started. See `docs/capture-format.md`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{BufRead, Write};

pub const FORMAT: &str = "ofp-capture";
pub const FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct DeviceInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Bluetooth company id of the manufacturer data, four hex digits (`"0102"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub company_id: Option<String>,
    /// Advertised service UUIDs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GattCharacteristic {
    pub uuid: String,
    /// read, write, write_without_response, notify, indicate
    pub properties: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GattService {
    pub uuid: String,
    pub characteristics: Vec<GattCharacteristic>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PacketKind {
    /// A BLE advertisement.
    Adv,
    /// A notification or indication from a characteristic.
    Notify,
    /// A value read from a characteristic.
    Read,
    /// Bytes the tool wrote to a characteristic (the Studio's tester: a trainer command).
    Write,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Line {
    Header {
        format: String,
        version: u32,
        tool: String,
        host: String,
        /// UTC, ISO 8601.
        started: String,
        device: DeviceInfo,
        /// Services and characteristics found after connecting (connectable devices only).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        gatt: Vec<GattService>,
    },
    Packet {
        t: u64,
        kind: PacketKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rssi: Option<i16>,
        /// Advertisement: manufacturer data in hex, company id first.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mfr: Option<String>,
        /// Advertisement: service data, UUID → hex.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        service_data: BTreeMap<String, String>,
        /// Notify / read: the characteristic UUID.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chr: Option<String>,
        /// Notify / read: the bytes in hex.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data: Option<String>,
    },
    /// What the rider wrote while recording. `values` are what the device's own display showed
    /// (`power=150 cadence=82`), by field key.
    Note {
        t: u64,
        text: String,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        values: BTreeMap<String, f64>,
    },
    /// The rider compared a decoded value with the device's own display.
    Confirm {
        t: u64,
        source_id: String,
        field: String,
        /// What OFP showed.
        shown: f64,
        /// What the device's display showed, when it was different.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        console: Option<f64>,
        ok: bool,
    },
}

impl Line {
    pub fn t(&self) -> u64 {
        match self {
            Line::Header { .. } => 0,
            Line::Packet { t, .. } | Line::Note { t, .. } | Line::Confirm { t, .. } => *t,
        }
    }

    pub fn adv(t: u64, name: Option<String>, rssi: Option<i16>, mfr: &[u8], service_data: BTreeMap<String, String>) -> Line {
        Line::Packet { t, kind: PacketKind::Adv, name, rssi, mfr: if mfr.is_empty() { None } else { Some(hex(mfr)) }, service_data, chr: None, data: None }
    }
}

pub fn header(tool: &str, device: DeviceInfo, gatt: Vec<GattService>) -> Line {
    Line::Header {
        format: FORMAT.into(),
        version: FORMAT_VERSION,
        tool: tool.into(),
        host: std::env::consts::OS.into(),
        started: now_iso(),
        device,
        gatt,
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(s: &str) -> Result<Vec<u8>, String> {
    let s: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if s.len() % 2 != 0 {
        return Err("hex with an odd number of digits".into());
    }
    s.chunks(2)
        .map(|c| {
            let h = (c[0] as char).to_digit(16).ok_or("not hex")?;
            let l = (c[1] as char).to_digit(16).ok_or("not hex")?;
            Ok((h * 16 + l) as u8)
        })
        .collect()
}

/// A device name as a file name: lower case, anything else becomes `-`.
pub fn file_stem_for(name: &str) -> String {
    let s: String = name.trim().chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    if s.is_empty() { "device".into() } else { s }
}

/// Parse what a rider typed: `power=150 cadence=82 felt steady` → the values and the full text.
pub fn parse_note(text: &str) -> BTreeMap<String, f64> {
    let mut values = BTreeMap::new();
    for word in text.split_whitespace() {
        if let Some((k, v)) = word.split_once('=') {
            if let Ok(v) = v.parse::<f64>() {
                if !k.is_empty() {
                    values.insert(k.to_string(), v);
                }
            }
        }
    }
    values
}

pub fn read<R: BufRead>(r: R) -> Result<Vec<Line>, String> {
    let mut lines = Vec::new();
    for (i, l) in r.lines().enumerate() {
        let l = l.map_err(|e| e.to_string())?;
        if l.trim().is_empty() {
            continue;
        }
        lines.push(serde_json::from_str(&l).map_err(|e| format!("line {}: {e}", i + 1))?);
    }
    match lines.first() {
        Some(Line::Header { format, version, .. }) if format == FORMAT && *version == FORMAT_VERSION => Ok(lines),
        Some(Line::Header { format, version, .. }) => Err(format!("not a capture this tool reads ({format} version {version})")),
        _ => Err("the first line is not a capture header".into()),
    }
}

pub fn read_file(path: &std::path::Path) -> Result<Vec<Line>, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    read(std::io::BufReader::new(f)).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn write_line<W: Write>(w: &mut W, line: &Line) -> std::io::Result<()> {
    serde_json::to_writer(&mut *w, line)?;
    w.write_all(b"\n")
}

/// UTC now as `2026-01-31T12:34:56Z`.
pub fn now_iso() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // civil-from-days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let lines = vec![
            header("test", DeviceInfo { name: Some("M3".into()), company_id: Some("0102".into()), services: vec![] }, vec![]),
            Line::adv(10, Some("M3".into()), Some(-60), &[2, 1, 6], BTreeMap::new()),
            Line::Note { t: 20, text: "power=150 steady".into(), values: parse_note("power=150 steady") },
            Line::Confirm { t: 30, source_id: "x:1".into(), field: "power".into(), shown: 150.0, console: None, ok: true },
        ];
        let mut buf = Vec::new();
        for l in &lines {
            write_line(&mut buf, l).unwrap();
        }
        assert_eq!(read(&buf[..]).unwrap(), lines);
    }

    #[test]
    fn note_values() {
        let v = parse_note("power=150 cadence=82.5 legs=tired");
        assert_eq!(v.get("power"), Some(&150.0));
        assert_eq!(v.get("cadence"), Some(&82.5));
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn hex_round_trip() {
        assert_eq!(unhex(&hex(&[0, 255, 16])).unwrap(), vec![0, 255, 16]);
        assert!(unhex("abc").is_err());
    }
}
