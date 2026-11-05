use super::*;
use crate::yaml_editor::*;
use eframe::egui::{self, Color32};

/// Helper to paint a DAW-style bottom-right corner triangle indicator (procedural "◢")
/// onto a button's rect, dynamically matching the button's interactive text color.
fn paint_corner_triangle(ui: &egui::Ui, response: &egui::Response) {
    let r = response.rect;
    let margin = 3.5;
    let size = 5.0;
    let color = ui.style().interact(response).fg_stroke.color;
    let p_corner = egui::pos2(r.max.x - margin, r.max.y - margin);
    let p_left = egui::pos2(r.max.x - margin - size, r.max.y - margin);
    let p_top = egui::pos2(r.max.x - margin, r.max.y - margin - size);
    ui.painter().add(egui::Shape::convex_polygon(
        vec![p_left, p_corner, p_top],
        color,
        egui::Stroke::NONE,
    ));
}

/// Helper to procedurally paint a status indicator circle instead of relying on unicode glyphs.
fn paint_status_dot(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.5, color);
}

impl MnxEditorApp {
    pub(crate) fn render_top_panel(
        &mut self,
        ui: &mut egui::Ui,
        event_ranges: &[EventRange],
        total_events: usize,
    ) {
        egui::Panel::top("top_panel").show(ui, |ui| {
            let cmd_label = if cfg!(target_os = "macos") {
                "Cmd"
            } else {
                "Ctrl"
            };

            egui::MenuBar::new().ui(ui, |ui| {
                // 1. File Menu
                ui.menu_button("File", |ui| {
                    if ui
                        .add(
                            egui::Button::new("New")
                                .shortcut_text(format!("{}+N", cmd_label)),
                        )
                        .clicked()
                    {
                        self.new_file();
                        ui.close();
                    }
                    if ui
                        .add(
                            egui::Button::new("Open")
                                .shortcut_text(format!("{}+O", cmd_label)),
                        )
                        .clicked()
                    {
                        self.open_file();
                        ui.close();
                    }
                    if ui
                        .add(
                            egui::Button::new("Save")
                                .shortcut_text(format!("{}+S", cmd_label)),
                        )
                        .clicked()
                    {
                        self.save_file();
                        ui.close();
                    }
                    if ui
                        .add(
                            egui::Button::new("Save As")
                                .shortcut_text(format!("{}+Shift+S", cmd_label)),
                        )
                        .clicked()
                    {
                        self.save_as_file();
                        ui.close();
                    }
                    ui.separator();
                    ui.menu_button("Export", |ui| {
                        if ui.button("PDF").clicked() {
                            self.export_pdf_file();
                            ui.close();
                        }
                        if ui.button("MNX").clicked() {
                            self.export_mnx_file();
                            ui.close();
                        }
                    });
                });

                // 2. Edit Menu
                ui.menu_button("Edit", |ui| {
                    let can_undo = !self.undo_stack.is_empty();
                    let can_redo = !self.redo_stack.is_empty();
                    let has_selection = self.selected_event_idx.is_some();
                    let has_clipboard = self.clipboard.is_some();

                    if ui
                        .add_enabled(
                            can_undo,
                            egui::Button::new("Undo")
                                .shortcut_text(format!("{}+Z", cmd_label)),
                        )
                        .clicked()
                    {
                        self.undo();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            can_redo,
                            egui::Button::new("Redo")
                                .shortcut_text(format!("{}+Shift+Z", cmd_label)),
                        )
                        .clicked()
                    {
                        self.redo();
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            has_selection,
                            egui::Button::new("Cut")
                                .shortcut_text(format!("{}+X", cmd_label)),
                        )
                        .clicked()
                    {
                        self.action_cut_overwrite();
                        ui.ctx().copy_text("mnx_tab_cli_data".to_string());
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            has_selection,
                            egui::Button::new("Cut time")
                                .shortcut_text(format!("{}+Shift+X", cmd_label)),
                        )
                        .clicked()
                    {
                        self.action_cut_time();
                        ui.ctx().copy_text("mnx_tab_cli_data".to_string());
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            has_selection,
                            egui::Button::new("Copy")
                                .shortcut_text(format!("{}+C", cmd_label)),
                        )
                        .clicked()
                    {
                        self.action_copy();
                        ui.ctx().copy_text("mnx_tab_cli_data".to_string());
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            has_selection && has_clipboard,
                            egui::Button::new("Paste")
                                .shortcut_text(format!("{}+V", cmd_label)),
                        )
                        .clicked()
                    {
                        self.action_paste_overwrite();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            has_selection && has_clipboard,
                            egui::Button::new("Paste insert")
                                .shortcut_text(format!("{}+Shift+V", cmd_label)),
                        )
                        .clicked()
                    {
                        self.action_paste_insert();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            has_selection,
                            egui::Button::new("Duplicate")
                                .shortcut_text(format!("{}+D", cmd_label)),
                        )
                        .clicked()
                    {
                        self.action_duplicate_overwrite();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            has_selection,
                            egui::Button::new("Duplicate insert")
                                .shortcut_text(format!("{}+Shift+D", cmd_label)),
                        )
                        .clicked()
                    {
                        self.action_duplicate_insert();
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Score Information...").clicked() {
                        let title = self.parsed_mnx.as_ref().and_then(|d| d.score_title()).unwrap_or("").to_string();
                        let subtitle = self.parsed_mnx.as_ref().and_then(|d| d.score_subtitle()).unwrap_or("").to_string();
                        let composer = self.parsed_mnx.as_ref().and_then(|d| d.score_composer()).unwrap_or("").to_string();
                        self.active_dialog = ActiveDialog::ScoreInfo { title, subtitle, composer };
                        ui.close();
                    }
                });

                // 3. View Menu
                ui.menu_button("View", |ui| {
                    let is_fullscreen = ui
                        .ctx()
                        .input(|i| i.viewport().fullscreen.unwrap_or(false));
                    if ui.button("Toggle Full Screen").clicked() {
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::Fullscreen(!is_fullscreen));
                        ui.close();
                    }

                    let yaml_toggle_label = if self.show_yaml_panel {
                        "Hide MNX YAML editor"
                    } else {
                        "Show MNX YAML editor"
                    };
                    if ui.button(yaml_toggle_label).clicked() {
                        self.show_yaml_panel = !self.show_yaml_panel;
                        ui.close();
                    }

                    let mode_toggle_label = match self.view_mode {
                        ViewMode::Continuous => "Layout Mode",
                        ViewMode::Layout => "Continuous Mode",
                    };
                    if ui.button(mode_toggle_label).clicked() {
                        self.view_mode = match self.view_mode {
                            ViewMode::Continuous => ViewMode::Layout,
                            ViewMode::Layout => ViewMode::Continuous,
                        };
                        ui.close();
                    }

                    ui.separator();

                    let is_dark = ui.visuals().dark_mode;
                    ui.menu_button("Theme", |ui| {
                        if ui.selectable_label(is_dark, "Dark Mode").clicked() {
                            ui.ctx().set_visuals(egui::Visuals::dark());
                            ui.close();
                        }
                        if ui.selectable_label(!is_dark, "Light Mode").clicked() {
                            ui.ctx().set_visuals(egui::Visuals::light());
                            ui.close();
                        }
                    });

                    let current_page_size = self
                        .parsed_mnx
                        .as_ref()
                        .and_then(|d| d.layout.as_ref())
                        .map(|l| l.page_size.as_str())
                        .unwrap_or("letter");
                    let is_a4 = current_page_size.eq_ignore_ascii_case("a4");

                    ui.menu_button("Page Size", |ui| {
                        if ui.selectable_label(!is_a4, "Letter (8.5 x 11)").clicked() {
                            self.action_set_layout_page_size("letter");
                            ui.close();
                        }
                        if ui.selectable_label(is_a4, "A4").clicked() {
                            self.action_set_layout_page_size("a4");
                            ui.close();
                        }
                    });

                    let current_mn = self
                        .parsed_mnx
                        .as_ref()
                        .and_then(|d| d.layout.as_ref())
                        .map(|l| l.measure_numbers.as_str())
                        .unwrap_or("systemStart")
                        .to_string();

                    ui.menu_button("Bar Numbers", |ui| {
                        if ui
                            .selectable_label(
                                current_mn != "all" && current_mn != "none",
                                "System Start",
                            )
                            .clicked()
                        {
                            self.action_set_layout_measure_numbers("systemStart");
                            ui.close();
                        }
                        if ui.selectable_label(current_mn == "all", "All Bars").clicked() {
                            self.action_set_layout_measure_numbers("all");
                            ui.close();
                        }
                        if ui.selectable_label(current_mn == "none", "None").clicked() {
                            self.action_set_layout_measure_numbers("none");
                            ui.close();
                        }
                    });

                    if let Some(sel_id) = self.selected_layout_element_id.clone() {
                        ui.separator();
                        if ui
                            .button(format!("Reset Offset ({})", sel_id))
                            .clicked()
                        {
                            self.action_set_element_offset(&sel_id, 0.0, 0.0);
                            ui.close();
                        }
                    }
                });

                // 4. Help Menu
                ui.menu_button("Help", |ui| {
                    if ui.button("Manual").clicked() {
                        self.active_dialog = ActiveDialog::Manual;
                        ui.close();
                    }
                    if ui.button("About").clicked() {
                        self.active_dialog = ActiveDialog::About;
                        ui.close();
                    }
                });

                ui.separator();

                // Display the current filename or "Untitled"
                let file_name = self
                    .current_file_path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .unwrap_or("Untitled");

                ui.label(egui::RichText::new(format!("Editing: {}", file_name)).italics());

                if let Some(sel) = self.selected_event_idx {
                    ui.separator();
                    if self.is_editing_fret {
                        ui.label(
                            egui::RichText::new(
                                "EDIT FRET (type number, press Enter to commit, Esc to cancel)",
                            )
                            .color(Color32::from_rgb(255, 175, 75))
                            .strong(),
                        );
                    } else {
                        let note_info = if let Some(n_idx) = self.selected_note_idx_in_event {
                            let note_count = event_ranges
                                .get(sel)
                                .map(|er| er.note_ranges.len())
                                .unwrap_or(1);
                            format!(
                                "Note {} of {} (note {}/{})",
                                sel + 1,
                                total_events,
                                n_idx + 1,
                                note_count
                            )
                        } else {
                            let note_count = event_ranges
                                .get(sel)
                                .map(|er| er.note_ranges.len())
                                .unwrap_or(1);
                            if note_count > 1 {
                                format!(
                                    "Chord {} of {} ({} notes)",
                                    sel + 1,
                                    total_events,
                                    note_count
                                )
                            } else {
                                format!("Note {} of {}", sel + 1, total_events)
                            }
                        };
                        ui.label(
                            egui::RichText::new(note_info)
                                .color(Color32::from_rgb(100, 180, 255))
                                .strong(),
                        );
                    }
                }
            });

            ui.separator();

            // Horizontal Duration Toolbar
            enum DurationAction {
                Base(&'static str),
                ToggleDot,
            }
            let mut clicked_action: Option<DurationAction> = None;
            let mut rest_clicked = false;
            let mut tuplet_clicked = false;
            let mut grace_clicked = false;
            let mut tie_clicked = false;
            let mut slur_clicked = false;
            let mut tap_clicked = false;
            let mut bend_clicked = false;
            let mut slide_clicked = false;
            let mut accent_clicked = false;
            let mut strum_up_clicked = false;
            let mut strum_down_clicked = false;
            let mut arpeggio_up_clicked = false;
            let mut arpeggio_down_clicked = false;
            let mut harmonic_clicked = false;
            let has_text_focus = self.is_editing_lyric
                || self.active_dialog != ActiveDialog::None
                || ui.memory(|m| {
                    m.has_focus(egui::Id::new("bpm_input"))
                        || m.has_focus(egui::Id::new("yaml_source_editor"))
                });

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Duration:").strong());

                let current_dur = self.get_selected_event_duration();
                let current_base = current_dur.as_ref().map(|d| d.base.to_lowercase()).unwrap_or_default();
                let is_dotted = current_dur.as_ref().map(|d| d.dots.unwrap_or(0) > 0 || d.base.contains("dotted") || d.base.contains('.')).unwrap_or(false);

                let is_whole = current_base.contains("whole") || current_base == "1";
                let is_half = current_base.contains("half") || current_base == "1/2";
                let is_quarter = current_base.contains("quarter") || current_base == "4th" || current_base == "1/4";
                let is_eighth = current_base.contains("eighth") || current_base == "8th" || current_base == "1/8";
                let is_16th = current_base.contains("16th") || current_base.contains("sixteenth") || current_base == "1/16";
                let is_32nd = current_base.contains("32nd") || current_base.contains("thirtysecond") || current_base == "1/32";

                if let Some(textures) = &self.duration_textures {
                    let icon_size = egui::vec2(24.0, 24.0);
                    let btn_size = egui::vec2(30.0, 30.0);

                    let make_icon_btn = |ui: &mut egui::Ui, texture: &egui::TextureHandle, is_active: bool, tooltip: &str| -> bool {
                        let img = egui::Image::new(texture).fit_to_exact_size(icon_size).tint(ui.visuals().text_color());
                        let btn = egui::Button::image(img).selected(is_active);
                        ui.add_sized(btn_size, btn).on_hover_text(tooltip).clicked()
                    };

                    let make_svg_btn = |ui: &mut egui::Ui, source: egui::ImageSource<'static>, is_active: bool, tooltip: &str| -> bool {
                        let img = egui::Image::new(source).fit_to_exact_size(icon_size).tint(ui.visuals().text_color());
                        let btn = egui::Button::image(img).selected(is_active);
                        ui.add_sized(btn_size, btn).on_hover_text(tooltip).clicked()
                    };

                    if make_icon_btn(ui, &textures.whole, is_whole, "Whole Note (8)") {
                        clicked_action = Some(DurationAction::Base("whole"));
                    }
                    if make_icon_btn(ui, &textures.half, is_half, "Half Note (7)") {
                        clicked_action = Some(DurationAction::Base("half"));
                    }
                    if make_icon_btn(ui, &textures.quarter, is_quarter, "Quarter Note (6)") {
                        clicked_action = Some(DurationAction::Base("quarter"));
                    }
                    if make_icon_btn(ui, &textures.eighth, is_eighth, "Eighth Note (5)") {
                        clicked_action = Some(DurationAction::Base("eighth"));
                    }
                    if make_icon_btn(ui, &textures.sixteenth, is_16th, "16th Note (4)") {
                        clicked_action = Some(DurationAction::Base("16th"));
                    }
                    if make_icon_btn(ui, &textures.thirty_second, is_32nd, "32nd Note (3)") {
                        clicked_action = Some(DurationAction::Base("32nd"));
                    }

                    ui.separator();

                    if make_icon_btn(ui, &textures.dot, is_dotted, "Augmentation Dot (.)") {
                        clicked_action = Some(DurationAction::ToggleDot);
                    }

                    // Rest button (quarter rest glyph from Bravura)
                    let is_rest = self.is_selected_event_rest();
                    let (r_rect, r_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(r_rect) {
                        let visuals = ui.style().interact_selectable(&r_resp, is_rest);
                        ui.painter().rect(
                            r_rect,
                            visuals.corner_radius,
                            visuals.bg_fill,
                            visuals.bg_stroke,
                            egui::StrokeKind::Inside,
                        );
                        let color = visuals.text_color();
                        let font_id = egui::FontId::new(20.0, egui::FontFamily::Name("Bravura".into()));
                        ui.painter().text(
                            r_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "\u{E4E5}",
                            font_id,
                            color,
                        );
                    }
                    if r_resp.on_hover_text("Rest (R)").clicked() {
                        rest_clicked = true;
                    }

                    // Tuplet button (brackets and small 3)
                    let is_tuplet = self.is_selected_event_in_tuplet();
                    let (t_rect, t_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(t_rect) {
                        let visuals = ui.style().interact_selectable(&t_resp, is_tuplet);
                        ui.painter().rect(
                            t_rect,
                            visuals.corner_radius,
                            visuals.bg_fill,
                            visuals.bg_stroke,
                            egui::StrokeKind::Inside,
                        );
                        let color = visuals.text_color();

                        let center = t_rect.center();
                        ui.painter().text(
                            center,
                            egui::Align2::CENTER_CENTER,
                            "3",
                            egui::FontId::monospace(12.0),
                            color,
                        );

                        let bracket_y = center.y + 7.0;
                        let hook_len = 3.5;
                        let left_x = center.x - 9.0;
                        let left_inner_x = center.x - 2.0;
                        let right_inner_x = center.x + 2.0;
                        let right_x = center.x + 9.0;
                        let stroke = egui::Stroke::new(1.3, color);

                        ui.painter().line_segment([egui::pos2(left_x, bracket_y + hook_len), egui::pos2(left_x, bracket_y)], stroke);
                        ui.painter().line_segment([egui::pos2(left_x, bracket_y), egui::pos2(left_inner_x, bracket_y)], stroke);
                        ui.painter().line_segment([egui::pos2(right_inner_x, bracket_y), egui::pos2(right_x, bracket_y)], stroke);
                        ui.painter().line_segment([egui::pos2(right_x, bracket_y), egui::pos2(right_x, bracket_y + hook_len)], stroke);
                    }
                    if t_resp.on_hover_text("Tuplet").clicked() {
                        tuplet_clicked = true;
                    }


                    // Grace Notes button
                    let is_grace = if let (Some(e), Some(doc)) = (self.selected_event_idx, &self.parsed_mnx) {
                        doc.find_event_grace_info_for_part(self.selected_part_idx, e).is_some()
                    } else {
                        false
                    };
                    let (g_rect, g_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(g_rect) {
                        let visuals = ui.style().interact_selectable(&g_resp, is_grace);
                        ui.painter().rect(
                            g_rect,
                            visuals.corner_radius,
                            visuals.bg_fill,
                            visuals.bg_stroke,
                            egui::StrokeKind::Inside,
                        );
                        let color = visuals.text_color();
                        let font_id = egui::FontId::new(28.0, egui::FontFamily::Name("Bravura".into()));
                        ui.painter().text(
                            g_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "\u{E561}",
                            font_id,
                            color,
                        );
                    }
                    if g_resp.on_hover_text("Insert Grace Notes").clicked() {
                        grace_clicked = true;
                    }

                    // Tie button
                    let is_tie = self.is_selected_event_tied();
                    if make_svg_btn(ui, include_image!("../../assets/tie.svg"), is_tie, "Tie (Shift+T)") || (!has_text_focus && ui.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::T))) {
                        tie_clicked = true;
                    }

                }

                // ── Current Grid Dropdown ────────────────────────────────────
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
                ui.label(egui::RichText::new("Current Grid:").strong());
                ui.scope(|ui| {
                    ui.spacing_mut().interact_size.y = 30.0;
                    let combo_resp = egui::ComboBox::from_id_salt("current_grid_combo")
                        .width(42.0)
                        .selected_text("")
                        .show_ui(ui, |ui| {
                            ui.spacing_mut().interact_size.y = 18.0;
                            ui.selectable_value(
                                &mut self.current_grid,
                                "half".to_string(),
                                egui::RichText::new("\u{ECA3}")
                                    .family(egui::FontFamily::Name("BravuraText".into()))
                                    .size(20.0),
                            );
                            ui.selectable_value(
                                &mut self.current_grid,
                                "quarter".to_string(),
                                egui::RichText::new("\u{ECA5}")
                                    .family(egui::FontFamily::Name("BravuraText".into()))
                                    .size(20.0),
                            );
                            ui.selectable_value(
                                &mut self.current_grid,
                                "eighth".to_string(),
                                egui::RichText::new("\u{ECA7}")
                                    .family(egui::FontFamily::Name("BravuraText".into()))
                                    .size(20.0),
                            );
                            ui.selectable_value(
                                &mut self.current_grid,
                                "16th".to_string(),
                                egui::RichText::new("\u{ECA9}")
                                    .family(egui::FontFamily::Name("BravuraText".into()))
                                    .size(20.0),
                            );
                        });

                    let c_rect = combo_resp.response.rect;
                    if ui.is_rect_visible(c_rect) {
                        let visuals = ui.style().interact(&combo_resp.response);
                        let glyph = match self.current_grid.as_str() {
                            "half" => "\u{ECA3}",
                            "eighth" => "\u{ECA7}",
                            "16th" | "sixteenth" => "\u{ECA9}",
                            _ => "\u{ECA5}",
                        };
                        let font_id =
                            egui::FontId::new(20.0, egui::FontFamily::Name("BravuraText".into()));
                        ui.painter().text(
                            egui::pos2(c_rect.left() + 13.0, c_rect.center().y - 4.0),
                            egui::Align2::CENTER_CENTER,
                            glyph,
                            font_id,
                            visuals.text_color(),
                        );
                    }
                });

                // ── Insert TS Button ─────────────────────────────────────────
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                
                let btn_size = egui::vec2(30.0, 30.0);
                let (rect, mut ts_btn) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                let visuals = ui.style().interact(&ts_btn);
                
                if ui.is_rect_visible(rect) {
                    ui.painter().rect(
                        rect,
                        visuals.corner_radius,
                        visuals.bg_fill,
                        visuals.bg_stroke,
                        egui::StrokeKind::Inside,
                    );
                    
                    let font_id = egui::FontId::new(25.0, egui::FontFamily::Name("Bravura".into()));
                    let center = rect.center();
                    
                    // Draw numerals manually to fix the massive baseline offset in SMuFL fonts
                    ui.painter().text(
                        center - egui::vec2(0.0, 6.0), // push top number up
                        egui::Align2::CENTER_CENTER,
                        "\u{E083}",
                        font_id.clone(),
                        visuals.text_color(),
                    );
                    ui.painter().text(
                        center + egui::vec2(0.0, 5.0), // push bottom number down
                        egui::Align2::CENTER_CENTER,
                        "\u{E084}",
                        font_id,
                        visuals.text_color(),
                    );
                }
                
                ts_btn = ts_btn.on_hover_text("Insert Time Signature");
                
                if ts_btn.clicked() {
                    let mut m_idx = 0;
                    if let Some(doc) = &self.parsed_mnx {
                        if let Some(part) = doc.parts.get(self.selected_part_idx).or_else(|| doc.parts.first()) {
                            if let Some(e_idx) = self.selected_event_idx {
                                let mut count = 0;
                                'find_m: for (i, m) in part.measures.iter().enumerate() {
                                    for seq in &m.sequences {
                                        for _ in seq.flattened_events() {
                                            if count == e_idx {
                                                m_idx = i;
                                                break 'find_m;
                                            }
                                            count += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    self.active_dialog = ActiveDialog::InsertTimeSignature {
                        count: 4,
                        unit: 4,
                        m_idx,
                        warning: None,
                    };
                }

                // ── Tempo Input ───────────────────────────────────────────────
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                if ui.button("Insert tempo").clicked() {
                    let start_event_idx = self.selected_event_idx.unwrap_or(0);
                    let mut bpm_str = "120".to_string();
                    let mut duration_value = "quarter".to_string();
                    let mut dots = 0;
                    let mut existing_tempo = None;

                    if let Some((m_idx, t_idx)) = self.selected_tempo {
                        if let Some(doc) = &self.parsed_mnx {
                            if let Some(gm) = doc.global.measures.get(m_idx) {
                                if let Some(t) = gm.tempos.get(t_idx) {
                                    bpm_str = t.bpm.to_string();
                                    duration_value = t.value.base.clone();
                                    dots = t.value.dots.unwrap_or(0);
                                    existing_tempo = Some((m_idx, t_idx));
                                }
                            }
                        }
                    } else if let Some((m_idx, t_idx, tempo)) = self.find_tempo_at_event(start_event_idx) {
                        bpm_str = tempo.bpm.to_string();
                        duration_value = tempo.value.base.clone();
                        dots = tempo.value.dots.unwrap_or(0);
                        existing_tempo = Some((m_idx, t_idx));
                    }

                    self.active_dialog = ActiveDialog::InsertTempo {
                        bpm_str,
                        duration_value,
                        dots,
                        event_idx: start_event_idx,
                        existing_tempo,
                    };
                }

                if ui.button("rit. / accel.").clicked() {
                    let mut start_event_idx = if let (Some(anchor), Some(current)) = (self.selection_anchor, self.selected_event_idx) {
                        anchor.min(current)
                    } else {
                        self.selected_event_idx.unwrap_or(0)
                    };
                    
                    // calculate default duration beats based on selected range
                    let mut default_beats = 4.0;
                    if let (Some(_anchor), Some(_current)) = (self.selection_anchor, self.selected_event_idx) {
                        if let Some((items, ..)) = self.extract_selected_items() {
                            let mut total = 0.0;
                            for item in &items {
                                total += item.total_beats();
                            }
                            if total > 0.0 {
                                default_beats = total;
                            }
                        }
                    }

                    
                    let mut ce = crate::curve_editor::MonotonicCurveEditor::new(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 100.0)));
 // high to low by default
                    let mut existing_target = None;
                    let mut start_bpm = "120".to_string();
                    if let Some(doc) = &self.parsed_mnx {
                        let mut event_beats = Vec::new();
                        let mut cur_beat = 0.0;
                        if let Some(part) = doc.parts.get(self.selected_part_idx).or_else(|| doc.parts.first()) {
                            for m in &part.measures {
                                let mut measure_beats: f64 = 0.0;
                                for seq in &m.sequences {
                                    let mut seq_beat: f64 = 0.0;
                                    for item in &seq.content {
                                        event_beats.push(cur_beat + seq_beat);
                                        seq_beat += item.total_beats();
                                    }
                                    measure_beats = measure_beats.max(seq_beat);
                                }
                                cur_beat += measure_beats;
                            }
                        }
                        let cursor_beat = event_beats.get(start_event_idx).copied().unwrap_or(0.0);
                        
                        let parse_loc = |loc: &Option<String>| -> f64 {
                            if let Some(s) = loc {
                                if let Some((n, d)) = s.split_once('/') {
                                    if let (Ok(num), Ok(den)) = (n.parse::<f64>(), d.parse::<f64>()) {
                                        if den > 0.0 { return (num / den) * 4.0; }
                                    }
                                }
                            }
                            0.0
                        };

                        let mut measure_start_beats = Vec::new();
                        let mut cb = 0.0;
                        if let Some(part) = doc.parts.get(self.selected_part_idx).or_else(|| doc.parts.first()) {
                            for m in &part.measures {
                                measure_start_beats.push(cb);
                                let mut mb = 0.0;
                                for seq in &m.sequences {
                                    let mut sb = 0.0;
                                    for item in &seq.content {
                                        sb += item.total_beats();
                                    }
                                    mb = f64::max(mb, sb);
                                }
                                cb += mb;
                            }
                        }

                        'tc_search: for (m_idx, gm) in doc.global.measures.iter().enumerate() {
                            let m_start = measure_start_beats.get(m_idx).copied().unwrap_or(0.0);
                            for tc in &gm.tempo_changes {
                                let tc_beat = m_start + parse_loc(&tc.location);
                                if cursor_beat >= tc_beat - 0.001 {
                                    if let Some(tid) = &tc.target {
                                        for (m_idx2, gm2) in doc.global.measures.iter().enumerate() {
                                            let m_start2 = measure_start_beats.get(m_idx2).copied().unwrap_or(0.0);
                                            for t in &gm2.tempos {
                                                if t.id.as_ref() == Some(tid) {
                                                    let t_beat = m_start2 + t.location.as_ref().map(|p| p.to_quarter_beats()).unwrap_or(0.0);
                                                    if cursor_beat <= t_beat + 0.001 {
                                                        existing_target = Some(tid.clone());
                                                        start_bpm = t.bpm.to_string();
                                                        default_beats = t_beat - tc_beat;
                                                        if let Some(bend) = &tc.bend {
                                                            ce = crate::curve_editor::parse_bend_string(bend, egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 100.0)));
                                                        }
                                                        
                                                        let mut best_idx = 0;
                                                        let mut best_diff = f64::MAX;
                                                        for (idx, &eb) in event_beats.iter().enumerate() {
                                                            let diff = (eb - tc_beat).abs();
                                                            if diff < best_diff {
                                                                best_diff = diff;
                                                                best_idx = idx;
                                                            }
                                                        }
                                                        start_event_idx = best_idx;
                                                        
                                                        break 'tc_search;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    self.active_dialog = ActiveDialog::InsertTempoChange {
                        event_idx: start_event_idx,
                        ending_tempo: start_bpm,
                        duration_beats: default_beats.to_string(),
                        curve_editor: ce,
                        existing_target_id: existing_target,
                    };
                }

                if ui.button("Add Lyrics").on_hover_text("Add Lyrics (Shift+L)").clicked() {
                    let start_event_idx = self.selected_event_idx.unwrap_or(0);
                    self.open_lyric_dialog(start_event_idx, 1);
                }

                if ui.button("Insert Text").on_hover_text("Insert Text Marking on Duration").clicked() {
                    let target_event_idx = self.selected_event_idx.unwrap_or(0);
                    self.open_text_marking_dialog(target_event_idx);
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Technique:").strong());
                let btn_size = egui::vec2(30.0, 30.0);
                let icon_size = egui::vec2(24.0, 24.0);
                let make_svg_btn = |ui: &mut egui::Ui, source: egui::ImageSource<'static>, is_active: bool, tooltip: &str| -> bool {
                    let img = egui::Image::new(source).fit_to_exact_size(icon_size).tint(ui.visuals().text_color());
                    let btn = egui::Button::image(img).selected(is_active);
                    ui.add_sized(btn_size, btn).on_hover_text(tooltip).clicked()
                };
                    // Slur button
                    let is_slur = self.is_selected_note_slurred();
                    if make_svg_btn(ui, include_image!("../../assets/hammer_pulloff.svg"), is_slur, "Hammer / Pull-off (S)") || (!has_text_focus && ui.input(|i| i.key_pressed(egui::Key::S))) {
                        slur_clicked = true;
                    }

                    // Tap button
                    let is_tap = self.is_selected_note_tapped();
                    let (tap_rect, tap_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(tap_rect) {
                        let visuals = ui.style().interact_selectable(&tap_resp, is_tap);
                        ui.painter().rect(
                            tap_rect,
                            visuals.corner_radius,
                            visuals.bg_fill,
                            visuals.bg_stroke,
                            egui::StrokeKind::Inside,
                        );
                        let color = visuals.text_color();
                        ui.painter().text(
                            tap_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "T",
                            egui::FontId::proportional(16.0),
                            color,
                        );
                    }
                    if tap_resp.on_hover_text("Tap (T)").clicked() || (!has_text_focus && !ui.input(|i| i.modifiers.shift) && ui.input(|i| i.key_pressed(egui::Key::T))) {
                        tap_clicked = true;
                    }
                    
                    // Slide button
                    let is_slide = self.is_selected_note_sliding();
                    let (slide_rect, slide_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(slide_rect) {
                        let visuals = ui.style().interact_selectable(&slide_resp, is_slide);
                        ui.painter().rect(
                            slide_rect,
                            visuals.corner_radius,
                            visuals.bg_fill,
                            visuals.bg_stroke,
                            egui::StrokeKind::Inside,
                        );
                        let color = visuals.text_color();
                        let pad = 8.0;
                        ui.painter().line_segment(
                            [
                                slide_rect.left_bottom() + egui::vec2(pad, -pad),
                                slide_rect.right_top() + egui::vec2(-pad, pad),
                            ],
                            egui::Stroke::new(2.0, color),
                        );
                    }
                    if slide_resp.on_hover_text("Slide (L)").clicked() || (!has_text_focus && ui.input(|i| i.key_pressed(egui::Key::L))) {
                        slide_clicked = true;
                    }
                    
                    // Bend button
                    let is_bend = self.is_selected_note_bent();
                    if ui.add_sized(btn_size, egui::Button::new(egui::RichText::new("Bnd").size(11.0)).selected(is_bend)).on_hover_text("Bend (B)").clicked() || (!has_text_focus && ui.input(|i| i.key_pressed(egui::Key::B))) {
                        bend_clicked = true;
                    }

                    // Natural Harmonic button
                    let is_harmonic = self.is_selected_note_harmonic();
                    let nh_btn = egui::Button::new(egui::RichText::new("<N.H.>").size(10.5).strong())
                        .selected(is_harmonic);
                    if ui.add_sized(egui::vec2(44.0, 30.0), nh_btn).on_hover_text("Natural Harmonic (H)").clicked()
                        || (!has_text_focus && !ui.input(|i| i.modifiers.any()) && ui.input(|i| i.key_pressed(egui::Key::H)))
                    {
                        harmonic_clicked = true;
                    }
                    
                    // Strum Up button
                    // Accent button
                    let is_accent = self.is_selected_event_accented();
                    let (acc_rect, acc_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(acc_rect) {
                        let visuals = ui.style().interact_selectable(&acc_resp, is_accent);
                        ui.painter().rect(acc_rect, visuals.corner_radius, visuals.bg_fill, visuals.bg_stroke, egui::StrokeKind::Inside);
                        let color = visuals.text_color();
                        let font_id = egui::FontId::new(18.0, egui::FontFamily::Name("Bravura".into()));
                        ui.painter().text(acc_rect.center(), egui::Align2::CENTER_CENTER, "\u{E4A0}", font_id, color);
                    }
                    if acc_resp.on_hover_text("Accent").clicked() {
                        accent_clicked = true;
                    }
                    // Arpeggio Up button
                    let is_arpeggio_up = self.is_selected_event_arpeggiated("up");
                    let (arpu_rect, arpu_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(arpu_rect) {
                        let visuals = ui.style().interact_selectable(&arpu_resp, is_arpeggio_up);
                        ui.painter().rect(arpu_rect, visuals.corner_radius, visuals.bg_fill, visuals.bg_stroke, egui::StrokeKind::Inside);
                        let color = visuals.text_color();
                        let font_id = egui::FontId::new(18.0, egui::FontFamily::Name("Bravura".into()));
                        ui.painter().text(arpu_rect.center() + egui::vec2(0.0, 13.0), egui::Align2::CENTER_CENTER, "\u{E634}", font_id, color);
                    }
                    if arpu_resp.on_hover_text("Roll chord (low to high)").clicked() {
                        arpeggio_up_clicked = true;
                    }

                    // Arpeggio Down button
                    let is_arpeggio_down = self.is_selected_event_arpeggiated("down");
                    let (arpd_rect, arpd_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(arpd_rect) {
                        let visuals = ui.style().interact_selectable(&arpd_resp, is_arpeggio_down);
                        ui.painter().rect(arpd_rect, visuals.corner_radius, visuals.bg_fill, visuals.bg_stroke, egui::StrokeKind::Inside);
                        let color = visuals.text_color();
                        let font_id = egui::FontId::new(18.0, egui::FontFamily::Name("Bravura".into()));
                        ui.painter().text(arpd_rect.center() + egui::vec2(0.0, 13.0), egui::Align2::CENTER_CENTER, "\u{E635}", font_id, color);
                    }
                    if arpd_resp.on_hover_text("Roll chord (high to low)").clicked() {
                        arpeggio_down_clicked = true;
                    }

                    let is_strum_up = self.is_selected_event_strummed("up");
                    let (sup_rect, sup_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(sup_rect) {
                        let visuals = ui.style().interact_selectable(&sup_resp, is_strum_up);
                        ui.painter().rect(sup_rect, visuals.corner_radius, visuals.bg_fill, visuals.bg_stroke, egui::StrokeKind::Inside);
                        let color = visuals.text_color();
                        let font_id = egui::FontId::new(24.0, egui::FontFamily::Name("Bravura".into()));
                        ui.painter().text(sup_rect.center() + egui::vec2(0.0, 7.5), egui::Align2::CENTER_CENTER, "\u{EB70}", font_id, color);
                    }
                    if sup_resp.on_hover_text("Up Strum").clicked() {
                        strum_up_clicked = true;
                    }

                    // Strum Down button
                    let is_strum_down = self.is_selected_event_strummed("down");
                    let (sdown_rect, sdown_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(sdown_rect) {
                        let visuals = ui.style().interact_selectable(&sdown_resp, is_strum_down);
                        ui.painter().rect(sdown_rect, visuals.corner_radius, visuals.bg_fill, visuals.bg_stroke, egui::StrokeKind::Inside);
                        let color = visuals.text_color();
                        let font_id = egui::FontId::new(24.0, egui::FontFamily::Name("Bravura".into()));
                        ui.painter().text(sdown_rect.center() + egui::vec2(0.0, 7.5), egui::Align2::CENTER_CENTER, "\u{EB74}", font_id, color);
                    }
                    if sdown_resp.on_hover_text("Down Strum").clicked() {
                        strum_down_clicked = true;
                    }

                    ui.separator();
                    ui.label(egui::RichText::new("Barlines:").strong());

                    let bravura_font = egui::FontId::new(20.0, egui::FontFamily::Name("BravuraText".into()));
                    let m_idx = self.get_target_measure_for_layout_or_breaks();

                    let is_repeat_start = self.parsed_mnx.as_ref()
                        .and_then(|doc| doc.global.measures.get(m_idx))
                        .map(|gm| gm.repeat_start.is_some())
                        .unwrap_or(false);
                    let (rstart_rect, rstart_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(rstart_rect) {
                        let visuals = ui.style().interact_selectable(&rstart_resp, is_repeat_start);
                        ui.painter().rect(rstart_rect, visuals.corner_radius, visuals.bg_fill, visuals.bg_stroke, egui::StrokeKind::Inside);
                        let color = visuals.text_color();
                        ui.painter().text(rstart_rect.center(), egui::Align2::CENTER_CENTER, "\u{E040}", bravura_font.clone(), color);
                    }
                    if rstart_resp.on_hover_text("Start Repeat").clicked() {
                        let new_yaml = if is_repeat_start {
                            crate::yaml_editor::set_global_measure_property(&self.yaml_text, m_idx, "repeatStart", "")
                        } else {
                            crate::yaml_editor::set_global_measure_property(&self.yaml_text, m_idx, "repeatStart", "{}")
                        };
                        self.update_yaml(new_yaml);
                        self.process_and_render();
                    }

                    let is_repeat_end = self.parsed_mnx.as_ref()
                        .and_then(|doc| doc.global.measures.get(m_idx))
                        .map(|gm| gm.repeat_end.is_some())
                        .unwrap_or(false);
                    let (rend_rect, rend_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(rend_rect) {
                        let visuals = ui.style().interact_selectable(&rend_resp, is_repeat_end);
                        ui.painter().rect(rend_rect, visuals.corner_radius, visuals.bg_fill, visuals.bg_stroke, egui::StrokeKind::Inside);
                        let color = visuals.text_color();
                        ui.painter().text(rend_rect.center(), egui::Align2::CENTER_CENTER, "\u{E041}", bravura_font.clone(), color);
                    }
                    if rend_resp.on_hover_text("End Repeat").clicked() {
                        let new_yaml = if is_repeat_end {
                            crate::yaml_editor::set_global_measure_property(&self.yaml_text, m_idx, "repeatEnd", "")
                        } else {
                            crate::yaml_editor::set_global_measure_property(&self.yaml_text, m_idx, "repeatEnd", "{ times: 2 }")
                        };
                        self.update_yaml(new_yaml);
                        self.process_and_render();
                    }

                    let is_already_double = self.parsed_mnx.as_ref()
                        .and_then(|doc| doc.global.measures.get(m_idx))
                        .and_then(|gm| gm.barline.as_ref())
                        .map(|b| b.barline_type == "double")
                        .unwrap_or(false);
                    let (dbar_rect, dbar_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(dbar_rect) {
                        let visuals = ui.style().interact_selectable(&dbar_resp, is_already_double);
                        ui.painter().rect(dbar_rect, visuals.corner_radius, visuals.bg_fill, visuals.bg_stroke, egui::StrokeKind::Inside);
                        let color = visuals.text_color();
                        ui.painter().text(dbar_rect.center(), egui::Align2::CENTER_CENTER, "\u{E031}", bravura_font.clone(), color);
                    }
                    if dbar_resp.on_hover_text("Double Barline").clicked() {
                        let new_yaml = if is_already_double {
                            crate::yaml_editor::set_global_measure_property(&self.yaml_text, m_idx, "barline", "")
                        } else {
                            crate::yaml_editor::set_global_measure_property(
                                &self.yaml_text,
                                m_idx,
                                "barline",
                                "{ type: double }"
                            )
                        };
                        self.update_yaml(new_yaml);
                        self.process_and_render();
                    }

                    let is_already_final = self.parsed_mnx.as_ref()
                        .and_then(|doc| doc.global.measures.get(m_idx))
                        .and_then(|gm| gm.barline.as_ref())
                        .map(|b| b.barline_type == "final" || b.barline_type == "ending")
                        .unwrap_or(false);
                    let (fbar_rect, fbar_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(fbar_rect) {
                        let visuals = ui.style().interact_selectable(&fbar_resp, is_already_final);
                        ui.painter().rect(fbar_rect, visuals.corner_radius, visuals.bg_fill, visuals.bg_stroke, egui::StrokeKind::Inside);
                        let color = visuals.text_color();
                        ui.painter().text(fbar_rect.center(), egui::Align2::CENTER_CENTER, "\u{E032}", bravura_font.clone(), color);
                    }
                    if fbar_resp.on_hover_text("Ending Barline").clicked() {
                        let new_yaml = if is_already_final {
                            crate::yaml_editor::set_global_measure_property(&self.yaml_text, m_idx, "barline", "")
                        } else {
                            crate::yaml_editor::set_global_measure_property(
                                &self.yaml_text,
                                m_idx,
                                "barline",
                                "{ type: final }"
                            )
                        };
                        self.update_yaml(new_yaml);
                        self.process_and_render();
                    }

                    let (start_m, end_m) = self.get_selected_measure_range();
                    let (has_closed_ending, has_open_ending) = if let Some(doc) = &self.parsed_mnx {
                        if let Some(gm) = doc.global.measures.get(start_m) {
                            if let Some(ending) = &gm.ending {
                                if ending.open == Some(true) {
                                    (false, true)
                                } else {
                                    (true, false)
                                }
                            } else {
                                (false, false)
                            }
                        } else {
                            (false, false)
                        }
                    } else {
                        (false, false)
                    };

                    let ending_icon_size = egui::vec2(24.0, 11.56);

                    let img_closed = egui::Image::new(include_image!("../../assets/closed ending.svg"))
                        .fit_to_exact_size(ending_icon_size)
                        .tint(ui.visuals().text_color());
                    if ui
                        .add_sized(btn_size, egui::Button::image(img_closed).selected(has_closed_ending))
                        .on_hover_text("Insert 1st... Ending")
                        .clicked()
                    {
                        if has_closed_ending {
                            let yaml = crate::yaml_editor::set_global_measure_property(&self.yaml_text, start_m, "ending", "");
                            self.update_yaml(yaml);
                            self.process_and_render();
                        } else {
                            self.active_dialog = ActiveDialog::InsertEnding {
                                start_m,
                                end_m,
                                times_str: "1".to_string(),
                            };
                        }
                    }

                    let img_open = egui::Image::new(include_image!("../../assets/open ending.svg"))
                        .fit_to_exact_size(ending_icon_size)
                        .tint(ui.visuals().text_color());
                    if ui
                        .add_sized(btn_size, egui::Button::image(img_open).selected(has_open_ending))
                        .on_hover_text("Insert Open Ending")
                        .clicked()
                    {
                        if has_open_ending {
                            let yaml = crate::yaml_editor::set_global_measure_property(&self.yaml_text, start_m, "ending", "");
                            self.update_yaml(yaml);
                            self.process_and_render();
                        } else {
                            let prev_max = if let Some(doc) = &self.parsed_mnx {
                                doc.get_prior_ending_max(start_m)
                            } else {
                                None
                            }.unwrap_or(0);
                            let start_num = prev_max + 1;
                            let numbers = vec![start_num];
                            let duration = (end_m - start_m + 1) as u32;
                            let ending_val = format!("{{ duration: {}, numbers: {:?}, open: true }}", duration, numbers);
                            let yaml = crate::yaml_editor::set_global_measure_property(
                                &self.yaml_text,
                                start_m,
                                "ending",
                                &ending_val,
                            );
                            self.update_yaml(yaml);
                            self.process_and_render();
                        }
                    }

                    if self.view_mode != ViewMode::Continuous {
                        let target_m = self.get_target_measure_for_layout_or_breaks();
                        let (is_system_break, is_page_break) = if let Some(doc) = &self.parsed_mnx {
                            doc.global.measures.get(target_m).map(|gm| (gm.system_break.unwrap_or(false), gm.page_break.unwrap_or(false))).unwrap_or((false, false))
                        } else {
                            (false, false)
                        };

                        let img_sys = egui::Image::new(include_image!("../../assets/system_break.svg"))
                            .fit_to_exact_size(icon_size)
                            .tint(ui.visuals().text_color());
                        if ui.add_sized(btn_size, egui::Button::image(img_sys).selected(is_system_break)).on_hover_text("Toggle System Break").clicked() {
                            let target_m = self.get_target_measure_for_layout_or_breaks();
                            self.action_toggle_system_break(target_m);
                        }

                        let img_page = egui::Image::new(include_image!("../../assets/page_break.svg"))
                            .fit_to_exact_size(icon_size)
                            .tint(ui.visuals().text_color());
                        if ui.add_sized(btn_size, egui::Button::image(img_page).selected(is_page_break)).on_hover_text("Toggle Page Break").clicked() {
                            let target_m = self.get_target_measure_for_layout_or_breaks();
                            self.action_toggle_page_break(target_m);
                        }
                    }
            });
                if let Some(action) = clicked_action {
                    match action {
                        DurationAction::Base(b) => self.change_selected_duration_base(b),
                        DurationAction::ToggleDot => self.toggle_selected_duration_dot(),
                    }
                }

                if rest_clicked {
                    self.convert_selected_to_rest();
                }
                if tuplet_clicked {
                    self.handle_tuplet_button_click();
                }
                if grace_clicked {
                    self.action_toggle_grace();
                }
                if tie_clicked {
                    self.action_add_tie();
                }
                if slur_clicked {
                    self.action_add_slur();
                }
                if tap_clicked {
                    self.action_toggle_tap();
                }
                if slide_clicked {
                    self.action_toggle_slide();
                }
                if bend_clicked {
                    self.action_edit_bend();
                }
                if harmonic_clicked {
                    self.action_toggle_harmonic();
                }
                if accent_clicked {
                    self.action_toggle_accent();
                }
                if arpeggio_up_clicked {
                    self.action_toggle_arpeggio("up");
                }
                if arpeggio_down_clicked {
                    self.action_toggle_arpeggio("down");
                }
                if strum_up_clicked {
                    self.action_toggle_strum("up");
                }
                if strum_down_clicked {
                    self.action_toggle_strum("down");
                }


            ui.separator();

            // ── MIDI Settings & Playback Toolbar ───────────────────────────
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("MIDI:").strong());

                // 1. Play / Stop buttons
                let is_playing = self.midi_player.is_playing();
                if is_playing {
                    let stop_btn = egui::Button::new(
                        egui::RichText::new("⏹ Stop").color(Color32::from_rgb(255, 100, 100)).strong()
                    );
                    if ui.add(stop_btn).on_hover_text("Stop MIDI playback").clicked() {
                        self.midi_player.stop();
                        self.midi_status_msg = Some("Playback stopped.".to_string());
                    }
                } else {
                    let play_label = if let Some(sel) = self.selected_event_idx {
                        format!("▶ Play (from note {})", sel + 1)
                    } else {
                        "▶ Play".to_string()
                    };
                    let play_btn = egui::Button::new(
                        egui::RichText::new(play_label).color(Color32::from_rgb(100, 255, 130)).strong()
                    );
                    if ui.add(play_btn).on_hover_text("Play tablature via MIDI (Hotkey 'P' to play from selected note, 'Shift+P' from start, 'Ctrl+P' / 'Shift+Ctrl+P' for current part only)").clicked() {
                        self.start_midi_playback(self.selected_event_idx.is_some());
                    }
                }

                ui.separator();

                // Active Instrument Selector & Multi-Instrument Controls (#2a)
                let part_names: Vec<String> = self
                    .parsed_mnx
                    .as_ref()
                    .map(|d| {
                        d.parts
                            .iter()
                            .enumerate()
                            .map(|(i, p)| p.display_name(i))
                            .collect()
                    })
                    .unwrap_or_else(|| vec!["Instrument 1".to_string()]);
                let num_parts = part_names.len();
                let current_part_name = part_names
                    .get(self.selected_part_idx)
                    .cloned()
                    .unwrap_or_else(|| format!("Instrument {}", self.selected_part_idx + 1));

                let mut newly_selected_part = None;
                egui::ComboBox::from_id_salt("active_instrument_selector")
                    .selected_text(format!("🎸 {}", current_part_name))
                    .show_ui(ui, |ui| {
                        for (idx, name) in part_names.iter().enumerate() {
                            if ui
                                .selectable_label(self.selected_part_idx == idx, name)
                                .clicked()
                            {
                                newly_selected_part = Some(idx);
                            }
                        }
                    });
                if let Some(new_idx) = newly_selected_part {
                    self.select_part(new_idx);
                }

                if ui.button("Instrument Setup").on_hover_text("Configure active instrument tuning, EDO, and strings").clicked() {
                    self.open_instrument_setup_dialog(false);
                }

                if ui.button("+ Add Instrument").on_hover_text("Add a new instrument part to the score").clicked() {
                    self.open_add_instrument_dialog();
                }

                if num_parts > 1 {
                    if ui.button("Remove Instrument").on_hover_text("Remove the currently selected instrument part").clicked() {
                        self.active_dialog = ActiveDialog::ConfirmRemoveInstrument {
                            part_idx: self.selected_part_idx,
                        };
                    }
                }

                ui.separator();

                // 5. String-to-Channel Mappings button with Popup (Per-Instrument MIDI/String Channels)
                let sc_btn = egui::Button::new("MIDI/String Channels  ")
                    .selected(self.show_string_channels_popup);
                let sc_btn_resp = ui.add(sc_btn);
                paint_corner_triangle(ui, &sc_btn_resp);
                if sc_btn_resp.clicked() {
                    self.show_string_channels_popup = !self.show_string_channels_popup;
                    if self.show_string_channels_popup {
                        self.show_grid_controllers_popup = false;
                        self.string_channel_dialog_part_idx = self.selected_part_idx;
                    }
                }

                if self.show_string_channels_popup {
                    let sc_area_id = ui.make_persistent_id("string_channels_dropdown_area");
                    let sc_popup_pos = sc_btn_resp.rect.left_bottom() + egui::vec2(0.0, 3.0);
                    let mut close_sc_dropdown = false;

                    let sc_area_resp = egui::Area::new(sc_area_id)
                        .order(egui::Order::Foreground)
                        .fixed_pos(sc_popup_pos)
                        .show(ui.ctx(), |ui| {
                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                ui.set_min_width(320.0);
                                let mut dialog_p_idx = self
                                    .string_channel_dialog_part_idx
                                    .min(num_parts.saturating_sub(1));
                                self.string_channel_dialog_part_idx = dialog_p_idx;

                                let part_combo_id =
                                    ui.make_persistent_id("string_channels_part_combo");
                                let port_combo_id =
                                    ui.make_persistent_id(dialog_p_idx).with("string_channels_port_combo");
                                let was_any_sc_combo_open =
                                    egui::ComboBox::is_open(ui.ctx(), part_combo_id)
                                        || egui::ComboBox::is_open(ui.ctx(), port_combo_id);

                                if num_parts > 1 {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("Instrument:").strong());
                                        let sel_name = part_names
                                            .get(dialog_p_idx)
                                            .cloned()
                                            .unwrap_or_else(|| {
                                                format!("Instrument {}", dialog_p_idx + 1)
                                            });
                                        let mut newly_selected_p = None;
                                        egui::ComboBox::from_id_salt("string_channels_part_combo")
                                            .selected_text(sel_name)
                                            .show_ui(ui, |ui| {
                                                for (idx, name) in part_names.iter().enumerate() {
                                                    if ui
                                                        .selectable_label(dialog_p_idx == idx, name)
                                                        .clicked()
                                                    {
                                                        newly_selected_p = Some(idx);
                                                    }
                                                }
                                            });
                                        if let Some(new_p) = newly_selected_p {
                                            self.string_channel_dialog_part_idx = new_p;
                                            self.select_part(new_p);
                                            dialog_p_idx = new_p;
                                        }
                                    });
                                } else {
                                    let sel_name = part_names
                                        .first()
                                        .cloned()
                                        .unwrap_or_else(|| "Instrument 1".to_string());
                                    ui.label(egui::RichText::new(format!("Instrument: {}", sel_name)).strong());
                                }

                                ui.push_id(dialog_p_idx, |ui| {
                                    let current_port_id = self.get_part_midi_port(dialog_p_idx);

                                // 1. Per-instrument MIDI Out Device selector + refresh button
                                ui.horizontal(|ui| {
                                    ui.label("MIDI Out:");
                                    let selected_port_name = current_port_id
                                        .and_then(|id| {
                                            self.midi_ports.iter().find(|p| p.id == id).map(|p| p.name.clone())
                                        })
                                        .unwrap_or_else(|| {
                                            if self.midi_ports.is_empty() {
                                                "No MIDI Out Found".to_string()
                                            } else {
                                                "Select Device...".to_string()
                                            }
                                        });

                                    let mut newly_selected_port = None;
                                    egui::ComboBox::from_id_salt("string_channels_port_combo")
                                        .selected_text(selected_port_name)
                                        .width(160.0)
                                        .show_ui(ui, |ui| {
                                            if self.midi_ports.is_empty() {
                                                ui.label("No devices detected");
                                            } else {
                                                for port in &self.midi_ports {
                                                    let is_selected = current_port_id == Some(port.id);
                                                    if ui.selectable_label(is_selected, &port.name).clicked() {
                                                        newly_selected_port = Some(port.id);
                                                    }
                                                }
                                            }
                                        });
                                    if let Some(port_id) = newly_selected_port {
                                        self.set_part_midi_port(dialog_p_idx, port_id);
                                    }

                                    if ui.small_button("⟳").on_hover_text("Refresh MIDI devices").clicked() {
                                        self.midi_ports = crate::midi::get_midi_output_ports();
                                        if let Some(port_id) = current_port_id {
                                            if !self.midi_ports.iter().any(|p| p.id == port_id) {
                                                if let Some(first_p) = self.midi_ports.first() {
                                                    self.set_part_midi_port(dialog_p_idx, first_p.id);
                                                }
                                            }
                                        } else if let Some(first_p) = self.midi_ports.first() {
                                            self.set_part_midi_port(dialog_p_idx, first_p.id);
                                        }
                                    }
                                });

                                // 2. Per-instrument PB Range
                                ui.horizontal(|ui| {
                                    ui.label("PB Range: ±");
                                    let mut pb_range_val = self.get_part_pitch_bend_range(dialog_p_idx) as i32;
                                    if ui
                                        .add(egui::DragValue::new(&mut pb_range_val).range(1..=48).speed(0.2))
                                        .on_hover_text("Pitch bend range in semitones (1 - 48)")
                                        .changed()
                                    {
                                        let new_pb = pb_range_val.clamp(1, 48) as u8;
                                        self.set_part_pitch_bend_range(dialog_p_idx, new_pb);
                                    }
                                    ui.label("st");
                                });

                                // 3. Per-instrument Mode toggle (Multi-timbral vs MPE)
                                let is_mpe = self.is_part_mpe_mode(dialog_p_idx);
                                let mode_label = if is_mpe { "Mode: MPE" } else { "Mode: Multi-Timbral" };
                                if ui
                                    .selectable_label(is_mpe, mode_label)
                                    .on_hover_text(
                                        "Toggle between Multi-Timbral (channel per string) and MPE mode (Ch 1 Master, Ch 2..n+1 Member channels, default ±48 semitones)",
                                    )
                                    .clicked()
                                {
                                    self.set_part_mpe_mode(dialog_p_idx, !is_mpe);
                                }

                                // 4. Check if total strings sharing this port exceed 15 in MPE mode
                                if is_mpe {
                                    let target_port = self.get_part_midi_port(dialog_p_idx).unwrap_or(0);
                                    let total_strings_on_port: usize = self
                                        .parsed_mnx
                                        .as_ref()
                                        .map(|d| {
                                            d.parts
                                                .iter()
                                                .enumerate()
                                                .filter(|(pi, _)| {
                                                    self.get_part_midi_port(*pi).unwrap_or(0) == target_port
                                                })
                                                .map(|(_, p)| p.num_strings())
                                                .sum()
                                        })
                                        .unwrap_or(0);
                                    if total_strings_on_port > 15 {
                                        ui.colored_label(
                                            Color32::from_rgb(230, 160, 40),
                                            format!(
                                                "⚠ {} strings share this port (MPE supports max 15 member channels per port). Assign a second MIDI port above to avoid channel sharing.",
                                                total_strings_on_port
                                            ),
                                        );
                                    }
                                }

                                ui.separator();

                                // 5. String-to-Channel Mapping Table
                                let target_port = self.get_part_midi_port(dialog_p_idx).unwrap_or(0);
                                let prior_strings_on_port =
                                    self.compute_prior_strings_on_port(dialog_p_idx, target_port);
                                let num_strings = self
                                    .parsed_mnx
                                    .as_ref()
                                    .and_then(|d| d.parts.get(dialog_p_idx))
                                    .map(|p| p.num_strings())
                                    .unwrap_or(6) as u8;

                                if dialog_p_idx == self.selected_part_idx
                                    && self
                                        .part_string_channel_maps
                                        .get(&dialog_p_idx)
                                        .map(|m| m.is_empty())
                                        .unwrap_or(true)
                                    && !self.string_channel_map.is_empty()
                                {
                                    self.part_string_channel_maps
                                        .insert(dialog_p_idx, self.string_channel_map.clone());
                                }

                                ui.label(
                                    egui::RichText::new(if is_mpe {
                                        "String to MPE Member Channel Mapping (Ch 2-16, Ch 1 is Master)"
                                    } else {
                                        "String to MIDI Channel Mapping (Ch 1-16)"
                                    })
                                    .strong(),
                                );
                                ui.separator();
                                let mut map_changed = false;
                                let part_map = self
                                    .part_string_channel_maps
                                    .entry(dialog_p_idx)
                                    .or_default();
                                for s in 1..=num_strings {
                                    ui.horizontal(|ui| {
                                        ui.label(format!("String {}:", s));
                                        let default_ch = if is_mpe {
                                            2 + ((prior_strings_on_port + s - 1) % 15)
                                        } else {
                                            1 + ((prior_strings_on_port + s - 1) % 16)
                                        };
                                        let current_ch = part_map.entry(s).or_insert(default_ch);
                                        let mut ch_val = *current_ch as i32;
                                        let (min_ch, max_ch) =
                                            if is_mpe { (2, 16) } else { (1, 16) };
                                        if ui
                                            .add(
                                                egui::DragValue::new(&mut ch_val)
                                                    .range(min_ch..=max_ch)
                                                    .speed(0.1),
                                            )
                                            .changed()
                                        {
                                            *current_ch = ch_val.clamp(min_ch, max_ch) as u8;
                                            map_changed = true;
                                        }
                                        ui.label(format!("(Ch {})", ch_val));
                                    });
                                }
                                if ui
                                    .button(if is_mpe {
                                        "Reset to MPE Default"
                                    } else {
                                        "Reset to Default"
                                    })
                                    .clicked()
                                {
                                    if let Some(m) =
                                        self.part_string_channel_maps.get_mut(&dialog_p_idx)
                                    {
                                        for s in 1..=num_strings {
                                            let def_ch = if is_mpe {
                                                2 + ((prior_strings_on_port + s - 1) % 15)
                                            } else {
                                                1 + ((prior_strings_on_port + s - 1) % 16)
                                            };
                                            m.insert(s, def_ch);
                                        }
                                    }
                                    map_changed = true;
                                }
                                if (map_changed || dialog_p_idx == self.selected_part_idx)
                                    && dialog_p_idx == self.selected_part_idx
                                {
                                    if let Some(m) =
                                        self.part_string_channel_maps.get(&dialog_p_idx)
                                    {
                                        self.string_channel_map = m.clone();
                                    }
                                }
                                });

                                let is_any_sc_combo_open =
                                    egui::ComboBox::is_open(ui.ctx(), part_combo_id)
                                        || egui::ComboBox::is_open(ui.ctx(), port_combo_id);
                                was_any_sc_combo_open || is_any_sc_combo_open
                            })
                            .inner
                        });

                    if ui.input(|i| i.pointer.any_click()) {
                        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                            let in_btn = sc_btn_resp.rect.contains(pos);
                            let in_dropdown = sc_area_resp.response.rect.contains(pos);
                            let any_combo_open = sc_area_resp.inner;
                            if !in_btn && !in_dropdown && !any_combo_open {
                                close_sc_dropdown = true;
                            }
                        }
                    }

                    if close_sc_dropdown {
                        self.show_string_channels_popup = false;
                    }
                }

                ui.separator();

                // 6. Grid Controllers Select UI/UX & Quick On-The-Fly Fret Offset Controls
                let btn = egui::Button::new("Grid Controllers  ")
                    .selected(self.show_grid_controllers_popup);
                let btn_resp = ui.add(btn);
                paint_corner_triangle(ui, &btn_resp);
                if btn_resp.clicked() {
                    self.show_grid_controllers_popup = !self.show_grid_controllers_popup;
                    if self.show_grid_controllers_popup {
                        self.show_string_channels_popup = false;
                    }
                }

                if self.show_grid_controllers_popup {
                    let area_id = ui.make_persistent_id("grid_controllers_dropdown_area");
                    let popup_pos = btn_resp.rect.left_bottom() + egui::vec2(0.0, 3.0);
                    let mut close_dropdown = false;

                    let area_resp = egui::Area::new(area_id)
                        .order(egui::Order::Foreground)
                        .fixed_pos(popup_pos)
                        .show(ui.ctx(), |ui| {
                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                let was_any_grid_combo_open =
                                    (0..crate::grid_input::MAX_GRID_CONTROLLERS).any(|s| {
                                        let cid =
                                            ui.make_persistent_id(format!("toolbar_grid_port_{}", s));
                                        egui::ComboBox::is_open(ui.ctx(), cid)
                                    });
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new("Grid Controller MIDI Inputs (up to 4)")
                                            .strong(),
                                    );
                                    if ui.small_button("🔄 Refresh Ports").clicked() {
                                        self.midi_input_ports =
                                            crate::grid_input::get_midi_input_ports();
                                    }
                                });
                                ui.separator();

                                let mut row_width: f32 = 0.0;
                                let ports_snapshot = self.midi_input_ports.clone();
                                let mut slot_port_change: Option<(usize, Option<usize>)> = None;
                                for slot_idx in 0..crate::grid_input::MAX_GRID_CONTROLLERS {
                                    let sel_port_id = self.grid_input.selected_port_ids[slot_idx];
                                    let row_resp = ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!("Grid {}:", slot_idx + 1))
                                                .strong(),
                                        );
                                        let current_label = sel_port_id
                                            .and_then(|id| ports_snapshot.iter().find(|p| p.id == id))
                                            .map(|p| p.name.clone())
                                            .unwrap_or_else(|| "Disconnected".to_string());

                                        egui::ComboBox::from_id_salt(format!(
                                            "toolbar_grid_port_{}",
                                            slot_idx
                                        ))
                                        .selected_text(current_label)
                                        .width(170.0)
                                        .show_ui(ui, |ui| {
                                            if ui
                                                .selectable_label(sel_port_id.is_none(), "Disconnected")
                                                .clicked()
                                            {
                                                slot_port_change = Some((slot_idx, None));
                                            }
                                            for port in &ports_snapshot {
                                                if ui
                                                    .selectable_label(
                                                        sel_port_id == Some(port.id),
                                                        &port.name,
                                                    )
                                                    .clicked()
                                                {
                                                    slot_port_change =
                                                        Some((slot_idx, Some(port.id)));
                                                }
                                            }
                                        });

                                        ui.label("Fret Offset:");
                                        let slot_cfg = &mut self.grid_input.slots[slot_idx];
                                        if ui.small_button("-").clicked() {
                                            slot_cfg.fret_offset =
                                                (slot_cfg.fret_offset - 1).max(0);
                                        }
                                        ui.add(
                                            egui::DragValue::new(&mut slot_cfg.fret_offset)
                                                .range(0..=48)
                                                .speed(0.2),
                                        );
                                        if ui.small_button("+").clicked() {
                                            slot_cfg.fret_offset =
                                                (slot_cfg.fret_offset + 1).min(48);
                                        }
                                    });
                                    row_width = row_width.max(row_resp.response.rect.width());
                                }
                                if let Some((s_idx, pid)) = slot_port_change {
                                    if let Err(err) = self.grid_input.connect_slot(s_idx, pid) {
                                        self.midi_status_msg = Some(err);
                                    }
                                }

                                ui.separator();

                                // 4 equally divided calibration rectangles matching exact content width
                                ui.horizontal(|ui| {
                                    let total_width = row_width;
                                    let num_slots = crate::grid_input::MAX_GRID_CONTROLLERS as f32;
                                    let spacing = ui.spacing().item_spacing.x;
                                    let btn_width = ((total_width - (num_slots - 1.0) * spacing)
                                        / num_slots)
                                        .floor()
                                        .max(40.0);
                                    let btn_size = egui::vec2(btn_width, 42.0);

                                    for slot_idx in 0..crate::grid_input::MAX_GRID_CONTROLLERS {
                                        let is_slot_calibrating = self
                                            .grid_input
                                            .active_calibration
                                            .map(|(s, _, _)| s == slot_idx)
                                            .unwrap_or(false);

                                        let mut btn = egui::Button::new(
                                            egui::RichText::new(format!(
                                                "Calibrate\nGrid {}",
                                                slot_idx + 1
                                            ))
                                            .size(11.0)
                                            .strong(),
                                        )
                                        .min_size(btn_size);

                                        if is_slot_calibrating {
                                            btn = btn
                                                .stroke(egui::Stroke::new(
                                                    2.0,
                                                    egui::Color32::from_rgb(255, 215, 0),
                                                ))
                                                .fill(egui::Color32::from_rgb(50, 45, 10));
                                        }

                                        if ui.add(btn).clicked() {
                                            if is_slot_calibrating {
                                                self.grid_input.cancel_calibration();
                                            } else {
                                                self.grid_input.start_calibration(slot_idx);
                                            }
                                        }
                                    }
                                });

                                // Active step instructions or idle guide text
                                ui.add_space(4.0);
                                if let Some((cal_slot, step, _)) =
                                    self.grid_input.active_calibration
                                {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "Grid {}: {}",
                                                cal_slot + 1,
                                                step.prompt_text()
                                            ))
                                            .strong()
                                            .color(egui::Color32::from_rgb(255, 215, 0)),
                                        );
                                        if ui.button("Cancel Calibration").clicked() {
                                            self.grid_input.cancel_calibration();
                                        }
                                    });
                                } else {
                                    ui.label(
                                        egui::RichText::new(
                                            "Click a rectangle above to calibrate the 4 corners for that controller.",
                                        )
                                        .weak()
                                        .size(11.5),
                                    );
                                }

                                let is_any_grid_combo_open =
                                    (0..crate::grid_input::MAX_GRID_CONTROLLERS).any(|s| {
                                        let cid =
                                            ui.make_persistent_id(format!("toolbar_grid_port_{}", s));
                                        egui::ComboBox::is_open(ui.ctx(), cid)
                                    });
                                was_any_grid_combo_open || is_any_grid_combo_open
                            })
                            .inner
                        });

