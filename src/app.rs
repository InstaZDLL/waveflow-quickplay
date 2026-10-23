//! The application: owns the queue, the (lazily created) audio engine, the
//! background loader and the window state; drives the UI and applies its
//! actions.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{
    Pos2, Rect, TextureHandle, Vec2, ViewportBuilder, ViewportClass, ViewportCommand, ViewportId,
};

use crate::audio::{AudioCmd, AudioEngine, EngineEvent, PlayerState};
use crate::capture::Capture;
use crate::config::{Repeat, Settings, MIN_WIDTH};
use crate::i18n::{fill, Lang, Strings};
use crate::ipc;
use crate::library::{self, Loader, LoaderEvent};
use crate::lyrics::Lyrics;
use crate::queue::Queue;
use crate::ui::widgets::{self, TitleAction};
use crate::ui::{
    player, playlist, settings as settings_page, theme, Action, PlayerView, COMPACT_HEIGHT,
    CONTROLS_HEIGHT, TITLE_HEIGHT,
};
use crate::waveflow;

pub const APP_ID: &str = "app.waveflow.quickplay";
pub const MAIN_TITLE: &str = "WaveFlow QuickPlay";
pub const PLAYLIST_TITLE: &str = "Playlist · WaveFlow QuickPlay";
/// The name painted in the player's title bar. The logo beside it already
/// says WaveFlow and the about block gives the full name, so the bar keeps
/// the room; the OS title stays [`MAIN_TITLE`], which the playlist uses to
/// find the player.
const BAR_TITLE: &str = "QuickPlay";
/// A playlist that lands within this distance of where the player sent it
/// is our own move arriving, not the user dragging it.
const DOCK_TOLERANCE: f32 = 2.0;
/// Space left between the player and a freshly opened playlist.
const DOCK_GAP: f32 = 8.0;

/// Position-display cadence while playing — the desktop's `player:position`
/// rate. Idle and paused repaint only on input.
const POSITION_TICK: Duration = Duration::from_millis(250);

/// How long a status line stays visible.
const STATUS_TTL: Duration = Duration::from_secs(6);

/// Visualizer cadence while bars are moving (~30 Hz).
const SPECTRUM_TICK: Duration = Duration::from_millis(33);
/// Time constants of the bar easing, in seconds: bars rise almost at
/// once and fall over a few hundred milliseconds.
const SPECTRUM_ATTACK: f32 = 0.035;
const SPECTRUM_RELEASE: f32 = 0.16;

/// Pressing Previous inside the first seconds goes to the previous track;
/// later it restarts the current one.
const PREV_RESTART_THRESHOLD_MS: u64 = 3_000;

/// The window's largest allowed inner size (winit wants finite numbers).
const MAX_WINDOW: f32 = 8192.0;

pub struct QuickPlayApp {
    settings: Settings,
    queue: Queue,
    engine: Option<AudioEngine>,
    loader: Loader,
    ctx: egui::Context,

    /// The load most recently sent; events from older loads are ignored.
    generation: u64,
    /// Queue entry the current generation plays.
    playing_id: Option<u64>,
    state: PlayerState,
    duration_ms: Option<u64>,
    codec: Option<String>,
    seek_preview: Option<f32>,

    cover: Option<TextureHandle>,
    cover_for: Option<u64>,
    cover_requested: Option<u64>,

    selected: Option<usize>,
    status: Option<(String, Instant)>,
    /// Consecutive tracks that failed while auto-advancing.
    fail_streak: usize,
    /// Where the playlist window was first placed (beside the player).
    playlist_pos: Option<Pos2>,
    window_mode_applied: bool,
    /// Last known minimized state of the main window, so the playlist can
    /// follow it down and back up (as PotPlayer does with its playlist).
    root_minimized: bool,
    /// The main window has reported a position at least once; before that
    /// a missing `outer_rect` says nothing about minimization (macOS path).
    root_placed: bool,
    /// Interface language, resolved from the settings (or the system).
    lang: Lang,
    /// The settings page is shown in place of the controls.
    settings_open: bool,
    /// The playlist window is owned by the player (one taskbar button).
    playlist_attached: bool,
    /// Where the playlist sits relative to the player's top-right corner;
    /// the player carries it along when it moves or widens.
    playlist_offset: Option<Vec2>,
    /// The player's top-right corner last frame.
    dock_anchor: Option<Pos2>,
    /// The playlist's position last frame, and where we last sent it.
    playlist_last_pos: Option<Pos2>,
    playlist_expected_pos: Option<Pos2>,
    /// A reopened playlist window goes back to the remembered offset on its
    /// first frame (distinct from `dock_anchor`, which is also cleared when
    /// the player is minimized).
    playlist_place_pending: bool,
    /// Generation whose `Loaded` event has arrived; state events before
    /// that belong to the previous track.
    loaded_generation: u64,
    /// Where the pending load starts. Until its `Loaded` arrives, the
    /// engine's clock still counts the previous track.
    load_start_ms: u64,
    /// Bumped by every `clear`: a folder scan requested before it belongs to
    /// a queue that is gone, and its result is dropped when it arrives.
    scan_generation: u64,
    /// Synced lyrics of the current track, from its `.lrc` sidecar.
    lyrics: Option<Lyrics>,
    /// Test-only window capture, armed by `QUICKPLAY_CAPTURE_DIR`.
    capture: Option<Capture>,
    /// Single-instance socket; `None` when another instance holds it.
    ipc: Option<ipc::Server>,
    /// When the last remote `play` arrived: a burst of them (Explorer
    /// launches one process per selected file) must not clear the queue
    /// for each file after the first.
    last_remote_play: Option<Instant>,
    /// Smoothed bar heights shown by the visualizer.
    spectrum: Vec<f32>,
    /// The last frame taken, which the bars ease toward.
    spectrum_target: Vec<f32>,
    spectrum_tick: Instant,
    /// Xruns already logged; the output only counts them.
    xruns_logged: u64,
    /// WaveFlow's executable, looked for in the background: the player
    /// offers to carry on there when it is installed.
    waveflow: waveflow::Finder,
}

