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
mod restart;
mod ui;
mod waveflow;

use std::cell::Cell;
use std::ffi::OsString;
use std::path::PathBuf;
use std::rc::Rc;

use tracing_subscriber::EnvFilter;

use crate::app::{QuickPlayApp, APP_ID};
use crate::config::Settings;
use crate::ui::COMPACT_HEIGHT;
// On macOS the event loop is built by `macos::run_native`, which also
// receives the files Finder opens.
#[cfg(not(target_os = "macos"))]
use eframe::run_native;
#[cfg(target_os = "macos")]
use macos::run_native;

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
    /// Internal, when QuickPlay restarts itself for the pin
    /// (`src/restart.rs`): where the previous one was, and its process,
    /// which this one waits out.
    restart_state: Option<PathBuf>,
    after_pid: Option<u32>,
    /// Internal: XWayland failed to make the window, stay on Wayland.
    no_xwayland: bool,
}

/// Options are read up to `--`, after which everything is a file, so a
/// file named `--help` can still be opened. Anything else that starts with
/// `--` is an unknown option, not a file.
fn parse_args(args: impl IntoIterator<Item = OsString>) -> Args {
    let mut parsed = Args::default();
    let mut options_done = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if options_done {
            parsed.files.push(PathBuf::from(arg));
            continue;
        }
        match arg.to_str() {
            Some("--") => options_done = true,
            Some("--restart-state") => parsed.restart_state = args.next().map(PathBuf::from),
            Some("--after-pid") => {
                parsed.after_pid = args.next().and_then(|p| p.to_str()?.parse().ok());
            }
            Some("--no-xwayland") => parsed.no_xwayland = true,
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

    // Restarted for the pin: the previous QuickPlay hands over the socket
    // and the device by exiting, and where it was comes from its file.
    if let Some(pid) = args.after_pid {
        restart::wait_for_exit(pid);
    }
    let resume = args.restart_state.as_deref().and_then(restart::take_state);

    // Single instance: hand the files to a running player if there is one;
    // otherwise become the one that listens. Losing the creation race means
    // another launch is starting right now, so hand over to it instead. A
    // restart is the running player itself and hands nothing over.
    if resume.is_none() {
        match ipc::send(mode, &files) {
            Ok(true) => {
                tracing::info!(files = files.len(), "handed to the running instance");
                return Ok(());
            }
            Ok(false) => {}
            Err(e) => tracing::warn!(error = %e, "single-instance check failed"),
        }
    }
    // A restart takes the socket over from the previous process, waiting
    // for it to let go if it is slow to exit.
    let started_server = if resume.is_some() {
        restart::take_over_socket()
    } else {
        ipc::Server::start()
    };
    let server = match started_server {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && resume.is_none() => {
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
    // Pinned in a Wayland session whose XWayland answers: run under
    // XWayland, where a window can be kept on top (src/restart.rs).
    let xwayland = !args.no_xwayland && restart::wants_xwayland(settings.always_on_top);
    let options = native_options(&settings, xwayland);

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        files = files.len(),
        "starting WaveFlow QuickPlay"
    );
    // Kept for a start on Wayland if the window cannot be made under
    // XWayland: winit builds one event loop per process, so that start is
    // a new process.
    let fallback = xwayland.then(|| (resume.clone(), files.clone()));
    let created = Rc::new(Cell::new(false));
    let app_created = Rc::clone(&created);
    let result = run_native(
        "WaveFlow QuickPlay",
        options,
        Box::new(move |cc| {
            app_created.set(true);
            let app = QuickPlayApp::new(cc, settings, files, server, resume, xwayland);
            tracing::info!(elapsed_ms = started.elapsed().as_millis(), "window ready");
            Ok(Box::new(app))
        }),
    );
    if let (Err(e), Some((resume, files)), false) = (&result, fallback, created.get()) {
        tracing::warn!(error = %e, "no window under XWayland, starting again on Wayland");
        match restart::relaunch_on_wayland(resume.as_ref(), &files, args.enqueue) {
            Ok(()) => return Ok(()),
            Err(e) => tracing::error!(error = %e, "starting again on Wayland failed"),
        }
    }
    result
}

/// The window as the settings leave it, under X11 when `x11` (XWayland in
/// a Wayland session).
fn native_options(settings: &Settings, x11: bool) -> eframe::NativeOptions {
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
    #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
    let mut options = eframe::NativeOptions {
        viewport,
        centered: false,
        ..Default::default()
    };
    #[cfg(target_os = "linux")]
    if x11 {
        use winit::platform::x11::EventLoopBuilderExtX11;
        tracing::info!("pinned in a Wayland session: running under XWayland");
        options.event_loop_builder = Some(Box::new(|builder| {
            builder.with_x11();
        }));
    }
    #[cfg(not(target_os = "linux"))]
    let _ = x11;
    options
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

    #[test]
    fn the_restart_options_take_their_values() {
        let a = parse(&[
            "--after-pid",
            "4242",
            "--restart-state",
            "/run/user/1000/state.json",
            "--no-xwayland",
            "a.flac",
            "b.flac",
        ]);
        assert_eq!(a.after_pid, Some(4242));
        assert_eq!(
            a.restart_state,
            Some(PathBuf::from("/run/user/1000/state.json"))
        );
        assert!(a.no_xwayland);
        assert_eq!(a.files, [PathBuf::from("a.flac"), "b.flac".into()]);
        assert!(a.unknown.is_empty());
    }

    #[test]
    fn a_bad_pid_takes_only_its_own_value() {
        let a = parse(&["--after-pid", "not-a-pid", "a.flac"]);
        assert_eq!(a.after_pid, None);
        assert_eq!(a.files, [PathBuf::from("a.flac")]);
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
