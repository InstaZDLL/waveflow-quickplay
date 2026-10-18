//! WaveFlow QuickPlay — a standalone, lightweight audio player.
//!
//! ```text
//! waveflow-quickplay [FILE|FOLDER]...
//! ```
//!
//! Files given on the command line are queued and the first one plays.
//! `RUST_LOG` controls the log level (default `info`).

#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod app;
mod audio;
mod capture;
mod config;
mod integration;
mod ipc;
mod library;
mod lyrics;
mod platform;
mod queue;
mod ui;

use std::path::PathBuf;

use tracing_subscriber::EnvFilter;

use crate::app::{QuickPlayApp, APP_ID};
use crate::config::Settings;
use crate::ui::COMPACT_HEIGHT;

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("info,lofty=error,symphonia=warn,eframe=warn,egui_glow=warn")
    });
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .try_init();
}

fn app_icon() -> Option<egui::IconData> {
    let png: &[u8] = include_bytes!("../assets/icon-128.png");
    eframe::icon_data::from_png_bytes(png)
        .map_err(|err| tracing::warn!(%err, "app icon not loaded"))
        .ok()
}

fn main() -> eframe::Result {
    init_tracing();
    let started = std::time::Instant::now();

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| {
        matches!(
            a.as_str(),
            "--version" | "-V" | "--help" | "-h" | "--register" | "--unregister"
        )
    }) {
        platform::attach_parent_console();
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("waveflow-quickplay {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("usage: waveflow-quickplay [--enqueue] [FILE|FOLDER]...");
        println!();
        println!("A second launch hands its files to the running player and exits.");
        println!("  --enqueue      append to its queue instead of replacing it");
        println!(
            "  --register     add \"{}\" / \"{}\" to the file manager (current user)",
            integration::PLAY_LABEL,
            integration::ENQUEUE_LABEL
        );
        println!("  --unregister   remove them");
        return Ok(());
    }
    for (flag, run) in [
        (
            "--register",
            integration::register as fn() -> std::io::Result<String>,
        ),
        ("--unregister", integration::unregister),
    ] {
        if args.iter().any(|a| a == flag) {
            return match run() {
                Ok(done) => {
                    println!("{done}");
                    Ok(())
                }
                Err(e) => {
                    eprintln!("{flag} failed: {e}");
                    std::process::exit(1);
                }
            };
        }
    }
    let mode = if args.iter().any(|a| a == "--enqueue") {
        ipc::Mode::Enqueue
    } else {
        ipc::Mode::Play
    };
    let files: Vec<PathBuf> = args
        .iter()
        .filter(|a| !a.starts_with("--"))
        .map(PathBuf::from)
        .collect();

    // Single instance: hand the files to a running player if there is one;
    // otherwise become the one that listens. Losing the creation race means
    // another launch is starting right now, so hand over to it instead.
    match ipc::send(mode, &files) {
        Ok(true) => {
            tracing::info!(files = files.len(), "handed to the running instance");
            return Ok(());
        }
        Ok(false) => {}
        Err(e) => tracing::warn!(error = %e, "single-instance check failed"),
    }
    let server = match ipc::Server::start() {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            let mut handed = false;
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if ipc::send(mode, &files).unwrap_or(false) {
                    handed = true;
                    break;
                }
            }
            if handed {
                tracing::info!("handed to the instance that won the start race");
                return Ok(());
            }
            tracing::warn!(
                "another instance holds the socket but does not answer; running standalone"
            );
            None
        }
        Err(e) => {
            tracing::warn!(error = %e, "single-instance socket unavailable; running standalone");
            None
        }
    };

    let settings = Settings::load();
    let (inner, max): ([f32; 2], [f32; 2]) = if settings.expanded {
        (settings.expanded_size, [8192.0, 8192.0])
    } else {
        (
            [settings.compact_width, COMPACT_HEIGHT],
            [8192.0, COMPACT_HEIGHT],
        )
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(app::MAIN_TITLE)
        .with_app_id(APP_ID)
        .with_inner_size(inner)
        .with_min_inner_size([config::MIN_WIDTH, COMPACT_HEIGHT])
        .with_max_inner_size(max)
        .with_drag_and_drop(true)
        .with_decorations(false)
        .with_window_level(if settings.always_on_top {
            egui::WindowLevel::AlwaysOnTop
        } else {
            egui::WindowLevel::Normal
        });
    if let Some(icon) = app_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        centered: false,
        ..Default::default()
    };

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        files = files.len(),
        "starting WaveFlow QuickPlay"
    );
    eframe::run_native(
        "WaveFlow QuickPlay",
        options,
        Box::new(move |cc| {
            let app = QuickPlayApp::new(cc, settings, files, server);
            tracing::info!(elapsed_ms = started.elapsed().as_millis(), "window ready");
            Ok(Box::new(app))
        }),
    )
}
