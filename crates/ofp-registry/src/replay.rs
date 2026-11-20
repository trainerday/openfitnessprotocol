//! Runs a capture through the core: the same path a live device takes.

use ofp_capture::{unhex, Line, PacketKind};
use ofp_core::{Advertisement, Core, Message};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct Decoded {
    pub t: u64,
    pub source_id: String,
    pub fields: BTreeMap<String, f64>,
    pub stale: bool,
}

#[derive(Default, Debug)]
pub struct Stats {
    pub adverts: usize,
    pub other_packets: usize,
    /// Advertisements that produced values.
    pub decoded: usize,
    pub out_of_range: BTreeMap<String, usize>,
    /// source id → display name
    pub sources: BTreeMap<String, String>,
    pub stale_events: usize,
    pub duration_ms: u64,
    pub longest_gap_ms: u64,
}

pub fn values_of(fields: &BTreeMap<String, serde_json::Value>) -> BTreeMap<String, f64> {
    fields.iter().filter_map(|(k, v)| v.as_f64().map(|v| (k.clone(), v))).collect()
}

/// Collects what the core says into a timeline and statistics.
#[derive(Default)]
pub struct Collector {
    pub timeline: Vec<Decoded>,
    pub stats: Stats,
}

impl Collector {
    pub fn take(&mut self, messages: Vec<Message>) {
        for m in messages {
            match m {
                Message::SourceAppeared { source_id, name, .. } => {
                    self.stats.sources.insert(source_id, name);
                }
                Message::Values { source_id, t, fields, out_of_range, stale, .. } => {
                    if !stale {
                        self.stats.decoded += 1;
                    }
                    for k in out_of_range {
                        *self.stats.out_of_range.entry(k).or_default() += 1;
                    }
                    self.timeline.push(Decoded { t, source_id, fields: values_of(&fields), stale });
                }
                Message::SourceStale { .. } => self.stats.stale_events += 1,
                Message::SourceInUse { .. } | Message::SourceLost { .. } => {}
            }
        }
    }
}

pub fn replay(core: &mut Core, lines: &[Line]) -> Result<Collector, String> {
    let mut c = Collector::default();
    let mut next_tick = 1000u64;
    let mut last_adv: Option<u64> = None;
    for line in lines {
        let t = line.t();
        if t > next_tick + 60_000 {
            c.take(core.tick(next_tick + 30_000));
            next_tick = t - t % 1000;
        }
        while next_tick <= t {
            c.take(core.tick(next_tick));
            next_tick += 1000;
        }
        c.stats.duration_ms = c.stats.duration_ms.max(t);
        if let Line::Packet { kind, name, rssi, mfr, .. } = line {
            if *kind != PacketKind::Adv {
                c.stats.other_packets += 1;
                continue;
            }
            c.stats.adverts += 1;
            if let Some(prev) = last_adv {
                c.stats.longest_gap_ms = c.stats.longest_gap_ms.max(t.saturating_sub(prev));
            }
            last_adv = Some(t);
            let data = unhex(mfr.as_deref().unwrap_or(""))?;
            c.take(core.on_advertisement(&Advertisement { device_id: "capture", name: name.as_deref(), rssi: *rssi, manufacturer_data: &data, now_ms: t }));
        }
    }
    Ok(c)
}
