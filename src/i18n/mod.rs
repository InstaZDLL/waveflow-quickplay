//! Interface languages. Every user-visible string lives in a [`Strings`]
//! table, one `const` per language in a file of its own (`en.rs` is the
//! reference the others were translated from). Window titles the OS shows
//! (`MAIN_TITLE`, `PLAYLIST_TITLE`) stay as they are: they are also how the
//! two windows find each other.
//!
//! Placeholders are named, `{n}` `{t}` `{e}` `{v}` `{l}`, and filled with
//! [`fill`]; the shortcut in parentheses at the end of a tooltip is the key
//! itself and is not translated.

mod de;
mod en;
mod es;
mod fr;
mod it;
mod ja;
mod ko;
mod zh_cn;

/// A language the interface can be shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Fr,
    De,
    Es,
    It,
    ZhCn,
    Ja,
    Ko,
}

impl Lang {
    /// In the order the language picker lists them.
    pub const ALL: [Lang; 8] = [
        Lang::En,
        Lang::Fr,
        Lang::De,
        Lang::Es,
        Lang::It,
        Lang::ZhCn,
        Lang::Ja,
        Lang::Ko,
    ];

    /// BCP 47 tag, as stored in the settings file.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Fr => "fr",
            Lang::De => "de",
            Lang::Es => "es",
            Lang::It => "it",
            Lang::ZhCn => "zh-CN",
            Lang::Ja => "ja",
            Lang::Ko => "ko",
        }
    }

    /// The language's own name for itself, shown in the picker whatever the
    /// current language is.
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Fr => "Français",
            Lang::De => "Deutsch",
            Lang::Es => "Español",
            Lang::It => "Italiano",
            Lang::ZhCn => "简体中文",
            Lang::Ja => "日本語",
            Lang::Ko => "한국어",
        }
    }

    /// Parses a stored code; `None` for anything else, including the empty
    /// string and `"auto"` that mean "follow the system".
    pub fn from_code(code: &str) -> Option<Lang> {
        let code = code.trim();
        Lang::ALL
            .into_iter()
            .find(|l| l.code().eq_ignore_ascii_case(code))
            .or_else(|| Lang::from_locale(code).filter(|_| !code.is_empty() && code != "auto"))
    }

    /// Best match for a locale such as `fr-CH`, `zh_CN.UTF-8` or `ja`;
    /// `None` when the language is not one we have.
    fn from_locale(locale: &str) -> Option<Lang> {
        let language = locale
            .split(['-', '_', '.', '@'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        match language.as_str() {
            "en" => Some(Lang::En),
            "fr" => Some(Lang::Fr),
            "de" => Some(Lang::De),
            "es" => Some(Lang::Es),
            "it" => Some(Lang::It),
            "zh" => Some(Lang::ZhCn),
            "ja" => Some(Lang::Ja),
            "ko" => Some(Lang::Ko),
            _ => None,
        }
    }

    /// The system's language, English when it is not one we have.
    pub fn system() -> Lang {
        sys_locale::get_locales()
            .find_map(|l| Lang::from_locale(&l))
            .unwrap_or(Lang::En)
    }

    /// The language to use for a stored preference: the code when it names
    /// a language, the system's otherwise.
    pub fn resolve(preference: &str) -> Lang {
        Lang::from_code(preference).unwrap_or_else(Lang::system)
    }

    pub fn strings(self) -> &'static Strings {
        match self {
            Lang::En => &en::STRINGS,
            Lang::Fr => &fr::STRINGS,
            Lang::De => &de::STRINGS,
            Lang::Es => &es::STRINGS,
            Lang::It => &it::STRINGS,
            Lang::ZhCn => &zh_cn::STRINGS,
            Lang::Ja => &ja::STRINGS,
            Lang::Ko => &ko::STRINGS,
        }
    }
}

