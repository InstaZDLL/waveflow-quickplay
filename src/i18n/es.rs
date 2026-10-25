//! Spanish.

use super::Strings;

pub const STRINGS: Strings = Strings {
    // ── Player pane ────────────────────────────────────────────────────
    nothing_playing: "Nada en reproducción",
    unknown_artist: "Artista desconocido",
    drop_hint: "Arrastra archivos de audio aquí o pulsa Ctrl+O",
    no_artwork: "Sin carátula",
    drop_to_add: "Suelta para añadir a la lista",
    volume_percent: "Volumen {n} %",

    // ── Tooltips ───────────────────────────────────────────────────────
    tip_play: "Reproducir (Espacio)",
    tip_pause: "Pausar (Espacio)",
    tip_stop: "Detener",
    tip_previous: "Anterior (P)",
    tip_next: "Siguiente (N)",
    tip_open_files: "Abrir archivos (Ctrl+O)",
    tip_playlist: "Lista de reproducción (L)",
    tip_artwork: "Carátula (A)",
    tip_spectrum: "Espectro (S)",
    tip_settings: "Ajustes",
    tip_always_on_top: "Siempre encima (T)",
    tip_always_on_top_xwayland: "Siempre encima (T): reinicia QuickPlay en XWayland",
    tip_always_on_top_wayland: "Siempre encima (T): al desactivarlo, QuickPlay se reinicia en Wayland",
    tip_minimize: "Minimizar",
    tip_close: "Cerrar",

    // ── Playlist window ────────────────────────────────────────────────
    playlist: "Lista de reproducción",
    playlist_empty: "Vacía",
    playlist_one_track: "1 pista · {t}",
    playlist_n_tracks: "{n} pistas · {t}",
    tip_add_files: "Añadir archivos",
    tip_remove_selected: "Quitar seleccionados (Supr)",
    tip_clear_playlist: "Vaciar la lista",
    tip_repeat_off: "Repetición desactivada (R)",
    tip_repeat_all: "Repetir todo (R)",
    tip_repeat_one: "Repetir pista (R)",
    tip_shuffle: "Reproducción aleatoria (H)",
    playlist_drop_hint: "Arrastra archivos aquí o pulsa +",
    menu_play: "Reproducir",
    menu_remove: "Quitar",

    // ── Status line ────────────────────────────────────────────────────
    status_audio_unavailable: "Audio no disponible: {e}",
    status_scanning: "Explorando carpetas…",
    status_no_audio_files: "Sin archivos de audio compatibles",
    status_cannot_play: "No se puede reproducir: {e}",
    status_cannot_open_waveflow: "No se puede abrir WaveFlow: {e}",
    status_pin_needs_xwayland: "Siempre encima necesita XWayland, que esta sesión no tiene",
    status_cannot_restart: "No se puede reiniciar QuickPlay: {e}",
    tip_open_in_waveflow: "Continuar en WaveFlow",

    // ── File dialog ────────────────────────────────────────────────────
    dialog_open_title: "Abrir archivos de audio",
    dialog_filter_audio: "Audio",

    // ── Settings page ──────────────────────────────────────────────────
    settings: "Ajustes",
    language: "Idioma",
    language_system: "Idioma del sistema ({l})",
    about: "Acerca de",
    about_tagline: "El pequeño reproductor que acompaña a WaveFlow: haz doble clic en una pista y escúchala al instante.",
    about_version: "Versión {v}",
    about_license: "Software libre, licencia GPL-3.0",
    about_engine: "Basado en waveflow-core, el motor de escritorio de WaveFlow",
    about_source: "Código fuente y versiones",

    // ── File-manager integration ───────────────────────────────────────
    action_play_with: "Reproducir con QuickPlay",
    action_add_to_playlist: "Añadir a la lista de QuickPlay",
    desktop_comment: "Reproduce archivos de audio al instante",
};
