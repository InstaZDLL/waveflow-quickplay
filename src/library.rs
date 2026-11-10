//! Reading what a file says about itself — tags, duration, the embedded or
//! folder cover — off the UI thread, through the same `waveflow-core`
//! helpers the desktop scanner uses.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver, Sender};
use lofty::picture::PictureType;
use lofty::prelude::*;
use lofty::tag::ItemKey;
use waveflow_core::audio_format::dsd::metadata::read_metadata as read_dsd_metadata;
use waveflow_core::audio_format::dsd::parser::{parse_dff, parse_dsf, DsdContainer, DsdLayout};
use waveflow_core::scanner::extract::{
    file_type_label, is_scannable_audio, probe_playable_stream, read_tagged_file, AUDIO_EXTENSIONS,
};

/// Longest edge of the decoded cover kept in memory. 1024 covers a
/// 512-point artwork pane at 2× DPI.
const COVER_MAX_PX: u32 = 1024;

/// How deep a dropped folder is walked.
const FOLDER_DEPTH: usize = 4;

#[derive(Debug, Clone, Default)]
pub struct TrackMeta {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
    /// `FLAC`, `MP3`, `DSD64`, … as WaveFlow's format chip spells them.
    pub codec: Option<String>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u8>,
    /// kbps, when the container states it.
    pub bitrate_kbps: Option<u32>,
}

/// A decoded cover, RGBA8, ready for `egui::ColorImage`.
pub struct CoverImage {
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

/// Playlists QuickPlay opens as the files they list.
pub const PLAYLIST_EXTENSIONS: &[&str] = &["m3u", "m3u8"];

/// Largest playlist file read, the desktop importer's bound.
const MAX_PLAYLIST_BYTES: u64 = 16 << 20;

/// Every extension QuickPlay opens: the audio formats, then the playlists.
pub fn openable_extensions() -> Vec<&'static str> {
    AUDIO_EXTENSIONS
        .iter()
        .chain(PLAYLIST_EXTENSIONS)
        .copied()
        .collect()
}

pub fn is_playlist(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        PLAYLIST_EXTENSIONS
            .iter()
            .any(|p| p.eq_ignore_ascii_case(e))
    })
}

/// Expand files, folders and playlists into the audio files they hold, in
/// a stable order. Folders are walked a few levels deep (the playlists in
/// them are not read: they would add their files twice); a playlist gives
/// its entries in its own order; non-audio files are skipped.
pub fn collect_audio_paths(inputs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for input in inputs {
        if input.is_dir() {
            let mut found = Vec::new();
            walk(input, 0, &mut found);
            found.sort();
            out.extend(found);
        } else if is_playlist(input) {
            out.extend(playlist_entries(input));
        } else if is_scannable_audio(input) {
            out.push(input.clone());
        }
    }
    out
}

/// The audio files an `.m3u` / `.m3u8` lists, in its order. The text is
/// decoded in whatever code page wrote it and parsed by WaveFlow's own
/// importer (`#EXTINF` and the other comments are passed over). An entry
/// is an absolute path, a path relative to the playlist's folder, or a
/// `file://` URL; web addresses are skipped, QuickPlay plays files, and so
/// is any entry that is not an audio file on disk.
fn playlist_entries(path: &Path) -> Vec<PathBuf> {
    use std::io::Read;
    let mut raw = Vec::new();
    let read = std::fs::File::open(path)
        .and_then(|file| file.take(MAX_PLAYLIST_BYTES + 1).read_to_end(&mut raw));
    if let Err(err) = read {
        tracing::warn!(path = %path.display(), %err, "playlist unreadable");
        return Vec::new();
    }
    if raw.len() as u64 > MAX_PLAYLIST_BYTES {
        tracing::warn!(path = %path.display(), "playlist larger than 16 MiB, not read");
        return Vec::new();
    }
    let text = waveflow_core::text_encoding::decode(&raw);
    let folder = path.parent().unwrap_or_else(|| Path::new(""));
    let home = location(path);
    let mut out = Vec::new();
    let mut skipped = 0usize;
    for entry in waveflow_core::playlist_import::m3u::parse(&text).entries {
        match entry_path(&entry.path, folder) {
            // Checked before the file is touched: see `may_follow`.
            Some(file)
                if may_follow(&location(&file), &home)
                    && file.is_file()
                    && is_scannable_audio(&file) =>
            {
                out.push(file)
            }
            _ => skipped += 1,
        }
    }
    if skipped > 0 {
        tracing::debug!(path = %path.display(), skipped, "playlist entries skipped");
    }
    out
}

