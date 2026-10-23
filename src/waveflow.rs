//! Handing playback over to WaveFlow.
//!
//! When WaveFlow is installed, the player offers to carry on there: the
//! queue, the track, the position and whether it was playing go into a
//! [`Handoff`] file, `waveflow --handoff <file>` is started, and QuickPlay
//! stops and closes. The contract is `waveflow_core::handoff`, so both
//! programs build and read the same format; WaveFlow removes the file
//! after reading it, drops the paths that are gone and resumes paused
//! without a sound when the track was paused.
//!
//! Finding WaveFlow touches the registry on Windows and walks `PATH` on
//! Linux, so it runs once on a thread of its own ([`Finder`]) instead of
//! delaying the first frame. The Flatpak build cannot take a handoff (its
//! sandbox has its own `/tmp`, where our file is not), and it is never
//! found: its launcher is named after the application id, not `waveflow`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use waveflow_core::handoff::Handoff;

/// Points at a WaveFlow executable, for development builds and scripted
/// tests; it wins over the installed one when the file exists.
const OVERRIDE_ENV: &str = "QUICKPLAY_WAVEFLOW";

/// WaveFlow's executable, looked for once in the background.
#[derive(Clone)]
pub struct Finder {
    found: Arc<OnceLock<Option<PathBuf>>>,
}

impl Finder {
    /// Start looking; `notify` runs once the answer is known, so a button
    /// can appear without waiting for the next input.
    pub fn spawn(notify: impl FnOnce() + Send + 'static) -> Self {
        let found = Arc::new(OnceLock::new());
        let slot = Arc::clone(&found);
        let spawned = std::thread::Builder::new()
            .name("quickplay-find-waveflow".into())
            .spawn(move || {
                let exe = locate();
                tracing::debug!(?exe, "WaveFlow lookup");
                let _ = slot.set(exe);
                notify();
            });
        if spawned.is_err() {
            let _ = found.set(None);
        }
        Self { found }
    }

    /// The executable, once found; `None` while looking or when WaveFlow
    /// is not installed.
    pub fn exe(&self) -> Option<&Path> {
        self.found.get()?.as_deref()
    }
}

/// Write the handoff and start WaveFlow with it. The caller stops and
/// closes once this succeeds; on failure nothing was started and the
/// file, if written, is removed again.
pub fn hand_over(exe: &Path, handoff: &Handoff) -> std::io::Result<()> {
    let file = handoff.write()?;
    let started = std::process::Command::new(exe)
        .args(Handoff::args(&file))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    match started {
        // An already running WaveFlow gets the arguments through its
        // single-instance plugin, and the process started here exits on
        // its own; either way there is nothing to wait for.
        Ok(_) => Ok(()),
        Err(err) => {
            let _ = std::fs::remove_file(&file);
            Err(err)
        }
    }
}

fn locate() -> Option<PathBuf> {
    if let Some(exe) = std::env::var_os(OVERRIDE_ENV).map(PathBuf::from) {
        if exe.is_file() {
            return Some(exe);
        }
    }
    platform::locate()
}

