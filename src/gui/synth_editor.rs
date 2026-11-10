//! Interactive Sound Design & Patch Editor UI (`src/gui/synth_editor.rs`).
//!
//! Provides real-time parameter tweaking with lock-free SPSC dispatch, visual frequency
//! response and envelope preview painting, factory preset selection, and JSON preset export/import.

use super::*;
use crate::synth::body::{BiquadFilter, InstrumentProfile};
use crate::synth::command::SynthCommand;
use crate::synth::patch::{
    AdsrEnvelopeParams, DrumRole, DrumVoiceParams, FmVoiceParams, StringSynthParams, SynthPreset,
};
use crate::synth::SynthEditorTab;
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};

/// Renders the complete Sound Design & Patch Editor floating window.
pub fn render_synth_editor_window(app: &mut MnxEditorApp, ctx: &egui::Context) {
    if !app.synth_lab.open {
        return;
    }

    let mut open = app.synth_lab.open;
    let mut commands_to_send: Vec<SynthCommand> = Vec::new();
    let mut reconnect_requested = false;
    let mut strum_open_requested = false;
    let mut export_json_requested = false;
    let mut import_json_requested = false;

    let engine_info = app
        .synth_engine
        .as_ref()
        .map(|e| (e.sample_rate(), e.channels()));

    let sample_rate = engine_info.map(|(sr, _)| sr).unwrap_or(48_000.0);

    egui::Window::new("Sound Design & Patch Editor (Synth Lab)")
        .open(&mut open)
        .resizable(true)
        .default_width(560.0)
        .default_height(680.0)
        .show(ctx, |ui| {
            // -------------------------------------------------------------
            // Top Audio Status & Panic Bar
            // -------------------------------------------------------------
            ui.horizontal(|ui| {
                if let Some((sr, ch)) = engine_info {
                    ui.colored_label(
                        Color32::from_rgb(90, 230, 140),
                        format!("Audio Stream: {:.0} Hz, {} ch (Zero-Lock SPSC)", sr, ch),
                    );
                } else {
                    ui.colored_label(
                        Color32::from_rgb(255, 190, 90),
                        "Audio stream inactive",
                    );
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("All Notes Off")
                        .on_hover_text("Send SynthCommand::AllNotesOff to immediately silence all voices")
                        .clicked()
                    {
                        commands_to_send.push(SynthCommand::AllNotesOff);
                    }
                    if ui
                        .small_button("Init / Reconnect")
                        .on_hover_text("Start or restart the real-time cpal stream")
                        .clicked()
                    {
                        reconnect_requested = true;
                    }
                });
            });

            if let Some(msg) = &app.synth_lab.status_msg {
                ui.label(egui::RichText::new(msg).size(11.0).italics().color(Color32::from_rgb(200, 200, 100)));
            }

            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut app.synth_lab.route_score_preview,
                    "Route Score Note Clicks & Tab Navigation Previews to Internal Synth",
                );
            });

            ui.separator();

            // -------------------------------------------------------------
            // 1. Preset Management & Master Level Bar
            // -------------------------------------------------------------
            render_preset_bar(
                ui,
                app,
                &mut commands_to_send,
                &mut export_json_requested,
                &mut import_json_requested,
            );

            ui.separator();

            // -------------------------------------------------------------
            // 2. Live Audition Strip (Zero-Latency Real-Time Triggers)
            // -------------------------------------------------------------
            render_audition_strip(ui, app, &mut commands_to_send, &mut strum_open_requested);

            ui.separator();

            // -------------------------------------------------------------
            // 3. Interactive Frequency / Envelope Preview Canvas
            // -------------------------------------------------------------
            render_visual_preview_canvas(ui, &app.synth_lab, sample_rate);

            ui.separator();

            // -------------------------------------------------------------
            // 4. Tabbed Synthesis Inspector
            // -------------------------------------------------------------
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut app.synth_lab.active_tab,
                    SynthEditorTab::StringAndPluck,
                    "1. String & Pluck (EKS)",
                );
                ui.selectable_value(
                    &mut app.synth_lab.active_tab,
                    SynthEditorTab::BodyAndAmp,
                    "2. Body & Amp Resonator",
                );
                ui.selectable_value(
                    &mut app.synth_lab.active_tab,
                    SynthEditorTab::DrumsAndCymbals,
                    "3. Drums & Cymbals",
                );
                ui.selectable_value(
                    &mut app.synth_lab.active_tab,
                    SynthEditorTab::GmFmSynth,
                    "4. GM FM Synth",
                );
            });

            ui.add_space(4.0);

            match app.synth_lab.active_tab {
                SynthEditorTab::StringAndPluck => {
                    render_string_tab(ui, app, &mut commands_to_send);
                }
                SynthEditorTab::BodyAndAmp => {
                    render_body_tab(ui, app, &mut commands_to_send);
                }
                SynthEditorTab::DrumsAndCymbals => {
                    render_drums_tab(ui, app, &mut commands_to_send);
                }
                SynthEditorTab::GmFmSynth => {
                    render_fm_tab(ui, app, &mut commands_to_send);
                }
            }
        });

    app.synth_lab.open = open;

    // Dispatch accumulated lock-free commands
    if !commands_to_send.is_empty() {
        let _ = app.ensure_synth_engine();
        if let Some(engine) = &mut app.synth_engine {
            for cmd in commands_to_send {
                engine.send(cmd);
            }
        }
    }

    if strum_open_requested {
        strum_current_part_open_strings(app);
    }

    if reconnect_requested {
        app.synth_engine = None;
        let _ = app.ensure_synth_engine();
    }

    if export_json_requested {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Export Synthesizer Preset")
            .add_filter("JSON Preset", &["json"])
            .set_file_name(&format!("{}.json", app.synth_lab.preset.name.replace(' ', "_").to_lowercase()))
            .save_file()
        {
            if let Err(e) = app.synth_lab.preset.save_to_json_file(&path) {
                app.synth_lab.status_msg = Some(format!("Export failed: {}", e));
            } else {
                app.synth_lab.status_msg = Some(format!("Exported preset to {:?}", path));
            }
        }
    }

    if import_json_requested {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Import Synthesizer Preset")
            .add_filter("JSON Preset", &["json"])
            .pick_file()
        {
            match SynthPreset::load_from_json_file(&path) {
                Ok(loaded) => {
                    app.synth_lab.preset = loaded.clone();
                    app.synth_lab.profile = loaded.body_params.profile_base;
                    app.synth_lab.user_presets.push(loaded.clone());
                    app.synth_lab.selected_preset_idx = 7 + app.synth_lab.user_presets.len() - 1;
                    app.synth_lab.status_msg = Some(format!("Loaded preset '{}'", loaded.name));

                    // Push full update to audio thread
                    let _ = app.ensure_synth_engine();
                    if let Some(engine) = &mut app.synth_engine {
                        engine.send(SynthCommand::SetInstrumentProfile(loaded.body_params.profile_base));
                        engine.send(SynthCommand::UpdateStringParams(loaded.string_params));
                        engine.send(SynthCommand::UpdateBodyParams(loaded.body_params));
                        engine.send(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::Kick,
                            params: DrumVoiceParams::Kick(loaded.drum_params.kick),
                        });
                        engine.send(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::Snare,
                            params: DrumVoiceParams::Snare(loaded.drum_params.snare),
                        });
                        engine.send(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::HiHatAndCymbal,
                            params: DrumVoiceParams::HiHatAndCymbal(loaded.drum_params.hats_cymbals),
                        });
                        engine.send(SynthCommand::UpdateGmVoiceParams(loaded.fm_params));
                    }
                }
                Err(e) => {
                    app.synth_lab.status_msg = Some(format!("Import error: {}", e));
                }
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Sub-panels & UI Component Implementations
// -----------------------------------------------------------------------------