/// Where a path lives, as far as reaching it goes.
#[derive(Debug, PartialEq, Eq)]
// Only Windows tells shares and devices apart (`location`).
#[cfg_attr(not(windows), allow(dead_code))]
enum Location {
    /// A drive of this machine, a mapped drive included.
    Local,
    /// A network share, `\\host\share\…` (Windows), by its host.
    Share(String),
    /// A device path (`\\.\…`, `\\?\GLOBALROOT\…`), a verbatim share
    /// (`\\?\UNC\…`), or anything not clearly a drive or a share: nothing a
    /// playlist has reason to name.
    Device,
}

/// Fails closed: the path is made absolute first, the way Windows will
/// resolve it (against the current directory, which may itself be a
/// share), and only a drive or a `\\host\share` prefix is taken for what
/// it says. Anything else, a bare `\\host` or a verbatim share included,
/// is a `Device`.
#[cfg(windows)]
fn location(path: &Path) -> Location {
    use std::path::{Component, Prefix};
    let Ok(path) = std::path::absolute(path) else {
        return Location::Device;
    };
    match path.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::UNC(host, share) if !host.is_empty() && !share.is_empty() => {
                Location::Share(host.to_string_lossy().to_lowercase())
            }
            Prefix::Disk(_) | Prefix::VerbatimDisk(_) => Location::Local,
            _ => Location::Device,
        },
        _ => Location::Device,
    }
}

#[cfg(not(windows))]
fn location(_path: &Path) -> Location {
    Location::Local
}

/// Whether an entry at `entry` may be opened from a playlist at `home`.
/// On Windows, merely opening `\\host\…` connects to that host and hands
/// it this user's NTLM credentials, so a playlist downloaded from anywhere
/// could leak them by naming a share of the sender's. A share is followed
/// only from a playlist on the same host: an `.m3u` on the NAS listing the
/// NAS's files works, one in Downloads naming a stranger's server does
/// not. Mapped drives are local paths and always work.
fn may_follow(entry: &Location, home: &Location) -> bool {
    match entry {
        Location::Local => true,
        Location::Share(_) => entry == home,
        Location::Device => false,
    }
}

/// The file a playlist entry names, `None` for a web address.
fn entry_path(written: &str, folder: &Path) -> Option<PathBuf> {
    let path = match written.split_once("://") {
        Some((scheme, rest)) if scheme.eq_ignore_ascii_case("file") => file_url_path(rest)?,
        Some(_) => return None,
        None => PathBuf::from(written),
    };
    if path.is_absolute() {
        return Some(path);
    }
    let joined = folder.join(path);
    // A playlist written on Windows separates with `\`, a valid file name
    // character elsewhere: the entry as written comes first.
    if cfg!(not(windows)) && written.contains('\\') && !joined.is_file() {
        let slashed = folder.join(written.replace('\\', "/"));
        if slashed.is_file() {
            return Some(slashed);
        }
    }
    Some(joined)
}

