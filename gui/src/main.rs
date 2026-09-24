//! Kore Vanity — graphical Tor v3 vanity address generator.

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
            .with_title("Kore Vanity")
            .with_app_id("kore-vanity")
            .with_inner_size([760.0, 820.0])
            .with_min_inner_size([560.0, 520.0])
            .with_icon(icon),
        ..Default::default()
    }
}

fn run(renderer: eframe::Renderer) -> eframe::Result<()> {
    eframe::run_native(
        "Kore Vanity",
        options(renderer),
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}

fn main() {
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
            .set_title("Kore Vanity could not start")
            .set_description(format!(
                "The window could not be opened:\n{e}\n\nUpdating your graphics driver usually \
                 fixes this. The command-line version (kore-vanity.exe) works without graphics."
            ))
            .show();
        std::process::exit(1);
    }
}
