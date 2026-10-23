//! Japanese.

use super::Strings;

pub const STRINGS: Strings = Strings {
    // ── Player pane ────────────────────────────────────────────────────
    nothing_playing: "再生中の曲はありません",
    unknown_artist: "不明なアーティスト",
    drop_hint: "ここにオーディオファイルをドロップ、または Ctrl+O",
    no_artwork: "アートワークなし",
    drop_to_add: "ドロップしてプレイリストに追加",
    volume_percent: "音量 {n}%",

    // ── Tooltips ───────────────────────────────────────────────────────
    tip_play: "再生（スペース）",
    tip_pause: "一時停止（スペース）",
    tip_stop: "停止",
    tip_previous: "前へ（P）",
    tip_next: "次へ（N）",
    tip_open_files: "ファイルを開く（Ctrl+O）",
    tip_playlist: "プレイリスト（L）",
    tip_artwork: "アートワーク（A）",
    tip_spectrum: "スペクトラム（S）",
    tip_settings: "設定",
    tip_always_on_top: "常に手前に表示（T）",
    tip_minimize: "最小化",
    tip_close: "閉じる",

    // ── Playlist window ────────────────────────────────────────────────
    playlist: "プレイリスト",
    playlist_empty: "曲がありません",
    playlist_one_track: "1 曲 · {t}",
    playlist_n_tracks: "{n} 曲 · {t}",
    tip_add_files: "ファイルを追加",
    tip_remove_selected: "選択項目をリストから削除（Delete）",
    tip_clear_playlist: "プレイリストをクリア",
    tip_repeat_off: "リピートオフ (R)",
    tip_repeat_all: "全曲リピート (R)",
    tip_repeat_one: "1曲リピート (R)",
    tip_shuffle: "シャッフル再生 (H)",
    playlist_drop_hint: "ここにファイルをドロップ、または + を押してください",
    menu_play: "再生",
    menu_remove: "リストから削除",

    // ── Status line ────────────────────────────────────────────────────
    status_audio_unavailable: "オーディオを利用できません：{e}",
    status_scanning: "フォルダーをスキャン中…",
    status_no_audio_files: "対応するオーディオファイルがありません",
    status_cannot_play: "再生できません：{e}",
    status_cannot_open_waveflow: "WaveFlow を開けません：{e}",
    tip_open_in_waveflow: "WaveFlow で続ける",

    // ── File dialog ────────────────────────────────────────────────────
    dialog_open_title: "オーディオファイルを開く",
    dialog_filter_audio: "オーディオ",

    // ── Settings page ──────────────────────────────────────────────────
    settings: "設定",
    language: "言語",
    language_system: "システムの言語（{l}）",
    about: "このアプリについて",
    about_tagline: "WaveFlow の小さな相棒。曲をダブルクリックするだけで、すぐに音楽を楽しめます。",
    about_version: "バージョン {v}",
    about_license: "フリーソフトウェア、GPL-3.0 ライセンス",
    about_engine: "waveflow-core（WaveFlow のデスクトップエンジン）をベースに構築",
    about_source: "ソースコードとリリース",

    // ── File-manager integration ───────────────────────────────────────
    action_play_with: "QuickPlay で再生",
    action_add_to_playlist: "QuickPlay のプレイリストに追加",
    desktop_comment: "オーディオファイルをすばやく再生",
};
