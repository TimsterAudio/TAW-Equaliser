// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use taw_equaliser::{icon, ui};

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TAW Equaliser")
            .with_app_id("taw-equaliser")
            .with_inner_size(ui::app::DESIGN)
            .with_min_inner_size(ui::app::DESIGN * 0.5)
            // The Equaliser draws its own title strip and window buttons;
            // Settings can bring back the system's.
            .with_decorations(false)
            .with_icon(icon::icon()),
        ..Default::default()
    };
    eframe::run_native(
        "TAW Equaliser",
        options,
        Box::new(|cc| Ok(Box::new(ui::app::EqApp::new(cc)))),
    )
}
