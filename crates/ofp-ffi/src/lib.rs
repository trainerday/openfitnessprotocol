//! The host interface of OFP Core as plain C functions: JSON in, JSON out.
//!
//! The same five functions serve WebAssembly (desktop apps, browsers) and native builds (iOS,
//! Android, microcontrollers). All strings are UTF-8, passed as pointer + length.
//!
//! ```text
//! ofp_alloc(len) -> ptr                 memory for the host to write a request into
//! ofp_free(ptr, len)                    free memory from ofp_alloc, or a reply (len = 4 + reply length)
//! ofp_core_new(bundle_ptr, len) -> core 0 when the bundle is refused (see ofp_last_error)
//! ofp_core_free(core)
//! ofp_call(core, req_ptr, len) -> reply the reply: u32 length (little endian), then that many bytes of JSON
//! ofp_last_error() -> reply             why the last ofp_core_new returned 0
//! ```
//!
//! Requests: `{"op":"advertisement","device_id":"…","name":"M3","rssi":-60,"manufacturer_data":"0201…","now_ms":1234}`
//! `{"op":"tick","now_ms":1234}` (about once a second), `{"op":"configure",…}` (host settings),
//! `{"op":"use_source","source_id":"…","now_ms":…}`, `{"op":"release_source","source_id":"…"}` and
//! `{"op":"sources","now_ms":…}`. Replies: `{"messages":[…]}` or `{"error":"…"}`.

use ofp_core::{Advertisement, Core, Message};
use serde::Deserialize;
use std::cell::RefCell;

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Advertisement {
        #[serde(default)]
        device_id: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        rssi: Option<i16>,
        /// Hex, company id first.
        #[serde(default)]
        manufacturer_data: String,
        /// Service UUIDs the device advertises.
        #[serde(default)]
        services: Vec<String>,
        now_ms: u64,
    },
    /// The host connected the device and subscribed, as a `connect` message asked.
    Connected { device_id: String, now_ms: u64 },
    /// The connection ended, or could not be made.
    Disconnected { device_id: String, now_ms: u64 },
    /// One notification from a subscribed characteristic. `data` in hex.
    Notification { device_id: String, characteristic: String, data: String, now_ms: u64 },
    Tick {
        now_ms: u64,
    },
    /// Host settings, sent once at the start (any of them may be left out).
    Configure {
        /// Wait at least this long before a source counts as silent.
        #[serde(default)]
        min_stale_ms: Option<u64>,
        /// Repeat a source in use's standing values when nothing went out for this long.
        #[serde(default)]
        repeat_ms: Option<u64>,
        /// A source in use that is silent this long is lost.
        #[serde(default)]
        lost_after_ms: Option<u64>,
        /// Sources the rider used before: used again as soon as they are heard.
        #[serde(default)]
        remembered: Option<Vec<String>>,
        /// Bluetooth services the app reads by itself.
        #[serde(default)]
        handled_services: Option<Vec<String>>,
    },
    /// The rider chose this source.
    UseSource { source_id: String, now_ms: u64 },
    /// The rider stopped using it.
    ReleaseSource { source_id: String },
    /// Sources in range (and in use), for a device list that just opened.
    Sources {
        now_ms: u64,
        #[serde(default = "default_heard_within")]
        heard_within_ms: u64,
    },
}

fn default_heard_within() -> u64 {
    30_000
}

pub fn unhex(s: &str) -> Result<Vec<u8>, String> {
    let s: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if s.len() % 2 != 0 {
        return Err("hex with an odd number of digits".into());
    }
    s.chunks(2)
        .map(|c| {
            let h = (c[0] as char).to_digit(16).ok_or("not hex")?;
            let l = (c[1] as char).to_digit(16).ok_or("not hex")?;
            Ok((h * 16 + l) as u8)
        })
        .collect()
}

