//! The Studio behind HTTP, on this computer only.

use crate::studio::{Config, FieldReq, ProposeReq, Reply, Studio};
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::stream::{Stream, StreamExt};
use serde::Deserialize;
use serde_json::json;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::BroadcastStream;

const INDEX: &str = include_str!("../web/index.html");
const CSS: &str = include_str!("../web/studio.css");
const JS: &str = include_str!("../web/studio.js");
const TESTER_JS: &str = include_str!("../web/tester.js");
/// The rules for adding to the registry, as an LLM or a person should read them.
const GUIDE: &str = include_str!("../../../AGENTS.md");

/// How long a connection may take before the radio gives up and goes back to scanning.
#[cfg(feature = "ble")]
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

pub enum Radio {
    Connect(String),
    Disconnect,
    /// characteristic, bytes, with a response (None = whatever the characteristic supports)
    Write(String, Vec<u8>, Option<bool>),
}

#[derive(Clone)]
struct App {
    studio: Arc<Studio>,
    radio: mpsc::UnboundedSender<Radio>,
    has_radio: bool,
}

fn reply(r: Reply) -> Response {
    match r {
        Ok(v) => Json(v).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    }
}

#[derive(Deserialize)]
struct NameReq {
    name: String,
}

#[derive(Deserialize)]
struct SessionReq {
    #[serde(default)]
    device_id: Option<String>,
    /// A recording's file name, as `/api/captures` lists it (never a path)
    #[serde(default)]
    capture: Option<String>,
    /// Where it is: `captures` (the default) or `registry`
    #[serde(default)]
    place: Option<String>,
}

#[derive(Deserialize, Default)]
struct RecordReq {
    #[serde(default)]
    connect: Option<bool>,
}

#[derive(Deserialize)]
struct TextReq {
    text: String,
}

#[derive(Deserialize)]
struct SeriesReq {
    #[serde(default)]
    stream: Option<String>,
    expr: String,
}

#[derive(Deserialize, Default)]
struct StreamReq {
    #[serde(default)]
    stream: Option<String>,
}

#[derive(Deserialize)]
struct YamlReq {
    yaml: String,
    #[serde(default)]
    overwrite: bool,
}

#[derive(Deserialize)]
struct UploadReq {
    name: String,
    text: String,
}

#[derive(Deserialize)]
struct WriteReq {
    chr: String,
    hex: String,
    #[serde(default)]
    response: Option<bool>,
}

#[derive(Deserialize)]
struct ConfirmReq {
    field: String,
    #[serde(default)]
    console: Option<f64>,
    /// "No, this is wrong" (with or without what the display showed)
    #[serde(default)]
    wrong: bool,
}

