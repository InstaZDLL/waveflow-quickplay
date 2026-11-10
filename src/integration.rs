//! `--register` / `--unregister`: make the file manager offer QuickPlay on
//! audio files, per user, without an installer.
//!
//! - Windows: `HKCU\Software\Classes` only. Two verbs on every audio
//!   extension and on folders ("Play with…", "Add to … playlist"), each a
//!   `DropTarget` on a COM class the executable serves (src/drop_verb.rs),
//!   so a selection of any size reaches one process; plus an
//!   `Applications\waveflow-quickplay.exe` entry so the player shows in
//!   "Open with", one ProgID per extension carrying that format's document
//!   icon (src/file_icons.rs), and the capabilities that list QuickPlay in
//!   Settings › Default apps. The default handler is left alone: that is
//!   the user's choice there.
//! - Linux: a `.desktop` file with the audio MIME types and an "Add to
//!   playlist" action, plus the icon, under `~/.local/share`.
//! - macOS: nothing to write. The app bundle declares the audio types in
//!   its Info.plist, so Finder offers it under "Open With" once it is in
//!   Applications, and the files arrive through src/macos.rs.
//!
//! Both directions are idempotent and print what they did.

use std::io;
use std::path::PathBuf;

use crate::i18n::{Lang, Strings};

/// The path the file manager is given: this executable, or from an
/// AppImage the AppImage itself.
fn exe_path() -> io::Result<PathBuf> {
    registered_path(std::env::current_exe()?, crate::update::appimage())
}

/// `exe` is this executable, `image` the AppImage it runs from. Inside an
/// AppImage the executable lives on a mount that goes away with the
/// process, so an entry pointing there would fail once QuickPlay closes:
/// the AppImage is what the file manager must start.
fn registered_path(exe: PathBuf, image: Option<PathBuf>) -> io::Result<PathBuf> {
    let exe = image.unwrap_or(exe);
    let exe = exe.canonicalize().unwrap_or(exe);
    // The registry and the desktop entry are text: a path that is not
    // Unicode cannot be written into them unchanged, so it is refused
    // rather than registered as another, nonexistent one.
    let s = exe.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} is not valid Unicode", exe.display()),
        )
    })?;
    Ok(PathBuf::from(strip_verbatim(s)))
}