/// The path of a `file://` URL, given what follows `file://`: a host
/// (empty or `localhost` for this machine) and a percent-encoded path.
/// `file:///C:/Music/a%20b.flac` is `C:/Music/a b.flac` on Windows, and a
/// host there is a network share (`\\host\share\…`).
fn file_url_path(rest: &str) -> Option<PathBuf> {
    let slash = rest.find('/')?;
    let (host, path) = rest.split_at(slash);
    let path = percent_decode(path)?;
    let local = host.is_empty() || host.eq_ignore_ascii_case("localhost");
    if cfg!(windows) {
        if !local {
            return Some(PathBuf::from(format!(
                r"\\{host}{}",
                path.replace('/', r"\")
            )));
        }
        // `/C:/…`: the drive comes after the slash that ends the host.
        let bytes = path.as_bytes();
        if bytes.len() >= 3
            && bytes[0] == b'/'
            && bytes[1].is_ascii_alphabetic()
            && bytes[2] == b':'
        {
            return Some(PathBuf::from(&path[1..]));
        }
        Some(PathBuf::from(path))
    } else {
        local.then(|| PathBuf::from(path))
    }
}

/// `%XX` escapes to bytes, read as UTF-8; `None` when they are not. A `%`
/// without two hex digits after it is kept as it is.
fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = (bytes[i] == b'%')
            .then(|| bytes.get(i + 1..i + 3))
            .flatten()
            .filter(|hex| hex.iter().all(u8::is_ascii_hexdigit))
            .and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok());
        match escaped {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth + 1 < FOLDER_DEPTH {
                walk(&path, depth + 1, out);
            }
        } else if is_scannable_audio(&path) {
            out.push(path);
        }
    }
}

fn clean(s: Option<&str>) -> Option<String> {
    s.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Tags + duration + codec. Never fails: a file nothing can read still gets
/// its file name as a title.
pub fn read_meta(path: &Path) -> TrackMeta {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    let mut meta = match ext.as_deref() {
        Some("dsf") | Some("dff") => read_dsd_meta(path, ext.as_deref().unwrap_or("")),
        _ => read_tagged_meta(path),
    };
    if meta.title.is_none() {
        meta.title = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned);
    }
    meta
}

fn read_tagged_meta(path: &Path) -> TrackMeta {
    match read_tagged_file(path) {
        Ok(tagged) => {
            let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
            let props = tagged.properties();
            let duration = props.duration();
            TrackMeta {
                title: tag.and_then(|t| clean(t.title().as_deref())),
                artist: tag.and_then(|t| {
                    clean(t.artist().as_deref())
                        .or_else(|| clean(t.get_string(ItemKey::AlbumArtist)))
                }),
                album: tag.and_then(|t| clean(t.album().as_deref())),
                duration_ms: (duration.as_millis() > 0).then_some(duration.as_millis() as u64),
                codec: file_type_label(tagged.file_type()),
                sample_rate: props.sample_rate(),
                bit_depth: props.bit_depth(),
                bitrate_kbps: props.audio_bitrate().filter(|b| *b > 0),
            }
        }
        Err(err) => {
            tracing::debug!(path = %path.display(), %err, "tags unreadable; probing the stream");
            // The player will still open it if symphonia can — list it the
            // way the scanner would (#873 in the desktop app).
            match probe_playable_stream(path) {
                Some(info) => TrackMeta {
                    duration_ms: (info.duration_ms > 0).then_some(info.duration_ms as u64),
                    codec: info.codec,
                    sample_rate: info.sample_rate.and_then(|r| u32::try_from(r).ok()),
                    bit_depth: info.bit_depth.and_then(|b| u8::try_from(b).ok()),
                    ..TrackMeta::default()
                },
                None => TrackMeta::default(),
            }
        }
    }
}

fn read_dsd_meta(path: &Path, ext: &str) -> TrackMeta {
    let Ok(mut file) = std::fs::File::open(path) else {
        return TrackMeta::default();
    };
    let (layout, container) = match ext {
        "dsf" => (parse_dsf(&mut file).ok(), DsdContainer::Dsf),
        _ => (parse_dff(&mut file).ok(), DsdContainer::Dff),
    };
    let tags = read_dsd_metadata(&mut file, container).unwrap_or_default();
    TrackMeta {
        title: clean(tags.title.as_deref()),
        artist: clean(tags.artist.as_deref()),
        album: clean(tags.album.as_deref()),
        duration_ms: layout.as_ref().map(|l| l.duration_ms()),
        codec: layout
            .as_ref()
            .and_then(|l| l.dsd_rate_multiple())
            .map(|m| format!("DSD{m}"))
            .or_else(|| Some("DSD".into())),
        sample_rate: layout.as_ref().map(|l| l.sample_rate_hz),
        bit_depth: Some(1),
        bitrate_kbps: layout.as_ref().map(dsd_bitrate_kbps),
    }
}

