// OFP Studio page. Plain JavaScript, no build step. The server holds the session; this page shows it
// and follows the live event stream, so what an assistant changes over the API appears here too.
'use strict';

const S = {
  tab: 'devices',
  // the full Studio (tabs for writing tables); off = the simple Connect, check, send
  advanced: localStorage.getItem('ofp-advanced') === '1',
  sent: null,         // { code } once this recording was sent to the project
  state: null,        // GET /api/state
  devices: [],
  captures: [],
  capture: null,      // { packets: [[t, stream, hex]], notes, confirms, io }
  header: null,       // the capture's first line (device, services found after connecting)
  stream: null,
  sel: [],            // selected byte positions (Map step)
  expr: '',
  series: null,
  field: '',
  solve: null,
  similar: null,
  yaml: '',
  yamlDirty: false,
  checked: null,
  saved: null,
  live: { source: null, values: {}, stale: false },
  radio: 'off',
};

const $ = (sel, root = document) => root.querySelector(sel);

function h(tag, attrs = {}, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k === 'class') el.className = v;
    else if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else if (k === 'value') el.value = v;
    else if (k === 'checked' || k === 'disabled' || k === 'selected' || k === 'hidden') el[k] = !!v;
    else el.setAttribute(k, v);
  }
  for (const kid of kids.flat()) {
    if (kid == null || kid === false) continue;
    el.append(kid.nodeType ? kid : document.createTextNode(String(kid)));
  }
  return el;
}

let toastTimer;
function toast(text, bad = false) {
  const t = $('#toast');
  t.textContent = text;
  t.className = 'toast' + (bad ? ' bad' : '');
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { t.hidden = true; }, bad ? 7000 : 2600);
}

