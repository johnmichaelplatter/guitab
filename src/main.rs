mod edo;
mod mnx;
mod render;
mod validate;
mod gui;
pub mod tuning_table;
pub mod midi;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 600.0]),
        ..Default::default()
    };
    
    eframe::run_native(
        "MNX Tablature Editor",
        options,
        Box::new(|_cc| Ok(Box::new(gui::MnxEditorApp::default()))),
    )
}