                    // Check for click outside to close the dropdown cleanly
                    if ui.input(|i| i.pointer.any_click()) {
                        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                            let in_btn = btn_resp.rect.contains(pos);
                            let in_dropdown = area_resp.response.rect.contains(pos);
                            let any_combo_open = area_resp.inner;

                            if !in_btn
                                && !in_dropdown
                                && !any_combo_open
                                && self.grid_input.active_calibration.is_none()
                            {
                                close_dropdown = true;
                            }
                        }
                    }

                    if close_dropdown {
                        self.show_grid_controllers_popup = false;
                    }
                }

                // Inline quick on-the-fly Fret Offset controls for connected grid controllers
                for slot_idx in 0..crate::grid_input::MAX_GRID_CONTROLLERS {
                    if self.grid_input.selected_port_ids[slot_idx].is_some()
                        || self.grid_input.slots[slot_idx].enabled
                    {
                        let slot_cfg = &mut self.grid_input.slots[slot_idx];
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("G{} Offset:", slot_idx + 1))
                                    .size(11.5)
                                    .color(Color32::from_rgb(255, 160, 160)),
                            );
                            if ui
                                .small_button("-")
                                .on_hover_text("Decrease leftmost column fret offset")
                                .clicked()
                            {
                                slot_cfg.fret_offset = (slot_cfg.fret_offset - 1).max(0);
                            }
                            ui.add(
                                egui::DragValue::new(&mut slot_cfg.fret_offset)
                                    .range(0..=48)
                                    .speed(0.2),
                            )
                            .on_hover_text(format!(
                                "Grid {} leftmost column fret number (quick on-the-fly adjustment)",
                                slot_idx + 1
                            ));
                            if ui
                                .small_button("+")
                                .on_hover_text("Increase leftmost column fret offset")
                                .clicked()
                            {
                                slot_cfg.fret_offset = (slot_cfg.fret_offset + 1).min(48);
                            }
                        });
                    }
                }

                ui.horizontal(|ui| {
                    let can_just_play = self.grid_input.has_connected_and_calibrated_controller();
                    let just_play_btn = if self.grid_input.just_play_mode {
                        egui::Button::new(
                            egui::RichText::new("Just Play")
                                .color(Color32::BLACK)
                                .strong(),
                        )
                        .fill(Color32::from_rgb(255, 215, 0))
                    } else {
                        egui::Button::new("Just Play")
                    };
                    let resp = ui.add_enabled(can_just_play, just_play_btn);
                    if can_just_play {
                        if resp
                            .on_hover_text(
                                "Toggle Just Play mode: forward grid controller MIDI through internal string/fret pitches to MIDI output without tab interception",
                            )
                            .clicked()
                        {
                            let new_state = !self.grid_input.just_play_mode;
                            let note_offs = self.grid_input.set_just_play_mode(new_state);
                            if !note_offs.is_empty() {
                                self.handle_grid_live_play_events(note_offs);
                            }
                        }
                    } else {
                        resp.on_hover_text(
                            "Connect and calibrate at least one grid controller to use Just Play",
                        );
                    }
                    if self.grid_input.just_play_mode {
                        paint_status_dot(ui, Color32::from_rgb(255, 215, 0));
                        ui.label(
                            egui::RichText::new("Active (Bypassing tab)")
                                .size(11.0)
                                .color(Color32::from_rgb(255, 215, 0)),
                        );
                    }
                });

                if self.grid_input.is_grid_input_mode {
                    ui.separator();
                    paint_status_dot(ui, Color32::from_rgb(255, 110, 110));
                    if ui
                        .button(
                            egui::RichText::new("GRID INPUT MODE (Esc to exit)")
                                .strong()
                                .color(Color32::from_rgb(255, 110, 110)),
                        )
                        .on_hover_text(
                            "Hold pads to preview chord, release all pads for 500ms to finalize & advance. Press Esc or click to exit.",
                        )
                        .clicked()
                    {
                        self.grid_input.exit_grid_input_mode();
                    }
                }

                // Status message tooltip / indicator
                if let Some(msg) = &self.midi_status_msg {
                    ui.separator();
                    ui.label(egui::RichText::new(msg).italics().color(Color32::from_rgb(180, 210, 255)));
                }
            });
        });
    }

    pub(crate) fn render_error_panel(&mut self, ui: &mut egui::Ui) {
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
    }

    pub(crate) fn render_yaml_panel(
        &mut self,
        ui: &mut egui::Ui,
        text_edit_id: egui::Id,
        event_ranges: &[EventRange],
    ) {
        egui::Panel::right("yaml_panel")
            .resizable(true)
            .min_size(350.0)
            .show(ui, |ui| {
                ui.heading("MNX Source (YAML)");
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let selected_range = self.selected_event_idx.and_then(|e_idx| {
                        event_ranges.get(e_idx).and_then(|er| {
                            if let Some(n_idx) = self.selected_note_idx_in_event {
                                er.note_ranges
                                    .get(n_idx)
                                    .cloned()
                                    .or_else(|| Some(er.total_range.clone()))
                            } else {
                                Some(er.total_range.clone())
                            }
                        })
                    });

                    let is_editing_now = self.is_editing_fret;
                    let is_grid_mode_now = self.grid_input.is_grid_input_mode;

                    let mut layouter =
                        |ui: &egui::Ui, text_buf: &dyn egui::TextBuffer, wrap_width: f32| {
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
                                    let bg_hl = if is_editing_now {
                                        Color32::from_rgba_unmultiplied(220, 140, 40, 140) // Orange highlight
                                    } else if is_grid_mode_now {
                                        Color32::from_rgba_unmultiplied(245, 95, 95, 140) // Light-red Grid Input Mode highlight
                                    } else {
                                        Color32::from_rgba_unmultiplied(60, 130, 210, 120) // Blue highlight
                                    };
                                    job.append(
                                        &text[sel_start..sel_end],
                                        0.0,
                                        egui::TextFormat {
                                            font_id: font_id.clone(),
                                            color: Color32::from_rgb(255, 255, 255),
                                            background: bg_hl,
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
    }

    pub(crate) fn render_central_panel(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            match self.view_mode {
                ViewMode::Continuous => {
                    egui::ScrollArea::both().show(ui, |ui| {
                        let num_parts = self
                            .parsed_mnx
                            .as_ref()
                            .map(|d| d.parts.len())
                            .unwrap_or(0);
                        if num_parts == 0 {
                            if self.parsed_mnx.is_some() {
                                ui.label("No parts found in document.");
                            } else {
                                ui.label("Could not parse document.");
                            }
                            return;
                        }

                        // Precompute synchronized prefix width and measure content widths across all stacked parts (#2b)
                        let (min_prefix_width, shared_measure_widths) = if let Some(doc) = &self.parsed_mnx {
                            let mut max_prefix = 0.0_f32;
                            let mut max_measures = 0usize;
                            let mut per_part_widths = Vec::with_capacity(doc.parts.len());

                            for (p_idx, part) in doc.parts.iter().enumerate() {
                                let pw = crate::render::compute_part_max_prefix_width(ui, part);
                                if pw > max_prefix {
                                    max_prefix = pw;
                                }
                                let is_active_part = p_idx == self.selected_part_idx;
                                let is_editing_this_part = is_active_part && self.is_editing_fret;
                                let active_ev = if is_editing_this_part {
                                    self.selected_event_idx
                                } else {
                                    None
                                };
                                let editing_text = if is_editing_this_part {
                                    self.editing_fret_buf.as_str()
                                } else {
                                    ""
                                };
                                let mw = crate::render::compute_part_measure_content_widths(
                                    part,
                                    is_editing_this_part,
                                    active_ev,
                                    editing_text,
                                );
                                if mw.len() > max_measures {
                                    max_measures = mw.len();
                                }
                                per_part_widths.push(mw);
                            }

                            let mut shared = vec![0.0_f32; max_measures];
                            for mw in &per_part_widths {
                                for (m_i, &w) in mw.iter().enumerate() {
                                    if w > shared[m_i] {
                                        shared[m_i] = w;
                                    }
                                }
                            }
                            (max_prefix, shared)
                        } else {
                            (0.0, Vec::new())
                        };

                        let is_playing = self.midi_player.is_playing();
                        let mut pending_lyric_click: Option<(usize, usize, usize)> = None;
                        let mut pending_add_bar = false;
                        let mut pending_event_click: Option<(usize, usize, Option<usize>)> = None;
                        let mut pending_tempo_click: Option<(usize, usize)> = None;
                        let mut pending_tempo_double_click: Option<(usize, usize)> = None;

                        if let Some(doc) = &self.parsed_mnx {
                            for (p_idx, part) in doc.parts.iter().enumerate() {
                                let is_active_part = p_idx == self.selected_part_idx;
                                if num_parts > 1 {
                                    ui.horizontal(|ui| {
                                        let label_text = part.display_name(p_idx);
                                        let rich = if is_active_part {
                                            egui::RichText::new(format!("▶ {}", label_text))
                                                .strong()
                                                .color(Color32::from_rgb(80, 170, 255))
                                        } else {
                                            egui::RichText::new(format!("   {}", label_text))
                                                .color(Color32::from_gray(160))
                                        };
                                        if ui.selectable_label(is_active_part, rich).clicked() && !is_active_part {
                                            pending_event_click = Some((p_idx, 0, None));
                                        }
                                    });
                                }

                                let part_selected_event = if is_playing {
                                    self.midi_player.current_event_idx_for_part(p_idx)
                                } else if is_active_part {
                                    self.selected_event_idx
                                } else {
                                    None
                                };
                                let part_pb = self.get_part_pitch_bend_range(p_idx);

                                let (_resp, clicked, add_bar_clicked, double_clicked_lyric, clicked_tempo, double_clicked_tempo) = render::draw_tablature(
                                    ui,
                                    crate::render::TablatureOptions {
                                        part,
                                        part_idx: p_idx,
                                        global: Some(&doc.global),
                                        errors: &self.parsed_errors,
                                        selected_event: part_selected_event,
                                        selection_anchor: if is_active_part && !is_playing {
                                            self.selection_anchor
                                        } else {
                                            None
                                        },
                                        selected_note_in_event: if is_active_part && !is_playing {
                                            self.selected_note_idx_in_event
                                        } else {
                                            None
                                        },
                                        is_editing_fret: is_active_part && self.is_editing_fret,
                                        is_grid_input_mode: is_active_part && self.grid_input.is_grid_input_mode,
                                        editing_fret_text: if is_active_part {
                                            &self.editing_fret_buf
                                        } else {
                                            ""
                                        },
                                        editing_string: if is_active_part {
                                            self.editing_string
                                        } else {
                                            None
                                        },
                                        is_editing_lyric: is_active_part && self.is_editing_lyric,
                                        editing_lyric_text: if is_active_part {
                                            &self.editing_lyric_buf
                                        } else {
                                            ""
                                        },
                                        scroll_to_selected: is_active_part && self.needs_scroll,
                                        pitch_bend_range: part_pb,
                                        min_prefix_width: Some(min_prefix_width),
                                        target_measure_widths: Some(&shared_measure_widths),
                                        show_global_headers: p_idx == 0,
                                        selected_tempo: if is_active_part { self.selected_tempo } else { None },
                                    },
                                );

                                if let Some((ev_idx, line_idx)) = double_clicked_lyric {
                                    pending_lyric_click = Some((p_idx, ev_idx, line_idx));
                                } else if double_clicked_tempo.is_some() {
                                    pending_tempo_double_click = double_clicked_tempo;
                                } else if clicked_tempo.is_some() {
                                    pending_tempo_click = clicked_tempo;
                                } else if add_bar_clicked {
                                    pending_add_bar = true;
                                } else if let Some((clicked_event, clicked_note)) = clicked {
                                    pending_event_click = Some((p_idx, clicked_event, clicked_note));
                                }
                            }
                        }

                        self.needs_scroll = false;

                        if let Some((m_idx, t_idx)) = pending_tempo_double_click {
                            let mut bpm_str = "120".to_string();
                            let mut duration_value = "quarter".to_string();
                            let mut dots = 0;
                            if let Some(doc) = &self.parsed_mnx {
                                if let Some(gm) = doc.global.measures.get(m_idx) {
                                    if let Some(t) = gm.tempos.get(t_idx) {
                                        bpm_str = t.bpm.to_string();
                                        duration_value = t.value.base.clone();
                                        dots = t.value.dots.unwrap_or(0);
                                    }
                                }
                            }
                            self.selected_tempo = Some((m_idx, t_idx));
                            self.active_dialog = ActiveDialog::InsertTempo {
                                bpm_str,
                                duration_value,
                                dots,
                                event_idx: self.selected_event_idx.unwrap_or(0),
                                existing_tempo: Some((m_idx, t_idx)),
                            };
                        } else if let Some((m_idx, t_idx)) = pending_tempo_click {
                            self.selected_tempo = Some((m_idx, t_idx));
                        } else if let Some((p_idx, ev_idx, line_idx)) = pending_lyric_click {
                            self.selected_tempo = None;
                            if p_idx != self.selected_part_idx {
                                self.select_part(p_idx);
                            }
                            self.selected_event_idx = Some(ev_idx);
                            self.selected_note_idx_in_event = None;
                            self.open_lyric_dialog(ev_idx, line_idx);
                        } else if pending_add_bar {
                            self.selected_tempo = None;
                            self.add_measure_to_end();
                        } else if let Some((p_idx, mut clicked_event, clicked_note)) = pending_event_click {
                            self.selected_tempo = None;
                            self.needs_scroll = false;
                            let old_part_idx = self.selected_part_idx;
                            let old_event_idx = self.selected_event_idx;
                            let switching_part = p_idx != old_part_idx;

                            if let Some(old_e_idx) = old_event_idx {
                                if switching_part || clicked_event != old_e_idx {
                                    if let Some(info) = self.find_event_measure_end_info(old_e_idx) {
                                        if info.is_last_in_measure && info.is_blank {
                                            let m_idx = info.measure_idx;
                                            let old_total = self.cached_event_ranges.len();
                                            let removed = self.mutate_doc(|doc| {
                                                if remove_blank_event_at_end_of_measure_in_doc_part(
                                                    doc,
                                                    old_part_idx,
                                                    m_idx,
                                                ) {
                                                    Some(())
                                                } else {
                                                    None
                                                }
                                            });
                                            let new_total = self.cached_event_ranges.len();
                                            if removed.is_some()
                                                && !switching_part
                                                && new_total < old_total
                                                && clicked_event > old_e_idx
                                            {
                                                clicked_event = clicked_event.saturating_sub(1);
                                            }
                                        }
                                    }
                                }
                            }

                            if switching_part {
                                self.select_part(p_idx);
                            }

                            let shift_down = ui.input(|i| i.modifiers.shift);
                            if shift_down && !switching_part {
                                if self.selection_anchor.is_none() {
                                    self.selection_anchor = self.selected_event_idx;
                                }
                            } else {
                                self.selection_anchor = None;
                            }
                            if self.grid_input.is_grid_input_mode {
                                self.grid_input.release_latch_start = None;
                                self.grid_input.buffered_chord.clear();
                            }
                            self.selected_event_idx = Some(clicked_event);
                            self.selected_note_idx_in_event = clicked_note;
                            self.is_editing_fret = false;
                            self.editing_fret_buf.clear();
                            self.editing_string = None;

                            if clicked_note.is_some() {
                                self.preview_selected_sound();
                            }
                        }
                    });
                }
                ViewMode::Layout => {
                    self.render_layout_panel(ui);
                }
            }
        });
    }

    pub(crate) fn render_layout_panel(&mut self, ui: &mut egui::Ui) {
        let active_parts = self.active_layout_part_indices();
        let (pages, offsets, part_names) = if let Some(doc) = &self.parsed_mnx {
            let p = crate::layout::compute_score_layout_for_parts(doc, 0.0, &active_parts);
            let offs = doc.layout.as_ref().map(|l| l.offsets.clone()).unwrap_or_default();
            let names: Vec<String> = doc
                .parts
                .iter()
                .enumerate()
                .map(|(i, pt)| pt.display_name(i))
                .collect();
            (p, offs, names)
        } else {
            ui.label("No document loaded.");
            return;
        };

        if pages.is_empty() {
            ui.label("No pages in score layout.");
            return;
        }

        let mouse_pos = ui.input(|i| i.pointer.hover_pos());
        let mouse_clicked = ui.input(|i| i.pointer.primary_clicked());
        let drag_delta = ui.input(|i| i.pointer.delta());

        let mut arrow_nudge = egui::vec2(0.0, 0.0);
        if ui.input(|i| i.key_pressed(egui::Key::ArrowLeft)) { arrow_nudge.x -= 2.0; }
        if ui.input(|i| i.key_pressed(egui::Key::ArrowRight)) { arrow_nudge.x += 2.0; }
        if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) { arrow_nudge.y -= 2.0; }
        if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) { arrow_nudge.y += 2.0; }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.selected_layout_element_id = None;
            self.selected_layout_element_ids.clear();
            self.selected_layout_measures.clear();
        }

        let delete_pressed = ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));
        if delete_pressed {
            if let Some(sel_id) = &self.selected_layout_element_id {
                if sel_id.starts_with("sys_lock_") {
                    let parts: Vec<&str> = sel_id.split('_').collect();
                    if parts.len() >= 4 {
                        if let (Ok(s), Ok(e)) = (parts[2].parse::<usize>(), parts[3].parse::<usize>()) {
                            self.action_unlock_system(s, e);
                        }
                    }
                } else if sel_id.starts_with("tempo_") {
                    let parts: Vec<&str> = sel_id.split('_').collect();
                    if parts.len() >= 3 {
                        if let (Ok(m_idx), Ok(t_idx)) = (parts[1].parse::<usize>(), parts[2].parse::<usize>()) {
                            self.action_remove_tempo(m_idx, t_idx);
                            self.selected_layout_element_id = None;
                            self.selected_layout_element_ids.clear();
                        }
                    }
                } else {
                    let ids: Vec<String> = self.selected_layout_element_ids.iter().cloned().collect();
                    for id in ids {
                        self.action_set_element_offset(&id, 0.0, 0.0);
                    }
                }
            }
        }

        // Layout Mode Toolbar & Ensemble / Part Tabs (#2c)
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Layout Mode").strong());
            ui.separator();

            // View Tabs: Ensemble Score vs Individual Part Tabs
            if ui
                .selectable_label(
                    self.layout_view_tab == LayoutViewTab::Ensemble,
                    "🎼 Ensemble Score",
                )
                .clicked()
            {
                self.layout_view_tab = LayoutViewTab::Ensemble;
            }
            for (idx, name) in part_names.iter().enumerate() {
                if ui
                    .selectable_label(
                        self.layout_view_tab == LayoutViewTab::Part(idx),
                        format!("🎸 {}", name),
                    )
                    .clicked()
                {
                    self.layout_view_tab = LayoutViewTab::Part(idx);
                    self.select_part(idx);
                }
            }

            if self.layout_view_tab == LayoutViewTab::Ensemble && part_names.len() > 1 {
                ui.separator();
                ui.label("Include:");
                while self.ensemble_visible_parts.len() < part_names.len() {
                    self.ensemble_visible_parts.push(true);
                }
                for (idx, name) in part_names.iter().enumerate() {
                    let mut vis = self.ensemble_visible_parts[idx];
                    if ui.checkbox(&mut vis, name).changed() {
                        self.ensemble_visible_parts[idx] = vis;
                        // Ensure at least one instrument remains visible
                        if !self.ensemble_visible_parts.iter().any(|&v| v) {
                            self.ensemble_visible_parts[idx] = true;
                        }
                    }
                }
            }

            ui.separator();
            if ui
                .button("Export PDF...")
                .on_hover_text("Export the currently displayed Layout Mode tab (Ensemble or Part) to PDF")
                .clicked()
            {
                self.export_pdf_file();
            }

            if self.selected_layout_measures.len() >= 2 {
                ui.separator();
                let start_m = *self.selected_layout_measures.iter().next().unwrap();
                let end_m = *self.selected_layout_measures.iter().last().unwrap();
                let btn_text = format!("Make into System (Bars {}..{})", start_m + 1, end_m + 1);
                if ui.button(btn_text).clicked() {
                    self.action_make_into_system(start_m, end_m);
                }
            } else if !self.selected_layout_measures.is_empty() {
                ui.separator();
                let m = *self.selected_layout_measures.iter().next().unwrap();
                ui.label(format!("Bar {} selected (Shift+click another bar to make a system)", m + 1));
            }

            if let Some(sel_id) = &self.selected_layout_element_id {
                if sel_id.starts_with("sys_lock_") {
                    let parts: Vec<&str> = sel_id.split('_').collect();
                    if parts.len() >= 4 {
                        if let (Ok(s), Ok(e)) = (parts[2].parse::<usize>(), parts[3].parse::<usize>()) {
                            if ui.button(format!("Unlock System (Bars {}..{})", s + 1, e + 1)).clicked() {
                                self.action_unlock_system(s, e);
                            }
                        }
                    }
                } else if !self.selected_layout_element_ids.is_empty() {
                    if ui.button("Reset Selected Offset(s)").clicked() {
                        let ids: Vec<String> = self.selected_layout_element_ids.iter().cloned().collect();
                        for id in ids {
                            self.action_set_element_offset(&id, 0.0, 0.0);
                        }
                    }
                }
            }
        });
        ui.separator();

        let mut offsets_to_apply: Vec<(String, f32, f32)> = Vec::new();
        let active_parts = self.active_layout_part_indices();

        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);

                for (page_idx, page) in pages.iter().enumerate() {
                    let page_size = egui::vec2(page.width, page.height);
                    let (page_rect, page_resp) = ui.allocate_exact_size(page_size, egui::Sense::click_and_drag());

                    let painter = ui.painter();

                    // Drop shadow
                    painter.rect_filled(
                        page_rect.translate(egui::vec2(4.0, 4.0)),
                        4.0,
                        Color32::from_rgba_unmultiplied(0, 0, 0, 35),
                    );
                    // Paper
                    painter.rect_filled(page_rect, 2.0, Color32::WHITE);
                    // Border
                    painter.rect_stroke(
                        page_rect,
                        2.0,
                        egui::Stroke::new(1.0, Color32::from_rgb(210, 210, 210)),
                        egui::StrokeKind::Outside,
                    );

                    // Page number footer
                    let page_footer = format!("Page {} of {}", page_idx + 1, pages.len());
                    painter.text(
                        page_rect.center_bottom() - egui::vec2(0.0, page.margins.bottom * 0.4),
                        egui::Align2::CENTER_BOTTOM,
                        page_footer,
                        egui::FontId::proportional(10.0),
                        Color32::from_rgb(140, 140, 140),
                    );

                    // Build page display list
                    let mut dl = crate::layout::display_list::DisplayList::new();
                    if let Some(doc) = &self.parsed_mnx {
                        crate::pdf_export::populate_page_display_list_for_parts(doc, page, &mut dl, &active_parts);
                        if let Some(layout_cfg) = &doc.layout {
                            dl.apply_offsets(&layout_cfg.offsets);
                        }
                    }

                    // Handle hit testing
                    if mouse_clicked {
                        let shift_down = ui.input(|i| i.modifiers.shift);
                        if let Some(mpos) = mouse_pos {
                            if page_rect.contains(mpos) {
                                let local_pos = egui::pos2(mpos.x - page_rect.min.x, mpos.y - page_rect.min.y);
                                if let Some(hit_id) = dl.hit_test(local_pos) {
                                    if hit_id.starts_with("measure_") {
                                        if let Ok(m) = hit_id["measure_".len()..].parse::<usize>() {
                                            if shift_down {
                                                if self.selected_layout_measures.contains(&m) {
                                                    self.selected_layout_measures.remove(&m);
                                                } else {
                                                    self.selected_layout_measures.insert(m);
                                                }
                                            } else {
                                                self.selected_layout_measures.clear();
                                                self.selected_layout_measures.insert(m);
                                                self.selected_layout_element_ids.clear();
                                                self.selected_layout_element_id = None;
                                            }
                                        }
                                    } else {
                                        if shift_down {
                                            if self.selected_layout_element_ids.contains(&hit_id) {
                                                self.selected_layout_element_ids.remove(&hit_id);
                                                if self.selected_layout_element_id.as_ref() == Some(&hit_id) {
                                                    self.selected_layout_element_id = self.selected_layout_element_ids.iter().next().cloned();
                                                }
                                            } else {
                                                self.selected_layout_element_ids.insert(hit_id.clone());
                                                self.selected_layout_element_id = Some(hit_id);
                                            }
                                        } else {
                                            self.selected_layout_measures.clear();
                                            self.selected_layout_element_ids.clear();
                                            self.selected_layout_element_ids.insert(hit_id.clone());
                                            self.selected_layout_element_id = Some(hit_id);
                                        }
                                    }
                                } else if !shift_down {
                                    self.selected_layout_measures.clear();
                                    self.selected_layout_element_ids.clear();
                                    self.selected_layout_element_id = None;
                                }
                            }
                        }
                    }

                    if page_resp.double_clicked() {
                        if let Some(mpos) = mouse_pos {
                            if page_rect.contains(mpos) {
                                let local_pos = egui::pos2(mpos.x - page_rect.min.x, mpos.y - page_rect.min.y);
                                if let Some(hit_id) = dl.hit_test(local_pos) {
                                    if hit_id.starts_with("lyric_") {
                                        let parts: Vec<&str> = hit_id.split('_').collect();
                                        if parts.len() >= 3 {
                                            if let (Ok(m_idx), Ok(ev_in_m)) = (parts[1].parse::<usize>(), parts[2].parse::<usize>()) {
                                                let line_idx = if parts.len() >= 4 {
                                                    parts[3].parse::<usize>().unwrap_or(1)
                                                } else {
                                                    1
                                                };
                                                let mut flat_idx = 0;
                                                let mut found = false;
                                                if let Some(doc) = &self.parsed_mnx {
                                                    if let Some(part) = doc.parts.first() {
                                                        for (curr_m_idx, m) in part.measures.iter().enumerate() {
                                                            for seq in &m.sequences {
                                                                for (curr_ev_in_m, _) in seq.flattened_events().iter().enumerate() {
                                                                    if curr_m_idx == m_idx && curr_ev_in_m == ev_in_m {
                                                                        found = true;
                                                                        break;
                                                                    }
                                                                    flat_idx += 1;
                                                                }
                                                                if found { break; }
                                                            }
                                                            if found { break; }
                                                        }
                                                    }
                                                }
                                                if found {
                                                    self.open_lyric_dialog(flat_idx, line_idx);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Handle hover cursor
                    if let Some(mpos) = mouse_pos {
                        if page_rect.contains(mpos) {
                            let local_pos = egui::pos2(mpos.x - page_rect.min.x, mpos.y - page_rect.min.y);
                            if dl.hit_test(local_pos).is_some() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                        }
                    }

                    // Render display list items
                    let page_origin = page_rect.min.to_vec2();
                    for item in &dl.items {
                        match &item.kind {
                            crate::layout::display_list::DrawKind::Line { from, to, stroke } => {
                                painter.line_segment(
                                    [*from + page_origin, *to + page_origin],
                                    *stroke,
                                );
                            }
                            crate::layout::display_list::DrawKind::Rect { rect, fill, stroke, rounding } => {
                                let srect = rect.translate(page_origin);
                                if fill.a() > 0 {
                                    painter.rect_filled(srect, *rounding, *fill);
                                }
                                if stroke.width > 0.0 {
                                    painter.rect_stroke(srect, *rounding, *stroke, egui::StrokeKind::Middle);
                                }
                            }
                            crate::layout::display_list::DrawKind::Text {
                                pos,
                                align,
                                text,
                                font_id,
                                color,
                                bold,
                                italic,
                            } => {
                                let screen_pos = *pos + page_origin;
                                if *italic || *bold {
                                    let mut job = egui::text::LayoutJob::default();
                                    job.append(
                                        text,
                                        0.0,
                                        egui::TextFormat {
                                            font_id: font_id.clone(),
                                            color: *color,
                                            italics: *italic,
                                            ..Default::default()
                                        },
                                    );
                                    let galley = painter.layout_job(job);
                                    let size = galley.size();
                                    let min_x = match align.x() {
                                        eframe::emath::Align::Min => screen_pos.x,
                                        eframe::emath::Align::Center => screen_pos.x - size.x * 0.5,
                                        eframe::emath::Align::Max => screen_pos.x - size.x,
                                    };
                                    let min_y = match align.y() {
                                        eframe::emath::Align::Min => screen_pos.y,
                                        eframe::emath::Align::Center => screen_pos.y - size.y * 0.5,
                                        eframe::emath::Align::Max => screen_pos.y - size.y,
                                    };
                                    let draw_pos = egui::pos2(min_x, min_y);
                                    painter.galley(draw_pos, galley.clone(), *color);
                                    if *bold {
                                        painter.galley(draw_pos + egui::vec2(0.65, 0.0), galley, *color);
                                    }
                                } else {
                                    painter.text(
                                        screen_pos,
                                        *align,
                                        text,
                                        font_id.clone(),
                                        *color,
                                    );
                                }
                            }
                            crate::layout::display_list::DrawKind::Glyph { pos, align, codepoint, font_family, size, color } => {
                                let font_id = egui::FontId::new(*size, egui::FontFamily::Name(font_family.clone().into()));
                                painter.text(
                                    *pos + page_origin,
                                    *align,
                                    codepoint.to_string(),
                                    font_id,
                                    *color,
                                );
                            }
                            crate::layout::display_list::DrawKind::Circle { center, radius, fill } => {
                                painter.circle_filled(*center + page_origin, *radius, *fill);
                            }
                            crate::layout::display_list::DrawKind::Curve { points, stroke } => {
                                let pts = [
                                    points[0] + page_origin,
                                    points[1] + page_origin,
                                    points[2] + page_origin,
                                    points[3] + page_origin,
                                ];
                                let shape = egui::epaint::CubicBezierShape::from_points_stroke(
                                    pts,
                                    false,
                                    Color32::TRANSPARENT,
                                    *stroke,
                                );
                                painter.add(shape);
                            }
                        }
                    }

                    // Highlight selected measures
                    for &m_idx in &self.selected_layout_measures {
                        let m_id = format!("measure_{}", m_idx);
                        for item in &dl.items {
                            if item.id.as_deref() == Some(&m_id) {
                                let mrect = item.bounding_box().translate(page_origin);
                                painter.rect_filled(mrect, 2.0, Color32::from_rgba_unmultiplied(0, 120, 215, 30));
                                painter.rect_stroke(mrect, 2.0, egui::Stroke::new(1.5, Color32::from_rgba_unmultiplied(0, 120, 215, 120)), egui::StrokeKind::Inside);
                            }
                        }
                    }

                    // Render unified selection highlight box for selected elements
                    for sel_id in &self.selected_layout_element_ids {
                        let mut union_box: Option<egui::Rect> = None;
                        for item in &dl.items {
                            if item.id.as_ref() == Some(sel_id) {
                                let bbox = item.bounding_box().translate(page_origin);
                                if let Some(ub) = &mut union_box {
                                    *ub = ub.union(bbox);
                                } else {
                                    union_box = Some(bbox);
                                }
                            }
                        }
                        if let Some(ub) = union_box {
                            let sel_box = ub.expand(4.0);
                            painter.rect_stroke(
                                sel_box,
                                2.0,
                                egui::Stroke::new(1.5, Color32::from_rgb(0, 120, 215)),
                                egui::StrokeKind::Outside,
                            );
                        }
                    }

                    // Drag handling for selected elements
                    if !self.selected_layout_element_ids.is_empty() {
                        if page_resp.dragged() {
                            self.is_dragging_layout_element = true;
                            if drag_delta.length_sq() > 0.0 {
                                let ids: Vec<String> = self.selected_layout_element_ids.iter().cloned().collect();
                                for id in ids {
                                    let cur_off = offsets.get(&id).copied().unwrap_or(crate::mnx::Offset2D { dx: 0.0, dy: 0.0 });
                                    offsets_to_apply.push((id, cur_off.dx + drag_delta.x, cur_off.dy + drag_delta.y));
                                }
                            }
                        } else {
                            self.is_dragging_layout_element = false;
                        }
                    } else {
                        self.is_dragging_layout_element = false;
                    }

                    ui.add_space(30.0);
                }
            });
        });

        for (id, ox, oy) in offsets_to_apply {
            self.action_set_element_offset(&id, ox, oy);
        }

        if arrow_nudge != egui::vec2(0.0, 0.0) {
            let ids: Vec<String> = self.selected_layout_element_ids.iter().cloned().collect();
            for id in ids {
                let cur_off = offsets.get(&id).copied().unwrap_or(crate::mnx::Offset2D { dx: 0.0, dy: 0.0 });
                self.action_set_element_offset(&id, cur_off.dx + arrow_nudge.x, cur_off.dy + arrow_nudge.y);
            }
        }
    }
}
