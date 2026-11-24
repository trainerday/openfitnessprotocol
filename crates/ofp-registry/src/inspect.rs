//! `ofp inspect`: what is in a capture, without any table.

use ofp_capture::{unhex, Line, PacketKind};
use std::collections::{BTreeMap, BTreeSet};

pub fn inspect(lines: &[Line]) -> String {
    let mut out = String::new();
    if let Some(Line::Header { tool, host, started, device, gatt, .. }) = lines.first() {
        out.push_str(&format!("recorded {started} on {host} with {tool}\n"));
        out.push_str(&format!(
            "device: {}{}{}\n",
            device.name.as_deref().unwrap_or("(no name)"),
            device.company_id.as_ref().map(|c| format!(", company id 0x{c}")).unwrap_or_default(),
            if device.services.is_empty() { String::new() } else { format!(", services {}", device.services.join(" ")) }
        ));
        for s in gatt {
            out.push_str(&format!("  service {}\n", s.uuid));
            for c in &s.characteristics {
                out.push_str(&format!("    {}  {}\n", c.uuid, c.properties.join(", ")));
            }
        }
    }

    // One stream per kind of packet: advertisements, and each characteristic.
    let mut streams: BTreeMap<String, Vec<(u64, Vec<u8>)>> = BTreeMap::new();
    let mut duration = 0;
    for l in lines {
        duration = duration.max(l.t());
        if let Line::Packet { t, kind, mfr, chr, data, .. } = l {
            let (key, hex) = match kind {
                PacketKind::Adv => ("advertisements (manufacturer data)".to_string(), mfr.as_deref()),
                PacketKind::Notify => (format!("notify {}", chr.as_deref().unwrap_or("?")), data.as_deref()),
                PacketKind::Read => (format!("read {}", chr.as_deref().unwrap_or("?")), data.as_deref()),
                PacketKind::Write => (format!("write {}", chr.as_deref().unwrap_or("?")), data.as_deref()),
            };
            if let Some(bytes) = hex.and_then(|h| unhex(h).ok()) {
                streams.entry(key).or_default().push((*t, bytes));
            }
        }
    }
    out.push_str(&format!("length: {:.1} min\n", duration as f64 / 60000.0));

    for (name, packets) in &streams {
        let n = packets.len();
        // (times from a file someone sent: out of order is possible, so no plain subtraction)
        let span = packets.last().map(|p| p.0).unwrap_or(0).saturating_sub(packets.first().map(|p| p.0).unwrap_or(0)).max(1) as f64 / 60000.0;
        let gap = packets.windows(2).map(|w| w[1].0.saturating_sub(w[0].0)).max().unwrap_or(0);
        let lens: BTreeSet<usize> = packets.iter().map(|p| p.1.len()).collect();
        out.push_str(&format!(
            "\n{name}: {n} packets, {:.0}/min, longest gap {:.1} s, length {} bytes\n",
            if n > 1 { (n - 1) as f64 / span } else { 0.0 },
            gap as f64 / 1000.0,
            lens.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("/")
        ));
        let width = lens.iter().copied().max().unwrap_or(0).min(64);
        let mut seen: Vec<BTreeSet<u8>> = vec![BTreeSet::new(); width];
        for (_, bytes) in packets {
            for (i, b) in bytes.iter().take(width).enumerate() {
                seen[i].insert(*b);
            }
        }
        out.push_str("  byte   ");
        for i in 0..width {
            out.push_str(&format!("{i:>3}"));
        }
        out.push_str("\n  last   ");
        if let Some((_, last)) = packets.last() {
            for b in last.iter().take(width) {
                out.push_str(&format!(" {b:02x}"));
            }
        }
        out.push_str("\n  values ");
        for s in &seen {
            out.push_str(&match s.len() {
                0 | 1 => "  .".to_string(),
                n if n > 99 => " ++".to_string(),
                n => format!("{n:>3}"),
            });
        }
        out.push_str("\n  (values = how many different values each byte took; . = never changed)\n");
    }

    let notes: Vec<&Line> = lines.iter().filter(|l| matches!(l, Line::Note { .. } | Line::Confirm { .. })).collect();
    if !notes.is_empty() {
        out.push_str("\nnotes and confirmations:\n");
        for l in notes {
            match l {
                Line::Note { t, text, .. } => out.push_str(&format!("  {:>6.0} s  {text}\n", *t as f64 / 1000.0)),
                Line::Confirm { t, field, shown, console, ok, .. } => out.push_str(&format!(
                    "  {:>6.0} s  {field} = {shown}: {}\n",
                    *t as f64 / 1000.0,
                    if *ok { "matches the console".to_string() } else { format!("wrong, console showed {}", console.map(|c| c.to_string()).unwrap_or_else(|| "?".into())) }
                )),
                _ => {}
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ofp_capture::{header, DeviceInfo};
    use std::collections::BTreeMap;

    #[test]
    fn times_out_of_order_or_huge_do_not_panic() {
        let lines = vec![
            header("test", DeviceInfo::default(), vec![]),
            Line::adv(5000, None, None, &[1, 2], BTreeMap::new()),
            Line::adv(10, None, None, &[1, 3], BTreeMap::new()),
            Line::adv(u64::MAX, None, None, &[1, 4], BTreeMap::new()),
            Line::adv(0, None, None, &[1, 5], BTreeMap::new()),
        ];
        assert!(inspect(&lines).contains("4 packets"));
    }
}