fn render_preset_bar(
    ui: &mut egui::Ui,
    app: &mut MnxEditorApp,
    commands: &mut Vec<SynthCommand>,
    export_json: &mut bool,
    import_json: &mut bool,
) {
    let factory_presets = SynthPreset::factory_presets();

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Preset:").strong());

        let current_name = if app.synth_lab.selected_preset_idx < factory_presets.len() {
            factory_presets[app.synth_lab.selected_preset_idx].name.clone()
        } else {
            let u_idx = app.synth_lab.selected_preset_idx - factory_presets.len();
            app.synth_lab
                .user_presets
                .get(u_idx)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| app.synth_lab.preset.name.clone())
        };

        egui::ComboBox::from_id_salt("synth_preset_combo")
            .selected_text(current_name)
            .width(180.0)
            .show_ui(ui, |ui| {
                ui.label(egui::RichText::new("— Factory Presets —").italics().weak());
                for (i, p) in factory_presets.iter().enumerate() {
                    if ui.selectable_label(app.synth_lab.selected_preset_idx == i, &p.name).clicked() {
                        app.synth_lab.selected_preset_idx = i;
                        app.synth_lab.preset = p.clone();
                        app.synth_lab.profile = p.body_params.profile_base;
                        app.synth_lab.pluck_pos = p.string_params.pluck_pos;
                        app.synth_lab.status_msg = Some(format!("Switched to preset '{}'", p.name));

                        commands.push(SynthCommand::SetInstrumentProfile(p.body_params.profile_base));
                        commands.push(SynthCommand::UpdateStringParams(p.string_params));
                        commands.push(SynthCommand::UpdateBodyParams(p.body_params));
                        commands.push(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::Kick,
                            params: DrumVoiceParams::Kick(p.drum_params.kick),
                        });
                        commands.push(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::Snare,
                            params: DrumVoiceParams::Snare(p.drum_params.snare),
                        });
                        commands.push(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::HiHatAndCymbal,
                            params: DrumVoiceParams::HiHatAndCymbal(p.drum_params.hats_cymbals),
                        });
                        commands.push(SynthCommand::UpdateGmVoiceParams(p.fm_params));
                    }
                }

                if !app.synth_lab.user_presets.is_empty() {
                    ui.separator();
                    ui.label(egui::RichText::new("— User Presets —").italics().weak());
                    for (u_i, p) in app.synth_lab.user_presets.iter().enumerate() {
                        let idx = factory_presets.len() + u_i;
                        if ui.selectable_label(app.synth_lab.selected_preset_idx == idx, &p.name).clicked() {
                            app.synth_lab.selected_preset_idx = idx;
                            app.synth_lab.preset = p.clone();
                            app.synth_lab.profile = p.body_params.profile_base;
                            app.synth_lab.status_msg = Some(format!("Switched to user preset '{}'", p.name));

                            commands.push(SynthCommand::SetInstrumentProfile(p.body_params.profile_base));
                            commands.push(SynthCommand::UpdateStringParams(p.string_params));
                            commands.push(SynthCommand::UpdateBodyParams(p.body_params));
                            commands.push(SynthCommand::UpdateDrumParams {
                                drum_role: DrumRole::Kick,
                                params: DrumVoiceParams::Kick(p.drum_params.kick),
                            });
                            commands.push(SynthCommand::UpdateDrumParams {
                                drum_role: DrumRole::Snare,
                                params: DrumVoiceParams::Snare(p.drum_params.snare),
                            });
                            commands.push(SynthCommand::UpdateDrumParams {
                                drum_role: DrumRole::HiHatAndCymbal,
                                params: DrumVoiceParams::HiHatAndCymbal(p.drum_params.hats_cymbals),
                            });
                            commands.push(SynthCommand::UpdateGmVoiceParams(p.fm_params));
                        }
                    }
                }
            });

        if ui
            .button("Reset")
            .on_hover_text("Reset to the default state of the current preset")
            .clicked()
        {
            let def = if app.synth_lab.selected_preset_idx < factory_presets.len() {
                factory_presets[app.synth_lab.selected_preset_idx].clone()
            } else {
                SynthPreset::default()
            };
            app.synth_lab.preset = def.clone();
            app.synth_lab.profile = def.body_params.profile_base;
            commands.push(SynthCommand::SetInstrumentProfile(def.body_params.profile_base));
            commands.push(SynthCommand::UpdateStringParams(def.string_params));
            commands.push(SynthCommand::UpdateBodyParams(def.body_params));
            commands.push(SynthCommand::UpdateDrumParams {
                drum_role: DrumRole::Kick,
                params: DrumVoiceParams::Kick(def.drum_params.kick),
            });
            commands.push(SynthCommand::UpdateDrumParams {
                drum_role: DrumRole::Snare,
                params: DrumVoiceParams::Snare(def.drum_params.snare),
            });
            commands.push(SynthCommand::UpdateDrumParams {
                drum_role: DrumRole::HiHatAndCymbal,
                params: DrumVoiceParams::HiHatAndCymbal(def.drum_params.hats_cymbals),
            });
            commands.push(SynthCommand::UpdateGmVoiceParams(def.fm_params));
            app.synth_lab.status_msg = Some("Reset parameters to default".to_string());
        }

        if ui
            .button("Save As...")
            .on_hover_text("Save current sound design state as a new user preset")
            .clicked()
        {
            app.synth_lab.save_dialog_open = !app.synth_lab.save_dialog_open;
        }

        if ui
            .button("Export JSON")
            .on_hover_text("Save complete patch to a .json file")
            .clicked()
        {
            *export_json = true;
        }

        if ui
            .button("Import JSON")
            .on_hover_text("Load patch from a .json file")
            .clicked()
        {
            *import_json = true;
        }
    });

    if app.synth_lab.save_dialog_open {
        ui.horizontal(|ui| {
            ui.label("Preset Name:");
            ui.text_edit_singleline(&mut app.synth_lab.save_preset_name);
            if ui.button("Save").clicked() {
                let mut new_p = app.synth_lab.preset.clone();
                new_p.name = app.synth_lab.save_preset_name.trim().to_string();
                if new_p.name.is_empty() {
                    new_p.name = "Custom Preset".to_string();
                }
                app.synth_lab.user_presets.push(new_p.clone());
                app.synth_lab.selected_preset_idx = factory_presets.len() + app.synth_lab.user_presets.len() - 1;
                app.synth_lab.save_dialog_open = false;
                app.synth_lab.status_msg = Some(format!("Saved user preset '{}'", new_p.name));
            }
            if ui.button("Cancel").clicked() {
                app.synth_lab.save_dialog_open = false;
            }
        });
    }

    // Master Gain & Output Level Slider
    ui.horizontal(|ui| {
        ui.label("Master Gain:");
        let gain_resp = ui.add(
            egui::Slider::new(&mut app.synth_lab.master_gain_db, -24.0..=12.0)
                .suffix(" dB")
                .clamping(egui::SliderClamping::Always),
        );
        if gain_resp.changed() {
            app.synth_lab.preset.body_params.post_gain_db = app.synth_lab.master_gain_db;
            commands.push(SynthCommand::UpdateBodyParams(app.synth_lab.preset.body_params));
        }
        if gain_resp.double_clicked() {
            app.synth_lab.master_gain_db = 0.0;
            app.synth_lab.preset.body_params.post_gain_db = 0.0;
            commands.push(SynthCommand::UpdateBodyParams(app.synth_lab.preset.body_params));
        }
    });
}