async function api(path, body) {
  const res = await fetch(path, body === undefined ? {} : { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) {
    // The page believed a device was chosen and the Studio says otherwise (it was restarted): resync
    if (/Choose a device/.test(data.error || '') && session()) { await reload(); setTab('devices'); throw new Error('The Studio was restarted: choose the device again.'); }
    throw new Error(data.error || res.statusText);
  }
  return data;
}
const guard = (fn) => async (...args) => { try { return await fn(...args); } catch (e) { toast(e.message, true); } };

const session = () => S.state && S.state.session;
const fields = () => (S.state && S.state.registry.fields) || [];
const fieldOf = (key) => fields().find((f) => f.key === key);
const bytesOf = (hex) => { const out = []; for (let i = 0; i < hex.length; i += 2) out.push(parseInt(hex.substr(i, 2), 16)); return out; };
const fmt = (v, f) => (v == null ? '–' : Number(v).toFixed(f ? f.decimals : (Number.isInteger(v) ? 0 : 1)));
const secs = (ms) => { const s = Math.round(ms / 1000); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`; };

// ───────────────────────── loading ─────────────────────────

async function loadState() {
  S.state = await api('/api/state');
  S.radio = S.state.radio;
  const s = session();
  if (s) {
    if (!S.yamlDirty) S.yaml = s.draft || '';
    S.live.source = s.live.source;
    S.live.values = { ...s.live.values };
    if (!S.stream || !s.streams.some((x) => x.key === S.stream)) S.stream = s.streams[0] ? s.streams[0].key : null;
  }
}

async function loadCapture() {
  if (!session()) { S.capture = null; S.header = null; S.similar = null; Tester.rebuild(null); return; }
  const c = await api('/api/capture');
  S.capture = { packets: c.packets, notes: c.notes, confirms: c.confirms, io: c.io };
  S.header = c.header;
  Tester.rebuild(S.capture);
  S.similar = c.packets.length ? await api('/api/similar').catch(() => null) : null;
}

async function reload() {
  await loadState();
  await loadCapture().catch(() => { S.capture = null; });
  S.captures = await api('/api/captures').catch(() => []);
  render();
}

function streamPackets() {
  if (!S.capture || !S.stream) return [];
  return S.capture.packets.filter((p) => p[1] === S.stream);
}

// ───────────────────────── events ─────────────────────────

let framePending = false;
function renderSoon() {
  if (framePending) return;
  framePending = true;
  requestAnimationFrame(() => { framePending = false; renderLive(); });
}

function connectEvents() {
  const es = new EventSource('/api/events');
  // The stream reconnects by itself when the Studio was restarted. The session lives in the Studio, so
  // what this page shows may no longer be true: ask again, and say so if the device has to be chosen again.
  let opened = false;
  es.onopen = guard(async () => {
    if (!opened) { opened = true; return; }
    const had = !!session();
    await reload();
    if (had && !session()) { toast('The Studio was restarted: choose the device again.', true); setTab('devices'); }
  });
  es.onmessage = guard(async (ev) => {
    const m = JSON.parse(ev.data);
    switch (m.type) {
      case 'devices': S.devices = m.devices; if (S.tab === 'devices') renderDevicesTable(); if (!S.advanced) renderDeviceCards(); break;
      case 'packet':
        S.lastPacketAt = Date.now();
        if (S.capture) S.capture.packets.push([m.t, m.stream, m.hex]);
        if (m.stream !== 'adv') Tester.onPacket(m.t, m.stream, m.hex);
        if (!S.stream) { S.stream = m.stream; render(); } else renderSoon();
        break;
      case 'tx': Tester.onTx(m); if (m.error) toast(m.error, true); renderSoon(); break;
      case 'note': if (S.capture) S.capture.notes.push(m); if (showingLive()) renderTiles(); else render(); break;
      case 'confirm': if (S.capture) S.capture.confirms.push(m); renderSoon(); break;
      case 'source': S.live.source = m.source; if (showingLive()) renderTiles(); break;
      case 'values': Object.assign(S.live.values, m.fields); S.live.stale = !!m.stale; for (const k of m.out_of_range || []) delete S.live.values[k]; if (showingLive()) renderTiles(); break;
      case 'recording': if (m.on) S.lastPacketAt = null; await loadState(); render(); break;
      case 'session': S.sel = []; S.expr = ''; S.series = null; S.solve = null; S.checked = null; S.saved = null; S.yamlDirty = false; S.live = { source: null, values: {}, stale: false }; await reload(); break;
      case 'draft': S.yaml = m.yaml || ''; S.yamlDirty = false; await loadState(); render(); break;
      case 'checked':
        if (!S.yamlDirty) S.yaml = m.yaml;
        S.checked = { ...(S.checked && S.checked.yaml === m.yaml ? S.checked : {}), ok: m.ok, problems: m.problems, summary: m.summary, compiled: true, yaml: m.yaml };
        if (S.tab === 'save') render();
        break;
      case 'radio': S.radio = m.status; renderChip(); if (m.status.startsWith('error')) toast(m.status.slice(7), true); break;
      case 'connected': toast('Connected. Recording what it sends.'); await reload(); break;
    }
  });
}

// ───────────────────────── chrome ─────────────────────────

function renderChip() {
  const s = session();
  const chip = $('#chip');
  chip.replaceChildren();
  if (s) {
    chip.append(h('span', { class: 'dot' + (s.recording ? ' rec' : '') }), h('span', {}, s.name + (s.from_file ? ' (capture file)' : s.recording ? ' · recording' : '')));
  } else {
    chip.append(h('span', {}, 'no device chosen'));
  }
  chip.append(h('span', { class: 'muted' }, '· radio: ' + S.radio.replace(/^error: .*/, 'problem')));
}

function renderBanner() {
  const b = $('#banner');
  const existing = S.similar && S.similar.existing_tables && S.similar.existing_tables[0];
  if (!existing || !session()) { b.hidden = true; return; }
  b.hidden = false;
  b.replaceChildren(
    h('b', {}, `The registry already knows this device (table “${existing.table}”). `),
    'Extend that table rather than adding a second one. ',
    h('button', { class: 'small', onclick: guard(async () => { S.yaml = existing.yaml; S.yamlDirty = true; setTab('save'); await runCheck(); }) }, 'Open that table'),
  );
}

function setTab(tab) {
  S.tab = tab;
  render();
  // The list of recordings is read when its tab is opened: one made a moment ago is in it
  if (tab === 'recordings') api('/api/captures').then((c) => { S.captures = c; if (S.tab === 'recordings') render(); }).catch(() => {});
}

function render() {
  for (const b of document.querySelectorAll('#tabs button')) b.classList.toggle('on', b.dataset.tab === S.tab);
  renderChip();
  renderBanner();
  const main = $('#main');
  document.body.classList.toggle('simple', !S.advanced);
  $('#advanced').checked = S.advanced;
  const view = !S.advanced ? viewSimple() : { devices: viewDevices, record: viewRecord, map: viewMap, live: viewLive, save: viewSave, tester: viewTester, recordings: viewRecordings }[S.tab]();
  main.replaceChildren(view);
  renderLive();
}

/** Things that change with every packet: grids, counters, charts that follow the capture. */
function renderLive() {
  for (const el of document.querySelectorAll('[data-grid]')) fillGrid(el);
  const st = $('#rec-stats');
  if (st) fillStats(st);
  if (S.tab === 'tester') renderTester();
  if (showingLive()) renderTiles();
}

function needSession(title) {
  return h('div', { class: 'panel empty' }, h('p', {}, title), h('button', { class: 'primary', onclick: () => setTab('devices') }, 'Choose a device'));
}

// ───────────────────────── 1 Devices ─────────────────────────

function viewDevices() {
  const wrap = h('div', {});
  wrap.append(h('div', { class: 'panel' },
    h('h2', {}, 'Devices in range'),
    h('p', { class: 'hint' }, 'Devices something can read are listed with what reads them: a standard Bluetooth profile, or an OFP table. Wake your device (pedal, press a button) and it shows up within a few seconds. Click Use and you see its values.'),
    h('div', { class: 'scroll' }, h('table', { id: 'devices-table' })),
  ));
  setTimeout(renderDevicesTable);
  return wrap;
}

// ───────────────────────── Recordings (the last tab) ─────────────────────────

function viewRecordings() {
  const wrap = h('div', {});
  const caps = h('div', { class: 'panel' }, h('h2', {}, 'Earlier recordings'), h('p', { class: 'hint' }, 'Every time you use a device it is recorded into its own file. Open one to look at it again, or to send it to someone.'));
  if (!S.captures.length) caps.append(h('p', { class: 'muted' }, 'None yet.'));
  // A recording someone sent you (they made it with this page and "Download the recording")
  const pick = h('input', { type: 'file', accept: '.jsonl,.txt', onchange: guard(async (e) => {
    const file = e.target.files[0];
    if (!file) return;
    await api('/api/capture/upload', { name: file.name, text: await file.text() });
    toast('Opened ' + file.name);
    setTab('live');
  }) });
  caps.append(h('div', { class: 'row', style: 'margin-bottom:10px' }, h('b', {}, 'A file someone sent you:'), pick));
  const list = h('ul', { class: 'list' });
  for (const c of S.captures) {
    list.append(h('li', {}, h('span', { class: 'mono fill' }, c.name), h('span', { class: 'muted' }, c.kb + ' KB'), c.place === 'registry' ? h('span', { class: 'badge' }, 'ships with a table') : null,
      h('button', { class: 'small', onclick: guard(async () => { await api('/api/session', { capture: c.path }); setTab('map'); }) }, 'Open')));
  }
  caps.append(list);
  wrap.append(caps);
  return wrap;
}

/** Silent this long: the row is dimmed and cannot be chosen (the server drops it after a minute). */
const DEVICE_QUIET_S = 20;
const deviceRows = new Map(); // device id → its row: rows are updated in place, never rebuilt, so nothing blinks
let showOthers = false;

/** The standard Bluetooth profiles for fitness devices: a device advertising one can be read with no table. */
const STANDARD_SERVICES = { '1826': 'FTMS', '1818': 'Cycling Power', '1816': 'Speed and Cadence', '180d': 'Heart Rate', '1814': 'Running Speed and Cadence', '1809': 'Thermometer' };

/** What reads this device: an OFP table, standard profiles, or nothing yet. */
function readsAs(d) {
  const by = [];
  // a Fitness Machine says which kind it is (bike, rower, treadmill) only by what it sends once connected
  if (d.known) by.push(d.known.startsWith('Fitness machine') ? 'Fitness machine (FTMS)' + (d.known.includes(' · ') ? d.known.slice(d.known.indexOf(' · ')) : '') : d.known);
  // a standard the core already names (its table "Heart Rate", "Fitness machine (bike)", …) is not repeated
  const named = (d.known || '').toLowerCase();
  for (const u of d.services) {
    const label = STANDARD_SERVICES[u];
    if (label && !(named.startsWith('fitness machine') && u === '1826') && !named.startsWith(label.toLowerCase())) by.push(label);
  }
  return by;
}

/**
 * Words in a device's name that say "fitness equipment or a sensor": kinds of machine, and makers that
 * make little else. Only for sorting the list: a device nothing reads yet that matches is shown, not folded
 * away with the phones and headsets. A miss costs nothing (it is one click away under "Don't see your
 * device?"), so the list is short and plain, not complete.
 */
const FITNESS_WORDS = /bike|cycl|spin|trainer|treadmill|\brow(er|ing)?\b|\berg\b|ellip|stepper|fitness|\bgym|cadence|watt|\bhrm?\b|heart|strap|pedal|crank|breath|\bm3i?\b|keiser|wahoo|kickr|tickr|tacx|saris|wattbike|concept2|\bc2\b|pm5|peloton|echelon|schwinn|bowflex|nordictrack|ifit|proform|technogym|life ?fitness|precor|yesoul|stages|assioma|favero|quarq|4iiii|\bsrm\b|zwift|garmin|polar|suunto|coros|\bcore\b|tyme|moxy|stryd|magene|thinkrider|xoss|igpsport|bryton|hammerhead|domyos|kettler|bkool|jetblack|cyclops|van ?rysel|direto|suito|csc\b/i;
/** Nothing reads it yet, but its name says fitness equipment: worth showing without being asked. */
function looksLikeFitness(d) {
  return !!d.name && FITNESS_WORDS.test(d.name);
}

function renderDevicesTable() {
  const t = $('#devices-table');
  if (!t) return;
  const cur = session() && session().device_id;
  if (!t.tHead) {
    deviceRows.clear();
    t.replaceChildren();
    t.createTHead().append(h('tr', {}, h('th', {}, 'Device'), h('th', {}, 'Read as'), h('th', { class: 'num' }, 'Heard / min'), h('th', { class: 'num' }, 'Longest gap'), h('th', { class: 'num' }, 'Signal'), h('th', {}, '')));
    // three bodies: what can be read; what nothing reads yet but looks like fitness equipment (a device to
    // add); and everything else nearby (folded away until asked for)
    t.append(h('tbody', { id: 'devices-readable' }),
      h('tbody', { id: 'devices-maybe' }, h('tr', { class: 'group' }, h('td', { colspan: 6 }, h('b', {}, 'Looks like a fitness device, and nothing reads it yet. '), 'Click Use to record what it sends: that is the first step of adding it.'))),
      h('tbody', { id: 'devices-toggle' }, h('tr', {}, h('td', { colspan: 6 },
        h('button', { class: 'small', id: 'others-button', onclick: () => { showOthers = !showOthers; renderDevicesTable(); } })))), h('tbody', { id: 'devices-others' }));
  }
  const readable = $('#devices-readable'), maybe = $('#devices-maybe'), others = $('#devices-others');
  const seen = new Set();
  const last = { readable: null, maybe: maybe.firstChild, others: null };
  let otherCount = 0, readableCount = 0, maybeCount = 0;
  for (const d of S.devices) {
    seen.add(d.device_id);
    const by = readsAs(d);
    // the chosen device stays on top whatever it is
    const group = by.length || d.device_id === cur ? 'readable' : looksLikeFitness(d) ? 'maybe' : 'others';
    if (group === 'others') otherCount++; else if (group === 'maybe') maybeCount++; else readableCount++;
    const body = { readable, maybe, others }[group];
    let row = deviceRows.get(d.device_id);
    if (!row) {
      // One click: choose it, start recording, show its values. (Whether it has to be connected to is
      // worked out from what it advertises.)
      const use = h('button', { class: 'small primary', onclick: guard(async () => {
        if (!(session() && session().device_id === d.device_id)) {
          await api('/api/session', { device_id: d.device_id });
          await api('/api/record/start', {});
          await loadState();
        }
        setTab('live');
      }) }, 'Use');
      row = h('tr', {}, h('td', {}), h('td', {}), h('td', { class: 'num' }), h('td', { class: 'num' }), h('td', { class: 'num' }), h('td', {}, use, h('span', { class: 'badge', hidden: true }, 'chosen')));
      deviceRows.set(d.device_id, row);
    }
    // keep the order within each group without moving rows that are already in place
    const want = last[group] ? last[group].nextSibling : body.firstChild;
    if (want !== row) body.insertBefore(row, want);
    last[group] = row;

    const quiet = d.silent_s > DEVICE_QUIET_S;
    const set = (cell, text) => { if (cell.textContent !== text) cell.textContent = text; };
    set(row.cells[0], d.name || '(no name)');
    row.cells[0].className = d.name ? '' : 'muted';
    const sends = [];
    if (d.companies.length) sends.push('sends data in its adverts (maker ' + d.companies.map((c) => '0x' + c).join(', ') + ')');
    if (d.services.length) sends.push('services ' + d.services.map((u) => (u.length > 8 ? u.slice(0, 8) + '…' : u)).join(' '));
    set(row.cells[1], by.length ? by.join(' · ') : 'nothing reads it yet' + (sends.length ? ': ' + sends.join(' · ') : ''));
    row.cells[1].className = by.length ? '' : 'muted';
    set(row.cells[2], String(d.per_min));
    set(row.cells[3], d.longest_gap_s + ' s');
    set(row.cells[4], quiet ? `silent ${d.silent_s} s` : d.rssi == null ? '–' : String(d.rssi));
    row.style.opacity = quiet ? '.45' : '';
    const [use, chosen] = row.cells[5].children;
    const isCur = d.device_id === cur;
    set(use, isCur ? 'Open' : 'Use');
    use.disabled = quiet && !isCur;
    use.title = quiet ? 'Not heard for a while: wake the device' : '';
    chosen.hidden = !isCur;
  }
  for (const [id, row] of deviceRows) {
    if (!seen.has(id)) { row.remove(); deviceRows.delete(id); }
  }
  others.hidden = !showOthers;
  maybe.hidden = !maybeCount;
  const button = $('#others-button');
  const things = otherCount === 1 ? 'the 1 other thing nearby' : `the ${otherCount} other things nearby`;
  // The question is what the eye should catch: bold and underlined
  const label = !otherCount ? '' : showOthers ? `Hide ${things}` : `Don't see your device? Show ${things}`;
  if (button.textContent !== label) {
    if (showOthers || !otherCount) button.replaceChildren(label);
    else button.replaceChildren(h('b', { style: 'text-decoration:underline' }, "Don't see your device?"), ` Show ${things}`);
  }
  $('#devices-toggle').hidden = !otherCount;
  let empty = $('#devices-empty');
  if (!readableCount && !maybeCount) {
    if (!empty) { empty = h('tr', { id: 'devices-empty' }, h('td', { colspan: 6, class: 'empty' })); readable.append(empty); }
    const text = S.radio.startsWith('error') ? S.radio.slice(7) : S.radio === 'off' ? 'This build has no Bluetooth: open a recording in the Recordings tab.' : 'Listening… wake your device (pedal, press a button, put the strap on).';
    if (empty.firstChild.textContent !== text) empty.firstChild.textContent = text;
  } else if (empty) empty.remove();
}

// ───────────────────────── byte grid ─────────────────────────

function gridEl(selectable) {
  return h('div', { class: 'grid', 'data-grid': selectable ? 'select' : 'view' });
}

function fillGrid(el) {
  const packets = streamPackets();
  if (!packets.length) { el.replaceChildren(h('div', { class: 'muted' }, 'No packets yet.')); return; }
  const last = bytesOf(packets[packets.length - 1][2]);
  const prev = packets.length > 1 ? bytesOf(packets[packets.length - 2][2]) : last;
  const recent = packets.slice(-150).map((p) => bytesOf(p[2]));
  // how many different values each byte took over the whole capture (cheap enough at this size)
  const distinct = last.map(() => new Set());
  const step = Math.max(1, Math.floor(packets.length / 1500));
  for (let i = 0; i < packets.length; i += step) { const b = bytesOf(packets[i][2]); for (let j = 0; j < distinct.length && j < b.length; j++) distinct[j].add(b[j]); }
  const selectable = el.dataset.grid === 'select';
  if (el.children.length !== last.length || !el.firstChild.classList || !el.firstChild.classList.contains('byte')) {
    el.replaceChildren(...last.map((_, i) => {
      const cell = h('div', { class: 'byte', title: 'position ' + i }, h('div', { class: 'i' }, i), h('div', { class: 'h' }), h('div', { class: 'd' }), h('canvas', { width: 80, height: 32 }));
      if (selectable) cell.addEventListener('click', () => selectByte(i));
      return cell;
    }));
  }
  [...el.children].forEach((cell, i) => {
    cell.children[1].textContent = last[i].toString(16).padStart(2, '0');
    cell.children[2].textContent = last[i];
    cell.style.setProperty('--heat', Math.min(0.28, Math.log2(distinct[i].size) * 0.04).toFixed(3));
    cell.classList.toggle('flash', last[i] !== prev[i]);
    cell.classList.toggle('sel', selectable && S.sel.includes(i));
    spark(cell.children[3], recent.map((b) => b[i]));
  });
}

function spark(canvas, values) {
  const c = canvas.getContext('2d');
  c.clearRect(0, 0, canvas.width, canvas.height);
  const vals = values.filter((v) => v != null);
  if (vals.length < 2) return;
  const min = Math.min(...vals), max = Math.max(...vals);
  if (min === max) return;
  c.strokeStyle = getComputedStyle(document.body).getPropertyValue('--accent');
  c.lineWidth = 2;
  c.beginPath();
  vals.forEach((v, i) => {
    const x = (i / (vals.length - 1)) * canvas.width, y = canvas.height - 2 - ((v - min) / (max - min)) * (canvas.height - 4);
    i ? c.lineTo(x, y) : c.moveTo(x, y);
  });
  c.stroke();
}

function streamPicker() {
  const s = session();
  if (!s || s.streams.length < 2) return null;
  return h('div', { class: 'row' }, h('span', { class: 'muted' }, 'Stream'),
    h('select', { onchange: (e) => { S.stream = e.target.value; S.sel = []; S.series = null; render(); } },
      s.streams.map((x) => h('option', { value: x.key, selected: x.key === S.stream }, (x.key === 'adv' ? 'advertisements' : 'characteristic ' + x.key) + ` (${x.packets})`))));
}

// ───────────────────────── 2 Record ─────────────────────────

function fillStats(el) {
  const packets = streamPackets();
  const span = packets.length > 1 ? (packets[packets.length - 1][0] - packets[0][0]) / 60000 : 0;
  let gap = 0;
  for (let i = 1; i < packets.length; i++) gap = Math.max(gap, packets[i][0] - packets[i - 1][0]);
  const stat = (v, label) => h('div', { class: 'stat' }, h('b', {}, v), h('span', {}, label));
  el.replaceChildren(
    stat(packets.length, 'packets'),
    stat(span > 0 ? Math.round((packets.length - 1) / span) : '–', 'per minute'),
    stat((gap / 1000).toFixed(1) + ' s', 'longest gap'),
    stat(packets.length ? secs(packets[packets.length - 1][0]) : '0:00', 'recorded'),
    stat(S.capture ? S.capture.notes.length : 0, 'notes'),
  );
}

function viewRecord() {
  const s = session();
  if (!s) return needSession('Choose the device to record first.');
  const wrap = h('div', {});
  const toggle = s.recording
    ? h('button', { class: 'danger', onclick: guard(async () => { await api('/api/record/stop', {}); await reload(); }) }, 'Stop recording')
    : h('button', { class: 'primary', disabled: s.from_file, onclick: guard(async () => { await api('/api/record/start', {}); await reload(); }) }, 'Record again (a new file)');
  wrap.append(h('div', { class: 'panel' },
    h('h2', {}, 'Record ' + s.name),
    h('p', { class: 'hint' }, s.from_file ? 'This is a capture file: it can be read and mapped, not recorded into. Choose the device in Devices to record more.'
      : 'Use the device as you normally would while this records: change effort, speed, gear, whatever it measures.'),
    h('div', { class: 'row' }, toggle),
    h('div', { class: 'stats', id: 'rec-stats' }),
    s.file ? h('div', { class: 'muted mono' }, s.file) : null,
  ));
  wrap.append(h('div', { class: 'panel' },
    h('h2', {}, 'What it sends'),
    h('p', { class: 'hint' }, 'The latest packet, one box per byte. A byte that just changed is red; a blue tint means it changes often; the small line is its recent history.'),
    streamPicker(), gridEl(false)));

  const noteInput = h('input', { class: 'fill', placeholder: 'power=150 cadence=82', onkeydown: (e) => { if (e.key === 'Enter') saveNote(); } });
  const saveNote = guard(async () => { if (!noteInput.value.trim()) return; await api('/api/note', { text: noteInput.value }); noteInput.value = ''; toast('Noted'); });
  const notes = h('ul', { class: 'list' });
  for (const n of (S.capture ? S.capture.notes : []).slice().reverse()) notes.append(h('li', {}, h('span', { class: 'muted mono' }, secs(n.t)), h('span', {}, n.text)));
  wrap.append(h('div', { class: 'panel' },
    h('h2', {}, 'What the device’s own display shows'),
    h('p', { class: 'hint' }, 'Only needed for a device no table reads yet. Hold a steady effort until the numbers on the device settle, then type what you read and press Enter. “Find it for me” in the next step works from these. Two or three at clearly different efforts are enough. (For a device that is already read, use Live: it only asks right or wrong.)'),
    h('div', { class: 'row' }, noteInput, h('button', { onclick: saveNote, disabled: !s.recording }, 'Save note')),
    h('div', { class: 'row', style: 'margin-top:8px' }, h('span', { class: 'muted' }, 'Fields:'),
      fields().map((f) => h('button', { class: 'small ghost', title: f.description || '', onclick: () => { noteInput.value = (noteInput.value + ' ' + f.key + '=').trimStart(); noteInput.focus(); } }, f.key))),
    notes,
    h('div', { class: 'row', style: 'margin-top:12px' }, h('button', { onclick: () => setTab('map') }, 'Next: find the bytes →')),
  ));
  return wrap;
}

// ───────────────────────── 3 Map ─────────────────────────

function selectByte(i) {
  if (S.sel.includes(i)) S.sel = [];
  else if (S.sel.length === 1 && Math.abs(S.sel[0] - i) === 1) S.sel = [Math.min(S.sel[0], i), Math.max(S.sel[0], i)];
  else S.sel = [i];
  if (S.sel.length === 1) setExpr(`u8[${S.sel[0]}]`);
  else if (S.sel.length === 2) setExpr(`u16le[${S.sel[0]}]`);
  else render();
}

const setExpr = guard(async (expr) => {
  S.expr = expr;
  S.series = null;
  render();
  if (!expr.trim()) return;
  try {
    S.series = await api('/api/series', { stream: S.stream, expr });
  } catch (e) {
    S.series = { error: e.message };
  }
  render();
});

function design() {
  const d = (session() && session().design) || {};
  return { id: d.id || '', name: d.name || '', mappings: d.mappings || [], only_when: d.only_when || '', instance_label: d.instance_label || '', instance_expr: d.instance_expr || '', stale_after_ms: d.stale_after_ms || 3000, provenance: d.provenance || '', verified_on: d.verified_on || '' };
}

const saveDesign = guard(async (d) => {
  await api('/api/table/design', { ...d, stream: S.stream });
  S.checked = null;
});

function addMapping(field, expr) {
  if (!field) return toast('Choose which field this is.', true);
  if (!expr.trim()) return toast('Select a byte or type an expression first.', true);
  const d = design();
  d.mappings = d.mappings.filter((m) => m.field !== field).concat([{ field, expr }]);
  saveDesign(d);
  toast(`${field} ← ${expr}`);
}

function viewMap() {
  const s = session();
  if (!s) return needSession('Choose a device or open a capture first.');
  const d = design();
  const left = h('div', {});
  const right = h('div', {});

  // readers + scale for the selection
  const readers = [];
  if (S.sel.length === 1) readers.push(`u8[${S.sel[0]}]`, `i8[${S.sel[0]}]`);
  if (S.sel.length === 2) readers.push(`u16le[${S.sel[0]}]`, `u16be[${S.sel[0]}]`, `i16le[${S.sel[0]}]`, `u8[${S.sel[0]}] * 60 + u8[${S.sel[1]}]`);
  const base = S.expr.replace(/ [/*] \d+$/, '');
  const exprInput = h('input', { class: 'fill mono', value: S.expr, placeholder: 'u16le[4] / 10', onchange: (e) => setExpr(e.target.value) });
  const f = fieldOf(S.field);
  const fieldSelect = h('select', { onchange: (e) => { if (e.target.value === '+') { e.target.value = S.field; newFieldDialog(); } else { S.field = e.target.value; render(); } } },
    h('option', { value: '' }, 'Which field is this?'),
    fields().map((x) => h('option', { value: x.key, selected: x.key === S.field }, `${x.name}${x.unit ? ' (' + x.unit + ')' : ''}`)),
    h('option', { value: '+' }, '+ A new field…'));

  const chart = h('canvas', { class: 'chart', width: 1200, height: 380 });
  left.append(h('div', { class: 'panel' },
    h('h2', {}, 'Pick the bytes'),
    h('p', { class: 'hint' }, 'Click a byte, or two neighbours for a two-byte number. The chart shows that reading over the whole capture; the dots are your notes for the chosen field.'),
    streamPicker(), gridEl(true),
    h('div', { class: 'row', style: 'margin-top:10px' },
      readers.map((r) => h('button', { class: 'small' + (base === r ? ' primary' : ''), onclick: () => setExpr(r) }, r.replace(/\[.*/, '').replace('u8', readers.length > 2 && r.includes('*') ? 'min:sec' : 'u8'))),
      S.sel.length ? [['× 1', ''], ['÷ 10', ' / 10'], ['÷ 100', ' / 100'], ['× 10', ' * 10']].map(([label, suffix]) =>
        h('button', { class: 'small ghost', onclick: () => setExpr((base.includes(' ') && suffix ? `(${base})` : base) + suffix) }, label)) : null),
    h('label', {}, 'Reading (the table language; edit freely)'),
    h('div', { class: 'row' }, exprInput, fieldSelect, h('button', { class: 'primary', onclick: () => addMapping(S.field, exprInput.value) }, 'Add to table')),
    f ? h('p', { class: 'muted', style: 'margin:6px 0 0' }, `${f.type === 'int' ? 'whole number' : f.decimals + ' decimal' + (f.decimals > 1 ? 's' : '')} · plausible ${f.min}–${f.max} ${f.unit} · ${f.kind}. ${f.description || ''}`) : null,
    S.series && S.series.error ? h('p', { class: 'problem' }, S.series.error) : null,
    S.series && !S.series.error ? h('p', { class: 'muted', style: 'margin:8px 0 4px' }, `${S.series.count} values, from ${S.series.min} to ${S.series.max}`) : null,
    chart,
  ));
  setTimeout(() => drawChart(chart, S.series && S.series.points || [], notesFor(S.field)));

  // find it for me
  const found = h('div', {});
  if (S.solve) {
    for (const [key, r] of Object.entries(S.solve.fields)) {
      const known = !!fieldOf(key);
      found.append(h('h3', {}, `${key} · ${r.notes} note${r.notes > 1 ? 's' : ''}` + (known ? '' : ' · not a registry field')));
      if (!r.candidates.length) found.append(h('p', { class: 'muted' }, 'No reading of the packets gives these values.'));
      const ul = h('ul', { class: 'list' });
      for (const c of r.candidates) {
        ul.append(h('li', {}, h('span', { class: 'mono fill' }, c.expr),
          h('span', { class: 'badge ' + (c.fits === c.notes ? 'good' : '') }, `${c.fits} of ${c.notes}`),
          h('span', { class: 'muted' }, '±' + c.mean_error.toFixed(1)),
          h('button', { class: 'small', onclick: () => { S.field = known ? key : S.field; S.sel = []; setExpr(c.expr); } }, 'Show'),
          known ? h('button', { class: 'small primary', onclick: () => addMapping(key, c.expr) }, 'Use') : null));
      }
      found.append(ul);
    }
  }
  right.append(h('div', { class: 'panel' },
    h('h2', {}, 'Find it for me'),
    h('p', { class: 'hint' }, 'Tries every position, width and scale against the values you noted, best fit first.'),
    h('button', { class: 'primary', onclick: guard(async () => { S.solve = await api('/api/solve', { stream: S.stream }); render(); }) }, 'Find the bytes from my notes'),
    found));

  // the table so far
  const mapped = h('ul', { class: 'list' });
  for (const m of d.mappings) {
    mapped.append(h('li', {}, h('b', {}, m.field), h('span', { class: 'mono fill' }, '← ' + m.expr),
      h('button', { class: 'small ghost', onclick: () => { S.field = m.field; S.sel = []; setExpr(m.expr); } }, 'Show'),
      h('button', { class: 'small ghost', onclick: () => { const n = design(); n.mappings = n.mappings.filter((x) => x.field !== m.field); saveDesign(n); } }, 'Remove')));
  }
  const text = (key, label, placeholder) => [h('label', {}, label), h('input', { style: 'width:100%', class: key.includes('expr') || key === 'only_when' ? 'mono' : '', value: d[key], placeholder, onchange: (e) => { const n = design(); n[key] = key === 'stale_after_ms' ? Number(e.target.value) || 3000 : e.target.value; saveDesign(n); } })];
  right.append(h('div', { class: 'panel' },
    h('h2', {}, 'The table so far'),
    s.draft_edited ? h('p', { class: 'problem' }, 'The table text was edited by hand in Save: changing anything here rewrites it.') : null,
    d.mappings.length ? mapped : h('p', { class: 'muted' }, 'Nothing mapped yet.'),
    text('name', 'Device name', s.name),
    text('id', 'Table id (file name)', 'made from the name'),
    text('only_when', 'Only read packets when… (optional)', 'u8[2] == 0'),
    h('div', { class: 'row' }, h('div', { class: 'fill' }, text('instance_expr', 'What tells two of these apart (optional)', 'u8[3]')), h('div', { style: 'width:130px' }, text('instance_label', 'Called', 'bike'))),
    text('stale_after_ms', 'Silent after (ms)', '3000'),
    text('provenance', 'Where this knowledge comes from', 'My own capture, compared with the device’s display.'),
    text('verified_on', 'Device and firmware checked on', s.name),
    h('div', { class: 'row', style: 'margin-top:12px' }, h('button', { onclick: () => setTab('live') }, 'Next: watch it live →'), h('button', { class: 'ghost', onclick: () => setTab('save') }, 'Check and save →')),
  ));
  return h('div', { class: 'cols' }, left, right);
}