/// The JSON interface as a Rust function (used by the C functions below and by tests).
pub fn call(core: &mut Core, request: &str) -> Result<Vec<Message>, String> {
    let req: Request = serde_json::from_str(request).map_err(|e| format!("request: {e}"))?;
    Ok(match req {
        Request::Advertisement { device_id, name, rssi, manufacturer_data, services, now_ms } => {
            let data = unhex(&manufacturer_data)?;
            core.on_advertisement(&Advertisement { device_id: &device_id, name: name.as_deref(), rssi, manufacturer_data: &data, services: &services, now_ms })
        }
        Request::Connected { device_id, now_ms } => {
            core.on_connected(&device_id, now_ms);
            Vec::new()
        }
        Request::Disconnected { device_id, now_ms } => core.on_disconnected(&device_id, now_ms),
        Request::Notification { device_id, characteristic, data, now_ms } => core.on_notification(&device_id, &characteristic, &unhex(&data)?, now_ms),
        Request::Tick { now_ms } => core.tick(now_ms),
        Request::Configure { min_stale_ms, repeat_ms, lost_after_ms, remembered, handled_services } => {
            let settings = core.settings_mut();
            if let Some(ms) = min_stale_ms {
                settings.min_stale_ms = ms;
            }
            if let Some(ms) = repeat_ms {
                settings.repeat_ms = ms;
            }
            if let Some(ms) = lost_after_ms {
                settings.lost_after_ms = ms;
            }
            if let Some(r) = remembered {
                settings.remembered = r;
            }
            if let Some(h) = handled_services {
                settings.handled_services = Some(h);
            }
            Vec::new()
        }
        Request::UseSource { source_id, now_ms } => core.use_source(&source_id, now_ms)?,
        Request::ReleaseSource { source_id } => core.release_source(&source_id),
        Request::Sources { now_ms, heard_within_ms } => core.sources(now_ms, heard_within_ms),
    })
}

fn reply(json: String) -> *mut u8 {
    let bytes = json.into_bytes();
    let mut buf = Vec::with_capacity(4 + bytes.len());
    buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(&bytes);
    let mut boxed = buf.into_boxed_slice();
    let ptr = boxed.as_mut_ptr();
    std::mem::forget(boxed);
    ptr
}

fn error_reply(msg: &str) -> *mut u8 {
    reply(serde_json::json!({ "error": msg }).to_string())
}

thread_local! {
    static LAST_ERROR: RefCell<String> = const { RefCell::new(String::new()) };
}

#[no_mangle]
pub extern "C" fn ofp_alloc(len: usize) -> *mut u8 {
    let mut boxed = vec![0u8; len].into_boxed_slice();
    let ptr = boxed.as_mut_ptr();
    std::mem::forget(boxed);
    ptr
}

/// # Safety
/// `ptr` and `len` must be exactly what `ofp_alloc` gave, or a reply pointer with `4 + reply length`.
#[no_mangle]
pub unsafe extern "C" fn ofp_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)));
    }
}

/// # Safety
/// `ptr` must point to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn ofp_core_new(ptr: *const u8, len: usize) -> *mut Core {
    let bytes = std::slice::from_raw_parts(ptr, len);
    let made = std::str::from_utf8(bytes).map_err(|e| e.to_string()).and_then(Core::new);
    match made {
        Ok(core) => Box::into_raw(Box::new(core)),
        Err(e) => {
            LAST_ERROR.with(|l| *l.borrow_mut() = e);
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// `core` must come from `ofp_core_new` and not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn ofp_core_free(core: *mut Core) {
    if !core.is_null() {
        drop(Box::from_raw(core));
    }
}

/// # Safety
/// `core` must come from `ofp_core_new`; `ptr` must point to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn ofp_call(core: *mut Core, ptr: *const u8, len: usize) -> *mut u8 {
    let Some(core) = core.as_mut() else { return error_reply("no core") };
    let bytes = std::slice::from_raw_parts(ptr, len);
    let Ok(request) = std::str::from_utf8(bytes) else { return error_reply("request is not UTF-8") };
    match call(core, request) {
        Ok(messages) => reply(serde_json::json!({ "messages": messages }).to_string()),
        Err(e) => error_reply(&e),
    }
}

