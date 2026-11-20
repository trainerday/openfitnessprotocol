//! The Bluetooth commands: scan, record, live.

use ofp_registry::registry::Registry;
use ofp_registry::replay::values_of;
use crossterm::{cursor, event, execute, queue, style, terminal};
use futures::StreamExt;
use ofp_capture::ble::{self, Adv, Scanner};
use ofp_capture::{header, hex, parse_note, file_stem_for, write_line, DeviceInfo, Line, PacketKind};
use ofp_core::{Advertisement, FieldDescription, Message, SampleKind};
use std::collections::BTreeMap;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|e| e.to_string())
}

/// Lines typed on standard input, from a thread (works in a terminal and when piped).
fn stdin_lines() -> mpsc::UnboundedReceiver<String> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lines() {
            match line {
                Ok(l) => {
                    if tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    rx
}

fn company_id(mfr: &[u8]) -> Option<String> {
    (mfr.len() >= 2).then(|| format!("{:02x}{:02x}", mfr[1], mfr[0]))
}

fn default_output(name: &str) -> PathBuf {
    let stamp = ofp_capture::now_iso().replace([':', '-'], "").replace('T', "-").replace('Z', "");
    PathBuf::from(format!("{}-{stamp}.capture.jsonl", file_stem_for(name)))
}

/// Rate and gap of a stream of packets.
#[derive(Default, Clone)]
struct Rate {
    count: u64,
    first: Option<Instant>,
    last: Option<Instant>,
    longest_gap: Duration,
}

impl Rate {
    fn hit(&mut self, now: Instant) {
        if let Some(last) = self.last {
            self.longest_gap = self.longest_gap.max(now - last);
        }
        self.first.get_or_insert(now);
        self.last = Some(now);
        self.count += 1;
    }

    fn per_minute(&self, now: Instant) -> f64 {
        match self.first {
            Some(first) if self.count > 1 => {
                let secs = (now - first).as_secs_f64().max(1.0);
                self.count as f64 * 60.0 / secs
            }
            _ => 0.0,
        }
    }
}

// ───────────────────────────── scan ─────────────────────────────

pub fn scan(seconds: u64) -> Result<(), String> {
    struct Seen {
        name: Option<String>,
        companies: Vec<String>,
        services: Vec<String>,
        rssi: Option<i16>,
        rate: Rate,
        connectable_hint: bool,
    }
    runtime()?.block_on(async {
        let mut scanner = Scanner::start().await?;
        eprintln!("scanning for {seconds} s …");
        let mut seen: BTreeMap<String, Seen> = BTreeMap::new();
        let start = Instant::now();
        let end = start + Duration::from_secs(seconds);
        loop {
            let adv = tokio::select! {
                a = scanner.next() => match a { Some(a) => a, None => break },
                _ = tokio::time::sleep_until(end.into()) => break,
            };
            let s = seen.entry(adv.device_id.clone()).or_insert_with(|| Seen { name: None, companies: vec![], services: vec![], rssi: None, rate: Rate::default(), connectable_hint: false });
            s.rate.hit(Instant::now());
            if adv.name.is_some() {
                s.name = adv.name.clone();
            }
            if adv.rssi.is_some() {
                s.rssi = adv.rssi;
            }
            if let Some(c) = company_id(&adv.mfr) {
                if !s.companies.contains(&c) {
                    s.companies.push(c);
                }
            }
            for u in adv.services.iter().chain(adv.service_data.keys()) {
                if !s.services.contains(u) {
                    s.services.push(u.clone());
                    s.connectable_hint = true;
                }
            }
        }
        scanner.stop().await;
        let now = Instant::now();
        let mut rows: Vec<&Seen> = seen.values().collect();
        rows.sort_by(|a, b| b.rate.count.cmp(&a.rate.count));
        println!("{:<24} {:>8} {:>9} {:>7}  {:<10} services", "name", "adverts", "per min", "gap s", "company");
        for s in rows {
            println!(
                "{:<24} {:>8} {:>9.0} {:>7.1}  {:<10} {}",
                s.name.as_deref().unwrap_or("(no name)"),
                s.rate.count,
                s.rate.per_minute(now),
                s.rate.longest_gap.as_secs_f64(),
                s.companies.iter().map(|c| format!("0x{c}")).collect::<Vec<_>>().join(" "),
                s.services.join(" ")
            );
        }
        println!("\n{} devices. `ofp record \"<name>\"` records one; add --connect for a device that lists services and sends no data in its adverts.", seen.len());
        Ok(())
    })
}

// ───────────────────────────── record ─────────────────────────────

/// The packet in hex, the bytes that changed since the previous one in bold.
fn diff_hex(prev: Option<&[u8]>, now: &[u8], color: bool) -> String {
    let mut s = String::new();
    for (i, b) in now.iter().enumerate() {
        let changed = prev.is_some_and(|p| p.get(i) != Some(b));
        if changed && color {
            s.push_str(&format!("\x1b[1;33m{b:02x}\x1b[0m "));
        } else {
            s.push_str(&format!("{b:02x} "));
        }
    }
    s
}

pub fn record(device: &str, output: Option<PathBuf>, connect: bool, seconds: Option<u64>) -> Result<(), String> {
    let output = output.unwrap_or_else(|| default_output(device));
    let color = std::io::stdout().is_terminal();
    runtime()?.block_on(async {
        let mut scanner = Scanner::start().await?;
        let mut notes = stdin_lines();
        let mut stdin_open = true;
        eprintln!("looking for \"{device}\" …");
        let wanted = |a: &Adv| a.name.as_deref().is_some_and(|n| n.trim().eq_ignore_ascii_case(device.trim()));

        // The first advert tells what the device is; the file starts there.
        let first = loop {
            tokio::select! {
                a = scanner.next() => match a { Some(a) if wanted(&a) => break a, Some(_) => {}, None => return Err("Bluetooth stopped".to_string()) },
                _ = tokio::signal::ctrl_c() => return Ok(()),
            }
        };
        let info = DeviceInfo { name: first.name.clone(), company_id: company_id(&first.mfr), services: first.services.clone() };
        let mut file = std::io::BufWriter::new(std::fs::File::create(&output).map_err(|e| format!("{}: {e}", output.display()))?);
        let start = Instant::now();
        let t = |i: Instant| (i - start).as_millis() as u64;
        let deadline = seconds.map(|s| start + Duration::from_secs(s));
        let mut out = |line: &Line| -> Result<(), String> {
            write_line(&mut file, line).and_then(|_| file.flush()).map_err(|e| e.to_string())
        };

        let mut connection = None;
        if connect {
            scanner.stop().await;
            eprintln!("connecting …");
            let c = ble::connect(scanner.peripheral(&first.device_id).await?).await?;
            out(&header(concat!("ofp ", env!("CARGO_PKG_VERSION")), info, c.gatt.clone()))?;
            for s in &c.gatt {
                println!("service {}", s.uuid);
                for ch in &s.characteristics {
                    println!("  {}  {}", ch.uuid, ch.properties.join(", "));
                }
            }
            for (uuid, value) in &c.reads {
                println!("read {uuid}: {}", hex(value));
                out(&Line::Packet { t: 0, kind: PacketKind::Read, name: None, rssi: None, mfr: None, service_data: BTreeMap::new(), chr: Some(uuid.clone()), data: Some(hex(value)) })?;
            }
            println!("subscribed to {}", if c.subscribed.is_empty() { "nothing (no characteristic notifies)".to_string() } else { c.subscribed.join(", ") });
            connection = Some(c);
        } else {
            out(&header(concat!("ofp ", env!("CARGO_PKG_VERSION")), info, vec![]))?;
        }
        println!("recording to {} — type a note and Enter (what the console shows: power=150 cadence=82), q + Enter to stop", output.display());

        let mut rate = Rate::default();
        let mut last: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let mut status = tokio::time::interval(Duration::from_secs(5));
        status.tick().await;
        let mut pending: Option<Adv> = (!connect).then_some(first);
        loop {
            // (stream key, bytes) of a packet just written, for the display
            let mut shown: Option<(String, Vec<u8>)> = None;
            if let Some(a) = pending.take() {
                let now = Instant::now();
                rate.hit(now);
                out(&Line::adv(t(now), a.name.clone(), a.rssi, &a.mfr, a.service_data.iter().map(|(k, v)| (k.clone(), hex(v))).collect()))?;
                shown = Some(("adv".into(), a.mfr.clone()));
            } else {
                tokio::select! {
                    a = scanner.next(), if connection.is_none() => match a {
                        Some(a) if wanted(&a) => { pending = Some(a); continue; }
                        Some(_) => continue,
                        None => break,
                    },
                    n = async { connection.as_mut().unwrap().notifications.next().await }, if connection.is_some() => match n {
                        Some(n) => {
                            let now = Instant::now();
                            rate.hit(now);
                            let uuid = ble::short_uuid(&n.uuid);
                            out(&Line::Packet { t: t(now), kind: PacketKind::Notify, name: None, rssi: None, mfr: None, service_data: BTreeMap::new(), chr: Some(uuid.clone()), data: Some(hex(&n.value)) })?;
                            shown = Some((uuid, n.value));
                        }
                        None => { eprintln!("the device disconnected"); break; }
                    },
                    line = notes.recv(), if stdin_open => match line {
                        Some(l) if l.trim() == "q" => break,
                        Some(l) if l.trim().is_empty() => {}
                        Some(l) => {
                            let values = parse_note(&l);
                            out(&Line::Note { t: t(Instant::now()), text: l.trim().to_string(), values })?;
                            println!("  noted at {:.0} s", start.elapsed().as_secs_f64());
                        }
                        None => stdin_open = false,
                    },
                    _ = status.tick() => {
                        let now = Instant::now();
                        println!("  {:>5.0} s  {} packets  {:.0}/min  longest gap {:.1} s", start.elapsed().as_secs_f64(), rate.count, rate.per_minute(now), rate.longest_gap.as_secs_f64());
                    }
                    _ = tokio::signal::ctrl_c() => break,
                    _ = async { tokio::time::sleep_until(deadline.unwrap().into()).await }, if deadline.is_some() => break,
                }
            }
            if let Some((key, bytes)) = shown {
                let prev = last.get(&key);
                if prev.map(|p| p.as_slice()) != Some(bytes.as_slice()) {
                    println!("{:>7.1} s  {:<5} {}", start.elapsed().as_secs_f64(), if key == "adv" { "" } else { &key[..key.len().min(5)] }, diff_hex(prev.map(|p| p.as_slice()), &bytes, color));
                    last.insert(key, bytes);
                }
            }
        }
        if let Some(c) = &connection {
            ble::disconnect(&c.peripheral).await;
        } else {
            scanner.stop().await;
        }
        let now = Instant::now();
        println!("{} packets in {:.0} s ({:.0}/min, longest gap {:.1} s) → {}", rate.count, start.elapsed().as_secs_f64(), rate.per_minute(now), rate.longest_gap.as_secs_f64(), output.display());
        println!("`ofp inspect {}` shows which bytes change.", output.display());
        Ok(())
    })
}

// ───────────────────────────── live ─────────────────────────────

#[derive(Clone, PartialEq)]
enum Verdict {
    Unchecked,
    Matches,
    Wrong(Option<f64>),
}

enum Mode {
    Normal,
    /// Field index, the value frozen when the key was pressed, what was typed so far.
    Confirm(usize, f64, String),
    Note(String),
}

struct LiveState {
    source_id: Option<String>,
    device_id: Option<String>,
    name: String,
    fields: Vec<FieldDescription>,
    values: BTreeMap<String, f64>,
    verdicts: Vec<Verdict>,
    stale: bool,
    rssi: Option<i16>,
    rate: Rate,
    others: Vec<String>,
    message: String,
}

enum Input {
    Key(event::KeyEvent),
    Line(String),
    Closed,
}

fn fmt_value(v: f64, f: &FieldDescription) -> String {
    format!("{:.*}", f.decimals as usize, v)
}

const GLYPHS: [(char, [&str; 5]); 12] = [
    ('0', ["███", "█ █", "█ █", "█ █", "███"]),
    ('1', ["  █", "  █", "  █", "  █", "  █"]),
    ('2', ["███", "  █", "███", "█  ", "███"]),
    ('3', ["███", "  █", "███", "  █", "███"]),
    ('4', ["█ █", "█ █", "███", "  █", "  █"]),
    ('5', ["███", "█  ", "███", "  █", "███"]),
    ('6', ["███", "█  ", "███", "█ █", "███"]),
    ('7', ["███", "  █", "  █", "  █", "  █"]),
    ('8', ["███", "█ █", "███", "█ █", "███"]),
    ('9', ["███", "█ █", "███", "  █", "███"]),
    ('.', [" ", " ", " ", " ", "█"]),
    ('-', ["   ", "   ", "███", "   ", "   "]),
];

fn big(text: &str) -> [String; 5] {
    let mut rows: [String; 5] = Default::default();
    for ch in text.chars() {
        if let Some((_, g)) = GLYPHS.iter().find(|(c, _)| *c == ch) {
            for (row, part) in rows.iter_mut().zip(g) {
                row.push_str(part);
                row.push(' ');
            }
        }
    }
    rows
}

fn verdict_text(v: &Verdict) -> String {
    match v {
        Verdict::Unchecked => "not checked".into(),
        Verdict::Matches => "✓ matches the console".into(),
        Verdict::Wrong(Some(c)) => format!("✗ console showed {c}"),
        Verdict::Wrong(None) => "✗ wrong".into(),
    }
}

fn draw(s: &LiveState, mode: &Mode, file: &std::path::Path) -> std::io::Result<()> {
    let mut o = std::io::stdout();
    let (cols, _) = terminal::size().unwrap_or((100, 40));
    let cols = if cols == 0 { 100 } else { cols }; // some terminals report no size
    queue!(o, cursor::MoveTo(0, 0), terminal::Clear(terminal::ClearType::All))?;
    let mut lines: Vec<String> = Vec::new();
    if s.source_id.is_none() {
        lines.push("Listening … no device that a table knows has been heard yet.".into());
    } else {
        let now = Instant::now();
        lines.push(format!(
            "{}    {:.0} packets/min · longest gap {:.1} s · signal {}{}",
            s.name,
            s.rate.per_minute(now),
            s.rate.longest_gap.as_secs_f64(),
            s.rssi.map(|r| r.to_string()).unwrap_or_else(|| "?".into()),
            if s.stale { "    ·· SILENT ··" } else { "" }
        ));
        lines.push(String::new());
        let width = 30usize;
        let per_row = ((cols as usize) / width).max(1);
        let instant: Vec<usize> = (0..s.fields.len()).filter(|i| s.fields[*i].kind == SampleKind::Instantaneous).collect();
        for chunk in instant.chunks(per_row) {
            let mut block = vec![String::new(); 8];
            for &i in chunk {
                let f = &s.fields[i];
                let text = s.values.get(&f.key).map(|v| fmt_value(*v, f)).unwrap_or_else(|| "-".into());
                let rows = big(&text);
                let cell = |t: String| format!("{:<w$}", t.chars().take(width - 1).collect::<String>(), w = width);
                block[0].push_str(&cell(format!("[{}] {}{}", i + 1, f.name, if f.unit.is_empty() { String::new() } else { format!(" ({})", f.unit) })));
                for (r, row) in rows.iter().enumerate() {
                    block[1 + r].push_str(&cell(row.clone()));
                }
                block[6].push_str(&cell(verdict_text(&s.verdicts[i])));
            }
            lines.extend(block);
        }
        for (i, f) in s.fields.iter().enumerate() {
            if f.kind == SampleKind::Instantaneous {
                continue;
            }
            let text = s.values.get(&f.key).map(|v| fmt_value(*v, f)).unwrap_or_else(|| "-".into());
            lines.push(format!("[{}] {:<16} {:>10} {:<6} {}", i + 1, f.name, text, f.unit, verdict_text(&s.verdicts[i])));
        }
        if !s.others.is_empty() {
            lines.push(String::new());
            lines.push(format!("also heard: {} (choose one with --source)", s.others.join(", ")));
        }
    }
    lines.push(String::new());
    lines.push(match mode {
        Mode::Normal => "Press a field's number to compare it with the console · n = note · q = finish".into(),
        Mode::Confirm(i, shown, typed) => format!(
            "{} shows {}. Enter = the console shows the same · or type what the console shows, then Enter · Esc = cancel   > {typed}",
            s.fields[*i].name,
            fmt_value(*shown, &s.fields[*i])
        ),
        Mode::Note(typed) => format!("Note (Enter to save, Esc to cancel) > {typed}"),
    });
    if !s.message.is_empty() {
        lines.push(s.message.clone());
    }
    lines.push(format!("recording to {}", file.display()));
    for (row, l) in lines.iter().enumerate() {
        let clipped: String = l.chars().take(cols as usize).collect();
        queue!(o, cursor::MoveTo(0, row as u16), style::Print(clipped))?;
    }
    o.flush()
}

pub fn live(reg: Registry, want_source: Option<String>, output: Option<PathBuf>, seconds: Option<u64>, plain: bool) -> Result<(), String> {
    let tui = !plain && std::io::stdout().is_terminal() && std::io::stdin().is_terminal();
    let rt = runtime()?;
    let result = rt.block_on(async {
        let mut scanner = Scanner::start().await?;
        let mut core = reg.core();
        let (tx, mut inputs) = mpsc::unbounded_channel::<Input>();
        if tui {
            terminal::enable_raw_mode().map_err(|e| e.to_string())?;
            execute!(std::io::stdout(), terminal::EnterAlternateScreen, cursor::Hide).map_err(|e| e.to_string())?;
            std::thread::spawn(move || loop {
                match event::read() {
                    Ok(event::Event::Key(k)) if k.kind != event::KeyEventKind::Release => {
                        if tx.send(Input::Key(k)).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            });
        } else {
            let mut lines = stdin_lines();
            tokio::spawn(async move {
                while let Some(l) = lines.recv().await {
                    if tx.send(Input::Line(l)).is_err() {
                        return;
                    }
                }
                let _ = tx.send(Input::Closed);
            });
            eprintln!("listening … (lines you type: `ok power`, `wrong power 150`, anything else is a note, `q` stops)");
        }

        let start = Instant::now();
        let t = |i: Instant| (i - start).as_millis() as u64;
        let deadline = seconds.map(|s| start + Duration::from_secs(s));
        let mut s = LiveState { source_id: None, device_id: None, name: String::new(), fields: vec![], values: BTreeMap::new(), verdicts: vec![], stale: false, rssi: None, rate: Rate::default(), others: vec![], message: String::new() };
        let mut mode = Mode::Normal;
        let mut file: Option<std::io::BufWriter<std::fs::File>> = None;
        let mut path = output.clone().unwrap_or_else(|| PathBuf::from("(waiting for a device)"));
        let mut tick = tokio::time::interval(Duration::from_millis(1000));
        let mut inputs_open = true;
        let mut dirty = true;

        fn put(file: &mut Option<std::io::BufWriter<std::fs::File>>, line: &Line) -> Result<(), String> {
            match file {
                Some(f) => write_line(f, line).and_then(|_| f.flush()).map_err(|e| e.to_string()),
                None => Ok(()),
            }
        }

        loop {
            if tui && dirty {
                draw(&s, &mode, &path).map_err(|e| e.to_string())?;
                dirty = false;
            }
            tokio::select! {
                adv = scanner.next() => {
                    let Some(adv) = adv else { break };
                    let now = Instant::now();
                    let messages = core.on_advertisement(&Advertisement { device_id: &adv.device_id, name: adv.name.as_deref(), rssi: adv.rssi, manufacturer_data: &adv.mfr, services: &adv.services, now_ms: t(now) });
                    let mut ours = s.device_id.as_deref() == Some(adv.device_id.as_str());
                    for m in messages {
                        match m {
                            Message::SourceAppeared { source_id, name, fields, .. } => {
                                let wanted = want_source.as_ref().map_or(s.source_id.is_none(), |w| *w == source_id);
                                if wanted && s.source_id.is_none() {
                                    if output.is_none() {
                                        path = default_output(&source_id);
                                    }
                                    let mut f = std::io::BufWriter::new(std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?);
                                    let info = DeviceInfo { name: adv.name.clone(), company_id: company_id(&adv.mfr), services: adv.services.clone() };
                                    write_line(&mut f, &header(concat!("ofp ", env!("CARGO_PKG_VERSION")), info, vec![])).map_err(|e| e.to_string())?;
                                    file = Some(f);
                                    s.verdicts = vec![Verdict::Unchecked; fields.len()];
                                    s.fields = fields;
                                    s.name = name.clone();
                                    s.source_id = Some(source_id.clone());
                                    s.device_id = Some(adv.device_id.clone());
                                    ours = true;
                                    if !tui {
                                        println!("{name} ({source_id}) → {}", path.display());
                                    }
                                } else if !s.others.contains(&source_id) && s.source_id.as_ref() != Some(&source_id) {
                                    s.others.push(source_id);
                                }
                            }
                            Message::Values { source_id, fields, out_of_range, rssi, .. } if s.source_id.as_ref() == Some(&source_id) => {
                                s.rate.hit(now);
                                s.stale = false;
                                s.rssi = rssi.or(s.rssi);
                                s.values.extend(values_of(&fields));
                                for k in &out_of_range {
                                    s.values.remove(k);
                                }
                                if !tui {
                                    let line: Vec<String> = s.fields.iter().filter_map(|f| s.values.get(&f.key).map(|v| format!("{}={}", f.key, fmt_value(*v, f)))).collect();
                                    println!("{:>7.1} s  {}{}", start.elapsed().as_secs_f64(), line.join(" "), if out_of_range.is_empty() { String::new() } else { format!("  out of range: {}", out_of_range.join(",")) });
                                }
                            }
                            _ => {}
                        }
                    }
                    if ours {
                        put(&mut file, &Line::adv(t(now), adv.name.clone(), adv.rssi, &adv.mfr, BTreeMap::new()))?;
                        dirty = true;
                    }
                }
                _ = tick.tick() => {
                    for m in core.tick(t(Instant::now())) {
                        match m {
                            Message::Values { source_id, fields, .. } if s.source_id.as_ref() == Some(&source_id) => {
                                s.values.extend(values_of(&fields));
                            }
                            Message::SourceStale { source_id } if s.source_id.as_ref() == Some(&source_id) => {
                                s.stale = true;
                                if !tui {
                                    println!("{:>7.1} s  silent — values read as zero", start.elapsed().as_secs_f64());
                                }
                            }
                            _ => {}
                        }
                    }
                    dirty = true;
                }
                input = inputs.recv(), if inputs_open => {
                    dirty = true;
                    let now = t(Instant::now());
                    let confirm = |s: &mut LiveState, i: usize, shown: f64, console: Option<f64>, file: &mut Option<std::io::BufWriter<std::fs::File>>| -> Result<(), String> {
                        let ok = console.is_none_or(|c| (c - shown).abs() < 1e-9);
                        let console = if ok { None } else { console };
                        s.verdicts[i] = if ok { Verdict::Matches } else { Verdict::Wrong(console) };
                        s.message = format!("{}: {}", s.fields[i].name, verdict_text(&s.verdicts[i]));
                        put(file, &Line::Confirm { t: now, source_id: s.source_id.clone().unwrap_or_default(), field: s.fields[i].key.clone(), shown, console, ok })
                    };
                    match input {
                        None | Some(Input::Closed) => inputs_open = false,
                        Some(Input::Line(l)) => {
                            let words: Vec<&str> = l.split_whitespace().collect();
                            match words.as_slice() {
                                ["q"] => break,
                                [] => {}
                                [cmd @ ("ok" | "wrong"), field, rest @ ..] => {
                                    let found = s.fields.iter().position(|f| f.key == *field).and_then(|i| s.values.get(*field).map(|v| (i, *v)));
                                    match (found, *cmd, rest.first().and_then(|c| c.parse::<f64>().ok())) {
                                        (Some((i, shown)), "ok", _) => { confirm(&mut s, i, shown, None, &mut file)?; println!("  {}", s.message); }
                                        (Some((i, shown)), _, Some(console)) => { confirm(&mut s, i, shown, Some(console), &mut file)?; println!("  {}", s.message); }
                                        (Some(_), _, None) => println!("  say what the console shows: wrong {field} 150"),
                                        (None, _, _) => println!("  no value for '{field}' yet"),
                                    }
                                }
                                _ => {
                                    put(&mut file, &Line::Note { t: now, text: l.trim().to_string(), values: parse_note(&l) })?;
                                    println!("  noted");
                                }
                            }
                        }
                        Some(Input::Key(k)) => {
                            use event::KeyCode::*;
                            let ctrl_c = k.code == Char('c') && k.modifiers.contains(event::KeyModifiers::CONTROL);
                            mode = match (mode, k.code) {
                                (_, _) if ctrl_c => break,
                                (Mode::Normal, Char('q')) => break,
                                (Mode::Normal, Char('n')) => Mode::Note(String::new()),
                                (Mode::Normal, Char(c)) if c.is_ascii_digit() && c != '0' => {
                                    let i = c as usize - '1' as usize;
                                    match s.fields.get(i).and_then(|f| s.values.get(&f.key)) {
                                        Some(v) => Mode::Confirm(i, *v, String::new()),
                                        None => Mode::Normal,
                                    }
                                }
                                (Mode::Confirm(i, shown, typed), Enter) => {
                                    if typed.is_empty() {
                                        confirm(&mut s, i, shown, None, &mut file)?;
                                        Mode::Normal
                                    } else if let Ok(console) = typed.parse::<f64>() {
                                        confirm(&mut s, i, shown, Some(console), &mut file)?;
                                        Mode::Normal
                                    } else {
                                        s.message = "that is not a number".into();
                                        Mode::Confirm(i, shown, String::new())
                                    }
                                }
                                (Mode::Confirm(i, shown, mut typed), Char(c)) if c.is_ascii_digit() || c == '.' || c == '-' => { typed.push(c); Mode::Confirm(i, shown, typed) }
                                (Mode::Confirm(i, shown, mut typed), Backspace) => { typed.pop(); Mode::Confirm(i, shown, typed) }
                                (Mode::Note(typed), Enter) => {
                                    if !typed.trim().is_empty() {
                                        put(&mut file, &Line::Note { t: now, text: typed.trim().to_string(), values: parse_note(&typed) })?;
                                        s.message = "noted".into();
                                    }
                                    Mode::Normal
                                }
                                (Mode::Note(mut typed), Char(c)) => { typed.push(c); Mode::Note(typed) }
                                (Mode::Note(mut typed), Backspace) => { typed.pop(); Mode::Note(typed) }
                                (Mode::Confirm(..) | Mode::Note(_), Esc) => Mode::Normal,
                                (m, _) => m,
                            };
                        }
                    }
                }
                _ = tokio::signal::ctrl_c() => break,
                _ = async { tokio::time::sleep_until(deadline.unwrap().into()).await }, if deadline.is_some() => break,
            }
        }
        scanner.stop().await;
        Ok::<_, String>((s, path))
    });
    if tui {
        let _ = execute!(std::io::stdout(), cursor::Show, terminal::LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
    let (s, path) = result?;
    let Some(source) = &s.source_id else {
        println!("No device that a table knows was heard.");
        return Ok(());
    };
    println!("{} ({source}): {:.0} packets/min, longest gap {:.1} s", s.name, s.rate.per_minute(Instant::now()), s.rate.longest_gap.as_secs_f64());
    for (f, v) in s.fields.iter().zip(&s.verdicts) {
        println!("  {:<16} {}", f.name, verdict_text(v));
    }
    let table = source.split(':').next().unwrap_or("");
    println!("capture: {}", path.display());
    if s.verdicts.iter().any(|v| *v != Verdict::Unchecked) {
        println!("To make it part of the table's evidence: copy it to registry/tables/{table}.capture.jsonl and run `ofp check`.");
    }
    // The key thread may still sit in a blocking read.
    rt.shutdown_background();
    Ok(())
}
