use eframe::egui::{self, Color32};

use std::fs;
use crate::mnx::MnxDocument;
use crate::validate;
use crate::render;

pub struct MnxEditorApp {
    yaml_text: String,
    parsed_mnx: Option<MnxDocument>,
    parsed_errors: Vec<validate::ValidationError>,
    validation_errors: Vec<String>,
    current_file_path: Option<std::path::PathBuf>, // Tracks the active file
    selected_event_idx: Option<usize>,             // Selected note/beat index
}

impl Default for MnxEditorApp {
    fn default() -> Self {
        let initial_yaml = fs::read_to_string("example.yaml")
            .unwrap_or_else(|_| String::from("# Could not load example.yaml"));
        
        let mut app = Self {
            yaml_text: initial_yaml,
            parsed_mnx: None,
            parsed_errors: Vec::new(),
            validation_errors: Vec::new(),
            current_file_path: None,
            selected_event_idx: None,
        };
        
        app.process_and_render();
        app
    }
}

/// Locates character byte ranges `start..end` for all events under `content:` blocks in the YAML text.
pub fn find_all_event_ranges(yaml: &str) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let lines: Vec<&str> = yaml.lines().collect();
    if lines.is_empty() {
        return ranges;
    }

    let mut line_starts = Vec::with_capacity(lines.len());
    let mut offset = 0;
    for line in yaml.split_inclusive('\n') {
        line_starts.push(offset);
        offset += line.len();
    }
    while line_starts.len() < lines.len() {
        let last_start = *line_starts.last().unwrap_or(&0);
        let prev_len = lines[line_starts.len() - 1].len();
        line_starts.push(last_start + prev_len);
    }

    let mut in_content = false;
    let mut content_indent = 0;
    let mut current_event_start_line: Option<usize> = None;
    let mut event_indent = 0;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with("content:") || trimmed.ends_with("content:") {
            if let Some(start_line) = current_event_start_line {
                let end_line = i.saturating_sub(1);
                let start = line_starts[start_line];
                let end = line_starts[end_line] + lines[end_line].len();
                ranges.push(start..end);
                current_event_start_line = None;
            }
            in_content = true;
            content_indent = indent;
            continue;
        }

        if in_content {
            if indent <= content_indent
                && (trimmed.starts_with("measures:")
                    || trimmed.starts_with("sequences:")
                    || trimmed.starts_with("parts:")
                    || trimmed.starts_with("- sequences:")
                    || trimmed.starts_with("- measures:"))
            {
                if let Some(start_line) = current_event_start_line {
                    let end_line = i.saturating_sub(1);
                    let start = line_starts[start_line];
                    let end = line_starts[end_line] + lines[end_line].len();
                    ranges.push(start..end);
                    current_event_start_line = None;
                }
                in_content = false;
                continue;
            }

            if trimmed.starts_with('-') {
                if let Some(start_line) = current_event_start_line {
                    if indent <= event_indent {
                        let end_line = i.saturating_sub(1);
                        let start = line_starts[start_line];
                        let end = line_starts[end_line] + lines[end_line].len();
                        ranges.push(start..end);
                        current_event_start_line = Some(i);
                        event_indent = indent;
                    }
                } else {
                    current_event_start_line = Some(i);
                    event_indent = indent;
                }
            }
        }
    }

    if let Some(start_line) = current_event_start_line {
        let end_line = lines.len().saturating_sub(1);
        let start = line_starts[start_line];
        let end = line_starts[end_line] + lines[end_line].len();
        ranges.push(start..end);
    }

    ranges
}

impl MnxEditorApp {
    fn process_and_render(&mut self) {
        match serde_saphyr::from_str::<MnxDocument>(&self.yaml_text) {
            Ok(doc) => {
                let errors = validate::validate_mnx(&doc);
                
                // Store formatted error strings for the bottom panel UI
                self.validation_errors = errors.iter()
                    .map(|err| format!("Measure {}, Event {}: {}", err.measure_idx, err.event_idx + 1, err.message))
                    .collect();

                self.parsed_errors = errors;
                self.parsed_mnx = Some(doc);
            }
            Err(e) => {
                self.parsed_mnx = None;
                self.parsed_errors = Vec::new();
                self.validation_errors = vec![format!("YAML PARSE ERROR:\n{}", e)];
            }
        }
    }
    fn open_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("YAML Documents", &["yaml", "yml"])
            .pick_file()
        {
            match fs::read_to_string(&path) {
                Ok(content) => {
                    self.yaml_text = content;
                    self.current_file_path = Some(path);
                    self.selected_event_idx = None;
                    self.process_and_render();
                }
                Err(e) => {
                    self.validation_errors = vec![format!("Failed to read file: {}", e)];
                }
            }
        }
    }

    fn save_file(&mut self) {
        // If we have a known file path, save to it. Otherwise, trigger "Save As".
        if let Some(path) = &self.current_file_path {
            if let Err(e) = fs::write(path, &self.yaml_text) {
                self.validation_errors = vec![format!("Failed to save file: {}", e)];
            }
        } else {
            self.save_as_file();
        }
    }

    fn save_as_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("YAML Documents", &["yaml", "yml"])
            .set_file_name("untitled.yaml")
            .save_file()
        {
            if let Err(e) = fs::write(&path, &self.yaml_text) {
                self.validation_errors = vec![format!("Failed to save file: {}", e)];
            } else {
                // Update the current path so subsequent "Saves" go here
                self.current_file_path = Some(path);
            }
        }
    }
}

