//! Italian.

use super::Strings;

pub const STRINGS: Strings = Strings {
    // ── Player pane ────────────────────────────────────────────────────
    nothing_playing: "Nessun brano in riproduzione",
    unknown_artist: "Artista sconosciuto",
    drop_hint: "Trascina qui i file audio o premi Ctrl+O",
    no_artwork: "Nessuna copertina",
    drop_to_add: "Rilascia per aggiungere alla playlist",
    volume_percent: "Volume {n}%",

    // ── Tooltips ───────────────────────────────────────────────────────
    tip_play: "Riproduci (Spazio)",
    tip_pause: "Pausa (Spazio)",
    tip_stop: "Interrompi",
    tip_previous: "Precedente (P)",
    tip_next: "Successivo (N)",
    tip_open_files: "Apri file (Ctrl+O)",
    tip_playlist: "Playlist (L)",
    tip_artwork: "Copertina (A)",
    tip_spectrum: "Spettro (S)",
    tip_settings: "Impostazioni",
    tip_always_on_top: "Sempre in primo piano (T)",
    tip_always_on_top_xwayland: "Sempre in primo piano (T): riavvia QuickPlay con XWayland",
    tip_always_on_top_wayland:
        "Sempre in primo piano (T): disattivarlo riavvia QuickPlay con Wayland",
    tip_minimize: "Riduci a icona",
    tip_close: "Chiudi",

    // ── Playlist window ────────────────────────────────────────────────
    playlist: "Playlist",
    playlist_empty: "Vuota",
    playlist_one_track: "1 brano · {t}",
    playlist_n_tracks: "{n} brani · {t}",
    tip_add_files: "Aggiungi file",
    tip_remove_selected: "Rimuovi i brani selezionati (Canc)",
    tip_clear_playlist: "Svuota playlist",
    tip_repeat_off: "Ripetizione disattivata (R)",
    tip_repeat_all: "Ripeti tutto (R)",
    tip_repeat_one: "Ripeti brano (R)",
    tip_shuffle: "Riproduzione casuale (H)",
    playlist_drop_hint: "Trascina qui i file o premi +",
    menu_play: "Riproduci",
    menu_remove: "Rimuovi",

    // ── Status line ────────────────────────────────────────────────────
    status_audio_unavailable: "Audio non disponibile: {e}",
    status_scanning: "Scansione cartelle…",
    status_no_audio_files: "Nessun file audio supportato",
    status_cannot_play: "Impossibile riprodurre: {e}",
    status_cannot_open_waveflow: "Impossibile aprire WaveFlow: {e}",
    status_pin_needs_xwayland:
        "Sempre in primo piano richiede XWayland, assente in questa sessione",
    status_cannot_restart: "Impossibile riavviare QuickPlay: {e}",
    tip_open_in_waveflow: "Continua in WaveFlow",

    // ── File dialog ────────────────────────────────────────────────────
    dialog_open_title: "Apri file audio",
    dialog_filter_audio: "Audio",

    // ── Settings page ──────────────────────────────────────────────────
    settings: "Impostazioni",
    language: "Lingua",
    language_system: "Lingua di sistema ({l})",
    about: "Informazioni",
    about_tagline:
        "Il piccolo lettore accanto a WaveFlow: doppio clic su un brano e lo ascolti subito.",
    about_version: "Versione {v}",
    about_license: "Software libero, licenza GPL-3.0-only",
    about_engine: "Basato su waveflow-core, il motore desktop di WaveFlow",
    about_source: "Codice sorgente e versioni",

    // ── File-manager integration ───────────────────────────────────────
    action_play_with: "Riproduci con QuickPlay",
    action_add_to_playlist: "Aggiungi alla playlist di QuickPlay",
    desktop_comment: "Riproduci rapidamente i file audio",
};
