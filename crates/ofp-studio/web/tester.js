// The Tester: connect to a device and see what it says in the standard Bluetooth profiles
// (FTMS, Cycling Power, Speed and Cadence, Heart Rate), control a trainer, read the log, and
// download a report for troubleshooting. Works from the same recording as the other steps.
'use strict';

const Tester = (() => {
  const SERVICE_NAMES = { '1826': 'Fitness Machine (FTMS)', '1818': 'Cycling Power', '1816': 'Cycling Speed and Cadence', '180d': 'Heart Rate', '180a': 'Device Information', '180f': 'Battery', '1814': 'Running Speed and Cadence' };
  const CHAR_NAMES = {
    '2ad2': 'Indoor Bike Data', '2ad9': 'Fitness Machine Control Point', '2ada': 'Fitness Machine Status', '2acc': 'Fitness Machine Feature', '2ad6': 'Supported Resistance Range', '2ad8': 'Supported Power Range',
    '2acd': 'Treadmill Data', '2ad1': 'Rower Data', '2ace': 'Cross Trainer Data', '2ad3': 'Training Status',
    '2a63': 'Cycling Power Measurement', '2a65': 'Cycling Power Feature', '2a5d': 'Sensor Location', '2a66': 'Cycling Power Control Point', '2a64': 'Cycling Power Vector',
    '2a5b': 'CSC Measurement', '2a5c': 'CSC Feature', '2a37': 'Heart Rate Measurement', '2a38': 'Body Sensor Location', '2a19': 'Battery Level',
    '2a29': 'Manufacturer Name', '2a24': 'Model Number', '2a26': 'Firmware Revision', '2a27': 'Hardware Revision', '2a28': 'Software Revision', '2a25': 'Serial Number',
  };
  const CP_REQUESTS = { 0x00: 'Request control', 0x01: 'Reset', 0x04: 'Set resistance', 0x05: 'Set target power', 0x07: 'Start', 0x08: 'Stop / pause', 0x11: 'Set slope' };
  const CP_RESULTS = { 0x01: 'success', 0x02: 'not supported', 0x03: 'invalid parameter', 0x04: 'operation failed', 0x05: 'control not permitted' };
  const WHEEL_M = 2.105;

  let T;
  function reset() {
    T = {
      src: { FTMS: {}, CPS: {}, CSC: {}, HR: {} },   // latest values per source
      seen: {},                                      // characteristic → { count, first, last, gap }
      log: [],                                       // { t, dir, chr, hex, text }
      control: { granted: false, last: '' },
      prev: {},                                      // for cadence/speed from revolution counters
      ftmsFlags: null,
      mismatch: 0, compared: 0,
      zeroPowerSince: null, zeroPowerWorst: 0,
      failures: [],
    };
  }
  reset();

  const u16 = (b, i) => b[i] | (b[i + 1] << 8);
  const s16 = (b, i) => { const v = u16(b, i); return v > 32767 ? v - 65536 : v; };
  const u24 = (b, i) => b[i] | (b[i + 1] << 8) | (b[i + 2] << 16);
  const u32 = (b, i) => (b[i] | (b[i + 1] << 8) | (b[i + 2] << 16) | (b[i + 3] << 24)) >>> 0;
  const toBytes = (hex) => { const o = []; for (let i = 0; i < hex.length; i += 2) o.push(parseInt(hex.substr(i, 2), 16)); return o; };
  const toHex = (bytes) => bytes.map((b) => (b & 0xff).toString(16).padStart(2, '0')).join('');

  /** FTMS Indoor Bike Data (0x2AD2): which values are present is said by the flags. */
  function indoorBike(b) {
    if (b.length < 2) return {};
    const flags = u16(b, 0);
    let i = 2;
    const o = { flags };
    const need = (n) => i + n <= b.length;
    if (!(flags & 0x0001)) { if (need(2)) o.speed = u16(b, i) / 100; i += 2; }
    if (flags & 0x0002) i += 2;                                              // average speed
    if (flags & 0x0004) { if (need(2)) o.cadence = u16(b, i) / 2; i += 2; }
    if (flags & 0x0008) i += 2;                                              // average cadence
    if (flags & 0x0010) { if (need(3)) o.distance = u24(b, i); i += 3; }
    if (flags & 0x0020) { if (need(2)) o.resistance = s16(b, i); i += 2; }
    if (flags & 0x0040) { if (need(2)) o.power = s16(b, i); i += 2; }
    if (flags & 0x0080) i += 2;                                              // average power
    if (flags & 0x0100) { if (need(2)) o.energy = u16(b, i); i += 5; }
    if (flags & 0x0200) { if (need(1)) o.heartRate = b[i]; i += 1; }
    if (flags & 0x0400) i += 1;                                              // metabolic equivalent
    if (flags & 0x0800) { if (need(2)) o.elapsed = u16(b, i); i += 2; }
    return o;
  }

  /** Revolutions and an event time (1/`hz` s) → per minute, from the change since the last packet. */
  function rate(key, revs, time, hz, revMax, t) {
    const p = T.prev[key];
    T.prev[key] = { revs, time, t };
    if (!p) return undefined;
    const dr = (revs - p.revs + revMax) % revMax, dt = ((time - p.time + 65536) % 65536) / hz;
    if (dt === 0) return t - p.t > 3000 ? 0 : undefined;   // no new event: stopped, after a moment
    if (dr === 0 || dr > 200) return undefined;
    return (dr / dt) * 60;
  }

  /** Cycling Power Measurement (0x2A63). */
  function cyclingPower(b, t) {
    if (b.length < 4) return {};
    const flags = u16(b, 0);
    const o = { power: s16(b, 2) };
    let i = 4;
    if (flags & 0x01) i += 1;                // pedal power balance
    if (flags & 0x04) i += 2;                // accumulated torque
    if (flags & 0x10) { if (i + 6 <= b.length) { const rpm = rate('cps-wheel', u32(b, i), u16(b, i + 4), 2048, 4294967296, t); if (rpm !== undefined) o.speed = (rpm * WHEEL_M * 60) / 1000; } i += 6; }
    if (flags & 0x20) { if (i + 4 <= b.length) { const rpm = rate('cps-crank', u16(b, i), u16(b, i + 2), 1024, 65536, t); if (rpm !== undefined) o.cadence = Math.round(rpm); } i += 4; }
    return o;
  }

  /** CSC Measurement (0x2A5B). */
  function csc(b, t) {
    if (b.length < 1) return {};
    const flags = b[0];
    const o = {};
    let i = 1;
    if (flags & 0x01) { if (i + 6 <= b.length) { const rpm = rate('csc-wheel', u32(b, i), u16(b, i + 4), 1024, 4294967296, t); if (rpm !== undefined) o.speed = (rpm * WHEEL_M * 60) / 1000; } i += 6; }
    if (flags & 0x02) { if (i + 4 <= b.length) { const rpm = rate('csc-crank', u16(b, i), u16(b, i + 2), 1024, 65536, t); if (rpm !== undefined) o.cadence = Math.round(rpm); } }
    return o;
  }

  const heartRate = (b) => (b.length < 2 ? {} : { heartRate: b[0] & 0x01 ? u16(b, 1) : b[1] });

  function describe(o) {
    const parts = [];
    if (o.power !== undefined) parts.push(o.power + ' W');
    if (o.cadence !== undefined) parts.push(o.cadence + ' rpm');
    if (o.speed !== undefined) parts.push(o.speed.toFixed(1) + ' km/h');
    if (o.heartRate !== undefined) parts.push(o.heartRate + ' bpm');
    if (o.resistance !== undefined) parts.push('resistance ' + o.resistance);
    return parts.join(' · ');
  }

  function log(t, dir, chr, hex, text) {
    T.log.push({ t, dir, chr, hex, text });
    if (T.log.length > 3000) T.log.splice(0, T.log.length - 3000);
  }

  /** One notification (or a value read after connecting). */
  function onPacket(t, chr, hex, dir = 'rx') {
    const b = toBytes(hex);
    const seen = (T.seen[chr] = T.seen[chr] || { count: 0, first: t, last: t, gap: 0 });
    if (dir === 'rx') { if (seen.count) seen.gap = Math.max(seen.gap, t - seen.last); seen.count++; seen.last = t; }
    let text = '';
    const merge = (name, o) => { Object.assign(T.src[name], o, { at: t }); text = describe(o); };
    if (chr === '2ad2') { const o = indoorBike(b); T.ftmsFlags = o.flags; merge('FTMS', o); }
    else if (chr === '2a63') merge('CPS', cyclingPower(b, t));
    else if (chr === '2a5b') merge('CSC', csc(b, t));
    else if (chr === '2a37') merge('HR', heartRate(b));
    else if (chr === '2ad9' && b[0] === 0x80 && b.length >= 3) {
      const req = CP_REQUESTS[b[1]] || 'request 0x' + b[1].toString(16), res = CP_RESULTS[b[2]] || 'result 0x' + b[2].toString(16);
      text = `${req}: ${res}`;
      T.control.last = text;
      if (b[1] === 0x00 && b[2] === 0x01) T.control.granted = true;
      if (b[2] !== 0x01) T.failures.push({ t, text });
    } else if (chr === '2ada') text = 'machine status 0x' + (b[0] || 0).toString(16);
    else if (dir === 'read' && ['2a29', '2a24', '2a26', '2a27', '2a28', '2a25'].includes(chr)) text = String.fromCharCode(...b.filter((x) => x >= 32 && x < 127));
    else if (chr === '2a19' && b.length) text = b[0] + ' % battery';
    // two sources for power: do they agree?
    const f = T.src.FTMS, c = T.src.CPS;
    if ((chr === '2ad2' || chr === '2a63') && f.power > 20 && c.power > 20 && Math.abs(f.at - c.at) < 1500) {
      T.compared++;
      if (Math.abs(f.power - c.power) > Math.max(10, 0.1 * c.power)) T.mismatch++;
    }
    // pedalling but no watts
    const power = f.power ?? c.power, cadence = f.cadence ?? c.cadence ?? T.src.CSC.cadence;
    if (power === 0 && cadence > 30) { T.zeroPowerSince = T.zeroPowerSince ?? t; T.zeroPowerWorst = Math.max(T.zeroPowerWorst, t - T.zeroPowerSince); } else T.zeroPowerSince = null;
    log(t, dir, chr, hex, text);
  }

  function onTx(m) {
    const b = toBytes(m.hex);
    let text = m.chr === '2ad9' ? (CP_REQUESTS[b[0]] || 'command 0x' + (b[0] || 0).toString(16)) : '';
    if (m.chr === '2ad9' && b[0] === 0x05) text += ' ' + s16(b, 1) + ' W';
    if (m.chr === '2ad9' && b[0] === 0x11) text += ' ' + (s16(b, 3) / 100).toFixed(1) + ' %';
    if (m.error) { text += ' — FAILED: ' + m.error; T.failures.push({ t: m.t, text }); }
    log(m.t, 'tx', m.chr, m.hex, text);
  }

  /** Rebuild from a loaded capture (a file, or after a reload of the page). */
  function rebuild(capture) {
    reset();
    if (!capture) return;
    const all = [];
    for (const p of capture.packets) if (p[1] !== 'adv') all.push({ t: p[0], dir: 'rx', chr: p[1], hex: p[2] });
    for (const x of capture.io || []) all.push({ t: x[0], dir: x[1] === 'write' ? 'tx' : 'read', chr: x[2], hex: x[3] });
    all.sort((a, b) => a.t - b.t);
    for (const e of all) e.dir === 'tx' ? onTx({ t: e.t, chr: e.chr, hex: e.hex }) : onPacket(e.t, e.chr, e.hex, e.dir);
  }

  const gattOf = (header) => (header && header.gatt) || [];
  const hasChr = (gatt, uuid) => gatt.some((s) => s.characteristics.some((c) => c.uuid === uuid));
  const hasSvc = (gatt, uuid) => gatt.some((s) => s.uuid === uuid);

  /** What looks wrong, in plain words. Each finding: { level: 'bad' | 'warn' | 'good', text }. */
  function findings(header, connected, now) {
    const gatt = gattOf(header);
    const out = [];
    if (!gatt.length) return out;
    const standard = ['1826', '1818', '1816', '180d'].filter((u) => hasSvc(gatt, u));
    if (!standard.length) out.push({ level: 'warn', text: 'No standard fitness service (FTMS, Cycling Power, Speed and Cadence, Heart Rate). An app needs a table (or its own code) to read this device: use Record and Map.' });
    else out.push({ level: 'good', text: 'Standard services: ' + standard.map((u) => SERVICE_NAMES[u]).join(', ') + '.' });
    if (hasSvc(gatt, '1826')) {
      if (!hasChr(gatt, '2ad9')) out.push({ level: 'warn', text: 'FTMS without a control point: the machine can be read but not controlled (no ERG mode, no slope).' });
      else if (!T.control.granted && T.log.some((l) => l.dir === 'tx')) out.push({ level: 'bad', text: 'Control was asked for but not granted' + (T.control.last ? ` (${T.control.last})` : '') + '. Another app may be controlling the trainer: close it and reconnect.' });
      if (T.ftmsFlags !== null) {
        if (!(T.ftmsFlags & 0x40)) out.push({ level: 'warn', text: 'The FTMS data has no power in it. Apps reading only FTMS will show no watts' + (hasSvc(gatt, '1818') ? '; Cycling Power has it.' : '.') });
        if (!(T.ftmsFlags & 0x04)) out.push({ level: 'warn', text: 'The FTMS data has no cadence in it' + (T.src.CPS.cadence !== undefined || T.src.CSC.cadence !== undefined ? '; another service has it.' : '.') });
      }
    }
    for (const s of gatt) for (const c of s.characteristics) {
      if (!c.properties.includes('notify') && !c.properties.includes('indicate')) continue;
      if (['2ad9', '2a66', '2ada', '2ad3'].includes(c.uuid)) continue;   // these only speak when something happens
      const seen = T.seen[c.uuid];
      const name = CHAR_NAMES[c.uuid] || c.uuid;
      if (!seen || !seen.count) { if (now > 10000 && ['2ad2', '2a63', '2a5b', '2a37', '2acd', '2ad1'].includes(c.uuid)) out.push({ level: 'bad', text: `${name}: subscribed, but nothing has arrived. Is the device awake (pedalling, strap on skin)?` }); continue; }
      const span = (seen.last - seen.first) / 1000;
      if (span > 10 && seen.count / span < 0.8) out.push({ level: 'warn', text: `${name}: only ${(seen.count / span).toFixed(1)} messages a second (about 1 or more is normal).` });
      if (seen.gap > 3500) out.push({ level: 'warn', text: `${name}: a gap of ${(seen.gap / 1000).toFixed(1)} s with no data. Apps show a dropout after about 3 s. Usually distance, interference, or a weak battery.` });
      if (connected && now - seen.last > 5000) out.push({ level: 'bad', text: `${name}: silent for ${Math.round((now - seen.last) / 1000)} s.` });
    }
    if (T.compared > 10 && T.mismatch / T.compared > 0.3) out.push({ level: 'warn', text: `FTMS and Cycling Power disagree on the watts in ${Math.round((100 * T.mismatch) / T.compared)} % of readings (now ${T.src.FTMS.power} vs ${T.src.CPS.power} W).` });
    if (T.zeroPowerWorst > 10000) out.push({ level: 'warn', text: `Cadence but 0 W for ${Math.round(T.zeroPowerWorst / 1000)} s: the power meter may need waking or calibrating.` });
    for (const f of T.failures.slice(-3)) out.push({ level: 'bad', text: f.text });
    return out;
  }

  /** FTMS control point commands, as the bytes to write. */
  const commands = {
    requestControl: () => [0x00],
    start: () => [0x07],
    stop: () => [0x08, 0x01],
    pause: () => [0x08, 0x02],
    power: (watts) => [0x05, watts & 0xff, (watts >> 8) & 0xff],
    resistance: (level) => [0x04, level & 0xff, (level >> 8) & 0xff],
    // wind speed 0, grade in 0.01 %, rolling resistance 0.004, wind resistance 0.51 kg/m
    slope: (percent) => { const g = Math.round(percent * 100) & 0xffff; return [0x11, 0x00, 0x00, g & 0xff, (g >> 8) & 0xff, 0x28, 0x33]; },
  };

  function report(session, header, now) {
    const gatt = gattOf(header);
    const L = [];
    const rule = (title) => L.push('', '=====================================', title, '=====================================');
    L.push('OFP STUDIO — DEVICE REPORT', 'Generated: ' + new Date().toISOString(), 'Device: ' + (session ? session.name : '?'), 'Recorded: ' + Math.round(now / 1000) + ' s');
    rule('WHAT WE FOUND');
    for (const f of findings(header, false, now)) L.push((f.level === 'good' ? '[ok]   ' : f.level === 'bad' ? '[FAIL] ' : '[note] ') + f.text);
    rule('SERVICES AND CHARACTERISTICS');
    for (const s of gatt) {
      L.push(`● ${SERVICE_NAMES[s.uuid] || 'Unknown service'}  (${s.uuid})`);
      for (const c of s.characteristics) L.push(`    - ${c.uuid}  ${CHAR_NAMES[c.uuid] || ''}  [${c.properties.join(', ')}]`);
    }
    rule('LATEST VALUES BY SOURCE');
    for (const [name, v] of Object.entries(T.src)) if (v.at !== undefined) L.push(`${name}: ${describe(v) || '(no values)'}`);
    rule('DATA RATES');
    for (const [chr, s] of Object.entries(T.seen)) { const span = Math.max(1, (s.last - s.first) / 1000); L.push(`${chr} ${CHAR_NAMES[chr] || ''}: ${s.count} messages, ${(s.count / span).toFixed(1)}/s, longest gap ${(s.gap / 1000).toFixed(1)} s`); }
    rule('COMMANDS SENT (TX)');
    const tx = T.log.filter((l) => l.dir === 'tx');
    if (!tx.length) L.push('(none)');
    for (const l of tx) L.push(`${(l.t / 1000).toFixed(1).padStart(8)} s  ${l.chr}  ${l.hex}  ${l.text}`);
    rule('LOG (last 500 lines)');
    for (const l of T.log.slice(-500)) L.push(`${(l.t / 1000).toFixed(1).padStart(8)} s  ${l.dir.toUpperCase().padEnd(4)} ${l.chr.padEnd(6)} ${l.hex}${l.text ? '   ' + l.text : ''}`);
    return L.join('\n');
  }

  return { onPacket, onTx, rebuild, findings, commands, report, toHex, state: () => T, SERVICE_NAMES, CHAR_NAMES, gattOf, hasChr };
})();
