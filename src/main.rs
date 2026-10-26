pub mod curve_editor;
mod edo;
pub mod harmonic;
mod gui;
pub mod layout;
pub mod midi;
mod mnx;
pub mod pdf_export;
mod render;
pub mod tuning_table;
mod validate;
pub mod yaml_editor;

use eframe::egui;

/// Register the Bravura SMuFL font so egui can render music notation glyphs.
pub(crate) fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Bravura".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/Bravura.otf")).into(),
    );
    fonts.font_data.insert(
        "BravuraText".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/BravuraText.otf")).into(),
    );
    fonts
        .families
        .entry(egui::FontFamily::Name("Bravura".into()))
        .or_default()
        .insert(0, "Bravura".to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Name("BravuraText".into()))
        .or_default()
        .insert(0, "BravuraText".to_owned());
    ctx.set_fonts(fonts);
}

fn main() -> eframe::Result<()> {
    let icon = eframe::icon_data::from_png_bytes(&include_bytes!("../assets/icon.png")[..])
        .expect("Failed to load application icon");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 600.0])
            .with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        "guitab",
        options,
        Box::new(|cc| {
            setup_custom_fonts(&cc.egui_ctx);
            Ok(Box::new(gui::MnxEditorApp::default()))
        }),
    )
}
pub mod rebar;