function notesFor(key) {
  if (!key || !S.capture) return [];
  return S.capture.notes.filter((n) => n.values && key in n.values).map((n) => [n.t, n.values[key]]);
}

function drawChart(canvas, points, markers = []) {
  const c = canvas.getContext('2d');
  const css = getComputedStyle(document.body);
  const W = canvas.width, H = canvas.height, pad = 46;
  c.clearRect(0, 0, W, H);
  c.font = '22px system-ui';
  c.fillStyle = css.getPropertyValue('--muted');
  if (!points.length) { c.fillText('Select a byte to see its values here.', pad, H / 2); return; }
  const all = points.concat(markers);
  let t0 = Math.min(...all.map((p) => p[0])), t1 = Math.max(...all.map((p) => p[0]));
  let v0 = Math.min(...all.map((p) => p[1])), v1 = Math.max(...all.map((p) => p[1]));
  if (t0 === t1) t1 = t0 + 1;
  if (v0 === v1) { v1 = v0 + 1; v0 -= 1; }
  const x = (t) => pad + ((t - t0) / (t1 - t0)) * (W - pad - 12);
  const y = (v) => H - 30 - ((v - v0) / (v1 - v0)) * (H - 52);
  c.fillText(String(+v1.toFixed(2)), 4, 26);
  c.fillText(String(+v0.toFixed(2)), 4, H - 30);
  c.fillText(secs(t0), pad, H - 4);
  c.fillText(secs(t1), W - 70, H - 4);
  c.strokeStyle = css.getPropertyValue('--accent');
  c.lineWidth = 2.5;
  c.beginPath();
  points.forEach((p, i) => (i ? c.lineTo(x(p[0]), y(p[1])) : c.moveTo(x(p[0]), y(p[1]))));
  c.stroke();
  c.fillStyle = css.getPropertyValue('--bad');
  for (const m of markers) { c.beginPath(); c.arc(x(m[0]), y(m[1]), 9, 0, Math.PI * 2); c.fill(); }
}

