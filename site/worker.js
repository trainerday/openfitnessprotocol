// openfitnessprotocol.org: the pages (static assets, built by build.mjs) and the downloads (R2).
// The other names (openfitnessprotocol.com, and www. on both) are sent to the same path on the main one.

const HOME = 'openfitnessprotocol.org';

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.hostname !== HOME && !url.hostname.endsWith('.workers.dev') && url.hostname !== 'localhost' && url.hostname !== '127.0.0.1') {
      url.hostname = HOME;
      url.protocol = 'https:';
      url.port = '';
      return Response.redirect(url.toString(), 301);
    }
    if (url.pathname.startsWith('/download/') && url.pathname.length > '/download/'.length) {
      return fromBucket(request, env, url.pathname.slice(1));
    }
    return env.ASSETS.fetch(request);
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
  headers.set('cache-control', moving ? 'public, max-age=60, must-revalidate' : 'public, max-age=31536000, immutable');
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

/** The Studio, on 127.0.0.1, asks whether a newer version exists. */
function cors() {
  return { 'access-control-allow-origin': '*', 'access-control-allow-methods': 'GET, HEAD, OPTIONS' };
}
