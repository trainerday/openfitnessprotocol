// Open Fitness Protocol for JavaScript apps.
//
// OFP has no Bluetooth of its own. An app wires up four things:
//   1. new Ofp({ engine, bundle, iRead: [...services the app reads itself] })
//   2. ofp.advertisement(...)      for every advertisement its scanner hears, unfiltered
//   3. on.device / on.values       devices OFP recognises, and their readings
//   4. ofp.use(id) / ofp.release(id) when the rider picks one or lets it go
//
// The engine is the OFP core: WebAssembly where the runtime has it (`wasmEngine`), or the same core
// translated to plain JavaScript where it does not, such as React Native's Hermes (`moduleEngine`).
// No dependencies, no Node or browser globals beyond typed arrays.

function utf8Encode(text) {
  const out = [];
  for (let i = 0; i < text.length; i++) {
    let c = text.charCodeAt(i);
    if (c >= 0xd800 && c <= 0xdbff && i + 1 < text.length) {
      const d = text.charCodeAt(i + 1);
      if (d >= 0xdc00 && d <= 0xdfff) { c = 0x10000 + ((c - 0xd800) << 10) + (d - 0xdc00); i++; }
    }
    if (c < 0x80) out.push(c);
    else if (c < 0x800) out.push(0xc0 | (c >> 6), 0x80 | (c & 63));
    else if (c < 0x10000) out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
    else out.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
  }
  return out;
}

function utf8Decode(bytes) {
  let s = '';
  for (let i = 0; i < bytes.length; ) {
    const b = bytes[i++];
    let c;
    if (b < 0x80) c = b;
    else if (b < 0xe0) c = ((b & 31) << 6) | (bytes[i++] & 63);
    else if (b < 0xf0) { c = ((b & 15) << 12) | ((bytes[i] & 63) << 6) | (bytes[i + 1] & 63); i += 2; }
    else { c = ((b & 7) << 18) | ((bytes[i] & 63) << 12) | ((bytes[i + 1] & 63) << 6) | (bytes[i + 2] & 63); i += 3; }
    if (c >= 0x10000) { c -= 0x10000; s += String.fromCharCode(0xd800 + (c >> 10), 0xdc00 + (c & 1023)); } else s += String.fromCharCode(c);
  }
  return s;
}

const HEX = [];
for (let i = 0; i < 256; i++) HEX.push((i < 16 ? '0' : '') + i.toString(16));

/** Bytes (Uint8Array, Node Buffer, or number[]) as lower-case hex. */
export function toHex(bytes) {
  let s = '';
  for (let i = 0; i < bytes.length; i++) s += HEX[bytes[i] & 0xff];
  return s;
}

/** Base64 (what React Native BLE libraries hand over) as hex. */
export function base64ToHex(b64) {
  const A = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  let s = '', buf = 0, bits = 0;
  for (let i = 0; i < b64.length; i++) {
    const v = A.indexOf(b64[i]);
    if (v < 0) continue;
    buf = (buf << 6) | v;
    bits += 6;
    if (bits >= 8) { bits -= 8; s += HEX[(buf >> bits) & 0xff]; }
  }
  return s;
}

/**
 * The core behind its five C functions. `exports` is what a WebAssembly instance exports, or the
 * module `wasm2js` makes from the same file: both look the same from here.
 */
export function moduleEngine(exports) {
  const x = exports;
  // The memory can grow (and its buffer be replaced) on any call: take the view each time.
  const mem = () => new Uint8Array(x.memory.buffer);
  const put = (text) => {
    const bytes = utf8Encode(text);
    const ptr = x.ofp_alloc(bytes.length);
    mem().set(bytes, ptr);
    return [ptr, bytes.length];
  };
  const take = (ptr) => {
    const m = mem();
    const len = (m[ptr] | (m[ptr + 1] << 8) | (m[ptr + 2] << 16) | (m[ptr + 3] << 24)) >>> 0;
    const text = utf8Decode(m.subarray(ptr + 4, ptr + 4 + len));
    x.ofp_free(ptr, 4 + len);
    return text;
  };
  return {
    /** A core loaded with a registry bundle (JSON text). Throws when the bundle is refused. */
    create(bundleJson) {
      const [bp, bl] = put(bundleJson);
      let core = x.ofp_core_new(bp, bl);
      x.ofp_free(bp, bl);
      if (!core) throw new Error('OFP bundle refused: ' + take(x.ofp_last_error()));
      return {
        call(request) {
          if (!core) return [];
          const [p, l] = put(JSON.stringify(request));
          const reply = JSON.parse(take(x.ofp_call(core, p, l)));
          x.ofp_free(p, l);
          if (reply.error) throw new Error('OFP: ' + reply.error);
          return reply.messages || [];
        },
        destroy() {
          if (core) x.ofp_core_free(core);
          core = 0;
        },
      };
    },
  };
}

/** The core from its WebAssembly bytes (Node, Electron, browsers). */
export function wasmEngine(wasmBytes) {
  return moduleEngine(new WebAssembly.Instance(new WebAssembly.Module(wasmBytes), {}).exports);
}

/** The four values every training app has a place for, from a values message's fields. */
export function basic(fields) {
  return {
    power: fields.power ?? 0,
    cadence: fields.cadence ?? 0,
    speed: fields.speed ?? 0,
    heartRate: fields.heart_rate ?? 0,
  };
}

