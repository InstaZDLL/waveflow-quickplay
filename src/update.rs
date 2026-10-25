//! Updates. At each launch, unless it is switched off in the settings,
//! QuickPlay reads the manifest of the latest release (`latest.json`, next
//! to the packages on GitHub). A newer version is downloaded in the
//! background and checked against the release key (minisign), then
//! installed when QuickPlay closes: the next launch is the new version, and
//! nothing that plays is cut.
//!
//! Only the copies QuickPlay can replace by itself update this way: the
//! Windows installer's (per user), an app bundle in a folder this user can
//! write and an AppImage. A .deb, an .rpm or a build from source only says
//! that a new version is out: the package manager, or whoever built it,
//! owns that binary.

use std::collections::HashMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use serde::Deserialize;

const MANIFEST_URL: &str =
    "https://github.com/InstaZDLL/waveflow-quickplay/releases/latest/download/latest.json";
/// The release key: its secret half signs the packages in the release
/// workflow (`.github/workflows/release.yml`).
const PUBLIC_KEY: &str = "RWTCT9DA0wrP3Arx6fuGQUSRLNuMIetYOx4pCu9F+/S9PHb0fzeMHF/4";
/// A release's page, before its version.
pub const RELEASE_PAGE: &str = "https://github.com/InstaZDLL/waveflow-quickplay/releases/tag/v";
/// No package comes near this, nor a manifest near the other; a larger
/// download is not one of ours.
const MAX_PACKAGE: u64 = 256 << 20;
const MAX_MANIFEST: u64 = 1 << 20;

#[derive(Debug, Deserialize)]
struct Manifest {
    version: String,
    /// By [`Install::platform`].
    platforms: HashMap<String, Package>,
}

#[derive(Debug, Deserialize)]
struct Package {
    url: String,
    /// The minisign signature file, whose trusted comment names the version
    /// and the file (`waveflow-quickplay 1.2.0 <file>`), so a package cannot
    /// be passed off as another version.
    signature: String,
}

/// How this copy was installed, which decides what an update can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Install {
    /// The Windows installer's copy: the new installer runs silently.
    Windows,
    /// An app bundle in a folder this user can write: the new one takes
    /// its place.
    MacApp(PathBuf),
    /// An AppImage in a folder this user can write: the new file takes its
    /// place.
    AppImage(PathBuf),
    /// Anything else: say that a new version is out, change nothing.
    Elsewhere,
}

impl Install {
    pub fn detect() -> Self {
        let Ok(exe) = std::env::current_exe() else {
            return Install::Elsewhere;
        };
        if cfg!(windows) {
            // The installer leaves its uninstaller beside the executable.
            if exe.with_file_name("uninstall.exe").is_file() {
                return Install::Windows;
            }
        } else if cfg!(target_os = "macos") {
            // …/WaveFlow QuickPlay.app/Contents/MacOS/waveflow-quickplay
            let app = exe.ancestors().nth(3).filter(|a| {
                a.extension().is_some_and(|e| e == "app")
                    && exe.parent().and_then(Path::file_name) == Some("MacOS".as_ref())
            });
            // Not where it was put when the system runs it from a read-only
            // copy (a disk image, or App Translocation for a quarantined
            // app): the update could not land there.
            if let Some(app) = app.filter(|a| writable(a) && a.parent().is_some_and(writable)) {
                return Install::MacApp(app.to_path_buf());
            }
        } else if let Some(image) = appimage() {
            if image.parent().is_some_and(writable) {
                return Install::AppImage(image);
            }
        }
        Install::Elsewhere
    }

    /// The manifest's key for the package this copy takes.
    fn platform(&self) -> Option<&'static str> {
        match self {
            Install::Windows => Some("windows-x86_64"),
            Install::MacApp(_) => Some("darwin-universal"),
            Install::AppImage(_) => Some("linux-x86_64-appimage"),
            Install::Elsewhere => None,
        }
    }
}

/// The AppImage this process runs from: its runtime names it in
/// `APPIMAGE`, and mounts it at `APPDIR`.
pub fn appimage() -> Option<PathBuf> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let appdir = std::env::var_os("APPDIR").and_then(|d| std::fs::canonicalize(d).ok());
    appimage_of(&exe, appdir.as_deref(), std::env::var_os("APPIMAGE"))
        .filter(|image| image.is_file())
}

/// Both variables are inherited by whatever an AppImage starts, a
/// QuickPlay installed otherwise included: only an executable inside the
/// mount is that AppImage's.
fn appimage_of(
    exe: &Path,
    appdir: Option<&Path>,
    image: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    exe.starts_with(appdir?).then(|| image.map(PathBuf::from))?
}

