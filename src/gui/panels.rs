use super::*;
use eframe::egui::{self, Color32};
use crate::yaml_editor::*;

impl MnxEditorApp {
    pub(crate) fn render_top_panel(
        &mut self,
        ui: &mut egui::Ui,
        event_ranges: &[EventRange],
        total_events: usize,
    ) {
        egui::Panel::top("top_panel").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("New").clicked() {
                    self.new_file();
                }
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
                    if self.is_editing_fret {
                        ui.label(
                            egui::RichText::new("EDIT FRET (type number, press Enter to commit, Esc to cancel)")
                                .color(Color32::from_rgb(255, 175, 75))
                                .strong(),
                        );
                    } else {
                        let note_info = if let Some(n_idx) = self.selected_note_idx_in_event {
                            let note_count = event_ranges.get(sel).map(|er| er.note_ranges.len()).unwrap_or(1);
                            format!("Note {} of {} (note {}/{})", sel + 1, total_events, n_idx + 1, note_count)
                        } else {
                            let note_count = event_ranges.get(sel).map(|er| er.note_ranges.len()).unwrap_or(1);
                            if note_count > 1 {
                                format!("Chord {} of {} ({} notes)", sel + 1, total_events, note_count)
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

                enum DurationAction {
                    Base(&'static str),
                    ToggleDot,
                }
                let mut clicked_action: Option<DurationAction> = None;
                let mut rest_clicked = false;
                let mut tuplet_clicked = false;
                let mut tie_clicked = false;
                let mut slur_clicked = false;
                let mut tap_clicked = false;
                let mut bend_clicked = false;
                let mut slide_clicked = false;
                let mut strum_up_clicked = false;
                let mut strum_down_clicked = false;
                let has_text_focus = ui.memory(|m| m.has_focus(egui::Id::new("bpm_input")) || m.has_focus(egui::Id::new("yaml_editor")));

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

                    // Tie button
                    let is_tie = self.is_selected_event_tied();
                    if make_svg_btn(ui, include_image!("../../assets/tie.svg"), is_tie, "Tie (Shift+T)") || (!has_text_focus && ui.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::T))) {
                        tie_clicked = true;
                    }

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
                    
                    // Strum Up button
                    let is_strum_up = self.is_selected_event_strummed("up");
                    let (sup_rect, sup_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(sup_rect) {
                        let visuals = ui.style().interact_selectable(&sup_resp, is_strum_up);
                        ui.painter().rect(sup_rect, visuals.corner_radius, visuals.bg_fill, visuals.bg_stroke, egui::StrokeKind::Inside);
                        let color = visuals.text_color();
                        let font_id = egui::FontId::new(18.0, egui::FontFamily::Name("Bravura".into()));
                        ui.painter().text(sup_rect.center() + egui::vec2(0.0, 13.0), egui::Align2::CENTER_CENTER, "\u{E634}", font_id, color);
                    }
                    if sup_resp.on_hover_text("Strum Up (low to high)").clicked() {
                        strum_up_clicked = true;
                    }

                    // Strum Down button
                    let is_strum_down = self.is_selected_event_strummed("down");
                    let (sdown_rect, sdown_resp) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                    if ui.is_rect_visible(sdown_rect) {
                        let visuals = ui.style().interact_selectable(&sdown_resp, is_strum_down);
                        ui.painter().rect(sdown_rect, visuals.corner_radius, visuals.bg_fill, visuals.bg_stroke, egui::StrokeKind::Inside);
                        let color = visuals.text_color();
                        let font_id = egui::FontId::new(18.0, egui::FontFamily::Name("Bravura".into()));
                        ui.painter().text(sdown_rect.center() + egui::vec2(0.0, 13.0), egui::Align2::CENTER_CENTER, "\u{E635}", font_id, color);
                    }
                    if sdown_resp.on_hover_text("Strum Down (high to low)").clicked() {
                        strum_down_clicked = true;
                    }
                }

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
                if strum_up_clicked {
                    self.action_toggle_strum("up");
                }
                if strum_down_clicked {
                    self.action_toggle_strum("down");
                }

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

                // ── BPM Input ─────────────────────────────────────────────────
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                ui.label(egui::RichText::new("♩ =").strong());

                let bpm_id = egui::Id::new("bpm_input");
                let bpm_resp = ui.add(
                    egui::TextEdit::singleline(&mut self.bpm_buf)
                        .id(bpm_id)
                        .desired_width(48.0)
                        .hint_text("120"),
                );

                // Commit when user presses Enter or loses focus after typing
                let commit_bpm = bpm_resp.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let commit_bpm = commit_bpm || (bpm_resp.lost_focus() && !bpm_resp.has_focus());

                if commit_bpm {
                    if let Ok(new_bpm) = self.bpm_buf.trim().parse::<f64>() {
                        if new_bpm > 0.0 {
                            self.update_yaml(set_bpm_in_yaml(&self.yaml_text, new_bpm));
                            self.process_and_render();
                        }
                    }
                }

                ui.label("BPM").on_hover_text("Beats per minute (written into global/measures[0]/directions)");
            });

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
                    self.pitch_bend_range = pb_range_val.clamp(1, 48) as u8;
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
                        self.pitch_bend_range = 48;
                        self.string_channel_map.clear();
                        for s in 1..=num_strings {
                            self.string_channel_map.insert(s, s + 1);
                        }
                    } else {
                        self.pitch_bend_range = 2;
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
                                _ => ("", 0),
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

                ui.menu_button("String Channels ▾", |ui| {
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
            egui::ScrollArea::both().show(ui, |ui| {
                if let Some(doc) = &self.parsed_mnx {
                    if let Some(part) = doc.parts.first() {
                        let (_resp, clicked, add_bar_clicked) = render::draw_tablature(
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
                                scroll_to_selected: self.needs_scroll,
                                pitch_bend_range: self.pitch_bend_range,
                            },
                        );
                        self.needs_scroll = false;
                        if add_bar_clicked {
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
        });
    }

}
