//! Simplified Chinese.

use super::Strings;

pub const STRINGS: Strings = Strings {
    // ── Player pane ────────────────────────────────────────────────────
    nothing_playing: "当前未播放",
    unknown_artist: "未知艺人",
    drop_hint: "将音频文件拖到这里，或按 Ctrl+O",
    no_artwork: "无封面",
    drop_to_add: "松开即可添加到播放列表",
    volume_percent: "音量 {n}%",

    // ── Tooltips ───────────────────────────────────────────────────────
    tip_play: "播放 (空格)",
    tip_pause: "暂停 (空格)",
    tip_stop: "停止",
    tip_previous: "上一曲 (P)",
    tip_next: "下一曲 (N)",
    tip_open_files: "打开文件 (Ctrl+O)",
    tip_playlist: "播放列表 (L)",
    tip_artwork: "封面 (A)",
    tip_spectrum: "频谱 (S)",
    tip_settings: "设置",
    tip_always_on_top: "始终置顶 (T)",
    tip_minimize: "最小化",
    tip_close: "关闭",

    // ── Playlist window ────────────────────────────────────────────────
    playlist: "播放列表",
    playlist_empty: "暂无曲目",
    playlist_one_track: "1 首 · {t}",
    playlist_n_tracks: "{n} 首 · {t}",
    tip_add_files: "添加文件",
    tip_remove_selected: "移除所选 (Delete)",
    tip_clear_playlist: "清空播放列表",
    tip_repeat_off: "循环关闭 (R)",
    tip_repeat_all: "列表循环 (R)",
    tip_repeat_one: "单曲循环 (R)",
    tip_shuffle: "随机播放 (H)",
    playlist_drop_hint: "将文件拖到这里，或点击 +",
    menu_play: "播放",
    menu_remove: "移除",

    // ── Status line ────────────────────────────────────────────────────
    status_audio_unavailable: "音频不可用：{e}",
    status_scanning: "正在扫描文件夹…",
    status_no_audio_files: "未找到支持的音频文件",
    status_cannot_play: "无法播放：{e}",
    status_cannot_open_waveflow: "无法打开 WaveFlow：{e}",
    tip_open_in_waveflow: "在 WaveFlow 中继续播放",

    // ── File dialog ────────────────────────────────────────────────────
    dialog_open_title: "打开音频文件",
    dialog_filter_audio: "音频文件",

    // ── Settings page ──────────────────────────────────────────────────
    settings: "设置",
    language: "语言",
    language_system: "跟随系统 ({l})",
    about: "关于",
    about_tagline: "WaveFlow 的轻巧播放器：双击曲目，即刻播放。",
    about_version: "版本 {v}",
    about_license: "自由软件，GPL-3.0 许可证",
    about_engine: "基于 WaveFlow 桌面引擎 waveflow-core 构建",
    about_source: "源代码与发布版本",

    // ── File-manager integration ───────────────────────────────────────
    action_play_with: "使用 QuickPlay 播放",
    action_add_to_playlist: "添加到 QuickPlay 播放列表",
    desktop_comment: "快速播放音频文件",
};
