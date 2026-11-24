//! "Find it for me": which bytes, read how, give the values the rider noted?

use ofp_core::Expr;
use ofp_registry::{exprparse, Tolerance};
use serde::Serialize;
use std::collections::BTreeSet;

/// One packet of a stream: time and the bytes a table would read (after the company id).
#[derive(Clone, Debug)]
pub struct Sample {
    pub t: u64,
    pub payload: Vec<u8>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Candidate {
    /// In the table language, ready to paste: `u16le[4] / 10`.
    pub expr: String,
    /// Notes this candidate agrees with.
    pub fits: usize,
    pub notes: usize,
    /// Mean distance from the noted values, in the field's unit.
    pub mean_error: f64,
    /// False when the value never changed in the capture.
    pub varies: bool,
}

/// A person reads a display and then types: look from 8 s before a note to 2 s after.
const BEFORE_MS: u64 = 8000;
const AFTER_MS: u64 = 2000;
const SCALES: [(&str, f64); 5] = [("", 1.0), (" / 10", 0.1), (" / 100", 0.01), (" / 1000", 0.001), (" * 10", 10.0)];

/// Kinds of reading, in the order they are preferred when the notes cannot tell them apart:
/// Bluetooth devices are little-endian, a plain read comes before a masked one, and a composed
/// one (minutes and seconds) last.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    U16le,
    U8,
    U16be,
    Masked,
    Composed,
}

fn candidates(len: usize) -> Vec<(String, Kind)> {
    let mut out = Vec::new();
    for at in 0..len {
        out.push((format!("u8[{at}]"), Kind::U8));
        if at + 1 < len {
            out.push((format!("u16le[{at}]"), Kind::U16le));
            out.push((format!("u16be[{at}]"), Kind::U16be));
            out.push((format!("u16le[{at}] & 0x7fff"), Kind::Masked));
            // minutes then seconds, and the other way round
            out.push((format!("u8[{at}] * 60 + u8[{}]", at + 1), Kind::Composed));
            out.push((format!("u8[{}] * 60 + u8[{at}]", at + 1), Kind::Composed));
        }
    }
    out
}

/// Rank the ways of reading `samples` that give the noted values. `notes` are (time, value);
/// `range` is the field's plausible range, when the field is known.
pub fn solve(samples: &[Sample], notes: &[(u64, f64)], tolerance: Tolerance, range: Option<(f64, f64)>, top: usize) -> Vec<Candidate> {
    if samples.is_empty() || notes.is_empty() {
        return Vec::new();
    }
    let len = samples.iter().map(|s| s.payload.len()).max().unwrap_or(0).min(64);
    // A field with no tolerance of its own: 2 % or half a unit.
    let tolerance = if tolerance.abs == 0.0 && tolerance.pct == 0.0 { Tolerance { abs: 0.5, pct: 2.0 } } else { tolerance };
    // The packets around each note, found once.
    let windows: Vec<Vec<usize>> = notes
        .iter()
        .map(|(t, _)| samples.iter().enumerate().filter(|(_, s)| s.t.saturating_add(BEFORE_MS) >= *t && s.t <= t.saturating_add(AFTER_MS)).map(|(i, _)| i).collect())
        .collect();

    // (candidate, stays in the plausible range, order of preference)
    let mut found: Vec<(Candidate, bool, u8)> = Vec::new();
    for (base, kind) in candidates(len) {
        let Ok(tree) = exprparse::parse(&base) else { continue };
        let raw: Vec<Option<f64>> = samples.iter().map(|s| tree.eval(&s.payload)).collect();
        let distinct: BTreeSet<u64> = raw.iter().flatten().map(|v| v.to_bits()).collect();
        if distinct.is_empty() {
            continue;
        }
        let varies = distinct.len() > 1;
        // A two-byte read whose second byte never carries anything is no better than one byte.
        let preference = match kind {
            Kind::U16le if raw.iter().flatten().any(|v| *v > 255.0) => 0,
            Kind::U8 => 1,
            Kind::U16le => 2,
            Kind::U16be => 3,
            Kind::Masked => 4,
            Kind::Composed => 5,
        };
        for (suffix, scale) in SCALES {
            let (mut fits, mut error_sum, mut judged) = (0usize, 0.0f64, 0usize);
            for ((_, want), window) in notes.iter().zip(&windows) {
                let nearest = window.iter().filter_map(|i| raw[*i]).map(|v| v * scale).min_by(|a, b| (a - want).abs().total_cmp(&(b - want).abs()));
                if let Some(v) = nearest {
                    judged += 1;
                    error_sum += (v - want).abs();
                    if tolerance.allows(v, *want) {
                        fits += 1;
                    }
                }
            }
            if judged == 0 || fits == 0 {
                continue;
            }
            let in_range = range.is_none_or(|(min, max)| {
                let all = raw.iter().flatten().count().max(1);
                let inside = raw.iter().flatten().filter(|v| **v * scale >= min && **v * scale <= max).count();
                inside * 100 >= all * 98
            });
            let expr = if suffix.is_empty() { base.clone() } else if base.contains(' ') { format!("({base}){suffix}") } else { format!("{base}{suffix}") };
            found.push((Candidate { expr, fits, notes: notes.len(), mean_error: error_sum / judged as f64, varies }, in_range, preference));
        }
    }
    // Most notes explained first; a value that changes before a constant; one that stays plausible
    // over the whole capture; the smaller error; then the order of preference.
    found.sort_by(|(a, a_in, a_pref), (b, b_in, b_pref)| {
        b.fits
            .cmp(&a.fits)
            .then(b.varies.cmp(&a.varies))
            .then(b_in.cmp(a_in))
            .then(((a.mean_error * 100.0).round() as i64).cmp(&((b.mean_error * 100.0).round() as i64)))
            .then(a_pref.cmp(b_pref))
            .then(a.expr.len().cmp(&b.expr.len()))
    });
    found.into_iter().take(top).map(|(c, _, _)| c).collect()
}