function newFieldDialog(similarShown) {
  const dlg = $('#dialog');
  const v = (id) => $('#nf-' + id, dlg).value;
  const submit = guard(async (confirmNew) => {
    const r = await api('/api/field', { key: v('key').trim(), name: v('name'), unit: v('unit'), type: v('type'), decimals: Number(v('decimals')) || 1, min: Number(v('min')), max: Number(v('max')), kind: v('kind'), description: v('description'), confirm_new: confirmNew });
    if (r.similar) {
      $('#nf-similar', dlg).replaceChildren(h('p', { class: 'problem' }, 'Fields like this already exist. Use one of them if it means the same thing:'),
        h('ul', { class: 'list' }, r.similar.map((x) => h('li', {}, h('b', {}, x.key), h('span', { class: 'fill muted' }, `${x.name} ${x.unit ? '(' + x.unit + ')' : ''} — ${x.description || ''}`),
          h('button', { class: 'small', onclick: () => { S.field = x.key; dlg.close(); render(); } }, 'Use this')))),
        h('button', { onclick: () => submit(true) }, 'Mine is different: add it'));
      return;
    }
    dlg.close();
    await loadState();
    S.field = r.key;
    toast(`Field ${r.key} added to registry/fields (experimental)`);
    render();
  });
  dlg.replaceChildren(
    h('h2', { style: 'margin-top:0' }, 'A new field'),
    h('p', { class: 'hint' }, 'Only for a measurement the registry has no field for. It starts as experimental.'),
    h('div', { class: 'row' }, h('div', { class: 'fill' }, h('label', {}, 'Key (lower case, _)'), h('input', { id: 'nf-key', style: 'width:100%', placeholder: 'body_temperature' })),
      h('div', { class: 'fill' }, h('label', {}, 'Name'), h('input', { id: 'nf-name', style: 'width:100%', placeholder: 'Body temperature' })),
      h('div', { style: 'width:90px' }, h('label', {}, 'Unit'), h('input', { id: 'nf-unit', style: 'width:100%', placeholder: '°C' }))),
    h('div', { class: 'row' },
      h('div', {}, h('label', {}, 'Number'), h('select', { id: 'nf-type' }, h('option', { value: 'int' }, 'whole number'), h('option', { value: 'decimal' }, 'decimal'))),
      h('div', { style: 'width:90px' }, h('label', {}, 'Decimals'), h('input', { id: 'nf-decimals', type: 'number', min: 1, max: 4, value: 1, style: 'width:100%' })),
      h('div', { style: 'width:110px' }, h('label', {}, 'Lowest plausible'), h('input', { id: 'nf-min', type: 'number', value: 0, style: 'width:100%' })),
      h('div', { style: 'width:110px' }, h('label', {}, 'Highest plausible'), h('input', { id: 'nf-max', type: 'number', value: 100, style: 'width:100%' })),
      h('div', {}, h('label', {}, 'Kind'), h('select', { id: 'nf-kind' },
        h('option', { value: 'instantaneous' }, 'the value right now'), h('option', { value: 'cumulative' }, 'a running total'), h('option', { value: 'event' }, 'one thing that happened'), h('option', { value: 'state' }, 'a setting or state')))),
    h('label', {}, 'What it means, in a sentence'), h('input', { id: 'nf-description', style: 'width:100%', placeholder: 'Core body temperature right now, in degrees Celsius.' }),
    h('div', { id: 'nf-similar', style: 'margin-top:10px' }),
    h('div', { class: 'row', style: 'margin-top:14px; justify-content:flex-end' }, h('button', { class: 'ghost', onclick: () => dlg.close() }, 'Cancel'), h('button', { class: 'primary', onclick: () => submit(false) }, 'Add field')),
  );
  dlg.showModal();
}

// ───────────────────────── 4 Live ─────────────────────────

/** Every value is shown as a number (a dial and a text form were offered once; nobody needs the choice). */
function displayMode() {
  return 'number';
}
function setDisplayMode(key, mode) { try { localStorage.setItem('ofp-display-' + key, mode); } catch { /* private window */ } renderTiles(); }

const dialMax = {};
function dial(value, f) {
  const seen = Math.max(dialMax[f.key] || 0, value || 0);
  dialMax[f.key] = seen;
  // a round ceiling above what was seen, never beyond the field's plausible range
  const steps = [10, 20, 50, 100, 150, 200, 300, 500, 1000, 2000, 5000, 10000, 100000];
  const max = Math.min(f.max, steps.find((s) => s >= seen * 1.15) || f.max);
  const frac = Math.max(0, Math.min(1, ((value || 0) - f.min) / (max - f.min || 1)));
  const a = Math.PI * (1 - frac), r = 80, cx = 100, cy = 100;
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('viewBox', '0 0 200 118');
  svg.innerHTML = `<path d="M20 100 A80 80 0 0 1 180 100" fill="none" stroke="var(--line)" stroke-width="12" stroke-linecap="round"/>
    <path d="M20 100 A80 80 0 0 1 ${(cx + r * Math.cos(a)).toFixed(1)} ${(cy - r * Math.sin(a)).toFixed(1)}" fill="none" stroke="var(--accent)" stroke-width="12" stroke-linecap="round"/>
    <text x="100" y="92" text-anchor="middle" font-size="34" font-weight="700" fill="currentColor">${fmt(value, f)}</text>
    <text x="20" y="116" text-anchor="middle" font-size="10" fill="var(--muted)">${f.min}</text>
    <text x="180" y="116" text-anchor="middle" font-size="10" fill="var(--muted)">${max}</text>`;
  return svg;
}

// What the standard Bluetooth profiles can carry, described like registry fields so one tile draws both.
const STANDARD = {
  FTMS: { title: 'FTMS (the standard for trainers and bikes)', keys: ['power', 'cadence', 'speed', 'heartRate', 'resistance'] },
  CPS: { title: 'Cycling Power (standard)', keys: ['power', 'cadence', 'speed'] },
  CSC: { title: 'Speed and Cadence (standard)', keys: ['cadence', 'speed'] },
  HR: { title: 'Heart Rate (standard)', keys: ['heartRate'] },
};
const STANDARD_FIELDS = {
  power: { name: 'Power', unit: 'W', decimals: 0, min: 0, max: 2000 },
  cadence: { name: 'Cadence', unit: 'rpm', decimals: 0, min: 0, max: 200 },
  speed: { name: 'Speed', unit: 'km/h', decimals: 1, min: 0, max: 100 },
  heartRate: { name: 'Heart rate', unit: 'bpm', decimals: 0, min: 0, max: 220 },
  resistance: { name: 'Resistance', unit: '', decimals: 0, min: 0, max: 200 },
};

/** Everything being decoded right now, grouped by what decodes it: [{ id, title, hint, tiles: [{ id, proto, key, f, value }] }] */
function liveGroups() {
  const groups = [];
  const src = S.live.source;
  if (src) {
    const table = ((S.state && S.state.registry.tables) || []).find((t) => t.id === src.table);
    const isDraft = table && table.draft && !(session() && session().draft);
    groups.push({
      id: 'ofp', title: src.name,
      hint: `OFP table “${src.table}” (version ${src.table_version})` + (isDraft ? ' · DRAFT: written from documentation, never tried on this device. Your answers below are what verifies it.' : ''),
      tiles: src.fields.map((f) => ({ id: 'ofp:' + f.key, proto: 'ofp', key: f.key, f, value: S.live.values[f.key], stale: S.live.stale })),
    });
  }
  const T = Tester.state();
  const now = testerNow();
  // the standard table the core is already showing above is not shown twice
  const shownByTable = { 'std-ftms-bike': 'FTMS', 'std-cycling-power': 'CPS', 'std-speed-cadence': 'CSC', 'std-heart-rate': 'HR' }[src && src.table];
  for (const [proto, def] of Object.entries(STANDARD)) {
    if (proto === shownByTable) continue;
    const v = T.src[proto];
    if (v.at === undefined) continue;
    const tiles = def.keys.filter((k) => v[k] !== undefined).map((k) => ({ id: proto + ':' + k, proto, key: k, f: { key: proto + '-' + k, ...STANDARD_FIELDS[k] }, value: v[k], stale: now - v.at > 4000 }));
    if (tiles.length) groups.push({ id: proto, title: def.title, hint: 'read by the standard, no table needed', tiles });
  }
  return groups;
}

