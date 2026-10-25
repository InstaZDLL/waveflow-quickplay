//! Opt-in window capture for testing on displays that offer no external
//! screenshot route (GNOME on Wayland refuses D-Bus screenshots from
//! scripts, and a Wayland window cannot be grabbed from outside).
//!
//! `QUICKPLAY_CAPTURE_DIR=<dir>` makes the app ask egui for a screenshot of
//! the player window `QUICKPLAY_CAPTURE_AFTER_MS` (default 2000 ms) after
//! start and write it as `<dir>/player.png`. Nothing happens when the
//! variable is unset. The player only: eframe 0.36 serves screenshot
//! requests for the root viewport and drops them for immediate child
//! viewports such as the playlist.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{Event, UserData, ViewportCommand, ViewportId};

pub struct Capture {
    dir: PathBuf,
    after: Duration,
    started: Instant,
    requested: bool,
}

impl Capture {
    pub fn from_env() -> Option<Self> {
        let dir = PathBuf::from(std::env::var_os("QUICKPLAY_CAPTURE_DIR")?);
        let after = std::env::var("QUICKPLAY_CAPTURE_AFTER_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .map(Duration::from_millis)
            .unwrap_or(Duration::from_millis(2000));
        tracing::info!(dir = %dir.display(), after_ms = after.as_millis(), "capture armed");
        Some(Self {
            dir,
            after,
            started: Instant::now(),
            requested: false,
        })
    }

    /// Called once per frame from the root viewport: keeps the window
    /// repainting until the delay has passed, then requests the screenshot.
    pub fn tick(&mut self, ctx: &egui::Context) {
        if self.requested {
            return;
        }
        let elapsed = self.started.elapsed();
        if elapsed < self.after {
            ctx.request_repaint_after(self.after - elapsed);
            return;
        }
        ctx.send_viewport_cmd_to(
            ViewportId::ROOT,
            ViewportCommand::Screenshot(UserData::default()),
        );
        self.requested = true;
    }

    /// Writes any screenshot delivered to the root viewport's input as
    /// `<dir>/player.png`.
    pub fn save(&self, ctx: &egui::Context) {
        let images: Vec<_> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
                .collect()
        });
        for image in images {
            let path = self.dir.join("player.png");
            let (w, h) = (image.width() as u32, image.height() as u32);
            let Some(buf) = image::RgbaImage::from_raw(w, h, image.as_raw().to_vec()) else {
                tracing::warn!(w, h, "capture: unexpected buffer size");
                continue;
            };
            if let Err(e) = std::fs::create_dir_all(&self.dir) {
                tracing::warn!(dir = %self.dir.display(), error = %e, "capture: no directory");
                continue;
            }
            match buf.save(&path) {
                Ok(()) => tracing::info!(path = %path.display(), w, h, "capture written"),
                Err(e) => tracing::warn!(path = %path.display(), error = %e, "capture failed"),
            }
        }
    }
}
