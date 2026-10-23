//! Handing playback over to WaveFlow.
//!
//! When a WaveFlow that takes handoffs is installed, the player offers to
//! carry on there: the queue, the track, the position and whether it was
//! playing go into a [`Handoff`] file, `waveflow --handoff <file>` is
//! started, and QuickPlay stops and closes. The contract is
//! `waveflow_core::handoff`, so both programs build and read the same
//! format; WaveFlow removes the file after reading it, drops the paths that
//! are gone and resumes paused without a sound when the track was paused.
//!
//! An older WaveFlow ignores `--handoff` and opens empty, so the button is
//! offered only for a version known to take it ([`MIN_WAVEFLOW_HANDOFF`]),
//! read from the install's metadata: the uninstall entry or the version
//! resource on Windows, `Info.plist` on macOS, the package manager on
//! Linux. WaveFlow itself is never started to ask: it does not know a
//! `--version` option and would open its window. An install whose version
//! cannot be read is not offered.
//!
//! The Flatpak build cannot take a handoff either (its sandbox has its own
//! `/tmp`, where our file is not), and it is never found: its launcher is
//! named after the application id, not `waveflow`.
//!
//! Finding WaveFlow touches the registry, the disk or a package manager, so
//! it runs once on a thread of its own ([`Finder`]) instead of delaying the
//! first frame.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use waveflow_core::handoff::Handoff;

/// The first WaveFlow version that takes a handoff (WaveFlow #942, in the
/// 2.0 line; no 1.9.x has it). Pre-releases of a version come before it,
/// as in semver, so the 2.0.0 betas qualify.
const MIN_WAVEFLOW_HANDOFF: &str = "2.0.0-beta.1";

/// Points at a WaveFlow executable, for development builds and scripted
/// tests. It wins over the installed one when the file exists, and its
/// version is not checked: a development build still reports the version
/// of the last release cut.
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
                let _ = slot.set(locate());
                notify();
            });
        if spawned.is_err() {
            let _ = found.set(None);
        }
        Self { found }
    }

    /// The executable, once found; `None` while looking, when WaveFlow is
    /// not installed, or when the installed one cannot take a handoff.
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

/// An installed WaveFlow and the version its metadata gives, if any.
struct Install {
    exe: PathBuf,
    version: Option<String>,
}

fn locate() -> Option<PathBuf> {
    if let Some(exe) = std::env::var_os(OVERRIDE_ENV).map(PathBuf::from) {
        if exe.is_file() {
            tracing::debug!(exe = %exe.display(), "WaveFlow from {OVERRIDE_ENV}");
            return Some(exe);
        }
    }
    let install = platform::locate()?;
    let takes_handoff = install.version.as_deref().is_some_and(takes_handoff);
    tracing::debug!(
        exe = %install.exe.display(),
        version = ?install.version,
        takes_handoff,
        "WaveFlow lookup"
    );
    takes_handoff.then_some(install.exe)
}

/// Whether an installed version is [`MIN_WAVEFLOW_HANDOFF`] or later.
fn takes_handoff(version: &str) -> bool {
    match (
        Version::parse(version),
        Version::parse(MIN_WAVEFLOW_HANDOFF),
    ) {
        (Some(v), Some(min)) => v >= min,
        _ => false,
    }
}

/// A version as the installers and package managers spell it, compared
/// with semver's precedence. Lenient on the spelling: a fourth number
/// (Windows' `2.0.0.0`) is dropped, and the pre-release may follow a `-`,
/// a `~` (Debian, RPM), a `_` or nothing at all (AUR `pkgver`).
#[derive(Debug, PartialEq, Eq)]
struct Version {
    core: [u64; 3],
    /// Empty for a release.
    pre: Vec<Identifier>,
}

/// One dot-separated part of a pre-release. Numbers sort before words, as
/// in semver (the variant order gives that).
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Identifier {
    Number(u64),
    Word(String),
}

impl Version {
    fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let text = text.strip_prefix(['v', 'V']).unwrap_or(text);
        let text = text.split('+').next().unwrap_or(text);
        let split = text
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(text.len());
        let (numbers, rest) = text.split_at(split);
        let mut parts = numbers.split('.').filter(|p| !p.is_empty());
        let mut core = [0u64; 3];
        core[0] = parts.next()?.parse().ok()?;
        for slot in &mut core[1..] {
            match parts.next() {
                Some(p) => *slot = p.parse().ok()?,
                None => break,
            }
        }
        let rest = rest.trim_start_matches(['-', '~', '_', '.']);
        let pre = if rest.is_empty() {
            Vec::new()
        } else {
            rest.split(['.', '-'])
                .filter(|p| !p.is_empty())
                .map(|p| match p.parse() {
                    Ok(n) => Identifier::Number(n),
                    Err(_) => Identifier::Word(p.to_ascii_lowercase()),
                })
                .collect()
        };
        Some(Self { core, pre })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core.cmp(&other.core).then_with(|| {
            match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                // A release comes after its pre-releases.
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                // Part by part, and a longer list after its own prefix.
                (false, false) => self.pre.cmp(&other.pre),
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(windows)]
mod platform {
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
    };
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER,
        HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ,
    };

