use eframe::egui;
use eframe::egui::text::LayoutJob;
use std::fs;
use crate::mnx::MnxDocument;
use crate::validate;
use crate::render;

pub struct MnxEditorApp {
    yaml_text: String,
    rendered_tab: LayoutJob,
    validation_errors: Vec<String>,
}

impl Default for MnxEditorApp {
    fn default() -> Self {
        let initial_yaml = fs::read_to_string("example.yaml")
            .unwrap_or_else(|_| String::from("# Could not load example.yaml"));
        
        let mut app = Self {
            yaml_text: initial_yaml,
            rendered_tab: LayoutJob::default(),
            validation_errors: Vec::new(),
        };
        
        app.process_and_render();
        app
    }
}

impl MnxEditorApp {
    fn process_and_render(&mut self) {
        match serde_yaml::from_str::<MnxDocument>(&self.yaml_text) {
            Ok(doc) => {
                let errors = validate::validate_mnx(&doc);
                
                // Store formatted error strings for the bottom panel UI
                self.validation_errors = errors.iter()
                    .map(|err| format!("Measure {}, Event {}: {}", err.measure_idx, err.event_idx + 1, err.message))
                    .collect();

                if let Some(part) = doc.parts.first() {
                    // Always render the tab, pass errors for red coordinate highlighting
                    self.rendered_tab = render::render_part_to_tab(part, &errors);
                } else {
                    let mut job = LayoutJob::default();
                    job.append("No parts found in document.", 0.0, egui::TextFormat::default());
                    self.rendered_tab = job;
                }
            }
            Err(e) => {
                self.validation_errors = vec![format!("YAML PARSE ERROR:\n{}", e)];
            }
        }
    }
}

impl eframe::App for MnxEditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        
        // 1. Bottom Panel for Errors (only shown if errors exist)
        if !self.validation_errors.is_empty() {
            egui::Panel::bottom("error_panel")
                .resizable(true)
                .min_size(100.0)
                .show(ui, |ui| {
                    ui.heading(egui::RichText::new("Validation Errors").color(egui::Color32::RED));
                    ui.separator();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for err in &self.validation_errors {
                            ui.label(egui::RichText::new(err).color(egui::Color32::LIGHT_RED));
                        }
                    });
                });
        }

        // 2. Right Pane: YAML Text Editor
        egui::Panel::right("yaml_panel")
            .resizable(true)
            .min_size(350.0)
            .show(ui, |ui| {
                ui.heading("MNX Source (YAML)");
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let response = ui.add(
                        egui::TextEdit::multiline(&mut self.yaml_text)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .code_editor(), 
                    );
                    
                    // Trigger real-time re-render on any keystroke
                    if response.changed() {
                        self.process_and_render();
                    }
                });
            });

        // 3. Left Pane: Render Output
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Tablature Render");
            ui.separator();
            egui::ScrollArea::both().show(ui, |ui| {
                ui.label(self.rendered_tab.clone());
            });
        });
    }
}