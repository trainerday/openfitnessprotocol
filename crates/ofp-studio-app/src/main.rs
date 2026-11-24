//! OFP Studio as an app with its own window.
//!
//! It is the Studio that `ofp studio` serves (the same page, the same API, the same radio), started inside
//! this process and shown in a window of its own. Nothing here knows about devices or tables.
//!
//! Where it keeps things:
//!  - started with `--registry <dir> --captures <dir>` (what `npm run app` does from the repo): those folders,
//!    so Save writes into the repo's registry, ready for a pull request;
//!  - otherwise `Documents/OFP Studio/registry` and `…/captures`. The registry the app was built with is
//!    copied in at the start, file by file, and never over a file that is already there.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use ofp_studio::server::{self, Options, Running};
use ofp_studio::Config;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

const PORT: u16 = 4747;

/// `--name value` from the command line.
fn arg(name: &str) -> Option<PathBuf> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == name {
            return args.next().map(PathBuf::from);
        }
    }
    None
}

/// Copy the files of `from` that `to` does not have yet. A file the owner saved or changed stays.
fn copy_missing(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_missing(&entry.path(), &target)?;
        } else if !target.exists() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// The folders the Studio works in (see the top of this file).
fn folders(app: &tauri::App) -> Result<Config, String> {
    if let (Some(registry), Some(captures)) = (arg("--registry"), arg("--captures")) {
        return Ok(Config { registry, captures });
    }
    let home = app.path().document_dir().map_err(|e| format!("no Documents folder: {e}"))?.join("OFP Studio");
    let registry = arg("--registry").unwrap_or_else(|| home.join("registry"));
    let captures = arg("--captures").unwrap_or_else(|| home.join("captures"));
    if arg("--registry").is_none() {
        let built_in = app.path().resource_dir().map_err(|e| e.to_string())?.join("registry");
        copy_missing(&built_in, &registry).map_err(|e| format!("{}: {e}", registry.display()))?;
    }
    std::fs::create_dir_all(&captures).map_err(|e| format!("{}: {e}", captures.display()))?;
    Ok(Config { registry, captures })
}

fn start(app: &tauri::App) -> Result<Running, String> {
    let config = folders(app)?;
    // Refuse to start on a registry that does not load, and say which file is wrong
    ofp_registry::registry::load(&config.registry).map_err(|e| format!("The registry in {} does not load:\n{}", config.registry.display(), e.join("\n")))?;
    let options = Options { config, port: PORT, open: false, capture: arg("--capture"), any_port: true };
    tauri::async_runtime::block_on(server::start(options))
}

/// A page that only says what went wrong (there is no Studio to show it).
fn error_page(message: &str) -> String {
    let escaped = message.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let html = format!(
        "<body style=\"font:15px/1.5 -apple-system,Segoe UI,sans-serif;margin:48px;color:#1a202c\"><h2>OFP Studio could not start</h2><pre style=\"white-space:pre-wrap\">{escaped}</pre></body>"
    );
    let mut url = String::from("data:text/html;charset=utf-8,");
    for b in html.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            url.push(b as char);
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
    url
}

fn main() {
    let app = tauri::Builder::default()
        .manage(Mutex::new(None::<Running>))
        .setup(|app| {
            let url = match start(app) {
                Ok(running) => {
                    let url = running.url.clone();
                    *app.state::<Mutex<Option<Running>>>().lock().unwrap() = Some(running);
                    url
                }
                Err(e) => {
                    eprintln!("OFP Studio: {e}");
                    error_page(&e)
                }
            };
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url.parse()?))
                .title(format!("OFP Studio {}", env!("CARGO_PKG_VERSION")))
                .inner_size(1280.0, 860.0)
                .min_inner_size(900.0, 600.0)
                // The window is the Studio only: a link to anywhere else opens in the browser
                .on_download(|webview, event| {
                    // a recording the person downloads: into their Downloads folder, under its own name
                    if let tauri::webview::DownloadEvent::Requested { url, destination } = event {
                        // only the last part of the name: `?name=../../x` or an absolute path stays in Downloads
                        let name = url
                            .query_pairs()
                            .find(|(k, _)| k == "name")
                            .and_then(|(_, v)| Path::new(v.as_ref()).file_name().map(|n| n.to_owned()))
                            .unwrap_or_else(|| "recording.capture.jsonl".into());
                        if let Ok(dir) = webview.path().download_dir() {
                            *destination = dir.join(name);
                        }
                    }
                    true
                })
                .on_navigation(|to| {
                    let ours = matches!(to.scheme(), "data" | "about") || to.host_str() == Some("127.0.0.1");
                    if !ours {
                        server::open_browser(to.as_str());
                    }
                    ours
                })
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("OFP Studio could not start");
    app.run(|handle, event| {
        // The window was closed (the app ends with it): stop the radio and the server first
        if let RunEvent::Exit = event {
            let running = handle.state::<Mutex<Option<Running>>>().lock().unwrap().take();
            if let Some(running) = running {
                let _ = tauri::async_runtime::block_on(running.stop());
            }
        }
    });
}
