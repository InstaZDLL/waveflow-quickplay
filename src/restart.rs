//! Starting QuickPlay again under the other window system, for the pin.
//!
//! A Wayland compositor alone decides which window stays on top, so winit
//! cannot pin one there. Under XWayland it can (GNOME, KDE). When the pin
//! is switched on in a Wayland session, QuickPlay writes where it was (the
//! queue, the track, the position, playing or paused) to a file, starts a
//! new copy of itself that waits for this one to exit, and closes; the new
//! copy starts under XWayland, since the pin is now on, and picks up from
//! the file. Unpinning does the same the other way.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// The name of a restart state file, before the process id.
const STATE_PREFIX: &str = "waveflow-quickplay-restart-";

/// Where playback was when QuickPlay restarted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resume {
    pub paths: Vec<PathBuf>,
    /// The current row, if any.
    pub index: Option<usize>,
    pub position_ms: u64,
    /// The track was loaded (playing or paused), not stopped.
    pub loaded: bool,
    pub playing: bool,
}

/// Linux: a Wayland session. winit picks Wayland whenever one of these is
/// set and not empty.
pub fn wayland_session() -> bool {
    cfg!(target_os = "linux")
        && ["WAYLAND_DISPLAY", "WAYLAND_SOCKET"]
            .iter()
            .any(|v| std::env::var_os(v).is_some_and(|s| !s.is_empty()))
}

/// Linux: an X server to fall back on, XWayland in a Wayland session.
pub fn xwayland_available() -> bool {
    cfg!(target_os = "linux") && std::env::var_os("DISPLAY").is_some_and(|s| !s.is_empty())
}

/// Whether the X server answers, as winit would open it (Xlib, with the
/// session's `XAUTHORITY`). Started for nothing, the event loop could not
/// fall back to Wayland in the same process (winit builds one per
/// process), so this is asked before choosing X11, and only then: in GNOME
/// the first X client starts XWayland.
#[cfg(target_os = "linux")]
pub fn xwayland_reachable() -> bool {
    if !xwayland_available() {
        return false;
    }
    let Ok(xlib) = x11_dl::xlib::Xlib::open() else {
        return false;
    };
    // SAFETY: Xlib's own calls. XInitThreads first, as winit makes it: in
    // a threaded program it must come before any other Xlib call (libX11
    // before 1.8 does not make it by itself), and a second call is a
    // no-op. Then open and close a display it hands back.
    unsafe {
        if (xlib.XInitThreads)() == 0 {
            return false;
        }
        let display = (xlib.XOpenDisplay)(std::ptr::null());
        if display.is_null() {
            return false;
        }
        (xlib.XCloseDisplay)(display);
    }
    true
}

#[cfg(not(target_os = "linux"))]
pub fn xwayland_reachable() -> bool {
    false
}

/// Whether QuickPlay should run under XWayland: pinned, in a Wayland
/// session whose XWayland answers.
pub fn wants_xwayland(pinned: bool) -> bool {
    pinned && wayland_session() && xwayland_reachable()
}

/// Writes `resume` to a file of its own and starts a new QuickPlay that
/// waits for this process to exit, then reads it. The caller closes.
pub fn relaunch(resume: &Resume) -> io::Result<()> {
    spawn_after_exit(resume, false)
}

/// The window could not be made under XWayland (the display answered but
/// the window or its OpenGL context failed): start again on Wayland, where
/// the pin does nothing but the player works, with what this process was
/// given, the state of a restart (`resume`) or the files of a launch.
/// Without this the queue of a restart would be lost, the previous process
/// having closed already.
pub fn relaunch_on_wayland(
    resume: Option<&Resume>,
    files: &[PathBuf],
    enqueue: bool,
) -> io::Result<()> {
    let Some(resume) = resume else {
        let mut command = after_exit_command()?;
        command.arg("--no-xwayland");
        if enqueue {
            command.arg("--enqueue");
        }
        command.arg("--").args(files);
        return command.spawn().map(drop);
    };
    spawn_after_exit(resume, true)
}

/// This executable, started to wait for this process to exit.
fn after_exit_command() -> io::Result<std::process::Command> {
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command
        .arg("--after-pid")
        .arg(std::process::id().to_string())
        .stdin(std::process::Stdio::null());
    Ok(command)
}

