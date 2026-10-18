//! Windows only: stamp the WaveFlow icon and a VERSIONINFO block into the
//! executable so Explorer, the taskbar and Alt-Tab show it. Nothing to do on
//! the other targets.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon-128.png");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let ico = std::path::Path::new("assets/icon.ico");
        if ico.exists() {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("assets/icon.ico");
            res.set("ProductName", "WaveFlow QuickPlay");
            res.set("FileDescription", "WaveFlow QuickPlay");
            if let Err(err) = res.compile() {
                println!("cargo:warning=windows resource not embedded: {err}");
            }
        }
    }
}
