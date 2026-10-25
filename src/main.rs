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
mod i18n;
mod integration;
mod ipc;
mod library;
mod lyrics;
#[cfg(target_os = "macos")]
mod macos;
mod platform;
mod queue;
mod ui;
mod waveflow;

use std::ffi::OsString;
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

/// What the command line asks for.
#[derive(Debug, Default)]
struct Args {
    version: bool,
    help: bool,
    register: bool,
    unregister: bool,
    enqueue: bool,
    /// Files and folders, kept as the OS gave them: a file name need not
    /// be valid Unicode.
    files: Vec<PathBuf>,
    /// Options this version does not know; logged and otherwise ignored.
    unknown: Vec<String>,
}

/// Options are read up to `--`, after which everything is a file, so a
/// file named `--help` can still be opened. Anything else that starts with
/// `--` is an unknown option, not a file.
fn parse_args(args: impl IntoIterator<Item = OsString>) -> Args {
    let mut parsed = Args::default();
    let mut options_done = false;
    for arg in args {
        if options_done {
            parsed.files.push(PathBuf::from(arg));
            continue;
        }
        match arg.to_str() {
            Some("--") => options_done = true,
            Some("--version" | "-V") => parsed.version = true,
            Some("--help" | "-h") => parsed.help = true,
            Some("--register") => parsed.register = true,
            Some("--unregister") => parsed.unregister = true,
            Some("--enqueue") => parsed.enqueue = true,
            Some(other) if other.starts_with("--") => parsed.unknown.push(other.to_string()),
            _ => parsed.files.push(PathBuf::from(arg)),
        }
    }
    parsed
}

fn main() -> eframe::Result {
    init_tracing();
    let started = std::time::Instant::now();

    let args = parse_args(std::env::args_os().skip(1));
    if args.version || args.help || args.register || args.unregister {
        platform::attach_parent_console();
    }
    for option in &args.unknown {
        tracing::warn!(%option, "unknown option ignored");
    }
    if args.version {
        println!("waveflow-quickplay {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    // The interface language also names the file-manager verbs.
    let lang = || i18n::Lang::resolve(&Settings::load().language);
    if args.help {
        let t = lang().strings();
        println!("usage: waveflow-quickplay [--enqueue] [--] [FILE|FOLDER]...");
        println!();
        println!("A second launch hands its files to the running player and exits.");
        println!("  --enqueue      append to its queue instead of replacing it");
        println!(
            "  --register     add \"{}\" / \"{}\" to the file manager (current user)",
            t.action_play_with, t.action_add_to_playlist
        );
        println!("  --unregister   remove them");
        return Ok(());
    }
    if args.register || args.unregister {
        let flag = if args.register {
            "--register"
        } else {
            "--unregister"
        };
        let run = if args.register {
            integration::register(lang())
        } else {
            integration::unregister()
        };
        return match run {
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
    let mode = if args.enqueue {
        ipc::Mode::Enqueue
    } else {
        ipc::Mode::Play
    };
    let files = args.files;

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
                match ipc::send(mode, &files) {
                    Ok(true) => {
                        handed = true;
                        break;
                    }
                    Ok(false) => {}
                    // Paths the socket cannot carry, or an instance that
                    // does not answer: waiting longer does not help.
                    Err(e) => {
                        tracing::warn!(error = %e, "hand-off failed");
                        break;
                    }
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
    // On macOS the event loop is built by `macos::run_native`, which also
    // receives the files Finder opens.
    #[cfg(target_os = "macos")]
    let run_native = macos::run_native;
    #[cfg(not(target_os = "macos"))]
    let run_native = eframe::run_native;
    run_native(
        "WaveFlow QuickPlay",
        options,
        Box::new(move |cc| {
            let app = QuickPlayApp::new(cc, settings, files, server);
            tracing::info!(elapsed_ms = started.elapsed().as_millis(), "window ready");
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Args {
        parse_args(args.iter().map(OsString::from))
    }

    #[test]
    fn options_end_at_a_double_dash() {
        let a = parse(&["--enqueue", "a.flac", "--", "--help", "-V"]);
        assert!(a.enqueue && !a.help && !a.version);
        assert_eq!(
            a.files,
            [PathBuf::from("a.flac"), "--help".into(), "-V".into()]
        );
    }

    #[test]
    fn an_unknown_option_is_not_a_file() {
        let a = parse(&["--shuffle", "-x.mp3"]);
        assert_eq!(a.unknown, ["--shuffle"]);
        assert_eq!(a.files, [PathBuf::from("-x.mp3")]);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_name_need_not_be_unicode() {
        use std::os::unix::ffi::OsStringExt;
        let name = OsString::from_vec(b"caf\xe9.flac".to_vec());
        let a = parse_args([name.clone()]);
        assert_eq!(a.files, [PathBuf::from(name)]);
    }
}
