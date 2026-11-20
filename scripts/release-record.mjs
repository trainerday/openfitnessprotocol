// Records a published version: its entry in site/releases.json (committed: the version history the site
// is built from) and release/latest.json (what a running Studio checks). Called by scripts/release.sh publish.
//   node scripts/release-record.mjs <version> <date> <release dir> <site url>
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { readChangelog } from '../site/build.mjs';

const [version, date, dir, site] = process.argv.slice(2);
const platforms = {
  mac: { file: `OFP-Studio-${version}-mac.dmg`, key: `download/mac/OFP-Studio-${version}-mac.dmg` },
  windows: { file: `OFP-Studio-${version}-windows-setup.exe`, key: `download/windows/OFP-Studio-${version}-windows-setup.exe` },
};
const files = {};
for (const [name, p] of Object.entries(platforms)) {
  const path = join(dir, p.file);
  if (!existsSync(path)) continue;
  const bytes = readFileSync(path);
  files[name] = { url: `${site}/${p.key}`, size: statSync(path).size, sha256: createHash('sha256').update(bytes).digest('hex') };
}
if (!files.mac) throw new Error(`${dir}/${platforms.mac.file} is missing`);

const listPath = 'site/releases.json';
const list = JSON.parse(readFileSync(listPath, 'utf8')).filter((r) => r.version !== version);
list.push({ version, date, files });
list.sort((a, b) => b.version.localeCompare(a.version, undefined, { numeric: true }));
writeFileSync(listPath, JSON.stringify(list, null, 2) + '\n');

const notes = (readChangelog(readFileSync('CHANGELOG.md', 'utf8'))[version] || { notes: [] }).notes;
writeFileSync(join(dir, 'latest.json'), JSON.stringify({ version, date, page: `${site}/download`, notes, files }, null, 2) + '\n');
console.log(`  recorded ${version} (${Object.keys(files).join(', ')})`);