fn render_audition_strip(
    ui: &mut egui::Ui,
    app: &mut MnxEditorApp,
    commands: &mut Vec<SynthCommand>,
    strum_open: &mut bool,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("Audition:").strong());

        if ui
            .button("Pluck Low (E2)")
            .on_hover_text("Trigger low string at 82.41 Hz (E2)")
            .clicked()
        {
            commands.push(SynthCommand::TriggerString {
                string_idx: 1,
                freq_hz: 82.40689,
                velocity: app.synth_lab.velocity,
                pluck_pos: app.synth_lab.preset.string_params.pluck_pos,
                palm_mute: app.synth_lab.palm_mute,
                harmonic_node: app.synth_lab.harmonic_node,
            });
        }

        if ui
            .button("Pluck High (E4)")
            .on_hover_text("Trigger high string at 329.63 Hz (E4)")
            .clicked()
        {
            commands.push(SynthCommand::TriggerString {
                string_idx: 6,
                freq_hz: 329.6276,
                velocity: app.synth_lab.velocity,
                pluck_pos: app.synth_lab.preset.string_params.pluck_pos,
                palm_mute: app.synth_lab.palm_mute,
                harmonic_node: app.synth_lab.harmonic_node,
            });
        }

        if ui
            .button("Strum Chord (JI 7-Limit)")
            .on_hover_text("Strum a 4-note pure Just Intonation harmonic chord (1/1, 5/4, 3/2, 7/4)")
            .clicked()
        {
            for (idx, ratio) in [(1, 1.0), (2, 1.25), (3, 1.50), (4, 1.75)] {
                let f = 196.0 * ratio; // G3 root
                commands.push(SynthCommand::TriggerString {
                    string_idx: idx,
                    freq_hz: f,
                    velocity: app.synth_lab.velocity * 0.9,
                    pluck_pos: app.synth_lab.preset.string_params.pluck_pos,
                    palm_mute: app.synth_lab.palm_mute,
                    harmonic_node: None,
                });
            }
        }

        if ui
            .button("Harmonic 1/2 (12th)")
            .on_hover_text("Pluck with natural octave harmonic node divider (Some(2))")
            .clicked()
        {
            commands.push(SynthCommand::TriggerString {
                string_idx: 2,
                freq_hz: 110.0,
                velocity: app.synth_lab.velocity,
                pluck_pos: 0.15,
                palm_mute: 0.0,
                harmonic_node: Some(2),
            });
        }

        if ui
            .button("Kick (36)")
            .on_hover_text("Trigger procedural kick drum")
            .clicked()
        {
            commands.push(SynthCommand::TriggerDrum {
                gm_drum_note: 36,
                velocity: app.synth_lab.velocity,
            });
        }

        if ui
            .button("Snare (38)")
            .on_hover_text("Trigger procedural snare drum")
            .clicked()
        {
            commands.push(SynthCommand::TriggerDrum {
                gm_drum_note: 38,
                velocity: app.synth_lab.velocity,
            });
        }

        if ui
            .button("Closed Hat (42)")
            .on_hover_text("Trigger closed metallic hi-hat")
            .clicked()
        {
            commands.push(SynthCommand::TriggerDrum {
                gm_drum_note: 42,
                velocity: app.synth_lab.velocity,
            });
        }

        if ui
            .button("Open Hat (46)")
            .on_hover_text("Trigger open metallic hi-hat")
            .clicked()
        {
            commands.push(SynthCommand::TriggerDrum {
                gm_drum_note: 46,
                velocity: app.synth_lab.velocity,
            });
        }

        if ui
            .button("Strum Tab Open Strings")
            .on_hover_text("Strum all open strings of the current tab part using exact microtonal tuning")
            .clicked()
        {
            *strum_open = true;
        }
    });
}