    use super::Install;

    const EXE: &str = "waveflow.exe";
    const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";

    /// Where the installers put WaveFlow: the uninstall entry first, which
    /// follows a folder the user picked and carries the version
    /// (`DisplayVersion`, written by both the NSIS and the MSI installer),
    /// then the default folders (the per-user NSIS and MSI installs both
    /// default to `%LOCALAPPDATA%`), whose version comes from the
    /// executable's version resource.
    pub fn locate() -> Option<Install> {
        let registered = [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
            .into_iter()
            .find_map(registered);
        if let Some((dir, version)) = registered {
            let exe = dir.join(EXE);
            let version = version.or_else(|| file_version(&exe));
            return Some(Install { exe, version });
        }
        [
            ("LOCALAPPDATA", r"WaveFlow"),
            ("LOCALAPPDATA", r"Programs\WaveFlow"),
            ("ProgramFiles", r"WaveFlow"),
            ("ProgramFiles(x86)", r"WaveFlow"),
        ]
        .into_iter()
        .filter_map(|(var, rest)| Some(PathBuf::from(std::env::var_os(var)?).join(rest)))
        .map(|dir| dir.join(EXE))
        .find(|exe| exe.is_file())
        .map(|exe| {
            let version = file_version(&exe);
            Install { exe, version }
        })
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

    /// The install folder and the version of the uninstall entry named
    /// "WaveFlow" under `hive`. The folder is its `InstallLocation`, else
    /// that of its `DisplayIcon` (the NSIS installer points that at the
    /// executable). The NSIS entry is keyed by name, the MSI one by product
    /// code, so the entries are walked and matched on their display name,
    /// exactly: QuickPlay's own entry is "WaveFlow QuickPlay".
    fn registered(hive: HKEY) -> Option<(PathBuf, Option<String>)> {
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
                let version =
                    string_value(key, &subkey, "DisplayVersion").filter(|v| !v.trim().is_empty());
                found = Some((dir, version));
                break;
            }
        }
        // SAFETY: `key` was opened above and is not used afterwards.
        unsafe { RegCloseKey(key) };
        found
    }

