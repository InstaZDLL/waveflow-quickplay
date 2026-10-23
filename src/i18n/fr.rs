//! French.

use super::Strings;

pub const STRINGS: Strings = Strings {
    // ── Player pane ────────────────────────────────────────────────────
    nothing_playing: "Aucune lecture en cours",
    unknown_artist: "Artiste inconnu",
    drop_hint: "Déposez des fichiers audio ici ou appuyez sur Ctrl+O",
    no_artwork: "Aucune pochette",
    drop_to_add: "Déposez pour ajouter à la liste de lecture",
    volume_percent: "Volume {n}\u{a0}%",

    // ── Tooltips ───────────────────────────────────────────────────────
    tip_play: "Lire (Espace)",
    tip_pause: "Pause (Espace)",
    tip_stop: "Arrêter",
    tip_previous: "Précédent (P)",
    tip_next: "Suivant (N)",
    tip_open_files: "Ouvrir des fichiers (Ctrl+O)",
    tip_playlist: "Liste de lecture (L)",
    tip_artwork: "Pochette (A)",
    tip_spectrum: "Spectre (S)",
    tip_settings: "Paramètres",
    tip_always_on_top: "Toujours au premier plan (T)",
    tip_minimize: "Réduire",
    tip_close: "Fermer",

    // ── Playlist window ────────────────────────────────────────────────
    playlist: "Liste de lecture",
    playlist_empty: "Vide",
    playlist_one_track: "1 titre · {t}",
    playlist_n_tracks: "{n} titres · {t}",
    tip_add_files: "Ajouter des fichiers",
    tip_remove_selected: "Retirer la sélection (Suppr)",
    tip_clear_playlist: "Vider la liste de lecture",
    tip_repeat_off: "Répétition désactivée (R)",
    tip_repeat_all: "Répéter tout (R)",
    tip_repeat_one: "Répéter la piste (R)",
    tip_shuffle: "Lecture aléatoire (H)",
    playlist_drop_hint: "Déposez des fichiers ici ou appuyez sur +",
    menu_play: "Lire",
    menu_remove: "Retirer",

    // ── Status line ────────────────────────────────────────────────────
    status_audio_unavailable: "Audio indisponible\u{a0}: {e}",
    status_scanning: "Analyse des dossiers…",
    status_no_audio_files: "Aucun fichier audio compatible",
    status_cannot_play: "Lecture impossible\u{a0}: {e}",
    status_cannot_open_waveflow: "Impossible d’ouvrir WaveFlow\u{a0}: {e}",
    tip_open_in_waveflow: "Continuer dans WaveFlow",

    // ── File dialog ────────────────────────────────────────────────────
    dialog_open_title: "Ouvrir des fichiers audio",
    dialog_filter_audio: "Audio",

    // ── Settings page ──────────────────────────────────────────────────
    settings: "Paramètres",
    language: "Langue",
    language_system: "Langue du système ({l})",
    about: "À propos",
    about_tagline: "Le petit lecteur qui accompagne WaveFlow\u{a0}: double-cliquez sur un titre, écoutez-le aussitôt.",
    about_version: "Version {v}",
    about_license: "Logiciel libre, licence GPL-3.0",
    about_engine: "Basé sur waveflow-core, le moteur de WaveFlow",
    about_source: "Code source et versions",

    // ── File-manager integration ───────────────────────────────────────
    action_play_with: "Lire avec WaveFlow QuickPlay",
    action_add_to_playlist: "Ajouter à la liste de lecture de WaveFlow QuickPlay",
    desktop_comment: "Lire rapidement des fichiers audio",
};