async fn events(State(app): State<App>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // A page that fell behind loses events (the channel drops the oldest): tell it to read the state again
    let stream = BroadcastStream::new(app.studio.events.subscribe()).map(|m| {
        Ok(match m {
            Ok(s) => Event::default().data(s),
            Err(_lagged) => Event::default().data(r#"{"type":"reload"}"#),
        })
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Is this host name (from a `Host` header, with or without a port) this computer?
fn local_host(host: &str) -> bool {
    let name = match host.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or(""),
        None => host.rsplit_once(':').map_or(host, |(name, _)| name),
    };
    matches!(name.to_ascii_lowercase().as_str(), "127.0.0.1" | "localhost" | "::1")
}

/// Only the Studio's own page may use the API. Binding to 127.0.0.1 is not enough on its own: a web page
/// whose host name is re-pointed at 127.0.0.1 (DNS rebinding) would be same-origin with the Studio, and any
/// page can send a plain POST with no body. So the `Host` must name this computer, and a request that
/// changes something must not come from another page (`Origin`, which browsers send on every cross-site POST).
fn request_is_ours(method: &Method, headers: &HeaderMap) -> bool {
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else { return false };
    if !local_host(host) {
        return false;
    }
    if method == Method::GET || method == Method::HEAD {
        return true;
    }
    match headers.get(header::ORIGIN) {
        None => true,
        Some(origin) => origin.to_str().is_ok_and(|o| o.eq_ignore_ascii_case(&format!("http://{host}"))),
    }
}

async fn only_ours(req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    if !request_is_ours(req.method(), req.headers()) {
        return (StatusCode::FORBIDDEN, Json(json!({ "error": "Only the Studio's own page can use this address." }))).into_response();
    }
    next.run(req).await
}

fn text(kind: &'static str, body: &'static str) -> Response {
    ([(header::CONTENT_TYPE, kind), (header::CACHE_CONTROL, "no-store")], body).into_response()
}

pub fn router(studio: Arc<Studio>, radio: mpsc::UnboundedSender<Radio>, has_radio: bool) -> Router {
    let app = App { studio, radio, has_radio };
    Router::new()
        .route("/", get(|| async { text("text/html; charset=utf-8", INDEX) }))
        .route("/studio.css", get(|| async { text("text/css; charset=utf-8", CSS) }))
        .route("/studio.js", get(|| async { text("text/javascript; charset=utf-8", JS) }))
        .route("/tester.js", get(|| async { text("text/javascript; charset=utf-8", TESTER_JS) }))
        .route("/api/guide", get(|| async { text("text/markdown; charset=utf-8", GUIDE) }))
        .route("/api/events", get(events))
        .route("/api/state", get(|State(a): State<App>| async move { Json(a.studio.state_json()) }))
        .route("/api/devices", get(|State(a): State<App>| async move { Json(a.studio.devices_json()) }))
        .route("/api/captures", get(|State(a): State<App>| async move { Json(a.studio.captures_json()) }))
        .route(
            "/api/captures/download",
            get(|State(a): State<App>, Query(q): Query<NameReq>| async move {
                match a.studio.saved_recording(&q.name) {
                    Ok(text) => ([(header::CONTENT_TYPE, "application/x-ndjson".to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"", q.name))], text).into_response(),
                    Err(e) => reply(Err(e)),
                }
            }),
        )
        .route(
            "/api/capture/file",
            get(|State(a): State<App>| async move {
                match a.studio.capture_file() {
                    Ok((name, text)) => ([(header::CONTENT_TYPE, "application/x-ndjson".to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\""))], text).into_response(),
                    Err(e) => reply(Err(e)),
                }
            }),
        )
        .route("/api/capture/upload", post(|State(a): State<App>, Json(r): Json<UploadReq>| async move { reply(a.studio.open_uploaded(&r.name, &r.text)) }))
        .route("/api/capture", get(|State(a): State<App>| async move { reply(a.studio.capture_json()) }))
        .route(
            "/api/session",
            post(|State(a): State<App>, Json(r): Json<SessionReq>| async move {
                // Choosing a device (or a file) ends whatever was being recorded
                let _ = a.radio.send(Radio::Disconnect);
                let _ = a.studio.record_stop();
                reply(match (r.device_id, r.capture) {
                    (Some(id), _) => a.studio.open_device(&id),
                    (None, Some(name)) => a.studio.open_saved(&name, r.place.as_deref().unwrap_or("captures")),
                    (None, None) => a.studio.close(),
                })
            }),
        )
        .route(
            "/api/record/start",
            post(|State(a): State<App>, body: Option<Json<RecordReq>>| async move {
                if !a.has_radio {
                    return reply(Err("This build of ofp has no Bluetooth. Open a capture file instead.".into()));
                }
                reply(a.studio.record_start(body.and_then(|b| b.0.connect)).map(|(v, connect_to)| {
                    if let Some(id) = connect_to {
                        let _ = a.radio.send(Radio::Connect(id));
                    }
                    v
                }))
            }),
        )
        .route(
            "/api/record/stop",
            post(|State(a): State<App>| async move {
                let _ = a.radio.send(Radio::Disconnect);
                reply(a.studio.record_stop())
            }),
        )
        .route("/api/note", post(|State(a): State<App>, Json(r): Json<TextReq>| async move { reply(a.studio.note(&r.text)) }))
        .route("/api/series", post(|State(a): State<App>, Json(r): Json<SeriesReq>| async move { reply(a.studio.series(r.stream.as_deref(), &r.expr)) }))
        .route("/api/solve", post(|State(a): State<App>, body: Option<Json<StreamReq>>| async move { reply(a.studio.solve(body.and_then(|b| b.0.stream).as_deref())) }))
        .route("/api/similar", get(|State(a): State<App>| async move { reply(a.studio.similar()) }))
        .route("/api/table/propose", post(|State(a): State<App>, Json(r): Json<ProposeReq>| async move { reply(a.studio.propose(r)) }))
        .route("/api/table/design", post(|State(a): State<App>, Json(r): Json<ProposeReq>| async move { reply(a.studio.set_design(r)) }))
        .route("/api/table/check", post(|State(a): State<App>, Json(r): Json<YamlReq>| async move { reply(a.studio.check_table(&r.yaml)) }))
        .route("/api/field", post(|State(a): State<App>, Json(r): Json<FieldReq>| async move { reply(a.studio.add_field(r)) }))
        .route("/api/confirm", post(|State(a): State<App>, Json(r): Json<ConfirmReq>| async move { reply(a.studio.confirm(&r.field, r.console, r.wrong)) }))
        .route(
            "/api/write",
            post(|State(a): State<App>, Json(r): Json<WriteReq>| async move {
                reply(a.studio.can_write().and_then(|_| ofp_capture::unhex(&r.hex)).map(|bytes| {
                    let _ = a.radio.send(Radio::Write(r.chr, bytes, r.response));
                    json!({ "sent": true })
                }))
            }),
        )
        .route("/api/save", post(|State(a): State<App>, Json(r): Json<YamlReq>| async move { reply(a.studio.save(&r.yaml, r.overwrite)) }))
        .layer(axum::extract::DefaultBodyLimit::max(64 * 1024 * 1024))
        .layer(axum::middleware::from_fn(only_ours))
        .with_state(app)
}

/// Hears advertisements for the device list and the recording; connects when a recording needs it.
#[cfg(feature = "ble")]
async fn radio_task(studio: Arc<Studio>, mut commands: mpsc::UnboundedReceiver<Radio>) {
    use crate::studio::AdvIn;
    use ofp_capture::ble::{self, Scanner};
    loop {
        let mut scanner = match Scanner::start().await {
            Ok(s) => s,
            Err(e) => {
                studio.set_radio(&format!("error: {e}"));
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };
        studio.set_radio("scanning");
        let connect_to = loop {
            tokio::select! {
                adv = scanner.next() => match adv {
                    Some(a) => studio.on_adv(AdvIn { device_id: a.device_id, name: a.name, rssi: a.rssi, mfr: a.mfr, service_data: a.service_data, services: a.services }),
                    None => break None,
                },
                cmd = commands.recv() => match cmd {
                    Some(Radio::Connect(id)) => break Some(id),
                    Some(Radio::Disconnect) => {}
                    Some(Radio::Write(chr, bytes, _)) => studio.on_write(&chr, &bytes, Some("not connected".into())),
                    None => return,
                },
            }
        };
        let Some(device_id) = connect_to else {
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        };
        // A connection: the scan stops, the device's notifications are the recording.
        studio.set_radio("connecting");
        let peripheral = scanner.peripheral(&device_id).await;
        scanner.stop().await;
        let connection = match peripheral {
            // A device that never answers must not hold the radio: give up after a while, or when the
            // person stops or chooses something else meanwhile (and leave the device disconnected).
            Ok(p) => {
                let attempt = ble::connect(p.clone());
                tokio::pin!(attempt);
                let deadline = tokio::time::sleep(CONNECT_TIMEOUT);
                tokio::pin!(deadline);
                let outcome = loop {
                    tokio::select! {
                        c = &mut attempt => break c,
                        _ = &mut deadline => break Err(format!("the device did not answer in {} s", CONNECT_TIMEOUT.as_secs())),
                        cmd = commands.recv() => match cmd {
                            Some(Radio::Disconnect) | None => break Err("stopped".into()),
                            Some(Radio::Connect(_)) => {}
                            Some(Radio::Write(chr, bytes, _)) => studio.on_write(&chr, &bytes, Some("not connected yet".into())),
                        },
                    }
                };
                if outcome.is_err() {
                    ble::disconnect(&p).await;
                }
                outcome
            }
            Err(e) => Err(e),
        };
        match connection {
            Err(e) => {
                studio.set_radio(&format!("error: {e}"));
                let _ = studio.record_stop();
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            Ok(c) if !studio.on_connected(&device_id, c.gatt.clone(), c.reads.clone(), c.subscribed.clone()) => {
                // the person moved on while it was connecting
                ble::disconnect(&c.peripheral).await;
            }
            Ok(mut c) => {
                studio.set_radio("connected");
                loop {
                    tokio::select! {
                        n = c.notifications.next() => match n {
                            Some(n) => studio.on_notify(&ble::short_uuid(&n.uuid), &n.value),
                            None => {
                                studio.set_radio("error: the device disconnected");
                                let _ = studio.record_stop();
                                break;
                            }
                        },
                        cmd = commands.recv() => match cmd {
                            Some(Radio::Disconnect) | None => break,
                            Some(Radio::Connect(_)) => {}
                            Some(Radio::Write(chr, bytes, response)) => {
                                let result = ble::write(&c.peripheral, &chr, &bytes, response).await;
                                studio.on_write(&chr, &bytes, result.err());
                            }
                        },
                        _ = tokio::time::sleep(Duration::from_millis(500)) => {
                            if !studio.is_recording_connected() {
                                break;
                            }
                        }
                    }
                }
                ble::disconnect(&c.peripheral).await;
            }
        }
    }
}

pub struct Options {
    pub config: Config,
    pub port: u16,
    pub open: bool,
    pub capture: Option<PathBuf>,
    /// When `port` is taken (another Studio is running), use any free port instead of failing.
    /// The app with its own window does; the `ofp studio` command does not, so its address stays the same.
    pub any_port: bool,
}

/// Is this an address a browser should open (`http` or `https`, nothing a shell could read as more)?
fn web_address(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://")) && !url.chars().any(|c| c.is_control() || c == '"')
}

/// Open an address in the computer's default browser. Only `http` and `https` addresses: anything else
/// (`file:`, an app's own scheme) is not handed to the system.
pub fn open_browser(url: &str) {
    if !web_address(url) {
        return;
    }
    let (cmd, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("/usr/bin/open", vec![url])
    } else if cfg!(target_os = "windows") {
        // not `cmd /C start`: cmd reads `&` in an address as the start of another command
        ("rundll32", vec!["url.dll,FileProtocolHandler", url])
    } else {
        ("xdg-open", vec![url])
    };
    let _ = std::process::Command::new(cmd).args(args).spawn();
}

/// A Studio that is being served. Dropping it does not stop it: call [`Running::stop`].
pub struct Running {
    /// Where the page is, e.g. `http://127.0.0.1:4747`
    pub url: String,
    pub port: u16,
    stop: tokio::sync::oneshot::Sender<()>,
    served: tokio::task::JoinHandle<Result<(), String>>,
}

impl Running {
    /// Stop serving and wait until the server has ended.
    pub async fn stop(self) -> Result<(), String> {
        let _ = self.stop.send(());
        self.served.await.map_err(|e| e.to_string())?
    }

    /// Wait until the server ends by itself (it does not, short of an error).
    pub async fn wait(self) -> Result<(), String> {
        let _keep = self.stop;
        self.served.await.map_err(|e| e.to_string())?
    }
}

/// Start the Studio: the radio, the clock and the page on 127.0.0.1. Must be called inside a tokio
/// runtime; returns as soon as the page can be opened. Whoever calls decides what shows the page (the
/// `ofp studio` command opens the browser, the app opens its own window) and when it stops.
pub async fn start(options: Options) -> Result<Running, String> {
    let studio = Arc::new(Studio::new(options.config));
    if let Some(path) = &options.capture {
        studio.open_capture(path)?;
    }
    // This computer only: the page can write into the registry folder.
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", options.port)).await {
        Ok(l) => l,
        Err(_) if options.any_port => tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.map_err(|e| format!("no free port: {e}"))?,
        Err(e) => return Err(format!("port {}: {e} (another `ofp studio` running? try --port)", options.port)),
    };
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let (tx, rx) = mpsc::unbounded_channel();
    let has_radio = cfg!(feature = "ble");
    #[cfg(feature = "ble")]
    tokio::spawn(radio_task(studio.clone(), rx));
    #[cfg(not(feature = "ble"))]
    drop(rx);
    {
        let studio = studio.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(Duration::from_secs(1));
            loop {
                every.tick().await;
                studio.tick();
            }
        });
    }
    {
        // a device whose table writes fixed start and poll frames: sent through the radio as they come due
        let studio = studio.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(Duration::from_millis(200));
            loop {
                every.tick().await;
                for (chr, bytes, with_response) in studio.poll_tick() {
                    let _ = tx.send(Radio::Write(chr, bytes, Some(with_response)));
                }
            }
        });
    }
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let app = router(studio, tx, has_radio);
    let served = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .map_err(|e| e.to_string())
    });
    Ok(Running { url: format!("http://127.0.0.1:{port}"), port, stop, served })
}

