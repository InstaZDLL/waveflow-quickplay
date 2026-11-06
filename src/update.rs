//! Updates. At each launch, unless it is switched off in the settings,
//! QuickPlay reads the manifest of the latest release (`latest.json`) from
//! two hosts: GitHub, next to the packages, and its copy on WaveFlow's R2
//! bucket, which keeps working if the GitHub account does not. The newest
//! version either offers is downloaded in the background and checked
//! against the release key (minisign), then installed when QuickPlay
//! closes: the next launch is the new version, and nothing that plays is
//! cut.
//!
//! The manifest is not trusted for the version: each package's signature
//! names the version and the file it was signed for, so neither host, nor
//! anyone answering in its place, can pass an older package off as a newer
//! version. A version no package backs is passed over for the next newest,
//! so a host cannot hold updates back by announcing one that does not
//! exist.
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

/// Where the manifest is published, asked in this order: the GitHub
/// release, then its copy on R2 (`scripts/release/publish-r2.sh`), whose
/// packages sit under `quickplay/releases/<tag>/` with the same names and
/// signatures. A host added here reaches only the versions built after it.
const MANIFEST_URLS: [&str; 2] = [
    "https://github.com/InstaZDLL/waveflow-quickplay/releases/latest/download/latest.json",
    "https://download.waveflow.app/quickplay/latest.json",
];
/// A host that accepts the connection and never answers must not keep the
/// next one from being asked.
const MANIFEST_TIMEOUT: Duration = Duration::from_secs(30);
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
    /// How this copy was installed, read from where its executable is.
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

/// Whether this user may write `path`, by the kernel's own check.
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

/// Taken as writable elsewhere: a write that fails says so when it happens.
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

/// Looks for a newer version on the update hosts and, when this copy can
/// install it, downloads and verifies it.
fn check(install: &Install) -> io::Result<Option<Found>> {
    // A test manifest replaces both hosts, in any build.
    let urls = match std::env::var("QUICKPLAY_UPDATE_URL") {
        Ok(url) => vec![url],
        Err(_) => MANIFEST_URLS.iter().map(|u| u.to_string()).collect(),
    };
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION")).map_err(invalid)?;
    check_at(&urls, PUBLIC_KEY, &current, install, cache_dir())
}

