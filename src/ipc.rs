//! Single instance: a second launch hands its files to the running player
//! and exits, so "Play with" / "Add to playlist" from the file manager land
//! in the open window instead of opening another one (what PotPlayer does).
//!
//! Transport: one local socket per user. A socket file in a per-user,
//! mode-0700 directory (`$XDG_RUNTIME_DIR`, or `$TMPDIR` on macOS) where
//! there is one, so no other local user can reach or squat it; a named pipe
//! on Windows, whose default DACL lets only the owner write; the Linux
//! abstract namespace only as a last resort. Messages are capped in size
//! and count. What crosses the socket is a list of file paths to play,
//! nothing secret. Protocol, UTF-8 lines: `play` or `enqueue`, one absolute
//! path per line, an empty line; the server answers `ok` once it has queued
//! the message, so the client can exit knowing the files were taken.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crossbeam_channel::{Receiver, Sender};
use interprocess::local_socket::{
    prelude::*, GenericFilePath, GenericNamespaced, ListenerOptions, Name, Stream,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Replace the queue with these files and play them.
    Play,
    /// Append them to the queue, do not interrupt.
    Enqueue,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Mode::Play => "play",
            Mode::Enqueue => "enqueue",
        }
    }
}

#[derive(Debug)]
pub struct Message {
    pub mode: Mode,
    pub paths: Vec<PathBuf>,
}

/// Set by the app once the window exists, so the socket thread can wake
/// the UI (`request_repaint`) when a message arrives.
pub type CtxSlot = Arc<Mutex<Option<egui::Context>>>;

fn socket_name() -> io::Result<(Name<'static>, Option<PathBuf>)> {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "default".into());
    let base = format!("waveflow-quickplay-{user}.sock");
    // Per-user private directory first; the Linux abstract namespace is
    // host-wide and only used when there is none. On the other Unices
    // `GenericNamespaced` would mean a file in /tmp (interprocess's
    // deprecated `SpecialDirUdSocket`), which is host-wide too, so they
    // always get a path.
    let private_dir = private_runtime_dir();
    let abstract_ns = cfg!(any(target_os = "linux", target_os = "android"));
    if cfg!(windows) || (private_dir.is_none() && abstract_ns) {
        Ok((base.to_ns_name::<GenericNamespaced>()?, None))
    } else {
        let dir = private_dir
            .or_else(dirs::cache_dir)
            .unwrap_or_else(std::env::temp_dir);
        let path = dir.join(base);
        let name = path
            .clone()
            .into_os_string()
            .to_fs_name::<GenericFilePath>()?;
        Ok((name, Some(path)))
    }
}

/// A directory only this user can enter: `$XDG_RUNTIME_DIR` (mode 0700 by
/// the spec) where there is one; on macOS the per-user temporary directory
/// launchd hands out in `$TMPDIR` (`/var/folders/…/T`, mode 0700), never
/// the shared `/tmp`.
fn private_runtime_dir() -> Option<PathBuf> {
    if let Some(dir) = dirs::runtime_dir().filter(|d| d.is_dir()) {
        return Some(dir);
    }
    if cfg!(target_os = "macos") {
        let dir = std::env::temp_dir();
        let shared = ["/tmp", "/private/tmp"]
            .iter()
            .any(|shared| dir.starts_with(shared));
        if !shared && dir.is_dir() {
            return Some(dir);
        }
    }
    None
}

/// Hands `paths` to a running instance. `Ok(true)` when one took them,
/// `Ok(false)` when nobody is listening.
pub fn send(mode: Mode, paths: &[PathBuf]) -> io::Result<bool> {
    let (name, _) = socket_name()?;
    let mut conn = match Stream::connect(name) {
        Ok(c) => c,
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(false);
        }
        Err(e) => return Err(e),
    };
    let mut body = String::new();
    body.push_str(mode.as_str());
    body.push('\n');
    for p in paths {
        let abs = std::path::absolute(p).unwrap_or_else(|_| p.clone());
        body.push_str(&abs.to_string_lossy());
        body.push('\n');
    }
    body.push('\n');
    conn.write_all(body.as_bytes())?;
    conn.flush()?;
    let mut reply = Vec::new();
    let mut byte = [0u8; 1];
    while conn.read(&mut byte)? == 1 && byte[0] != b'\n' && reply.len() < 16 {
        reply.push(byte[0]);
    }
    Ok(reply == b"ok")
}

