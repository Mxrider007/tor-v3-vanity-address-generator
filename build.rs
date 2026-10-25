//! Embeds the icon and version information in the Windows command-line .exe,
//! and with the `kernel-test` feature builds the CUDA kernel for the CPU.

fn main() {
    println!("cargo:rerun-if-changed=gui/assets/icon.ico");
    // The tests run the CUDA kernel on the CPU, compiled as plain C++.
    #[cfg(feature = "kernel-test")]
    {
        const KERNEL: &str = "src/search/gpu/cuda/kernel.cu";
        println!("cargo:rerun-if-changed={KERNEL}");
        let mut build = cc::Build::new();
        build.cpp(true).file(KERNEL).opt_level(2);
        if build.get_compiler().is_like_msvc() {
            build.flag("/TP").flag("/std:c++20");
        } else {
            build
                .flag("-x")
                .flag("c++")
                .flag("-std=c++20")
                .warnings_into_errors(true);
        }
        build.compile("torv3_cuda_kernel_host");
    }
    // MSVC only: with the GNU toolchain the resource would also be attached
    // to the library, and so end up in every program that links it.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("gui/assets/icon.ico")
            .set("ProductName", "Tor V3 Vanity Address Generator")
            .set(
                "FileDescription",
                "Tor V3 Vanity Address Generator (command line)",
            )
            .set("LegalCopyright", "MIT License");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the Windows icon: {e}");
        }
    }
}
