//! Build script: puts the app icon into `SiftMail.exe` on Windows.
//!
//! GPUI gives every window icon resource 1 from the executable, so the same
//! icon shows in Explorer, the taskbar and Alt+Tab. macOS and Linux take
//! their icons from the installer bundles instead (see `[package.metadata.packager]`).

fn main() {
    println!("cargo:rerun-if-changed=assets/app-icon/SiftMail.rc");
    println!("cargo:rerun-if-changed=assets/app-icon/SiftMail.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if let Err(error) = embed_resource::compile("assets/app-icon/SiftMail.rc", embed_resource::NONE)
        .manifest_optional()
    {
        eprintln!("couldn't embed the app icon: {error}");
        std::process::exit(1);
    }
}