/// The listening side, owned by the running player.
pub struct Server {
    rx: Receiver<Message>,
    ctx: CtxSlot,
    socket_path: Option<PathBuf>,
}

impl Server {
    /// Starts listening; `AddrInUse` means another instance won the race.
    pub fn start() -> io::Result<Self> {
        let (name, socket_path) = socket_name()?;
        let listener = match ListenerOptions::new().name(name.clone()).create_sync() {
            Err(e) if e.kind() == io::ErrorKind::AddrInUse && socket_path.is_some() => {
                // Either a file left by a crashed instance, or an instance
                // that came up between our connect attempt and now. Only a
                // socket nobody answers is stale; a live one is handed to.
                if Stream::connect(name.clone()).is_ok() {
                    return Err(e);
                }
                let path = socket_path.as_deref().expect("checked");
                std::fs::remove_file(path)?;
                ListenerOptions::new().name(name).create_sync()?
            }
            other => other?,
        };
        let (tx, rx) = crossbeam_channel::unbounded();
        let ctx: CtxSlot = Arc::default();
        let thread_ctx = ctx.clone();
        std::thread::Builder::new()
            .name("quickplay-ipc".into())
            .spawn(move || serve(listener, tx, thread_ctx))?;
        tracing::debug!("single-instance socket ready");
        Ok(Self {
            rx,
            ctx,
            socket_path,
        })
    }

    /// Lets the socket thread repaint the window when a message arrives.
    pub fn attach(&self, ctx: &egui::Context) {
        if let Ok(mut slot) = self.ctx.lock() {
            *slot = Some(ctx.clone());
        }
    }

    pub fn try_recv(&self) -> Option<Message> {
        self.rx.try_recv().ok()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // The listener lives on the blocked thread and is never dropped;
        // remove the socket file ourselves where one exists.
        if let Some(path) = &self.socket_path {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn serve(listener: interprocess::local_socket::Listener, tx: Sender<Message>, ctx: CtxSlot) {
    for conn in listener.incoming() {
        let conn = match conn {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, "ipc: incoming connection failed");
                continue;
            }
        };
        let mut reader = BufReader::new(conn);
        let Some(message) = read_message(&mut reader) else {
            continue;
        };
        tracing::info!(mode = ?message.mode, paths = message.paths.len(), "ipc: files received");
        if tx.send(message).is_err() {
            return;
        }
        if let Ok(slot) = ctx.lock() {
            if let Some(ctx) = slot.as_ref() {
                ctx.request_repaint();
            }
        }
        let _ = reader.get_mut().write_all(b"ok\n");
    }
}

/// Upper bounds on one message: paths are short, bursts are per file.
const MAX_MESSAGE_BYTES: u64 = 4 << 20;
const MAX_PATHS: usize = 10_000;

fn read_message<R: BufRead>(reader: &mut R) -> Option<Message> {
    let mut reader = reader.take(MAX_MESSAGE_BYTES);
    let mut line = String::new();
    if reader.read_line(&mut line).ok()? == 0 {
        return None;
    }
    let mode = match line.trim_end_matches(['\r', '\n']) {
        "enqueue" => Mode::Enqueue,
        "play" => Mode::Play,
        other => {
            tracing::warn!(line = other, "ipc: unknown message");
            return None;
        }
    };
    let mut paths = Vec::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).unwrap_or(0);
        let path = line.trim_end_matches(['\r', '\n']);
        if n == 0 || path.is_empty() || paths.len() >= MAX_PATHS {
            break;
        }
        paths.push(PathBuf::from(path));
    }
    Some(Message { mode, paths })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parses_a_message() {
        let mut r = Cursor::new("enqueue\n/a/b.flac\nC:\\x y\\z.mp3\n\nignored\n");
        let m = read_message(&mut r).unwrap();
        assert_eq!(m.mode, Mode::Enqueue);
        assert_eq!(m.paths.len(), 2);
        assert_eq!(m.paths[1], PathBuf::from("C:\\x y\\z.mp3"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(read_message(&mut Cursor::new("hello\n")).is_none());
        assert!(read_message(&mut Cursor::new("")).is_none());
        let m = read_message(&mut Cursor::new("play\n")).unwrap();
        assert!(m.paths.is_empty(), "a bare play is a focus request");
    }
}
