//! German.

use super::Strings;

pub const STRINGS: Strings = Strings {
    // ── Player pane ────────────────────────────────────────────────────
    nothing_playing: "Keine Wiedergabe",
    unknown_artist: "Unbekannter Interpret",
    drop_hint: "Audiodateien hier ablegen oder Strg+O drücken",
    no_artwork: "Kein Cover",
    drop_to_add: "Zum Hinzufügen zur Wiedergabeliste ablegen",
    volume_percent: "Lautstärke {n} %",

    // ── Tooltips ───────────────────────────────────────────────────────
    tip_play: "Wiedergabe (Leertaste)",
    tip_pause: "Pause (Leertaste)",
    tip_stop: "Stopp",
    tip_previous: "Zurück (P)",
    tip_next: "Weiter (N)",
    tip_open_files: "Dateien öffnen (Strg+O)",
    tip_playlist: "Wiedergabeliste (L)",
    tip_artwork: "Cover (A)",
    tip_spectrum: "Spektrum (S)",
    tip_settings: "Einstellungen",
    tip_always_on_top: "Immer im Vordergrund (T)",
    tip_minimize: "Minimieren",
    tip_close: "Schließen",

    // ── Playlist window ────────────────────────────────────────────────
    playlist: "Wiedergabeliste",
    playlist_empty: "Leer",
    playlist_one_track: "1 Titel · {t}",
    playlist_n_tracks: "{n} Titel · {t}",
    tip_add_files: "Dateien hinzufügen",
    tip_remove_selected: "Auswahl entfernen (Entf)",
    tip_clear_playlist: "Wiedergabeliste leeren",
    tip_repeat_off: "Wiederholen aus (R)",
    tip_repeat_all: "Alle wiederholen (R)",
    tip_repeat_one: "Titel wiederholen (R)",
    tip_shuffle: "Zufallswiedergabe (H)",
    playlist_drop_hint: "Dateien hier ablegen oder + drücken",
    menu_play: "Wiedergeben",
    menu_remove: "Entfernen",

    // ── Status line ────────────────────────────────────────────────────
    status_audio_unavailable: "Audio nicht verfügbar: {e}",
    status_scanning: "Ordner werden durchsucht…",
    status_no_audio_files: "Keine unterstützten Audiodateien",
    status_cannot_play: "Nicht abspielbar: {e}",
    status_cannot_open_waveflow: "WaveFlow lässt sich nicht öffnen: {e}",
    tip_open_in_waveflow: "In WaveFlow fortsetzen",

    // ── File dialog ────────────────────────────────────────────────────
    dialog_open_title: "Audiodateien öffnen",
    dialog_filter_audio: "Audio",

    // ── Settings page ──────────────────────────────────────────────────
    settings: "Einstellungen",
    language: "Sprache",
    language_system: "Systemsprache ({l})",
    about: "Info",
    about_tagline: "Der kleine Player neben WaveFlow: Titel doppelt anklicken, sofort hören.",
    about_version: "Version {v}",
    about_license: "Freie Software unter der GPL-3.0",
    about_engine: "Basiert auf waveflow-core, der Desktop-Engine von WaveFlow",
    about_source: "Quellcode und Releases",

    // ── File-manager integration ───────────────────────────────────────
    action_play_with: "Mit WaveFlow QuickPlay wiedergeben",
    action_add_to_playlist: "Zur WaveFlow QuickPlay-Wiedergabeliste hinzufügen",
    desktop_comment: "Audiodateien schnell wiedergeben",
};
