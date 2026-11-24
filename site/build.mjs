// Builds the site's pages into site/dist/ from CHANGELOG.md (the notes) and site/releases.json (what was
// published: version, date, file, size, sha256). No dependencies: `node site/build.mjs`.
import { readFileSync, writeFileSync, mkdirSync, rmSync, copyFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..');
const out = join(here, 'dist');
const SITE = 'https://openfitnessprotocol.org';
const REPO = 'https://github.com/trainerday/openfitnessprotocol';

const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
/** The little Markdown the changelog uses: `code`, **bold**, [text](url). */
const inline = (s) => esc(s)
  .replace(/`([^`]+)`/g, '<code>$1</code>')
  .replace(/\*\*([^*]+)\*\*/g, '<b>$1</b>')
  .replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, '<a href="$2">$1</a>');

/** CHANGELOG.md → { "0.1.0": { date, notes: [...] }, Next: {...} } */
export function readChangelog(text) {
  const sections = {};
  let cur = null;
  for (const line of text.split('\n')) {
    const head = /^## (Next|\d+\.\d+\.\d+)(?: \((\d{4}-\d{2}-\d{2})\))?\s*$/.exec(line);
    if (head) { cur = sections[head[1]] = { date: head[2] || null, notes: [] }; continue; }
    if (/^## /.test(line)) { cur = null; continue; }
    if (!cur) continue;
    if (/^- /.test(line)) cur.notes.push(line.slice(2).trim());
    else if (/^\s+\S/.test(line) && cur.notes.length) cur.notes[cur.notes.length - 1] += ' ' + line.trim();
  }
  return sections;
}

const mb = (bytes) => (bytes / 1048576).toFixed(1) + ' MB';
const longDate = (d) => new Date(d + 'T12:00:00Z').toLocaleDateString('en-US', { year: 'numeric', month: 'long', day: 'numeric', timeZone: 'UTC' });

const MARK = `<svg viewBox="0 0 64 64" aria-hidden="true"><rect x="2" y="2" width="60" height="60" rx="14" fill="#171626"/><g fill="none" stroke-width="5" stroke-linecap="round"><path d="M21 34a9 9 0 0 1 9 9" stroke="#60a5fa"/><path d="M21 25a18 18 0 0 1 18 18" stroke="#2563eb" stroke-opacity=".85"/><path d="M21 16a27 27 0 0 1 27 27" stroke="#2563eb" stroke-opacity=".55"/></g><circle cx="21" cy="43" r="4.5" fill="#fff"/></svg>`;

function page({ path, title, description, body }) {
  const nav = [['/download', 'Download'], ['/developers', 'Developers'], ['/releases', 'Releases'], [REPO, 'GitHub']]
    .map(([href, label]) => `<a href="${href}"${href === path ? ' class="on"' : ''}>${label}</a>`).join('');
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${esc(title)}</title>
<meta name="description" content="${esc(description)}">
<link rel="canonical" href="${SITE}${path === '/' ? '/' : path}">
<link rel="icon" href="/icon.svg" type="image/svg+xml">
<meta property="og:title" content="${esc(title)}">
<meta property="og:description" content="${esc(description)}">
<link rel="stylesheet" href="/style.css">
<script defer src="https://webstats.trainerday.com/script.js" data-website-id="1c6ebc3e-1f85-49fe-8b57-699df4a580e4" data-domains="openfitnessprotocol.org"></script>
</head>
<body>
<header class="top"><div class="wrap">
  <a class="mark" href="/">${MARK}<span>Open Fitness Protocol</span></a>
  <nav class="links">${nav}</nav>
</div></header>
<main class="wrap">
${body}
</main>
<footer class="bottom"><div class="wrap">
  <p>Open Fitness Protocol is open source under the <a href="${REPO}/blob/main/LICENSE">Apache-2.0 license</a>. <a href="${REPO}/blob/main/CONTRIBUTING.md">Contributing</a> · <a href="${REPO}/blob/main/SECURITY.md">Security</a></p>
  <p>“Open Fitness Protocol” and “OFP” are trademarks of TrainerDay Inc (<a href="${REPO}/blob/main/TRADEMARKS.md">how the names may be used</a>).</p>
</div></footer>
</body>
</html>
`;
}