fn render_visual_preview_canvas(ui: &mut egui::Ui, state: &crate::synth::SynthTestLabState, sample_rate: f32) {
    let desired_size = Vec2::new(ui.available_width().max(320.0), 85.0);
    let (rect, _response) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
    let painter = ui.painter_at(rect);

    // Dark sleek synthesizer canvas background
    painter.rect_filled(rect, 4.0, Color32::from_rgb(18, 22, 28));
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, Color32::from_rgb(45, 55, 70)),
        egui::StrokeKind::Inside,
    );

    match state.active_tab {
        SynthEditorTab::BodyAndAmp => {
            // Plot 4-Band Biquad composite frequency response curve (-24 dB to +24 dB, 20 Hz to 20 kHz)
            paint_frequency_response_preview(&painter, rect, &state.preset.body_params, sample_rate);
        }
        SynthEditorTab::StringAndPluck => {
            // Plot Karplus-Strong string decay & comb filter preview
            paint_string_decay_preview(&painter, rect, &state.preset.string_params);
        }
        SynthEditorTab::DrumsAndCymbals => {
            // Plot procedural drum pitch & envelope decay preview
            paint_drum_envelopes_preview(&painter, rect, &state.preset.drum_params);
        }
        SynthEditorTab::GmFmSynth => {
            // Plot Carrier ADSR and Modulator ADSR curves side-by-side
            paint_fm_adsr_preview(&painter, rect, &state.preset.fm_params);
        }
    }
}

/// Paints the magnitude response curve of the 4 parametric biquad bands.
fn paint_frequency_response_preview(
    painter: &egui::Painter,
    rect: Rect,
    body: &crate::synth::patch::BodyResonatorParams,
    sample_rate: f32,
) {
    let min_freq = 20.0_f32;
    let max_freq = 20_000.0_f32;
    let log_min = min_freq.log10();
    let log_max = max_freq.log10();

    let db_min = -24.0_f32;
    let db_max = 24.0_f32;

    let y_from_db = |db: f32| -> f32 {
        let norm = (db - db_min) / (db_max - db_min);
        rect.max.y - norm.clamp(0.0, 1.0) * rect.height()
    };

    let x_from_freq = |f: f32| -> f32 {
        let log_f = f.clamp(min_freq, max_freq).log10();
        rect.min.x + ((log_f - log_min) / (log_max - log_min)) * rect.width()
    };

    // Grid lines: 0 dB line
    let zero_y = y_from_db(0.0);
    painter.line_segment(
        [Pos2::new(rect.min.x, zero_y), Pos2::new(rect.max.x, zero_y)],
        Stroke::new(1.0, Color32::from_rgb(60, 75, 95)),
    );
    // +12 dB & -12 dB lines
    painter.line_segment(
        [Pos2::new(rect.min.x, y_from_db(12.0)), Pos2::new(rect.max.x, y_from_db(12.0))],
        Stroke::new(0.5, Color32::from_rgb(35, 45, 60)),
    );
    painter.line_segment(
        [Pos2::new(rect.min.x, y_from_db(-12.0)), Pos2::new(rect.max.x, y_from_db(-12.0))],
        Stroke::new(0.5, Color32::from_rgb(35, 45, 60)),
    );

    // Vertical frequency markers: 100 Hz, 1 kHz, 10 kHz
    for &f_mark in &[100.0, 1_000.0, 10_000.0] {
        let x = x_from_freq(f_mark);
        painter.line_segment(
            [Pos2::new(x, rect.min.y), Pos2::new(x, rect.max.y)],
            Stroke::new(0.5, Color32::from_rgb(35, 45, 60)),
        );
    }

    // Build biquad filter objects for each active band to evaluate their transfer function
    let mut filters: Vec<BiquadFilter> = Vec::with_capacity(4);
    for b in &body.bands {
        if b.enabled {
            let f = b.freq_hz.clamp(20.0, sample_rate * 0.45);
            let q = b.q.clamp(0.1, 15.0);
            let g = b.gain_db.clamp(-24.0, 24.0);
            // Use peaking EQ or bandpass peak
            let filt = BiquadFilter::peaking_eq(sample_rate, f, q, g);
            filters.push(filt);
        }
    }

    let steps = 96;
    let mut points: Vec<Pos2> = Vec::with_capacity(steps + 1);

    for s in 0..=steps {
        let t = (s as f32) / (steps as f32);
        let log_f = log_min + t * (log_max - log_min);
        let freq = 10.0_f32.powf(log_f);

        let mut total_db = 0.0_f32;
        for filt in &filters {
            total_db += filt.magnitude_db(freq, sample_rate);
        }
        total_db += body.post_gain_db;

        let px = rect.min.x + t * rect.width();
        let py = y_from_db(total_db);
        points.push(Pos2::new(px, py));
    }

    // Draw the curve with vibrant teal stroke
    painter.add(egui::Shape::line(
        points,
        Stroke::new(2.0, Color32::from_rgb(90, 230, 200)),
    ));

    // Legend label
    painter.text(
        Pos2::new(rect.min.x + 8.0, rect.min.y + 6.0),
        egui::Align2::LEFT_TOP,
        format!("EQ / Body Spectrum: {} (Gain {:+.1} dB)", body.profile_base.label(), body.post_gain_db),
        egui::FontId::proportional(11.0),
        Color32::from_rgb(170, 210, 230),
    );
}