/** The rider's latest word on a value: { ok, text } or null. */
function verdictOf(tile) {
  if (!S.capture) return null;
  if (tile.proto === 'ofp') {
    const c = S.capture.confirms.filter((x) => x.field === tile.key).pop();
    if (!c) return null;
    return c.ok ? { ok: true, text: 'you said this is right' } : { ok: false, text: 'you said this is wrong' + (c.console != null ? ` (display showed ${c.console})` : '') };
  }
  const tag = `${tile.proto} ${tile.key}`;
  const n = S.capture.notes.filter((x) => x.text.startsWith('RIGHT: ' + tag) || x.text.startsWith('PROBLEM: ' + tag)).pop();
  if (!n) return null;
  return n.text.startsWith('RIGHT') ? { ok: true, text: 'you said this is right' } : { ok: false, text: 'you said this is wrong' };
}

const sayRight = guard(async (tile) => {
  if (tile.proto === 'ofp') await api('/api/confirm', { field: tile.key });
  else await api('/api/note', { text: `RIGHT: ${tile.proto} ${tile.key} (showed ${fmt(tile.value, tile.f)})` });
  toast(`${tile.f.name}: right`);
});

/** "No, this is wrong": one optional line in the rider's own words. A plain number is taken as what the display showed. */
function sayWrong(tile) {
  const dlg = $('#dialog');
  const shown = fmt(tile.value, tile.f);
  const what = h('input', { style: 'width:100%', placeholder: 'e.g. the console shows about 70 · it jumps around · it is double' });
  const save = guard(async () => {
    const text = what.value.trim();
    const number = text !== '' && !isNaN(Number(text)) ? Number(text) : undefined;
    if (tile.proto === 'ofp') {
      await api('/api/confirm', { field: tile.key, wrong: true, console: number });
      if (text && number === undefined) await api('/api/note', { text: `PROBLEM: ${tile.f.name}: ${text} (showed ${shown})` });
    } else {
      await api('/api/note', { text: `PROBLEM: ${tile.proto} ${tile.key}: ${text || 'wrong'} (showed ${shown})` });
    }
    dlg.close();
    toast('Saved. Tell your assistant: it can see what the device sent at that moment.');
  });
  what.addEventListener('keydown', (e) => { if (e.key === 'Enter') save(); });
  dlg.replaceChildren(
    h('h2', { style: 'margin-top:0' }, `${tile.f.name} is wrong`),
    h('p', { class: 'hint' }, `It showed ${shown} ${tile.f.unit || ''} when you pressed. If you can, say what you saw on the device, in your own words. You can leave this empty.`),
    what,
    h('div', { class: 'row', style: 'margin-top:14px; justify-content:flex-end' }, h('button', { class: 'ghost', onclick: () => dlg.close() }, 'Cancel'), h('button', { class: 'danger', onclick: save }, 'Save “wrong”')));
  dlg.showModal();
  what.focus();
}

function renderTiles() {
  const box = $('#tiles');
  if (!box) return;
  const groups = liveGroups();
  const s = session();
  // The state of things in large type: listening, receiving, or gone quiet.
  const status = $('#live-status');
  if (status) {
    const quietFor = S.lastPacketAt ? Math.round((Date.now() - S.lastPacketAt) / 1000) : null;
    let cls = 'listening', big, small = '';
    if (!s || !s.recording) { cls = 'stopped'; big = 'Stopped'; small = S.advanced ? 'Nothing is being read. Press “Start again” to listen.' : ''; }
    else if (quietFor === null && !S.advanced) { cls = 'listening waiting'; big = 'Connecting to your device…'; small = 'This can take up to 20 seconds. Keep it awake: pedal, press a button, or put the strap on.'; }
    else if (quietFor === null) { big = 'Listening… nothing received yet'; small = 'Wake the device: pedal, press a button, put the strap on.'; }
    else if (quietFor > 8) { cls = 'quiet'; big = `Nothing received for ${quietFor} s`; small = 'The device has gone quiet, or is out of range.'; }
    else if (!groups.length && !S.advanced) { big = 'We can hear your device, but OFP can\'t read its numbers yet'; small = 'That is fine: keep it going for a minute, then send it to us below. That is how a device gets added.'; }
    else if (!groups.length) { big = 'Receiving data, but nothing reads it yet'; small = 'No table and no standard profile knows these packets. See them in Record, and map them in Map.'; }
    else { cls = 'receiving'; const n = groups.reduce((a, g) => a + g.tiles.filter((t) => t.value !== undefined).length, 0); big = `Receiving · ${n} value${n === 1 ? '' : 's'} reported`; }
    const text = big + '|' + small;
    if (status.dataset.text !== text) {
      status.dataset.text = text;
      status.className = 'live-status ' + cls;
      status.replaceChildren(...[h('div', { class: 'big' }, big), small ? h('div', { class: 'muted' }, small) : null].filter(Boolean));
    }
  }
  if (!groups.length) {
    box.dataset.sig = '';
    box.replaceChildren();
    return;
  }
  // rebuild only when the set of values changes, so nothing jumps while you look or click
  const sig = groups.map((g) => g.id + '=' + g.tiles.map((t) => t.id + displayMode(t.f.key)).join(',')).join('|');
  if (box.dataset.sig !== sig) {
    box.dataset.sig = sig;
    box.replaceChildren(...groups.map((g) => h('div', { class: 'group' },
      h('div', { class: 'group-head' }, h('b', {}, g.title), h('span', { class: 'muted' }, ' · ' + g.hint)),
      h('div', { class: 'tiles' }, g.tiles.map((t) => h('div', { class: 'tile', 'data-id': t.id },
        h('div', { class: 'name' }, h('span', {}, t.f.name, t.f.unit ? h('span', { class: 'unit' }, ' ' + t.f.unit) : null)),
        h('div', { class: 'show' }),
        h('div', { class: 'desc muted' }, (fieldOf(t.key) && fieldOf(t.key).description) || ''),
        h('div', { class: 'verdict' }),
        h('div', { class: 'row answer' },
          h('button', { class: 'small yes', onclick: () => sayRight(currentTile(t.id)) }, S.advanced ? 'Yes, this is right' : 'Looks right'),
          S.advanced
            ? h('button', { class: 'small no', onclick: () => sayWrong(currentTile(t.id)) }, 'No, this is wrong')
            : h('button', { class: 'linkish wrong', onclick: () => sayWrong(currentTile(t.id)) }, 'Looks wrong'))))))));
  }
  for (const g of groups) for (const t of g.tiles) {
    const tile = box.querySelector(`.tile[data-id="${t.id}"]`);
    if (!tile) continue;
    const mode = displayMode(t.f.key);
    tile.classList.toggle('silent', !!t.stale);
    const show = tile.querySelector('.show');
    if (mode === 'dial') show.replaceChildren(dial(t.value, t.f));
    else {
      const none = t.value === undefined;
      const text = none ? 'nothing reported yet' : fmt(t.value, t.f) + (mode === 'text' && t.f.unit ? ' ' + t.f.unit : '');
      const cls = 'value' + (none ? ' none' : mode === 'text' ? ' text' : '');
      if (!show.firstChild || show.firstChild.className !== cls) show.replaceChildren(h('div', { class: cls }, text));
      else if (show.firstChild.textContent !== text) show.firstChild.textContent = text;
    }
    const v = verdictOf(t);
    const el = tile.querySelector('.verdict');
    const text = v ? (v.ok ? '✓ ' : '✗ ') + v.text : 'not checked yet';
    if (el.textContent !== text) { el.textContent = text; el.className = 'verdict ' + (v ? (v.ok ? 'good' : 'bad') : 'muted'); }
  }
}

/** The tile as it is at the moment of the click (the value keeps moving). */
function currentTile(id) {
  for (const g of liveGroups()) for (const t of g.tiles) if (t.id === id) return t;
  return null;
}

function viewLive() {
  const s = session();
  if (!s) return needSession('Choose a device first.');
  const wrap = h('div', {});
  const head = h('div', { class: 'panel' },
    h('h2', {}, 'Live: ' + s.name),
    h('p', { class: 'hint' }, 'What the device is sending right now, grouped by what reads it. Use the device normally and look at its own display: for each value, is this right? You do not need to match an exact number, they move. ',
      'If something is wrong, say so here and then tell your assistant what you saw.'));
  if (s.from_file) head.append(h('p', { class: 'problem' }, 'This is a capture file. Live values need the device: choose it in Devices.'));
  else if (!s.recording) {
    // Stopped: the recording is already saved (it is written as it happens). Say where, and offer it.
    const file = s.file ? s.file.split('/').pop() : null;
    head.append(h('div', { class: 'row' },
      file ? h('span', { class: 'good' }, '✓ Saved as ' + file) : h('span', { class: 'muted' }, 'Stopped.'),
      file ? h('a', { class: 'btn', href: '/api/capture/file', download: '' }, 'Download it to send') : null,
      h('button', { class: 'primary', onclick: guard(async () => { await api('/api/record/start', {}); await reload(); }) }, 'Start again (a new recording)')));
  } else {
    head.append(h('div', { class: 'row' }, h('span', { class: 'dot rec' }), h('span', {}, 'Recording what the device sends'),
      h('button', { class: 'danger', onclick: guard(async () => { await api('/api/record/stop', {}); await reload(); toast('Stopped and saved.'); }) }, 'Stop and save')));
  }

  // Anything else that looks wrong, in the rider's own words: saved with the time, next to what the device sent.
  const problem = h('input', { class: 'fill', placeholder: 'everything freezes for a few seconds now and then · …', onkeydown: (e) => { if (e.key === 'Enter') tell(); } });
  const tell = guard(async () => { if (!problem.value.trim()) return; await api('/api/note', { text: 'PROBLEM: ' + problem.value }); problem.value = ''; toast('Saved with the time.'); });
  const told = (S.capture ? S.capture.notes : []).filter((n) => n.text.startsWith('PROBLEM:')).slice(-5).reverse();
  wrap.append(head, h('div', { id: 'live-status', class: 'live-status' }), h('div', { id: 'tiles' }),
    h('div', { class: 'panel', style: 'margin-top:16px' },
      h('h2', {}, 'Anything else?'),
      h('p', { class: 'hint' }, 'Something that is not about one value. Saved in the recording with the time.'),
      h('div', { class: 'row' }, problem, h('button', { onclick: tell, disabled: !s.recording }, 'Save')),
      h('ul', { class: 'list' }, told.map((n) => h('li', {}, h('span', { class: 'muted mono' }, secs(n.t)), h('span', {}, n.text.slice(9)))))));
  setTimeout(renderTiles);
  return wrap;
}

// ───────────────────────── 5 Save ─────────────────────────

