//! The application: owns the queue, the (lazily created) audio engine, the
//! background loader and the window state; drives the UI and applies its
//! actions.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{
    Pos2, TextureHandle, Vec2, ViewportBuilder, ViewportClass, ViewportCommand, ViewportId,
};

use crate::audio::{AudioCmd, AudioEngine, EngineEvent, PlayerState};
use crate::config::{Settings, MIN_WIDTH};
use crate::library::{self, Loader, LoaderEvent};
use crate::queue::Queue;
use crate::ui::widgets::{self, TitleAction};
use crate::ui::{
    player, playlist, theme, Action, PlayerView, COMPACT_HEIGHT, CONTROLS_HEIGHT, TITLE_HEIGHT,
};

pub const APP_ID: &str = "app.waveflow.quickplay";

/// Position-display cadence while playing — the desktop's `player:position`
/// rate. Idle and paused repaint only on input.
const POSITION_TICK: Duration = Duration::from_millis(250);

/// How long a status line stays visible.
const STATUS_TTL: Duration = Duration::from_secs(6);

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
}

impl QuickPlayApp {
    pub fn new(cc: &eframe::CreationContext<'_>, settings: Settings, files: Vec<PathBuf>) -> Self {
        theme::apply(&cc.egui_ctx);
        let ctx = cc.egui_ctx.clone();
        let notify_ctx = ctx.clone();
        let notify: Arc<dyn Fn() + Send + Sync> = Arc::new(move || notify_ctx.request_repaint());
        let mut app = Self {
            queue: Queue::default(),
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
                    tracing::info!(device = ?engine.device_name(), "audio engine started");
                    self.engine = Some(engine);
                }
                Err(err) => {
                    tracing::error!(%err, "audio engine unavailable");
                    self.set_status(format!("Audio unavailable: {err}"));
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

    fn set_status(&mut self, message: impl Into<String>) {
        self.status = Some((message.into(), Instant::now()));
    }

    // ── Queue ───────────────────────────────────────────────────────────

    /// Expand `inputs` and append them. With `autoplay`, start the first new
    /// row when nothing is playing.
    fn add_paths(&mut self, inputs: &[PathBuf], autoplay: bool) {
        let paths = library::collect_audio_paths(inputs);
        if paths.is_empty() {
            if !inputs.is_empty() {
                self.set_status("No supported audio files");
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

    fn play_index(&mut self, index: usize, start_ms: u64, autoplay: bool) {
        let Some(entry) = self.queue.get(index) else {
            return;
        };
        let (id, path) = (entry.id, entry.path.clone());
        let meta_duration = entry.duration_ms();
        self.queue.set_current(Some(index));
        self.selected = Some(index);
        self.generation += 1;
        self.playing_id = Some(id);
        self.duration_ms = meta_duration;
        self.codec = None;
        self.seek_preview = None;
        self.state = PlayerState::Loading;
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
            if let Some(i) = self.queue.current_index() {
                self.play_index(i, 0, true);
            }
        } else if let Some(i) = self.queue.prev_index() {
            self.play_index(i, 0, true);
        }
    }

    fn toggle_play(&mut self) {
        match self.state {
            PlayerState::Playing => self.send(AudioCmd::Pause),
            PlayerState::Paused | PlayerState::Loading => self.send(AudioCmd::Resume),
            PlayerState::Idle | PlayerState::Ended => {
                if let Some(i) = self.queue.current_index() {
                    self.play_index(i, 0, true);
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
            PlayerState::Ended | PlayerState::Idle => {
                if let Some(i) = self.queue.current_index() {
                    self.play_index(i, ms, true);
                }
            }
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
        let dialog = rfd::FileDialog::new()
            .set_title("Open audio files")
            .add_filter("Audio", library::audio_extensions());
        if let Some(paths) = dialog.pick_files() {
            self.add_paths(&paths, true);
        }
    }

    // ── Events ──────────────────────────────────────────────────────────

    fn pump_engine_events(&mut self) {
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
                    self.status = None;
                }
                EngineEvent::Loaded { .. } => {}
                EngineEvent::State(state) => {
                    // A stale `Idle` from a superseded load would flicker
                    // the button; the decoder's state is authoritative
                    // otherwise.
                    if !(state == PlayerState::Idle && self.state == PlayerState::Loading) {
                        self.state = state;
                    }
                }
                EngineEvent::Ended { generation } if generation == self.generation => {
                    self.state = PlayerState::Ended;
                    self.next();
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
                    self.set_status(format!("Cannot play: {message}"));
                    // Skip to the next playable row, bounded so a queue of
                    // broken files cannot loop forever.
                    self.fail_streak += 1;
                    if self.fail_streak < self.queue.len().max(1) {
                        if let Some(i) = self.queue.next_index() {
                            self.play_index(i, 0, true);
                        }
                    }
                }
                EngineEvent::Output(message) => self.set_status(message),
            }
        }
    }

    fn pump_loader_events(&mut self) {
        for event in self.loader.poll() {
            match event {
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
                .map(|r| Pos2::new(r.right() + 8.0, r.top()));
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
            Action::Stop => {
                self.send(AudioCmd::Stop);
                self.state = PlayerState::Idle;
                self.seek_preview = None;
            }
            Action::ClosePlaylist => {
                self.settings.show_playlist = false;
                self.settings.save();
            }
            Action::Minimize => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
            Action::CloseWindow => ctx.send_viewport_cmd(ViewportCommand::Close),
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
        PlayerView {
            title: current.map_or("", |e| e.title()),
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
            always_on_top: self.settings.always_on_top,
            has_track: current.is_some(),
            has_prev: self.queue.prev_index().is_some(),
            has_next: self.queue.next_index().is_some(),
            cover: self
                .cover
                .as_ref()
                .filter(|_| self.cover_for == current.map(|e| e.id)),
            status,
        }
    }

    fn show_playlist_viewport(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let id = ViewportId::from_hash_of("quickplay.playlist");
        let [w, h] = self.settings.playlist_size;
        let mut builder = ViewportBuilder::default()
            .with_title("Playlist · WaveFlow QuickPlay")
            .with_app_id(APP_ID)
            .with_inner_size([w, h])
            .with_min_inner_size([240.0, 160.0])
            .with_drag_and_drop(true)
            .with_decorations(false);
        if let Some(pos) = self.playlist_pos {
            builder = builder.with_position(pos);
        }
        let snapshot = self.shortcut_snapshot();
        let queue = &self.queue;
        let selected = self.selected;
        let mut size: Option<Vec2> = None;
        let mut dropped: Vec<PathBuf> = Vec::new();
        let close = ctx.show_viewport_immediate(id, builder, |ui, class| {
            let close = ui.ctx().input(|i| i.viewport().close_requested());
            if class != ViewportClass::EmbeddedWindow {
                size = ui
                    .ctx()
                    .input(|i| i.viewport().inner_rect)
                    .map(|r| r.size());
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
            playlist::show(ui, queue, selected, actions);
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
        }
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
        self.handle_dropped_files(ctx);
        self.handle_shortcuts(ctx);
        self.track_window_size(ctx);
        if self
            .status
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() >= STATUS_TTL)
        {
            self.status = None;
        }
        if self.state == PlayerState::Playing {
            ctx.request_repaint_after(POSITION_TICK);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mut actions: Vec<Action> = Vec::new();
        let view = self.view();
        let hovering_files = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());

        // Custom chrome: the title bar carries the window title, the pin and
        // the window buttons; the edges resize.
        let subtitle = view.has_track.then(|| view.title.to_string());
        egui::Panel::top("title")
            .exact_size(TITLE_HEIGHT)
            .show_separator_line(false)
            .frame(theme::title_frame())
            .show(ui, |ui| {
                match widgets::title_bar(
                    ui,
                    "WaveFlow QuickPlay",
                    subtitle.as_deref(),
                    Some(view.always_on_top),
                    true,
                ) {
                    Some(TitleAction::Pin) => actions.push(Action::TogglePin),
                    Some(TitleAction::Minimize) => actions.push(Action::Minimize),
                    Some(TitleAction::Close) => actions.push(Action::CloseWindow),
                    None => {}
                }
            });
        if view.expanded {
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
            player::drop_overlay(ui);
        }
        drop(view);

        if self.settings.show_playlist {
            let ctx = ui.ctx().clone();
            self.show_playlist_viewport(&ctx, &mut actions);
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
        if i.consume_key(Modifiers::NONE, Key::A) {
            out.push(Action::ToggleExpanded);
        }
        if i.consume_key(Modifiers::NONE, Key::T) {
            out.push(Action::TogglePin);
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