/// Evaluate one expression over a stream, for the chart. `Err` when the expression does not parse.
pub fn series(samples: &[Sample], expr: &str) -> Result<Vec<(u64, f64)>, String> {
    let tree: Expr = exprparse::parse(expr)?;
    Ok(samples.iter().filter_map(|s| tree.eval(&s.payload).map(|v| (s.t, v))).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ofp_capture::{unhex, Line, PacketKind};
    use std::path::Path;

    /// The real Keiser ride that ships with the registry.
    fn ride() -> Vec<Sample> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../registry/tables/keiser-m3i.capture.jsonl");
        ofp_capture::read_file(&path)
            .unwrap()
            .iter()
            .filter_map(|l| match l {
                Line::Packet { t, kind: PacketKind::Adv, mfr: Some(m), .. } => Some(Sample { t: *t, payload: unhex(m).unwrap()[2..].to_vec() }),
                _ => None,
            })
            // live packets only, as a rider would see them on the console
            .filter(|s| s.payload[2] == 0)
            .collect()
    }

    /// What the console would have shown at a few moments, read from the packets by the known layout.
    fn notes(samples: &[Sample], value: impl Fn(&[u8]) -> f64) -> Vec<(u64, f64)> {
        [60, 150, 240, 330, 420, 510].iter().map(|i| (samples[*i].t + 500, value(&samples[*i].payload))).collect()
    }

    fn best(samples: &[Sample], notes: &[(u64, f64)], tolerance: Tolerance) -> String {
        solve(samples, notes, tolerance, None, 5).first().map(|c| c.expr.clone()).unwrap_or_default()
    }

    #[test]
    fn finds_the_keiser_fields_from_a_few_notes() {
        let s = ride();
        let u16le = |p: &[u8], at: usize| u16::from_le_bytes([p[at], p[at + 1]]) as f64;
        assert_eq!(best(&s, &notes(&s, |p| u16le(p, 8)), Tolerance { abs: 3.0, pct: 2.0 }), "u16le[8]", "power");
        assert_eq!(best(&s, &notes(&s, |p| (u16le(p, 4) / 10.0).round()), Tolerance { abs: 1.0, pct: 2.0 }), "u16le[4] / 10", "cadence");
        assert_eq!(best(&s, &notes(&s, |p| p[16] as f64), Tolerance::default()), "u8[16]", "gear");
        assert_eq!(best(&s, &notes(&s, |p| p[12] as f64 * 60.0 + p[13] as f64), Tolerance { abs: 2.0, pct: 0.0 }), "u8[12] * 60 + u8[13]", "elapsed time");
        // energy never passed 255 kcal in this ride: one byte is all the capture can show
        assert_eq!(best(&s, &notes(&s, |p| u16le(p, 10)), Tolerance { abs: 1.0, pct: 0.0 }), "u8[10]", "energy");
    }

    #[test]
    fn every_note_is_explained_by_the_winner() {
        let s = ride();
        let n = notes(&s, |p| u16::from_le_bytes([p[8], p[9]]) as f64);
        let c = &solve(&s, &n, Tolerance { abs: 3.0, pct: 2.0 }, Some((0.0, 3000.0)), 5)[0];
        assert_eq!((c.fits, c.notes, c.varies), (6, 6, true));
        assert!(c.mean_error < 1.0);
    }

    #[test]
    fn nothing_fits_nonsense_and_nothing_is_invented_without_notes() {
        let s = ride();
        assert!(solve(&s, &[], Tolerance::default(), None, 5).is_empty());
        assert!(solve(&[], &[(0, 1.0)], Tolerance::default(), None, 5).is_empty());
        let nonsense: Vec<(u64, f64)> = [60usize, 150, 240].iter().map(|i| (s[*i].t, -5000.0)).collect();
        assert!(solve(&s, &nonsense, Tolerance { abs: 1.0, pct: 1.0 }, None, 5).is_empty());
    }

    #[test]
    fn series_reads_a_stream() {
        let s = ride();
        let power = series(&s, "u16le[8]").unwrap();
        assert_eq!(power.len(), s.len());
        assert_eq!(power.iter().map(|(_, v)| *v as i64).max(), Some(346));
        assert!(series(&s, "u16le[8").is_err());
    }
}
