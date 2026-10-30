//! Single instance: a second launch hands its files to the running player
//! and exits, so "Play with" / "Add to playlist" from the file manager land
//! in the open window instead of opening another one (what PotPlayer does).
//!
//! Transport: one local socket per user. A socket file in a per-user,
//! mode-0700 directory (`$XDG_RUNTIME_DIR`, or `$TMPDIR` on macOS) where
//! there is one, so no other local user can reach or squat it; a named pipe
//! on Windows, whose default DACL lets only the owner write; the Linux
//! abstract namespace only as a last resort. Pipe and abstract names are
//! machine-wide, so another user could create ours first: there both ends
//! check that the other runs as the same user. Messages are capped in size
//! and count. What
//! crosses the socket is a list of file paths to play, nothing secret.
//! Protocol, UTF-8 lines: `play` or `enqueue`, one absolute path per line,
//! an empty line; the server answers `ok` once it has queued the message,
//! so the client can exit knowing the files were taken. A message that is
//! cut short, too large or unreadable gets no `ok`.
//!
//! Every connection is read on a thread of its own, bounded in number,
//! against one deadline for the whole message ([`MESSAGE_DEADLINE`]), and
//! the client gives up after [`CLIENT_TIMEOUT`]: a client that connects and
//! never finishes cannot hold up the launches after it, and a server that
//! never answers cannot hang one.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TrySendError};
use interprocess::local_socket::{
    prelude::*, GenericFilePath, GenericNamespaced, ListenerOptions, Name, Stream,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

/// Longest a hand-off may take, connection to answer. Named pipes have no
/// read timeout, so the exchange runs on a thread and is waited for.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(3);
/// Longest the server waits for a client to finish its message, all reads
/// together.
const MESSAGE_DEADLINE: Duration = Duration::from_secs(2);
/// Connections read at the same time. One more waits for a slot.
const MAX_CLIENTS: usize = 8;
/// Messages taken and not yet picked up by the UI. One more is refused.
/// Room for a large selection handed over before the window is up, one
/// message per file.
const MAX_QUEUED: usize = 1024;

/// The socket's name, its file when it has one, and whether its name is
/// machine-wide (a Windows pipe, the Linux abstract namespace): anyone can
/// take such a name first, so each end checks who the other runs as.
struct Endpoint {
    name: Name<'static>,
    path: Option<PathBuf>,
    check_peer: bool,
}

fn endpoint() -> io::Result<Endpoint> {
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
        Ok(Endpoint {
            name: base.to_ns_name::<GenericNamespaced>()?,
            path: None,
            check_peer: true,
        })
    } else {
        let dir = private_dir
            .or_else(dirs::cache_dir)
            .unwrap_or_else(std::env::temp_dir);
        let path = dir.join(base);
        let name = path
            .clone()
            .into_os_string()
            .to_fs_name::<GenericFilePath>()?;
        Ok(Endpoint {
            name,
            path: Some(path),
            check_peer: false,
        })
    }
}

/// A directory only this user can enter: `$XDG_RUNTIME_DIR` (mode 0700 by
/// the spec) where there is one; on macOS the per-user temporary directory
/// launchd hands out in `$TMPDIR` (`/var/folders/…/T`, mode 0700), never
/// the shared `/tmp`. Both come from the environment, so they are checked:
/// owned by this user, no access for anyone else.
fn private_runtime_dir() -> Option<PathBuf> {
    if let Some(dir) = dirs::runtime_dir().filter(|d| is_private(d)) {
        return Some(dir);
    }
    if cfg!(target_os = "macos") {
        let dir = std::env::temp_dir();
        let shared = ["/tmp", "/private/tmp"]
            .iter()
            .any(|shared| dir.starts_with(shared));
        if !shared && is_private(&dir) {
            return Some(dir);
        }
    }
    None
}

/// A directory owned by this user that nobody else can enter.
#[cfg(unix)]
pub fn is_private(dir: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(dir).is_ok_and(|m| {
        // SAFETY: geteuid has no preconditions and cannot fail.
        m.is_dir() && m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o077 == 0
    })
}

#[cfg(not(unix))]
pub fn is_private(dir: &std::path::Path) -> bool {
    dir.is_dir()
}

/// Whether the process at the other end of `conn` runs as this user.
#[cfg(unix)]
fn same_user(conn: &Stream) -> bool {
    // SAFETY: geteuid has no preconditions and cannot fail.
    let me = unsafe { libc::geteuid() };
    conn.peer_creds()
        .ok()
        .and_then(|c| c.euid())
        .is_some_and(|uid| uid == me)
}