/// Every string the interface shows. Field order is the order they appear
/// in the player, the playlist, the status line, the file dialog, the
/// settings page and the file-manager integration.
#[derive(Debug)]
pub struct Strings {
    // ── Player pane ────────────────────────────────────────────────────
    /// Title line when the queue is empty.
    pub nothing_playing: &'static str,
    /// Subtitle when the file has neither artist nor album tag.
    pub unknown_artist: &'static str,
    /// Subtitle when the queue is empty.
    pub drop_hint: &'static str,
    /// In the artwork area when the file has no cover.
    pub no_artwork: &'static str,
    /// Overlay while files are dragged over the window.
    pub drop_to_add: &'static str,
    /// Volume tooltip; `{n}` is the percentage.
    pub volume_percent: &'static str,

    // ── Tooltips (the key in parentheses is the shortcut) ──────────────
    pub tip_play: &'static str,
    pub tip_pause: &'static str,
    pub tip_stop: &'static str,
    pub tip_previous: &'static str,
    pub tip_next: &'static str,
    pub tip_open_files: &'static str,
    pub tip_playlist: &'static str,
    pub tip_artwork: &'static str,
    pub tip_spectrum: &'static str,
    pub tip_settings: &'static str,
    pub tip_always_on_top: &'static str,
    /// The pin in a Wayland session, where pinning restarts QuickPlay
    /// under XWayland (Wayland lets only the compositor keep a window on
    /// top).
    pub tip_always_on_top_xwayland: &'static str,
    /// The pin once QuickPlay runs under XWayland for it: unpinning
    /// restarts it under Wayland.
    pub tip_always_on_top_wayland: &'static str,
    pub tip_minimize: &'static str,
    pub tip_close: &'static str,

    // ── Playlist window ────────────────────────────────────────────────
    /// Its title bar.
    pub playlist: &'static str,
    /// Summary next to the title when there is nothing queued.
    pub playlist_empty: &'static str,
    /// Summary for one track; `{t}` is its duration, `3:26`.
    pub playlist_one_track: &'static str,
    /// Summary for several; `{n}` the count, `{t}` the total duration.
    pub playlist_n_tracks: &'static str,
    pub tip_add_files: &'static str,
    pub tip_remove_selected: &'static str,
    pub tip_clear_playlist: &'static str,
    /// The repeat button, one per mode it is in.
    pub tip_repeat_off: &'static str,
    pub tip_repeat_all: &'static str,
    pub tip_repeat_one: &'static str,
    pub tip_shuffle: &'static str,
    /// Centre of the empty list.
    pub playlist_drop_hint: &'static str,
    /// Context menu of a row.
    pub menu_play: &'static str,
    pub menu_remove: &'static str,

    // ── Status line (red, under the subtitle) ──────────────────────────
    /// `{e}` is the error text from the audio backend.
    pub status_audio_unavailable: &'static str,
    pub status_scanning: &'static str,
    pub status_no_audio_files: &'static str,
    /// `{e}` is the decoder's message.
    pub status_cannot_play: &'static str,
    /// `{e}` is why WaveFlow could not be started.
    pub status_cannot_open_waveflow: &'static str,
    /// The pin was clicked in a Wayland session that has no XWayland.
    pub status_pin_needs_xwayland: &'static str,
    /// `{e}` is why QuickPlay could not start again under the other
    /// window system.
    pub status_cannot_restart: &'static str,
    /// QuickPlay restarted under the other window system (the pin under
    /// Wayland) and the state the previous process left could not be read:
    /// it runs, with an empty playlist.
    pub status_restart_state_lost: &'static str,
    /// `{v}` is the version of an update downloaded at launch, installed
    /// when QuickPlay closes.
    pub status_update_ready: &'static str,
    /// `{v}` is a newer version this copy cannot install by itself (a Linux
    /// package, a build from source); also the link to its page in the
    /// settings.
    pub status_update_available: &'static str,
    /// The button that hands playback over to WaveFlow, shown when it is
    /// installed.
    pub tip_open_in_waveflow: &'static str,

    // ── File dialog ────────────────────────────────────────────────────
    pub dialog_open_title: &'static str,
    /// Name of the "all supported audio" filter.
    pub dialog_filter_audio: &'static str,

