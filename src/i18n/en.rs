//! English, the reference the other languages are translated from.

use super::Strings;

pub const STRINGS: Strings = Strings {
    // ── Player pane ────────────────────────────────────────────────────
    nothing_playing: "Nothing playing",
    unknown_artist: "Unknown artist",
    drop_hint: "Drop audio files here, or press Ctrl+O",
    no_artwork: "No artwork",
    drop_to_add: "Drop to add to the playlist",
    volume_percent: "Volume {n}%",

    // ── Tooltips ───────────────────────────────────────────────────────
    tip_play: "Play (Space)",
    tip_pause: "Pause (Space)",
    tip_stop: "Stop",
    tip_previous: "Previous (P)",
    tip_next: "Next (N)",
    tip_open_files: "Open files (Ctrl+O)",
    tip_playlist: "Playlist (L)",
    tip_artwork: "Artwork (A)",
    tip_spectrum: "Spectrum (S)",
    tip_settings: "Settings",
    tip_always_on_top: "Always on top (T)",
    tip_always_on_top_xwayland: "Always on top (T) — restarts QuickPlay under XWayland",
    tip_always_on_top_wayland:
        "Always on top (T) — turning it off restarts QuickPlay under Wayland",
    tip_minimize: "Minimize",
    tip_close: "Close",

    // ── Playlist window ────────────────────────────────────────────────
    playlist: "Playlist",
    playlist_empty: "Empty",
    playlist_one_track: "1 track · {t}",
    playlist_n_tracks: "{n} tracks · {t}",
    tip_add_files: "Add files",
    tip_remove_selected: "Remove selected (Delete)",
    tip_clear_playlist: "Clear playlist",
    tip_repeat_off: "Repeat off (R)",
    tip_repeat_all: "Repeat all (R)",
    tip_repeat_one: "Repeat one (R)",
    tip_shuffle: "Shuffle (H)",
    playlist_drop_hint: "Drop files here or press +",
    menu_play: "Play",
    menu_remove: "Remove",

    // ── Status line ────────────────────────────────────────────────────
    status_audio_unavailable: "Audio unavailable: {e}",
    status_scanning: "Scanning folders…",
    status_no_audio_files: "No supported audio files",
    status_cannot_play: "Cannot play: {e}",
    status_cannot_open_waveflow: "Cannot open WaveFlow: {e}",
    status_pin_needs_xwayland: "Always on top needs XWayland, which this session does not have",
    status_cannot_restart: "Cannot restart QuickPlay: {e}",
    status_update_ready: "QuickPlay {v} will be installed when you close it",
    status_update_available: "QuickPlay {v} is available",
    tip_open_in_waveflow: "Continue in WaveFlow",

    // ── File dialog ────────────────────────────────────────────────────
    dialog_open_title: "Open audio files",
    dialog_filter_audio: "Audio",

    // ── Settings page ──────────────────────────────────────────────────
    settings: "Settings",
    language: "Language",
    language_system: "System language ({l})",
    settings_check_updates: "Check for updates at launch",
    about: "About",
    about_tagline: "The small player next to WaveFlow: double-click a track, hear it now.",
    about_version: "Version {v}",
    about_license: "Free software, GPL-3.0-only license",
    about_engine: "Built on waveflow-core, the WaveFlow desktop engine",
    about_source: "Source code and releases",

    // ── File-manager integration ───────────────────────────────────────
    action_play_with: "Play with QuickPlay",
    action_add_to_playlist: "Add to QuickPlay playlist",
    desktop_comment: "Play audio files quickly",
};