impl QuickPlayApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        settings: Settings,
        files: Vec<PathBuf>,
        ipc: Option<ipc::Server>,
    ) -> Self {
        theme::apply(&cc.egui_ctx);
        let ctx = cc.egui_ctx.clone();
        if let Some(server) = &ipc {
            server.attach(&ctx);
        }
        #[cfg(target_os = "macos")]
        crate::macos::set_repaint(&ctx);
        let notify_ctx = ctx.clone();
        let notify: Arc<dyn Fn() + Send + Sync> = Arc::new(move || notify_ctx.request_repaint());
        let lang = Lang::resolve(&settings.language);
        let mut queue = Queue::default();
        queue.set_repeat(settings.repeat);
        queue.set_shuffle(settings.shuffle);
        let mut app = Self {
            queue,
            engine: None,
            loader: Loader::new(notify),
            ctx,
            generation: 0,
            playing_id: None,
            state: PlayerState::Idle,
            duration_ms: None,
            codec: None,
            seek_preview: None,
            cover: None,
            cover_for: None,
            cover_requested: None,
            selected: None,
            status: None,
            fail_streak: 0,
            playlist_pos: None,
            window_mode_applied: false,
            root_minimized: false,
            root_placed: false,
            lang,
            settings_open: false,
            playlist_attached: false,
            playlist_offset: None,
            dock_anchor: None,
            playlist_last_pos: None,
            playlist_expected_pos: None,
            playlist_place_pending: false,
            lyrics: None,
            loaded_generation: 0,
            load_start_ms: 0,
            scan_generation: 0,
            capture: Capture::from_env(),
            ipc,
            // Files given on the command line count as the first `play` of a
            // burst: when Explorer starts one process per selected file, the
            // ones that hand over to us must join these, not replace them.
            last_remote_play: (!files.is_empty()).then(Instant::now),
            spectrum: vec![0.0; crate::audio::spectrum::BAND_COUNT],
            spectrum_target: vec![0.0; crate::audio::spectrum::BAND_COUNT],
            spectrum_tick: Instant::now(),
            xruns_logged: 0,
            waveflow: waveflow::Finder::spawn({
                let ctx = cc.egui_ctx.clone();
                move || ctx.request_repaint()
            }),
            settings,
        };
        if !files.is_empty() {
            app.add_paths(&files, true);
        }
        app
    }

    // ── Engine ──────────────────────────────────────────────────────────

    fn ensure_engine(&mut self) -> Option<&AudioEngine> {
        if self.engine.is_none() {
            let ctx = self.ctx.clone();
            let notify: Arc<dyn Fn() + Send + Sync> = Arc::new(move || ctx.request_repaint());
            match AudioEngine::new(notify) {
                Ok(engine) => {
                    engine.send(AudioCmd::SetVolume(self.settings.volume));
                    engine.shared().spectrum_enabled.store(
                        self.settings.show_spectrum,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                    tracing::info!(device = ?engine.device_name(), "audio engine started");
                    self.engine = Some(engine);
                }
                Err(err) => {
                    tracing::error!(%err, "audio engine unavailable");
                    let message = fill(
                        self.t().status_audio_unavailable,
                        &[("e", &err.to_string())],
                    );
                    self.set_status(message);
                }
            }
        }
        self.engine.as_ref()
    }

    fn send(&mut self, cmd: AudioCmd) {
        if let Some(engine) = self.ensure_engine() {
            engine.send(cmd);
        }
    }

    /// Carry on in WaveFlow: hand over the queue, the track, the position
    /// and whether it was playing, then stop and close. On failure the
    /// player stays as it was and says why.
    fn open_in_waveflow(&mut self) {
        let Some(exe) = self.waveflow.exe().map(PathBuf::from) else {
            return;
        };
        if self.queue.is_empty() {
            return;
        }
        let paths = (0..self.queue.len())
            .filter_map(|i| self.queue.get(i))
            .map(|e| std::path::absolute(&e.path).unwrap_or_else(|_| e.path.clone()))
            .collect();
        let index = self.queue.current_index().unwrap_or(0);
        // Before the pending load's `Loaded`, the engine's clock is still the
        // previous track's: the new one starts where it was asked to.
        let position = if self.loaded_generation == self.generation {
            self.engine.as_ref().map_or(0, |e| e.shared().position_ms())
        } else {
            self.load_start_ms
        };
        // Stopped or at the end: WaveFlow gets the track, from its start,
        // and waits; playing or loading: it carries on.
        let (position_ms, playing) = match self.state {
            PlayerState::Playing | PlayerState::Loading => (position, true),
            PlayerState::Paused => (position, false),
            PlayerState::Idle | PlayerState::Ended => (0, false),
        };
        let handoff = waveflow_core::handoff::Handoff::new(paths, index, position_ms, playing);
        match waveflow::hand_over(&exe, &handoff) {
            Ok(()) => {
                tracing::info!(exe = %exe.display(), index, position_ms, playing, "handed over to WaveFlow");
                // Silence at once rather than when the window has gone, so
                // the two players are not heard together.
                if let Some(engine) = &self.engine {
                    engine.send(AudioCmd::Stop);
                }
                self.ctx
                    .send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close);
            }
            Err(err) => {
                tracing::error!(%err, exe = %exe.display(), "handing over to WaveFlow failed");
                let message = fill(
                    self.t().status_cannot_open_waveflow,
                    &[("e", &err.to_string())],
                );
                self.set_status(message);
            }
        }
    }

    /// The strings of the interface language in force.
    fn t(&self) -> &'static Strings {
        self.lang.strings()
    }

    fn set_status(&mut self, message: impl Into<String>) {
        self.status = Some((message.into(), Instant::now()));
    }

    // ── Queue ───────────────────────────────────────────────────────────

    /// Expand `inputs` and append them. With `autoplay`, start the first new
    /// row when nothing is playing.
    fn add_paths(&mut self, inputs: &[PathBuf], autoplay: bool) {
        if inputs.iter().any(|p| p.is_dir()) {
            // Folders are walked on the loader's thread; the result comes
            // back as `LoaderEvent::Scanned`.
            self.set_status(self.t().status_scanning);
            self.loader
                .request_scan(inputs.to_vec(), autoplay, self.scan_generation);
            return;
        }
        let paths = library::collect_audio_paths(inputs);
        self.add_resolved(paths, !inputs.is_empty(), autoplay);
    }

    /// Queues already-expanded audio paths.
    fn add_resolved(&mut self, paths: Vec<PathBuf>, had_inputs: bool, autoplay: bool) {
        if paths.is_empty() {
            if had_inputs {
                self.set_status(self.t().status_no_audio_files);
            }
            return;
        }
        let first_new = self.queue.len();
        for path in paths {
            let id = self.queue.push(path.clone());
            self.loader.request_meta(id, path);
        }
        let idle = matches!(self.state, PlayerState::Idle | PlayerState::Ended);
        if autoplay && idle {
            self.play_index(first_new, 0, true);
        }
    }

    /// Play row `index`: the cursor moves on to it (Next, a track's end,
    /// a pick from the list, a skipped failure).
    fn play_index(&mut self, index: usize, start_ms: u64, autoplay: bool) {
        self.play_index_from(index, start_ms, autoplay, false);
    }

    /// Play the current track again from `start_ms` (its end under "repeat
    /// one", Play after the end, a seek after the end, Previous in the
    /// first seconds). The cursor does not move, so neither the shuffle
    /// history nor the round changes: a round that was over stays over.
    fn restart_current(&mut self, start_ms: u64) {
        if let Some(i) = self.queue.current_index() {
            self.play_index_from(i, start_ms, true, true);
        }
    }

    /// `back`: the cursor goes back to `index` (Previous, or the current
    /// track again), unwinding the shuffle history to it instead of
    /// recording it again.
    fn play_index_from(&mut self, index: usize, start_ms: u64, autoplay: bool, back: bool) {
        let Some(entry) = self.queue.get(index) else {
            return;
        };
        let (id, path) = (entry.id, entry.path.clone());
        let meta_duration = entry.duration_ms();
        if back {
            self.queue.set_current_back(index);
        } else {
            self.queue.set_current(Some(index));
        }
        self.selected = Some(index);
        self.generation += 1;
        self.playing_id = Some(id);
        self.duration_ms = meta_duration;
        self.codec = None;
        self.seek_preview = None;
        self.state = PlayerState::Loading;
        self.load_start_ms = start_ms;
        self.lyrics = Lyrics::load_for(&path);
        let generation = self.generation;
        self.send(AudioCmd::Load {
            generation,
            path,
            start_ms,
            autoplay,
        });
        self.request_cover_for_current();
    }

    fn request_cover_for_current(&mut self) {
        let Some(entry) = self.queue.current() else {
            return;
        };
        if self.cover_for == Some(entry.id) || self.cover_requested == Some(entry.id) {
            return;
        }
        self.cover_requested = Some(entry.id);
        self.loader.request_cover(entry.id, entry.path.clone());
    }

    /// The track played to its end: the same one again under "repeat one",
    /// the next one otherwise. Only here; the Next button always moves on.
    fn track_ended(&mut self) {
        if self.queue.repeat() == Repeat::One && self.queue.current_index().is_some() {
            self.restart_current(0);
            return;
        }
        self.next();
    }

    fn next(&mut self) {
        match self.queue.next_index() {
            Some(i) => self.play_index(i, 0, true),
            None => {
                // End of the queue: stop, keep the cursor on the last track.
                self.send(AudioCmd::Stop);
                self.state = PlayerState::Ended;
            }
        }
    }

    fn prev(&mut self) {
        let position = self.engine.as_ref().map_or(0, |e| e.shared().position_ms());
        if position > PREV_RESTART_THRESHOLD_MS || self.queue.prev_index().is_none() {
            self.restart_current(0);
        } else if let Some(i) = self.queue.prev_index() {
            self.play_index_from(i, 0, true, true);
        }
    }

    fn toggle_play(&mut self) {
        match self.state {
            PlayerState::Playing => self.send(AudioCmd::Pause),
            PlayerState::Paused | PlayerState::Loading => self.send(AudioCmd::Resume),
            PlayerState::Idle | PlayerState::Ended => {
                if self.queue.current_index().is_some() {
                    self.restart_current(0);
                } else if let Some(i) = self.queue.next_index() {
                    self.play_index(i, 0, true);
                }
            }
        }
    }

    fn seek_fraction(&mut self, fraction: f32) {
        let Some(duration) = self.duration_ms.filter(|d| *d > 0) else {
            return;
        };
        let ms = (fraction.clamp(0.0, 1.0) as f64 * duration as f64) as u64;
        match self.state {
            PlayerState::Playing | PlayerState::Paused | PlayerState::Loading => {
                self.send(AudioCmd::Seek(ms));
            }
            // After a natural end the decoder is idle: reload at the offset.
            PlayerState::Ended | PlayerState::Idle => self.restart_current(ms),
        }
    }

    fn set_volume(&mut self, volume: f32) {
        let v = volume.clamp(0.0, 1.0);
        self.settings.volume = v;
        if let Some(engine) = &self.engine {
            engine.send(AudioCmd::SetVolume(v));
        }
    }

    fn remove(&mut self, index: usize) {
        let was_current = self.queue.remove(index);
        if was_current && self.playing_id.is_some() {
            // The playing row is gone: stop, and let Next move on.
            self.send(AudioCmd::Stop);
            self.playing_id = None;
            self.state = PlayerState::Idle;
            self.duration_ms = None;
            self.cover = None;
            self.cover_for = None;
            // The cursor now points at the row that moved up — Play resumes
            // from it, which is what a list does when you delete the
            // current entry.
            if let Some(c) = self.queue.current_index() {
                self.queue.set_current(Some(c));
            }
        }
        self.selected = match self.selected {
            Some(s) if s == index => (index < self.queue.len()).then_some(index),
            Some(s) if s > index => Some(s - 1),
            other => other,
        };
    }

    fn clear(&mut self) {
        self.queue.clear();
        self.scan_generation += 1;
        self.selected = None;
        self.playing_id = None;
        self.duration_ms = None;
        self.codec = None;
        self.cover = None;
        self.cover_for = None;
        self.state = PlayerState::Idle;
        if let Some(engine) = &self.engine {
            engine.send(AudioCmd::Stop);
        }
    }

    fn open_dialog(&mut self) {
        let t = self.t();
        let dialog = rfd::FileDialog::new()
            .set_title(t.dialog_open_title)
            .add_filter(t.dialog_filter_audio, library::audio_extensions());
        if let Some(paths) = dialog.pick_files() {
            self.add_paths(&paths, true);
        }
    }

    // ── Events ──────────────────────────────────────────────────────────

    fn pump_engine_events(&mut self) {
        if let Some(engine) = &self.engine {
            let xruns = engine
                .shared()
                .xruns
                .load(std::sync::atomic::Ordering::Relaxed);
            if xruns > self.xruns_logged {
                tracing::debug!(total = xruns, "audio xrun, the stream carried on");
            }
            self.xruns_logged = xruns;
        }
        let events = match &self.engine {
            Some(engine) => engine.poll_events(),
            None => return,
        };
        for event in events {
            match event {
                EngineEvent::Loaded {
                    generation,
                    duration_ms,
                    codec,
                } if generation == self.generation => {
                    if let Some(d) = duration_ms.filter(|d| *d > 0) {
                        self.duration_ms = Some(d);
                        // Give the playlist the authoritative duration too.
                        if let Some(id) = self.playing_id {
                            if let Some(entry) = self.queue.entry_mut(id) {
                                if let Some(meta) = entry.meta.as_mut() {
                                    meta.duration_ms = Some(d);
                                }
                            }
                        }
                    }
                    self.codec = codec;
                    self.fail_streak = 0;
                    self.loaded_generation = generation;
                    self.status = None;
                }
                EngineEvent::Loaded { .. } => {}
                EngineEvent::State(state) => {
                    // `State` carries no generation: while a load is pending
                    // and its `Loaded` has not arrived, any state event still
                    // belongs to the superseded track and is ignored.
                    let stale = self.state == PlayerState::Loading
                        && self.loaded_generation != self.generation;
                    if !stale {
                        self.state = state;
                    }
                }
                EngineEvent::Ended { generation } if generation == self.generation => {
                    self.state = PlayerState::Ended;
                    self.track_ended();
                }
                EngineEvent::Ended { .. } => {}
                EngineEvent::Error {
                    generation,
                    message,
                } => {
                    if generation != Some(self.generation) {
                        continue;
                    }
                    tracing::warn!(%message, "track failed");
                    if let Some(id) = self.playing_id {
                        if let Some(entry) = self.queue.entry_mut(id) {
                            entry.failed = Some(message.clone());
                        }
                    }
                    self.state = PlayerState::Idle;
                    let status = fill(self.t().status_cannot_play, &[("e", &message)]);
                    self.set_status(status);
                    // Skip to the next playable row, bounded so a queue of
                    // broken files cannot loop forever.
                    self.fail_streak += 1;
                    if self.fail_streak < self.queue.len().max(1) {
                        if let Some(i) = self.queue.next_index() {
                            self.play_index(i, 0, true);
                        }
                    }
                }
                EngineEvent::Output(message) => {
                    // The device is gone (unplugged, format changed): the
                    // stream is dead, so release the engine; the next play
                    // opens a fresh one on the current default device.
                    tracing::warn!(%message, "output stream lost, releasing the engine");
                    self.set_status(message);
                    self.state = PlayerState::Idle;
                    self.engine = None;
                    break;
                }
            }
        }
    }

    fn pump_loader_events(&mut self) {
        for event in self.loader.poll() {
            match event {
                LoaderEvent::Scanned {
                    paths,
                    autoplay,
                    generation,
                } => {
                    // Requested for a queue that has been cleared since.
                    if generation != self.scan_generation {
                        continue;
                    }
                    self.status = None;
                    self.add_resolved(paths, true, autoplay);
                }
                LoaderEvent::Meta { id, meta } => {
                    let is_playing = self.playing_id == Some(id);
                    if is_playing && self.duration_ms.is_none() {
                        self.duration_ms = meta.duration_ms;
                    }
                    if let Some(entry) = self.queue.entry_mut(id) {
                        entry.meta = Some(meta);
                    }
                }
                LoaderEvent::Cover { id, image } => {
                    if self.cover_requested == Some(id) {
                        self.cover_requested = None;
                    }
                    if self.playing_id != Some(id) && self.queue.current().map(|e| e.id) != Some(id)
                    {
                        continue;
                    }
                    self.cover_for = Some(id);
                    self.cover = image.map(|img| {
                        let color = egui::ColorImage::from_rgba_unmultiplied(img.size, &img.rgba);
                        self.ctx.load_texture(
                            "quickplay-cover",
                            color,
                            egui::TextureOptions::LINEAR,
                        )
                    });
                }
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if !dropped.is_empty() {
            self.add_paths(&dropped, true);
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let mut actions = Vec::new();
        collect_shortcuts(ctx, self.shortcut_snapshot(), &mut actions);
        for action in actions {
            self.apply(action);
        }
    }

    /// Pull the spectrum frame due at the output position and ease the bars
    /// toward it: instant attack, exponential release. Returns whether any
    /// bar is still moving, so the loop knows to keep repainting.
    fn advance_spectrum(&mut self) -> bool {
        if !self.settings.show_spectrum {
            return false;
        }
        let now = Instant::now();
        let dt = now
            .duration_since(self.spectrum_tick)
            .as_secs_f32()
            .clamp(0.0, 0.1);
        self.spectrum_tick = now;
        // A new frame replaces the target; otherwise the last one holds, so a
        // tick that lands between two frames does not dip the bars.
        match (&self.engine, self.state) {
            (Some(engine), PlayerState::Playing) => {
                if let Some(frame) = engine.shared().take_due_spectrum() {
                    self.spectrum_target.copy_from_slice(&frame);
                }
            }
            _ => self.spectrum_target.iter_mut().for_each(|t| *t = 0.0),
        }
        let attack = 1.0 - (-dt / SPECTRUM_ATTACK).exp();
        let release = 1.0 - (-dt / SPECTRUM_RELEASE).exp();
        let mut moving = false;
        for (bar, &goal) in self.spectrum.iter_mut().zip(&self.spectrum_target) {
            let k = if goal > *bar { attack } else { release };
            let next = *bar + (goal - *bar) * k;
            let next = if next < 0.003 && goal == 0.0 {
                0.0
            } else {
                next
            };
            if (next - *bar).abs() > 0.0005 {
                moving = true;
            }
            *bar = next;
        }
        moving || self.state == PlayerState::Playing
    }

    fn shortcut_snapshot(&self) -> ShortcutSnapshot {
        ShortcutSnapshot {
            volume: self.settings.volume,
            position_ms: self.engine.as_ref().map_or(0, |e| e.shared().position_ms()),
            duration_ms: self.duration_ms,
        }
    }

    // ── Window ──────────────────────────────────────────────────────────

    fn compact_size(&self) -> Vec2 {
        Vec2::new(self.settings.compact_width, COMPACT_HEIGHT)
    }

    /// Push the current mode's size constraints to the OS window.
    fn apply_window_mode(&mut self, ctx: &egui::Context, resize: bool) {
        if self.settings.expanded {
            ctx.send_viewport_cmd(ViewportCommand::MaxInnerSize(Vec2::splat(MAX_WINDOW)));
            ctx.send_viewport_cmd(ViewportCommand::MinInnerSize(Vec2::new(
                MIN_WIDTH,
                COMPACT_HEIGHT + 80.0,
            )));
            if resize {
                let [w, h] = self.settings.expanded_size;
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(Vec2::new(w, h)));
            }
        } else {
            ctx.send_viewport_cmd(ViewportCommand::MinInnerSize(Vec2::new(
                MIN_WIDTH,
                COMPACT_HEIGHT,
            )));
            ctx.send_viewport_cmd(ViewportCommand::MaxInnerSize(Vec2::new(
                MAX_WINDOW,
                COMPACT_HEIGHT,
            )));
            if resize {
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(self.compact_size()));
            }
        }
        ctx.send_viewport_cmd(ViewportCommand::WindowLevel(
            if self.settings.always_on_top {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            },
        ));
    }

    /// Remember the size the user left the window at, per mode.
    fn track_window_size(&mut self, ctx: &egui::Context) {
        let inner = ctx.input(|i| i.viewport().inner_rect);
        let Some(rect) = inner else {
            return;
        };
        let size = rect.size();
        if !(size.x.is_finite() && size.y.is_finite()) || size.x < MIN_WIDTH {
            return;
        }
        if self.settings.expanded {
            if size.y > COMPACT_HEIGHT + 40.0 {
                self.settings.expanded_size = [size.x, size.y];
            }
        } else {
            self.settings.compact_width = size.x;
        }
    }

    fn toggle_expanded(&mut self, ctx: &egui::Context) {
        self.settings.expanded = !self.settings.expanded;
        tracing::debug!(expanded = self.settings.expanded, "artwork mode toggled");
        if self.settings.expanded {
            // Open to a square artwork over the controls, at the current
            // width, unless a saved size exists for that width.
            let w = self.settings.compact_width;
            if (self.settings.expanded_size[0] - w).abs() > 1.0 {
                self.settings.expanded_size = [w, w + COMPACT_HEIGHT];
            }
        }
        self.apply_window_mode(ctx, true);
        self.settings.save();
    }

    fn toggle_playlist(&mut self, ctx: &egui::Context) {
        self.settings.show_playlist = !self.settings.show_playlist;
        if self.settings.show_playlist && self.playlist_pos.is_none() {
            // Beside the player, if the backend tells us where that is.
            self.playlist_pos = ctx
                .input(|i| i.viewport().outer_rect)
                .map(|r| Pos2::new(r.right() + DOCK_GAP, r.top()));
        }
        self.settings.save();
    }

    // ── Actions ─────────────────────────────────────────────────────────

    fn apply(&mut self, action: Action) {
        let ctx = self.ctx.clone();
        match action {
            Action::TogglePlay => self.toggle_play(),
            Action::Next => self.next(),
            Action::Prev => self.prev(),
            Action::SeekPreview(f) => self.seek_preview = Some(f),
            Action::SeekCommit(f) => {
                self.seek_preview = None;
                self.seek_fraction(f);
            }
            Action::SetVolume(v) => self.set_volume(v),
            Action::ToggleExpanded => self.toggle_expanded(&ctx),
            Action::TogglePlaylist => self.toggle_playlist(&ctx),
            Action::ToggleSpectrum => {
                self.settings.show_spectrum = !self.settings.show_spectrum;
                if let Some(engine) = &self.engine {
                    engine.shared().spectrum_enabled.store(
                        self.settings.show_spectrum,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                    engine.shared().clear_spectrum();
                }
                self.spectrum.iter_mut().for_each(|b| *b = 0.0);
                self.spectrum_target.iter_mut().for_each(|b| *b = 0.0);
                self.settings.save();
            }
            Action::TogglePin => {
                self.settings.always_on_top = !self.settings.always_on_top;
                self.apply_window_mode(&ctx, false);
                self.settings.save();
            }
            Action::OpenFiles => self.open_dialog(),
            Action::PlayIndex(i) => self.play_index(i, 0, true),
            Action::Select(s) => self.selected = s,
            Action::Remove(i) => self.remove(i),
            Action::RemoveSelected => {
                if let Some(i) = self.selected {
                    self.remove(i);
                }
            }
            Action::Clear => self.clear(),
            Action::CycleRepeat => {
                self.settings.repeat = self.settings.repeat.cycled();
                self.queue.set_repeat(self.settings.repeat);
                self.settings.save();
            }
            Action::ToggleShuffle => {
                self.settings.shuffle = !self.settings.shuffle;
                self.queue.set_shuffle(self.settings.shuffle);
                self.settings.save();
            }
            Action::Stop => {
                self.send(AudioCmd::Stop);
                self.state = PlayerState::Idle;
                self.seek_preview = None;
            }
            Action::ClosePlaylist => {
                self.settings.show_playlist = false;
                self.settings.save();
            }
            Action::Minimize => {
                ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Minimized(true));
                self.set_playlist_minimized(true);
            }
            Action::CloseWindow => ctx.send_viewport_cmd(ViewportCommand::Close),
            Action::OpenInWaveFlow => self.open_in_waveflow(),
            Action::ToggleSettings => self.settings_open = !self.settings_open,
            Action::SetLanguage(code) => {
                self.settings.language =
                    Lang::from_code(&code).map_or_else(String::new, |l| l.code().to_string());
                self.lang = Lang::resolve(&self.settings.language);
                self.settings.save();
                // The file-manager entries carry the labels of one
                // language on Windows: rewrite them when this executable
                // is the registered one. `reg import` takes a moment, so
                // off the UI thread.
                let lang = self.lang;
                std::thread::spawn(move || match crate::integration::relabel(lang) {
                    Ok(true) => tracing::info!(lang = lang.code(), "file-manager labels rewritten"),
                    Ok(false) => {}
                    Err(err) => tracing::warn!(%err, "file-manager labels not rewritten"),
                });
            }
        }
    }

    fn view(&self) -> PlayerView<'_> {
        let current = self.queue.current();
        let position_ms = match self.state {
            PlayerState::Idle => 0,
            PlayerState::Ended => self.duration_ms.unwrap_or(0),
            _ => self.engine.as_ref().map_or(0, |e| e.shared().position_ms()),
        };
        let format_chips = current.map_or_else(Vec::new, |e| {
            let meta = e.meta.as_ref();
            let mut parts = Vec::new();
            if let Some(codec) = self
                .codec
                .clone()
                .or_else(|| meta.and_then(|m| m.codec.clone()))
            {
                parts.push(codec);
            }
            if let Some(kbps) = meta.and_then(|m| m.bitrate_kbps) {
                parts.push(format!("{kbps} kbps"));
            }
            if let Some(rate) = meta.and_then(|m| m.sample_rate).filter(|r| *r > 0) {
                if rate >= 1_000_000 {
                    parts.push(format!("{:.1} MHz", rate as f64 / 1_000_000.0));
                } else {
                    parts.push(format!("{:.1} kHz", rate as f64 / 1000.0));
                }
            }
            if let Some(bits) = meta.and_then(|m| m.bit_depth).filter(|b| *b > 1) {
                parts.push(format!("{bits}-bit"));
            }
            parts
        });
        let status = self
            .status
            .as_ref()
            .filter(|(_, at)| at.elapsed() < STATUS_TTL)
            .map(|(s, _)| s.as_str());
        let lyrics = current.and(self.lyrics.as_ref());
        let lyric = lyrics.and_then(|l| l.current(position_ms));
        let lyric_span = lyric.and_then(|(at, _)| {
            let next = lyrics?.next_change_after(position_ms)?;
            Some((position_ms.saturating_sub(at), next.saturating_sub(at)))
        });
        PlayerView {
            title: current.map_or("", |e| e.title()),
            lyric: lyric.map(|(_, text)| text),
            lyric_epoch: lyric.map_or(0, |(at, _)| at),
            lyric_span,
            artist: current.and_then(|e| e.artist()),
            album: current.and_then(|e| e.album()),
            format_chips,
            state: self.state,
            position_ms,
            duration_ms: self.duration_ms,
            seek_preview: self.seek_preview,
            volume: self.settings.volume,
            expanded: self.settings.expanded,
            playlist_open: self.settings.show_playlist,
            spectrum: self
                .settings
                .show_spectrum
                .then_some(self.spectrum.as_slice()),
            always_on_top: self.settings.always_on_top,
            has_track: current.is_some(),
            has_prev: self.queue.prev_index().is_some(),
            has_next: self.queue.has_next(),
            can_hand_over: self.waveflow.exe().is_some(),
            cover: self
                .cover
                .as_ref()
                .filter(|_| self.cover_for == current.map(|e| e.id)),
            status,
            t: self.t(),
        }
    }

    fn playlist_viewport_id() -> ViewportId {
        ViewportId::from_hash_of("quickplay.playlist")
    }

    /// Minimizes / restores the playlist window together with the player.
    fn set_playlist_minimized(&self, minimized: bool) {
        if self.settings.show_playlist {
            self.ctx.send_viewport_cmd_to(
                Self::playlist_viewport_id(),
                ViewportCommand::Minimized(minimized),
            );
        }
    }

    /// The taskbar, Win+D or a `-` click can minimize the main window without
    /// going through [`Action::Minimize`]; the playlist follows every
    /// transition the backend reports, in both directions.
    fn sync_playlist_minimized(&mut self, ctx: &egui::Context) {
        let (reported, has_rect) =
            ctx.input(|i| (i.viewport().minimized, i.viewport().outer_rect.is_some()));
        // On macOS egui-winit only polls the minimized state at window
        // creation (egui#3494), so `minimized` never goes back to `false`
        // after a restore from the Dock. It does drop the rects of a
        // miniaturized window every frame, so a missing `outer_rect` is the
        // live state there, once the window has been placed at all.
        let minimized = if cfg!(target_os = "macos") {
            if has_rect {
                self.root_placed = true;
            } else if !self.root_placed {
                return;
            }
            let minimized = !has_rect;
            // eframe keeps the stale `minimized = true` after a restore from
            // the Dock and treats the window as invisible from then on: it
            // runs the app logic but paints nothing and drops the close
            // request, so the player looks frozen and `×` does nothing.
            // Re-sending the (now true) state is the one way to update it.
            if !minimized && self.root_minimized {
                ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Minimized(false));
            }
            minimized
        } else {
            let Some(minimized) = reported else {
                return;
            };
            minimized
        };
        if minimized != self.root_minimized {
            self.root_minimized = minimized;
            self.set_playlist_minimized(minimized);
        }
    }

    fn show_playlist_viewport(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let id = Self::playlist_viewport_id();
        let [w, h] = self.settings.playlist_size;
        let mut builder = ViewportBuilder::default()
            .with_title(PLAYLIST_TITLE)
            .with_app_id(APP_ID)
            // One taskbar button for the pair, as PotPlayer's playlist:
            // `with_taskbar` acts on Windows, the X11 window type tells other
            // window managers the same thing (utility windows get no taskbar
            // entry and stay with their application).
            .with_taskbar(false)
            .with_window_type(egui::X11WindowType::Utility)
            .with_inner_size([w, h])
            .with_min_inner_size([240.0, 160.0])
            .with_drag_and_drop(true)
            .with_decorations(false);
        if let Some(pos) = self.playlist_pos {
            builder = builder.with_position(pos);
        }
        let snapshot = self.shortcut_snapshot();
        let t = self.t();
        let queue = &self.queue;
        let selected = self.selected;
        let mut size: Option<Vec2> = None;
        let mut outer: Option<Rect> = None;
        let mut dropped: Vec<PathBuf> = Vec::new();
        let close = ctx.show_viewport_immediate(id, builder, |ui, class| {
            let close = ui.ctx().input(|i| i.viewport().close_requested());
            if class != ViewportClass::EmbeddedWindow {
                size = ui
                    .ctx()
                    .input(|i| i.viewport().inner_rect)
                    .map(|r| r.size());
                outer = ui.ctx().input(|i| i.viewport().outer_rect);
                dropped = ui.ctx().input(|i| {
                    i.raw
                        .dropped_files
                        .iter()
                        .map(|f| f.path().to_path_buf())
                        .filter(|p| !p.as_os_str().is_empty())
                        .collect()
                });
            }
            // Keys typed into this window land in its own input stream.
            collect_shortcuts(ui.ctx(), snapshot, actions);
            playlist::show(ui, t, queue, selected, actions);
            close
        });
        if let Some(size) = size.filter(|s| s.x >= 240.0 && s.y >= 160.0) {
            self.settings.playlist_size = [size.x, size.y];
        }
        if !dropped.is_empty() {
            self.add_paths(&dropped, true);
        }
        if close {
            self.settings.show_playlist = false;
            self.settings.save();
        } else if outer.is_some() {
            if !self.playlist_attached {
                self.playlist_attached =
                    crate::platform::attach_owned_window(MAIN_TITLE, PLAYLIST_TITLE);
            }
            self.dock_playlist(ctx, outer);
        }
    }

    /// Files handed over by a second launch (file manager verbs). `play`
    /// replaces the queue as PotPlayer's "Play with" does, except within a
    /// burst, where the following files are appended; `enqueue` appends.
    /// Either way the window comes to the front.
    fn pump_ipc_messages(&mut self, ctx: &egui::Context) {
        // Explorer starts the per-file processes some 30 ms apart; a user
        // double-clicking two files is slower than this.
        const BURST: Duration = Duration::from_millis(500);
        let mut got_any = false;
        // Each Finder open is one request, a multi-selection included: it
        // replaces the queue like `play`, without the burst rule, which is
        // for Explorer starting one process per selected file.
        #[cfg(target_os = "macos")]
        for paths in crate::macos::take_opened() {
            got_any = true;
            self.clear();
            self.add_paths(&paths, true);
        }
        while let Some(msg) = self.ipc.as_ref().and_then(|s| s.try_recv()) {
            got_any = true;
            match msg.mode {
                ipc::Mode::Play => {
                    let now = Instant::now();
                    let in_burst = self
                        .last_remote_play
                        .is_some_and(|at| now.duration_since(at) < BURST);
                    self.last_remote_play = Some(now);
                    if !msg.paths.is_empty() && !in_burst {
                        self.clear();
                    }
                    self.add_paths(&msg.paths, true);
                }
                ipc::Mode::Enqueue => self.add_paths(&msg.paths, false),
            }
        }
        if got_any {
            // Only un-minimize a window that is minimized: asking winit to
            // restore a window that has not been shown yet leaves it iconic.
            if self.root_minimized {
                ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Minimized(false));
            }
            ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Focus);
        }
    }

    /// Wakes the UI exactly when the lyric line changes, instead of letting
    /// it lag by up to a position tick.
    fn schedule_lyric_repaint(&self, ctx: &egui::Context) {
        if self.state != PlayerState::Playing {
            return;
        }
        let (Some(lyrics), Some(engine)) = (self.lyrics.as_ref(), self.engine.as_ref()) else {
            return;
        };
        let pos = engine.shared().position_ms();
        if let Some(next) = lyrics.next_change_after(pos) {
            ctx.request_repaint_after(Duration::from_millis(next.saturating_sub(pos) + 5));
        }
    }

    /// Forgets the playlist window: egui destroys it when it is not shown
    /// for a frame, so the next one needs attaching and anchoring afresh.
    fn forget_playlist_window(&mut self) {
        self.playlist_attached = false;
        self.playlist_place_pending = true;
        self.dock_anchor = None;
        self.playlist_last_pos = None;
        self.playlist_expected_pos = None;
    }

    /// Keeps the playlist where the user put it *relative to the player*:
    /// when the player moves or widens, the playlist is sent along; when the
    /// playlist alone moves, the new offset is remembered. `None` rects and
    /// the off-screen rect a minimized window reports are ignored.
    fn dock_playlist(&mut self, ctx: &egui::Context, playlist: Option<Rect>) {
        let main = ctx.input(|i| i.viewport().outer_rect);
        let (Some(main), Some(playlist)) = (main, playlist) else {
            return;
        };
        if self.root_minimized || main.min.x < -10_000.0 || playlist.min.x < -10_000.0 {
            self.dock_anchor = None;
            return;
        }
        let anchor = main.right_top();
        match (self.dock_anchor, self.playlist_offset) {
            (Some(prev), Some(offset)) if prev != anchor => {
                let target = anchor + offset;
                ctx.send_viewport_cmd_to(
                    Self::playlist_viewport_id(),
                    ViewportCommand::OuterPosition(target),
                );
                self.playlist_expected_pos = Some(target);
            }
            _ if self.playlist_offset.is_none() => {
                // First sight of this window: put it beside the player, whatever
                // the backend made of the builder position hint.
                let offset = Vec2::new(DOCK_GAP, 0.0);
                let target = anchor + offset;
                self.playlist_offset = Some(offset);
                if (playlist.min - target).length() > DOCK_TOLERANCE {
                    ctx.send_viewport_cmd_to(
                        Self::playlist_viewport_id(),
                        ViewportCommand::OuterPosition(target),
                    );
                    self.playlist_expected_pos = Some(target);
                }
            }
            _ if self.playlist_place_pending => {
                self.playlist_place_pending = false;
                let target = anchor + self.playlist_offset.unwrap_or_default();
                if (playlist.min - target).length() > DOCK_TOLERANCE {
                    ctx.send_viewport_cmd_to(
                        Self::playlist_viewport_id(),
                        ViewportCommand::OuterPosition(target),
                    );
                    self.playlist_expected_pos = Some(target);
                }
            }
            _ => {
                if self.playlist_last_pos != Some(playlist.min) {
                    let ours = self
                        .playlist_expected_pos
                        .take()
                        .is_some_and(|e| (playlist.min - e).length() <= DOCK_TOLERANCE);
                    if !ours {
                        self.playlist_offset = Some(playlist.min - anchor);
                    }
                }
            }
        }
        self.dock_anchor = Some(anchor);
        self.playlist_last_pos = Some(playlist.min);
    }
}

