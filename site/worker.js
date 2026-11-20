// openfitnessprotocol.org: the pages (static assets, built by build.mjs) and the downloads (R2).
// The other names (openfitnessprotocol.com, and www. on both) are sent to the same path on the main one.

const HOME = 'openfitnessprotocol.org';
/** Caching is off while the site is being built: every page, download and JSON is fetched fresh.
 *  Set to false to cache again (versioned downloads for a year, the rest for a minute). */
const NO_CACHE = true;

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.hostname !== HOME && !url.hostname.endsWith('.workers.dev') && url.hostname !== 'localhost' && url.hostname !== '127.0.0.1') {
      url.hostname = HOME;
      url.protocol = 'https:';
      url.port = '';
      return Response.redirect(url.toString(), 301);
    }
    if (url.pathname === '/api/submit') return submit(request, env);
    if (url.pathname.startsWith('/download/') && url.pathname.length > '/download/'.length) {
      return fromBucket(request, env, url.pathname.slice(1));
    }
    const page = await env.ASSETS.fetch(request);
    if (!NO_CACHE) return page;
    const fresh = new Response(page.body, page);
    fresh.headers.set('cache-control', 'no-store');
    fresh.headers.delete('etag');
    return fresh;
  },
};

/** A download: every version is kept under its own name and never changes; the "latest" names and the
 *  JSON move with each release, so they are cached for a minute only. */
async function fromBucket(request, env, key) {
  if (request.method === 'OPTIONS') return new Response(null, { status: 204, headers: cors() });
  if (request.method !== 'GET' && request.method !== 'HEAD') return new Response('Method not allowed', { status: 405 });
  const object = request.method === 'HEAD'
    ? await env.RELEASES.head(key)
    : await env.RELEASES.get(key, { range: request.headers, onlyIf: request.headers });
  if (!object) return new Response('Not found', { status: 404 });

  const headers = new Headers();
  object.writeHttpMetadata(headers);
  headers.set('etag', object.httpEtag);
  headers.set('accept-ranges', 'bytes');
  const moving = key.endsWith('.json') || key.includes('-latest-');
  headers.set('cache-control', NO_CACHE ? 'no-store' : moving ? 'public, max-age=60, must-revalidate' : 'public, max-age=31536000, immutable');
  if (key.endsWith('.json')) for (const [k, v] of Object.entries(cors())) headers.set(k, v);
  if (key.endsWith('.dmg')) headers.set('content-disposition', `attachment; filename="${key.split('/').pop()}"`);

  if (request.method === 'HEAD') {
    headers.set('content-length', String(object.size));
    return new Response(null, { headers });
  }
  // `onlyIf` not met: the object has no body
  if (!('body' in object) || !object.body) return new Response(null, { status: 304, headers });
  if (object.range && request.headers.has('range')) {
    const { offset = 0, length = object.size - offset } = object.range;
    headers.set('content-range', `bytes ${offset}-${offset + length - 1}/${object.size}`);
    return new Response(object.body, { status: 206, headers });
  }
  return new Response(object.body, { headers });
}

// ───────────── a recording someone sends from OFP Studio ─────────────

/** At most this much per submission (a long ride at a few packets a second is well under it). */
const MAX_BYTES = 5 * 1024 * 1024;
/** Letters that cannot be misread for one another when someone types a code. */
const CODE_LETTERS = 'ABCDEFGHJKMNPQRSTUVWXYZ23456789';

/** A code like OFP-7K3Q-2M: what the person keeps, and what a maintainer looks the recording up by. */
function newCode() {
  const r = crypto.getRandomValues(new Uint8Array(6));
  const c = [...r].map((b) => CODE_LETTERS[b % CODE_LETTERS.length]).join('');
  return `OFP-${c.slice(0, 4)}-${c.slice(4)}`;
}

function reply(status, body) {
  return new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json', 'cache-control': 'no-store', ...cors('POST') } });
}

/** Is this an OFP recording? The first line is its header; every line is one JSON object. */
function isRecording(text) {
  const lines = text.split('\n').filter((l) => l.trim());
  if (!lines.length) return false;
  try {
    const head = JSON.parse(lines[0]);
    if (head.type !== 'header' || head.format !== 'ofp-capture') return false;
    for (const l of lines) {
      const o = JSON.parse(l);
      if (!o || typeof o !== 'object' || Array.isArray(o)) return false;
    }
    return true;
  } catch {
    return false;
  }
}

/** POST /api/submit: kept privately (bucket ofp-submissions, never served), read by the maintainers,
 *  deleted after 90 days by the bucket's own rule. */
async function submit(request, env) {
  if (request.method === 'OPTIONS') return new Response(null, { status: 204, headers: cors('POST') });
  if (request.method !== 'POST') return reply(405, { error: 'Send with POST.' });
  const ip = request.headers.get('cf-connecting-ip') || 'unknown';
  const { success } = await env.SUBMIT_LIMIT.limit({ key: ip });
  if (!success) return reply(429, { error: 'Too many in a short time. Wait a minute and send again.' });
  const length = Number(request.headers.get('content-length') || 0);
  if (length > MAX_BYTES) return reply(413, { error: 'This recording is too large to send (more than 5 MB).' });
  const raw = await request.text();
  if (raw.length > MAX_BYTES) return reply(413, { error: 'This recording is too large to send (more than 5 MB).' });
  let body;
  try {
    body = JSON.parse(raw);
  } catch {
    return reply(400, { error: 'Not a submission.' });
  }
  const text = typeof body.recording === 'string' ? body.recording : '';
  if (!isRecording(text)) return reply(400, { error: 'That is not an OFP recording.' });
  const short = (v, n) => (typeof v === 'string' ? v.slice(0, n) : null);
  const code = newCode();
  const now = new Date();
  const day = now.toISOString().slice(0, 10);
  const key = `${day.slice(0, 4)}/${day.slice(5)}/${code}`;
  const meta = {
    code,
    received: now.toISOString(),
    comment: short(body.comment, 4000),
    email: short(body.email, 200),
    studio_version: short(body.version, 40),
    device: short(body.device, 200),
    read_as: short(body.read_as, 200),
    country: request.cf && request.cf.country,
  };
  await env.SUBMISSIONS.put(`${key}.capture.jsonl`, text, { httpMetadata: { contentType: 'application/x-ndjson' } });
  await env.SUBMISSIONS.put(`${key}.json`, JSON.stringify(meta, null, 2), { httpMetadata: { contentType: 'application/json' } });
  return reply(200, { code });
}

/** The Studio, on 127.0.0.1, asks whether a newer version exists. */
function cors(also) {
  return {
    'access-control-allow-origin': '*',
    'access-control-allow-methods': also ? `${also}, OPTIONS` : 'GET, HEAD, OPTIONS',
    'access-control-allow-headers': 'content-type',
  };
}
