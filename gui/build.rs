//! Embeds the application icon and version information in the Windows .exe.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    // MSVC only: with the GNU toolchain the resource would also be attached
    // to the library, and so end up in every program that links it.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico")
            .set("ProductName", "Tor V3 Vanity Address Generator")
            .set("FileDescription", "Tor V3 Vanity Address Generator")
            .set("LegalCopyright", "MIT License");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the Windows icon: {e}");
        }
    }
}