/// [`check`] against `urls`, for packages signed with `key`, from a copy at
/// version `current` that caches in `dir`.
fn check_at(
    urls: &[String],
    key: &str,
    current: &semver::Version,
    install: &Install,
    dir: Option<PathBuf>,
) -> io::Result<Option<Found>> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("waveflow-quickplay/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(io::Error::other)?;
    // One host at a time, each on its own: a host that fails, whatever the
    // way (an error page, a suspended account's page, a stale manifest),
    // hides nothing the other one offers. Only all of them failing is an
    // error.
    let mut manifests = Vec::new();
    let mut last_error = None;
    for url in urls {
        match read_manifest(&client, url) {
            Ok(manifest) => manifests.push(manifest),
            Err(e) => {
                tracing::warn!(%url, error = %e, "could not read the update manifest");
                last_error = Some(e);
            }
        }
    }
    if manifests.is_empty() {
        return Err(last_error.unwrap_or_else(|| io::Error::other("no update host")));
    }
    let offers = newer(manifests, current);
    let Some((newest, _)) = offers.first() else {
        // The installer of the update now running, which Windows could not
        // delete while it ran, goes.
        if let Some(dir) = &dir {
            clear_cache(dir, &[]);
        }
        return Ok(None);
    };
    // The packages of this copy's platform: newest version first, and for
    // one version in the order of the hosts. A release without one (or a
    // copy that takes none) only says that it is out.
    let candidates: Vec<(&str, &Package)> = match install.platform() {
        Some(p) => offers
            .iter()
            .flat_map(|(v, sources)| {
                sources
                    .iter()
                    .filter_map(move |m| Some((v.as_str(), m.platforms.get(p)?)))
            })
            .collect(),
        None => Vec::new(),
    };
    if candidates.is_empty() {
        tracing::info!(version = %newest, ?install, "a new version is available");
        return Ok(Some(Found::Available(newest.clone())));
    }
    let dir = dir.ok_or_else(|| io::Error::other("no cache directory"))?;
    std::fs::create_dir_all(&dir)?;
    let files: Vec<PathBuf> = candidates
        .iter()
        .filter_map(|(_, p)| file_name(&p.url))
        .map(|name| dir.join(name))
        .collect();
    clear_cache(&dir, &files);
    // The first package that downloads and verifies wins. A host that
    // answers the small manifest but not the package leaves it to the other
    // host. The version a manifest claims is not signed, only each package
    // is: a version that no package backs is passed over for the next one,
    // so a host announcing a version that does not exist cannot hold back
    // a real one. Nothing older than this copy is ever offered.
    let mut last_error = None;
    for (version, package) in candidates {
        match fetch_package(&client, key, &dir, package, version) {
            Ok(file) => {
                tracing::info!(%version, file = %file.display(), "update ready, installed on close");
                return Ok(Some(Found::Ready(Update {
                    version: version.to_string(),
                    file,
                    install: install.clone(),
                })));
            }
            Err(e) => {
                tracing::warn!(%version, url = %package.url, error = %e, "could not fetch the update");
                last_error = Some(e);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| io::Error::other("no package")))
}

/// The manifest at `url`, if it reads as one, with a version that parses.
fn read_manifest(client: &reqwest::blocking::Client, url: &str) -> io::Result<Manifest> {
    let response = client
        .get(url)
        .timeout(MANIFEST_TIMEOUT)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(io::Error::other)?;
    let manifest: Manifest =
        serde_json::from_slice(&read_at_most(response, MAX_MANIFEST, url)?).map_err(invalid)?;
    semver::Version::parse(&manifest.version).map_err(invalid)?;
    Ok(manifest)
}

/// The versions the manifests offer that are newer than `current`, newest
/// first, each with every manifest that offers it, in the order they came.
fn newer(manifests: Vec<Manifest>, current: &semver::Version) -> Vec<(String, Vec<Manifest>)> {
    let mut offers: Vec<(semver::Version, Vec<Manifest>)> = Vec::new();
    for manifest in manifests {
        let Ok(version) = semver::Version::parse(&manifest.version) else {
            continue;
        };
        if version <= *current {
            continue;
        }
        match offers.iter_mut().find(|(v, _)| *v == version) {
            Some((_, sources)) => sources.push(manifest),
            None => offers.push((version, vec![manifest])),
        }
    }
    // Stable: the hosts keep their order within a version.
    offers.sort_by(|(a, _), (b, _)| b.cmp(a));
    offers
        .into_iter()
        .map(|(v, sources)| (v.to_string(), sources))
        .collect()
}

/// The package in the cache, verified: already there from an earlier
/// launch that closed before installing it, or downloaded now.
fn fetch_package(
    client: &reqwest::blocking::Client,
    key: &str,
    dir: &Path,
    package: &Package,
    version: &str,
) -> io::Result<PathBuf> {
    let name = file_name(&package.url)
        .ok_or_else(|| invalid(format!("no file name in {}", package.url)))?;
    let file = dir.join(name);
    let cached = std::fs::File::open(&file)
        .and_then(|f| read_at_most(f, MAX_PACKAGE, "the cached package"))
        .ok()
        .filter(|bytes| verify(key, bytes, &package.signature, version, name).is_ok());
    if cached.is_none() {
        tracing::info!(%version, url = %package.url, "downloading the update");
        let bytes = download(client, &package.url, MAX_PACKAGE)?;
        verify(key, &bytes, &package.signature, version, name)?;
        let partial = file.with_extension("part");
        std::fs::write(&partial, &bytes)?;
        std::fs::rename(&partial, &file)?;
    }
    Ok(file)
}

/// The body at `url`, refused past `max` bytes.
fn download(client: &reqwest::blocking::Client, url: &str, max: u64) -> io::Result<Vec<u8>> {
    let response = client
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(io::Error::other)?;
    read_at_most(response, max, url)
}

/// All of `reader`, refused past `max` bytes.
fn read_at_most(reader: impl Read, max: u64, what: &str) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(invalid(format!("{what} is larger than {max} bytes")));
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

/// Where updates are downloaded to: `<cache dir>/waveflow-quickplay/updates`.
fn cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("waveflow-quickplay").join("updates"))
}