const runCheck = guard(async () => {
  const r = await api('/api/table/check', { yaml: S.yaml });
  S.checked = { ...r, yaml: S.yaml };
  S.yamlDirty = false;
  await loadState();
  render();
});

function viewSave() {
  const s = session();
  if (!s) return needSession('Choose a device or open a capture first.');
  const area = h('textarea', { spellcheck: 'false', oninput: (e) => { S.yaml = e.target.value; S.yamlDirty = true; } });
  area.value = S.yaml;
  const left = h('div', { class: 'panel' },
    h('h2', {}, 'The table'),
    h('p', { class: 'hint' }, 'Written from the Map step; you can edit it here. This is the file that goes into the registry.'),
    area,
    h('div', { class: 'row', style: 'margin-top:10px' },
      h('button', { class: 'primary', onclick: runCheck }, 'Check'),
      h('button', { class: 'ghost', onclick: () => saveDesign(design()) }, 'Rewrite from the Map step')));

  const right = h('div', {});
  const c = S.checked;
  const result = h('div', { class: 'panel' }, h('h2', {}, 'Check'), h('p', { class: 'hint' }, 'The same judgement the automated checks make: this capture is replayed through the table and compared with your notes and confirmations.'));
  if (!c) result.append(h('p', { class: 'muted' }, 'Not checked yet.'));
  else {
    result.append(h('p', {}, c.ok ? h('span', { class: 'badge good' }, 'passes') : h('span', { class: 'badge bad' }, c.compiled === false ? 'does not compile' : 'does not pass'), ' ', h('span', { class: 'muted' }, c.summary || '')));
    for (const p of c.problems || []) result.append(h('div', { class: 'problem' }, p));
    for (const [key, points] of Object.entries(c.series || {})) {
      const mini = h('canvas', { class: 'mini', width: 900, height: 120 });
      result.append(h('div', { class: 'row', style: 'margin-top:8px' }, h('b', {}, key), h('span', { class: 'muted' }, `${points.length} values · last ${points.length ? points[points.length - 1][1] : '–'}`)), mini);
      setTimeout(() => drawMini(mini, points, notesFor(key)));
    }
  }
  right.append(result);

  const save = h('div', { class: 'panel' }, h('h2', {}, 'Save to the registry'),
    h('p', { class: 'hint' }, 'Writes the table and this capture into registry/tables and runs the full check. Nothing is committed or pushed.'));
  const doSave = guard(async (overwrite) => { S.saved = await api('/api/save', { yaml: S.yaml, overwrite }); render(); });
  save.append(h('button', { class: 'primary', disabled: !(c && c.ok) || s.recording, onclick: () => doSave(false) }, 'Save'));
  if (!(c && c.ok)) save.append(h('span', { class: 'muted' }, '  after a check that passes'));
  if (S.saved) {
    if (S.saved.ok) {
      save.append(h('p', { class: 'good' }, `Saved: ${S.saved.table} and ${S.saved.capture}`), h('p', { class: 'muted' }, 'To propose it for the official registry:'), h('pre', {}, S.saved.next.join('\n')));
    } else {
      for (const p of S.saved.problems || []) save.append(h('div', { class: 'problem' }, p));
      if (S.saved.exists) save.append(h('button', { class: 'danger', onclick: () => doSave(true) }, 'Replace the existing table'));
    }
  }
  right.append(save);
  return h('div', { class: 'cols' }, left, right);
}

function drawMini(canvas, points, markers) {
  const c = canvas.getContext('2d');
  const css = getComputedStyle(document.body);
  c.clearRect(0, 0, canvas.width, canvas.height);
  if (points.length < 2) return;
  const all = points.concat(markers);
  const t0 = points[0][0], t1 = points[points.length - 1][0] || 1;
  let v0 = Math.min(...all.map((p) => p[1])), v1 = Math.max(...all.map((p) => p[1]));
  if (v0 === v1) { v1 += 1; v0 -= 1; }
  const x = (t) => ((t - t0) / (t1 - t0 || 1)) * canvas.width, y = (v) => canvas.height - 6 - ((v - v0) / (v1 - v0)) * (canvas.height - 12);
  c.strokeStyle = css.getPropertyValue('--accent'); c.lineWidth = 2; c.beginPath();
  points.forEach((p, i) => (i ? c.lineTo(x(p[0]), y(p[1])) : c.moveTo(x(p[0]), y(p[1])))); c.stroke();
  c.fillStyle = css.getPropertyValue('--bad');
  for (const m of markers) { c.beginPath(); c.arc(x(m[0]), y(m[1]), 6, 0, Math.PI * 2); c.fill(); }
}

// ───────────────────────── Tester ─────────────────────────

const testerNow = () => { const p = S.capture && S.capture.packets; return p && p.length ? p[p.length - 1][0] : 0; };
const writeChr = guard(async (chr, bytes) => { await api('/api/write', { chr, hex: Tester.toHex(bytes) }); });

function viewTester() {
  const s = session();
  if (!s) return needSession('Choose the device to test first.');
  const gatt = Tester.gattOf(S.header);
  const connected = s.recording && s.connect;
  const wrap = h('div', {});
  const connect = connected
    ? h('button', { class: 'danger', onclick: guard(async () => { await api('/api/record/stop', {}); await reload(); }) }, 'Disconnect')
    : h('button', { class: 'primary', disabled: s.from_file || s.recording, onclick: guard(async () => { await api('/api/record/start', { connect: true }); await loadState(); render(); toast('Connecting…'); }) }, 'Connect');
  wrap.append(h('div', { class: 'panel' },
    h('h2', {}, 'Test ' + s.name),
    h('p', { class: 'hint' }, 'Connects the way an app would and shows what the device says in the standard Bluetooth profiles, with a log and a report for troubleshooting. Everything is recorded into the capture.',
      s.from_file ? ' This is a capture file: the readings below are replayed from it.' : ''),
    h('div', { class: 'row' }, connect,
      h('button', { onclick: () => downloadText(`${s.name.replace(/[^a-z0-9]+/gi, '-')}-report.txt`, Tester.report(s, S.header, testerNow())) }, 'Download report'),
      h('span', { class: 'muted' }, connected ? 'connected' : S.radio === 'connecting' ? 'connecting…' : 'not connected')),
    h('div', { id: 'tester-findings', style: 'margin-top:12px' })));

  const matrix = h('table', { id: 'tester-matrix' });
  const left = h('div', {}, h('div', { class: 'panel' }, h('h2', {}, 'Live values, by where they come from'),
    h('p', { class: 'hint' }, 'The same number can arrive in more than one service. An app picks one; if they disagree, that explains “my watts are different in another app”.'),
    h('div', { class: 'scroll' }, matrix)));

  if (Tester.hasChr(gatt, '2ad9')) {
    const num = (id, value, step) => h('input', { id, type: 'number', value, step, style: 'width:90px' });
    const val = (id) => Number($('#' + id).value);
    left.append(h('div', { class: 'panel' }, h('h2', {}, 'Control the trainer'),
      h('p', { class: 'hint' }, 'FTMS control point. Ask for control first; only one app can hold it.'),
      h('div', { class: 'row' },
        h('button', { disabled: !connected, onclick: () => writeChr('2ad9', Tester.commands.requestControl()) }, 'Request control'),
        h('button', { disabled: !connected, onclick: () => writeChr('2ad9', Tester.commands.start()) }, 'Start'),
        h('button', { disabled: !connected, onclick: () => writeChr('2ad9', Tester.commands.pause()) }, 'Pause'),
        h('button', { disabled: !connected, onclick: () => writeChr('2ad9', Tester.commands.stop()) }, 'Stop')),
      h('div', { class: 'row', style: 'margin-top:10px' }, num('t-power', 200, 5), 'W', h('button', { disabled: !connected, onclick: () => writeChr('2ad9', Tester.commands.power(val('t-power'))) }, 'Set power (ERG)'),
        num('t-slope', 0, 0.5), '%', h('button', { disabled: !connected, onclick: () => writeChr('2ad9', Tester.commands.slope(val('t-slope'))) }, 'Set slope'),
        num('t-res', 10, 1), h('button', { disabled: !connected, onclick: () => writeChr('2ad9', Tester.commands.resistance(val('t-res'))) }, 'Set resistance')),
      h('p', { id: 'tester-control', class: 'muted', style: 'margin:10px 0 0' })));
  }

  const services = h('div', {});
  if (!gatt.length) services.append(h('p', { class: 'muted' }, 'Connect to list them.'));
  for (const sv of gatt) {
    services.append(h('h3', {}, `${Tester.SERVICE_NAMES[sv.uuid] || 'Unknown service'} · ${sv.uuid}`));
    services.append(h('ul', { class: 'list' }, sv.characteristics.map((c) => h('li', {}, h('span', { class: 'mono' }, c.uuid.length > 8 ? c.uuid.slice(0, 13) + '…' : c.uuid), h('span', { class: 'fill' }, Tester.CHAR_NAMES[c.uuid] || ''), h('span', { class: 'muted' }, c.properties.join(', '))))));
  }
  const right = h('div', {}, h('div', { class: 'panel' }, h('h2', {}, 'Services and characteristics'), services));

  wrap.append(h('div', { class: 'cols' }, left, right));
  wrap.append(h('div', { class: 'panel' }, h('h2', {}, 'Log'), h('p', { class: 'hint' }, 'Newest at the bottom: time, direction, characteristic, bytes, and what they mean where a standard says so.'),
    h('pre', { id: 'tester-log', style: 'max-height:340px; overflow:auto; white-space:pre' })));
  return wrap;
}

function renderTester() {
  const T = Tester.state();
  const s = session();
  const box = $('#tester-findings');
  if (!box || !s) return;
  const found = Tester.findings(S.header, s.recording && s.connect, testerNow());
  box.replaceChildren(...found.map((f) => h('div', { class: f.level === 'bad' ? 'problem' : f.level === 'good' ? 'good' : '', style: 'padding:3px 0' }, (f.level === 'bad' ? '✗ ' : f.level === 'good' ? '✓ ' : '• ') + f.text)));
  const cols = [['FTMS', 'FTMS'], ['CPS', 'Cycling Power'], ['CSC', 'Speed & Cadence'], ['HR', 'Heart Rate']];
  const rows = [['power', 'Power', 'W', 0], ['cadence', 'Cadence', 'rpm', 0], ['speed', 'Speed', 'km/h', 1], ['heartRate', 'Heart rate', 'bpm', 0], ['resistance', 'Resistance', '', 0]];
  const now = testerNow();
  $('#tester-matrix').replaceChildren(
    h('tr', {}, h('th', {}, ''), cols.map(([, label]) => h('th', { class: 'num' }, label))),
    ...rows.map(([key, label, unit, digits]) => h('tr', {}, h('td', {}, label, unit ? h('span', { class: 'muted' }, ' ' + unit) : null),
      cols.map(([c]) => { const v = T.src[c][key]; const old = T.src[c].at !== undefined && now - T.src[c].at > 4000; return h('td', { class: 'num' + (old ? ' muted' : ''), style: 'font-size:20px; font-weight:600' }, v === undefined ? '–' : Number(v).toFixed(digits)); }))));
  const ctl = $('#tester-control');
  if (ctl) ctl.textContent = (T.control.granted ? 'Control granted. ' : 'No control yet. ') + (T.control.last ? 'Last answer: ' + T.control.last : '');
  const logEl = $('#tester-log');
  const stick = logEl.scrollTop + logEl.clientHeight >= logEl.scrollHeight - 30;
  logEl.textContent = T.log.slice(-250).map((l) => `${(l.t / 1000).toFixed(1).padStart(8)}  ${l.dir.toUpperCase().padEnd(4)} ${l.chr.padEnd(6)} ${l.hex}${l.text ? '   ' + l.text : ''}`).join('\n') || 'Nothing yet.';
  if (stick) logEl.scrollTop = logEl.scrollHeight;
}

