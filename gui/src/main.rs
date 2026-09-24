//! Kore Vanity — graphical Tor v3 vanity address generator.

// Release builds are GUI-only on Windows: no console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("the embedded icon is a valid PNG");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Kore Vanity")
            .with_app_id("kore-vanity")
            .with_inner_size([760.0, 820.0])
            .with_min_inner_size([560.0, 520.0])
            .with_icon(icon),
        ..Default::default()
    };
    eframe::run_native(
        "Kore Vanity",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
