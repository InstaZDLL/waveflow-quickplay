//! The egui interface. UI functions read a view of the app state and push
//! [`Action`]s; the app applies them after the frame. That keeps every
//! widget free of engine and queue borrows.

pub mod icons;
pub mod player;
pub mod playlist;
pub mod settings;
pub mod theme;
pub mod widgets;

use egui::TextureHandle;

use crate::audio::PlayerState;
use crate::i18n::Strings;

/// Height of the custom title bar in logical points.
pub const TITLE_HEIGHT: f32 = 26.0;
/// Height of the controls block (information, seek row, bottom bar)
/// including its frame margins.
pub const CONTROLS_HEIGHT: f32 = 128.0;
/// The compact window's whole inner height.
pub const COMPACT_HEIGHT: f32 = TITLE_HEIGHT + CONTROLS_HEIGHT;

/// What the UI asks the app to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    TogglePlay,
    Stop,
    Next,
    Prev,
    /// The seek bar is being dragged: show this fraction, do not seek yet.
    SeekPreview(f32),
    /// Seek to this fraction of the track.
    SeekCommit(f32),
    SetVolume(f32),
    ToggleExpanded,
    TogglePlaylist,
    ToggleSpectrum,
    ClosePlaylist,
    TogglePin,
    Minimize,
    CloseWindow,
    OpenFiles,
    PlayIndex(usize),
    Select(Option<usize>),
    Remove(usize),
    RemoveSelected,
    Clear,
    /// Repeat off → all → one → off.
    CycleRepeat,
    ToggleShuffle,
    /// Show or hide the settings page in place of the controls.
    ToggleSettings,
    /// A language code from the picker; empty means "follow the system".
    SetLanguage(String),
    /// Hand the queue and the position over to WaveFlow, then close.
    OpenInWaveFlow,
}

/// Read-only snapshot of what the player pane shows.
pub struct PlayerView<'a> {
    pub title: &'a str,
    /// The synced lyric line in force, shown in place of the title.
    pub lyric: Option<&'a str>,
    /// Start time of that line, so two identical consecutive lines still
    /// restart the scroll; 0 when the title is shown.
    pub lyric_epoch: u64,
    /// How far into the lyric line the playback is, and how long the line
    /// lasts, so a long line scrolls at the pace of its own time. `None`
    /// for the title, and for a last line with no end.
    pub lyric_span: Option<widgets::LineSpan>,
    pub artist: Option<&'a str>,
    pub album: Option<&'a str>,
    /// `AAC`, `256 kbps`, `48.0 kHz`, `16-bit` — whatever is known.
    pub format_chips: Vec<String>,
    pub state: PlayerState,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    /// Fraction being previewed while the seek handle is dragged.
    pub seek_preview: Option<f32>,
    pub volume: f32,
    pub expanded: bool,
    pub playlist_open: bool,
    /// Bar heights in 0..1, `None` when the visualizer is hidden.
    pub spectrum: Option<&'a [f32]>,
    pub always_on_top: bool,
    pub has_track: bool,
    pub has_prev: bool,
    pub has_next: bool,
    /// WaveFlow is installed and can take the playback over.
    pub can_hand_over: bool,
    pub cover: Option<&'a TextureHandle>,
    pub status: Option<&'a str>,
    /// The interface language's strings.
    pub t: &'static Strings,
}

/// `m:ss`, or `h:mm:ss` from an hour on.
pub fn format_time(ms: u64) -> String {
    let total = ms / 1000;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::format_time;

    #[test]
    fn formats_minutes_and_hours() {
        assert_eq!(format_time(0), "0:00");
        assert_eq!(format_time(65_000), "1:05");
        assert_eq!(format_time(3_725_000), "1:02:05");
    }
}