/// The `ofp studio` command: start, open the browser if asked, run until Ctrl-C.
pub async fn run(options: Options) -> Result<(), String> {
    let open = options.open;
    let running = start(options).await?;
    println!("OFP Studio: {}   (Ctrl-C to stop)", running.url);
    if open {
        open_browser(&running.url);
    }
    let _ = tokio::signal::ctrl_c().await;
    running.stop().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn get(port: u16, path: &str) -> String {
        raw(port, &format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"))
    }

    fn raw(port: u16, request: &str) -> String {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(request.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    }

    fn options(dir: &std::path::Path, port: u16, any_port: bool) -> Options {
        let registry = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../registry");
        Options { config: Config { registry, captures: dir.join("captures") }, port, open: false, capture: None, any_port }
    }

    #[test]
    fn only_web_addresses_are_opened() {
        assert!(web_address("https://github.com/trainerday/openfitnessprotocol/issues/new?title=a&body=b"));
        assert!(web_address("http://127.0.0.1:4747"));
        for u in ["file:///etc/passwd", "javascript:alert(1)", "ms-settings:", "vscode://x", "https://x\n", ""] {
            assert!(!web_address(u), "{u}");
        }
    }

    #[test]
    fn host_names_of_this_computer() {
        for h in ["127.0.0.1", "127.0.0.1:4747", "localhost:4747", "LOCALHOST", "[::1]:4747"] {
            assert!(local_host(h), "{h}");
        }
        for h in ["evil.example", "evil.example:4747", "localhost.evil.example:4747", "127.0.0.1.evil.example", "[::2]:4747", ""] {
            assert!(!local_host(h), "{h}");
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn only_the_studios_own_page_can_use_the_api() {
        let dir = std::env::temp_dir().join(format!("ofp-studio-origin-{}", std::process::id()));
        let running = start(options(&dir, 0, false)).await.unwrap();
        let port = running.port;
        let answers = tokio::task::spawn_blocking(move || {
            let post = |host: &str, origin: Option<&str>| {
                let origin = origin.map(|o| format!("Origin: {o}\r\n")).unwrap_or_default();
                raw(port, &format!("POST /api/record/stop HTTP/1.1\r\nHost: {host}\r\n{origin}Content-Length: 0\r\nConnection: close\r\n\r\n"))
            };
            let ours = format!("127.0.0.1:{port}");
            vec![
                // a re-pointed host name (DNS rebinding): refused, reads included
                ("rebound GET", raw(port, &format!("GET /api/state HTTP/1.1\r\nHost: evil.example:{port}\r\nConnection: close\r\n\r\n")), "403"),
                ("rebound POST", post(&format!("evil.example:{port}"), None), "403"),
                // another page posting to the Studio: refused
                ("cross-site POST", post(&ours, Some("http://evil.example")), "403"),
                ("null origin POST", post(&ours, Some("null")), "403"),
                ("other local port", post(&ours, Some("http://127.0.0.1:1")), "403"),
                // the Studio's own page, and tools such as curl (no Origin)
                // (stop answers 400 "nothing is being recorded": the request reached it)
                ("own page", post(&ours, Some(&format!("http://{ours}"))), "400"),
                ("no origin", post(&ours, None), "400"),
                ("own GET", get(port, "/api/state"), "200"),
            ]
        })
        .await
        .unwrap();
        for (what, answer, status) in answers {
            assert!(answer.starts_with(&format!("HTTP/1.1 {status}")), "{what}: {answer}");
        }
        running.stop().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn started_by_a_caller_it_serves_the_page_and_stops_when_told() {
        let dir = std::env::temp_dir().join(format!("ofp-studio-start-{}", std::process::id()));
        let running = start(options(&dir, 0, false)).await.unwrap();
        let port = running.port;
        assert_eq!(running.url, format!("http://127.0.0.1:{port}"));
        let page = tokio::task::spawn_blocking(move || (get(port, "/"), get(port, "/api/state"))).await.unwrap();
        assert!(page.0.starts_with("HTTP/1.1 200"), "{}", page.0);
        assert!(page.0.contains("<html"));
        assert!(page.1.starts_with("HTTP/1.1 200"), "{}", page.1);

        // The port is taken now: the command's way fails with its message, the app's way moves on
        let taken = start(options(&dir, port, false)).await.err().unwrap();
        assert!(taken.contains("another `ofp studio` running"), "{taken}");
        let second = start(options(&dir, port, true)).await.unwrap();
        assert_ne!(second.port, port);
        second.stop().await.unwrap();

        running.stop().await.unwrap();
        assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err(), "the port is free again");
    }
}