function downloadText(name, text) {
  const a = h('a', { href: URL.createObjectURL(new Blob([text], { type: 'text/plain' })), download: name });
  document.body.append(a);
  a.click();
  a.remove();
}

// ───────────────────────── assistant ─────────────────────────

function assistantDialog() {
  const dlg = $('#dialog');
  const prompt = `OFP Studio is running at ${location.origin} and I have it open. Help me add a device to the Open Fitness Protocol registry, or fix the table of one that reads wrongly.

The job: one table (a data file in registry/tables/) that reads my device. You change the table and nothing else. Do not edit the core (anything under crates/, packages/, scripts/) to make a device work. If the table language cannot express what the device sends, stop and tell me exactly what it could not say: that goes to the project as an issue ("A table cannot express this device"), not as a change you make.

1. Read the rules first: GET ${location.origin}/api/guide ("The whole job, start to finish" and "Stay in the table").
2. See where we are: GET /api/state (fields, tables, the session) and GET /api/capture (packets, my notes, my confirmations). GET /api/similar says whether the registry already knows this device: if it does (a draft counts), fix that table, do not add a second one.
3. Find the bytes: POST /api/solve {} ranks readings against my notes; POST /api/series {"expr":"u16le[4] / 10"} gives one reading over the capture.
4. Set the table: POST /api/table/design {"name":…,"mappings":[{"field":"power","expr":"u16le[8]"}],"only_when":…,"instance_expr":…,"instance_label":…} — or POST /api/table/check {"yaml":…} with the full text. Both show up in my page as you do them.
5. POST /api/table/check tells you whether the capture passes. Do not call /api/save: I will press Save.

We are done when every value my device sends is on the Live step, I have marked each one right, and the check passes. Until then, ask me to record more, to note what the display shows, or to say right or wrong: those are mine to give, they are what I read on the device. POST bodies are JSON.`;
  dlg.replaceChildren(
    h('h2', { style: 'margin-top:0' }, 'Work with an assistant'),
    h('p', { class: 'hint' }, 'This page and an AI assistant can work on the same session: what it finds or changes appears here as it happens, and you stay the one who reads the device and presses Save. Paste this into Claude Code (or any assistant that can call a local address):'),
    h('pre', {}, prompt),
    h('div', { class: 'row', style: 'justify-content:flex-end' },
      h('button', { class: 'ghost', onclick: () => dlg.close() }, 'Close'),
      h('button', { class: 'primary', onclick: async () => { try { await navigator.clipboard.writeText(prompt); toast('Copied'); } catch { toast('Select the text and copy it', true); } } }, 'Copy')));
  dlg.showModal();
}

// ───────────────────────── the simple Studio: Connect, check, send ─────────────────────────

/** Is the live view on screen (the Live tab, or the simple Studio with a device connected)? */
function showingLive() {
  return S.tab === 'live' || (!S.advanced && !!session());
}

/** Where "Send to OFP" goes. `?submit=<url>` on the page points it elsewhere (for testing). */
const SUBMIT = new URLSearchParams(location.search).get('submit') || 'https://openfitnessprotocol.org/api/submit';

/** In the simple Studio: the device list ('connect') or the device being read ('check'). Going to the list
 *  does not disconnect: the device keeps recording until another one is connected. */
let simpleView = 'check';
/** The device a Connect click is waiting for (its button shows a spinner until the answer). */
let connectingId = null;

const connect = guard(async (d, button) => {
  S.sent = null;
  simpleView = 'check';
  connectingId = d.device_id;
  if (button) { button.disabled = true; button.classList.add('busy'); button.textContent = 'Connecting…'; }
  try {
    if (!(session() && session().device_id === d.device_id)) {
      await api('/api/session', { device_id: d.device_id });
      await api('/api/record/start', {});
    } else if (!session().recording) {
      await api('/api/record/start', {});
    }
  } finally {
    // the button is drawn again from the state below, whatever happened
    connectingId = null;
  }
  await reload();
});

/** Back to the list, the device still connected and recording. */
function chooseAnother() {
  S.sent = null;
  simpleView = 'connect';
  render();
}

/** Stop reading the device and let it go; what was recorded so far stays saved on this computer. */
const disconnect = guard(async () => {
  await api('/api/session', {});
  S.sent = null;
  simpleView = 'connect';
  await reload();
  toast('Disconnected.');
});

/** Back to the device being read. */
function backToDevice() {
  simpleView = 'check';
  render();
}

function viewSimple() {
  const s = session();
  if (S.sent) return viewSent();
  if (!s || s.from_file || simpleView === 'connect') return viewConnect(s && !s.from_file ? s : null);
  return viewCheck(s);
}

/** Step 1: the devices nearby, as big cards with one Connect button each. `current`: a device still connected. */
function viewConnect(current) {
  const wrap = h('div', { class: 'simple-page' },
    h('div', { class: 'steps' }, h('span', { class: 'on' }, '1 Connect'),
      current ? h('button', { class: 'done', title: 'Back to ' + current.name, onclick: backToDevice }, '2 Check your numbers') : h('span', {}, '2 Check your numbers'),
      h('span', {}, '3 Send')),
    current ? h('p', { class: 'connected-note' }, 'Connected to ', h('b', {}, current.name), '. ', h('button', { class: 'linkish', onclick: backToDevice }, 'Back to its numbers'), ' · ', h('button', { class: 'linkish', onclick: disconnect }, 'Disconnect')) : null,
    h('h1', {}, 'Connect your device'),
    h('p', { class: 'lead' }, 'Wake it up first: pedal, press a button, or put the strap on. It appears here within a few seconds.'),
    h('div', { id: 'cards-readable', class: 'cards' }),
    h('div', { id: 'cards-maybe-wrap', hidden: true }, h('h2', {}, 'Looks like a fitness device'), h('p', { class: 'muted' }, 'OFP can\'t read these yet. Connect yours anyway and send it to us: that is how it gets added.'), h('div', { id: 'cards-maybe', class: 'cards' })),
    h('div', { id: 'cards-others-wrap' }, h('button', { id: 'others-toggle', class: 'linkish', onclick: () => { showOthers = !showOthers; renderDeviceCards(); } }), h('div', { id: 'cards-others', class: 'cards' })),
    h('div', { id: 'cards-empty', class: 'empty-note' }));
  setTimeout(renderDeviceCards);
  return wrap;
}

const cardEls = new Map(); // device id → its card, updated in place so nothing jumps under the pointer

function renderDeviceCards() {
  const boxes = { readable: $('#cards-readable'), maybe: $('#cards-maybe'), others: $('#cards-others') };
  if (!boxes.readable) return;
  const counts = { readable: 0, maybe: 0, others: 0 };
  const seen = new Set();
  const last = { readable: null, maybe: null, others: null };
  for (const d of S.devices) {
    seen.add(d.device_id);
    const by = readsAs(d);
    const group = by.length ? 'readable' : looksLikeFitness(d) ? 'maybe' : 'others';
    counts[group]++;
    let card = cardEls.get(d.device_id);
    if (!card) {
      card = h('div', { class: 'device-card' }, h('div', { class: 'title' }), h('div', { class: 'what' }),
        h('button', { class: 'connect', onclick: (e) => (session() && session().device_id === d.device_id ? backToDevice() : connect(d, e.currentTarget)) }, 'Connect'));
      cardEls.set(d.device_id, card);
    }
    const box = boxes[group];
    const want = last[group] ? last[group].nextSibling : box.firstChild;
    if (want !== card) box.insertBefore(card, want);
    last[group] = card;
    const quiet = d.silent_s > DEVICE_QUIET_S;
    const set = (el, text) => { if (el.textContent !== text) el.textContent = text; };
    set(card.children[0], d.name || 'A device with no name');
    // what reads it, without the device's own name repeated after it
    const what = by.map((b) => b.split(' · ')[0].replace('Fitness machine (FTMS)', 'FTMS')).filter((b, i, all) => all.indexOf(b) === i);
    set(card.children[1], quiet ? 'Not heard for a while: wake it up, then Connect' : what.length ? 'OFP reads it: ' + what.join(', ') : 'OFP can\'t read it yet');
    // a device that is connected stops advertising, so "quiet" is normal for it: never greyed out or locked
    const isCur = !!(session() && session().device_id === d.device_id);
    card.classList.toggle('quiet', quiet && !isCur);
    card.classList.toggle('current', isCur);
    if (isCur) set(card.children[1], 'Connected now');
    const button = card.children[2];
    const busy = connectingId === d.device_id;
    button.classList.toggle('busy', busy);
    button.disabled = busy;
    set(button, busy ? 'Connecting…' : isCur ? 'Show its numbers' : 'Connect');
  }
  for (const [id, card] of cardEls) if (!seen.has(id)) { card.remove(); cardEls.delete(id); }
  $('#cards-maybe-wrap').hidden = !counts.maybe;
  boxes.others.hidden = !showOthers;
  const t = $('#others-toggle');
  const things = counts.others === 1 ? 'the 1 other thing nearby' : `the ${counts.others} other things nearby`;
  t.hidden = !counts.others;
  t.replaceChildren(...(showOthers ? [`Hide ${things}`] : [h('b', { style: 'text-decoration:underline' }, "Don't see your device?"), ` Show ${things}`]));
  const empty = $('#cards-empty');
  const text = counts.readable || counts.maybe ? '' : S.radio.startsWith('error') ? S.radio.slice(7) : S.radio === 'off' ? 'This build has no Bluetooth.' : 'Looking for devices… wake yours up.';
  if (empty.textContent !== text) empty.textContent = text;
  empty.classList.toggle('searching', text.startsWith('Looking'));
}

/** What the device says about itself when connected (Device Information: maker 2a29, model 2a24). */
function deviceInfo() {
  const text = (chr) => {
    const r = ((S.capture && S.capture.io) || []).find((x) => x[1] === 'read' && String(x[2]).toLowerCase().replace(/^0000|-0000-1000-8000-00805f9b34fb$/g, '') === chr);
    if (!r) return '';
    const hex = String(r[3]).replace(/\s/g, '');
    let out = '';
    for (let i = 0; i + 1 < hex.length; i += 2) out += String.fromCharCode(parseInt(hex.slice(i, i + 2), 16));
    return out.replace(/\0+$/, '').trim();
  };
  return { make: text('2a29'), model: text('2a24') };
}

