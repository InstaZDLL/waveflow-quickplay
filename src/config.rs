//! QuickPlay's own preferences file. Deliberately a separate namespace from
//! the desktop app (`app.waveflow` under the platform data dir, SQLite): the
//! two never read each other's state, so the mini-player's window
//! preferences and QuickPlay's cannot interfere.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const COMPACT_WIDTH: f32 = 420.0;
pub const MIN_WIDTH: f32 = 320.0;

/// What happens when the queue runs out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    /// Stop after the last track.
    #[default]
    Off,
    /// Start the queue over.
    All,
    /// Play the current track again.
    One,
}

impl Repeat {
    /// The next mode when the button is pressed: off → all → one → off.
    pub fn cycled(self) -> Self {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub compact_width: f32,
    pub expanded_size: [f32; 2],
    pub expanded: bool,
    pub always_on_top: bool,
    /// Linear gain in `[0, 1]`, as the engine applies it.
    pub volume: f32,
    pub show_playlist: bool,
    pub show_spectrum: bool,
    pub playlist_size: [f32; 2],
    /// Interface language as a BCP 47 code (`fr`, `zh-CN`); empty to follow
    /// the system. See [`crate::i18n::Lang`].
    pub language: String,
    pub repeat: Repeat,
    pub shuffle: bool,
    /// Look for a new version at launch (src/update.rs).
    pub check_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            compact_width: COMPACT_WIDTH,
            expanded_size: [COMPACT_WIDTH, COMPACT_WIDTH + crate::ui::COMPACT_HEIGHT],
            expanded: false,
            always_on_top: false,
            volume: 0.8,
            show_playlist: false,
            show_spectrum: true,
            playlist_size: [340.0, 420.0],
            language: String::new(),
            repeat: Repeat::Off,
            shuffle: false,
            check_updates: true,
        }
    }
}

impl Settings {
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("waveflow-quickplay").join("settings.json"))
    }

    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        let loaded = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Self>(&bytes).unwrap_or_else(|err| {
                tracing::warn!(%err, path = %path.display(), "settings unreadable; using defaults");
                Self::default()
            }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(err) => {
                tracing::warn!(%err, path = %path.display(), "settings unreadable; using defaults");
                Self::default()
            }
        };
        loaded.sanitised()
    }

    /// Write atomically (temp file + rename) so a crash mid-write cannot
    /// leave a truncated file behind.
    pub fn save(&self) {
        let Some(path) = Self::path() else {
            return;
        };
        let Some(dir) = path.parent() else {
            return;
        };
        if let Err(err) = std::fs::create_dir_all(dir) {
            tracing::warn!(%err, "cannot create settings directory");
            return;
        }
        let tmp = path.with_extension("json.tmp");
        let result = serde_json::to_vec_pretty(self)
            .map_err(|e| e.to_string())
            .and_then(|bytes| std::fs::write(&tmp, bytes).map_err(|e| e.to_string()))
            .and_then(|_| std::fs::rename(&tmp, &path).map_err(|e| e.to_string()));
        if let Err(err) = result {
            tracing::warn!(%err, path = %path.display(), "settings not saved");
            let _ = std::fs::remove_file(&tmp);
        }
    }

    /// Reject NaN, negative and absurd values from a hand-edited file.
    fn sanitised(mut self) -> Self {
        let defaults = Self::default();
        let sane = |v: f32, min: f32, max: f32, fallback: f32| {
            if v.is_finite() && v >= min && v <= max {
                v
            } else {
                fallback
            }
        };
        self.compact_width = sane(
            self.compact_width,
            MIN_WIDTH,
            4096.0,
            defaults.compact_width,
        );
        self.expanded_size = [
            sane(
                self.expanded_size[0],
                MIN_WIDTH,
                8192.0,
                defaults.expanded_size[0],
            ),
            sane(
                self.expanded_size[1],
                crate::ui::COMPACT_HEIGHT + 80.0,
                8192.0,
                defaults.expanded_size[1],
            ),
        ];
        self.volume = sane(self.volume, 0.0, 1.0, defaults.volume);
        self.playlist_size = [
            sane(
                self.playlist_size[0],
                240.0,
                4096.0,
                defaults.playlist_size[0],
            ),
            sane(
                self.playlist_size[1],
                160.0,
                4096.0,
                defaults.playlist_size[1],
            ),
        ];
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonsense_values_fall_back_to_defaults() {
        let s = Settings {
            compact_width: f32::NAN,
            expanded_size: [-5.0, 10.0],
            volume: 7.0,
            ..Settings::default()
        }
        .sanitised();
        let d = Settings::default();
        assert_eq!(s.compact_width, d.compact_width);
        assert_eq!(s.expanded_size, d.expanded_size);
        assert_eq!(s.volume, d.volume);
    }
}