    // ── Settings page ──────────────────────────────────────────────────
    pub settings: &'static str,
    pub language: &'static str,
    /// Picker entry that follows the system; `{l}` is the language it
    /// currently resolves to, in that language's own name.
    pub language_system: &'static str,
    /// The checkbox that turns the update check at launch on or off.
    pub settings_check_updates: &'static str,
    pub about: &'static str,
    /// One sentence under the app name.
    pub about_tagline: &'static str,
    /// `{v}` is the version number.
    pub about_version: &'static str,
    pub about_license: &'static str,
    /// Built on WaveFlow's engine crate; keep `waveflow-core` as is.
    pub about_engine: &'static str,
    /// Link to the repository.
    pub about_source: &'static str,

    // ── File-manager integration (context menu / "Open with") ──────────
    pub action_play_with: &'static str,
    pub action_add_to_playlist: &'static str,
    /// The `Comment=` of the Linux desktop entry.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub desktop_comment: &'static str,
}

/// Replaces each `{name}` placeholder of `template` with its value.
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (name, value) in values {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip_and_locales_match() {
        for lang in Lang::ALL {
            assert_eq!(Lang::from_code(lang.code()), Some(lang));
        }
        assert_eq!(Lang::from_code("fr-CH"), Some(Lang::Fr));
        assert_eq!(Lang::from_code("zh_CN.UTF-8"), Some(Lang::ZhCn));
        assert_eq!(Lang::from_code("ZH-cn"), Some(Lang::ZhCn));
        assert_eq!(Lang::from_code(""), None);
        assert_eq!(Lang::from_code("auto"), None);
        assert_eq!(Lang::from_code("pt-BR"), None);
    }

    #[test]
    fn fill_replaces_named_placeholders() {
        assert_eq!(
            fill("{n} tracks · {t}", &[("n", "3"), ("t", "10:00")]),
            "3 tracks · 10:00"
        );
    }

    /// Every language keeps the placeholders of the English template, so
    /// no `fill` call can leave a `{n}` on screen or lose a value.
    #[test]
    fn translations_keep_placeholders() {
        let en = Lang::En.strings();
        let placeholders = |s: &str| -> Vec<String> {
            let mut found: Vec<String> = s
                .match_indices('{')
                .filter_map(|(start, _)| {
                    let end = s[start..].find('}')?;
                    Some(s[start..=start + end].to_string())
                })
                .collect();
            found.sort();
            found
        };
        for lang in Lang::ALL {
            let t = lang.strings();
            for (name, reference, translated) in [
                ("volume_percent", en.volume_percent, t.volume_percent),
                (
                    "playlist_one_track",
                    en.playlist_one_track,
                    t.playlist_one_track,
                ),
                (
                    "playlist_n_tracks",
                    en.playlist_n_tracks,
                    t.playlist_n_tracks,
                ),
                (
                    "status_audio_unavailable",
                    en.status_audio_unavailable,
                    t.status_audio_unavailable,
                ),
                (
                    "status_cannot_play",
                    en.status_cannot_play,
                    t.status_cannot_play,
                ),
                (
                    "status_cannot_open_waveflow",
                    en.status_cannot_open_waveflow,
                    t.status_cannot_open_waveflow,
                ),
                (
                    "status_cannot_restart",
                    en.status_cannot_restart,
                    t.status_cannot_restart,
                ),
                (
                    "status_update_ready",
                    en.status_update_ready,
                    t.status_update_ready,
                ),
                (
                    "status_update_available",
                    en.status_update_available,
                    t.status_update_available,
                ),
                ("language_system", en.language_system, t.language_system),
                ("about_version", en.about_version, t.about_version),
            ] {
                assert_eq!(
                    placeholders(reference),
                    placeholders(translated),
                    "{name} in {}",
                    lang.code()
                );
                assert!(
                    !translated.trim().is_empty(),
                    "{name} empty in {}",
                    lang.code()
                );
            }
        }
    }
}