impl eframe::App for MnxEditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let event_ranges = find_all_event_ranges(&self.yaml_text);
        let total_events = event_ranges.len();
        let text_edit_id = egui::Id::new("yaml_source_editor");

        // Clamp selection if document changed
        if let Some(idx) = self.selected_event_idx {
            if total_events == 0 {
                self.selected_event_idx = None;
            } else if idx >= total_events {
                self.selected_event_idx = Some(total_events - 1);
            }
        }

        // Handle Arrow Left / Right navigation between notes when editor is not actively focused
        if total_events > 0 {
            let left_pressed = ui.input(|i| i.key_pressed(egui::Key::ArrowLeft));
            let right_pressed = ui.input(|i| i.key_pressed(egui::Key::ArrowRight));

            if left_pressed || right_pressed {
                let editor_focused = ui.memory(|m| m.has_focus(text_edit_id));
                if !editor_focused {
                    if left_pressed {
                        self.selected_event_idx = match self.selected_event_idx {
                            Some(idx) => Some(idx.saturating_sub(1)),
                            None => Some(total_events.saturating_sub(1)),
                        };
                    }
                    if right_pressed {
                        self.selected_event_idx = match self.selected_event_idx {
                            Some(idx) => Some((idx + 1).min(total_events - 1)),
                            None => Some(0),
                        };
                    }
                }
            }
        }

        // 1. Top Menu Bar (Updated with file operations)
        egui::Panel::top("top_panel").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Open").clicked() {
                    self.open_file();
                }
                if ui.button("Save").clicked() {
                    self.save_file();
                }
                if ui.button("Save As...").clicked() {
                    self.save_as_file();
                }

                ui.separator();

                // Display the current filename or "Untitled"
                let file_name = self.current_file_path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .unwrap_or("Untitled");
                
                ui.label(egui::RichText::new(format!("Editing: {}", file_name)).italics());

                if let Some(sel) = self.selected_event_idx {
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!("Note {} of {}", sel + 1, total_events))
                            .color(Color32::from_rgb(100, 180, 255))
                            .strong(),
                    );
                }
            });
        });

        // 2. Bottom Panel for Errors (only shown if errors exist)
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

        // 3. Right Pane: YAML Text Editor
        egui::Panel::right("yaml_panel")
            .resizable(true)
            .min_size(350.0)
            .show(ui, |ui| {
                ui.heading("MNX Source (YAML)");
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let selected_range = self.selected_event_idx
                        .and_then(|idx| event_ranges.get(idx).cloned());

                    let mut layouter = |ui: &egui::Ui, text_buf: &dyn egui::TextBuffer, wrap_width: f32| {
                        let text = text_buf.as_str();
                        let font_id = egui::TextStyle::Monospace.resolve(ui.style());
                        let text_color = ui.visuals().text_color();
                        let mut job = egui::text::LayoutJob::default();
                        job.wrap.max_width = wrap_width;

                        if let Some(sel) = &selected_range {
                            let sel_start = sel.start.min(text.len());
                            let sel_end = sel.end.min(text.len());

                            if sel_start > 0 {
                                job.append(
                                    &text[..sel_start],
                                    0.0,
                                    egui::TextFormat {
                                        font_id: font_id.clone(),
                                        color: text_color,
                                        ..Default::default()
                                    },
                                );
                            }

                            if sel_end > sel_start {
                                job.append(
                                    &text[sel_start..sel_end],
                                    0.0,
                                    egui::TextFormat {
                                        font_id: font_id.clone(),
                                        color: Color32::from_rgb(220, 240, 255),
                                        background: Color32::from_rgba_unmultiplied(60, 130, 210, 120),
                                        ..Default::default()
                                    },
                                );
                            }

                            if sel_end < text.len() {
                                job.append(
                                    &text[sel_end..],
                                    0.0,
                                    egui::TextFormat {
                                        font_id: font_id.clone(),
                                        color: text_color,
                                        ..Default::default()
                                    },
                                );
                            }
                        } else {
                            job.append(
                                text,
                                0.0,
                                egui::TextFormat {
                                    font_id: font_id.clone(),
                                    color: text_color,
                                    ..Default::default()
                                },
                            );
                        }

                        ui.fonts_mut(|f| f.layout_job(job))
                    };

                    let response = ui.add(
                        egui::TextEdit::multiline(&mut self.yaml_text)
                            .id(text_edit_id)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .code_editor()
                            .layouter(&mut layouter),
                    );
                    
                    // Trigger real-time re-render on any keystroke
                    if response.changed() {
                        self.process_and_render();
                    }
                });
            });

        // 4. Left Pane: Render Output
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Tablature Render");
            ui.separator();
            egui::ScrollArea::both().show(ui, |ui| {
                if let Some(doc) = &self.parsed_mnx {
                    if let Some(part) = doc.parts.first() {
                        let (_resp, clicked) = render::draw_tablature(
                            ui,
                            part,
                            &self.parsed_errors,
                            self.selected_event_idx,
                        );
                        if let Some(clicked_idx) = clicked {
                            self.selected_event_idx = Some(clicked_idx);
                        }
                    } else {
                        ui.label("No parts found in document.");
                    }
                } else {
                    ui.label("Could not parse document.");
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_all_event_ranges_example_yaml() {
        let yaml = include_str!("../example.yaml");
        let ranges = find_all_event_ranges(yaml);
        assert_eq!(ranges.len(), 15);
        for range in &ranges {
            let snippet = &yaml[range.clone()];
            assert!(snippet.contains("duration"));
            assert!(snippet.contains("notes"));
        }
    }
}