function home(latest) {
  const studio = latest
    ? `<a class="button" href="/download">Download OFP Studio</a><span class="small">Free · ${latest.files.windows ? 'Mac and Windows' : 'Mac'}</span>`
    : `<a class="button quiet" href="/download">OFP Studio: coming soon</a>`;
  return page({
    path: '/', title: 'Open Fitness Protocol: one library to read every fitness device',
    description: 'Add one open library to your app and read every fitness device the community supports. A device that does not work yet: test it with OFP Studio, and an AI assistant can set it up.',
    body: `
<div class="hero">
  <h1>One library to read every fitness device.</h1>
  <p class="lead">Add OFP to your app once. You don't maintain code for each device: the community does. New devices come to your app with a simple library update: no code of yours changes.</p>
  <div class="paths">
    <div class="path">
      <span class="who">Have a fitness device?</span>
      <a class="button" href="/download">Download OFP Studio</a>
      <span class="small">Test it in a minute · free · Mac and Windows</span>
    </div>
    <div class="path">
      <span class="who">Developer with a fitness app?</span>
      <a class="button quiet" href="/developers">Add OFP to your app</a>
      <code class="install" title="Install the library">npm install @openfitnessprotocol/ofp</code>
      <span class="small">One library for any JavaScript app · open source · Apache-2.0</span>
    </div>
  </div>
</div>

<section class="free" aria-label="Free and open source">
  <div><b>100% free</b>No fees, no paid tiers, no account. Ever.</div>
  <div><b>Fully open source</b>Apache-2.0. Use it in any app, open or closed, commercial or not.</div>
  <div><b>For every app</b>Not tied to one company's app. Any app can use it, and anyone can add a device.</div>
</section>

<section class="scope">
  <h2>OFP reads data. It does not control devices.</h2>
  <div class="cards">
    <div class="card yes"><h3><span class="mark-yes" aria-hidden="true">✓</span> Reading fitness data: yes</h3><p>A Concept2 rower's strokes and watts, a power meter, a heart rate strap, a bike that only broadcasts its numbers, a breathing or temperature sensor. A smart trainer's power and cadence too.</p></div>
    <div class="card no"><h3><span class="mark-no" aria-hidden="true">✕</span> Controlling fitness devices: no</h3><p>Setting a smart trainer's resistance or ERG mode (a Wahoo KICKR is read, not steered), or changing a treadmill's speed. Your app keeps doing that the way it does now. Control may come later.</p></div>
  </div>
</section>

<section class="card spotlight">
  <h2>Got a device that doesn't work?</h2>
  <p>Install OFP Studio and test it: wake the device, click <b>${latest && !simpleStudio(latest.version) ? 'Use' : 'Connect'}</b>, and see what it sends. Then give the recording and this project to an AI assistant (Claude, ChatGPT, any that can read code), and it can set the device up for you. You check the numbers against the device's own display, and send it in.</p>
  <div class="actions">${studio}</div>
  <div class="shots">
    <a href="/images/studio-connect.png" class="shot-link" data-shot><img class="shot" src="/images/studio-connect.png" width="1030" height="567" alt="OFP Studio, step 1: the Bluetooth devices nearby, each with a Connect button and what OFP reads it as"><span>1 · Connect your Bluetooth device</span></a>
    <a href="/images/studio-check.png" class="shot-link" data-shot><img class="shot" src="/images/studio-check.png" width="1015" height="511" alt="OFP Studio, step 2: the device's numbers, each with Looks right or Looks wrong"><span>2 · Check your numbers, then send</span></a>
  </div>
  <dialog id="shot-big" onclick="this.close()"><img alt=""></dialog>
  <script>
    for (const a of document.querySelectorAll('[data-shot]')) a.addEventListener('click', (e) => {
      e.preventDefault();
      const d = document.getElementById('shot-big');
      d.querySelector('img').src = a.href;
      d.querySelector('img').alt = a.querySelector('img').alt;
      d.showModal();
    });
  </script>
</section>

<section>
  <h2>How a device gets in</h2>
  <div class="flow">
    <div><b>Test it</b><br>in OFP Studio, and save the recording.</div>
    <div><b>An AI assistant sets it up</b><br>from the recording and this project.</div>
    <div><b>You check it</b><br>each value right or wrong, on the device.</div>
    <div><b>Send it in</b><br>an automated check replays your recording.</div>
    <div><b>It works everywhere</b><br>in every app that uses OFP.</div>
  </div>
</section>

<section>
  <h2>Why</h2>
  <p>Heart rate, power and smart trainers have Bluetooth standards. Newer sensors (breathing, core temperature, bikes that only broadcast their numbers) don't, so each maker invents a private format and each app has to write code for each device. Most of that code never gets written. OFP puts one open piece in the middle, and its rules live in a public repository, not in a committee.</p>
</section>

<section>
  <h2>Devices</h2>
  <div class="cards devices">
    <div class="card yes"><h3><span class="mark-yes" aria-hidden="true">✓</span> In the package today</h3>
      <p class="small">What an app that installs <code>@openfitnessprotocol/ofp</code> reads.</p>
      <ul>
        <li><b>Keiser M3i</b> bike: checked with a recording from a real bike</li>
        <li><b>CORE body temperature sensor</b>: from the maker's specification, checked by test vectors</li>
        <li><b>Tymewear VitalPro breathing sensor</b>: from open readers' findings, checked by test vectors</li>
      </ul>
    </div>
    <div class="card maybe"><h3><span class="mark-maybe" aria-hidden="true">✓</span> Being verified</h3>
      <p class="small">OFP Studio reads them; each goes into the package once a recording from a real device is in.</p>
      <ul>
        <li><b>Heart rate straps</b> (Garmin, Polar and others): heart rate and RR intervals for HRV</li>
        <li><b>Power meters</b>, <b>speed and cadence sensors</b>, running speed and cadence sensors</li>
        <li><b>Spin bikes and indoor bikes</b> with the Bluetooth fitness machine standard (FTMS): Schwinn IC4 / Bowflex C6, Stages, newer Yesoul and Domyos, and many more</li>
        <li><b>Rowers</b> (Concept2 and others) and <b>treadmills</b> with FTMS</li>
        <li>Domyos bikes without FTMS, older Yesoul, iConsole+ consoles, Nautilus</li>
        <li>Inspire IC, XCX, Chrono bikes</li>
      </ul>
      <p class="cta-line"><b>Have one of these? Please verify it.</b> Or add your own device. Connect it in <a href="/download">OFP Studio</a>, check the numbers, and send it.</p>
    </div>
  </div>
  <p class="small"><b>Smart trainers</b> (Wahoo KICKR, Tacx, Elite, Saris, Wattbike and others) are read too, power and cadence, but <b>read only</b>: OFP does not set their resistance or ERG mode. Your app keeps controlling them the way it does now, so there is usually little reason to read them through OFP.</p>
  <p class="small">OFP is new: this is its first version, and things can still change before 1.0.</p>
</section>
`,
  });
}