fn spawn_after_exit(resume: &Resume, no_xwayland: bool) -> io::Result<()> {
    let state = write_state(resume)?;
    let mut command = after_exit_command()?;
    command.arg("--restart-state").arg(&state);
    if no_xwayland {
        command.arg("--no-xwayland");
    }
    if let Err(e) = command.spawn() {
        let _ = std::fs::remove_file(&state);
        return Err(e);
    }
    Ok(())
}

/// Where the state file goes: the per-user runtime directory, which only
/// this user can enter, when there is one; the temporary directory
/// otherwise, where the file itself is private (mode 0600).
fn state_dir() -> PathBuf {
    dirs::runtime_dir()
        .filter(|d| crate::ipc::is_private(d))
        .unwrap_or_else(std::env::temp_dir)
}

fn write_state(resume: &Resume) -> io::Result<PathBuf> {
    use std::hash::{BuildHasher, Hasher};
    let json = serde_json::to_vec(resume).map_err(io::Error::other)?;
    // Not guessable, so nobody can take the name first.
    let nonce = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    let path = state_dir().join(format!(
        "{STATE_PREFIX}{}-{nonce:016x}.json",
        std::process::id()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&path)?;
    if let Err(e) = file.write_all(&json) {
        drop(file);
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    Ok(path)
}

/// Reads and removes the state a restarting QuickPlay left. Only a file
/// [`write_state`] could have written is taken: the option is on the
/// command line, and the file is deleted.
pub fn take_state(path: &Path) -> Option<Resume> {
    let ours = path.parent() == Some(state_dir().as_path())
        && path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(STATE_PREFIX) && n.ends_with(".json"));
    if !ours || !owned_file(path) {
        tracing::warn!(path = %path.display(), "not a restart state, ignored");
        return None;
    }
    let bytes = std::fs::read(path);
    let _ = std::fs::remove_file(path);
    match bytes.map(|b| serde_json::from_slice(&b)) {
        Ok(Ok(resume)) => Some(resume),
        Ok(Err(e)) => {
            tracing::warn!(path = %path.display(), error = %e, "restart state unreadable");
            None
        }
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "restart state missing");
            None
        }
    }
}

/// A plain file of this user's, not a link to one elsewhere.
#[cfg(unix)]
fn owned_file(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(path).is_ok_and(|m| {
        // SAFETY: geteuid has no preconditions and cannot fail.
        m.is_file() && m.uid() == unsafe { libc::geteuid() }
    })
}

#[cfg(not(unix))]
fn owned_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file())
}

/// Waits, up to ten seconds, for the QuickPlay that started this one to
/// exit: it still holds the single-instance socket and the audio device.
///
/// `false` when it is still running then. The caller carries on anyway,
/// rather than leave nothing playing, and gives the socket more time
/// ([`take_over_socket`]).
pub fn wait_for_exit(pid: u32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    let proc_dir = PathBuf::from(format!("/proc/{pid}"));
    while proc_dir.exists() {
        if Instant::now() >= deadline {
            tracing::warn!(pid, "the previous QuickPlay is still running");
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

/// After a restart the single-instance socket is this process's, once the
/// previous one lets it go: tried again for up to ten seconds more while
/// it is taken, so a slow exit does not leave QuickPlay without it (a later
/// launch would then find nobody and open a second player).
pub fn take_over_socket() -> io::Result<crate::ipc::Server> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match crate::ipc::Server::start() {
            Err(e) if e.kind() == io::ErrorKind::AddrInUse && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            other => return other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_round_trips_and_is_removed() {
        let resume = Resume {
            paths: vec![
                PathBuf::from("/music/a.flac"),
                PathBuf::from("/music/b.flac"),
            ],
            index: Some(1),
            position_ms: 61_500,
            loaded: true,
            playing: false,
        };
        let path = write_state(&resume).unwrap();
        assert_eq!(take_state(&path), Some(resume));
        assert!(!path.exists());
        assert_eq!(take_state(&path), None);
    }

    #[test]
    fn any_other_file_is_left_alone() {
        let other = state_dir().join(format!("quickplay-not-a-state-{}.json", std::process::id()));
        std::fs::write(&other, b"{}").unwrap();
        assert_eq!(take_state(&other), None);
        assert!(other.exists(), "not deleted");
        let _ = std::fs::remove_file(&other);
    }
}