/// Paints the Karplus-Strong string decay envelope curve.
fn paint_string_decay_preview(
    painter: &egui::Painter,
    rect: Rect,
    params: &StringSynthParams,
) {
    let t60 = params.t60_decay_sec.clamp(0.1, 10.0);
    let steps = 80;
    let mut points: Vec<Pos2> = Vec::with_capacity(steps + 1);

    for s in 0..=steps {
        let t = (s as f32) / (steps as f32);
        let time_sec = t * t60 * 1.1;
        // Exponential decay: exp(-ln(1000) * t / t60) = 10^(-3 * t / t60)
        let amp = 10.0_f32.powf(-3.0 * (time_sec / t60));
        let px = rect.min.x + t * rect.width();
        let py = rect.max.y - (amp.clamp(0.0, 1.0) * (rect.height() - 16.0) + 8.0);
        points.push(Pos2::new(px, py));
    }

    painter.add(egui::Shape::line(
        points,
        Stroke::new(2.0, Color32::from_rgb(255, 185, 75)),
    ));

    painter.text(
        Pos2::new(rect.min.x + 8.0, rect.min.y + 6.0),
        egui::Align2::LEFT_TOP,
        format!(
            "EKS Decay Curve: T60 = {:.2}s, Pluck Pos = {:.2}, Hardness = {:.0}Hz",
            params.t60_decay_sec, params.pluck_pos, params.pick_hardness
        ),
        egui::FontId::proportional(11.0),
        Color32::from_rgb(255, 210, 140),
    );
}

/// Paints the procedural drum pitch and noise decay envelopes.
fn paint_drum_envelopes_preview(
    painter: &egui::Painter,
    rect: Rect,
    params: &crate::synth::patch::DrumKitParams,
) {
    let half_w = rect.width() * 0.48;

    // Left half: Kick pitch drop
    let mut kick_points: Vec<Pos2> = Vec::with_capacity(40);
    for s in 0..=40 {
        let t = (s as f32) / 40.0;
        let p_env = (-t * (40.0 / (params.kick.pitch_decay_ms.max(5.0) * 0.5))).exp();
        let hz = params.kick.pitch_end_hz + (params.kick.pitch_start_hz - params.kick.pitch_end_hz) * p_env;
        let norm = (hz - 30.0) / (250.0 - 30.0);
        let px = rect.min.x + t * half_w;
        let py = rect.max.y - (norm.clamp(0.0, 1.0) * (rect.height() - 18.0) + 9.0);
        kick_points.push(Pos2::new(px, py));
    }
    painter.add(egui::Shape::line(
        kick_points,
        Stroke::new(2.0, Color32::from_rgb(255, 95, 110)),
    ));

    // Right half: Snare wire decay
    let right_start = rect.min.x + half_w + 10.0;
    let right_w = rect.max.x - right_start;
    let mut snare_points: Vec<Pos2> = Vec::with_capacity(40);
    for s in 0..=40 {
        let t = (s as f32) / 40.0;
        let amp = (-t * (180.0 / params.snare.noise_decay_ms.max(10.0))).exp();
        let px = right_start + t * right_w;
        let py = rect.max.y - (amp.clamp(0.0, 1.0) * (rect.height() - 18.0) + 9.0);
        snare_points.push(Pos2::new(px, py));
    }
    painter.add(egui::Shape::line(
        snare_points,
        Stroke::new(2.0, Color32::from_rgb(120, 190, 255)),
    ));

    painter.text(
        Pos2::new(rect.min.x + 8.0, rect.min.y + 6.0),
        egui::Align2::LEFT_TOP,
        format!(
            "Kick: {:.0}->{:.0}Hz ({:.0}ms) | Snare Wire Decay: {:.0}ms",
            params.kick.pitch_start_hz, params.kick.pitch_end_hz, params.kick.pitch_decay_ms, params.snare.noise_decay_ms
        ),
        egui::FontId::proportional(11.0),
        Color32::from_rgb(200, 215, 240),
    );
}

