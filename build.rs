//! Windows only: embed the app icon and version info into the .exe.
//! Failing to do so (no `rc.exe`) must not break the build, so it only warns.

fn main() {
    println!("cargo:rerun-if-changed=packaging/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("packaging/icon.ico");
    res.set("ProductName", "YTFast");
    res.set("FileDescription", "YTFast - audio-only YouTube player");
    res.set("LegalCopyright", "Copyright (c) AminPerez. MIT");
    if let Err(e) = res.compile() {
        println!("cargo:warning=could not embed the Windows icon/version resources: {e}");
    }
}