/** What a person calls a number → the field key OFP uses, when it is clearly one we know. */
const SAID_FIELD = [
  [/^(w|watts?|power)$/, 'power'], [/^(rpm|cadence|revs?)$/, 'cadence'], [/^(km\/?h|kph|speed)$/, 'speed'],
  [/^(bpm|heart ?rate|hr|pulse)$/, 'heart_rate'], [/^(level|resistance|gear)$/, 'resistance'], [/^(kcal|cal|calories|energy)$/, 'energy'],
  [/^(spm|strokes? ?(rate|per minute)?)$/, 'stroke_rate'], [/^(incline|grade|%)$/, 'incline'],
];
function fieldFromWords(words) {
  const w = words.trim().toLowerCase();
  for (const [re, key] of SAID_FIELD) if (re.test(w)) return key;
  return w.replace(/[^a-z0-9]+/g, '_').replace(/^_|_$/g, '') || 'value';
}

/** For a device OFP cannot read (or reads wrongly): hold steady, then say what the display shows. */
function holdSteady(s, open) {
  const rows = [0, 1, 2].map(() => ({ n: h('input', { type: 'text', inputmode: 'decimal', placeholder: '150' }), what: h('input', { placeholder: 'watts, rpm, level, km/h, …' }) }));
  const start = h('button', { class: 'small', onclick: () => {
    let left = 20;
    start.disabled = true;
    const tick = () => {
      if (left > 0) { start.textContent = `Hold it… ${left} s`; left--; setTimeout(tick, 1000); return; }
      start.textContent = 'Now: what does the display show?';
      rows[0].n.focus();
      setTimeout(() => { start.disabled = false; start.textContent = 'Start 20 seconds again'; }, 4000);
    };
    tick();
  } }, 'Start 20 seconds');
  const save = guard(async () => {
    const said = rows.filter((r) => r.n.value.trim() !== '' && !Number.isNaN(Number(r.n.value.replace(',', '.'))));
    if (!said.length) throw new Error('Type at least one number from the display.');
    const pairs = said.map((r) => `${fieldFromWords(r.what.value)}=${Number(r.n.value.replace(',', '.'))}`);
    const words = said.map((r) => `${r.n.value.trim()} ${r.what.value.trim() || '(no meaning given)'}`).join('; ');
    // the values go in a form the Studio's "find it for me" uses, the person's own words beside them
    await api('/api/note', { text: `DISPLAY ${pairs.join(' ')} (said: ${words})` });
    for (const r of rows) { r.n.value = ''; r.what.value = ''; }
    toast('Saved with the time. Hold another value and do it again if you can.');
  });
  return h('details', { class: 'panel hold', open: open || undefined },
    h('summary', {}, open ? 'Help us read it: hold a steady value' : 'A number missing or wrong? Tell us what your display shows'),
    h('p', {}, 'Ride (or row, or run) at a steady effort for about 20 seconds, so the numbers on your device\'s own display stop moving. Then type what it shows, and what each number means.'),
    h('div', { class: 'row' }, start),
    h('div', { class: 'said' }, rows.map((r) => h('div', { class: 'said-row' }, r.n, r.what))),
    h('div', { class: 'row' }, h('button', { class: 'small primary', onclick: save, disabled: !s.recording }, 'Save these numbers')));
}

/** Steps 2 and 3: the numbers, right or wrong, a comment, and Send. */
function viewCheck(s) {
  const info = deviceInfo();
  const readable = !!S.live.source;
  const make = h('input', { id: 'send-make', value: info.make, placeholder: 'e.g. Inspire, Yesoul, Concept2' });
  const model = h('input', { id: 'send-model', value: info.model, placeholder: 'e.g. IC2, G1M Plus, BikeErg' });
  const comment = h('textarea', { id: 'send-comment', rows: 3, placeholder: 'A number that is missing, one that is always 0, the display shows something else, …' });
  const email = h('input', { id: 'send-email', type: 'email', placeholder: 'you@example.com (optional)' });
  const send = guard(async () => {
    const button = $('#send-button');
    button.disabled = true;
    button.textContent = 'Sending…';
    try {
      // the comment goes into the recording too, with the time, while it is still recording
      if (comment.value.trim()) await api('/api/note', { text: 'COMMENT: ' + comment.value.trim() }).catch(() => {});
      if (make.value.trim() || model.value.trim()) await api('/api/note', { text: `DEVICE: make=${make.value.trim()} model=${model.value.trim()}` }).catch(() => {});
      if (session() && session().recording) await api('/api/record/stop', {});
      const recording = await (await fetch('/api/capture/file', { cache: 'no-store' })).text();
      const src = S.live.source;
      const r = await fetch(SUBMIT, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ recording, comment: comment.value.trim(), email: email.value.trim(), make: make.value.trim(), model: model.value.trim(), version: S.state && S.state.version, device: s.name, read_as: src ? src.name : 'nothing reads it yet' }),
      });
      const j = await r.json().catch(() => ({}));
      if (!r.ok || !j.code) throw new Error(j.error || 'Sending did not work. Check the internet connection and try again.');
      // did the person say their numbers are right (and none wrong)? Then the device works with OFP
      const confirms = (S.capture && S.capture.confirms) || [];
      const works = confirms.some((c) => c.ok) && !confirms.some((c) => !c.ok);
      S.sent = { code: j.code, name: [make.value.trim(), model.value.trim()].filter(Boolean).join(' ') || s.name, works };
      await reload();
    } catch (e) {
      button.disabled = false;
      button.textContent = 'Send to OFP';
      throw e;
    }
  });
  return h('div', { class: 'simple-page' },
    h('div', { class: 'steps' }, h('button', { class: 'done', title: 'Choose a different device', onclick: chooseAnother }, '1 Connect'), h('span', { class: 'on' }, '2 Check your numbers'), h('span', {}, '3 Send')),
    h('div', { class: 'row', style: 'justify-content:space-between; align-items:baseline' },
      h('h1', {}, s.name),
      h('div', { class: 'row' }, h('button', { class: 'linkish', onclick: chooseAnother }, 'Choose another device'), h('button', { class: 'small disconnect', onclick: disconnect }, 'Disconnect'))),
    h('p', { class: 'lead' }, 'Use your device normally and compare with its own display. For each number: does it look right? They move, so close is right.'),
    h('div', { id: 'live-status', class: 'live-status' }), h('div', { id: 'tiles' }),
    holdSteady(s, !readable),
    h('div', { class: 'panel send' },
      h('h2', {}, '3 Send it to us'),
      h('label', {}, readable ? 'Which device is it?' : 'Which device is it? OFP doesn\'t know it yet, so this helps us most'),
      h('div', { class: 'two' }, h('div', {}, h('span', { class: 'muted' }, 'Make'), make), h('div', {}, h('span', { class: 'muted' }, 'Model'), model)),
      info.make || info.model ? h('p', { class: 'muted small-print' }, 'Filled in from what the device says about itself: correct it if it is wrong.') : null,
      h('label', {}, 'Anything missing or wrong? Tell us'), comment,
      h('label', {}, 'Your email, only if you want us to reply'), email,
      h('p', { class: 'muted small-print' }, 'We receive what your device sent while it was connected, your answers above, your comment and your email if you give one. A recording can include your heart rate. It is kept privately for 90 days, read only by the people who maintain OFP, and used only to add or fix your device.'),
      h('button', { id: 'send-button', class: 'connect', onclick: send }, 'Send to OFP')));
}

/** The device works: the apps the person uses can read it too, once they use OFP. A sentence to send them. */
function tellYourApps(device) {
  const text = `My ${device} works with Open Fitness Protocol: I checked its numbers in OFP Studio. Could your app read it through OFP? https://openfitnessprotocol.org`;
  return h('div', { class: 'panel tell' },
    h('h2', {}, '✓ Your device works with OFP'),
    h('p', {}, 'Apps that use Open Fitness Protocol can read it (a device new to OFP reaches them with their next update of the library). If an app you use doesn\'t read your device, tell them. You can send this:'),
    h('pre', { class: 'tell-text' }, text),
    h('button', { class: 'small', onclick: async () => { try { await navigator.clipboard.writeText(text); toast('Copied'); } catch { toast('Select the text and copy it', true); } } }, 'Copy'));
}

function viewSent() {
  const issue = `https://github.com/trainerday/openfitnessprotocol/issues/new?template=device-request.yml&title=${encodeURIComponent('Recording ' + S.sent.code + ': ' + S.sent.name)}`;
  return h('div', { class: 'simple-page' },
    h('div', { class: 'steps' }, h('span', { class: 'done' }, '1 Connect'), h('span', { class: 'done' }, '2 Check your numbers'), h('span', { class: 'done' }, '3 Send')),
    h('h1', {}, 'Thank you'),
    h('p', { class: 'lead' }, 'Your recording is with us. Its code is'),
    h('div', { class: 'code' }, S.sent.code),
    h('p', {}, 'Keep the code: it is how we find your recording. If you want to follow along, ', h('a', { href: issue, target: '_blank', rel: 'noopener' }, 'open an issue on GitHub'), ' with the code in it. Someone (or an AI assistant) can now work out your device from what it sent, and you get it in a later version of OFP Studio.'),
    S.sent.works ? tellYourApps(S.sent.name) : null,
    h('div', { class: 'row' }, h('button', { class: 'connect', onclick: chooseAnother }, 'Connect another device')));
}

// ───────────────────────── version, and a newer release ─────────────────────────

/** Where releases are published. `?updates=<url>` points the check elsewhere (for testing the banner). */
const RELEASES = new URLSearchParams(location.search).get('updates') || 'https://openfitnessprotocol.org/download/latest.json';

/** 1 when a is newer than b ("0.10.0" > "0.9.3"), -1 when older, 0 when the same. */
function compareVersions(a, b) {
  const pa = String(a).split('.').map(Number), pb = String(b).split('.').map(Number);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] || 0) - (pb[i] || 0);
    if (d) return d > 0 ? 1 : -1;
  }
  return 0;
}

/** Once per start: show this build's version, and a banner when a newer one is published. Silent offline. */
async function checkForUpdate() {
  const mine = S.state && S.state.version;
  if (!mine) return;
  $('#version').textContent = mine;
  document.title = 'OFP Studio ' + mine;
  try {
    const latest = await (await fetch(RELEASES, { cache: 'no-store' })).json();
    if (!latest.version || compareVersions(latest.version, mine) <= 0) return;
    const b = $('#update');
    b.hidden = false;
    b.replaceChildren(
      h('span', {}, h('b', {}, `OFP Studio ${latest.version} is available.`), ` You have ${mine}.`),
      // A browser opens a tab; the app's window has none, and sends any address that is not the Studio's
      // to the computer's browser instead (its navigation handler).
      h('button', { class: 'small primary', onclick: () => { const url = latest.page || latest.url; if (!window.open(url, '_blank')) location.href = url; } }, 'Download'),
      h('button', { class: 'small ghost', onclick: () => { b.hidden = true; } }, 'Later'));
  } catch {
    // offline, or the site is down: say nothing
  }
}

// ───────────────────────── start ─────────────────────────

setInterval(() => { if (showingLive()) renderTiles(); }, 1000);
document.querySelectorAll('#tabs button').forEach((b) => b.addEventListener('click', () => setTab(b.dataset.tab)));
$('#assistant-open').addEventListener('click', assistantDialog);
$('#advanced').addEventListener('change', (e) => { S.advanced = e.target.checked; localStorage.setItem('ofp-advanced', S.advanced ? '1' : '0'); render(); });
reload().then(() => { checkForUpdate(); if (session()) setTab(session().from_file ? 'map' : 'record'); connectEvents(); }).catch((e) => toast(e.message, true));