export class Ofp {
  /**
   * @param {object} o
   * @param o.engine        from wasmEngine() or moduleEngine()
   * @param {string} o.bundle  the registry bundle (JSON text)
   * @param {string[]} [o.iRead]  Bluetooth services the app reads by itself (short UUIDs, e.g. '180d')
   * @param {string[]} [o.remembered]  sources the rider used before: used again as soon as heard
   * @param {object} [o.on]  { device, values, inUse, lost, silent, error }
   * @param {number} [o.minStaleMs]  wait this long before a source counts as silent (default 10 s:
   *   a phone's or computer's radio misses adverts for seconds at a time)
   * @param {number} [o.repeatMs]  repeat a source's standing values this often while it is in use
   *   (default 1 s, for apps that treat "no event" as "no data"); 0 = never
   * @param {number} [o.lostAfterMs]  give up a silent source after this long (default 60 s)
   * @param {number} [o.tickMs]  the timer's period (default 1 s); 0 = the app calls tick() itself
   * @param {() => number} [o.now]
   */
  constructor(o) {
    this.on = o.on || {};
    this.now = o.now || Date.now;
    this.failed = false;
    this.described = new Map();   // source id → its description
    this.inUse = new Set();
    this.core = o.engine.create(o.bundle);
    this.core.call({
      op: 'configure',
      min_stale_ms: o.minStaleMs ?? 10000,
      repeat_ms: o.repeatMs ?? 1000,
      lost_after_ms: o.lostAfterMs ?? 60000,
      remembered: o.remembered || [],
      ...(o.iRead ? { handled_services: o.iRead } : {}),
    });
    const period = o.tickMs ?? 1000;
    this.timer = period > 0 ? setInterval(() => this.tick(), period) : null;
    if (this.timer && typeof this.timer.unref === 'function') this.timer.unref();
  }

  /** One failure turns OFP off: it must never take the app's Bluetooth down with it. */
  _guard(fn) {
    if (this.failed || !this.core) return;
    try {
      this._deliver(fn());
    } catch (e) {
      this.failed = true;
      if (this.timer) clearInterval(this.timer);
      for (const id of [...this.inUse]) { this.inUse.delete(id); this.on.lost && this.on.lost(id); }
      this.on.error && this.on.error(e);
    }
  }

  _deliver(messages) {
    for (const m of messages) {
      switch (m.type) {
        case 'source_appeared': {
          const d = { id: m.source_id, name: m.name, table: m.table, tableVersion: m.table_version, instance: m.instance, instanceLabel: m.instance_label, fields: m.fields };
          this.described.set(d.id, d);
          this.on.device && this.on.device(d);
          break;
        }
        case 'values':
          this.on.values && this.on.values({ id: m.source_id, t: m.t, fields: m.fields, basic: basic(m.fields), silent: !!m.stale, repeat: !!m.repeat, rssi: m.rssi, outOfRange: m.out_of_range || [] });
          break;
        case 'source_stale':
          this.on.silent && this.on.silent(m.source_id);
          break;
        case 'source_in_use':
          this.inUse.add(m.source_id);
          this.on.inUse && this.on.inUse(m.source_id, !!m.auto, this.described.get(m.source_id));
          break;
        case 'source_lost':
          this.inUse.delete(m.source_id);
          this.on.lost && this.on.lost(m.source_id);
          break;
      }
    }
  }

  /**
   * Hook 2: every advertisement the app's scanner hears, before the app's own filtering.
   * `manufacturerData` is the raw manufacturer data with the 2-byte company id first, as bytes
   * (Uint8Array / Buffer) or already as hex.
   */
  advertisement(a) {
    const data = a.manufacturerData;
    if (!data || data.length < 2) return;
    this._guard(() => this.core.call({
      op: 'advertisement',
      device_id: a.deviceId || '',
      name: a.name || null,
      rssi: typeof a.rssi === 'number' ? Math.round(a.rssi) : null,
      manufacturer_data: typeof data === 'string' ? data : toHex(data),
      now_ms: Math.round(this.now()),
    }));
  }

  /** Hook 4: the rider picked this device. False when it has not been heard. */
  use(id) {
    if (this.failed || !this.core) return false;
    try {
      this._deliver(this.core.call({ op: 'use_source', source_id: id, now_ms: Math.round(this.now()) }));
      return this.inUse.has(id);
    } catch {
      return false;
    }
  }

  /** Hook 4: the rider let it go. The app also drops it from what it passes as `remembered`. */
  release(id) {
    this.inUse.delete(id);
    this._guard(() => this.core.call({ op: 'release_source', source_id: id }));
  }

  isInUse(id) {
    return this.inUse.has(id);
  }

  /** Ids of the devices in use. */
  used() {
    return [...this.inUse];
  }

  /** The devices in range (and in use): for a device list that just opened. */
  devices(heardWithinMs = 30000) {
    if (this.failed || !this.core) return [];
    try {
      return this.core.call({ op: 'sources', now_ms: Math.round(this.now()), heard_within_ms: heardWithinMs })
        .map((m) => ({ id: m.source_id, name: m.name, table: m.table, tableVersion: m.table_version, instance: m.instance, instanceLabel: m.instance_label, fields: m.fields }));
    } catch {
      return [];
    }
  }

  /** What `source_appeared` said about a device, if it was heard. */
  describe(id) {
    return this.described.get(id);
  }

  tick() {
    this._guard(() => this.core.call({ op: 'tick', now_ms: Math.round(this.now()) }));
  }

  destroy() {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    try { this.core && this.core.destroy(); } catch { /* already gone */ }
    this.core = null;
  }
}