/// Packages of versions passed over or installed, and partial downloads,
/// go; `keep` stays.
fn clear_cache(dir: &Path, keep: &[PathBuf]) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !keep.contains(&path) && path.is_file() {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// An `InvalidData` error saying `e`.
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
/// swaps them, the old one coming back if the swap fails. The image is
/// copied out of the cache at once, so a launch while the shell waits
/// cannot clear it from under the shell; the cached one goes only once the
/// new bundle is in place, so a failed attempt is retried at the next close
/// without downloading again. Everything the shell makes is in folders of
/// its own.
fn replace_app(dmg: &Path, app: &Path) -> io::Result<()> {
    const SCRIPT: &str = r#"
pid=$1 dmg=$2 app=$3
work=$(mktemp -d "${TMPDIR:-/tmp}/quickplay-update.XXXXXX") || exit 1
cp "$dmg" "$work/update.dmg" || { rm -rf "$work"; exit 1; }
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
if mv "$stage/new.app" "$app"; then rm -rf "$stage" "$dmg"; else mv "$stage/old.app" "$app"; exit 1; fi
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

/// Marks a new AppImage executable (`rwxr-xr-x`).
#[cfg(unix)]
fn set_executable(file: &std::fs::File) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(std::fs::Permissions::from_mode(0o755))
}

/// Nothing to mark outside Unix.
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

    /// A package and the signature the test key made for it agree.
    #[test]
    fn a_signed_package_passes() {
        verify(TEST_KEY, TEST_PACKAGE, TEST_SIGNATURE, "1.2.3", "test.bin").unwrap();
    }

    /// One flipped bit fails the signature.
    #[test]
    fn a_changed_package_fails() {
        let mut changed = TEST_PACKAGE.to_vec();
        changed[0] ^= 1;
        assert!(verify(TEST_KEY, &changed, TEST_SIGNATURE, "1.2.3", "test.bin").is_err());
    }

    /// The release key does not accept what the test key signed.
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

    /// `APPIMAGE` counts only when this executable runs from that image's mount.
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

    /// A read past its limit is refused, one at the limit is not.
    #[test]
    fn reads_stop_at_the_limit() {
        assert_eq!(read_at_most(&b"1234"[..], 4, "t").unwrap(), b"1234");
        assert!(read_at_most(&b"12345"[..], 4, "t").is_err());
    }

    /// The public key compiled in is a valid minisign key.
    #[test]
    fn the_release_key_reads() {
        minisign_verify::PublicKey::from_base64(PUBLIC_KEY).unwrap();
    }

    /// Only a plain last segment of a URL becomes a file in the cache.
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

    /// A manifest offering `version` for Windows at `url`.
    fn manifest(version: &str, url: &str) -> Manifest {
        Manifest {
            version: version.into(),
            platforms: HashMap::from([(
                "windows-x86_64".into(),
                Package {
                    url: url.into(),
                    signature: "s".into(),
                },
            )]),
        }
    }

    /// `newer` as (version, package URLs) pairs, against `current`.
    fn offers(manifests: Vec<Manifest>, current: (u64, u64, u64)) -> Vec<(String, Vec<String>)> {
        let current = semver::Version::new(current.0, current.1, current.2);
        newer(manifests, &current)
            .into_iter()
            .map(|(v, sources)| {
                let urls = sources
                    .iter()
                    .map(|m| m.platforms["windows-x86_64"].url.clone())
                    .collect();
                (v, urls)
            })
            .collect()
    }

    /// One `offers` entry: a version and its package URLs.
    fn offer(version: &str, urls: &[&str]) -> (String, Vec<String>) {
        (version.into(), urls.iter().map(|u| u.to_string()).collect())
    }

    /// A stale host does not hide a newer version the other one offers, and
    /// the older one stays behind it in case no package backs the newer.
    #[test]
    fn the_newest_version_comes_first() {
        assert_eq!(
            offers(
                vec![manifest("1.5.0", "github"), manifest("1.6.0", "r2")],
                (1, 4, 0)
            ),
            [offer("1.6.0", &["r2"]), offer("1.5.0", &["github"])]
        );
    }

    /// Both hosts offering the version are kept, in the order they were
    /// asked, so the second one's package is there if the first one's fails.
    #[test]
    fn hosts_that_agree_are_all_kept() {
        assert_eq!(
            offers(
                vec![manifest("1.5.0", "github"), manifest("1.5.0", "r2")],
                (1, 4, 0)
            ),
            [offer("1.5.0", &["github", "r2"])]
        );
    }

    /// Nothing newer than this copy, or no manifest, is no update.
    #[test]
    fn nothing_newer_is_no_update() {
        assert!(offers(
            vec![manifest("1.5.0", "github"), manifest("1.4.0", "r2")],
            (1, 5, 0)
        )
        .is_empty());
        assert!(offers(Vec::new(), (1, 5, 0)).is_empty());
    }

    /// Only versions newer than this copy are offered, whatever a host
    /// claims besides.
    #[test]
    fn older_versions_are_never_offered() {
        assert_eq!(
            offers(
                vec![manifest("99.0.0", "github"), manifest("1.3.0", "r2")],
                (1, 4, 0)
            ),
            [offer("99.0.0", &["github"])]
        );
    }

    /// A manifest whose version does not parse is left out, not an error.
    #[test]
    fn a_manifest_without_a_version_is_left_out() {
        assert_eq!(
            offers(
                vec![manifest("latest", "github"), manifest("1.5.0", "r2")],
                (1, 4, 0)
            ),
            [offer("1.5.0", &["r2"])]
        );
    }

    /// A local HTTP server on `listener` answering `routes` (path, status,
    /// body), 404 elsewhere, until the test process ends.
    fn serve(listener: std::net::TcpListener, routes: Vec<(String, u16, Vec<u8>)>) {
        use std::io::{BufRead, Write};
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = io::BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                if reader.read_line(&mut request).is_err() {
                    continue;
                }
                // The headers, up to the blank line.
                let mut header = String::new();
                while reader.read_line(&mut header).is_ok_and(|n| n > 2) {
                    header.clear();
                }
                let path = request.split_whitespace().nth(1).unwrap_or_default();
                let (status, body) = routes
                    .iter()
                    .find(|(p, ..)| p == path)
                    .map(|(_, s, b)| (*s, b.as_slice()))
                    .unwrap_or((404, b""));
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(body);
            }
        });
    }

    /// A manifest offering `version` for Windows at `url`, signed by the
    /// test key for 1.2.3 under the name test.bin.
    fn manifest_json(version: &str, url: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "version": version,
            "platforms": { "windows-x86_64": { "url": url, "signature": TEST_SIGNATURE } }
        }))
        .unwrap()
    }

    /// One route of the test server.
    fn route(path: &str, status: u16, body: &[u8]) -> (String, u16, Vec<u8>) {
        (path.into(), status, body.to_vec())
    }

    /// What `check_at` finds from a 1.0.0 Windows copy, against the two
    /// manifests the server has at /a and /b, with a cache of its own: the
    /// version and the bytes of the package it made ready.
    fn check_two_hosts(
        test: &str,
        routes: impl Fn(&str) -> Vec<(String, u16, Vec<u8>)>,
    ) -> io::Result<(String, Vec<u8>)> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        serve(listener, routes(&base));
        let dir =
            std::env::temp_dir().join(format!("quickplay-update-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let found = check_at(
            &[
                format!("{base}/a/latest.json"),
                format!("{base}/b/latest.json"),
            ],
            TEST_KEY,
            &semver::Version::new(1, 0, 0),
            &Install::Windows,
            Some(dir.clone()),
        )
        .map(|found| match found {
            Some(Found::Ready(update)) => {
                let bytes = std::fs::read(&update.file).unwrap();
                (update.version, bytes)
            }
            _ => panic!("no update ready"),
        });
        let _ = std::fs::remove_dir_all(&dir);
        found
    }

    /// A host answering with a page that is not a manifest leaves the update to
    /// the other one.
    #[test]
    fn a_host_that_fails_leaves_it_to_the_other() {
        let found = check_two_hosts("manifest", |base| {
            vec![
                route("/a/latest.json", 200, b"<html>account suspended</html>"),
                route(
                    "/b/latest.json",
                    200,
                    &manifest_json("1.2.3", &format!("{base}/b/test.bin")),
                ),
                route("/b/test.bin", 200, TEST_PACKAGE),
            ]
        });
        assert_eq!(found.unwrap(), ("1.2.3".into(), TEST_PACKAGE.to_vec()));
    }

    /// A package that cannot be downloaded is fetched from the other host.
    #[test]
    fn a_package_that_fails_leaves_it_to_the_other_host() {
        let found = check_two_hosts("package", |base| {
            vec![
                route(
                    "/a/latest.json",
                    200,
                    &manifest_json("1.2.3", &format!("{base}/a/test.bin")),
                ),
                route("/a/test.bin", 500, b""),
                route(
                    "/b/latest.json",
                    200,
                    &manifest_json("1.2.3", &format!("{base}/b/test.bin")),
                ),
                route("/b/test.bin", 200, TEST_PACKAGE),
            ]
        });
        assert_eq!(found.unwrap(), ("1.2.3".into(), TEST_PACKAGE.to_vec()));
    }

    /// A host announcing 9.9.9 over a package signed for 1.2.3 does not keep
    /// the real 1.2.3 from being installed.
    #[test]
    fn a_version_no_package_backs_is_passed_over() {
        let found = check_two_hosts("fake", |base| {
            vec![
                route(
                    "/a/latest.json",
                    200,
                    &manifest_json("9.9.9", &format!("{base}/a/test.bin")),
                ),
                route("/a/test.bin", 200, TEST_PACKAGE),
                route(
                    "/b/latest.json",
                    200,
                    &manifest_json("1.2.3", &format!("{base}/b/test.bin")),
                ),
                route("/b/test.bin", 200, TEST_PACKAGE),
            ]
        });
        assert_eq!(found.unwrap(), ("1.2.3".into(), TEST_PACKAGE.to_vec()));
    }

    /// No manifest at all is an error, not "up to date".
    #[test]
    fn every_host_failing_is_an_error() {
        let found = check_two_hosts("down", |_| vec![route("/a/latest.json", 503, b"")]);
        assert!(found.is_err());
    }

    /// The R2 copy is the one that outlives the GitHub account; the release
    /// workflows publish it at this address (scripts/release/publish-r2.sh).
    #[test]
    fn the_second_host_is_the_r2_copy() {
        assert!(MANIFEST_URLS[0].starts_with("https://github.com/InstaZDLL/waveflow-quickplay/"));
        assert_eq!(
            MANIFEST_URLS[1],
            "https://download.waveflow.app/quickplay/latest.json"
        );
    }

    /// The manifest format the release workflows write reads.
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