/// A DSD stream's bitrate: one bit per sample and channel, at a constant
/// rate (DSD64 stereo is 5 645 kbps), so the header says it exactly.
fn dsd_bitrate_kbps(layout: &DsdLayout) -> u32 {
    let bits = u64::from(layout.sample_rate_hz) * u64::from(layout.channels.count());
    u32::try_from((bits + 500) / 1000).unwrap_or(u32::MAX)
}

/// The embedded front cover, else any embedded picture, else a cover image
/// sitting next to the file. Decoded and bounded to [`COVER_MAX_PX`].
pub fn read_cover(path: &Path) -> Option<CoverImage> {
    let embedded = read_tagged_file(path).ok().and_then(|tagged| {
        let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
        let pictures = tag.pictures();
        let pic = pictures
            .iter()
            .find(|p| p.pic_type() == PictureType::CoverFront)
            .or_else(|| pictures.first())?;
        decode_cover(pic.data())
    });
    embedded.or_else(|| {
        folder_cover(path)
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| decode_cover(&b))
    })
}

fn folder_cover(track: &Path) -> Option<PathBuf> {
    const NAMES: &[&str] = &["cover", "folder", "front", "album", "artwork"];
    const EXTS: &[&str] = &["jpg", "jpeg", "png", "webp"];
    let dir = track.parent()?;
    let entries = std::fs::read_dir(dir).ok()?;
    let mut best: Option<(usize, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
            continue;
        };
        if !EXTS.iter().any(|e| e.eq_ignore_ascii_case(ext)) {
            continue;
        }
        let stem = stem.to_ascii_lowercase();
        if let Some(rank) = NAMES.iter().position(|n| *n == stem) {
            if best.as_ref().is_none_or(|(r, _)| rank < *r) {
                best = Some((rank, path));
            }
        }
    }
    best.map(|(_, p)| p)
}

fn decode_cover(bytes: &[u8]) -> Option<CoverImage> {
    let image = image::load_from_memory(bytes)
        .map_err(|err| tracing::debug!(%err, "cover decode failed"))
        .ok()?;
    let image = if image.width() > COVER_MAX_PX || image.height() > COVER_MAX_PX {
        image.thumbnail(COVER_MAX_PX, COVER_MAX_PX)
    } else {
        image
    };
    let rgba = image.to_rgba8();
    let (w, h) = rgba.dimensions();
    Some(CoverImage {
        size: [w as usize, h as usize],
        rgba: rgba.into_raw(),
    })
}

/// Results of background reads, keyed by the queue entry they belong to.
pub enum LoaderEvent {
    Meta {
        id: u64,
        meta: TrackMeta,
    },
    Cover {
        id: u64,
        image: Option<CoverImage>,
    },
    /// Folders expanded off the UI thread (see [`Loader::request_scan`]).
    Scanned {
        /// Which request this answers, for [`Loader::poll`]'s ordering.
        seq: u64,
        paths: Vec<PathBuf>,
        autoplay: bool,
        /// The `generation` the scan was requested with, handed back as is.
        generation: u64,
    },
}

/// Two worker threads: one walks the metadata queue in order, the other
/// decodes covers and always skips to the most recent request — the user
/// who presses Next five times wants the fifth cover, not the five. Scans
/// get a thread each (see [`Loader::request_scan`]).
pub struct Loader {
    meta_tx: Sender<(u64, PathBuf)>,
    cover_tx: Sender<(u64, PathBuf)>,
    tx: Sender<LoaderEvent>,
    notify: Arc<dyn Fn() + Send + Sync>,
    rx: Receiver<LoaderEvent>,
    next_scan: u64,
    /// Scans not handed out yet, oldest first, and since when the oldest
    /// has been waited for (running, not waiting for a thread).
    scan_order: VecDeque<u64>,
    head_since: Instant,
    /// Scans done that wait for an older one.
    scans_done: HashMap<u64, LoaderEvent>,
    /// Scan threads started and not yet answered, stuck ones included.
    scans_running: usize,
    /// Scans asked for while [`MAX_SCANS`] were running, oldest first.
    scans_waiting: VecDeque<ScanRequest>,
}

