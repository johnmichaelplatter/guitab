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
            TranslateInstrumentSetup {
                new_num_strings: u8,
                new_edo: u8,
                new_step_multiplier: u8,
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
            InsertEnding {
                start_m: usize,
                end_m: usize,
                times: u32,
            },
        }

        let mut close_dialog = false;
        let mut apply_action = None;
        let mut lyric_action: Option<(usize, usize, String, Option<String>, Option<bool>)> = None;
        let mut lyric_advance: Option<(usize, usize, bool)> = None;

        match &mut self.active_dialog {
            ActiveDialog::None => {}
            ActiveDialog::Manual => {
                egui::Window::new("User Manual")
                    .collapsible(false)
                    .resizable(true)
                    .default_width(480.0)
                    .show(ctx, |ui| {
                        ui.heading("MNX Tablature Editor — Manual");
                        ui.separator();
                        ui.label("Documentation placeholder — full user manual coming soon.");
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("Quick Shortcuts:").strong());
                        ui.label("• Arrow Keys: Move across events and strings");
                        ui.label("• Enter / Number Keys: Edit fret on selected string");
                        ui.label("• 1–6: Set note duration (Whole to 32nd)");
                        ui.label("• . (Period): Toggle dotted duration");
                        ui.label("• R: Convert event to rest");
                        ui.label("• Shift+L: Open lyric syllable editor");
                        ui.label("• Space (in Select mode): Play / Stop MIDI");
                        ui.add_space(12.0);
                        let esc_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));
                        if ui.button("Close").clicked() || esc_pressed {
                            close_dialog = true;
                        }
                    });
            }
            ActiveDialog::About => {
                egui::Window::new("About")
                    .collapsible(false)
                    .resizable(false)
                    .default_width(360.0)
                    .show(ctx, |ui| {
                        ui.heading("MNX Tablature Editor");
                        ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                        ui.separator();
                        ui.label(
                            "A microtonal-ready guitar & fretted instrument tablature editor built on the W3C MNX specification with YAML editing, MPE/multi-channel MIDI playback, and PDF/MNX export.",
                        );
                        ui.add_space(12.0);
                        let esc_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));
                        if ui.button("Close").clicked() || esc_pressed {
                            close_dialog = true;
                        }
                    });
            }
            ActiveDialog::InsertEnding {
                start_m,
                end_m,
                times_str,
            } => {
                let start_m_val = *start_m;
                let end_m_val = *end_m;

                egui::Window::new("Insert Ending")
                    .collapsible(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label("How many times will this ending be played?");
                        ui.add(egui::TextEdit::singleline(times_str).desired_width(60.0));

                        let parsed_times = times_str.trim().parse::<u32>();
                        let is_valid = match parsed_times {
                            Ok(t) => t >= 1 && t <= 15,
                            Err(_) => false,
                        };

                        ui.horizontal(|ui| {
                            let ok_btn = ui.add_enabled(is_valid, egui::Button::new("OK"));
                            let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if (ok_btn.clicked() || enter_pressed) && is_valid {
                                let times = parsed_times.unwrap();
                                apply_action = Some(DialogAction::InsertEnding {
                                    start_m: start_m_val,
                                    end_m: end_m_val,
                                    times,
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
                new_num_strings,
                new_edo,
                new_step_multiplier,
                new_tunings,
            } => {
                egui::Window::new("Fret pattern has changed")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(ctx, |ui| {
                        ui.label("Do you want to translate the current music to this new EDO or start a new tab?");
                        ui.horizontal(|ui| {
                            if ui.button(format!("Translate to {} EDO", new_edo)).clicked() {
                                apply_action = Some(DialogAction::TranslateInstrumentSetup {
                                    new_num_strings: *new_num_strings,
                                    new_edo: *new_edo,
                                    new_step_multiplier: *new_step_multiplier,
                                    new_tunings: new_tunings.clone(),
                                });
                                close_dialog = true;
                            }
                            if ui.button("Start New Tab").clicked() {
                                apply_action = Some(DialogAction::StartNewTab {
                                    num_strings: *new_num_strings,
                                    edo: *new_edo,
                                    step_multiplier: *new_step_multiplier,
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
                        .and_then(|p| p.micro_fret_divisions)
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
                new_edo,
                new_step_multiplier,
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
                                apply_action = Some(DialogAction::TranslateInstrumentSetup {
                                    new_num_strings: *new_num_strings,
                                    new_edo: *new_edo,
                                    new_step_multiplier: *new_step_multiplier,
                                    new_tunings: new_tunings.clone(),
                                });
                                close_dialog = true;
                            }
                            if ui.button("Start New Tab").clicked() {
                                apply_action = Some(DialogAction::StartNewTab {
                                    num_strings: *new_num_strings,
                                    edo: *new_edo,
                                    step_multiplier: *new_step_multiplier,
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
            ActiveDialog::LyricEditor {
                event_idx,
                line_idx,
                syllable_buf,
                in_word,
            } => {
                let curr_ev = *event_idx;
                let curr_line = *line_idx;
                let total_events = self
                    .parsed_mnx
                    .as_ref()
                    .map(|d| d.get_flattened_event_paths().len())
                    .unwrap_or(0);

                egui::Window::new(format!("Lyrics - Line {}", curr_line))
                    .collapsible(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("Event {} / {}", curr_ev + 1, total_events))
                                    .strong(),
                            );
                            ui.label(
                                egui::RichText::new(format!("(Line {})", curr_line)).italics(),
                            );
                        });

                        ui.add_space(4.0);

                        let text_edit_id = ui.make_persistent_id("lyric_syllable_input");
                        let text_edit = egui::TextEdit::singleline(syllable_buf)
                            .id(text_edit_id)
                            .desired_width(180.0)
                            .hint_text("Syllable...");

                        let resp = ui.add(text_edit);
                        resp.request_focus();

                        ui.add_space(2.0);
                        ui.label(
                            egui::RichText::new(
                                "Space: Next | -: Hyphenate | _: Extend | ↑/↓: Line",
                            )
                            .size(10.5)
                            .color(ui.visuals().weak_text_color()),
                        );

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            let ok_btn = ui.button("OK");
                            let cancel_btn = ui.button("Cancel");

                            let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
                            let esc_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));
                            let up_pressed = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
                            let down_pressed = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));
                            let space_pressed = ui.input(|i| i.key_pressed(egui::Key::Space))
                                || syllable_buf.ends_with(' ');
                            let hyphen_typed = syllable_buf.ends_with('-');
                            let underscore_typed = syllable_buf.ends_with('_');

                            if esc_pressed || cancel_btn.clicked() {
                                close_dialog = true;
                            } else if enter_pressed || ok_btn.clicked() {
                                let clean = syllable_buf.trim();
                                let l_type = if *in_word && !clean.is_empty() {
                                    Some("end".to_string())
                                } else {
                                    None
                                };
                                lyric_action = Some((curr_ev, curr_line, clean.to_string(), l_type, None));
                                close_dialog = true;
                            } else if hyphen_typed {
                                while syllable_buf.ends_with('-') {
                                    syllable_buf.pop();
                                }
                                let clean = syllable_buf.trim();
                                if !clean.is_empty() {
                                    let l_type = if *in_word { "middle" } else { "start" };
                                    lyric_action = Some((
                                        curr_ev,
                                        curr_line,
                                        clean.to_string(),
                                        Some(l_type.to_string()),
                                        None,
                                    ));
                                }
                                let next_ev = curr_ev + 1;
                                if next_ev < total_events {
                                    lyric_advance = Some((next_ev, curr_line, true));
                                } else {
                                    close_dialog = true;
                                }
                            } else if underscore_typed {
                                while syllable_buf.ends_with('_') {
                                    syllable_buf.pop();
                                }
                                let clean = syllable_buf.trim();
                                let l_type = if *in_word && !clean.is_empty() {
                                    Some("end".to_string())
                                } else {
                                    None
                                };
                                lyric_action = Some((
                                    curr_ev,
                                    curr_line,
                                    clean.to_string(),
                                    l_type,
                                    Some(true),
                                ));
                                let next_ev = curr_ev + 1;
                                if next_ev < total_events {
                                    lyric_advance = Some((next_ev, curr_line, false));
                                } else {
                                    close_dialog = true;
                                }
                            } else if space_pressed {
                                while syllable_buf.ends_with(' ') {
                                    syllable_buf.pop();
                                }
                                let clean = syllable_buf.trim();
                                let l_type = if *in_word && !clean.is_empty() {
                                    Some("end".to_string())
                                } else {
                                    None
                                };
                                lyric_action = Some((
                                    curr_ev,
                                    curr_line,
                                    clean.to_string(),
                                    l_type,
                                    None,
                                ));
                                let next_ev = curr_ev + 1;
                                if next_ev < total_events {
                                    lyric_advance = Some((next_ev, curr_line, false));
                                } else {
                                    close_dialog = true;
                                }
                            } else if down_pressed {
                                let clean = syllable_buf.trim();
                                let l_type = if *in_word && !clean.is_empty() {
                                    Some("end".to_string())
                                } else {
                                    None
                                };
                                lyric_action = Some((
                                    curr_ev,
                                    curr_line,
                                    clean.to_string(),
                                    l_type,
                                    None,
                                ));
                                lyric_advance = Some((curr_ev, curr_line + 1, false));
                            } else if up_pressed && curr_line > 1 {
                                let clean = syllable_buf.trim();
                                let l_type = if *in_word && !clean.is_empty() {
                                    Some("end".to_string())
                                } else {
                                    None
                                };
                                lyric_action = Some((
                                    curr_ev,
                                    curr_line,
                                    clean.to_string(),
                                    l_type,
                                    None,
                                ));
                                lyric_advance = Some((curr_ev, curr_line - 1, false));
                            }
                        });
                    });
            }
        }

        if let Some((ev, line, text, l_type, ext)) = lyric_action {
            self.action_set_event_lyric_line(ev, line, &text, l_type, ext);
        }

        if close_dialog {
            self.active_dialog = ActiveDialog::None;
        } else if let Some((next_ev, next_line, next_in_word)) = lyric_advance {
            self.selected_event_idx = Some(next_ev);
            self.needs_scroll = true;
            let mut loaded_text = String::new();
            let mut loaded_in_word = next_in_word;
            if let Some(lyr) = self.get_event_lyric_line(next_ev, next_line) {
                loaded_text = lyr.text.clone();
                if matches!(lyr.line_type.as_deref(), Some("start") | Some("middle")) {
                    loaded_in_word = true;
                }
            }
            self.active_dialog = ActiveDialog::LyricEditor {
                event_idx: next_ev,
                line_idx: next_line,
                syllable_buf: loaded_text,
                in_word: loaded_in_word,
            };
        }

        if let Some(action) = apply_action {
            match action {

                DialogAction::InsertTempoChange { event_idx, end_bpm, dur_beats, bend_str, existing_target_id } => {
                    self.action_insert_tempo_change(event_idx, end_bpm, dur_beats, &bend_str, existing_target_id);
                }
                DialogAction::RemoveTempoChange { target_id } => {
                    self.action_remove_tempo_change(&target_id);
                }
                DialogAction::InsertTempo { bpm, duration_value, dots, event_idx } => {
                    self.insert_tempo(bpm, &duration_value, dots, event_idx);
                }
                DialogAction::ApplyBend {
                    event_idx,
                    string,
                    bend_str,
                } => {
                    self.mutate_doc(|doc| {
                        if crate::yaml_editor::set_note_bend_in_doc(doc, event_idx, string, &bend_str) {
                            Some(())
                        } else {
                            None
                        }
                    });
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
                    if self
                        .mutate_doc(|doc| {
                            if crate::yaml_editor::create_tuplet_in_doc(
                                doc,
                                event_idx,
                                inner,
                                outer,
                                &base,
                            ) {
                                Some(())
                            } else {
                                None
                            }
                        })
                        .is_some()
                    {
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
                    if self
                        .mutate_doc(|doc| {
                            if crate::yaml_editor::remove_tuplet_in_doc(doc, event_idx) {
                                Some(())
                            } else {
                                None
                            }
                        })
                        .is_some()
                    {
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
                            new_num_strings: num_strings,
                            new_edo: edo,
                            new_step_multiplier: step_multiplier,
                            new_tunings: tunings,
                        };
                    } else if num_strings != old_num_strings
                        || part.and_then(|p| p.tuning.as_ref()) != Some(&tunings)
                    {
                        self.active_dialog = ActiveDialog::ConfirmTuningChange {
                            new_num_strings: num_strings,
                            new_edo: edo,
                            new_step_multiplier: step_multiplier,
                            new_tunings: tunings,
                        };
                    } else {
                        self.active_dialog = ActiveDialog::None;
                    }
                }
                DialogAction::TranslateInstrumentSetup {
                    new_num_strings,
                    new_edo,
                    new_step_multiplier,
                    new_tunings,
                } => {
                    self.translate_instrument_setup(
                        new_num_strings,
                        new_edo,
                        new_step_multiplier,
                        new_tunings,
                    );
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
                                        id: None,
                                        pitch: sounding_pitch,
                                        accidental_display: None,
                                        ties: None,
                                        comment: None,
                                        ext: crate::mnx::NoteExt::default(),
                                    }),
                                });
                            }
                        }
                        self.apply_mutated_doc(doc);
                    }
                }
                DialogAction::InsertEnding {
                    start_m,
                    end_m,
                    times,
                } => {
                    let prev_max = if let Some(doc) = &self.parsed_mnx {
                        doc.get_prior_ending_max(start_m)
                    } else {
                        None
                    }
                    .unwrap_or(0);
                    let start_num = prev_max + 1;
                    let numbers: Vec<u32> = (start_num..(start_num + times)).collect();
                    let duration = (end_m - start_m + 1) as u32;
                    let ending_val =
                        format!("{{ duration: {}, numbers: {:?} }}", duration, numbers);
                    self.mutate_doc(|doc| {
                        crate::yaml_editor::set_global_measure_property_in_doc(
                            doc,
                            start_m,
                            "ending",
                            &ending_val,
                        );
                        crate::yaml_editor::set_global_measure_property_in_doc(
                            doc,
                            end_m,
                            "repeatEnd",
                            "{}",
                        );
                        Some(())
                    });
                }
            }
        }
    }

    pub(crate) fn translate_instrument_setup(
        &mut self,
        new_num_strings: u8,
        new_edo: u8,
        new_step_multiplier: u8,
        new_tunings: Vec<crate::mnx::StringTuning>,
    ) {
        let mut doc = match self.parsed_mnx.clone() {
            Some(d) => d,
            None => return,
        };
        if doc.parts.is_empty() {
            return;
        }

        let old_edo = doc.parts[0].edo() as u32;
        let old_tuning = crate::edo::EdoTuning::new(old_edo);
        let new_tuning = crate::edo::EdoTuning::new(new_edo as u32);
        let step_mult = (new_step_multiplier.max(1)) as i32;

        if doc.parts[0].name.is_none() {
            doc.parts[0].name = Some("Electric Guitar".to_string());
        }
        doc.parts[0].micro_fret_divisions = Some(new_edo);
        doc.parts[0].fret_step_multiplier = Some(new_step_multiplier);
        doc.parts[0].tuning = Some(new_tunings.clone());

        if let Some(s) = self.editing_string {
            if s > new_num_strings {
                self.editing_string = Some(new_num_strings.max(1));
            }
        }

        let part_clone = doc.parts[0].clone();

        fn translate_items(
            items: &mut [crate::mnx::SequenceItem],
            part_clone: &crate::mnx::Part,
            old_tuning: &crate::edo::EdoTuning,
            new_tuning: &crate::edo::EdoTuning,
            new_num_strings: u8,
            step_mult: i32,
            new_tunings: &[crate::mnx::StringTuning],
        ) {
            for item in items {
                match item {
                    crate::mnx::SequenceItem::Event(ev) => {
                        let mut used_strings = std::collections::HashSet::new();
                        for note in &ev.notes {
                            if let Some(s) = note.string {
                                if s <= new_num_strings {
                                    used_strings.insert(s);
                                }
                            }
                        }

                        for note in &mut ev.notes {
                            if let (Some(string), Some(_fret)) = (note.string, note.fret) {
                                let target_steps = old_tuning.pitch_to_steps(&note.pitch);
                                let old_semitones =
                                    old_tuning.step_semitones * target_steps as f64;
                                let new_target_steps =
                                    (old_semitones / new_tuning.step_semitones).round() as i32;

                                let mut best_string = string;
                                let mut new_fret = -1;

                                if string <= new_num_strings {
                                    if let Some(st_pitch) = part_clone.get_string_tuning(string) {
                                        let base_steps = new_tuning.pitch_to_steps(&st_pitch);
                                        let diff = new_target_steps - base_steps;
                                        new_fret =
                                            (diff as f32 / step_mult as f32).round() as i32;
                                    }
                                }

                                if new_fret < 0 {
                                    used_strings.remove(&string);
                                    let mut found = false;
                                    for t in new_tunings {
                                        if !used_strings.contains(&t.string) {
                                            let alt_base = new_tuning.pitch_to_steps(&t.pitch);
                                            let alt_diff = new_target_steps - alt_base;
                                            let alt_fret =
                                                (alt_diff as f32 / step_mult as f32).round() as i32;
                                            if alt_fret >= 0 {
                                                best_string = t.string;
                                                new_fret = alt_fret;
                                                found = true;
                                                break;
                                            }
                                        }
                                    }
                                    if !found {
                                        // Fallback: pick lowest-pitched unused string (or clamp string) at fret 0
                                        if let Some(t) = new_tunings
                                            .iter()
                                            .filter(|t| !used_strings.contains(&t.string))
                                            .min_by_key(|t| new_tuning.pitch_to_steps(&t.pitch))
                                        {
                                            best_string = t.string;
                                        } else {
                                            best_string = string.min(new_num_strings.max(1));
                                        }
                                        new_fret = 0;
                                    }
                                    used_strings.insert(best_string);
                                }

                                let final_fret = new_fret.max(0);
                                note.string = Some(best_string);
                                note.fret = Some(final_fret);

                                if let Some(final_st_pitch) =
                                    part_clone.get_string_tuning(best_string)
                                {
                                    let final_base_steps =
                                        new_tuning.pitch_to_steps(&final_st_pitch);
                                    let actual_steps = final_base_steps + final_fret * step_mult;
                                    note.pitch = new_tuning.steps_to_pitch(actual_steps);
                                } else {
                                    note.pitch = new_tuning.steps_to_pitch(new_target_steps);
                                }
                            } else if let Some(string) = note.string {
                                // Muted note or note without fret
                                let mut best_string = string;
                                if string > new_num_strings {
                                    best_string = (1..=new_num_strings)
                                        .rev()
                                        .find(|s| !used_strings.contains(s))
                                        .unwrap_or(new_num_strings.max(1));
                                    used_strings.insert(best_string);
                                    note.string = Some(best_string);
                                }
                                if let Some(st_pitch) = part_clone.get_string_tuning(best_string) {
                                    note.pitch = st_pitch;
                                }
                            }
                        }
                    }
                    crate::mnx::SequenceItem::Tuplet(tup) => {
                        translate_items(
                            &mut tup.content,
                            part_clone,
                            old_tuning,
                            new_tuning,
                            new_num_strings,
                            step_mult,
                            new_tunings,
                        );
                    }
                    crate::mnx::SequenceItem::Grace(g) => {
                        translate_items(
                            &mut g.content,
                            part_clone,
                            old_tuning,
                            new_tuning,
                            new_num_strings,
                            step_mult,
                            new_tunings,
                        );
                    }
                }
            }
        }

        for m in &mut doc.parts[0].measures {
            for seq in &mut m.sequences {
                translate_items(
                    &mut seq.content,
                    &part_clone,
                    &old_tuning,
                    &new_tuning,
                    new_num_strings,
                    step_mult,
                    &new_tunings,
                );
            }
        }

        self.apply_mutated_doc(doc);
    }
}
