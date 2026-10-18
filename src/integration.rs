//! `--register` / `--unregister`: make the file manager offer QuickPlay on
//! audio files, per user, without an installer.
//!
//! - Windows: `HKCU\Software\Classes` only. Two verbs on every audio
//!   extension and on folders ("Play with…", "Add to … playlist"), plus an
//!   `Applications\waveflow-quickplay.exe` entry so the player shows in
//!   "Open with". The default handler is left alone: that is the user's
//!   choice in Settings › Default apps.
//! - Linux: a `.desktop` file with the audio MIME types and an "Add to
//!   playlist" action, plus the icon, under `~/.local/share`.
//! - macOS: file associations need an app bundle; not done from a bare
//!   binary.
//!
//! Both directions are idempotent and print what they did.

use std::io;
use std::path::PathBuf;

use crate::i18n::{Lang, Strings};

fn exe_path() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let exe = exe.canonicalize().unwrap_or(exe);
    // `canonicalize` yields `\\?\C:\…` on Windows; the shell wants `C:\…`.
    let s = exe.to_string_lossy();
    Ok(PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s)))
}

/// `lang` is the language of the menu labels: on Windows the registry holds
/// one label per verb, so the labels are written in the interface language
/// in force at registration (and rewritten by [`relabel`] when it changes);
/// the Linux desktop entry carries every language and the desktop picks.
pub fn register(lang: Lang) -> io::Result<String> {
    platform::register(&exe_path()?, lang.strings())
}

pub fn unregister() -> io::Result<String> {
    platform::unregister()
}