/// A scan waiting for a thread.
struct ScanRequest {
    seq: u64,
    inputs: Vec<PathBuf>,
    autoplay: bool,
    generation: u64,
}

/// How long a scan may keep the ones asked for after it waiting: past
/// this, a later scan that is done is handed out, and the slow one (a
/// share that does not answer) when it comes.
const SCAN_HOLD: Duration = Duration::from_secs(2);

/// Most scan threads at once: a share that does not answer can hold one
/// for minutes, and a burst of launches asks for one each. The others
/// wait their turn, in order.
const MAX_SCANS: usize = 8;

impl Loader {
    pub fn new(notify: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx, rx) = unbounded::<LoaderEvent>();
        let (meta_tx, meta_rx) = unbounded::<(u64, PathBuf)>();
        let (cover_tx, cover_rx) = unbounded::<(u64, PathBuf)>();

        let (tx_events, notify_events) = (tx.clone(), notify.clone());
        let (tx_meta, notify_meta) = (tx.clone(), notify.clone());
        if let Err(err) = std::thread::Builder::new()
            .name("quickplay-metadata".into())
            .spawn(move || {
                for (id, path) in meta_rx {
                    let meta = read_meta(&path);
                    if tx_meta.send(LoaderEvent::Meta { id, meta }).is_err() {
                        break;
                    }
                    notify_meta();
                }
            })
        {
            tracing::error!(%err, "metadata thread failed to start");
        }

        if let Err(err) = std::thread::Builder::new()
            .name("quickplay-artwork".into())
            .spawn(move || {
                while let Ok(mut request) = cover_rx.recv() {
                    // Collapse a burst to its newest request.
                    while let Ok(newer) = cover_rx.try_recv() {
                        request = newer;
                    }
                    let (id, path) = request;
                    let image = read_cover(&path);
                    if tx.send(LoaderEvent::Cover { id, image }).is_err() {
                        break;
                    }
                    notify();
                }
            })
        {
            tracing::error!(%err, "artwork thread failed to start");
        }