impl eframe::App for QuickPlayApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.window_mode_applied {
            self.window_mode_applied = true;
            self.apply_window_mode(ctx, false);
        }
        self.pump_engine_events();
        self.pump_loader_events();
        self.pump_ipc_messages(ctx);
        self.handle_dropped_files(ctx);
        self.handle_shortcuts(ctx);
        self.track_window_size(ctx);
        self.sync_playlist_minimized(ctx);
        if let Some(capture) = self.capture.as_mut() {
            capture.tick(ctx);
            capture.save(ctx);
        }
        if self
            .status
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() >= STATUS_TTL)
        {
            self.status = None;
        }
        let animating = self.advance_spectrum();
        self.schedule_lyric_repaint(ctx);
        if self.state == PlayerState::Playing {
            ctx.request_repaint_after(if animating {
                SPECTRUM_TICK
            } else {
                POSITION_TICK
            });
        } else if animating {
            ctx.request_repaint_after(SPECTRUM_TICK);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mut actions: Vec<Action> = Vec::new();
        let view = self.view();
        let hovering_files = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());

        // Custom chrome: the title bar carries the window title, the pin and
        // the window buttons; the edges resize.
        let subtitle = view.has_track.then(|| view.title.to_string());
        let t = view.t;
        let settings_open = self.settings_open;
        egui::Panel::top("title")
            .exact_size(TITLE_HEIGHT)
            .show_separator_line(false)
            .frame(theme::title_frame())
            .show(ui, |ui| {
                match widgets::title_bar(
                    ui,
                    t,
                    BAR_TITLE,
                    subtitle.as_deref(),
                    Some(view.always_on_top),
                    true,
                    Some(settings_open),
                ) {
                    Some(TitleAction::Pin) => actions.push(Action::TogglePin),
                    Some(TitleAction::Minimize) => actions.push(Action::Minimize),
                    Some(TitleAction::Close) => actions.push(Action::CloseWindow),
                    Some(TitleAction::Settings) => actions.push(Action::ToggleSettings),
                    None => {}
                }
            });
        if settings_open {
            // The page takes the whole content area in both modes; Esc or
            // its own close button brings the controls back.
            if ui
                .ctx()
                .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                actions.push(Action::ToggleSettings);
            }
            let language = self.settings.language.clone();
            egui::CentralPanel::default()
                .frame(theme::panel_frame())
                .show(ui, |ui| settings_page::show(ui, t, &language, &mut actions));
        } else if view.expanded {
            egui::Panel::bottom("controls")
                .exact_size(CONTROLS_HEIGHT)
                .show_separator_line(false)
                .frame(theme::panel_frame())
                .show(ui, |ui| player::controls(ui, &view, &mut actions));
            egui::CentralPanel::default()
                .frame(theme::artwork_frame())
                .show(ui, |ui| player::artwork(ui, &view));
        } else {
            egui::CentralPanel::default()
                .frame(theme::panel_frame())
                .show(ui, |ui| player::controls(ui, &view, &mut actions));
        }
        widgets::resize_edges(ui, view.expanded);
        if hovering_files {
            player::drop_overlay(ui, t);
        }
        drop(view);

        if self.settings.show_playlist {
            let ctx = ui.ctx().clone();
            self.show_playlist_viewport(&ctx, &mut actions);
        } else if self.playlist_attached {
            self.forget_playlist_window();
        }

        for action in actions {
            self.apply(action);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.settings.save();
        // Stop the decoder and close the device before the window goes.
        if let Some(mut engine) = self.engine.take() {
            engine.shutdown();
        }
    }
}