#[cfg(unix)]
fn writable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: a valid NUL-terminated path; access has no other
    // preconditions.
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

#[cfg(not(unix))]
fn writable(_: &Path) -> bool {
    true
}

/// What a check found.
pub enum Found {
    /// Downloaded and verified: installed when QuickPlay closes.
    Ready(Update),
    /// A newer version this copy cannot install by itself.
    Available(String),
}

/// A verified package, waiting for QuickPlay to close.
#[derive(Debug)]
pub struct Update {
    pub version: String,
    file: PathBuf,
    install: Install,
}

/// Whether to look for updates at all: release builds do; a debug build
/// only when `QUICKPLAY_UPDATE_URL` points it at a manifest to test with.
pub fn wanted() -> bool {
    !cfg!(debug_assertions) || std::env::var_os("QUICKPLAY_UPDATE_URL").is_some()
}

/// Checks on a thread of its own; `notify` wakes the UI when it is done.
/// The receiver gets something only when there is a newer version.
pub fn spawn_check(notify: impl Fn() + Send + 'static) -> mpsc::Receiver<Found> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        match check(&Install::detect()) {
            Ok(Some(found)) => {
                let _ = tx.send(found);
            }
            Ok(None) => tracing::info!("QuickPlay is up to date"),
            Err(e) => tracing::warn!(error = %e, "update check failed"),
        }
        notify();
    });
    rx
}

fn check(install: &Install) -> io::Result<Option<Found>> {
    let url = std::env::var("QUICKPLAY_UPDATE_URL").unwrap_or_else(|_| MANIFEST_URL.into());
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("waveflow-quickplay/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(io::Error::other)?;
    let manifest: Manifest =
        serde_json::from_slice(&download(&client, &url, MAX_MANIFEST)?).map_err(invalid)?;
    let latest = semver::Version::parse(&manifest.version).map_err(invalid)?;
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION")).map_err(invalid)?;
    if latest <= current {
        // The installer of the update now running, which Windows could not
        // delete while it ran, goes.
        if let Some(dir) = cache_dir() {
            clear_cache(&dir, None);
        }
        return Ok(None);
    }
    let version = manifest.version;
    // A release without this copy's package (or a copy that takes none)
    // only says that it is out.
    let Some(package) = install.platform().and_then(|p| manifest.platforms.get(p)) else {
        tracing::info!(%version, ?install, "a new version is available");
        return Ok(Some(Found::Available(version)));
    };
    let name = file_name(&package.url)
        .ok_or_else(|| invalid(format!("no file name in {}", package.url)))?;
    let dir = cache_dir().ok_or_else(|| io::Error::other("no cache directory"))?;
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(name);
    clear_cache(&dir, Some(&file));
    // Already fetched by an earlier launch that closed before installing it.
    let cached = std::fs::read(&file)
        .ok()
        .filter(|bytes| verify(PUBLIC_KEY, bytes, &package.signature, &version, name).is_ok());
    if cached.is_none() {
        tracing::info!(%version, url = %package.url, "downloading the update");
        let bytes = download(&client, &package.url, MAX_PACKAGE)?;
        verify(PUBLIC_KEY, &bytes, &package.signature, &version, name)?;
        let partial = file.with_extension("part");
        std::fs::write(&partial, &bytes)?;
        std::fs::rename(&partial, &file)?;
    }
    tracing::info!(%version, file = %file.display(), "update ready, installed on close");
    Ok(Some(Found::Ready(Update {
        version,
        file,
        install: install.clone(),
    })))
}

/// The body at `url`, refused past `max` bytes.
fn download(client: &reqwest::blocking::Client, url: &str, max: u64) -> io::Result<Vec<u8>> {
    let response = client
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(io::Error::other)?;
    let mut bytes = Vec::new();
    response.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(invalid(format!("{url} is larger than {max} bytes")));
    }
    Ok(bytes)
}

/// `bytes` are what the release key signed for `version`, under `name`.
fn verify(key: &str, bytes: &[u8], signature: &str, version: &str, name: &str) -> io::Result<()> {
    let key = minisign_verify::PublicKey::from_base64(key).map_err(invalid)?;
    let signature = minisign_verify::Signature::decode(signature).map_err(invalid)?;
    key.verify(bytes, &signature, false).map_err(invalid)?;
    let expected = format!("waveflow-quickplay {version} {name}");
    if signature.trusted_comment() != expected {
        return Err(invalid(format!(
            "signed as \"{}\", not \"{expected}\"",
            signature.trusted_comment()
        )));
    }
    Ok(())
}