        Self {
            meta_tx,
            cover_tx,
            tx: tx_events,
            notify: notify_events,
            rx,
            next_scan: 0,
            scan_order: VecDeque::new(),
            head_since: Instant::now(),
            scans_done: HashMap::new(),
            scans_running: 0,
            scans_waiting: VecDeque::new(),
        }
    }

    /// Starts the thread of one scan.
    fn spawn_scan(&mut self, request: ScanRequest) -> std::io::Result<()> {
        let ScanRequest {
            seq,
            inputs,
            autoplay,
            generation,
        } = request;
        let (tx, notify) = (self.tx.clone(), self.notify.clone());
        std::thread::Builder::new()
            .name("quickplay-scan".into())
            .spawn(move || {
                let event = LoaderEvent::Scanned {
                    seq,
                    paths: collect_audio_paths(&inputs),
                    autoplay,
                    generation,
                };
                if tx.send(event).is_ok() {
                    notify();
                }
            })?;
        self.scans_running += 1;
        // The wait for the oldest counts from when it runs.
        if self.scan_order.front() == Some(&seq) {
            self.head_since = Instant::now();
        }
        Ok(())
    }

    /// Starts the scans that wait, as threads free up. One whose thread
    /// cannot start is done here rather than lost.
    fn start_waiting_scans(&mut self) {
        while self.scans_running < MAX_SCANS {
            let Some(request) = self.scans_waiting.pop_front() else {
                break;
            };
            let (seq, autoplay, generation) = (request.seq, request.autoplay, request.generation);
            let inputs = request.inputs.clone();
            if let Err(err) = self.spawn_scan(request) {
                tracing::warn!(%err, "scan thread failed to start; scanning here");
                let event = LoaderEvent::Scanned {
                    seq,
                    paths: collect_audio_paths(&inputs),
                    autoplay,
                    generation,
                };
                self.scans_done.insert(seq, event);
            }
        }
    }

    /// Expands files and folders on a thread of the request's own: a
    /// dropped library root can hold tens of thousands of entries, and
    /// checking a file can read its header (Ogg, WavPack, Monkey's Audio),
    /// which on a slow share would freeze the window on the UI thread, and
    /// on a shared thread hold up every add after it. [`Loader::poll`]
    /// hands the results out in the order they were asked for.
    /// `generation` comes back with the result, so the caller can tell a
    /// scan it has since given up on. At most [`MAX_SCANS`] run at once;
    /// the next ones wait their turn. An error when no thread could start
    /// for a scan that had one free.
    pub fn request_scan(
        &mut self,
        inputs: Vec<PathBuf>,
        autoplay: bool,
        generation: u64,
    ) -> std::io::Result<()> {
        let seq = self.next_scan;
        let request = ScanRequest {
            seq,
            inputs,
            autoplay,
            generation,
        };
        if self.scans_running < MAX_SCANS && self.scans_waiting.is_empty() {
            self.scan_order.push_back(seq);
            if let Err(err) = self.spawn_scan(request) {
                self.scan_order.pop_back();
                return Err(err);
            }
        } else {
            self.scan_order.push_back(seq);
            self.scans_waiting.push_back(request);
        }
        self.next_scan += 1;
        Ok(())
    }

    /// Stops ordering the scans under way, for a queue that was cleared:
    /// their results are handed out as they come, and dropped by their
    /// generation. Those still waiting for a thread are not started.
    pub fn forget_scans(&mut self) {
        self.scan_order.clear();
        self.scans_done.clear();
        self.scans_waiting.clear();
    }

    /// How long until [`Loader::poll`] stops keeping finished scans behind
    /// a slow one, when it keeps any.
    pub fn scan_wait(&self) -> Option<Duration> {
        (!self.scans_done.is_empty()).then(|| SCAN_HOLD.saturating_sub(self.head_since.elapsed()))
    }

    pub fn request_meta(&self, id: u64, path: PathBuf) {
        let _ = self.meta_tx.send((id, path));
    }

    pub fn request_cover(&self, id: u64, path: PathBuf) {
        let _ = self.cover_tx.send((id, path));
    }

    /// Everything the workers have finished. Scans come in the order they
    /// were asked for, except that one still running after [`SCAN_HOLD`]
    /// no longer holds back the ones done after it.
    pub fn poll(&mut self) -> Vec<LoaderEvent> {
        let mut out = Vec::new();
        let received: Vec<LoaderEvent> = self.rx.try_iter().collect();
        for event in received {
            if matches!(event, LoaderEvent::Scanned { .. }) {
                self.scans_running = self.scans_running.saturating_sub(1);
            }
            match &event {
                LoaderEvent::Scanned { seq, .. } if self.scan_order.contains(seq) => {
                    self.scans_done.insert(*seq, event);
                }
                _ => out.push(event),
            }
        }
        self.start_waiting_scans();
        while let Some(&head) = self.scan_order.front() {
            if let Some(event) = self.scans_done.remove(&head) {
                out.push(event);
            } else if self.scans_done.is_empty() || self.head_since.elapsed() < SCAN_HOLD {
                break;
            }
            // Handed out, or given up on waiting for: the next one is first.
            self.scan_order.pop_front();
            self.head_since = Instant::now();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{collect_audio_paths, dsd_bitrate_kbps, entry_path, percent_decode};
    #[cfg(windows)]
    use super::{location, may_follow, Location};
    use std::path::{Path, PathBuf};
    use waveflow_core::audio_format::dsd::parser::{
        DsdChannels, DsdContainer, DsdLayout, DsdSpeakers,
    };

    #[cfg(windows)]
    #[test]
    fn a_share_is_followed_only_from_a_playlist_on_it() {
        let at = |p: &str| location(Path::new(p));
        assert_eq!(at(r"C:\Music\a.flac"), Location::Local);
        assert_eq!(at(r"\\NAS\music\a.flac"), Location::Share("nas".into()));
        assert_eq!(at("//nas/music/a.flac"), Location::Share("nas".into()));
        assert_eq!(at(r"\\?\UNC\nas\music\a.flac"), Location::Device);
        assert_eq!(at(r"\\?\C:\Music\a.flac"), Location::Local);
        assert_eq!(at(r"\\.\pipe\x"), Location::Device);
        assert_eq!(at(r"\\attacker"), Location::Device);
        assert_eq!(
            at(r"\/attacker/x/a.flac"),
            Location::Share("attacker".into())
        );
        // Resolved against the current directory, a drive or a share.
        let here = location(&std::env::current_dir().unwrap());
        assert_ne!(here, Location::Device);
        assert_eq!(at("a.flac"), here);
        assert_eq!(at(r"\a.flac"), here);
        let local = Location::Local;
        let nas = at(r"\\nas\music\list.m3u");
        // From a playlist in Downloads: local files only.
        assert!(may_follow(&at(r"D:\a.flac"), &local));
        assert!(!may_follow(&at(r"\\attacker\x\a.flac"), &local));
        assert!(!may_follow(&at(r"\\.\pipe\x"), &local));
        // From one on the NAS: its own files, no other host's.
        assert!(may_follow(&at(r"\\nas\other\a.flac"), &nas));
        assert!(!may_follow(&at(r"\\attacker\x\a.flac"), &nas));
        // A share named through a file:// URL is the same share.
        let url = entry_path(
            "file://attacker/x/a.flac",
            Path::new(r"C:\Users\me\Downloads"),
        );
        assert!(!may_follow(&location(&url.unwrap()), &local));
    }

    #[test]
    fn a_playlist_gives_its_files_in_its_order() {
        let dir = std::env::temp_dir().join(format!("qp-m3u-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        for name in ["a.flac", "sub/b.mp3", "sub/e.wav", "c d.ogg", "notes.txt"] {
            std::fs::write(dir.join(name), b"").unwrap();
        }
        // The URL form a player writes: forward slashes, a space escaped.
        let c = dir.join("c d.ogg").to_string_lossy().replace('\\', "/");
        let c_url = format!(
            "file://{}{}",
            if c.starts_with('/') { "" } else { "/" },
            c.replace(' ', "%20")
        );
        let list = format!(
            "\u{feff}#EXTM3U\n#EXTINF:12,Someone - B\nsub/b.mp3\nsub\\e.wav\n{c_url}\nhttps://radio.example/stream\nmissing.flac\nnotes.txt\n{}\n",
            dir.join("a.flac").display()
        );
        let m3u = dir.join("list.m3u8");
        std::fs::write(&m3u, list).unwrap();
        // Relative (either separator), URL and absolute entries; the web
        // address, the missing file and the text file are left out.
        let got = collect_audio_paths(&[m3u]);
        let names: Vec<_> = got
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["b.mp3", "e.wav", "c d.ogg", "a.flac"]);
        assert!(got.iter().all(|p| p.is_file()));
        // A folder's playlists are not read, or their files would come twice.
        assert_eq!(collect_audio_paths(std::slice::from_ref(&dir)).len(), 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_entry_is_a_path_or_a_file_url() {
        let folder = Path::new("/music");
        assert_eq!(entry_path("a.flac", folder), Some(folder.join("a.flac")));
        assert_eq!(entry_path("http://x/y.mp3", folder), None);
        assert_eq!(entry_path("HTTPS://x/y.mp3", folder), None);
        #[cfg(not(windows))]
        {
            assert_eq!(
                entry_path("file:///home/me/a%20b.flac", folder),
                Some(PathBuf::from("/home/me/a b.flac"))
            );
            assert_eq!(
                entry_path("file://localhost/home/a.flac", folder),
                Some(PathBuf::from("/home/a.flac"))
            );
            assert_eq!(entry_path("file://server/share/a.flac", folder), None);
        }
        #[cfg(windows)]
        {
            assert_eq!(
                entry_path("file:///C:/Music/a%20b.flac", folder),
                Some(PathBuf::from("C:/Music/a b.flac"))
            );
            assert_eq!(
                entry_path("file://nas/music/a.flac", folder),
                Some(PathBuf::from(r"\\nas\music\a.flac"))
            );
        }
    }

    #[test]
    fn escapes_are_decoded_as_utf8() {
        assert_eq!(
            percent_decode("/caf%C3%A9.flac").as_deref(),
            Some("/café.flac")
        );
        assert_eq!(percent_decode("/100%.flac").as_deref(), Some("/100%.flac"));
        assert_eq!(percent_decode("/%+1.flac").as_deref(), Some("/%+1.flac"));
        assert_eq!(percent_decode("/%FF.flac"), None);
    }

    /// Scans come back in the order asked for; one still running after
    /// `SCAN_HOLD` stops holding the others, and comes when it comes; a
    /// cleared queue's scans are handed out as they arrive.
    #[test]
    fn scans_come_back_in_order_unless_one_is_stuck() {
        use super::{Loader, LoaderEvent, SCAN_HOLD};
        let mut loader = Loader::new(std::sync::Arc::new(|| {}));
        let done = |loader: &Loader, seq: u64| {
            let event = LoaderEvent::Scanned {
                seq,
                paths: Vec::new(),
                autoplay: false,
                generation: 0,
            };
            loader.tx.send(event).unwrap();
        };
        let seqs = |events: Vec<LoaderEvent>| -> Vec<u64> {
            events
                .into_iter()
                .filter_map(|e| match e {
                    LoaderEvent::Scanned { seq, .. } => Some(seq),
                    _ => None,
                })
                .collect()
        };
        loader.scan_order.extend([0, 1]);
        done(&loader, 1);
        assert!(seqs(loader.poll()).is_empty(), "1 waits for 0");
        assert!(loader.scan_wait().is_some());
        done(&loader, 0);
        assert_eq!(seqs(loader.poll()), [0, 1]);
        assert!(loader.scan_wait().is_none());

        loader.scan_order.extend([2, 3]);
        done(&loader, 3);
        loader.head_since = std::time::Instant::now() - SCAN_HOLD;
        assert_eq!(seqs(loader.poll()), [3], "2 is stuck: 3 goes past it");
        done(&loader, 2);
        assert_eq!(seqs(loader.poll()), [2], "the stuck one when it comes");

        loader.scan_order.push_back(4);
        loader.forget_scans();
        done(&loader, 4);
        assert_eq!(seqs(loader.poll()), [4]);
    }

    /// Past `MAX_SCANS` running, a scan waits for a thread, keeps its place
    /// in the order and starts when one answers.
    #[test]
    fn scans_past_the_limit_wait_their_turn() {
        use super::{Loader, LoaderEvent, MAX_SCANS};
        let mut loader = Loader::new(std::sync::Arc::new(|| {}));
        loader.scans_running = MAX_SCANS;
        loader.request_scan(Vec::new(), false, 7).unwrap();
        assert_eq!(loader.scans_waiting.len(), 1);
        assert_eq!(loader.scan_order, [0]);
        // One of the running scans answers (for a cleared queue).
        loader
            .tx
            .send(LoaderEvent::Scanned {
                seq: 99,
                paths: Vec::new(),
                autoplay: false,
                generation: 0,
            })
            .unwrap();
        let mut got = Vec::new();
        for _ in 0..200 {
            got.extend(loader.poll().into_iter().filter_map(|e| match e {
                LoaderEvent::Scanned {
                    seq, generation, ..
                } => Some((seq, generation)),
                _ => None,
            }));
            if got.len() == 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(loader.scans_waiting.is_empty());
        assert_eq!(got, [(99, 0), (0, 7)]);
        assert_eq!(loader.scans_running, MAX_SCANS - 1);
    }

    fn layout(sample_rate_hz: u32, channels: DsdChannels) -> DsdLayout {
        DsdLayout {
            container: DsdContainer::Dsf,
            channels,
            speakers: DsdSpeakers::DsfType(2),
            sample_rate_hz,
            samples_per_channel: 0,
            data_offset: 0,
            data_len_bytes: 0,
            block_interleave: None,
            lsb_first: false,
        }
    }

    #[test]
    fn a_dsd_bitrate_is_one_bit_per_sample_and_channel() {
        assert_eq!(
            dsd_bitrate_kbps(&layout(2_822_400, DsdChannels::Stereo)),
            5_645
        );
        assert_eq!(
            dsd_bitrate_kbps(&layout(5_644_800, DsdChannels::Stereo)),
            11_290
        );
        assert_eq!(
            dsd_bitrate_kbps(&layout(2_822_400, DsdChannels::Multi(6))),
            16_934
        );
    }
}
