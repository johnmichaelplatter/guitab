use super::*;
use eframe::egui::{self};

impl MnxEditorApp {
    pub(crate) fn render_dialogs(&mut self, ctx: &egui::Context) {
        enum DialogAction {
            InsertTempo {
                bpm: f64,
                duration_value: String,
                dots: u32,
                event_idx: usize,
            },
            ApplyBend {
                event_idx: usize,
                string: u8,
                bend_str: String,
            },

            InsertTempoChange { event_idx: usize, end_bpm: f64, dur_beats: f64, bend_str: String, existing_target_id: Option<String> },
            RemoveTempoChange { target_id: String },
            InsertTimeSignature {
                count: u32,
                unit: u32,
                m_idx: usize,
                rebar_mode: u8,
            },
            CreateTuplet {
                inner: u32,
                outer: u32,
                base: String,
                from_select_mode: bool,
                event_idx: usize,
            },
            RemoveTuplet {
                event_idx: usize,
            },
            ApplyInstrumentSetup {
                num_strings: u8,
                edo: u8,
                step_multiplier: u8,
                tunings_text: Vec<String>,
                tunings_octave: Vec<i32>,
                is_new_tab: bool,
            },
            TranslateEdo {
                new_edo: u8,
                new_step_multiplier: u8,
            },
            TranslateTuning {
                new_tunings: Vec<crate::mnx::StringTuning>,
            },
            StartNewTab {
                num_strings: u8,
                edo: u8,
                step_multiplier: u8,
                tunings: Vec<crate::mnx::StringTuning>,
            },
            DiscardChangesSave,
            DiscardChangesSaveAs,
            DiscardChangesProceed,
            ApplyNaturalHarmonic {
                event_idx: usize,
                string: u8,
                touch: f64,
                sounding_pitch: crate::mnx::Pitch,
            },
        }

        let mut close_dialog = false;
        let mut apply_action = None;

        match &mut self.active_dialog {
            ActiveDialog::None => {}
            ActiveDialog::InsertTempo {
                bpm_str,
                duration_value,
                dots,
                event_idx,
            } => {
                let mut close_dialog = false;
                
                let event_idx_val = *event_idx;
                let dots_val = *dots;

                egui::Window::new("Insert Tempo")
                    .collapsible(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt("tempo_dur")
                                .width(40.0)
                                .selected_text(
                                    egui::RichText::new(match duration_value.as_str() {
                                        "eighth" => "\u{ECA7}",
                                        "16th" | "sixteenth" => "\u{ECA9}",
                                        _ => "\u{ECA5}",
                                    })
                                    .family(egui::FontFamily::Name("BravuraText".into()))
                                    .size(20.0)
                                )
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(duration_value, "quarter".to_string(), egui::RichText::new("\u{ECA5}").family(egui::FontFamily::Name("BravuraText".into())).size(20.0));
                                    ui.selectable_value(duration_value, "eighth".to_string(), egui::RichText::new("\u{ECA7}").family(egui::FontFamily::Name("BravuraText".into())).size(20.0));
                                    ui.selectable_value(duration_value, "16th".to_string(), egui::RichText::new("\u{ECA9}").family(egui::FontFamily::Name("BravuraText".into())).size(20.0));
                                });
                                
                            let mut is_dotted = *dots > 0;
                            if ui.checkbox(&mut is_dotted, "Dotted").changed() {
                                *dots = if is_dotted { 1 } else { 0 };
                            }

                            ui.label("=");

                            ui.add(
                                egui::TextEdit::singleline(bpm_str)
                                    .desired_width(40.0)
                            );
                        });