/// The last segment of a package URL, if it is a plain file name.
fn file_name(url: &str) -> Option<&str> {
    let name = url.rsplit('/').next()?;
    let plain = !name.is_empty()
        && name != ".."
        && name != "."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-+".contains(c));
    plain.then_some(name)
}

fn cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("waveflow-quickplay").join("updates"))
}

/// Packages of versions passed over or installed, and partial downloads,
/// go; `keep` stays.
fn clear_cache(dir: &Path, keep: Option<&Path>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if Some(path.as_path()) != keep && path.is_file() {
            let _ = std::fs::remove_file(&path);
        }
    }
}

fn invalid(e: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}

/// Installs `update` as this process closes.
pub fn install(update: Update) -> io::Result<()> {
    tracing::info!(version = %update.version, install = ?update.install, "installing the update");
    match &update.install {
        // The installer waits for nothing: it ends a QuickPlay still
        // running (this one, closing) before it copies. `/S` silent, `/NS`
        // no new shortcuts beyond the ones the user kept.
        Install::Windows => std::process::Command::new(&update.file)
            .args(["/S", "/NS"])
            .stdin(std::process::Stdio::null())
            .spawn()
            .map(drop),
        Install::MacApp(app) => replace_app(&update.file, app),
        Install::AppImage(image) => replace_appimage(&update.file, image),
        Install::Elsewhere => Ok(()),
    }
}

/// The bundle is in use until this process exits: a shell waits for it,
/// then copies the new bundle out of the disk image beside the old one and
/// swaps them, the old one coming back if the swap fails. The image leaves
/// the cache at once, so a launch while the shell waits cannot clear it,
/// and everything the shell makes is in folders of its own.
fn replace_app(dmg: &Path, app: &Path) -> io::Result<()> {
    const SCRIPT: &str = r#"
pid=$1 dmg=$2 app=$3
work=$(mktemp -d "${TMPDIR:-/tmp}/quickplay-update.XXXXXX") || exit 1
mv "$dmg" "$work/update.dmg" || { rm -rf "$work"; exit 1; }
while kill -0 "$pid" 2>/dev/null; do sleep 0.2; done
mkdir "$work/mnt"
hdiutil attach -nobrowse -noautoopen -readonly -mountpoint "$work/mnt" "$work/update.dmg" || { rm -rf "$work"; exit 1; }
new=$(find "$work/mnt" -maxdepth 1 -name '*.app' | head -n 1)
# Beside the bundle, on its volume, so the swap is two renames.
copied=0
if stage=$(mktemp -d "$(dirname "$app")/.quickplay-update.XXXXXX"); then
  [ -n "$new" ] && ditto "$new" "$stage/new.app" && copied=1
fi
hdiutil detach -quiet "$work/mnt" && rm -rf "$work"
[ "$copied" = 1 ] || { [ -n "$stage" ] && rm -rf "$stage"; exit 1; }
mv "$app" "$stage/old.app" || { rm -rf "$stage"; exit 1; }
if mv "$stage/new.app" "$app"; then rm -rf "$stage"; else mv "$stage/old.app" "$app"; exit 1; fi
echo "updated $app"
"#;
    let log = dmg.with_extension("log");
    let out = std::fs::File::create(&log)?;
    std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(SCRIPT)
        .arg("sh")
        .arg(std::process::id().to_string())
        .arg(dmg)
        .arg(app)
        .stdin(std::process::Stdio::null())
        .stdout(out.try_clone()?)
        .stderr(out)
        .spawn()
        .map(drop)
}

/// An AppImage can be replaced while it runs: the running one keeps the
/// file it opened. The new one is copied beside it, into a file created
/// under a name nobody can guess (the folder may be shared, such as
/// `/tmp`), then renamed over it.
fn replace_appimage(new: &Path, image: &Path) -> io::Result<()> {
    use std::hash::{BuildHasher, Hasher};
    let nonce = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    let stage = image.with_file_name(format!(
        ".waveflow-quickplay-update-{}-{nonce:016x}",
        std::process::id()
    ));
    let copy = || -> io::Result<()> {
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage)?;
        io::copy(&mut std::fs::File::open(new)?, &mut out)?;
        set_executable(&out)?;
        out.sync_all()?;
        drop(out);
        std::fs::rename(&stage, image)
    };
    let result = copy();
    if result.is_err() {
        let _ = std::fs::remove_file(&stage);
    } else {
        let _ = std::fs::remove_file(new);
    }
    result
}