#[cfg(windows)]
mod platform {
    use std::path::PathBuf;

    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER,
        HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ,
    };

    const EXE: &str = "waveflow.exe";
    const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";

    /// Where the installers put WaveFlow: the uninstall entry first, which
    /// follows a folder the user picked, then the default folders (the
    /// per-user NSIS and MSI installs both default to `%LOCALAPPDATA%`).
    pub fn locate() -> Option<PathBuf> {
        let registered = [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
            .into_iter()
            .filter_map(registered_dir);
        let defaults = [
            ("LOCALAPPDATA", r"WaveFlow"),
            ("LOCALAPPDATA", r"Programs\WaveFlow"),
            ("ProgramFiles", r"WaveFlow"),
            ("ProgramFiles(x86)", r"WaveFlow"),
        ]
        .into_iter()
        .filter_map(|(var, rest)| Some(PathBuf::from(std::env::var_os(var)?).join(rest)));
        registered
            .chain(defaults)
            .map(|dir| dir.join(EXE))
            .find(|exe| exe.is_file())
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// A string value of `subkey` under `root`.
    fn string_value(root: HKEY, subkey: &[u16], name: &str) -> Option<String> {
        let name = wide(name);
        let mut buf = [0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        // SAFETY: documented call; `subkey` and `name` are NUL-terminated,
        // `buf` and `size` describe a buffer we own, and `size` comes back
        // as the bytes written (NUL included).
        let status = unsafe {
            RegGetValueW(
                root,
                subkey.as_ptr(),
                name.as_ptr(),
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
        Some(String::from_utf16_lossy(&buf[..len]))
    }

    /// The install folder of the uninstall entry named "WaveFlow" under
    /// `hive`: its `InstallLocation`, else the folder of its `DisplayIcon`
    /// (the NSIS installer points that at the executable). The NSIS entry
    /// is keyed by name, the MSI one by product code, so the entries are
    /// walked and matched on their display name, exactly: QuickPlay's own
    /// entry is "WaveFlow QuickPlay".
    fn registered_dir(hive: HKEY) -> Option<PathBuf> {
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: documented call; the subkey is NUL-terminated and `key`
        // receives a handle closed below.
        if unsafe { RegOpenKeyExW(hive, wide(UNINSTALL).as_ptr(), 0, KEY_READ, &mut key) } != 0 {
            return None;
        }
        let mut found = None;
        let mut name = [0u16; 256];
        for index in 0.. {
            let mut len = name.len() as u32;
            // SAFETY: `key` is open; `name` and `len` describe a buffer we
            // own, in characters; the optional outputs are null.
            let status = unsafe {
                RegEnumKeyExW(
                    key,
                    index,
                    name.as_mut_ptr(),
                    &mut len,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if status != 0 {
                // ERROR_NO_MORE_ITEMS, or a name longer than any uninstall
                // key has: either way the walk is over.
                break;
            }
            let subkey: Vec<u16> = name[..len as usize]
                .iter()
                .copied()
                .chain(std::iter::once(0))
                .collect();
            if string_value(key, &subkey, "DisplayName").as_deref() != Some("WaveFlow") {
                continue;
            }
            let dir = string_value(key, &subkey, "InstallLocation")
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
                .or_else(|| {
                    let icon = string_value(key, &subkey, "DisplayIcon")?;
                    // `"C:\…\waveflow.exe",0` or a bare path.
                    let path = icon.split(',').next()?.trim().trim_matches('"');
                    Some(PathBuf::from(path).parent()?.to_path_buf())
                });
            if let Some(dir) = dir.filter(|d| d.join(EXE).is_file()) {
                found = Some(dir);
                break;
            }
        }
        // SAFETY: `key` was opened above and is not used afterwards.
        unsafe { RegCloseKey(key) };
        found
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::path::PathBuf;

    const IN_BUNDLE: &str = "WaveFlow.app/Contents/MacOS/waveflow";

    pub fn locate() -> Option<PathBuf> {
        let system = PathBuf::from("/Applications").join(IN_BUNDLE);
        let user = dirs::home_dir().map(|h| h.join("Applications").join(IN_BUNDLE));
        std::iter::once(system)
            .chain(user)
            .find(|exe| exe.is_file())
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use std::path::PathBuf;

    /// `waveflow` on `PATH`, where the .deb, .rpm, AUR and COPR packages
    /// put it. A Flatpak install exports `app.waveflow.WaveFlow` instead,
    /// so it is not found, which is what we want: it cannot read the
    /// handoff file.
    pub fn locate() -> Option<PathBuf> {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|dir| dir.join("waveflow"))
            .find(|exe| {
                std::fs::metadata(exe)
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_start_leaves_no_file_behind() {
        let before = handoff_files();
        let handoff = Handoff::new(vec![PathBuf::from("/music/a.flac")], 0, 1_500, true);
        let missing = std::env::temp_dir().join("quickplay-no-such-waveflow-binary");
        assert!(hand_over(&missing, &handoff).is_err());
        assert_eq!(handoff_files(), before);
    }

    fn handoff_files() -> Vec<PathBuf> {
        let pid = format!("waveflow-handoff-{}-", std::process::id());
        std::fs::read_dir(std::env::temp_dir())
            .map(|dir| {
                dir.filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| {
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.starts_with(&pid))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}