/// `canonicalize` yields `\\?\C:\…`, and `\\?\UNC\server\share\…` for a
/// network share or a mapped drive, on Windows; the shell wants `C:\…` and
/// `\\server\share\…`.
fn strip_verbatim(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(path).to_string()
    }
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
    use crate::drop_verb::{ENQUEUE_CLSID, PLAY_CLSID};
    use std::process::Command;
    use waveflow_core::scanner::extract::AUDIO_EXTENSIONS;

    // Explorer lists a key's verbs in the order of their key names, so the
    // names decide the menu: "Play with…" first, "Add to … playlist" under
    // it. `Playlist` sorts after its prefix `Play`.
    const PLAY_VERB: &str = "WaveFlowQuickPlay.Play";
    const ENQUEUE_VERB: &str = "WaveFlowQuickPlay.Playlist";
    /// The playlist verb's name up to 1.0.0, which sorted above "Play
    /// with…". Removed by every registration and removal so an upgrade
    /// does not leave the entry twice.
    const LEGACY_ENQUEUE_VERB: &str = "WaveFlowQuickPlay.Enqueue";
    const APP_KEY: &str = r"HKEY_CURRENT_USER\Software\Classes\Applications\waveflow-quickplay.exe";
    const CLSID_KEY: &str = r"HKEY_CURRENT_USER\Software\Classes\CLSID";
    const CLASSES_KEY: &str = r"HKEY_CURRENT_USER\Software\Classes";
    /// Where Settings › Default apps finds the registered players. The
    /// value is shared with every other application: only ours is written
    /// and removed, never the key.
    const REGISTERED_APPS_KEY: &str = r"HKEY_CURRENT_USER\Software\RegisteredApplications";
    const REGISTERED_APP_NAME: &str = "WaveFlow QuickPlay";
    /// Under [`APP_KEY`], so removing that key takes it too; given to
    /// `RegisteredApplications` relative to HKCU.
    const CAPABILITIES_PATH: &str =
        r"Software\Classes\Applications\waveflow-quickplay.exe\Capabilities";

    /// `.reg` string syntax: backslashes and quotes escaped.
    fn reg_str(s: &str) -> String {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }

    /// The ProgID of one extension. Windows takes a type's icon and its
    /// "Type" label from the ProgID of its default handler, so one per
    /// extension is what lets a FLAC look like a FLAC once QuickPlay plays
    /// it; the `Applications` entry has a single icon for every type.
    fn prog_id(ext: &str) -> String {
        format!("WaveFlowQuickPlay.{ext}")
    }

    /// Resource id of `ext`'s document icon in the executable (build.rs);
    /// the generic audio document for an extension without its own.
    fn icon_id(ext: &str) -> u16 {
        use crate::file_icons::{FILE_ICONS, FILE_ICON_ALIASES, GENERIC_FILE_ICON};
        let stem = FILE_ICON_ALIASES
            .iter()
            .find(|(alias, _)| alias.eq_ignore_ascii_case(ext))
            .map_or(ext, |(_, stem)| stem);
        FILE_ICONS
            .iter()
            .find(|(s, _)| s.eq_ignore_ascii_case(stem))
            .map_or(GENERIC_FILE_ICON, |(_, id)| *id)
    }

    /// Explorer's "Type" label for `ext`, in the registration language.
    fn type_label(ext: &str, t: &Strings) -> String {
        let template = if crate::library::PLAYLIST_EXTENSIONS.contains(&ext) {
            t.file_type_playlist
        } else {
            t.file_type_audio
        };
        crate::i18n::fill(template, &[("ext", &ext.to_ascii_uppercase())])
    }

    fn verb_keys(t: &Strings) -> Vec<(String, &'static str, &'static str)> {
        // (key under HKCU\Software\Classes, label, class the selection is
        // dropped on)
        let bases = crate::library::openable_extensions()
            .into_iter()
            .map(|ext| format!(r"SystemFileAssociations\.{ext}\shell"))
            .chain(std::iter::once(r"Directory\shell".to_string()));
        let mut out = Vec::new();
        for base in bases {
            out.push((
                format!(r"{base}\{PLAY_VERB}"),
                t.action_play_with,
                PLAY_CLSID,
            ));
            out.push((
                format!(r"{base}\{ENQUEUE_VERB}"),
                t.action_add_to_playlist,
                ENQUEUE_CLSID,
            ));
        }
        out
    }

    /// The two classes Explorer drops a selection on, with their names.
    const CLASSES: [(&str, &str); 2] = [
        (PLAY_CLSID, "WaveFlow QuickPlay: play"),
        (ENQUEUE_CLSID, "WaveFlow QuickPlay: add to playlist"),
    ];

    /// `[-key]` lines for the verb keys of earlier versions.
    fn legacy_removals() -> String {
        let mut s = String::new();
        for ext in AUDIO_EXTENSIONS {
            s.push_str(&format!(
                "[-HKEY_CURRENT_USER\\Software\\Classes\\SystemFileAssociations\\.{ext}\\shell\\{LEGACY_ENQUEUE_VERB}]\r\n\r\n"
            ));
        }
        s.push_str(&format!(
            "[-HKEY_CURRENT_USER\\Software\\Classes\\Directory\\shell\\{LEGACY_ENQUEUE_VERB}]\r\n\r\n"
        ));
        s
    }

    fn registration_script(exe: &std::path::Path, t: &Strings) -> String {
        let exe = exe.to_string_lossy();
        let icon = format!("\"{exe}\",0");
        let mut s = String::from("Windows Registry Editor Version 5.00\r\n\r\n");
        s.push_str(&legacy_removals());
        for (clsid, name) in CLASSES {
            // COM starts the executable with `-Embedding` when Explorer
            // asks for the class (src/drop_verb.rs).
            s.push_str(&format!(
                "[{CLSID_KEY}\\{clsid}]\r\n@={}\r\n\r\n[{CLSID_KEY}\\{clsid}\\LocalServer32]\r\n@={}\r\n\r\n",
                reg_str(name),
                reg_str(&format!("\"{exe}\""))
            ));
        }
        for (key, label, clsid) in verb_keys(t) {
            let full = format!(r"HKEY_CURRENT_USER\Software\Classes\{key}");
            // A `DropTarget` verb rather than a command: Explorer hands the
            // whole selection to one QuickPlay instead of starting one per
            // file. The command of earlier versions goes, so it cannot be
            // the one Explorer picks.
            s.push_str(&format!(
                "[-{full}\\command]\r\n\r\n[{full}]\r\n@={}\r\n\"Icon\"={}\r\n\"MultiSelectModel\"=\"Player\"\r\n\r\n[{full}\\DropTarget]\r\n\"CLSID\"={}\r\n\r\n",
                reg_str(label),
                reg_str(&icon),
                reg_str(clsid)
            ));
        }
        // "Open with" entry.
        s.push_str(&format!(
            "[{APP_KEY}]\r\n\"FriendlyAppName\"=\"WaveFlow QuickPlay\"\r\n\r\n[{APP_KEY}\\DefaultIcon]\r\n@={}\r\n\r\n[{APP_KEY}\\shell\\open\\command]\r\n@={}\r\n\r\n[{APP_KEY}\\SupportedTypes]\r\n",
            reg_str(&icon),
            reg_str(&format!("\"{exe}\" \"%1\""))
        ));
        for ext in crate::library::openable_extensions() {
            s.push_str(&format!("\".{ext}\"=\"\"\r\n"));
        }
        s.push_str("\r\n");
        // One ProgID per type, with its document icon, and the type's
        // `OpenWithProgids` naming it. REG_NONE, as Windows documents.
        let open = reg_str(&format!("\"{exe}\" \"%1\""));
        for ext in crate::library::openable_extensions() {
            let id = prog_id(ext);
            s.push_str(&format!(
                "[{CLASSES_KEY}\\{id}]\r\n@={}\r\n\r\n[{CLASSES_KEY}\\{id}\\DefaultIcon]\r\n@={}\r\n\r\n[{CLASSES_KEY}\\{id}\\shell\\open\\command]\r\n@={open}\r\n\r\n[{CLASSES_KEY}\\.{ext}\\OpenWithProgids]\r\n\"{id}\"=hex(0):\r\n\r\n",
                reg_str(&type_label(ext, t)),
                reg_str(&format!("\"{exe}\",-{}", icon_id(ext))),
            ));
        }
        // Settings › Default apps lists the players named under
        // `RegisteredApplications`, with the types their capabilities map
        // to a ProgID; picking QuickPlay there sets those ProgIDs.
        s.push_str(&format!(
            "[{APP_KEY}\\Capabilities]\r\n\"ApplicationName\"={}\r\n\"ApplicationDescription\"={}\r\n\"ApplicationIcon\"={}\r\n\r\n[{APP_KEY}\\Capabilities\\FileAssociations]\r\n",
            reg_str(REGISTERED_APP_NAME),
            reg_str(t.desktop_comment),
            reg_str(&format!("{exe},0")),
        ));
        for ext in crate::library::openable_extensions() {
            s.push_str(&format!("\".{ext}\"={}\r\n", reg_str(&prog_id(ext))));
        }
        s.push_str(&format!(
            "\r\n[{REGISTERED_APPS_KEY}]\r\n{}={}\r\n\r\n",
            reg_str(REGISTERED_APP_NAME),
            reg_str(CAPABILITIES_PATH)
        ));
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
        s.push_str(&legacy_removals());
        for (clsid, _) in CLASSES {
            s.push_str(&format!("[-{CLSID_KEY}\\{clsid}]\r\n\r\n"));
        }
        s.push_str(&format!("[-{APP_KEY}]\r\n\r\n"));
        // The ProgIDs are ours and go whole. `.ext\OpenWithProgids` and
        // `RegisteredApplications` are shared: only our value goes (`.reg`
        // has no way to drop a key only once it is empty, so an
        // `OpenWithProgids` key this registration created stays, empty).
        for ext in crate::library::openable_extensions() {
            let id = prog_id(ext);
            s.push_str(&format!(
                "[-{CLASSES_KEY}\\{id}]\r\n\r\n[{CLASSES_KEY}\\.{ext}\\OpenWithProgids]\r\n\"{id}\"=-\r\n\r\n"
            ));
        }
        s.push_str(&format!(
            "[{REGISTERED_APPS_KEY}]\r\n{}=-\r\n\r\n",
            reg_str(REGISTERED_APP_NAME)
        ));
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
        let path = write_new_script(&bytes)?;
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

    /// A script file of this call's own: two imports can run at once (two
    /// quick language changes each relabel on a thread), and neither may
    /// overwrite or delete the other's. Created new, never opened over a
    /// file that is already there.
    fn write_new_script(bytes: &[u8]) -> io::Result<PathBuf> {
        use std::io::Write;
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        loop {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("waveflow-quickplay-{}-{n}.reg", std::process::id()));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    if let Err(e) = file.write_all(bytes) {
                        drop(file);
                        let _ = std::fs::remove_file(&path);
                        return Err(e);
                    }
                    return Ok(path);
                }
                // Left by an earlier process with the same id: next name.
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists && n < 1000 => {}
                Err(e) => return Err(e),
            }
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
            "registered for the current user: \"{}\" and \"{}\" on {} audio and playlist extensions and on folders, plus an \"Open with\" entry and a Default apps entry with one icon per type ({}).",
            t.action_play_with,
            t.action_add_to_playlist,
            crate::library::openable_extensions().len(),
            exe.display()
        ))
    }

    pub fn unregister() -> io::Result<String> {
        import(&removal_script())?;
        notify_shell();
        Ok(
            "removed the context-menu verbs, the \"Open with\" entry and the Default apps entry for the current user."
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
            assert!(s.contains("@=\"Lire avec QuickPlay\""));
            assert!(s.starts_with("Windows Registry Editor Version 5.00\r\n"));
            // "Open with" keeps a command line.
            assert!(s.contains(r#"@="\"C:\\Program Files\\QP\\waveflow-quickplay.exe\" \"%1\"""#));
            assert!(s.contains("\".dsf\"=\"\""));
            assert!(s.contains("\".m3u8\"=\"\""));
            assert!(s.contains(r"SystemFileAssociations\.m3u\shell\WaveFlowQuickPlay.Play"));
            // The verbs are drop targets, served by the executable.
            for clsid in [PLAY_CLSID, ENQUEUE_CLSID] {
                assert!(s.contains(&format!(
                    "[HKEY_CURRENT_USER\\Software\\Classes\\CLSID\\{clsid}\\LocalServer32]\r\n@=\"\\\"C:\\\\Program Files\\\\QP\\\\waveflow-quickplay.exe\\\"\"\r\n"
                )));
            }
            let play = r"HKEY_CURRENT_USER\Software\Classes\SystemFileAssociations\.flac\shell\WaveFlowQuickPlay.Play";
            let enqueue =
                r"HKEY_CURRENT_USER\Software\Classes\Directory\shell\WaveFlowQuickPlay.Playlist";
            for (key, clsid) in [(play, PLAY_CLSID), (enqueue, ENQUEUE_CLSID)] {
                assert!(s.contains(&format!("[{key}\\DropTarget]\r\n\"CLSID\"=\"{clsid}\"\r\n")));
                assert!(s.contains(&format!("[{key}]\r\n@=")));
                // The 1.2.0 command goes, so the drop target is used.
                assert!(s.contains(&format!("[-{key}\\command]")));
            }
            assert_eq!(
                s.matches("\"MultiSelectModel\"=\"Player\"").count(),
                2 * (crate::library::openable_extensions().len() + 1)
            );
            assert!(!s.contains("--enqueue"));
            // The 1.0.0 key goes on every registration.
            assert!(s.contains(
                r"[-HKEY_CURRENT_USER\Software\Classes\Directory\shell\WaveFlowQuickPlay.Enqueue]"
            ));
            let r = removal_script();
            assert!(r.contains(
                r"[-HKEY_CURRENT_USER\Software\Classes\Directory\shell\WaveFlowQuickPlay.Playlist]"
            ));
            assert!(r.contains(
                r"[-HKEY_CURRENT_USER\Software\Classes\Directory\shell\WaveFlowQuickPlay.Enqueue]"
            ));
            assert!(
                !r.contains(r"[-HKEY_CURRENT_USER\Software\Classes\SystemFileAssociations\.flac]")
            );
            for clsid in [PLAY_CLSID, ENQUEUE_CLSID] {
                assert!(r.contains(&format!(
                    "[-HKEY_CURRENT_USER\\Software\\Classes\\CLSID\\{clsid}]"
                )));
            }
        }

        /// Shared with other applications: the removal takes our values
        /// out of these keys, never the keys.
        fn is_shared_key(key: &str) -> bool {
            key == REGISTERED_APPS_KEY || key.ends_with(r"\OpenWithProgids")
        }

        /// Everything the registration writes, the removal takes away: each
        /// key it creates sits under one the removal deletes, and each value
        /// it puts in a shared key is deleted from that key.
        #[test]
        fn removal_covers_every_registered_key() {
            let s = registration_script(
                std::path::Path::new(r"C:\QP\waveflow-quickplay.exe"),
                Lang::En.strings(),
            );
            let removal = removal_script();
            let removed: Vec<&str> = removal
                .lines()
                .filter_map(|l| l.strip_prefix("[-")?.strip_suffix(']'))
                .collect();
            let mut shared_values = 0;
            let mut key = "";
            for line in s.lines() {
                if line.starts_with("[HKEY") {
                    key = line.trim_matches(['[', ']']);
                    if !is_shared_key(key) {
                        assert!(
                            removed
                                .iter()
                                .any(|r| key == *r || key.starts_with(&format!("{r}\\"))),
                            "{key} is not removed"
                        );
                    }
                } else if is_shared_key(key) && line.starts_with('"') {
                    let name = &line[..line.find("\"=").expect("a value line") + 1];
                    assert!(
                        removal.contains(&format!("[{key}]\r\n{name}=-\r\n")),
                        "{name} in {key} is not removed"
                    );
                    shared_values += 1;
                }
            }
            assert_eq!(
                shared_values,
                crate::library::openable_extensions().len() + 1
            );
            // A shared key is never deleted whole.
            assert!(!removed.iter().any(|r| is_shared_key(r)));
            assert!(!removed.iter().any(|r| r.ends_with(r"\.flac")));
        }

        /// Each type gets a ProgID with its own document icon, a label in
        /// the registration language, and a place in Default apps.
        #[test]
        fn every_type_has_a_prog_id_with_its_icon() {
            let s = registration_script(
                std::path::Path::new(r"C:\QP\waveflow-quickplay.exe"),
                Lang::Fr.strings(),
            );
            for ext in crate::library::openable_extensions() {
                let id = prog_id(ext);
                assert!(s.contains(&format!(
                    "[{CLASSES_KEY}\\{id}\\DefaultIcon]\r\n@=\"\\\"C:\\\\QP\\\\waveflow-quickplay.exe\\\",-{}\"\r\n",
                    icon_id(ext)
                )));
                assert!(s.contains(&format!(
                    "[{CLASSES_KEY}\\{id}\\shell\\open\\command]\r\n@=\"\\\"C:\\\\QP\\\\waveflow-quickplay.exe\\\" \\\"%1\\\"\"\r\n"
                )));
                assert!(s.contains(&format!(
                    "[{CLASSES_KEY}\\.{ext}\\OpenWithProgids]\r\n\"{id}\"=hex(0):\r\n"
                )));
                assert!(s.contains(&format!("\".{ext}\"=\"{id}\"\r\n")));
            }
            assert!(s.contains(&format!(
                "[{CLASSES_KEY}\\WaveFlowQuickPlay.flac]\r\n@=\"Fichier audio FLAC\"\r\n"
            )));
            assert!(s.contains(&format!(
                "[{CLASSES_KEY}\\WaveFlowQuickPlay.m3u8]\r\n@=\"Liste de lecture M3U8\"\r\n"
            )));
            assert!(s.contains(
                "[HKEY_CURRENT_USER\\Software\\RegisteredApplications]\r\n\"WaveFlow QuickPlay\"=\"Software\\\\Classes\\\\Applications\\\\waveflow-quickplay.exe\\\\Capabilities\"\r\n"
            ));
            assert!(s.contains(&format!("[{APP_KEY}\\Capabilities]\r\n")));
            assert!(s.contains("\"ApplicationIcon\"=\"C:\\\\QP\\\\waveflow-quickplay.exe,0\""));
            // The default handler stays the user's choice.
            assert!(!s.contains("UserChoice"));
            assert!(!s.contains(&format!("[{CLASSES_KEY}\\.flac]\r\n@=")));
        }

        /// Every type QuickPlay opens has a document icon of its own, and
        /// every icon embedded is a type's; ids are distinct and above the
        /// application icon's.
        #[test]
        fn icons_match_the_openable_types() {
            use crate::file_icons::{FILE_ICONS, FILE_ICON_ALIASES, GENERIC_FILE_ICON};
            for ext in crate::library::openable_extensions() {
                assert_ne!(icon_id(ext), GENERIC_FILE_ICON, "{ext} has no icon");
            }
            assert_eq!(icon_id("aif"), icon_id("aiff"));
            assert_eq!(icon_id("OGA"), icon_id("ogg"));
            assert_eq!(icon_id("xyz"), GENERIC_FILE_ICON);
            assert!(FILE_ICONS.iter().any(|(_, id)| *id == GENERIC_FILE_ICON));
            let mut ids: Vec<u16> = FILE_ICONS.iter().map(|(_, id)| *id).collect();
            ids.sort();
            ids.dedup();
            assert_eq!(ids.len(), FILE_ICONS.len());
            assert!(ids[0] > 1);
            for (alias, stem) in FILE_ICON_ALIASES {
                assert!(crate::library::openable_extensions().contains(alias));
                assert!(FILE_ICONS.iter().any(|(s, _)| s == stem));
            }
            for (stem, id) in FILE_ICONS {
                assert!(
                    *id == GENERIC_FILE_ICON
                        || crate::library::openable_extensions().contains(stem),
                    "{stem}.ico is embedded for a type QuickPlay does not open"
                );
                let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("assets/file-icons")
                    .join(format!("{stem}.ico"));
                assert!(path.is_file(), "{} is missing", path.display());
            }
        }

        /// Explorer orders the verbs by key name: "Play with…" must sort
        /// before "Add to … playlist".
        #[test]
        fn play_verb_sorts_before_the_playlist_verb() {
            assert!(PLAY_VERB.to_ascii_lowercase() < ENQUEUE_VERB.to_ascii_lowercase());
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;

    const DESKTOP_ID: &str = "app.waveflow.quickplay";
    const MIME_TYPES: &str = "audio/flac;audio/mpeg;audio/mp4;audio/x-m4a;audio/aac;audio/x-aac;audio/ogg;audio/vorbis;audio/x-vorbis+ogg;audio/opus;audio/x-opus+ogg;audio/wav;audio/x-wav;audio/vnd.wave;audio/aiff;audio/x-aiff;audio/x-dsf;audio/x-dff;audio/x-dsd;audio/x-mpegurl;audio/mpegurl;application/vnd.apple.mpegurl;";

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

    /// One argument of the Exec key, quoted: `"`, `` ` ``, `$` and `\`
    /// escaped inside the quotes, and `%` doubled, since a single one
    /// starts a field code. The whole value then goes through
    /// [`desktop_string`].
    fn exec_quote(s: &str) -> String {
        let mut out = String::from("\"");
        for c in s.chars() {
            match c {
                '"' | '`' | '$' | '\\' => {
                    out.push('\\');
                    out.push(c);
                }
                '%' => out.push_str("%%"),
                _ => out.push(c),
            }
        }
        out.push('"');
        out
    }

    /// A desktop-entry string value: backslashes and the characters that
    /// would end or break the line escaped. Exec and TryExec are string
    /// values, so a literal backslash in a path is `\\\\` in Exec (quoted,
    /// then escaped) and `\\` in TryExec.
    fn desktop_string(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                '\\' => out.push_str(r"\\"),
                '\n' => out.push_str(r"\n"),
                '\r' => out.push_str(r"\r"),
                '\t' => out.push_str(r"\t"),
                _ => out.push(c),
            }
        }
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

    fn desktop_entry(exe: &str) -> String {
        let play = desktop_string(&format!("{} %F", exec_quote(exe)));
        let enqueue_exec = desktop_string(&format!("{} --enqueue %F", exec_quote(exe)));
        let try_exec = desktop_string(exe);
        let comment = localised("Comment", |t| t.desktop_comment);
        let enqueue = localised("Name", |t| t.action_add_to_playlist);
        format!(
            "[Desktop Entry]\nType=Application\nName=WaveFlow QuickPlay\n{comment}Exec={play}\nTryExec={try_exec}\nIcon={DESKTOP_ID}\nTerminal=false\nCategories=AudioVideo;Audio;Player;\nMimeType={MIME_TYPES}\nStartupWMClass={DESKTOP_ID}\nActions=Enqueue;\n\n[Desktop Action Enqueue]\n{enqueue}Exec={enqueue_exec}\n"
        )
    }

    pub fn register(exe: &std::path::Path, _t: &Strings) -> io::Result<String> {
        // `exe_path` has made sure the path is Unicode.
        let entry = desktop_entry(&exe.to_string_lossy());
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
            match std::fs::remove_file(&p) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        refresh();
        Ok("removed the desktop entry and the icon.".into())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn exec_and_try_exec_carry_any_path_unchanged() {
            let entry = desktop_entry("/opt/a\\b%c$d \"e\"/qp");
            assert!(
                entry.contains("\nExec=\"/opt/a\\\\\\\\b%%c\\\\$d \\\\\"e\\\\\"/qp\" %F\n"),
                "{entry}"
            );
            assert!(
                entry.contains("\nTryExec=/opt/a\\\\b%c$d \"e\"/qp\n"),
                "{entry}"
            );
            assert!(entry
                .contains("\nExec=\"/opt/a\\\\\\\\b%%c\\\\$d \\\\\"e\\\\\"/qp\" --enqueue %F\n"));
            let entry = desktop_entry("/opt/new\nline/qp");
            assert!(
                entry.contains("\nExec=\"/opt/new\\nline/qp\" %F\n"),
                "{entry}"
            );
            assert!(!entry.contains("new\nline"));
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use super::*;

    pub fn is_registered_at(_exe: &std::path::Path) -> bool {
        false
    }

    #[cfg(target_os = "macos")]
    pub fn register(_exe: &std::path::Path, _t: &Strings) -> io::Result<String> {
        Ok("nothing to register on macOS: the app declares the audio formats itself, so Finder offers it under Open With once WaveFlow QuickPlay is in Applications.".into())
    }

    #[cfg(target_os = "macos")]
    pub fn unregister() -> io::Result<String> {
        Ok(
            "nothing to remove on macOS: delete the app from Applications and Finder forgets it."
                .into(),
        )
    }

    #[cfg(not(target_os = "macos"))]
    pub fn register(_exe: &std::path::Path, _t: &Strings) -> io::Result<String> {
        Err(io::Error::other(
            "file-manager integration is not available on this platform.",
        ))
    }

    #[cfg(not(target_os = "macos"))]
    pub fn unregister() -> io::Result<String> {
        Err(io::Error::other("nothing to remove on this platform."))
    }
}

#[cfg(test)]
mod tests {
    use super::{registered_path, strip_verbatim};
    use std::path::PathBuf;

    /// From an AppImage the entry names the image, not the executable on
    /// its temporary mount; otherwise the executable itself.
    #[test]
    fn an_appimage_registers_the_image_not_its_mount() {
        let exe = || PathBuf::from("/tmp/.mount_waveflXYZ/usr/bin/waveflow-quickplay");
        assert_eq!(
            registered_path(
                exe(),
                Some("/home/u/Apps/waveflow-quickplay.AppImage".into())
            )
            .unwrap(),
            PathBuf::from("/home/u/Apps/waveflow-quickplay.AppImage")
        );
        assert_eq!(
            registered_path("/usr/bin/waveflow-quickplay".into(), None).unwrap(),
            PathBuf::from("/usr/bin/waveflow-quickplay")
        );
    }

    /// A path the entry cannot hold as text is refused, the AppImage's too.
    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_unicode_is_refused() {
        use std::os::unix::ffi::OsStrExt;
        let image = PathBuf::from(std::ffi::OsStr::from_bytes(b"/home/u/\xff.AppImage"));
        assert!(registered_path("/usr/bin/waveflow-quickplay".into(), Some(image)).is_err());
    }

    /// `exe_path` itself takes the AppImage when the runtime's variables
    /// say this executable runs from its mount, and not otherwise (a
    /// development build started by an AppImage). The variables are read
    /// from the environment, which the tests share: each case runs this
    /// test again in a child process that alone has them.
    #[cfg(target_os = "linux")]
    #[test]
    fn exe_path_takes_the_appimage_this_runs_from() {
        const NAME: &str = "integration::tests::exe_path_takes_the_appimage_this_runs_from";
        const EXPECTED: &str = "QUICKPLAY_TEST_EXPECTED_EXE";
        if let Some(expected) = std::env::var_os(EXPECTED) {
            assert_eq!(super::exe_path().unwrap(), PathBuf::from(expected));
            return;
        }
        let exe = std::env::current_exe().unwrap().canonicalize().unwrap();
        // A folder made here, so it cannot hold this executable (the
        // temporary folder itself may, when the build lives under it).
        let elsewhere = std::env::temp_dir().join(format!("quickplay-test-{}", std::process::id()));
        std::fs::create_dir(&elsewhere).unwrap();
        let elsewhere = elsewhere.canonicalize().unwrap();
        // `appimage()` only accepts an image that is a file.
        let image = elsewhere.join("waveflow-quickplay.AppImage");
        std::fs::write(&image, b"").unwrap();
        let mount = exe.parent().unwrap();
        let outputs: Vec<_> = [(mount, &image), (elsewhere.as_path(), &exe)]
            .into_iter()
            .map(|(appdir, expected)| {
                std::process::Command::new(&exe)
                    .args(["--exact", NAME, "--test-threads=1"])
                    .env("APPDIR", appdir)
                    .env("APPIMAGE", &image)
                    .env(EXPECTED, expected)
                    .output()
            })
            .collect();
        // Removed before anything is asserted, so a failure leaves nothing.
        let _ = std::fs::remove_dir_all(&elsewhere);
        for out in outputs {
            let out = out.unwrap();
            let stdout = String::from_utf8_lossy(&out.stdout);
            // "1 passed": the child found this test, rather than none.
            assert!(
                out.status.success() && stdout.contains("1 passed"),
                "{stdout}{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }

    #[test]
    fn verbatim_prefixes_come_off_shares_included() {
        assert_eq!(strip_verbatim(r"\\?\C:\QP\qp.exe"), r"C:\QP\qp.exe");
        assert_eq!(
            strip_verbatim(r"\\?\UNC\server\share\qp.exe"),
            r"\\server\share\qp.exe"
        );
        assert_eq!(strip_verbatim("/usr/bin/qp"), "/usr/bin/qp");
    }
}