/// What the keyboard shortcuts need to know to compute their action.
#[derive(Clone, Copy)]
struct ShortcutSnapshot {
    volume: f32,
    position_ms: u64,
    duration_ms: Option<u64>,
}

impl ShortcutSnapshot {
    fn relative_seek_fraction(&self, delta_ms: i64) -> f32 {
        let Some(duration) = self.duration_ms.filter(|d| *d > 0) else {
            return 0.0;
        };
        let target = (self.position_ms as i64 + delta_ms).clamp(0, duration as i64 - 1);
        target as f32 / duration as f32
    }
}

/// Consume the player's shortcuts from `ctx`'s current viewport. Called for
/// the main window and again inside the playlist window, since each native
/// window has its own input stream.
fn collect_shortcuts(ctx: &egui::Context, snap: ShortcutSnapshot, out: &mut Vec<Action>) {
    use egui::{Key, Modifiers};
    ctx.input_mut(|i| {
        if i.consume_key(Modifiers::NONE, Key::Space) {
            out.push(Action::TogglePlay);
        }
        if i.consume_key(Modifiers::NONE, Key::N) {
            out.push(Action::Next);
        }
        if i.consume_key(Modifiers::NONE, Key::P) {
            out.push(Action::Prev);
        }
        if i.consume_key(Modifiers::NONE, Key::L) {
            out.push(Action::TogglePlaylist);
        }
        if i.consume_key(Modifiers::NONE, Key::S) {
            out.push(Action::ToggleSpectrum);
        }
        if i.consume_key(Modifiers::NONE, Key::A) {
            out.push(Action::ToggleExpanded);
        }
        if i.consume_key(Modifiers::NONE, Key::T) {
            out.push(Action::TogglePin);
        }
        if i.consume_key(Modifiers::NONE, Key::R) {
            out.push(Action::CycleRepeat);
        }
        if i.consume_key(Modifiers::NONE, Key::H) {
            out.push(Action::ToggleShuffle);
        }
        if i.consume_key(Modifiers::COMMAND, Key::O) {
            out.push(Action::OpenFiles);
        }
        let step: i64 = if i.modifiers.shift { 30_000 } else { 5_000 };
        if i.consume_key(Modifiers::NONE, Key::ArrowRight)
            || i.consume_key(Modifiers::SHIFT, Key::ArrowRight)
        {
            out.push(Action::SeekCommit(snap.relative_seek_fraction(step)));
        }
        if i.consume_key(Modifiers::NONE, Key::ArrowLeft)
            || i.consume_key(Modifiers::SHIFT, Key::ArrowLeft)
        {
            out.push(Action::SeekCommit(snap.relative_seek_fraction(-step)));
        }
        if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
            out.push(Action::SetVolume(snap.volume + 0.05));
        }
        if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
            out.push(Action::SetVolume(snap.volume - 0.05));
        }
    });
}
