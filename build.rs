//! Windows only: stamp the WaveFlow icon, the document icons of the audio
//! types (src/file_icons.rs) and a VERSIONINFO block into the executable
//! so Explorer, the taskbar and Alt-Tab show it. Nothing to do on the
//! other targets.

include!("src/file_icons.rs");

fn main() {
    println!("cargo:rerun-if-changed=assets/icon-128.png");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/file-icons");
    println!("cargo:rerun-if-changed=src/file_icons.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let ico = std::path::Path::new("assets/icon.ico");
        if ico.exists() {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("assets/icon.ico");
            for (stem, id) in FILE_ICONS {
                let path = format!("assets/file-icons/{stem}.ico");
                // A missing file would fail the whole resource script below,
                // which only warns: the executable would ship without its
                // own icon and with ProgIDs pointing at nothing. The files
                // are in the repository, so a missing one is an error.
                assert!(
                    std::path::Path::new(&path).is_file(),
                    "{path} is missing (listed in src/file_icons.rs)"
                );
                res.set_icon_with_id(&path, &id.to_string());
            }
            res.set("ProductName", "WaveFlow QuickPlay");
            res.set("FileDescription", "WaveFlow QuickPlay");
            if let Err(err) = res.compile() {
                println!("cargo:warning=windows resource not embedded: {err}");
            }
        }
    }
}
