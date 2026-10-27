use super::*;
use crate::yaml_editor::*;
use eframe::egui::{self, Color32};

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
                        let img = egui::Image::new(texture).fit_to_exact_size(icon_size);
                        let btn = egui::Button::image(img).selected(is_active);
                        ui.add_sized(btn_size, btn).on_hover_text(tooltip).clicked()
                    };

                    let make_svg_btn = |ui: &mut egui::Ui, source: egui::ImageSource<'static>, is_active: bool, tooltip: &str| -> bool {
                        let img = egui::Image::new(source).fit_to_exact_size(icon_size);
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
                        doc.find_event_grace_info(e).is_some()
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
                        if let Some(part) = doc.parts.first() {
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
                    self.active_dialog = ActiveDialog::InsertTempo {
                        bpm_str: "120".to_string(),
                        duration_value: "quarter".to_string(),
                        dots: 0,
                        event_idx: start_event_idx,
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
                        if let Some((items, _, _, _, _)) = self.extract_selected_items() {
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
                        if let Some(part) = doc.parts.first() {
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
                        if let Some(part) = doc.parts.first() {
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

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);

                let bravura_font = egui::FontId::new(20.0, egui::FontFamily::Name("BravuraText".into()));
                if ui.button(egui::RichText::new("\u{E040}").font(bravura_font.clone())).on_hover_text("Start Repeat").clicked() {
                    let m_idx = self.get_target_measure_for_layout_or_breaks();
                    let new_yaml = crate::yaml_editor::set_global_measure_property(
                        &self.yaml_text,
                        m_idx,
                        "repeatStart",
                        "{}"
                    );
                    self.update_yaml(new_yaml);
                    self.process_and_render();
                }

                if ui.button(egui::RichText::new("\u{E041}").font(bravura_font.clone())).on_hover_text("End Repeat").clicked() {
                    let m_idx = self.get_target_measure_for_layout_or_breaks();
                    let new_yaml = crate::yaml_editor::set_global_measure_property(
                        &self.yaml_text,
                        m_idx,
                        "repeatEnd",
                        "{ times: 2 }"
                    );
                    self.update_yaml(new_yaml);
                    self.process_and_render();
                }

                if ui.button(egui::RichText::new("\u{E031}").font(bravura_font.clone())).on_hover_text("Double Barline").clicked() {
                    let m_idx = self.get_target_measure_for_layout_or_breaks();
                    let is_already_double = self.parsed_mnx.as_ref()
                        .and_then(|doc| doc.global.measures.get(m_idx))
                        .and_then(|gm| gm.barline.as_ref())
                        .map(|b| b.barline_type == "double")
                        .unwrap_or(false);
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

                if ui.button(egui::RichText::new("\u{E032}").font(bravura_font.clone())).on_hover_text("Ending Barline").clicked() {
                    let m_idx = self.get_target_measure_for_layout_or_breaks();
                    let is_already_final = self.parsed_mnx.as_ref()
                        .and_then(|doc| doc.global.measures.get(m_idx))
                        .and_then(|gm| gm.barline.as_ref())
                        .map(|b| b.barline_type == "final" || b.barline_type == "ending")
                        .unwrap_or(false);
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

                let icon_size = egui::vec2(24.0, 24.0);
                let btn_size = egui::vec2(30.0, 30.0);

                let img_closed = egui::Image::new(include_image!("../../assets/closed ending.svg")).fit_to_exact_size(icon_size);
                if ui.add_sized(btn_size, egui::Button::image(img_closed)).on_hover_text("Insert 1st... Ending").clicked() {
                    let (start_m, end_m) = self.get_selected_measure_range();
                    self.active_dialog = ActiveDialog::InsertEnding {
                        start_m,
                        end_m,
                        times_str: "1".to_string(),
                    };
                }

                let img_open = egui::Image::new(include_image!("../../assets/open ending.svg")).fit_to_exact_size(icon_size);
                if ui.add_sized(btn_size, egui::Button::image(img_open)).on_hover_text("Insert Open Ending").clicked() {
                    let (start_m, end_m) = self.get_selected_measure_range();
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
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Technique:").strong());
                let btn_size = egui::vec2(30.0, 30.0);
                let icon_size = egui::vec2(24.0, 24.0);
                let make_svg_btn = |ui: &mut egui::Ui, source: egui::ImageSource<'static>, is_active: bool, tooltip: &str| -> bool {
                    let img = egui::Image::new(source).fit_to_exact_size(icon_size);
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
                    if ui.add_sized(egui::vec2(44.0, 30.0), nh_btn).on_hover_text("Natural Harmonic").clicked() {
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
                    if ui.add(play_btn).on_hover_text("Play tablature via MIDI (Hotkey 'P' to play from selected note, 'Shift+P' from start)").clicked() {
                        self.start_midi_playback(self.selected_event_idx.is_some());
                    }
                }

                ui.separator();

                // 2. Dropdown of available MIDI output devices + refresh button
                let selected_port_name = self.selected_midi_port
                    .and_then(|id| self.midi_ports.iter().find(|p| p.id == id).map(|p| p.name.clone()))
                    .unwrap_or_else(|| {
                        if self.midi_ports.is_empty() {
                            "No MIDI Out Found".to_string()
                        } else {
                            "Select Device...".to_string()
                        }
                    });

                egui::ComboBox::from_id_salt("midi_out_device_select")
                    .selected_text(selected_port_name)
                    .width(160.0)
                    .show_ui(ui, |ui| {
                        if self.midi_ports.is_empty() {
                            ui.label("No devices detected");
                        } else {
                            for port in &self.midi_ports {
                                ui.selectable_value(&mut self.selected_midi_port, Some(port.id), &port.name);
                            }
                        }
                    });

                if ui.small_button("⟳").on_hover_text("Refresh MIDI devices").clicked() {
                    self.midi_ports = crate::midi::get_midi_output_ports();
                    if self.selected_midi_port.is_none() || !self.midi_ports.iter().any(|p| Some(p.id) == self.selected_midi_port) {
                        self.selected_midi_port = self.midi_ports.first().map(|p| p.id);
                    }
                }

                ui.separator();

                // 3. Pitchbend Range (int 1-48, default 2)
                ui.label("PB Range: ±");
                let mut pb_range_val = self.pitch_bend_range as i32;
                if ui.add(egui::DragValue::new(&mut pb_range_val).range(1..=48).speed(0.2)).on_hover_text("Pitch bend range in semitones (1 - 48)").changed() {
                    let new_pb = pb_range_val.clamp(1, 48) as u8;
                    self.set_pitch_bend_range_in_yaml(new_pb);
                }
                ui.label("st");

                ui.separator();

                // 4. Mode toggle: Multi-timbral vs MPE
                let mode_label = if self.is_mpe_mode { "Mode: MPE" } else { "Mode: Multi-Timbral" };
                if ui.selectable_label(self.is_mpe_mode, mode_label)
                    .on_hover_text("Toggle between Multi-Timbral (channel per string) and MPE mode (Ch 1 Master, Ch 2..n+1 Member channels, default ±48 semitones)")
                    .clicked()
                {
                    self.is_mpe_mode = !self.is_mpe_mode;
                    let num_strings = self.parsed_mnx.as_ref()
                        .and_then(|d| d.parts.first())
                        .map(|p| p.num_strings())
                        .unwrap_or(6) as u8;
                    if self.is_mpe_mode {
                        self.set_pitch_bend_range_in_yaml(48);
                        self.string_channel_map.clear();
                        for s in 1..=num_strings {
                            self.string_channel_map.insert(s, s + 1);
                        }
                    } else {
                        self.set_pitch_bend_range_in_yaml(2);
                        self.string_channel_map.clear();
                        for s in 1..=num_strings {
                            self.string_channel_map.insert(s, s);
                        }
                    }
                }

                if ui.button("Instrument Setup").clicked() {
                    let part = self.parsed_mnx.as_ref().and_then(|d| d.parts.first());
                    let num_strings = part.map(|p| p.num_strings()).unwrap_or(6) as u8;
                    let edo = part.map(|p| p.edo()).unwrap_or(12) as u8;
                    let step_multiplier = part.map(|p| p.step_multiplier()).unwrap_or(1) as u8;
                    let mut tunings_text = Vec::new();
                    let mut tunings_octave = Vec::new();
                    
                    let edo_scale = crate::edo::generate_edo_scale(edo as u32);
                    let tuning_calc = crate::edo::EdoTuning::new(edo as u32);
                    
                    for s in 1..=15 {
                        if let Some(pitch) = part.and_then(|p| p.get_string_tuning(s)) {
                            let steps = tuning_calc.pitch_to_steps(&pitch);
                            let step_in_oct = steps.rem_euclid(edo as i32);
                            let text = edo_scale.iter().find(|(st, _, _, _)| *st == step_in_oct).map(|(_, n, _, _)| n.clone()).unwrap_or_else(|| pitch.step.clone());
                            tunings_text.push(text);
                            tunings_octave.push(pitch.octave);
                        } else {
                            let (step, octave) = match s {
                                1 => ("E", 4),
                                2 => ("B", 3),
                                3 => ("G", 3),
                                4 => ("D", 3),
                                5 => ("A", 2),
                                6 => ("E", 2),
                                7 => ("B", 1),
                                8 => ("F#", 1),
                                9 => ("C#", 1),
                                10 => ("G#", 0),
                                11 => ("D#", 0),
                                12 => ("A#", 0),
                                13 => ("F", 0),
                                14 => ("C", 0),
                                15 => ("G", 0),
                                _ => ("E", 2),
                            };
                            tunings_text.push(step.to_string());
                            tunings_octave.push(octave);
                        }
                    }
                    self.active_dialog = ActiveDialog::InstrumentSetup {
                        num_strings,
                        edo,
                        step_multiplier,
                        tunings_text,
                        tunings_octave,
                        is_new_tab: false,
                    };
                }

                ui.separator();

                // 5. String-to-Channel Mappings button with Popup
                let num_strings = self.parsed_mnx.as_ref()
                    .and_then(|d| d.parts.first())
                    .map(|p| p.num_strings())
                    .unwrap_or(6) as u8;

                ui.menu_button("String Channels ◢", |ui| {
                    ui.label(egui::RichText::new(if self.is_mpe_mode {
                        "String to MPE Member Channel Mapping (Ch 2-16, Ch 1 is Master)"
                    } else {
                        "String to MIDI Channel Mapping (Ch 1-16)"
                    }).strong());
                    ui.separator();
                    for s in 1..=num_strings {
                        ui.horizontal(|ui| {
                            ui.label(format!("String {}:", s));
                            let default_ch = if self.is_mpe_mode { s + 1 } else { s };
                            let current_ch = self.string_channel_map.entry(s).or_insert(default_ch);
                            let mut ch_val = *current_ch as i32;
                            let (min_ch, max_ch) = if self.is_mpe_mode { (2, 16) } else { (1, 16) };
                            if ui.add(egui::DragValue::new(&mut ch_val).range(min_ch..=max_ch).speed(0.1)).changed() {
                                *current_ch = ch_val.clamp(min_ch, max_ch) as u8;
                            }
                            ui.label(format!("(Ch {})", ch_val));
                        });
                    }
                    if ui.button(if self.is_mpe_mode { "Reset to MPE Default (2..n+1)" } else { "Reset to Default (1..n)" }).clicked() {
                        for s in 1..=num_strings {
                            let def_ch = if self.is_mpe_mode { s + 1 } else { s };
                            self.string_channel_map.insert(s, def_ch);
                        }
                    }
                });

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
                        if let Some(doc) = &self.parsed_mnx {
                            if let Some(part) = doc.parts.first() {
                                let (_resp, clicked, add_bar_clicked, double_clicked_lyric) = render::draw_tablature(
                                    ui,
                                    crate::render::TablatureOptions {
                                        part,
                                        global: Some(&doc.global),
                                        errors: &self.parsed_errors,
                                        selected_event: self.selected_event_idx,
                                        selection_anchor: self.selection_anchor,
                                        selected_note_in_event: self.selected_note_idx_in_event,
                                        is_editing_fret: self.is_editing_fret,
                                        editing_fret_text: &self.editing_fret_buf,
                                        editing_string: self.editing_string,
                                        is_editing_lyric: self.is_editing_lyric,
                                        editing_lyric_text: &self.editing_lyric_buf,
                                        scroll_to_selected: self.needs_scroll,
                                        pitch_bend_range: self.pitch_bend_range,
                                    },
                                );
                                self.needs_scroll = false;
                                if let Some((ev_idx, line_idx)) = double_clicked_lyric {
                                    self.selected_event_idx = Some(ev_idx);
                                    self.selected_note_idx_in_event = None;
                                    self.open_lyric_dialog(ev_idx, line_idx);
                                } else if add_bar_clicked {
                                    self.add_measure_to_end();
                                } else if let Some((clicked_event, clicked_note)) = clicked {
                                    self.needs_scroll = false;
                                    let total_events = find_all_event_ranges(&self.yaml_text).len();
                                    let was_at_final =
                                        self.selected_event_idx == Some(total_events.saturating_sub(1));
                                    let is_final_blank = was_at_final
                                        && self
                                            .parsed_mnx
                                            .as_ref()
                                            .and_then(|d| d.parts.first())
                                            .and_then(|p| p.measures.last())
                                            .and_then(|m| m.sequences.first())
                                            .and_then(|s| s.flattened_events().last().copied())
                                            .map(|e| e.notes.is_empty() && e.rest.is_none())
                                            .unwrap_or(false);

                                    if is_final_blank && clicked_event < total_events.saturating_sub(1) {
                                        if let Some(new_yaml) = remove_last_blank_event(&self.yaml_text) {
                                            self.update_yaml(new_yaml);
                                            self.process_and_render();
                                        }
                                    }

                                    let shift_down = ui.input(|i| i.modifiers.shift);
                                    if shift_down {
                                        if self.selection_anchor.is_none() {
                                            self.selection_anchor = self.selected_event_idx;
                                        }
                                    } else {
                                        self.selection_anchor = None;
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
                            } else {
                                ui.label("No parts found in document.");
                            }
                        } else {
                            ui.label("Could not parse document.");
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
        let (pages, offsets) = if let Some(doc) = &self.parsed_mnx {
            let p = crate::layout::compute_score_layout(doc, 0.0);
            let offs = doc.layout.as_ref().map(|l| l.offsets.clone()).unwrap_or_default();
            (p, offs)
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
                } else {
                    let ids: Vec<String> = self.selected_layout_element_ids.iter().cloned().collect();
                    for id in ids {
                        self.action_set_element_offset(&id, 0.0, 0.0);
                    }
                }
            }
        }

        // Layout Mode Toolbar
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Layout Mode").strong());
            ui.separator();

            if self.selected_layout_measures.len() >= 2 {
                let start_m = *self.selected_layout_measures.iter().next().unwrap();
                let end_m = *self.selected_layout_measures.iter().last().unwrap();
                let btn_text = format!("Make into System (Bars {}..{})", start_m + 1, end_m + 1);
                if ui.button(btn_text).clicked() {
                    self.action_make_into_system(start_m, end_m);
                }
            } else if !self.selected_layout_measures.is_empty() {
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
                        crate::pdf_export::populate_page_display_list(doc, page, &mut dl);
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
