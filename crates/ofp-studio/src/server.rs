//! The Studio behind HTTP, on this computer only.

use crate::studio::{Config, FieldReq, ProposeReq, Reply, Studio};
use axum::extract::State;
use axum::http::{header, StatusCode};
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
struct SessionReq {
    #[serde(default)]
    device_id: Option<String>,
    #[serde(default)]
    capture: Option<PathBuf>,
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
}

async fn events(State(app): State<App>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(app.studio.events.subscribe()).filter_map(|m| async move { m.ok().map(|s| Ok(Event::default().data(s))) });
    Sse::new(stream).keep_alive(KeepAlive::default())
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
        .route("/api/capture", get(|State(a): State<App>| async move { reply(a.studio.capture_json()) }))
        .route(
            "/api/session",
            post(|State(a): State<App>, Json(r): Json<SessionReq>| async move {
                reply(match (r.device_id, r.capture) {
                    (Some(id), _) => a.studio.open_device(&id),
                    (None, Some(path)) => a.studio.open_capture(&path),
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
        .route("/api/confirm", post(|State(a): State<App>, Json(r): Json<ConfirmReq>| async move { reply(a.studio.confirm(&r.field, r.console)) }))
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
            Ok(p) => ble::connect(p).await,
            Err(e) => Err(e),
        };
        match connection {
            Err(e) => {
                studio.set_radio(&format!("error: {e}"));
                let _ = studio.record_stop();
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            Ok(mut c) => {
                studio.on_connected(c.gatt.clone(), c.reads.clone(), c.subscribed.clone());
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
}

fn open_browser(url: &str) {
    let (cmd, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("/usr/bin/open", vec![url])
    } else if cfg!(target_os = "windows") {
        ("cmd", vec!["/C", "start", "", url])
    } else {
        ("xdg-open", vec![url])
    };
    let _ = std::process::Command::new(cmd).args(args).spawn();
}

/// Run until Ctrl-C.
pub async fn run(options: Options) -> Result<(), String> {
    let studio = Arc::new(Studio::new(options.config));
    if let Some(path) = &options.capture {
        studio.open_capture(path)?;
    }
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
    // This computer only: the page can write into the registry folder.
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", options.port)).await.map_err(|e| format!("port {}: {e} (another `ofp studio` running? try --port)", options.port))?;
    let url = format!("http://127.0.0.1:{}", options.port);
    println!("OFP Studio: {url}   (Ctrl-C to stop)");
    if options.open {
        open_browser(&url);
    }
    axum::serve(listener, router(studio, tx, has_radio))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|e| e.to_string())
}