#[cfg(windows)]
fn same_user(conn: &Stream) -> bool {
    conn.peer_creds()
        .ok()
        .and_then(|c| c.pid())
        .is_some_and(crate::platform::same_user_process)
}

#[cfg(not(any(unix, windows)))]
fn same_user(_conn: &Stream) -> bool {
    true
}

/// Hands `paths` to a running instance. `Ok(true)` when one took them,
/// `Ok(false)` when nobody is listening or the one listening turned them
/// down. `InvalidInput` when a path cannot be carried (not UTF-8, or with
/// a line break in it), `TimedOut` when the instance did not answer.
pub fn send(mode: Mode, paths: &[PathBuf]) -> io::Result<bool> {
    let body = encode(mode, paths)?;
    let point = endpoint()?;
    let (tx, rx) = crossbeam_channel::bounded(1);
    // Set when this call gives up: an exchange still stuck in its connect
    // (a busy pipe) then sends nothing once it gets through, or the files
    // would land in the running player as well as in the window this
    // launch opens instead.
    let abandoned = Arc::new(AtomicBool::new(false));
    let worker_abandoned = abandoned.clone();
    std::thread::Builder::new()
        .name("quickplay-ipc-send".into())
        .spawn(move || {
            let _ = tx.send(exchange(point, &body, &worker_abandoned));
        })?;
    match rx.recv_timeout(CLIENT_TIMEOUT) {
        Ok(result) => result,
        Err(_) => {
            abandoned.store(true, Ordering::Release);
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the running instance did not answer",
            ))
        }
    }
}

fn encode(mode: Mode, paths: &[PathBuf]) -> io::Result<String> {
    if paths.len() > MAX_PATHS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("more than {MAX_PATHS} files"),
        ));
    }
    let mut body = String::new();
    body.push_str(mode.as_str());
    body.push('\n');
    for p in paths {
        let abs = std::path::absolute(p).unwrap_or_else(|_| p.clone());
        // One path per line, as UTF-8: a path that cannot be written that
        // way is not changed into another one, the hand-off is refused.
        let line = abs
            .to_str()
            .filter(|s| !s.contains(['\n', '\r']))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{} cannot be handed over", abs.display()),
                )
            })?;
        body.push_str(line);
        body.push('\n');
    }
    body.push('\n');
    if body.len() as u64 > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "too many files to hand over",
        ));
    }
    Ok(body)
}

fn exchange(point: Endpoint, body: &str, abandoned: &AtomicBool) -> io::Result<bool> {
    let mut conn = match Stream::connect(point.name) {
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
    if point.check_peer && !same_user(&conn) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the socket is held by another user",
        ));
    }
    if abandoned.load(Ordering::Acquire) {
        return Err(io::Error::new(io::ErrorKind::TimedOut, "given up"));
    }
    // Where the transport has timeouts (Unix sockets), the worker ends
    // with the wait instead of outliving it.
    let _ = conn.set_send_timeout(Some(CLIENT_TIMEOUT));
    let _ = conn.set_recv_timeout(Some(CLIENT_TIMEOUT));
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
        let point = endpoint()?;
        let socket_path = point.path.clone();
        let listener = match listen(point.name.clone()) {
            Err(e) if e.kind() == io::ErrorKind::AddrInUse && socket_path.is_some() => {
                // Either a file left by a crashed instance, or an instance
                // that came up between our connect attempt and now. Only a
                // socket that refuses the connection is stale; one that
                // answers, is too busy to, or fails some other way is left
                // alone and reported as taken, so the caller tries again.
                match answers(point.name.clone()) {
                    Ok(false) => {}
                    Ok(true) => return Err(e),
                    Err(probe) => {
                        tracing::warn!(error = %probe, "ipc: socket file neither live nor stale");
                        return Err(e);
                    }
                }
                let path = socket_path.as_deref().expect("checked");
                std::fs::remove_file(path)?;
                ListenerOptions::new().name(point.name).create_sync()?
            }
            other => other?,
        };
        let (tx, rx) = crossbeam_channel::bounded(MAX_QUEUED);
        let ctx: CtxSlot = Arc::default();
        let thread_ctx = ctx.clone();
        let check_peer = point.check_peer;
        std::thread::Builder::new()
            .name("quickplay-ipc".into())
            .spawn(move || serve(listener, tx, thread_ctx, check_peer))?;
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

