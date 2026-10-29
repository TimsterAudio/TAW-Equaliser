// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use taw_equaliser::{icon, instance, ui};

fn main() -> eframe::Result {
    // One Equaliser at a time: if one is open, bring it forward instead.
    let dir = instance::data_dir();
    if dir.as_deref().is_some_and(instance::hand_over) {
        return Ok(());
    }
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
        Box::new(move |cc| {
            if let Some(dir) = dir {
                instance::watch(dir, cc.egui_ctx.clone());
            }
            Ok(Box::new(ui::app::EqApp::new(cc)))
        }),
    )
}