/// Rewrites the file-manager labels in `lang` when this executable is the
/// registered one, so a language picked in the settings reaches the
/// context menu too. `Ok(false)` when nothing is registered, or another
/// copy of QuickPlay is (a development build must not hijack the menu of
/// the installed one). A no-op where the entries already carry every
/// language.
pub fn relabel(lang: Lang) -> io::Result<bool> {
    let exe = exe_path()?;
    if !platform::is_registered_at(&exe) {
        return Ok(false);
    }
    platform::register(&exe, lang.strings())?;
    Ok(true)
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::process::Command;
    use waveflow_core::scanner::extract::AUDIO_EXTENSIONS;

    const PLAY_VERB: &str = "WaveFlowQuickPlay.Play";
    const ENQUEUE_VERB: &str = "WaveFlowQuickPlay.Enqueue";
    const APP_KEY: &str = r"HKEY_CURRENT_USER\Software\Classes\Applications\waveflow-quickplay.exe";

    /// `.reg` string syntax: backslashes and quotes escaped.
    fn reg_str(s: &str) -> String {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }

    fn verb_keys(t: &Strings) -> Vec<(String, &'static str, &'static str, bool)> {
        // (key under HKCU\Software\Classes, label, extra arg, is_folder)
        let mut out = Vec::new();
        for ext in AUDIO_EXTENSIONS {
            let base = format!(r"SystemFileAssociations\.{ext}\shell");
            out.push((
                format!(r"{base}\{PLAY_VERB}"),
                t.action_play_with,
                "",
                false,
            ));
            out.push((
                format!(r"{base}\{ENQUEUE_VERB}"),
                t.action_add_to_playlist,
                "--enqueue",
                false,
            ));
        }
        out.push((
            format!(r"Directory\shell\{PLAY_VERB}"),
            t.action_play_with,
            "",
            true,
        ));
        out.push((
            format!(r"Directory\shell\{ENQUEUE_VERB}"),
            t.action_add_to_playlist,
            "--enqueue",
            true,
        ));
        out
    }

    fn registration_script(exe: &std::path::Path, t: &Strings) -> String {
        let exe = exe.to_string_lossy();
        let icon = format!("\"{exe}\",0");
        let mut s = String::from("Windows Registry Editor Version 5.00\r\n\r\n");
        for (key, label, arg, is_folder) in verb_keys(t) {
            let full = format!(r"HKEY_CURRENT_USER\Software\Classes\{key}");
            s.push_str(&format!(
                "[{full}]\r\n@={}\r\n\"Icon\"={}\r\n",
                reg_str(label),
                reg_str(&icon)
            ));
            if !is_folder {
                // One launch per selected file; the single-instance socket
                // merges them into the open player.
                s.push_str("\"MultiSelectModel\"=\"Player\"\r\n");
            }
            let cmd = if arg.is_empty() {
                format!("\"{exe}\" \"%1\"")
            } else {
                format!("\"{exe}\" {arg} \"%1\"")
            };
            s.push_str(&format!(
                "\r\n[{full}\\command]\r\n@={}\r\n\r\n",
                reg_str(&cmd)
            ));
        }
        // "Open with" entry.
        s.push_str(&format!(
            "[{APP_KEY}]\r\n\"FriendlyAppName\"=\"WaveFlow QuickPlay\"\r\n\r\n[{APP_KEY}\\DefaultIcon]\r\n@={}\r\n\r\n[{APP_KEY}\\shell\\open\\command]\r\n@={}\r\n\r\n[{APP_KEY}\\SupportedTypes]\r\n",
            reg_str(&icon),
            reg_str(&format!("\"{exe}\" \"%1\""))
        ));
        for ext in AUDIO_EXTENSIONS {
            s.push_str(&format!("\".{ext}\"=\"\"\r\n"));
        }
        s.push_str("\r\n");
        s
    }

    fn removal_script() -> String {
        let mut s = String::from("Windows Registry Editor Version 5.00\r\n\r\n");
        // Keys are language-independent; any table gives the same list.
        for (key, ..) in verb_keys(Lang::En.strings()) {
            s.push_str(&format!(
                "[-HKEY_CURRENT_USER\\Software\\Classes\\{key}]\r\n\r\n"
            ));
        }
        s.push_str(&format!("[-{APP_KEY}]\r\n\r\n"));
        s
    }

    /// The executable the "Open with" entry points at, when QuickPlay is
    /// registered for this user: the quoted path at the start of
    /// `Applications\waveflow-quickplay.exe\shell\open\command`.
    fn registered_exe() -> Option<PathBuf> {
        use windows_sys::Win32::System::Registry::{
            RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ,
        };
        let subkey: Vec<u16> =
            r"Software\Classes\Applications\waveflow-quickplay.exe\shell\open\command"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
        let mut buf = [0u16; 2048];
        let mut size = (buf.len() * 2) as u32;
        // SAFETY: documented call; `buf` and `size` describe a buffer we
        // own, and `size` comes back as the bytes written (NUL included).
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                std::ptr::null(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status != 0 {
            return None;
        }
        let len = (size as usize / 2).saturating_sub(1).min(buf.len());
        let command = String::from_utf16_lossy(&buf[..len]);
        let rest = command.strip_prefix('"')?;
        let end = rest.find('"')?;
        Some(PathBuf::from(&rest[..end]))
    }

    pub fn is_registered_at(exe: &std::path::Path) -> bool {
        registered_exe().is_some_and(|registered| {
            registered
                .to_string_lossy()
                .eq_ignore_ascii_case(&exe.to_string_lossy())
        })
    }

    fn import(script: &str) -> io::Result<()> {
        use std::os::windows::process::CommandExt;
        // reg.exe reads ANSI or UTF-16LE with BOM; the path may be non-ASCII.
        let mut bytes: Vec<u8> = vec![0xFF, 0xFE];
        for unit in script.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let path =
            std::env::temp_dir().join(format!("waveflow-quickplay-{}.reg", std::process::id()));
        std::fs::write(&path, bytes)?;
        // CREATE_NO_WINDOW: called from the GUI too, where a console
        // window flashing up for reg.exe would be a surprise.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let status = Command::new("reg")
            .arg("import")
            .arg(&path)
            .creation_flags(CREATE_NO_WINDOW)
            .status();
        let _ = std::fs::remove_file(&path);
        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(s) => Err(io::Error::other(format!("reg import exited with {s}"))),
            Err(e) => Err(e),
        }
    }

    fn notify_shell() {
        use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
        // SAFETY: documented call with null item pointers for this event.
        unsafe {
            SHChangeNotify(
                SHCNE_ASSOCCHANGED as i32,
                SHCNF_IDLIST,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
    }

    pub fn register(exe: &std::path::Path, t: &Strings) -> io::Result<String> {
        import(&registration_script(exe, t))?;
        notify_shell();
        Ok(format!(
            "registered for the current user: \"{}\" and \"{}\" on {} audio extensions and on folders, plus an \"Open with\" entry ({}).",
            t.action_play_with,
            t.action_add_to_playlist,
            AUDIO_EXTENSIONS.len(),
            exe.display()
        ))
    }

    pub fn unregister() -> io::Result<String> {
        import(&removal_script())?;
        notify_shell();
        Ok(
            "removed the context-menu verbs and the \"Open with\" entry for the current user."
                .into(),
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn script_escapes_paths() {
            let s = registration_script(
                std::path::Path::new(r"C:\Program Files\QP\waveflow-quickplay.exe"),
                Lang::Fr.strings(),
            );
            assert!(s.contains("@=\"Lire avec WaveFlow QuickPlay\""));
            assert!(s.starts_with("Windows Registry Editor Version 5.00\r\n"));
            assert!(s.contains(r#"@="\"C:\\Program Files\\QP\\waveflow-quickplay.exe\" \"%1\"""#));
            assert!(s.contains(
                r#"@="\"C:\\Program Files\\QP\\waveflow-quickplay.exe\" --enqueue \"%1\"""#
            ));
            assert!(s.contains(r"[HKEY_CURRENT_USER\Software\Classes\SystemFileAssociations\.flac\shell\WaveFlowQuickPlay.Play\command]"));
            assert!(s.contains("\".dsf\"=\"\""));
            let r = removal_script();
            assert!(r.contains(
                r"[-HKEY_CURRENT_USER\Software\Classes\Directory\shell\WaveFlowQuickPlay.Enqueue]"
            ));
            assert!(
                !r.contains(r"[-HKEY_CURRENT_USER\Software\Classes\SystemFileAssociations\.flac]")
            );
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;

    const DESKTOP_ID: &str = "app.waveflow.quickplay";
    const MIME_TYPES: &str = "audio/flac;audio/mpeg;audio/mp4;audio/x-m4a;audio/aac;audio/x-aac;audio/ogg;audio/vorbis;audio/x-vorbis+ogg;audio/opus;audio/x-opus+ogg;audio/wav;audio/x-wav;audio/vnd.wave;audio/aiff;audio/x-aiff;audio/x-dsf;audio/x-dff;audio/x-dsd;";

    fn data_dir() -> io::Result<PathBuf> {
        dirs::data_dir().ok_or_else(|| io::Error::other("no XDG data directory"))
    }

    fn desktop_file() -> io::Result<PathBuf> {
        Ok(data_dir()?
            .join("applications")
            .join(format!("{DESKTOP_ID}.desktop")))
    }

    fn icon_file() -> io::Result<PathBuf> {
        Ok(data_dir()?
            .join("icons/hicolor/128x128/apps")
            .join(format!("{DESKTOP_ID}.png")))
    }

    /// Desktop-entry quoting for the Exec key.
    fn exec_quote(s: &str) -> String {
        let mut out = String::from("\"");
        for c in s.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                out.push('\\');
            }
            out.push(c);
        }
        out.push('"');
        out
    }

    /// The desktop entry carries every language, so there is never
    /// anything to rewrite.
    pub fn is_registered_at(_exe: &std::path::Path) -> bool {
        false
    }

    fn refresh() {
        if let Ok(dir) = data_dir() {
            let _ = std::process::Command::new("update-desktop-database")
                .arg(dir.join("applications"))
                .status();
        }
    }

    /// `Key=value` in English, then `Key[lang]=value` for every other
    /// language, as the desktop entry spec localises keys; the desktop
    /// shows the one matching the session's locale.
    fn localised(key: &str, pick: impl Fn(&Strings) -> &'static str) -> String {
        let mut out = format!("{key}={}\n", pick(Lang::En.strings()));
        for lang in Lang::ALL {
            if lang != Lang::En {
                // `zh-CN` is `zh_CN` in desktop entries.
                let tag = lang.code().replace('-', "_");
                out.push_str(&format!("{key}[{tag}]={}\n", pick(lang.strings())));
            }
        }
        out
    }

    pub fn register(exe: &std::path::Path, _t: &Strings) -> io::Result<String> {
        let exe_s = exe.to_string_lossy();
        let quoted = exec_quote(&exe_s);
        let comment = localised("Comment", |t| t.desktop_comment);
        let enqueue = localised("Name", |t| t.action_add_to_playlist);
        let entry = format!(
            "[Desktop Entry]\nType=Application\nName=WaveFlow QuickPlay\n{comment}Exec={quoted} %F\nTryExec={exe_s}\nIcon={DESKTOP_ID}\nTerminal=false\nCategories=AudioVideo;Audio;Player;\nMimeType={MIME_TYPES}\nStartupWMClass={DESKTOP_ID}\nActions=Enqueue;\n\n[Desktop Action Enqueue]\n{enqueue}Exec={quoted} --enqueue %F\n"
        );
        let desktop = desktop_file()?;
        let icon = icon_file()?;
        for p in [&desktop, &icon] {
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(&desktop, entry)?;
        std::fs::write(&icon, include_bytes!("../assets/icon-128.png"))?;
        refresh();
        Ok(format!(
            "wrote {} and its icon; QuickPlay now appears in \"Open with\" for audio files. To make it the default: xdg-mime default {DESKTOP_ID}.desktop audio/flac (per type).",
            desktop.display()
        ))
    }

    pub fn unregister() -> io::Result<String> {
        for p in [desktop_file()?, icon_file()?] {
            if let Err(e) = std::fs::remove_file(&p) {
                if p.exists() {
                    return Err(e);
                }
            }
        }
        refresh();
        Ok("removed the desktop entry and the icon.".into())
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use super::*;

    pub fn is_registered_at(_exe: &std::path::Path) -> bool {
        false
    }

    pub fn register(_exe: &std::path::Path, _t: &Strings) -> io::Result<String> {
        Err(io::Error::other(
            "file associations on this platform need an application bundle; not available from a bare binary.",
        ))
    }

    pub fn unregister() -> io::Result<String> {
        Err(io::Error::other("nothing to remove on this platform."))
    }
}