/// Whether something listens on `name`, within [`CLIENT_TIMEOUT`]: `true`
/// when it accepts or keeps the connect waiting (a busy listener), `false`
/// when the connection is refused or the socket is gone (a stale file).
/// Any other error is the caller's to judge.
fn answers(name: Name<'static>) -> io::Result<bool> {
    let (tx, rx) = crossbeam_channel::bounded(1);
    std::thread::Builder::new()
        .name("quickplay-ipc-probe".into())
        .spawn(move || {
            let _ = tx.send(Stream::connect(name).map(drop));
        })?;
    match rx.recv_timeout(CLIENT_TIMEOUT) {
        Ok(Ok(())) | Err(_) => Ok(true),
        Ok(Err(e))
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            Ok(false)
        }
        Ok(Err(e)) => Err(e),
    }
}

/// Creates the listener. A pipe whose name is taken fails on Windows with
/// "access denied" (the first instance is created exclusive), reported here
/// as `AddrInUse` like a Unix socket, so a launch that loses the start race
/// hands its files over instead of opening a window of its own.
fn listen(name: Name<'static>) -> io::Result<interprocess::local_socket::Listener> {
    match ListenerOptions::new().name(name).create_sync() {
        #[cfg(windows)]
        Err(e)
            if e.raw_os_error()
                == Some(windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED as i32) =>
        {
            Err(io::Error::new(io::ErrorKind::AddrInUse, e))
        }
        other => other,
    }
}

fn serve(
    listener: interprocess::local_socket::Listener,
    tx: Sender<Message>,
    ctx: CtxSlot,
    check_peer: bool,
) {
    let active = Arc::new(AtomicUsize::new(0));
    let taken = Taken::default();
    for conn in listener.incoming() {
        let conn = match conn {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, "ipc: incoming connection failed");
                continue;
            }
        };
        if check_peer && !same_user(&conn) {
            tracing::warn!("ipc: connection from another user refused");
            continue;
        }
        // A burst (the file manager starts one launch per selected file)
        // waits for a slot rather than being refused: a refused launch
        // would open a window of its own. Every slot is freed within
        // MESSAGE_DEADLINE, so the wait is bounded.
        while active.load(Ordering::Acquire) >= MAX_CLIENTS {
            std::thread::sleep(Duration::from_millis(5));
        }
        let slot = Slot::take(&active);
        let (tx, ctx, taken) = (tx.clone(), ctx.clone(), taken.clone());
        // The slot goes back when the closure is dropped: after `handle`,
        // when it panics, or when no thread could be spawned for it.
        let spawned = std::thread::Builder::new()
            .name("quickplay-ipc-client".into())
            .spawn(move || {
                let _slot = slot;
                handle(conn, &tx, &ctx, &taken);
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "ipc: no thread for a connection");
        }
    }
}

/// One of the [`MAX_CLIENTS`] reading slots, given back on drop.
struct Slot(Arc<AtomicUsize>);