/// Paints the FM Carrier & Modulator ADSR curves side-by-side.
fn paint_fm_adsr_preview(
    painter: &egui::Painter,
    rect: Rect,
    params: &FmVoiceParams,
) {
    let half_w = rect.width() * 0.48;

    // Helper to generate ADSR polyline points
    let make_adsr_points = |adsr: &AdsrEnvelopeParams, offset_x: f32, width: f32| -> Vec<Pos2> {
        let total_time = adsr.attack_ms + adsr.decay_ms + 150.0 + adsr.release_ms;
        let x_from_t = |t_ms: f32| -> f32 {
            offset_x + (t_ms / total_time.max(10.0)) * width
        };
        let y_from_amp = |amp: f32| -> f32 {
            rect.max.y - (amp.clamp(0.0, 1.0) * (rect.height() - 20.0) + 10.0)
        };

        vec![
            Pos2::new(x_from_t(0.0), y_from_amp(0.0)),
            Pos2::new(x_from_t(adsr.attack_ms), y_from_amp(1.0)),
            Pos2::new(x_from_t(adsr.attack_ms + adsr.decay_ms), y_from_amp(adsr.sustain)),
            Pos2::new(x_from_t(adsr.attack_ms + adsr.decay_ms + 150.0), y_from_amp(adsr.sustain)),
            Pos2::new(x_from_t(total_time), y_from_amp(0.0)),
        ]
    };

    let carrier_pts = make_adsr_points(&params.carrier_adsr, rect.min.x, half_w);
    painter.add(egui::Shape::line(
        carrier_pts,
        Stroke::new(2.0, Color32::from_rgb(120, 240, 160)),
    ));

    let right_start = rect.min.x + half_w + 10.0;
    let right_w = rect.max.x - right_start;
    let mod_pts = make_adsr_points(&params.mod_adsr, right_start, right_w);
    painter.add(egui::Shape::line(
        mod_pts,
        Stroke::new(2.0, Color32::from_rgb(255, 140, 220)),
    ));

    painter.text(
        Pos2::new(rect.min.x + 8.0, rect.min.y + 6.0),
        egui::Align2::LEFT_TOP,
        format!(
            "FM Carrier ADSR (Green) | Modulator ADSR (Pink) - Ratio {:.2}, Index {:.2}",
            params.mod_ratio, params.mod_index
        ),
        egui::FontId::proportional(11.0),
        Color32::from_rgb(220, 240, 200),
    );
}

// -----------------------------------------------------------------------------
// Inspector Tabs: String, Body, Drums, FM
// -----------------------------------------------------------------------------

fn render_string_tab(ui: &mut egui::Ui, app: &mut MnxEditorApp, commands: &mut Vec<SynthCommand>) {
    let mut changed = false;
    let p = &mut app.synth_lab.preset.string_params;

    egui::Grid::new("string_synth_params_grid")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Excitation:").strong());
            ui.label("");
            ui.end_row();

            ui.label("Pluck Position:");
            let resp = ui.add(
                egui::Slider::new(&mut p.pluck_pos, 0.05..=0.50)
                    .text("bridge -> neck"),
            );
            if resp.changed() { changed = true; }
            if resp.double_clicked() { p.pluck_pos = 0.18; changed = true; }
            ui.end_row();

            ui.label("Pick Hardness (Cutoff):");
            let resp = ui.add(
                egui::Slider::new(&mut p.pick_hardness, 500.0..=16_000.0)
                    .logarithmic(true)
                    .suffix(" Hz"),
            );
            if resp.changed() { changed = true; }
            if resp.double_clicked() { p.pick_hardness = 8_500.0; changed = true; }
            ui.end_row();

            ui.label("Noise vs. Triangle Mix:");
            let resp = ui.add(
                egui::Slider::new(&mut p.noise_mix, 0.0..=1.0)
                    .text("triangle -> noise"),
            );
            if resp.changed() { changed = true; }
            if resp.double_clicked() { p.noise_mix = 0.65; changed = true; }
            ui.end_row();

            ui.separator();
            ui.separator();
            ui.end_row();

            ui.label(egui::RichText::new("Loop & Decay:").strong());
            ui.label("");
            ui.end_row();

            ui.label("Sustain Decay T60:");
            let resp = ui.add(
                egui::Slider::new(&mut p.t60_decay_sec, 0.1..=10.0)
                    .logarithmic(true)
                    .suffix(" s"),
            );
            if resp.changed() { changed = true; }
            if resp.double_clicked() { p.t60_decay_sec = 4.2; changed = true; }
            ui.end_row();

            ui.label("Loop Brightness S:");
            let resp = ui.add(
                egui::Slider::new(&mut p.brightness_s, 0.01..=0.99)
                    .text("bright -> warm"),
            );
            if resp.changed() { changed = true; }
            if resp.double_clicked() { p.brightness_s = 0.15; changed = true; }
            ui.end_row();

            ui.label("Wound String Dispersion:");
            let resp = ui.add(
                egui::Slider::new(&mut p.inharmonicity_dispersion, 0.0..=0.80)
                    .text("pure -> inharmonic"),
            );
            if resp.changed() { changed = true; }
            if resp.double_clicked() { p.inharmonicity_dispersion = 0.12; changed = true; }
            ui.end_row();

            ui.separator();
            ui.separator();
            ui.end_row();

            ui.label(egui::RichText::new("Damping & Harmonics:").strong());
            ui.label("");
            ui.end_row();

            ui.label("Palm Mute Damping:");
            let resp = ui.add(egui::Slider::new(&mut p.palm_mute_damping, 0.01..=0.80));
            if resp.changed() { changed = true; }
            if resp.double_clicked() { p.palm_mute_damping = 0.22; changed = true; }
            ui.end_row();

            ui.label("Harmonic Tap Coupling:");
            let resp = ui.add(
                egui::Slider::new(&mut p.harmonic_damping_ms, 10.0..=500.0)
                    .suffix(" ms"),
            );
            if resp.changed() { changed = true; }
            if resp.double_clicked() { p.harmonic_damping_ms = 30.0; changed = true; }
            ui.end_row();
        });

    if changed {
        commands.push(SynthCommand::UpdateStringParams(*p));
    }
}