function notesList(notes) {
  return notes.length ? `<ul>${notes.map((n) => `<li>${inline(n)}</li>`).join('')}</ul>` : '';
}

/** From 0.2.0 the Studio opens on Connect, check your numbers, send. */
function simpleStudio(version) {
  const [a, b] = String(version).split('.').map(Number);
  return a > 0 || b >= 2;
}

function download(latest, changelog) {
  if (!latest) {
    return page({ path: '/download', title: 'Download OFP Studio', description: 'OFP Studio is not released yet.',
      body: `<div class="hero"><h1>Download OFP Studio</h1><p class="lead">Not released yet. Soon.</p></div>` });
  }
  const notes = (changelog[latest.version] || { notes: [] }).notes;
  return page({
    path: '/download', title: `Download OFP Studio ${latest.version}`,
    description: 'OFP Studio: see what your fitness device sends, and help add it to Open Fitness Protocol.',
    body: `
<div class="hero">
  <h1>OFP Studio</h1>
  <p class="lead">See what your fitness device sends, check its values, and record it so it can be added to OFP.</p>
  <div class="actions">
    <a class="button" href="${esc(latest.files.mac.url)}">Download for Mac</a>
    ${latest.files.windows ? `<a class="button" href="${esc(latest.files.windows.url)}">Download for Windows</a>` : ''}
  </div>
  <p class="small">Version ${esc(latest.version)} · ${longDate(latest.date)} · free and open source (Apache-2.0)</p>
  <p class="small">Mac: macOS 11 or later, Apple Silicon and Intel, ${mb(latest.files.mac.size)}, signed and notarized by Apple.${latest.files.windows ? `<br>Windows: Windows 10 or 11 (64-bit), ${mb(latest.files.windows.size)}, signed.` : ' Windows is coming.'}</p>
</div>

<section>
  <h2>Start</h2>
  <ol class="steps">
    <li><b>Mac:</b> open the downloaded file and drag <b>OFP Studio</b> into Applications. <b>Windows:</b> run the installer.</li>
    <li>Start it. On a Mac, the first time, it asks to use Bluetooth: allow it. Bluetooth has to be on.</li>
    ${simpleStudio(latest.version) ? `<li>Wake your device (pedal, put the strap on). Find it in the list (or under <b>Don't see your device?</b>) and click <b>Connect</b>.</li>
    <li>Check your numbers: for each, <b>Looks right</b> or <b>Looks wrong</b>. Then <b>Send to OFP</b>, with a comment if something is missing. You get a code; we get the recording.</li>` : `<li>Wake your device (pedal, put the strap on). Find it in the list (or under <b>Don't see your device?</b>) and click <b>Use</b>.</li>
    <li>On <b>Live</b>, say for each value whether it is right. <b>Stop and save</b> gives you one file to send.</li>`}
  </ol>
</section>

<section class="release">
  <header><h2>What's new in ${esc(latest.version)}</h2><span class="date">${longDate(latest.date)}</span></header>
  ${notesList(notes)}
  <p><a href="/releases">Every version →</a></p>
  ${Object.entries(latest.files).map(([p, f]) => `<p class="sha">${p === 'mac' ? 'Mac' : 'Windows'} SHA-256 ${esc(f.sha256)}</p>`).join('')}
</section>
`,
  });
}

function releases(list, changelog) {
  const items = list.map((r, i) => `
<article class="release" id="v${esc(r.version)}">
  <header><h2>${esc(r.version)}</h2><span class="date">${longDate(r.date)}</span>${i === 0 ? '<span class="badge">Latest</span>' : ''}</header>
  ${notesList((changelog[r.version] || { notes: [] }).notes)}
  <p>${Object.entries(r.files).map(([p, f]) => `<a href="${esc(f.url)}">${p === 'mac' ? 'Mac' : 'Windows'}</a> <span class="small">${mb(f.size)}</span>`).join(' · ')}</p>
</article>`).join('');
  return page({
    path: '/releases', title: 'OFP Studio releases', description: 'Every release of OFP Studio, with what changed.',
    body: `<div class="hero"><h1>Releases</h1><p class="lead">Every version of OFP Studio, newest first.</p></div>${items || '<p>Nothing released yet.</p>'}`,
  });
}


const ELECTRON = `// main process (Node). npm install @openfitnessprotocol/ofp @abandonware/noble
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import noble from '@abandonware/noble'
import { Ofp, wasmEngine } from '@openfitnessprotocol/ofp'
import { advertisementFrom } from '@openfitnessprotocol/ofp/adapters/noble'

const require = createRequire(import.meta.url)
const file = (p) => readFileSync(require.resolve('@openfitnessprotocol/ofp/' + p))

const ofp = new Ofp({
  engine: wasmEngine(file('core/ofp_core.wasm')),
  bundle: file('registry.bundle.json').toString(),
  iRead: ['180d', '1818', '1816', '1826'],   // the services your app reads itself: OFP leaves them to you
  on: {
    device: (d) => addToDeviceList(d),       // a device OFP recognised: { id, name, fields }
    values: (v) => show(v.basic, v.fields),  // basic = { power, cadence, speed, heartRate }
    inUse: (id) => markConnected(id),
    lost: (id) => markDisconnected(id),
  },
})

noble.on('discover', (p) => {               // every advertisement, before your own filtering
  const a = advertisementFrom(p)
  if (a) ofp.advertisement(a)
})

// the rider picks a device: ofp.use(id) · lets it go: ofp.release(id)`;

const RN = `// React Native. npm install @openfitnessprotocol/ofp react-native-ble-manager
import { Platform } from 'react-native'
import BleManager from 'react-native-ble-manager'
import { Ofp, moduleEngine, companyIdsOf } from '@openfitnessprotocol/ofp'
import * as core from '@openfitnessprotocol/ofp/core/js'   // Hermes has no WebAssembly
import bundle from '@openfitnessprotocol/ofp/registry'
import { advertisementsFrom, scanOptionsFor } from '@openfitnessprotocol/ofp/adapters/react-native-ble-manager'

const companyIds = companyIdsOf(bundle)
const ofp = new Ofp({
  engine: moduleEngine(core),
  bundle,
  iRead: ['180d', '1818', '1816', '1826'],
  on: { device: addToDeviceList, values: (v) => show(v.basic, v.fields), inUse: markConnected, lost: markDisconnected },
})

BleManager.onDiscoverPeripheral((p) => {
  for (const a of advertisementsFrom(p, companyIds)) ofp.advertisement(a)
})

// scan with OFP's extra options so broadcast-only devices get through your service filter:
BleManager.scan({ serviceUUIDs: myServices, seconds: 30, ...scanOptionsFor({ companyIds }, ofp.scanNeed(), undefined, Platform.OS) })`;

function developers() {
  return page({
    path: '/developers', title: 'Add Open Fitness Protocol to your app',
    description: 'One JavaScript library to read fitness devices in your app: npm install @openfitnessprotocol/ofp. Electron, Node and React Native quick starts.',
    body: `
<div class="hero">
  <h1>Add OFP to your app</h1>
  <p class="lead">One library reads every checked device in the registry: today the Keiser M3i, CORE and Tymewear; heart rate straps, power meters, FTMS bikes, rowers and treadmills as their recordings come in, and the devices the community adds. You wire four hooks to your app's own Bluetooth once. New devices come with a version bump.</p>
  <pre class="code install-big"><code>npm install @openfitnessprotocol/ofp</code></pre>
  <p class="small">No dependencies · Apache-2.0 · reading only, never control · <a href="https://www.npmjs.com/package/@openfitnessprotocol/ofp">npm</a> · <a href="${REPO}">GitHub</a></p>
</div>

<section>
  <h2>The four hooks</h2>
  <div class="flow">
    <div><b>Start OFP</b><br>with the services your app already reads. OFP leaves those to you.</div>
    <div><b>Every advertisement in</b><br>from your scanner, before your own filtering.</div>
    <div><b>Devices and values out</b><br>into your device list and your screens, by field name.</div>
    <div><b>The rider's choice</b><br><code>use(id)</code> and <code>release(id)</code>.</div>
  </div>
</section>

<section>
  <h2>Electron or Node</h2>
  <pre class="code"><code>${esc(ELECTRON)}</code></pre>
</section>

<section>
  <h2>React Native</h2>
  <pre class="code"><code>${esc(RN)}</code></pre>
  <p class="small">Broadcast-only devices (like the Keiser M3i) send their data in the advertisement, so keep the scan running while one is in use: <code>ofp.scanNeed()</code> says when.</p>
</section>

<section>
  <h2>Let an AI assistant wire it</h2>
  <p>The <a href="${REPO}/blob/main/docs/integrating.md">integration guide</a> has the four hooks in detail, two complete reference integrations (Electron with noble, React Native with react-native-ble-manager), and a prompt to give your assistant. Most apps need one new file.</p>
</section>

<section>
  <h2>What you get</h2>
  <div class="cards">
    <div class="card"><h3>Values by name</h3><p><code>v.basic</code> has power, cadence, speed and heart rate for every app. <code>v.fields</code> has everything else a device sends (gear, temperature, breathing, RR intervals), each described once with its unit and range, so you can show a field you have never heard of.</p></div>
    <div class="card"><h3>New devices without code</h3><p>A device is a small data file, checked against a real recording before it ships. A new one reaches your app when you update the package.</p></div>
    <div class="card"><h3>Your app stays in charge</h3><p>OFP has no Bluetooth of its own and never writes to a trainer. It reads what your scanner hears, and leaves every service you read yourself to you.</p></div>
  </div>
</section>
`,
  });
}

export function build() {
  const changelog = readChangelog(readFileSync(join(root, 'CHANGELOG.md'), 'utf8'));
  const list = JSON.parse(readFileSync(join(here, 'releases.json'), 'utf8'))
    .sort((a, b) => b.date.localeCompare(a.date) || b.version.localeCompare(a.version, undefined, { numeric: true }));
  const latest = list[0] || null;
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  writeFileSync(join(out, 'index.html'), home(latest));
  writeFileSync(join(out, 'download.html'), download(latest, changelog));
  writeFileSync(join(out, 'releases.html'), releases(list, changelog));
  writeFileSync(join(out, 'developers.html'), developers());
  writeFileSync(join(out, '404.html'), page({ path: '/404', title: 'Not found', description: 'Not found', body: '<div class="hero"><h1>Not found</h1><p class="lead"><a href="/">Back to the start</a></p></div>' }));
  writeFileSync(join(out, 'icon.svg'), MARK.replace('<svg ', '<svg xmlns="http://www.w3.org/2000/svg" '));
  writeFileSync(join(out, 'robots.txt'), `User-agent: *\nAllow: /\nSitemap: ${SITE}/sitemap.xml\n`);
  writeFileSync(join(out, 'sitemap.xml'), `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">${['/', '/download', '/developers', '/releases'].map((p) => `<url><loc>${SITE}${p}</loc></url>`).join('')}</urlset>\n`);
  copyFileSync(join(here, 'style.css'), join(out, 'style.css'));
  mkdirSync(join(out, 'images'), { recursive: true });
  for (const f of readdirSync(join(here, 'images'))) copyFileSync(join(here, 'images', f), join(out, 'images', f));
  return { latest, versions: list.length };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const r = build();
  console.log(`site/dist built: ${r.versions} release(s)${r.latest ? `, latest ${r.latest.version}` : ''}`);
}