impl Slot {
    fn take(active: &Arc<AtomicUsize>) -> Self {
        active.fetch_add(1, Ordering::AcqRel);
        Self(active.clone())
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// How long a hand-off taken is remembered: longer than a launch keeps
/// trying (`HAND_OFF_DEADLINE` in main.rs).
const REMEMBER_TAKEN: Duration = Duration::from_secs(60);

/// Hand-offs taken lately, by the process that sent them and what they
/// carried. A launch whose message was queued but whose `ok` never reached
/// it (a timeout, a pipe broken after the write) tries again: that second
/// try is answered without its files being queued twice. Two launches
/// with the same files are two processes, so both are queued.
type Taken = Arc<Mutex<HashMap<(i64, u64), Instant>>>;

/// Whether `key` was taken less than [`REMEMBER_TAKEN`] before `now`. Older
/// entries are forgotten first, so a process id the system has reused, with
/// the same files, is not mistaken for a retry.
fn taken_lately(taken: &mut HashMap<(i64, u64), Instant>, key: (i64, u64), now: Instant) -> bool {
    taken.retain(|_, at| now.saturating_duration_since(*at) < REMEMBER_TAKEN);
    taken.contains_key(&key)
}

fn digest(message: &Message) -> u64 {
    let mut hasher = DefaultHasher::new();
    message.mode.hash(&mut hasher);
    message.paths.hash(&mut hasher);
    hasher.finish()
}

/// The process at the other end of `conn`: a pipe's client on Windows, a
/// socket's peer on Linux (`SO_PEERCRED`) and on macOS (`LOCAL_PEERPID`,
/// which interprocess does not read there).
#[cfg(not(target_os = "macos"))]
fn peer_pid(conn: &Stream) -> Option<i64> {
    conn.peer_creds().ok().and_then(|c| c.pid()).map(i64::from)
}

#[cfg(target_os = "macos")]
fn peer_pid(conn: &Stream) -> Option<i64> {
    use std::os::fd::{AsFd, AsRawFd};
    let Stream::UdSocket(socket) = conn;
    let mut pid: libc::pid_t = 0;
    let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: the descriptor is a connected socket borrowed from `conn`;
    // `pid` and `len` are valid for writes of the sizes given.
    let got = unsafe {
        libc::getsockopt(
            socket.as_fd().as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&raw mut pid).cast(),
            &raw mut len,
        )
    };
    (got == 0 && pid > 0).then(|| i64::from(pid))
}

fn handle(conn: Stream, tx: &Sender<Message>, ctx: &CtxSlot, taken: &Taken) {
    // Without the sender's process a try is never recognised as a repeat.
    let peer = peer_pid(&conn);
    // One deadline for the whole message: named pipes have no read
    // timeout, and a per-read timeout on a Unix socket lets a client that
    // sends a byte now and then hold its slot forever. Unix sockets are
    // read nonblocking; a pipe read nonblocking reports "no data yet" as
    // the end of the stream, so it is polled instead (see `Deadline`).
    #[cfg(not(windows))]
    if let Err(e) = conn.set_nonblocking(true) {
        tracing::warn!(error = %e, "ipc: connection cannot be read with a deadline");
        return;
    }
    let mut reader = BufReader::new(Deadline {
        inner: conn,
        until: Instant::now() + MESSAGE_DEADLINE,
    });
    let Some(message) = read_message(&mut reader) else {
        return;
    };
    let key = peer.map(|pid| (pid, digest(&message)));
    {
        let mut taken = taken.lock().unwrap_or_else(PoisonError::into_inner);
        if key.is_some_and(|key| taken_lately(&mut taken, key, Instant::now())) {
            tracing::info!("ipc: a hand-off tried again, already taken");
        } else {
            tracing::info!(mode = ?message.mode, paths = message.paths.len(), "ipc: files received");
            match tx.try_send(message) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    tracing::warn!("ipc: the player is not taking messages, one refused");
                    return;
                }
                Err(TrySendError::Disconnected(_)) => return,
            }
            if let Some(key) = key {
                taken.insert(key, Instant::now());
            }
            if let Ok(slot) = ctx.lock() {
                if let Some(ctx) = slot.as_ref() {
                    ctx.request_repaint();
                }
            }
        }
    }
    let conn = &mut reader.get_mut().inner;
    #[cfg(not(windows))]
    let _ = conn.set_nonblocking(false);
    let _ = conn.write_all(b"ok\n");
}

/// Reads a connection until `until`; past it the read fails, which ends
/// the message and frees the connection's slot. A Unix socket is read
/// nonblocking, and a read that would block waits a little and tries
/// again; a pipe stays blocking and is only read once it holds data.
struct Deadline {
    inner: Stream,
    until: Instant,
}

impl Deadline {
    fn wait(&self) -> io::Result<()> {
        if Instant::now() >= self.until {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "message not finished in time",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
        Ok(())
    }
}

impl Read for Deadline {
    #[cfg(windows)]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        use std::os::windows::io::{AsHandle, AsRawHandle};
        let Stream::NamedPipe(pipe) = &self.inner;
        // Nothing waiting yet: wait. A peek that fails (the client went
        // away) falls through to the read, which reports it.
        while crate::platform::pipe_bytes_available(pipe.as_handle().as_raw_handle())
            .is_ok_and(|n| n == 0)
        {
            self.wait()?;
        }
        self.inner.read(buf)
    }

    #[cfg(not(windows))]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            match self.inner.read(buf) {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => self.wait()?,
                other => return other,
            }
        }
    }
}

/// Upper bounds on one message: paths are short, bursts are per file.
const MAX_MESSAGE_BYTES: u64 = 4 << 20;
const MAX_PATHS: usize = 10_000;

