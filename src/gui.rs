use eframe::egui;
use std::fs;
use crate::mnx::MnxDocument;
use crate::validate;
use crate::render;

pub struct MnxEditorApp {
    yaml_text: String,
    rendered_tab: String,
}

impl Default for MnxEditorApp {
    fn default() -> Self {
        // Attempt to load the initial yaml from disk
        let initial_yaml = fs::read_to_string("example.yaml")
            .unwrap_or_else(|_| String::from("# Could not load example.yaml"));
        
        let mut app = Self {
            yaml_text: initial_yaml,
            rendered_tab: String::from("Ready to render."),
        };
        
        // Run an initial processing pass
        app.process_and_render();
        app
    }
}

impl MnxEditorApp {
    fn process_and_render(&mut self) {
        match serde_yaml::from_str::<MnxDocument>(&self.yaml_text) {
            Ok(doc) => {
                let errors = validate::validate_mnx(&doc);
                if errors.is_empty() {
                    if let Some(part) = doc.parts.first() {
                        self.rendered_tab = render::render_part_to_tab(part);
                    } else {
                        self.rendered_tab = String::from("No parts found in document.");
                    }
                } else {
                    let mut err_msg = String::from("VALIDATION ERRORS:\n");
                    for err in errors {
                        err_msg.push_str(&format!("- Measure {}: {}\n", err.measure_idx, err.message));
                    }
                    self.rendered_tab = err_msg;
                }
            }
            Err(e) => {
                self.rendered_tab = format!("YAML PARSE ERROR:\n{}", e);
            }
        }
    }
}

impl eframe::App for MnxEditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Top Menu Bar
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Process / Refresh").clicked() {
                    self.process_and_render();
                }
            });
        });

        // Right Pane: YAML Text Editor
        egui::SidePanel::right("yaml_panel")
            .resizable(true)
            .min_width(350.0)
            .show(ctx, |ui| {
                ui.heading("MNX Source (YAML)");
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.yaml_text)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .code_editor(), // Enables code-editor specific behaviors
                    );
                });
            });

        // Left Pane: ASCII Render Output
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Tablature Render");
            ui.separator();
            egui::ScrollArea::both().show(ui, |ui| {
                ui.label(
                    egui::RichText::new(&self.rendered_tab)
                        .family(egui::FontFamily::Monospace)
                        .size(16.0),
                );
            });
        });
    }
}