fn render_body_tab(ui: &mut egui::Ui, app: &mut MnxEditorApp, commands: &mut Vec<SynthCommand>) {
    let mut changed = false;
    let p = &mut app.synth_lab.preset.body_params;

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Archetype Profile:").strong());
        for prof in InstrumentProfile::ALL {
            if ui
                .selectable_label(p.profile_base == prof, prof.label())
                .clicked()
            {
                p.profile_base = prof;
                app.synth_lab.profile = prof;
                changed = true;
                commands.push(SynthCommand::SetInstrumentProfile(prof));
            }
        }
    });

    ui.add_space(4.0);

    egui::Grid::new("body_amp_overview_grid")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            ui.label("Body Wet Mix:");
            let resp = ui.add(egui::Slider::new(&mut p.body_wet_mix, 0.0..=1.0));
            if resp.changed() { changed = true; }
            if resp.double_clicked() { p.body_wet_mix = 0.35; changed = true; }
            ui.end_row();

            ui.label("Pre Drive / Saturation:");
            let resp = ui.add(egui::Slider::new(&mut p.pre_drive, 0.0..=5.0));
            if resp.changed() { changed = true; }
            if resp.double_clicked() { p.pre_drive = 1.0; changed = true; }
            ui.end_row();

            ui.label("Post Gain (Makeup):");
            let resp = ui.add(
                egui::Slider::new(&mut p.post_gain_db, -18.0..=12.0)
                    .suffix(" dB"),
            );
            if resp.changed() { changed = true; }
            if resp.double_clicked() { p.post_gain_db = 0.0; changed = true; }
            ui.end_row();
        });

    ui.separator();
    ui.label(egui::RichText::new("4-Band Parametric Filter Bank:").strong());

    let band_names = [
        "Band 1 (Helmholtz / Air Cavity)",
        "Band 2 (Top Plate Wood Dipole)",
        "Band 3 (Pickup / Presence Peak)",
        "Band 4 (Cabinet / Speaker Roll-off)",
    ];

    for (i, &name) in band_names.iter().enumerate() {
        let b = &mut p.bands[i];
        ui.group(|ui| {
            ui.horizontal(|ui| {
                if ui.checkbox(&mut b.enabled, name).changed() {
                    changed = true;
                }
            });

            ui.horizontal(|ui| {
                ui.label("Freq:");
                let resp_f = ui.add(
                    egui::Slider::new(&mut b.freq_hz, 20.0..=18_000.0)
                        .logarithmic(true)
                        .suffix(" Hz"),
                );
                if resp_f.changed() { changed = true; }

                ui.label("Q:");
                let resp_q = ui.add(egui::Slider::new(&mut b.q, 0.1..=15.0));
                if resp_q.changed() { changed = true; }

                ui.label("Gain:");
                let resp_g = ui.add(
                    egui::Slider::new(&mut b.gain_db, -24.0..=24.0)
                        .suffix(" dB"),
                );
                if resp_g.changed() { changed = true; }
            });
        });
    }

    if changed {
        commands.push(SynthCommand::UpdateBodyParams(*p));
    }
}

fn render_drums_tab(ui: &mut egui::Ui, app: &mut MnxEditorApp, commands: &mut Vec<SynthCommand>) {
    let dp = &mut app.synth_lab.preset.drum_params;

    // 1. Kick Drum
    ui.group(|ui| {
        ui.label(egui::RichText::new("Kick Drum (GM 35, 36):").strong());
        let mut k_changed = false;
        let k = &mut dp.kick;

        egui::Grid::new("kick_params_grid")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                ui.label("Pitch Start:");
                let r1 = ui.add(egui::Slider::new(&mut k.pitch_start_hz, 80.0..=250.0).suffix(" Hz"));
                if r1.changed() { k_changed = true; }
                ui.end_row();

                ui.label("Pitch End:");
                let r2 = ui.add(egui::Slider::new(&mut k.pitch_end_hz, 30.0..=65.0).suffix(" Hz"));
                if r2.changed() { k_changed = true; }
                ui.end_row();

                ui.label("Pitch Drop Time:");
                let r3 = ui.add(egui::Slider::new(&mut k.pitch_decay_ms, 10.0..=80.0).suffix(" ms"));
                if r3.changed() { k_changed = true; }
                ui.end_row();

                ui.label("Click Attack Level:");
                let r4 = ui.add(egui::Slider::new(&mut k.click_level, 0.0..=1.0));
                if r4.changed() { k_changed = true; }
                ui.end_row();
            });

        if k_changed {
            commands.push(SynthCommand::UpdateDrumParams {
                drum_role: DrumRole::Kick,
                params: DrumVoiceParams::Kick(*k),
            });
        }
    });

    // 2. Snare Drum
    ui.group(|ui| {
        ui.label(egui::RichText::new("Snare Drum (GM 38, 40):").strong());
        let mut s_changed = false;
        let s = &mut dp.snare;

        egui::Grid::new("snare_params_grid")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                ui.label("Shell Tone Freq:");
                let r1 = ui.add(egui::Slider::new(&mut s.tone_freq_hz, 120.0..=350.0).suffix(" Hz"));
                if r1.changed() { s_changed = true; }
                ui.end_row();

                ui.label("Wire Noise Decay:");
                let r2 = ui.add(egui::Slider::new(&mut s.noise_decay_ms, 50.0..=400.0).suffix(" ms"));
                if r2.changed() { s_changed = true; }
                ui.end_row();

                ui.label("Noise / Tone Mix:");
                let r3 = ui.add(egui::Slider::new(&mut s.noise_mix, 0.0..=1.0));
                if r3.changed() { s_changed = true; }
                ui.end_row();
            });

        if s_changed {
            commands.push(SynthCommand::UpdateDrumParams {
                drum_role: DrumRole::Snare,
                params: DrumVoiceParams::Snare(*s),
            });
        }
    });

    // 3. Hi-Hats & Cymbals
    ui.group(|ui| {
        ui.label(egui::RichText::new("Hi-Hats & Cymbals (GM 42, 44, 46, 49, 51):").strong());
        let mut c_changed = false;
        let c = &mut dp.hats_cymbals;

        egui::Grid::new("cymbal_params_grid")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                ui.label("Metallic Matrix Spread:");
                let r1 = ui.add(egui::Slider::new(&mut c.spread, 0.5..=2.0));
                if r1.changed() { c_changed = true; }
                ui.end_row();

                ui.label("Highpass Cutoff:");
                let r2 = ui.add(
                    egui::Slider::new(&mut c.highpass_cutoff_hz, 3_000.0..=10_000.0)
                        .suffix(" Hz"),
                );
                if r2.changed() { c_changed = true; }
                ui.end_row();

                ui.label("Closed Hat Decay:");
                let r3 = ui.add(egui::Slider::new(&mut c.closed_decay_ms, 20.0..=120.0).suffix(" ms"));
                if r3.changed() { c_changed = true; }
                ui.end_row();

                ui.label("Open Hat / Crash Decay:");
                let r4 = ui.add(egui::Slider::new(&mut c.open_decay_ms, 200.0..=1500.0).suffix(" ms"));
                if r4.changed() { c_changed = true; }
                ui.end_row();
            });

        if c_changed {
            commands.push(SynthCommand::UpdateDrumParams {
                drum_role: DrumRole::HiHatAndCymbal,
                params: DrumVoiceParams::HiHatAndCymbal(*c),
            });
        }
    });
}