/// One whole message, or `None`: a read error, a message cut short (no
/// empty line, or a last line without its line break, as when the size
/// limit cuts it), too many paths. A bare mode line followed by the end of
/// the stream is a focus request.
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
        let n = match reader.read_line(&mut line) {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!(error = %e, "ipc: message unreadable");
                return None;
            }
        };
        if n == 0 {
            if paths.is_empty() {
                break;
            }
            tracing::warn!("ipc: message cut short");
            return None;
        }
        let Some(path) = line.strip_suffix('\n') else {
            tracing::warn!("ipc: message cut short");
            return None;
        };
        if path.is_empty() {
            break;
        }
        if paths.len() >= MAX_PATHS {
            tracing::warn!(max = MAX_PATHS, "ipc: too many paths");
            return None;
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

    #[test]
    fn a_message_cut_short_is_refused() {
        assert!(read_message(&mut Cursor::new("play\n/a.flac\n")).is_none());
        assert!(read_message(&mut Cursor::new("play\n/a.flac\n/b.fl")).is_none());
        let many = format!("play\n{}\n", "/a.flac\n".repeat(MAX_PATHS + 1));
        assert!(read_message(&mut Cursor::new(many)).is_none());
    }

    #[test]
    fn a_slot_comes_back_when_its_reader_panics() {
        let active = Arc::new(AtomicUsize::new(0));
        let slot = Slot::take(&active);
        assert_eq!(active.load(Ordering::Acquire), 1);
        let reader = std::thread::spawn(move || {
            let _slot = slot;
            panic!("reader failed");
        });
        assert!(reader.join().is_err());
        assert_eq!(active.load(Ordering::Acquire), 0);
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn a_taken_name_is_addr_in_use() {
        let name = || {
            format!("waveflow-quickplay-test-{}.sock", std::process::id())
                .to_ns_name::<GenericNamespaced>()
                .unwrap()
        };
        let _first = listen(name()).unwrap();
        let err = listen(name()).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AddrInUse);
    }

    #[test]
    fn a_hand_off_taken_long_ago_is_not_a_retry() {
        let key = (4242, 7);
        let now = Instant::now();
        let mut taken = HashMap::new();
        // The same process id and files, seen more than a minute ago: the
        // id was reused by a new launch, whose files must be queued.
        taken.insert(key, now - REMEMBER_TAKEN - Duration::from_secs(1));
        assert!(!taken_lately(&mut taken, key, now));
        assert!(taken.is_empty(), "the old entry is forgotten");
        taken.insert(key, now - Duration::from_secs(1));
        assert!(taken_lately(&mut taken, key, now));
    }

    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_hand_off_tried_again_is_queued_once() {
        let base = format!("waveflow-quickplay-retry-{}.sock", std::process::id());
        #[cfg(not(target_os = "macos"))]
        let name = || base.clone().to_ns_name::<GenericNamespaced>().unwrap();
        // macOS has no abstract namespace: a socket file, in a directory
        // made for this run with mode 0700. Creating it fails if anything
        // is already there, so it is never someone else's, even when the
        // temporary directory is the shared /tmp.
        #[cfg(target_os = "macos")]
        let dir = {
            use std::os::unix::fs::DirBuilderExt;
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!("{base}-{nanos}"));
            std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
            dir
        };
        #[cfg(target_os = "macos")]
        let path = dir.join("retry.sock");
        #[cfg(target_os = "macos")]
        let name = || {
            path.clone()
                .into_os_string()
                .to_fs_name::<GenericFilePath>()
                .unwrap()
        };
        let listener = listen(name()).unwrap();
        let (tx, rx) = crossbeam_channel::bounded(MAX_QUEUED);
        std::thread::spawn(move || serve(listener, tx, CtxSlot::default(), false));
        let hand = |paths: &[&str]| {
            let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
            let body = encode(Mode::Enqueue, &paths).unwrap();
            let point = Endpoint {
                name: name(),
                path: None,
                check_peer: false,
            };
            exchange(point, &body, &AtomicBool::new(false)).unwrap()
        };
        // The same process sending the same files again: a retry.
        assert!(hand(&["/music/a.flac"]));
        assert!(hand(&["/music/a.flac"]), "a retry is answered");
        assert!(hand(&["/music/b.flac"]));
        // Paths are made absolute on the way, so only the names are compared.
        let got: Vec<Vec<_>> = rx
            .try_iter()
            .map(|m| {
                m.paths
                    .iter()
                    .map(|p| p.file_name().unwrap().to_owned())
                    .collect()
            })
            .collect();
        assert_eq!(got, [["a.flac"], ["b.flac"]]);
        #[cfg(target_os = "macos")]
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_message_round_trips_and_odd_paths_are_refused() {
        let paths = [PathBuf::from("/music/a b.flac"), PathBuf::from("/x/y.mp3")];
        let body = encode(Mode::Play, &paths).unwrap();
        let m = read_message(&mut Cursor::new(body)).unwrap();
        assert_eq!(m.mode, Mode::Play);
        assert_eq!(m.paths.len(), 2);
        assert!(m.paths[0].ends_with("a b.flac"));
        let err = encode(Mode::Play, &[PathBuf::from("/a\nb.flac")]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        let err = encode(Mode::Play, &[PathBuf::from("/a.flac\r")]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }
}