                        ui.horizontal(|ui| {
                            if ui.button("Insert").clicked() {
                                if let Ok(bpm) = bpm_str.parse::<f64>() {
                                    apply_action = Some(DialogAction::InsertTempo {
                                        bpm,
                                        duration_value: duration_value.clone(),
                                        dots: dots_val,
                                        event_idx: event_idx_val,
                                    });
                                    close_dialog = true;
                                }
                            }
                            if ui.button("Cancel").clicked() {
                                close_dialog = true;
                            }
                        });
                    });

                if close_dialog {
                    self.active_dialog = ActiveDialog::None;
                }
            }


            ActiveDialog::InsertTempoChange {
                event_idx,
                ending_tempo,
                duration_beats,
                curve_editor,
                existing_target_id,
            } => {
                let mut close_dialog = false;
                let event_idx_val = *event_idx;

                egui::Window::new("Insert Ritardando / Accelerando")
                    .collapsible(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Ending Tempo:");
                            let response = ui.text_edit_singleline(ending_tempo);
                            if response.changed() {
                                if let Ok(bpm) = ending_tempo.parse::<f64>() {
                                    let current_bpm = 120.0;
                                    
                                    let rect = curve_editor.last_rect.unwrap_or(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 100.0)));
                                    if bpm > current_bpm {
                                        let top_right = rect.right_top();
                                        let bottom_left = rect.left_bottom();
                                        let delta = top_right - bottom_left;
                                        curve_editor.nodes = vec![
                                            crate::curve_editor::CurveNode { anchor: bottom_left, left_handle: bottom_left, right_handle: bottom_left + delta * 0.33 },
                                            crate::curve_editor::CurveNode { anchor: top_right, left_handle: bottom_left + delta * 0.66, right_handle: top_right },
                                        ];
                                    } else {
                                        let top_left = rect.left_top();
                                        let bottom_right = rect.right_bottom();
                                        let delta = bottom_right - top_left;
                                        curve_editor.nodes = vec![
                                            crate::curve_editor::CurveNode { anchor: top_left, left_handle: top_left, right_handle: top_left + delta * 0.33 },
                                            crate::curve_editor::CurveNode { anchor: bottom_right, left_handle: top_left + delta * 0.66, right_handle: bottom_right },
                                        ];
                                    }

                                }
                            }
                        });
                        
                        ui.horizontal(|ui| {
                            ui.label("Duration (in beats):");
                            ui.text_edit_singleline(duration_beats);
                        });
                        
                        ui.label("Tempo Curve:");
                        let rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(300.0, 150.0));
                        curve_editor.ui(ui, rect, 1, 1);
                        ui.add_space(8.0);
                        
                        ui.horizontal(|ui| {
                            if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                if let (Ok(end_bpm), Ok(dur_beats)) = (ending_tempo.parse::<f64>(), duration_beats.parse::<f64>()) {
                                    
                                    let bend_str = curve_editor.to_svg_path_normalized();
                                    apply_action = Some(DialogAction::InsertTempoChange { 
                                        event_idx: event_idx_val, 
                                        end_bpm, 
                                        dur_beats, 
                                        bend_str,
                                        existing_target_id: existing_target_id.clone(),
                                    });
                                    close_dialog = true;
                                }
                            }
                            if let Some(tid) = existing_target_id {
                                if ui.button("Remove rit/accel").clicked() {
                                    apply_action = Some(DialogAction::RemoveTempoChange { target_id: tid.clone() });
                                    close_dialog = true;
                                }
                            }
                            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                close_dialog = true;
                            }
                        });
                    });

                if close_dialog {
                    self.active_dialog = ActiveDialog::None;
                }
            }

            ActiveDialog::ConfirmDiscardChanges => {
                egui::Window::new("Discard changes?")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(ctx, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                        ui.label("You have unsaved changes. What would you like to do?");
                        ui.separator();
                        ui.horizontal(|ui| {
                            if ui.button("Save").clicked() {
                                apply_action = Some(DialogAction::DiscardChangesSave);
                                close_dialog = true;
                            }
                            if ui.button("Save As").clicked() {
                                apply_action = Some(DialogAction::DiscardChangesSaveAs);
                                close_dialog = true;
                            }
                            if ui.button("Discard Changes").clicked() {
                                apply_action = Some(DialogAction::DiscardChangesProceed);
                                close_dialog = true;
                            }
                            if ui.button("Cancel").clicked() {
                                close_dialog = true;
                            }
                        });
                    });
            }
            ActiveDialog::InsertTimeSignature {
                count,
                unit,
                m_idx,
                warning,
            } => {
                egui::Window::new("Insert Time Signature")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(ctx, |ui| {
                        ui.label("Type Time Signature:");
                        ui.add_space(8.0);
                        
                        ui.horizontal(|ui| {
                            let mut c = *count;
                            let mut u = *unit;
                            ui.add(egui::DragValue::new(&mut c).speed(1.0).range(1..=32));
                            ui.label("/");
                            ui.add(egui::DragValue::new(&mut u).speed(1.0).range(1..=32));
                            *count = c;
                            *unit = u;
                        });
                        
                        ui.add_space(8.0);
                        
                        if let Some(w) = warning {
                            ui.label(egui::RichText::new(w.as_str()).color(egui::Color32::from_rgb(255, 100, 100)));
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                if ui.button("Re-bar all following measures").clicked() {
                                    apply_action = Some(DialogAction::InsertTimeSignature { count: *count, unit: *unit, m_idx: *m_idx, rebar_mode: 0 });
                                    close_dialog = true;
                                }
                                if ui.button("Truncate content in this bar only").clicked() {
                                    apply_action = Some(DialogAction::InsertTimeSignature { count: *count, unit: *unit, m_idx: *m_idx, rebar_mode: 1 });
                                    close_dialog = true;
                                }
                                if ui.button("Cancel").clicked() {
                                    close_dialog = true;
                                }
                            });
                        } else {
                            ui.horizontal(|ui| {
                                if ui.button("OK").clicked() {
                                    let capacity = (*count as f64) * 4.0 / (*unit as f64);
                                    let measure_beats = self.parsed_mnx.as_ref()
                                        .and_then(|d| d.parts.first())
                                        .and_then(|p| p.measures.get(*m_idx))
                                        .and_then(|m| m.sequences.first())
                                        .map(|s| s.total_beats())
                                        .unwrap_or(0.0);
                                        
                                    if measure_beats > capacity + 0.001 {
                                        *warning = Some(format!("Warning: there is note content outside the new barline ({} beats in bar, new TS holds {} beats)", measure_beats, capacity));
                                    } else {
                                        apply_action = Some(DialogAction::InsertTimeSignature { count: *count, unit: *unit, m_idx: *m_idx, rebar_mode: 0 });
                                        close_dialog = true;
                                    }
                                }
                                if ui.button("Cancel").clicked() {
                                    close_dialog = true;
                                }
                            });
                        }
                    });
            }
            ActiveDialog::Message { title, text } => {
                egui::Window::new(title.clone())
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(ctx, |ui| {
                        ui.label(text.clone());
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.button("OK").clicked() {
                                close_dialog = true;
                            }
                        });
                    });
            }
            ActiveDialog::CreateTuplet {
                inner_multiple,
                outer_multiple,
                base,
                from_select_mode,
                event_idx,
            } => {
                let from_sel = *from_select_mode;
                let ev_i = *event_idx;
                let base_str = base.clone();

                egui::Window::new("Create Tuplet")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(ctx, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                        ui.label(egui::RichText::new("Specify Tuplet Ratio:").strong());
                        ui.label(format!("Base duration: {}", base_str));

                        ui.horizontal(|ui| {
                            ui.label("Inner multiple:");
                            ui.add(egui::DragValue::new(inner_multiple).range(1..=64));
                        });

                        ui.horizontal(|ui| {
                            ui.label("Outer multiple:");
                            ui.add(egui::DragValue::new(outer_multiple).range(1..=64));
                        });

                        ui.separator();

                        let ok_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let esc_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));

                        ui.horizontal(|ui| {
                            if ui.button("OK").clicked() || ok_pressed {
                                apply_action = Some(DialogAction::CreateTuplet {
                                    inner: *inner_multiple,
                                    outer: *outer_multiple,
                                    base: base_str,
                                    from_select_mode: from_sel,
                                    event_idx: ev_i,
                                });
                                close_dialog = true;
                            }
                            if ui.button("Cancel").clicked() || esc_pressed {
                                close_dialog = true;
                            }
                        });
                    });
            }
            ActiveDialog::ConfirmRemoveTuplet { event_idx } => {
                let ev_i = *event_idx;
                egui::Window::new("Remove Tuplet")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(ctx, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                        ui.label(egui::RichText::new("Remove this tuple?").strong());
                        ui.label(
                            "This will convert all existing note and rhythm durations as standard durations outside the tuple.",
                        );

                        ui.separator();

                        let ok_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let esc_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));

                        ui.horizontal(|ui| {
                            if ui.button("OK").clicked() || ok_pressed {
                                apply_action = Some(DialogAction::RemoveTuplet { event_idx: ev_i });
                                close_dialog = true;
                            }
                            if ui.button("Cancel").clicked() || esc_pressed {
                                close_dialog = true;
                            }
                        });
                    });
            }
            ActiveDialog::InstrumentSetup {
                num_strings,
                edo,
                step_multiplier,
                tunings_text,
                tunings_octave,
                is_new_tab,
            } => {
                egui::Window::new("Instrument Setup")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(ctx, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                        ui.horizontal(|ui| {
                            ui.label("Number of strings:");
                            ui.add(egui::DragValue::new(num_strings).range(1..=15));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Equal Divisions of the Octave:");
                            ui.add(egui::DragValue::new(edo).range(1..=100));
                            ui.label("EDO steps per fret:");
                            ui.add(egui::DragValue::new(step_multiplier).range(1..=10));
                        });

                        let edo_val = *edo as u32;
                        let edo_scale = crate::edo::generate_edo_scale(edo_val);

                        ui.separator();
                        for s in 0..*num_strings as usize {
                            ui.horizontal(|ui| {
                                ui.label(format!("String {} tuning:", s + 1));

                                // Slider for EDO steps
                                let tuning_calc = crate::edo::EdoTuning::new(edo_val);
                                let mut current_step = 0;
                                let parsed_pitch = tuning_calc
                                    .parse_pitch_string(&tunings_text[s], tunings_octave[s]);
                                let is_valid = parsed_pitch.is_some();

                                if let Some(pitch) = &parsed_pitch {
                                    current_step = tuning_calc
                                        .pitch_to_steps(pitch)
                                        .rem_euclid(edo_val as i32);
                                }

                                let prev_step = current_step;
                                ui.add(
                                    egui::Slider::new(&mut current_step, 0..=(edo_val as i32 - 1))
                                        .text("step"),
                                );
                                if current_step != prev_step {
                                    tunings_text[s] = edo_scale
                                        .iter()
                                        .find(|(st, _, _, _)| *st == current_step)
                                        .map(|(_, n, _, _)| n.clone())
                                        .unwrap_or_else(|| "C".to_string());
                                }

                                let text_color = if is_valid {
                                    ui.visuals().text_color()
                                } else {
                                    egui::Color32::RED
                                };
                                ui.add(
                                    egui::TextEdit::singleline(&mut tunings_text[s])
                                        .text_color(text_color)
                                        .desired_width(40.0),
                                );
                                ui.label("Octave:");
                                ui.add(egui::DragValue::new(&mut tunings_octave[s]).range(-2..=9));
                            });
                        }

                        ui.separator();
                        ui.horizontal(|ui| {
                            if ui.button("Accept").clicked() {
                                apply_action = Some(DialogAction::ApplyInstrumentSetup {
                                    num_strings: *num_strings,
                                    edo: *edo,
                                    step_multiplier: *step_multiplier,
                                    tunings_text: tunings_text.clone(),
                                    tunings_octave: tunings_octave.clone(),
                                    is_new_tab: *is_new_tab,
                                });
                                close_dialog = true;
                            }
                            if ui.button("Cancel").clicked() {
                                close_dialog = true;
                            }
                        });
                    });
            }
            ActiveDialog::ConfirmEdoChange {
                new_edo,
                new_step_multiplier,
            } => {
                egui::Window::new("Fret pattern has changed")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(ctx, |ui| {
                        ui.label("Do you want to translate the current music to this new EDO or start a new tab?");
                        ui.horizontal(|ui| {
                            if ui.button(format!("Translate to {} EDO", new_edo)).clicked() {
                                apply_action = Some(DialogAction::TranslateEdo { new_edo: *new_edo, new_step_multiplier: *new_step_multiplier });
                                close_dialog = true;
                            }
                            if ui.button("Start New Tab").clicked() {
                                // For simplicity, we just use the original tunings with the new EDO if starting a new tab from here
                                let part = self.parsed_mnx.as_ref().and_then(|d| d.parts.first());
                                let tunings = part.and_then(|p| p.instrument.as_ref()).and_then(|i| i.tuning.clone()).unwrap_or_default();
                                let num_strings = part.map(|p| p.num_strings()).unwrap_or(6) as u8;
                                apply_action = Some(DialogAction::StartNewTab { num_strings, edo: *new_edo, step_multiplier: *new_step_multiplier, tunings });
                                close_dialog = true;
                            }
                            if ui.button("Cancel").clicked() {
                                close_dialog = true;
                            }
                        });
                    });
            }
            ActiveDialog::BendEditor {
                event_idx,
                string,
                curve_editor,
                bend_range_steps: _,
            } => {
                egui::Window::new(format!(
                    "Bend Editor - Note {} on String {}",
                    *event_idx + 1,
                    string
                ))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "Max Bend ({} semitones, set via MIDI config)",
                            self.pitch_bend_range
                        ));
                    });
                    ui.add_space(8.0);

                    let rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(400.0, 200.0));
                    let pb_range = self.pitch_bend_range;
                    let mfd = self
                        .parsed_mnx
                        .as_ref()
                        .and_then(|d| d.parts.first())
                        .and_then(|p| p.instrument.as_ref())
                        .and_then(|i| i.micro_fret_divisions)
                        .unwrap_or(12) as u32;
                    curve_editor.ui(ui, rect, pb_range, mfd);

                    ui.add_space(20.0); // manual space since ui() allocates manually
                    ui.horizontal(|ui| {
                        if ui.button("Apply").clicked() {
                            let bend_str = curve_editor.to_svg_path_normalized();
                            apply_action = Some(DialogAction::ApplyBend {
                                event_idx: *event_idx,
                                string: *string,
                                bend_str,
                            });
                            close_dialog = true;
                        }
                        if ui.button("Clear Bend").clicked() {
                            apply_action = Some(DialogAction::ApplyBend {
                                event_idx: *event_idx,
                                string: *string,
                                bend_str: String::new(),
                            });
                            close_dialog = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close_dialog = true;
                        }
                    });
                });
            }
            ActiveDialog::ConfirmTuningChange {
                new_num_strings,
                new_tunings,
            } => {
                egui::Window::new("String tuning has changed")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(ctx, |ui| {
                        ui.label(
                            "Convert your existing music to this new tuning, or start a new tab?",
                        );
                        ui.horizontal(|ui| {
                            if ui.button("Convert to new string tuning").clicked() {
                                apply_action = Some(DialogAction::TranslateTuning {
                                    new_tunings: new_tunings.clone(),
                                });
                                close_dialog = true;
                            }
                            if ui.button("Start New Tab").clicked() {
                                let part = self.parsed_mnx.as_ref().and_then(|d| d.parts.first());
                                let edo = part.map(|p| p.edo()).unwrap_or(12) as u8;
                                let step_multiplier =
                                    part.map(|p| p.step_multiplier()).unwrap_or(1) as u8;
                                apply_action = Some(DialogAction::StartNewTab {
                                    num_strings: *new_num_strings,
                                    edo,
                                    step_multiplier,
                                    tunings: new_tunings.clone(),
                                });
                                close_dialog = true;
                            }
                            if ui.button("Cancel").clicked() {
                                close_dialog = true;
                            }
                        });
                    });
            }
            ActiveDialog::NaturalHarmonicPrompt {
                event_idx,
                string,
                touch_buf,
            } => {
                let event_idx_val = *event_idx;
                let string_val = *string;

                let part = self.parsed_mnx.as_ref().and_then(|d| d.parts.first());
                let table = part.map(|p| crate::harmonic::HarmonicLookupTable::from_part(p));

                egui::Window::new("Natural Harmonic")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(ctx, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                        ui.horizontal(|ui| {
                            ui.label("Touch String at:");
                            ui.add(egui::TextEdit::singleline(touch_buf).desired_width(70.0));
                        });

                        // Real-time lookup of harmonic node
                        let parsed_touch = touch_buf.trim().parse::<f64>();
                        let mut matched_node: Option<crate::harmonic::HarmonicNode> = None;
                        if let (Ok(touch_val), Some(tbl)) = (&parsed_touch, &table) {
                            let matching = tbl.find_nodes_at_touch(string_val, *touch_val);
                            if !matching.is_empty() {
                                matched_node = Some(matching[0].clone());
                            }
                        }

                        if let Some(node) = &matched_node {
                            let alter_str = node
                                .sounding_pitch
                                .alter
                                .as_ref()
                                .map(|a| {
                                    let v = a.as_semitones();
                                    if v >= 0.0 {
                                        format!(" (+{:.3})", v)
                                    } else {
                                        format!(" ({:.3})", v)
                                    }
                                })
                                .unwrap_or_default();

                            ui.label(
                                egui::RichText::new(format!(
                                    "Harmonic: {}th Harmonic ({}/{} node)",
                                    node.harmonic_number, node.node_index, node.harmonic_number
                                ))
                                .color(egui::Color32::from_rgb(0, 180, 80)),
                            );
                            ui.label(
                                egui::RichText::new(format!(
                                    "Sounding Note: {}{}{}",
                                    node.sounding_pitch.step, node.sounding_pitch.octave, alter_str
                                ))
                                .strong(),
                            );
                        } else if !touch_buf.trim().is_empty() {
                            ui.label(
                                egui::RichText::new(format!(
                                    "Not a valid harmonic location on string {}.",
                                    string_val
                                ))
                                .color(egui::Color32::RED),
                            );
                        } else {
                            ui.label(
                                egui::RichText::new("Enter a decimal fret position (e.g. 3.9, 2.7, 7.0)")
                                    .weak(),
                            );
                        }

                        ui.separator();
                        ui.horizontal(|ui| {
                            let can_ok = matched_node.is_some();
                            let ok_btn = ui.add_enabled(can_ok, egui::Button::new("Ok"));
                            if (ok_btn.clicked()
                                || (can_ok && ui.input(|i| i.key_pressed(egui::Key::Enter))))
                                && matched_node.is_some()
                            {
                                let node = matched_node.unwrap();
                                apply_action = Some(DialogAction::ApplyNaturalHarmonic {
                                    event_idx: event_idx_val,
                                    string: string_val,
                                    touch: node.touch_fret,
                                    sounding_pitch: node.sounding_pitch.clone(),
                                });
                                close_dialog = true;
                            }

                            if ui.button("Cancel").clicked()
                                || ui.input(|i| i.key_pressed(egui::Key::Escape))
                            {
                                close_dialog = true;
                            }
                        });
                    });
            }
        }

        if close_dialog {
            self.active_dialog = ActiveDialog::None;
        }

        if let Some(action) = apply_action {
            match action {

                DialogAction::InsertTempoChange { event_idx, end_bpm, dur_beats, bend_str, existing_target_id } => {
                    self.action_insert_tempo_change(event_idx, end_bpm, dur_beats, &bend_str, existing_target_id);
                    self.process_and_render();
                }
                DialogAction::RemoveTempoChange { target_id } => {
                    self.action_remove_tempo_change(&target_id);
                    self.process_and_render();
                }
                DialogAction::InsertTempo { bpm, duration_value, dots, event_idx } => {
                    self.insert_tempo(bpm, &duration_value, dots, event_idx);
                    self.process_and_render();
                }
                DialogAction::ApplyBend {
                    event_idx,
                    string,
                    bend_str,
                } => {
                    self.update_yaml(crate::yaml_editor::set_note_bend_in_yaml(
                        &self.yaml_text,
                        event_idx,
                        string,
                        &bend_str,
                    ));
                    self.process_and_render();
                }
                DialogAction::InsertTimeSignature {
                    count,
                    unit,
                    m_idx,
                    rebar_mode,
                } => {
                    self.insert_time_signature(m_idx, count, unit, rebar_mode);
                }
                DialogAction::CreateTuplet {
                    inner,
                    outer,
                    base,
                    from_select_mode,
                    event_idx,
                } => {
                    if let Some(new_yaml) = crate::yaml_editor::create_tuplet_in_yaml(
                        &self.yaml_text,
                        event_idx,
                        inner,
                        outer,
                        &base,
                    ) {
                        self.update_yaml(new_yaml);
                        self.process_and_render();
                        self.selected_event_idx = Some(event_idx);
                        if from_select_mode {
                            self.is_editing_fret = true;
                            let cur_str =
                                self.find_primary_string_for_event(event_idx).unwrap_or(1);
                            self.editing_string = Some(cur_str);
                            self.select_note_on_string(event_idx, cur_str);
                            self.editing_fret_buf.clear();
                        }
                    }
                }
                DialogAction::RemoveTuplet { event_idx } => {
                    if let Some(new_yaml) =
                        crate::yaml_editor::remove_tuplet_in_yaml(&self.yaml_text, event_idx)
                    {
                        self.update_yaml(new_yaml);
                        self.process_and_render();
                        self.selected_event_idx = Some(event_idx);
                    }
                }
                DialogAction::ApplyInstrumentSetup {
                    num_strings,
                    edo,
                    step_multiplier,
                    tunings_text,
                    tunings_octave,
                    is_new_tab,
                } => {
                    let part = self.parsed_mnx.as_ref().and_then(|d| d.parts.first());
                    let old_num_strings = part.map(|p| p.num_strings()).unwrap_or(6) as u8;
                    let old_edo = part.map(|p| p.edo()).unwrap_or(12) as u8;
                    let old_step = part.map(|p| p.step_multiplier()).unwrap_or(1) as u8;

                    let mut tunings = Vec::new();
                    let edo_val = edo as u32;
                    let tuning_calc = crate::edo::EdoTuning::new(edo_val);
                    for s in 0..num_strings as usize {
                        let text = &tunings_text[s];
                        let pitch = if let Some(p) =
                            tuning_calc.parse_pitch_string(text, tunings_octave[s])
                        {
                            p
                        } else {
                            // Invalid string name
                            self.active_dialog = ActiveDialog::Message {
                                title: "Invalid String".to_string(),
                                text: format!("String {} has an invalid name.", s + 1),
                            };
                            return;
                        };
                        tunings.push(crate::mnx::StringTuning {
                            string: (s + 1) as u8,
                            pitch,
                        });
                    }

                    if is_new_tab {
                        self.start_new_tab(num_strings, edo, step_multiplier, tunings);
                    } else if edo != old_edo || step_multiplier != old_step {
                        self.active_dialog = ActiveDialog::ConfirmEdoChange {
                            new_edo: edo,
                            new_step_multiplier: step_multiplier,
                        };
                    } else if num_strings != old_num_strings
                        || part
                            .map(|p| p.instrument.as_ref().and_then(|i| i.tuning.as_ref()))
                            .flatten()
                            != Some(&tunings)
                    {
                        self.active_dialog = ActiveDialog::ConfirmTuningChange {
                            new_num_strings: num_strings,
                            new_tunings: tunings,
                        };
                    } else {
                        self.active_dialog = ActiveDialog::None;
                    }
                }
                DialogAction::TranslateEdo {
                    new_edo,
                    new_step_multiplier,
                } => {
                    let mut doc = self.parsed_mnx.clone().unwrap();
                    let old_edo = doc.parts[0].edo() as u32;
                    let old_tuning = crate::edo::EdoTuning::new(old_edo);
                    let new_tuning = crate::edo::EdoTuning::new(new_edo as u32);

                    let part_clone = doc.parts[0].clone();
                    for m in &mut doc.parts[0].measures {
                        for seq in &mut m.sequences {
                            for item in &mut seq.content {
                                match item {
                                    crate::mnx::SequenceItem::Event(ev) => {
                                        let mut used_strings = std::collections::HashSet::new();
                                        for note in &ev.notes {
                                            if let Some(s) = note.string {
                                                used_strings.insert(s);
                                            }
                                        }

                                        for note in &mut ev.notes {
                                            if let (Some(string), Some(_fret)) =
                                                (note.string, note.fret)
                                            {
                                                let target_steps =
                                                    old_tuning.pitch_to_steps(&note.pitch);
                                                let old_hz =
                                                    old_tuning.step_semitones * target_steps as f64;
                                                let new_target_steps =
                                                    (old_hz / new_tuning.step_semitones).round()
                                                        as i32;
                                                note.pitch = 
                                                    new_tuning.steps_to_pitch(new_target_steps);

                                                if let Some(st_pitch) =
                                                    part_clone.get_string_tuning(string)
                                                {
                                                    let base_steps =
                                                        new_tuning.pitch_to_steps(&st_pitch);
                                                    let diff = new_target_steps - base_steps;
                                                    let mut new_fret = (diff as f32
                                                        / new_step_multiplier as f32)
                                                        .round()
                                                        as i32;
                                                    let mut best_string = string;

                                                    if new_fret < 0 {
                                                        used_strings.remove(&string);
                                                        // try to find a better string
                                                        if let Some(tuning) = part_clone
                                                            .instrument
                                                            .as_ref()
                                                            .and_then(|i| i.tuning.as_ref())
                                                        {
                                                            for t in tuning {
                                                                if !used_strings.contains(&t.string)
                                                                {
                                                                    let alt_base = new_tuning
                                                                        .pitch_to_steps(&t.pitch);
                                                                    let alt_diff =
                                                                        new_target_steps - alt_base;
                                                                    let alt_fret = (alt_diff as f32
                                                                        / new_step_multiplier
                                                                            as f32)
                                                                        .round()
                                                                        as i32;
                                                                    if alt_fret >= 0 {
                                                                        best_string = t.string;
                                                                        new_fret = alt_fret;
                                                                        break;
                                                                    }
                                                                }
                                                            }
                                                        }
                                                        used_strings.insert(best_string);
                                                    }

                                                    note.string = Some(best_string);
                                                    note.fret = Some(new_fret);
                                                }
                                            }
                                        }
                                    }
                                    crate::mnx::SequenceItem::Tuplet(tup) => {
                                        for seq_item in &mut tup.content {
                                            if let crate::mnx::SequenceItem::Event(ev) = seq_item {
                                                let mut used_strings =
                                                    std::collections::HashSet::new();
                                                for note in &ev.notes {
                                                    if let Some(s) = note.string {
                                                        used_strings.insert(s);
                                                    }
                                                }

                                                for note in &mut ev.notes {
                                                    if let (Some(string), Some(_fret)) =
                                                        (note.string, note.fret)
                                                    {
                                                        let target_steps =
                                                            old_tuning.pitch_to_steps(&note.pitch);
                                                        let old_hz = old_tuning.step_semitones
                                                            * target_steps as f64;
                                                        let new_target_steps = (old_hz
                                                            / new_tuning.step_semitones)
                                                            .round()
                                                            as i32;
                                                        note.pitch =  new_tuning
                                                            .steps_to_pitch(new_target_steps);

                                                        if let Some(st_pitch) =
                                                            part_clone.get_string_tuning(string)
                                                        {
                                                            let base_steps = new_tuning
                                                                .pitch_to_steps(&st_pitch);
                                                            let diff =
                                                                new_target_steps - base_steps;
                                                            let mut new_fret = (diff as f32
                                                                / new_step_multiplier as f32)
                                                                .round()
                                                                as i32;
                                                            let mut best_string = string;

                                                            if new_fret < 0 {
                                                                used_strings.remove(&string);
                                                                if let Some(tuning) = part_clone
                                                                    .instrument
                                                                    .as_ref()
                                                                    .and_then(|i| i.tuning.as_ref())
                                                                {
                                                                    for t in tuning {
                                                                        if !used_strings
                                                                            .contains(&t.string)
                                                                        {
                                                                            let alt_base =
                                                                                new_tuning
                                                                                    .pitch_to_steps(
                                                                                        &t.pitch,
                                                                                    );
                                                                            let alt_diff =
                                                                                new_target_steps
                                                                                    - alt_base;
                                                                            let alt_fret = (alt_diff as f32 / new_step_multiplier as f32).round() as i32;
                                                                            if alt_fret >= 0 {
                                                                                best_string =
                                                                                    t.string;
                                                                                new_fret = alt_fret;
                                                                                break;
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                                used_strings.insert(best_string);
                                                            }

                                                            note.string = Some(best_string);
                                                            note.fret = Some(new_fret);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    crate::mnx::SequenceItem::Grace(tup) => {
                                        for seq_item in &mut tup.content {
                                            if let crate::mnx::SequenceItem::Event(ev) = seq_item {
                                                let mut used_strings =
                                                    std::collections::HashSet::new();
                                                for note in &ev.notes {
                                                    if let Some(s) = note.string {
                                                        used_strings.insert(s);
                                                    }
                                                }

                                                for note in &mut ev.notes {
                                                    if let (Some(string), Some(_fret)) =
                                                        (note.string, note.fret)
                                                    {
                                                        let target_steps =
                                                            old_tuning.pitch_to_steps(&note.pitch);
                                                        let old_hz = old_tuning.step_semitones
                                                            * target_steps as f64;
                                                        let new_target_steps = (old_hz
                                                            / new_tuning.step_semitones)
                                                            .round()
                                                            as i32;
                                                        note.pitch =  new_tuning
                                                            .steps_to_pitch(new_target_steps);

                                                        if let Some(st_pitch) =
                                                            part_clone.get_string_tuning(string)
                                                        {
                                                            let base_steps = new_tuning
                                                                .pitch_to_steps(&st_pitch);
                                                            let diff =
                                                                new_target_steps - base_steps;
                                                            let mut new_fret = (diff as f32
                                                                / new_step_multiplier as f32)
                                                                .round()
                                                                as i32;
                                                            let mut best_string = string;

                                                            if new_fret < 0 {
                                                                used_strings.remove(&string);
                                                                if let Some(tuning) = part_clone
                                                                    .instrument
                                                                    .as_ref()
                                                                    .and_then(|i| i.tuning.as_ref())
                                                                {
                                                                    for t in tuning {
                                                                        if !used_strings
                                                                            .contains(&t.string)
                                                                        {
                                                                            let alt_base =
                                                                                new_tuning
                                                                                    .pitch_to_steps(
                                                                                        &t.pitch,
                                                                                    );
                                                                            let alt_diff =
                                                                                new_target_steps
                                                                                    - alt_base;
                                                                            let alt_fret = (alt_diff as f32 / new_step_multiplier as f32).round() as i32;
                                                                            if alt_fret >= 0 {
                                                                                best_string =
                                                                                    t.string;
                                                                                new_fret = alt_fret;
                                                                                break;
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                                used_strings.insert(best_string);
                                                            }

                                                            note.string = Some(best_string);
                                                            note.fret = Some(new_fret);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    doc.parts[0]
                        .instrument
                        .as_mut()
                        .unwrap()
                        .micro_fret_divisions = Some(new_edo);
                    doc.parts[0]
                        .instrument
                        .as_mut()
                        .unwrap()
                        .fret_step_multiplier = Some(new_step_multiplier);
                    if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
                        self.update_yaml(new_yaml);
                        self.process_and_render();
                    }
                }
                DialogAction::TranslateTuning { new_tunings } => {
                    let mut doc = self.parsed_mnx.clone().unwrap();
                    let edo = doc.parts[0].edo() as u32;
                    let step_mult = doc.parts[0].step_multiplier() as i32;
                    let tuning_calc = crate::edo::EdoTuning::new(edo);

                    doc.parts[0].instrument.as_mut().unwrap().tuning = Some(new_tunings.clone());
                    // we don't strictly need to set num_strings unless we had a property for it, tuning size implies it

                    let part_clone = doc.parts[0].clone();
                    for m in &mut doc.parts[0].measures {
                        for seq in &mut m.sequences {
                            for item in &mut seq.content {
                                match item {
                                    crate::mnx::SequenceItem::Event(ev) => {
                                        let mut used_strings = std::collections::HashSet::new();
                                        for note in &ev.notes {
                                            if let Some(s) = note.string {
                                                used_strings.insert(s);
                                            }
                                        }

                                        for note in &mut ev.notes {
                                            if let (Some(string), Some(_fret)) =
                                                (note.string, note.fret)
                                            {
                                                let target_steps =
                                                    tuning_calc.pitch_to_steps(&note.pitch);
                                                if let Some(st_pitch) =
                                                    part_clone.get_string_tuning(string)
                                                {
                                                    let base_steps =
                                                        tuning_calc.pitch_to_steps(&st_pitch);
                                                    let diff = target_steps - base_steps;
                                                    let mut new_fret =
                                                        (diff as f32 / step_mult as f32).round()
                                                            as i32;
                                                    let mut best_string = string;

                                                    if new_fret < 0 {
                                                        used_strings.remove(&string);
                                                        if let Some(tuning) = part_clone
                                                            .instrument
                                                            .as_ref()
                                                            .and_then(|i| i.tuning.as_ref())
                                                        {
                                                            for t in tuning {
                                                                if !used_strings.contains(&t.string)
                                                                {
                                                                    let alt_base = tuning_calc
                                                                        .pitch_to_steps(&t.pitch);
                                                                    let alt_diff =
                                                                        target_steps - alt_base;
                                                                    let alt_fret = (alt_diff as f32
                                                                        / step_mult as f32)
                                                                        .round()
                                                                        as i32;
                                                                    if alt_fret >= 0 {
                                                                        best_string = t.string;
                                                                        new_fret = alt_fret;
                                                                        break;
                                                                    }
                                                                }
                                                            }
                                                        }
                                                        used_strings.insert(best_string);
                                                    }

                                                    note.string = Some(best_string);
                                                    note.fret = Some(new_fret);
                                                }
                                            }
                                        }
                                    }
                                    crate::mnx::SequenceItem::Tuplet(tup) => {
                                        for seq_item in &mut tup.content {
                                            if let crate::mnx::SequenceItem::Event(ev) = seq_item {
                                                let mut used_strings =
                                                    std::collections::HashSet::new();
                                                for note in &ev.notes {
                                                    if let Some(s) = note.string {
                                                        used_strings.insert(s);
                                                    }
                                                }

                                                for note in &mut ev.notes {
                                                    if let (Some(string), Some(_fret)) =
                                                        (note.string, note.fret)
                                                    {
                                                        let target_steps =
                                                            tuning_calc.pitch_to_steps(&note.pitch);
                                                        if let Some(st_pitch) =
                                                            part_clone.get_string_tuning(string)
                                                        {
                                                            let base_steps = tuning_calc
                                                                .pitch_to_steps(&st_pitch);
                                                            let diff = target_steps - base_steps;
                                                            let mut new_fret = (diff as f32
                                                                / step_mult as f32)
                                                                .round()
                                                                as i32;
                                                            let mut best_string = string;

                                                            if new_fret < 0 {
                                                                used_strings.remove(&string);
                                                                if let Some(tuning) = part_clone
                                                                    .instrument
                                                                    .as_ref()
                                                                    .and_then(|i| i.tuning.as_ref())
                                                                {
                                                                    for t in tuning {
                                                                        if !used_strings
                                                                            .contains(&t.string)
                                                                        {
                                                                            let alt_base =
                                                                                tuning_calc
                                                                                    .pitch_to_steps(
                                                                                        &t.pitch,
                                                                                    );
                                                                            let alt_diff =
                                                                                target_steps
                                                                                    - alt_base;
                                                                            let alt_fret = (alt_diff
                                                                                as f32
                                                                                / step_mult as f32)
                                                                                .round()
                                                                                as i32;
                                                                            if alt_fret >= 0 {
                                                                                best_string =
                                                                                    t.string;
                                                                                new_fret = alt_fret;
                                                                                break;
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                                used_strings.insert(best_string);
                                                            }

                                                            note.string = Some(best_string);
                                                            note.fret = Some(new_fret);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    crate::mnx::SequenceItem::Grace(tup) => {
                                        for seq_item in &mut tup.content {
                                            if let crate::mnx::SequenceItem::Event(ev) = seq_item {
                                                let mut used_strings =
                                                    std::collections::HashSet::new();
                                                for note in &ev.notes {
                                                    if let Some(s) = note.string {
                                                        used_strings.insert(s);
                                                    }
                                                }

                                                for note in &mut ev.notes {
                                                    if let (Some(string), Some(_fret)) =
                                                        (note.string, note.fret)
                                                    {
                                                        let target_steps =
                                                            tuning_calc.pitch_to_steps(&note.pitch);
                                                        if let Some(st_pitch) =
                                                            part_clone.get_string_tuning(string)
                                                        {
                                                            let base_steps = tuning_calc
                                                                .pitch_to_steps(&st_pitch);
                                                            let diff = target_steps - base_steps;
                                                            let mut new_fret = (diff as f32
                                                                / step_mult as f32)
                                                                .round()
                                                                as i32;
                                                            let mut best_string = string;

                                                            if new_fret < 0 {
                                                                used_strings.remove(&string);
                                                                if let Some(tuning) = part_clone
                                                                    .instrument
                                                                    .as_ref()
                                                                    .and_then(|i| i.tuning.as_ref())
                                                                {
                                                                    for t in tuning {
                                                                        if !used_strings
                                                                            .contains(&t.string)
                                                                        {
                                                                            let alt_base =
                                                                                tuning_calc
                                                                                    .pitch_to_steps(
                                                                                        &t.pitch,
                                                                                    );
                                                                            let alt_diff =
                                                                                target_steps
                                                                                    - alt_base;
                                                                            let alt_fret = (alt_diff
                                                                                as f32
                                                                                / step_mult as f32)
                                                                                .round()
                                                                                as i32;
                                                                            if alt_fret >= 0 {
                                                                                best_string =
                                                                                    t.string;
                                                                                new_fret = alt_fret;
                                                                                break;
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                                used_strings.insert(best_string);
                                                            }

                                                            note.string = Some(best_string);
                                                            note.fret = Some(new_fret);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
                        self.update_yaml(new_yaml);
                        self.process_and_render();
                    }
                }
                DialogAction::StartNewTab {
                    num_strings,
                    edo,
                    step_multiplier,
                    tunings,
                } => {
                    self.start_new_tab(num_strings, edo, step_multiplier, tunings);
                }
                DialogAction::DiscardChangesSave => {
                    if self.save_file() {
                        self.prompt_new_tab_instrument_setup();
                    }
                }
                DialogAction::DiscardChangesSaveAs => {
                    if self.save_as_file() {
                        self.prompt_new_tab_instrument_setup();
                    }
                }
                DialogAction::DiscardChangesProceed => {
                    self.prompt_new_tab_instrument_setup();
                }
                DialogAction::ApplyNaturalHarmonic {
                    event_idx,
                    string,
                    touch,
                    sounding_pitch,
                } => {
                    let mut doc = match self.parsed_mnx.clone() {
                        Some(d) => d,
                        None => return,
                    };
                    let flattened = doc.get_flattened_event_paths();
                    if event_idx < flattened.len() {
                        let cur_path = flattened[event_idx];
                        if let Some(ev) = doc.get_event_mut_at_path(cur_path) {
                            let note_idx = ev
                                .notes
                                .iter()
                                .position(|n| n.string == Some(string))
                                .or_else(|| {
                                    if ev.notes.len() == 1 {
                                        Some(0)
                                    } else {
                                        None
                                    }
                                });

                            if let Some(idx) = note_idx {
                                let note = &mut ev.notes[idx];
                                if note.string.is_none() {
                                    note.string = Some(string);
                                }
                                note.fret = Some(note.fret.unwrap_or(touch.round() as i32));
                                note.harmonic = Some(crate::mnx::Harmonic {
                                    touch,
                                    sounding_note: Box::new(crate::mnx::Note {
                                        muted: None,
                                        id: None,
                                        pitch: sounding_pitch,
                                        string: None,
                                        fret: None,
                                        accidental: None,
                                        ties: None,
                                        tap: None,
                                        slide: None,
                                        bend: None,
                                        harmonic: None,
                                    }),
                                });
                            }
                        }
                        if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
                            self.update_yaml(new_yaml);
                            self.process_and_render();
                        }
                    }
                }
            }
        }
    }
}
