pub mod curve_editor;
mod edo;
mod gui;
pub mod midi;
mod mnx;
mod render;
pub mod tuning_table;
mod validate;
pub mod yaml_editor;

use eframe::egui;

/// Register the Bravura SMuFL font so egui can render music notation glyphs.
fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Bravura".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/Bravura.otf")).into(),
    );
    fonts
        .families
        .entry(egui::FontFamily::Name("Bravura".into()))
        .or_default()
        .insert(0, "Bravura".to_owned());
    ctx.set_fonts(fonts);
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1000.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "MNX Tablature Editor",
        options,
        Box::new(|cc| {
            setup_custom_fonts(&cc.egui_ctx);
            Ok(Box::new(gui::MnxEditorApp::default()))
        }),
    )
}
pub mod rebar;