#[no_mangle]
pub extern "C" fn ofp_last_error() -> *mut u8 {
    reply(LAST_ERROR.with(|l| l.borrow().clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUNDLE: &str = r#"{"version":1,
      "fields":[{"id":1,"key":"power","name":"Power","unit":"W","type":"int","min":0,"max":3000,"kind":"instantaneous","tier":"experimental"}],
      "tables":[{"id":"bike","name":"Bike","reader":"broadcast","version":1,
        "match":[{"manufacturer_id":258,"min_length":4}],
        "instance":{"label":"bike","value":{"u8":1}},
        "stale_after_ms":3000,
        "fields":[{"field":"power","value":{"u16le":2}}]}]}"#;

    fn read_reply(ptr: *mut u8) -> String {
        unsafe {
            let len = u32::from_le_bytes(std::slice::from_raw_parts(ptr, 4).try_into().unwrap()) as usize;
            let s = String::from_utf8(std::slice::from_raw_parts(ptr.add(4), len).to_vec()).unwrap();
            ofp_free(ptr, 4 + len);
            s
        }
    }

    fn send(core: *mut Core, req: &str) -> serde_json::Value {
        unsafe {
            let p = ofp_alloc(req.len());
            std::ptr::copy_nonoverlapping(req.as_ptr(), p, req.len());
            let reply = read_reply(ofp_call(core, p, req.len()));
            ofp_free(p, req.len());
            serde_json::from_str(&reply).unwrap()
        }
    }

    #[test]
    fn the_c_interface_end_to_end() {
        unsafe {
            let core = ofp_core_new(BUNDLE.as_ptr(), BUNDLE.len());
            assert!(!core.is_null());
            let r = send(core, r#"{"op":"advertisement","device_id":"x","name":"M3","rssi":-60,"manufacturer_data":"0201 00 07 bb00","now_ms":10}"#);
            assert_eq!(r["messages"][0]["type"], "source_appeared");
            assert_eq!(r["messages"][0]["source_id"], "bike:7");
            assert_eq!(r["messages"][1]["fields"]["power"], 187);
            assert_eq!(send(core, r#"{"op":"configure","min_stale_ms":5000}"#)["messages"].as_array().unwrap().len(), 0);
            assert_eq!(send(core, r#"{"op":"tick","now_ms":4000}"#)["messages"].as_array().unwrap().len(), 0);
            assert_eq!(send(core, r#"{"op":"tick","now_ms":5010}"#)["messages"][1]["type"], "source_stale");
            assert!(send(core, r#"{"op":"nope"}"#)["error"].is_string());
            // the rider's choice, and the device list
            assert_eq!(send(core, r#"{"op":"use_source","source_id":"bike:7","now_ms":6000}"#)["messages"][0]["type"], "source_in_use");
            assert!(send(core, r#"{"op":"use_source","source_id":"bike:99","now_ms":6000}"#)["error"].is_string());
            assert_eq!(send(core, r#"{"op":"sources","now_ms":6000}"#)["messages"][0]["source_id"], "bike:7");
            assert_eq!(send(core, r#"{"op":"configure","repeat_ms":1000,"lost_after_ms":60000,"remembered":["bike:7"],"handled_services":["180d"]}"#)["messages"].as_array().unwrap().len(), 0);
            send(core, r#"{"op":"release_source","source_id":"bike:7"}"#);
            assert_eq!(send(core, r#"{"op":"sources","now_ms":90000}"#)["messages"].as_array().unwrap().len(), 0);
            assert!(send(core, r#"{"op":"advertisement","manufacturer_data":"zz","now_ms":1}"#)["error"].is_string());
            ofp_core_free(core);

            let bad = "{}";
            assert!(ofp_core_new(bad.as_ptr(), bad.len()).is_null());
            assert!(read_reply(ofp_last_error()).contains("bundle"));
        }
    }
}
