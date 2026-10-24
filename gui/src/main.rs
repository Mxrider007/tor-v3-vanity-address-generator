//! Tor V3 Vanity Address Generator — graphical Tor v3 vanity address generator.

// Release builds are GUI-only on Windows: no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use eframe::egui;

fn options(renderer: eframe::Renderer) -> eframe::NativeOptions {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("the embedded icon is a valid PNG");
    eframe::NativeOptions {
        renderer,
        viewport: egui::ViewportBuilder::default()
            .with_title("Tor V3 Vanity Address Generator")
            .with_app_id("torv3-vanity")
            .with_inner_size([760.0, 820.0])
            .with_min_inner_size([560.0, 520.0])
            .with_icon(icon),
        ..Default::default()
    }
}

fn run(renderer: eframe::Renderer) -> eframe::Result<()> {
    eframe::run_native(
        "Tor V3 Vanity Address Generator",
        options(renderer),
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}

/// Show crashes in a message box: release builds on Windows have no console
/// where the message would otherwise appear.
fn install_panic_dialog() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        // Search threads report their own crashes in the window.
        if std::thread::current().name() != Some("main") {
            return;
        }
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown error".into());
        let location = info
            .location()
            .map(|l| format!(" ({}:{})", l.file(), l.line()))
            .unwrap_or_default();
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("Tor V3 Vanity Address Generator crashed")
            .set_description(format!(
                "Sorry, something went wrong: {message}{location}\n\nVersion {}. Please \
                 report this at {}/issues",
                torv3_vanity::VERSION,
                app::REPO_URL
            ))
            .show();
    }));
}

fn main() {
    install_panic_dialog();
    // DirectX 12 / Vulkan / Metal first; OpenGL if that is unavailable (for
    // example in some virtual machines).
    let result = run(eframe::Renderer::Wgpu).or_else(|e| {
        eprintln!("Could not start with DirectX/Vulkan/Metal ({e}); trying OpenGL.");
        run(eframe::Renderer::Glow)
    });
    if let Err(e) = result {
        eprintln!("Could not open the window: {e}");
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("Tor V3 Vanity Address Generator could not start")
            .set_description(format!(
                "The window could not be opened:\n{e}\n\nUpdating your graphics driver usually \
                 fixes this. The command-line version (torv3-vanity.exe) works without graphics."
            ))
            .show();
        std::process::exit(1);
    }
}