    /// The product version of `exe`'s version resource, which Tauri fills
    /// with the application's version, as `major.minor.patch.build`.
    fn file_version(exe: &Path) -> Option<String> {
        let path: Vec<u16> = exe
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: documented call; `path` is NUL-terminated.
        let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), std::ptr::null_mut()) };
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        // SAFETY: `data` is `size` bytes we own.
        if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, data.as_mut_ptr().cast()) } == 0 {
            return None;
        }
        let mut info: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let root = wide("\\");
        // SAFETY: `data` holds the block just read; `info` comes back
        // pointing inside it, valid while `data` lives.
        let ok =
            unsafe { VerQueryValueW(data.as_ptr().cast(), root.as_ptr(), &mut info, &mut len) };
        if ok == 0 || info.is_null() || (len as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>() {
            return None;
        }
        // SAFETY: checked non-null and at least the struct's size above;
        // read unaligned, the block makes no alignment promise.
        let fixed = unsafe { std::ptr::read_unaligned(info.cast::<VS_FIXEDFILEINFO>()) };
        let (ms, ls) = (fixed.dwProductVersionMS, fixed.dwProductVersionLS);
        Some(format!(
            "{}.{}.{}.{}",
            ms >> 16,
            ms & 0xffff,
            ls >> 16,
            ls & 0xffff
        ))
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::path::{Path, PathBuf};

    use super::Install;

    const BUNDLE: &str = "WaveFlow.app";

    /// `WaveFlow.app` in `/Applications` or `~/Applications`, with the
    /// version from its `Info.plist`.
    pub fn locate() -> Option<Install> {
        let user = dirs::home_dir().map(|h| h.join("Applications").join(BUNDLE));
        std::iter::once(PathBuf::from("/Applications").join(BUNDLE))
            .chain(user)
            .map(|app| (app.join("Contents/MacOS/waveflow"), app))
            .find(|(exe, _)| exe.is_file())
            .map(|(exe, app)| Install {
                exe,
                version: bundle_version(&app),
            })
    }

    /// `CFBundleShortVersionString` from the XML `Info.plist` Tauri writes.
    fn bundle_version(app: &Path) -> Option<String> {
        let plist = std::fs::read_to_string(app.join("Contents/Info.plist")).ok()?;
        let after = plist
            .split("<key>CFBundleShortVersionString</key>")
            .nth(1)?;
        let value = after.trim_start().strip_prefix("<string>")?;
        Some(value.split("</string>").next()?.trim().to_string())
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use std::process::{Command, Stdio};

    use super::Install;

    /// `waveflow` on `PATH`, where the .deb, .rpm, AUR and COPR packages
    /// put it. A Flatpak install exports `app.waveflow.WaveFlow` instead,
    /// so it is not found, which is what we want: it cannot read the
    /// handoff file. A binary on `PATH` carries no readable version, so it
    /// comes from the package manager that installed it; a binary none of
    /// them knows (an AppImage put there by hand) has none.
    pub fn locate() -> Option<Install> {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::var_os("PATH")?;
        let exe = std::env::split_paths(&path)
            .map(|dir| dir.join("waveflow"))
            .find(|exe| {
                std::fs::metadata(exe)
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            })?;
        Some(Install {
            exe,
            version: package_version(),
        })
    }

    fn package_version() -> Option<String> {
        // Debian and apt: the package is `wave-flow`. A Debian revision
        // (`2.0.0-1`) would otherwise read as a pre-release.
        run("dpkg-query", &["-W", "-f=${Version}", "wave-flow"])
            .map(|v| super::strip_package_release(&v).to_string())
            // Fedora and COPR: `%{VERSION}` carries no release.
            .or_else(|| run("rpm", &["-q", "--qf", "%{VERSION}", "waveflow"]))
            // AUR: `waveflow-bin 2.0.0-1`, the version then the package
            // release.
            .or_else(|| {
                let line = run("pacman", &["-Q", "waveflow-bin"])?;
                Some(super::strip_package_release(line.split_whitespace().nth(1)?).to_string())
            })
    }

    /// The trimmed output of a command that succeeded with some.
    fn run(program: &str, args: &[&str]) -> Option<String> {
        let out = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        let text = String::from_utf8(out.stdout).ok()?;
        let text = text.trim();
        (out.status.success() && !text.is_empty()).then(|| text.to_string())
    }
}

/// `2.0.0-1` → `2.0.0`: dpkg and pacman append the package revision after
/// the last dash. A revision starts with a digit (`1`, `1+b1`,
/// `0ubuntu1`), a pre-release label with a letter (`beta.1`), so
/// `2.0.0-beta.1` is left whole and `2.0.0-alpha.1` does not become a
/// release.
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn strip_package_release(version: &str) -> &str {
    match version.rsplit_once('-') {
        Some((head, release)) if release.starts_with(|c: char| c.is_ascii_digit()) => head,
        _ => version,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_handoff_starts_with_the_two_zero_betas() {
        for old in ["1.9.7", "1.9.1", "2.0.0-alpha.3", "2.0.0-beta.0", "1.99.0"] {
            assert!(!takes_handoff(old), "{old}");
        }
        for new in [
            "2.0.0-beta.1",
            "2.0.0-beta.2",
            "2.0.0-beta.10",
            "2.0.0-rc.1",
            "2.0.0",
            "2.0.1",
            "2.1.0-beta.1",
            "3.0.0",
        ] {
            assert!(takes_handoff(new), "{new}");
        }
    }

    #[test]
    fn installer_and_package_spellings_are_understood() {
        // Windows version resource, Debian / RPM tilde, AUR pkgver.
        assert!(takes_handoff("2.0.0.0"));
        assert!(!takes_handoff("1.9.7.0"));
        assert!(takes_handoff("2.0.0~beta.1"));
        assert!(!takes_handoff("2.0.0~alpha.1"));
        assert!(takes_handoff("2.0.0beta.1"));
        assert!(takes_handoff("v2.0.0"));
        assert!(takes_handoff("2.0.0-beta.1+build.5"));
        assert_eq!(strip_package_release("2.0.0-1"), "2.0.0");
        assert_eq!(strip_package_release("2.0.0beta.1-2"), "2.0.0beta.1");
        assert_eq!(strip_package_release("1.9.7"), "1.9.7");
        // Debian revisions: a stable release with one must still qualify,
        // and a pre-release keeps its own label.
        assert!(!takes_handoff("2.0.0-1"));
        assert!(takes_handoff(strip_package_release("2.0.0-1")));
        assert!(takes_handoff(strip_package_release("2.0.0-beta.1-1")));
        assert!(takes_handoff(strip_package_release("2.0.0-beta.1")));
        assert!(!takes_handoff(strip_package_release("1.9.7-1")));
        // Revisions with letters after their first digit: binNMU, Ubuntu.
        assert_eq!(strip_package_release("2.0.0-1+b1"), "2.0.0");
        assert_eq!(strip_package_release("2.0.0-0ubuntu1"), "2.0.0");
        assert!(takes_handoff(strip_package_release("2.0.0-1+b1")));
        assert!(!takes_handoff(strip_package_release("1.9.7-1+b1")));
        // A label is not a revision: an alpha stays an alpha.
        assert_eq!(strip_package_release("2.0.0-alpha.1"), "2.0.0-alpha.1");
        assert!(!takes_handoff(strip_package_release("2.0.0-alpha.1")));
    }

    #[test]
    fn an_unreadable_version_is_not_offered() {
        for junk in ["", "  ", "beta", "unknown", "x.y.z"] {
            assert!(!takes_handoff(junk), "{junk:?}");
        }
    }

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