fn render_fm_tab(ui: &mut egui::Ui, app: &mut MnxEditorApp, commands: &mut Vec<SynthCommand>) {
    let mut changed = false;
    let fmp = &mut app.synth_lab.preset.fm_params;

    ui.group(|ui| {
        ui.label(egui::RichText::new("2-Operator Modulation Parameters:").strong());

        egui::Grid::new("fm_core_params_grid")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                ui.label("Modulator Frequency Ratio:");
                let r1 = ui.add(
                    egui::Slider::new(&mut fmp.mod_ratio, 0.5..=12.0)
                        .step_by(0.25),
                );
                if r1.changed() { changed = true; }
                ui.end_row();

                ui.label("Peak Modulation Index:");
                let r2 = ui.add(egui::Slider::new(&mut fmp.mod_index, 0.0..=10.0));
                if r2.changed() { changed = true; }
                ui.end_row();

                ui.label("Modulator Self-Feedback:");
                let r3 = ui.add(egui::Slider::new(&mut fmp.feedback, 0.0..=1.5));
                if r3.changed() { changed = true; }
                ui.end_row();
            });
    });

    ui.horizontal(|ui| {
        // Carrier ADSR
        ui.group(|ui| {
            ui.label(egui::RichText::new("Carrier Envelope (ADSR):").strong());
            let c_adsr = &mut fmp.carrier_adsr;

            ui.label("Attack:");
            if ui.add(egui::Slider::new(&mut c_adsr.attack_ms, 0.5..=500.0).suffix(" ms")).changed() { changed = true; }
            ui.label("Decay:");
            if ui.add(egui::Slider::new(&mut c_adsr.decay_ms, 10.0..=2000.0).suffix(" ms")).changed() { changed = true; }
            ui.label("Sustain:");
            if ui.add(egui::Slider::new(&mut c_adsr.sustain, 0.0..=1.0)).changed() { changed = true; }
            ui.label("Release:");
            if ui.add(egui::Slider::new(&mut c_adsr.release_ms, 10.0..=3000.0).suffix(" ms")).changed() { changed = true; }
        });

        // Modulator ADSR
        ui.group(|ui| {
            ui.label(egui::RichText::new("Modulator Envelope (ADSR):").strong());
            let m_adsr = &mut fmp.mod_adsr;

            ui.label("Attack:");
            if ui.add(egui::Slider::new(&mut m_adsr.attack_ms, 0.5..=500.0).suffix(" ms")).changed() { changed = true; }
            ui.label("Decay:");
            if ui.add(egui::Slider::new(&mut m_adsr.decay_ms, 10.0..=2000.0).suffix(" ms")).changed() { changed = true; }
            ui.label("Sustain:");
            if ui.add(egui::Slider::new(&mut m_adsr.sustain, 0.0..=1.0)).changed() { changed = true; }
            ui.label("Release:");
            if ui.add(egui::Slider::new(&mut m_adsr.release_ms, 10.0..=3000.0).suffix(" ms")).changed() { changed = true; }
        });
    });

    if changed {
        commands.push(SynthCommand::UpdateGmVoiceParams(*fmp));
    }
}

/// Helper to strum the open strings of the active tab part using microtonal tuning.
fn strum_current_part_open_strings(app: &mut MnxEditorApp) {
    let _ = app.ensure_synth_engine();

    if let Some(doc) = &app.parsed_mnx {
        if let Some(part) = doc
            .parts
            .get(app.selected_part_idx)
            .or_else(|| doc.parts.first())
        {
            if part.is_percussion() {
                if let Some(engine) = &mut app.synth_engine {
                    for (line, _id, comp) in part.ordered_kit_lines() {
                        engine.send(SynthCommand::TriggerDrum {
                            gm_drum_note: comp.sound.unwrap_or(38).min(127),
                            velocity: app.synth_lab.velocity,
                        });
                        let _ = line;
                    }
                }
            } else {
                let table = crate::tuning_table::FretboardFrequencyTable::from_doc_part(doc, part);
                let n_str = part.num_strings() as u8;
                let pluck_pos = app.synth_lab.preset.string_params.pluck_pos;
                let palm_mute = app.synth_lab.palm_mute;
                let velocity = app.synth_lab.velocity;

                if let Some(engine) = &mut app.synth_engine {
                    for s in (1..=n_str).rev() {
                        if let Some(hz) = table.string_fret_to_frequency_hz(part, s, 0) {
                            engine.send(SynthCommand::TriggerString {
                                string_idx: s,
                                freq_hz: hz,
                                velocity,
                                pluck_pos,
                                palm_mute,
                                harmonic_node: app.synth_lab.harmonic_node,
                            });
                        }
                    }
                }
            }
        }
    }
}
