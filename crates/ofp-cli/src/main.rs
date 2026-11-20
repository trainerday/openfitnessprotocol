//! The `ofp` command.

mod check;
mod exprparse;
mod inspect;
#[cfg(feature = "ble")]
mod live;
mod registry;
mod replay;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "ofp", version, about = "Open Fitness Protocol: check the registry, decode packets, record and watch devices")]
struct Cli {
    /// The registry folder (fields/ and tables/).
    #[arg(long, global = true, default_value = "registry")]
    registry: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Validate fields and tables, replay every capture and vector. What a pull request must pass.
    Check {
        /// Maintainers: freeze fields newly marked stable into fields.lock.json.
        #[arg(long)]
        accept_stable: bool,
    },
    /// Compile the registry into the bundle the core loads.
    Compile {
        #[arg(short, long, default_value = "dist/registry.bundle.json")]
        output: PathBuf,
    },
    /// Decode one packet: manufacturer data in hex, company id first.
    Decode {
        hex: String,
        /// The device's advertised name, if the table needs it.
        #[arg(long)]
        name: Option<String>,
    },
    /// Summarise a capture: rates, gaps, which bytes change, the notes. Needs no table.
    Inspect { file: PathBuf },
    /// Run a capture through the registry and print what it decodes.
    Replay {
        file: PathBuf,
        /// Print every decoded packet, not only the summary.
        #[arg(long)]
        all: bool,
    },
    /// List the Bluetooth devices in range with their advert rate.
    #[cfg(feature = "ble")]
    Scan {
        #[arg(long, default_value_t = 15)]
        seconds: u64,
    },
    /// Record what one device sends into a capture file, with your notes.
    #[cfg(feature = "ble")]
    Record {
        /// The device's advertised name (exact).
        device: String,
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Connect and record every characteristic that notifies (for devices that are not broadcast-only).
        #[arg(long)]
        connect: bool,
        /// Stop after this many seconds (default: until you quit).
        #[arg(long)]
        seconds: Option<u64>,
    },
    /// Show a device's decoded values live and confirm each one against its own display.
    #[cfg(feature = "ble")]
    Live {
        /// Only this source (for example keiser-m3i:197). Default: the first one heard.
        #[arg(long)]
        source: Option<String>,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long)]
        seconds: Option<u64>,
        /// One line per packet instead of the full-screen display.
        #[arg(long)]
        plain: bool,
    },
}

fn load(dir: &std::path::Path) -> Result<registry::Registry, String> {
    registry::load(dir).map_err(|errors| errors.join("\n"))
}

fn run(cli: Cli) -> Result<bool, String> {
    match cli.command {
        Command::Check { accept_stable } => {
            let report = check::run(&cli.registry, accept_stable);
            for l in &report.lines {
                println!("{l}");
            }
            for e in &report.errors {
                eprintln!("error: {e}");
            }
            if report.errors.is_empty() {
                println!("ok");
            } else {
                eprintln!("{} problem(s)", report.errors.len());
            }
            Ok(report.errors.is_empty())
        }
        Command::Compile { output } => {
            let reg = load(&cli.registry)?;
            if let Some(parent) = output.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(&output, reg.bundle_json() + "\n").map_err(|e| e.to_string())?;
            println!("{} ({} fields, {} tables)", output.display(), reg.bundle.fields.len(), reg.bundle.tables.len());
            Ok(true)
        }
        Command::Decode { hex, name } => {
            let reg = load(&cli.registry)?;
            let data = ofp_capture::unhex(&hex)?;
            let mut core = reg.core();
            let messages = core.on_advertisement(&ofp_core::Advertisement { device_id: "decode", name: name.as_deref(), rssi: None, manufacturer_data: &data, now_ms: 0 });
            if messages.is_empty() {
                println!("no table decodes this packet");
                return Ok(false);
            }
            for m in messages {
                println!("{}", serde_json::to_string_pretty(&m).map_err(|e| e.to_string())?);
            }
            Ok(true)
        }
        Command::Inspect { file } => {
            print!("{}", inspect::inspect(&ofp_capture::read_file(&file)?));
            Ok(true)
        }
        Command::Replay { file, all } => {
            let reg = load(&cli.registry)?;
            if all {
                let lines = ofp_capture::read_file(&file)?;
                let c = replay::replay(&mut reg.core(), &lines)?;
                for d in &c.timeline {
                    println!("{}", serde_json::json!({ "t": d.t, "source_id": d.source_id, "stale": d.stale, "fields": d.fields }));
                }
            }
            let r = check::check_capture(&reg, None, &file);
            println!("{}", r.summary);
            for e in &r.errors {
                eprintln!("error: {e}");
            }
            Ok(r.errors.is_empty())
        }
        #[cfg(feature = "ble")]
        Command::Scan { seconds } => live::scan(seconds).map(|_| true),
        #[cfg(feature = "ble")]
        Command::Record { device, output, connect, seconds } => live::record(&device, output, connect, seconds).map(|_| true),
        #[cfg(feature = "ble")]
        Command::Live { source, output, seconds, plain } => {
            let reg = load(&cli.registry)?;
            live::live(reg, source, output, seconds, plain).map(|_| true)
        }
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
