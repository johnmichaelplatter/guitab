mod edo;
mod mnx;
mod render;
mod validate;
mod gui; // Add this line

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
        Box::new(|_cc| Box::new(gui::MnxEditorApp::default())),
    )
}