#[cfg(unix)]
fn set_executable(file: &std::fs::File) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn set_executable(_: &std::fs::File) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A key made for these tests only, and a package it signed.
    const TEST_KEY: &str = "RWS/JtcG7p98v8SA0ESo8oJLfmIYwJX8A0Y5aZBI02+xgrr9H7Bq7M4y";
    const TEST_PACKAGE: &[u8] = b"QuickPlay update test package\n";
    const TEST_SIGNATURE: &str = "untrusted comment: signature from rsign secret key
RUS/JtcG7p98v3ApB6CayGc76gDlwk7lg2HRj40GxYbzERFJlALyNG+Pg9x0j7nMuzXx5CuOvJybdc5HZaqzFzArHmSyhrLHvAQ=
trusted comment: waveflow-quickplay 1.2.3 test.bin
2uxf9Jw1xvGmti8wC/Qq7BSjyyJO6Dc/8aPrzDjEz7Nd4wvstABBs3ef6IDagkp59iJoM35EXg8/8UmUeCN7BA==
";

    #[test]
    fn a_signed_package_passes() {
        verify(TEST_KEY, TEST_PACKAGE, TEST_SIGNATURE, "1.2.3", "test.bin").unwrap();
    }

    #[test]
    fn a_changed_package_fails() {
        let mut changed = TEST_PACKAGE.to_vec();
        changed[0] ^= 1;
        assert!(verify(TEST_KEY, &changed, TEST_SIGNATURE, "1.2.3", "test.bin").is_err());
    }

    #[test]
    fn another_key_fails() {
        assert!(verify(
            PUBLIC_KEY,
            TEST_PACKAGE,
            TEST_SIGNATURE,
            "1.2.3",
            "test.bin"
        )
        .is_err());
    }

    /// The manifest is not signed: a package claimed for another version
    /// or under another name is refused.
    #[test]
    fn the_signature_names_the_version_and_the_file() {
        assert!(verify(TEST_KEY, TEST_PACKAGE, TEST_SIGNATURE, "9.0.0", "test.bin").is_err());
        assert!(verify(TEST_KEY, TEST_PACKAGE, TEST_SIGNATURE, "1.2.3", "other.bin").is_err());
    }

    #[test]
    fn an_appimage_is_the_one_whose_mount_runs_this() {
        let image = || Some("/home/u/Apps/waveflow-quickplay.AppImage".into());
        let mount = Path::new("/tmp/.mount_waveflXYZ");
        assert_eq!(
            appimage_of(
                Path::new("/tmp/.mount_waveflXYZ/usr/bin/waveflow-quickplay"),
                Some(mount),
                image()
            ),
            Some(PathBuf::from("/home/u/Apps/waveflow-quickplay.AppImage"))
        );
        // Started by another AppImage, which left its variables behind.
        assert_eq!(
            appimage_of(
                Path::new("/usr/bin/waveflow-quickplay"),
                Some(mount),
                image()
            ),
            None
        );
        assert_eq!(
            appimage_of(Path::new("/usr/bin/waveflow-quickplay"), None, image()),
            None
        );
        assert_eq!(
            appimage_of(
                Path::new("/tmp/.mount_waveflXYZ/usr/bin/waveflow-quickplay"),
                Some(mount),
                None
            ),
            None
        );
    }

    #[test]
    fn the_release_key_reads() {
        minisign_verify::PublicKey::from_base64(PUBLIC_KEY).unwrap();
    }

    #[test]
    fn file_names_are_plain() {
        let base = "https://github.com/InstaZDLL/waveflow-quickplay/releases/download/v1.2.0/";
        assert_eq!(
            file_name(&format!("{base}waveflow-quickplay_1.2.0_x64-setup.exe")),
            Some("waveflow-quickplay_1.2.0_x64-setup.exe")
        );
        assert_eq!(file_name(&format!("{base}..")), None);
        assert_eq!(file_name(base), None);
        assert_eq!(file_name("https://example.org/a%2F..%2Fb"), None);
    }

    #[test]
    fn the_manifest_reads() {
        let json = r#"{
            "version": "1.2.0",
            "pub_date": "2026-10-26T10:00:00Z",
            "platforms": {
                "windows-x86_64": { "url": "https://example.org/a.exe", "signature": "s" }
            }
        }"#;
        let manifest: Manifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.version, "1.2.0");
        assert_eq!(
            manifest.platforms["windows-x86_64"].url,
            "https://example.org/a.exe"
        );
    }
}
