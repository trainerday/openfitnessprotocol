// Builds the site's pages into site/dist/ from CHANGELOG.md (the notes) and site/releases.json (what was
// published: version, date, file, size, sha256). No dependencies: `node site/build.mjs`.
import { readFileSync, writeFileSync, mkdirSync, rmSync, copyFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..');
const out = join(here, 'dist');
const SITE = 'https://openfitnessprotocol.com';
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
  const nav = [['/download', 'Download'], ['/releases', 'Releases'], [REPO, 'GitHub']]
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
  const download = latest
    ? `<a class="button" href="/download">Download OFP Studio</a><span class="small">Version ${esc(latest.version)} · ${latest.files.windows ? 'Mac and Windows' : 'Mac'} · free</span>`
    : `<a class="button quiet" href="${REPO}">See the project on GitHub</a>`;
  return page({
    path: '/', title: 'Open Fitness Protocol',
    description: 'One open way for any fitness device to talk to any fitness app. A device is described by a small data file, and every app that uses OFP can read it.',
    body: `
<div class="hero">
  <span class="status">First version · everything can still change until 1.0</span>
  <h1>One open way for any fitness device to talk to any fitness app.</h1>
  <p class="lead">A device is described by a <b>table</b>: a small data file that says which bytes mean what. Every app that uses OFP can read every device that has one. No special code for each device in each app.</p>
  <div class="actions">${download}</div>
</div>

<section>
  <h2>Why</h2>
  <p>Heart rate, power and smart trainers have Bluetooth standards. New sensors (breathing, core temperature, bikes that only broadcast their numbers) do not, so each maker invents a private format and each app has to write code for each device. Most of that code never gets written. OFP puts one open piece in the middle, and the rules for it live in a public git repository, not in a committee.</p>
</section>

<section>
  <h2>Take part</h2>
  <div class="cards">
    <div class="card"><h3>Have a device nothing reads?</h3><p>Install OFP Studio, wake the device and click Use. You see what it sends, and its values once a table reads it. Say for each value whether it is right.</p><p><a href="/download">Download OFP Studio →</a></p></div>
    <div class="card"><h3>Add a device</h3><p>Write its table from a recording, alone or with an AI assistant working beside you in the Studio. A pull request with the recording, and the automated check is the judge.</p><p><a href="${REPO}/blob/main/AGENTS.md">How a device is added →</a></p></div>
    <div class="card"><h3>Building an app?</h3><p>Add the library and wire up four hooks. Every device in the registry then works in your app, and new ones arrive as data, without a new release.</p><p><a href="${REPO}/blob/main/docs/integrating.md">Add OFP to your app →</a></p></div>
  </div>
</section>

<section>
  <h2>How a device gets in</h2>
  <div class="flow">
    <div><b>Record</b><br>what the device sends, in the Studio.</div>
    <div><b>Write the table</b><br>which bytes mean which field.</div>
    <div><b>Confirm</b><br>the owner marks each value right or wrong.</div>
    <div><b>Pull request</b><br>the check replays the recording through the table.</div>
    <div><b>Works everywhere</b><br>in every app that uses OFP.</div>
  </div>
</section>

<section>
  <h2>Devices so far</h2>
  <p>The <b>Keiser M3i</b> bike, checked on a real ride. First drafts for the <b>CORE</b> body temperature sensor and the <b>Tymewear VitalPro</b> breathing sensor, waiting for owners to try them.</p>
</section>
`,
  });
}

function notesList(notes) {
  return notes.length ? `<ul>${notes.map((n) => `<li>${inline(n)}</li>`).join('')}</ul>` : '';
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
  <p class="small">Version ${esc(latest.version)} · ${longDate(latest.date)}</p>
  <p class="small">Mac: macOS 11 or later, Apple Silicon and Intel, ${mb(latest.files.mac.size)}, signed and notarized by Apple.${latest.files.windows ? `<br>Windows: Windows 10 or 11 (64-bit), ${mb(latest.files.windows.size)}, signed.` : ' Windows is coming.'}</p>
</div>

<section>
  <h2>Start</h2>
  <ol class="steps">
    <li><b>Mac:</b> open the downloaded file and drag <b>OFP Studio</b> into Applications. <b>Windows:</b> run the installer.</li>
    <li>Start it. On a Mac, the first time, it asks to use Bluetooth: allow it. Bluetooth has to be on.</li>
    <li>Wake your device (pedal, put the strap on). Find it in the list (or under <b>Don't see your device?</b>) and click <b>Use</b>.</li>
    <li>On <b>Live</b>, say for each value whether it is right. <b>Stop and save</b> gives you one file to send.</li>
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
  writeFileSync(join(out, '404.html'), page({ path: '/404', title: 'Not found', description: 'Not found', body: '<div class="hero"><h1>Not found</h1><p class="lead"><a href="/">Back to the start</a></p></div>' }));
  writeFileSync(join(out, 'icon.svg'), MARK.replace('<svg ', '<svg xmlns="http://www.w3.org/2000/svg" '));
  writeFileSync(join(out, 'robots.txt'), `User-agent: *\nAllow: /\nSitemap: ${SITE}/sitemap.xml\n`);
  writeFileSync(join(out, 'sitemap.xml'), `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">${['/', '/download', '/releases'].map((p) => `<url><loc>${SITE}${p}</loc></url>`).join('')}</urlset>\n`);
  copyFileSync(join(here, 'style.css'), join(out, 'style.css'));
  return { latest, versions: list.length };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const r = build();
  console.log(`site/dist built: ${r.versions} release(s)${r.latest ? `, latest ${r.latest.version}` : ''}`);
}
