//! Reading what a file says about itself — tags, duration, the embedded or
//! folder cover — off the UI thread, through the same `waveflow-core`
//! helpers the desktop scanner uses.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossbeam_channel::{unbounded, Receiver, Sender};
use lofty::picture::PictureType;
use lofty::prelude::*;
use lofty::tag::ItemKey;
use waveflow_core::audio_format::dsd::metadata::read_metadata as read_dsd_metadata;
use waveflow_core::audio_format::dsd::parser::{parse_dff, parse_dsf, DsdContainer};
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

pub fn audio_extensions() -> &'static [&'static str] {
    AUDIO_EXTENSIONS
}

/// Expand files and folders into the audio files they hold, in a stable
/// order. Folders are walked a few levels deep; non-audio files are skipped.
pub fn collect_audio_paths(inputs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for input in inputs {
        if input.is_dir() {
            let mut found = Vec::new();
            walk(input, 0, &mut found);
            found.sort();
            out.extend(found);
        } else if is_scannable_audio(input) {
            out.push(input.clone());
        }
    }
    out
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
        bitrate_kbps: None,
    }
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
    Meta { id: u64, meta: TrackMeta },
    Cover { id: u64, image: Option<CoverImage> },
}

/// Two worker threads: one walks the metadata queue in order, the other
/// decodes covers and always skips to the most recent request — the user
/// who presses Next five times wants the fifth cover, not the five.
pub struct Loader {
    meta_tx: Sender<(u64, PathBuf)>,
    cover_tx: Sender<(u64, PathBuf)>,
    rx: Receiver<LoaderEvent>,
}

impl Loader {
    pub fn new(notify: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx, rx) = unbounded::<LoaderEvent>();
        let (meta_tx, meta_rx) = unbounded::<(u64, PathBuf)>();
        let (cover_tx, cover_rx) = unbounded::<(u64, PathBuf)>();

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
            rx,
        }
    }

    pub fn request_meta(&self, id: u64, path: PathBuf) {
        let _ = self.meta_tx.send((id, path));
    }

    pub fn request_cover(&self, id: u64, path: PathBuf) {
        let _ = self.cover_tx.send((id, path));
    }

    pub fn poll(&self) -> Vec<LoaderEvent> {
        self.rx.try_iter().collect()
    }
}
