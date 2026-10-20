//! Korean.

use super::Strings;

pub const STRINGS: Strings = Strings {
    // ── Player pane ────────────────────────────────────────────────────
    nothing_playing: "재생 중인 곡 없음",
    unknown_artist: "알 수 없는 아티스트",
    drop_hint: "오디오 파일을 여기에 끌어다 놓거나 Ctrl+O를 누르세요",
    no_artwork: "앨범 아트 없음",
    drop_to_add: "놓으면 재생목록에 추가됩니다",
    volume_percent: "볼륨 {n}%",

    // ── Tooltips ───────────────────────────────────────────────────────
    tip_play: "재생 (Space)",
    tip_pause: "일시 정지 (Space)",
    tip_stop: "정지",
    tip_previous: "이전 곡 (P)",
    tip_next: "다음 곡 (N)",
    tip_open_files: "파일 열기 (Ctrl+O)",
    tip_playlist: "재생목록 (L)",
    tip_artwork: "앨범 아트 (A)",
    tip_spectrum: "스펙트럼 (S)",
    tip_settings: "설정",
    tip_always_on_top: "항상 위에 표시 (T)",
    tip_minimize: "최소화",
    tip_close: "닫기",

    // ── Playlist window ────────────────────────────────────────────────
    playlist: "재생목록",
    playlist_empty: "비어 있음",
    playlist_one_track: "1곡 · {t}",
    playlist_n_tracks: "{n}곡 · {t}",
    tip_add_files: "파일 추가",
    tip_remove_selected: "선택 항목 제거 (Delete)",
    tip_clear_playlist: "재생목록 비우기",
    tip_repeat_off: "반복 끔 (R)",
    tip_repeat_all: "전체 반복 (R)",
    tip_repeat_one: "한 곡 반복 (R)",
    tip_shuffle: "셔플 재생 (H)",
    playlist_drop_hint: "파일을 여기에 끌어다 놓거나 +를 누르세요",
    menu_play: "재생",
    menu_remove: "제거",

    // ── Status line ────────────────────────────────────────────────────
    status_audio_unavailable: "오디오를 사용할 수 없음: {e}",
    status_scanning: "폴더 검색 중…",
    status_no_audio_files: "지원되는 오디오 파일 없음",
    status_cannot_play: "재생할 수 없음: {e}",

    // ── File dialog ────────────────────────────────────────────────────
    dialog_open_title: "오디오 파일 열기",
    dialog_filter_audio: "오디오",

    // ── Settings page ──────────────────────────────────────────────────
    settings: "설정",
    language: "언어",
    language_system: "시스템 언어 ({l})",
    about: "정보",
    about_tagline: "WaveFlow와 함께하는 작은 플레이어: 곡을 더블 클릭하면 바로 재생됩니다.",
    about_version: "버전 {v}",
    about_license: "자유 소프트웨어, GPL-3.0 라이선스",
    about_engine: "WaveFlow 데스크톱 엔진 waveflow-core 기반",
    about_source: "소스 코드 및 릴리스",

    // ── File-manager integration ───────────────────────────────────────
    action_play_with: "WaveFlow QuickPlay로 재생",
    action_add_to_playlist: "WaveFlow QuickPlay 재생목록에 추가",
    desktop_comment: "오디오 파일을 빠르게 재생합니다",
};
