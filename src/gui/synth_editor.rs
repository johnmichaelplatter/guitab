//! Interactive Sound Design & Patch Editor UI (`src/gui/synth_editor.rs`).
//!
//! Provides real-time parameter tweaking with lock-free SPSC dispatch, context-aware track targeting,
//! exclusive synthesis engine switching (Physical String vs FM Synth for pitched tracks, Procedural Drum Kit
//! for Channel 10 tracks), frequency response and envelope preview painting, factory preset selection,
//! and JSON preset export/import.

use super::*;
use crate::synth::body::{BiquadFilter, InstrumentProfile};
use crate::synth::command::SynthCommand;
use crate::synth::patch::{
    AdsrEnvelopeParams, DrumFamily, DrumKitParams, DrumRole, DrumVoiceParams, FmVoiceParams,
    HandClapParams, HiHatParams, KickParams, PercAuxParams, PitchedEngine, RideCymbalParams,
    ShakerAndEffectParams, SingleCymbalParams, SnareParams, SynthPreset, TomParams,
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

    if !app.synth_lab.output_devices_scanned && !cfg!(test) {
        app.synth_lab.available_output_devices = crate::synth::list_output_devices();
        app.synth_lab.output_devices_scanned = true;
    }

    let engine_info = app
        .synth_engine
        .as_ref()
        .map(|e| (e.sample_rate(), e.channels(), e.device_name().to_string()));

    let sample_rate = engine_info.as_ref().map(|(sr, _, _)| *sr).unwrap_or(48_000.0);

    // Validate and clamp target_part_idx
    let part_count = app
        .parsed_mnx
        .as_ref()
        .map(|d| d.parts.len())
        .unwrap_or(1)
        .max(1);
    if app.synth_lab.target_part_idx >= part_count {
        app.synth_lab.target_part_idx = 0;
    }

    // Always bind the editor state to the active target part's persisted preset
    let bound_preset = app.get_part_synth_preset(app.synth_lab.target_part_idx);
    app.synth_lab.preset = bound_preset.clone();
    app.synth_lab.profile = bound_preset.body_params.profile_base;

    let is_target_percussion = app
        .parsed_mnx
        .as_ref()
        .and_then(|d| d.parts.get(app.synth_lab.target_part_idx))
        .map(|p| p.is_percussion())
        .unwrap_or(false);

    egui::Window::new("Sound Design & Patch Editor (Synth Lab)")
        .open(&mut open)
        .resizable(true)
        .default_width(740.0)
        .default_height(780.0)
        .show(ctx, |ui| {
            // -------------------------------------------------------------
            // Top Audio Setup, Output Device Selector, Status & Panic Bar
            // -------------------------------------------------------------
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Audio Output:").strong());

                let selected_dev_label = match &app.synth_lab.selected_output_device {
                    Some(name) => name.clone(),
                    None => {
                        if let Some((_, _, active_dev)) = &engine_info {
                            format!("System Default ({})", active_dev)
                        } else {
                            "System Default".to_string()
                        }
                    }
                };

                egui::ComboBox::from_id_salt("synth_audio_output_device_combo")
                    .selected_text(selected_dev_label)
                    .width(245.0)
                    .show_ui(ui, |ui| {
                        let is_def = app.synth_lab.selected_output_device.is_none();
                        if ui.selectable_label(is_def, "System Default").clicked() && !is_def {
                            app.synth_lab.selected_output_device = None;
                            reconnect_requested = true;
                        }
                        if !app.synth_lab.available_output_devices.is_empty() {
                            ui.separator();
                            let devices = app.synth_lab.available_output_devices.clone();
                            for dev_name in devices {
                                let is_sel = app
                                    .synth_lab
                                    .selected_output_device
                                    .as_deref()
                                    == Some(dev_name.as_str());
                                if ui.selectable_label(is_sel, &dev_name).clicked() && !is_sel {
                                    app.synth_lab.selected_output_device = Some(dev_name);
                                    reconnect_requested = true;
                                }
                            }
                        }
                    });

                if ui
                    .small_button("Refresh")
                    .on_hover_text("Rescan available audio output devices on this machine")
                    .clicked()
                {
                    app.synth_lab.available_output_devices = crate::synth::list_output_devices();
                    app.synth_lab.output_devices_scanned = true;
                    app.synth_lab.status_msg = Some(format!(
                        "Found {} audio output device(s)",
                        app.synth_lab.available_output_devices.len()
                    ));
                }

                if let Some((sr, ch, _)) = &engine_info {
                    ui.colored_label(
                        Color32::from_rgb(90, 230, 140),
                        format!("{:.0} Hz, {} ch", sr, ch),
                    );
                } else {
                    ui.colored_label(
                        Color32::from_rgb(255, 190, 90),
                        "Inactive",
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
                        .on_hover_text("Start or restart the real-time cpal stream on the selected audio output")
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

            // -------------------------------------------------------------
            // 1. Target Instrument / Track Selector (Context-Aware Editing)
            // -------------------------------------------------------------
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Target Instrument / Track:").strong());

                let format_part_synth_label =
                    |idx: usize, part: &crate::mnx::Part, preset: &SynthPreset| -> String {
                        if part.is_percussion() {
                            format!(
                                "{}: {} — {} (Drum Kit, Ch {})",
                                idx + 1,
                                part.display_name(idx),
                                preset.name,
                                part.percussion_midi_channel()
                            )
                        } else {
                            let engine_desc = match preset.engine {
                                PitchedEngine::PhysicalString => {
                                    preset.body_params.profile_base.label()
                                }
                                PitchedEngine::FmSynth => "FM Synth",
                            };
                            format!(
                                "{}: {} — {} [{}]",
                                idx + 1,
                                part.display_name(idx),
                                preset.name,
                                engine_desc
                            )
                        }
                    };

                let current_label = if let Some(doc) = &app.parsed_mnx {
                    if let Some(part) = doc.parts.get(app.synth_lab.target_part_idx) {
                        format_part_synth_label(
                            app.synth_lab.target_part_idx,
                            part,
                            &app.synth_lab.preset,
                        )
                    } else {
                        format!(
                            "{}: Track {} — {}",
                            app.synth_lab.target_part_idx + 1,
                            app.synth_lab.target_part_idx + 1,
                            app.synth_lab.preset.name
                        )
                    }
                } else {
                    format!("1: Default Track — {}", app.synth_lab.preset.name)
                };

                let mut target_switched = false;

                egui::ComboBox::from_id_salt("synth_target_part_selector")
                    .selected_text(current_label)
                    .width(340.0)
                    .show_ui(ui, |ui| {
                        let part_entries: Vec<(usize, String)> = if let Some(doc) = &app.parsed_mnx
                        {
                            doc.parts
                                .iter()
                                .enumerate()
                                .map(|(idx, part)| {
                                    let p_preset = app.get_part_synth_preset(idx);
                                    (idx, format_part_synth_label(idx, part, &p_preset))
                                })
                                .collect()
                        } else {
                            Vec::new()
                        };

                        if !part_entries.is_empty() {
                            for (idx, label) in part_entries {
                                if ui
                                    .selectable_label(app.synth_lab.target_part_idx == idx, label)
                                    .clicked()
                                {
                                    if app.synth_lab.target_part_idx != idx {
                                        app.synth_lab.target_part_idx = idx;
                                        target_switched = true;
                                    }
                                }
                            }
                        } else if ui
                            .selectable_label(
                                app.synth_lab.target_part_idx == 0,
                                "1: Default Track (Pitched)",
                            )
                            .clicked()
                        {
                            app.synth_lab.target_part_idx = 0;
                        }
                    });

                if target_switched {
                    // Load new target state
                    let new_p = app.get_part_synth_preset(app.synth_lab.target_part_idx);
                    app.synth_lab.preset = new_p.clone();
                    app.synth_lab.profile = new_p.body_params.profile_base;

                    let is_perc = app
                        .parsed_mnx
                        .as_ref()
                        .and_then(|d| d.parts.get(app.synth_lab.target_part_idx))
                        .map(|p| p.is_percussion())
                        .unwrap_or(false);

                    if is_perc {
                        app.synth_lab.active_tab = SynthEditorTab::DrumKit;
                    } else {
                        app.synth_lab.active_tab = match new_p.engine {
                            PitchedEngine::PhysicalString => SynthEditorTab::PhysicalString,
                            PitchedEngine::FmSynth => SynthEditorTab::FmSynth,
                        };
                    }

                    app.sync_part_to_synth_engine(app.synth_lab.target_part_idx, &new_p);

                    app.synth_lab.status_msg = Some(format!(
                        "Editing Track {}: '{}' ({})",
                        app.synth_lab.target_part_idx + 1,
                        new_p.name,
                        match (is_perc, new_p.engine) {
                            (true, _) => "Drum Kit",
                            (false, PitchedEngine::PhysicalString) => "Physical String",
                            (false, PitchedEngine::FmSynth) => "FM Synth",
                        }
                    ));
                }
            });

            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut app.synth_lab.route_score_preview,
                    "Route Score Note Clicks & Tab Navigation Previews to Internal Synth",
                );
            });

            ui.separator();

            // -------------------------------------------------------------
            // 2. Preset Management & Master Level Bar
            // -------------------------------------------------------------
            render_preset_bar(
                ui,
                app,
                &mut commands_to_send,
                &mut export_json_requested,
                &mut import_json_requested,
                is_target_percussion,
            );

            ui.separator();

            // -------------------------------------------------------------
            // 3. Scoped Live Audition Strip
            // -------------------------------------------------------------
            render_scoped_audition_strip(
                ui,
                app,
                &mut commands_to_send,
                &mut strum_open_requested,
                is_target_percussion,
            );

            ui.separator();

            // -------------------------------------------------------------
            // 4. Interactive Frequency / Envelope Preview Canvas
            // -------------------------------------------------------------
            render_visual_preview_canvas(ui, &app.synth_lab, sample_rate, is_target_percussion);

            ui.separator();

            // -------------------------------------------------------------
            // 5. Exclusive Engine Selection & Unified Panels
            // -------------------------------------------------------------
            if is_target_percussion {
                // Drum Mode: Hide pitched tabs, show only Drum Kit Editor
                app.synth_lab.active_tab = SynthEditorTab::DrumKit;
                ui.horizontal(|ui| {
                    ui.colored_label(
                        Color32::from_rgb(255, 185, 90),
                        egui::RichText::new("Drum Mode: Procedural GM Drum Kit (Channel 10)").strong(),
                    );
                });
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        render_drum_kit_tab(ui, app, &mut commands_to_send);
                    });
            } else {
                // Pitched Mode: Hide Drums & Cymbals, show exclusive algorithm tabs
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Synthesis Engine:").strong());

                    let current_engine = app.synth_lab.preset.engine;
                    if ui
                        .selectable_label(
                            current_engine == PitchedEngine::PhysicalString,
                            "1. Physical String (EKS + Body)",
                        )
                        .clicked()
                    {
                        app.synth_lab.active_tab = SynthEditorTab::PhysicalString;
                        app.synth_lab.preset.engine = PitchedEngine::PhysicalString;
                        app.set_part_synth_preset(app.synth_lab.target_part_idx, app.synth_lab.preset.clone());
                        commands_to_send.push(SynthCommand::SetPitchedEngine(PitchedEngine::PhysicalString));
                        commands_to_send.push(SynthCommand::SetPartEngine {
                            part_idx: app.synth_lab.target_part_idx as u8,
                            engine: PitchedEngine::PhysicalString,
                        });
                    }
                    if ui
                        .selectable_label(
                            current_engine == PitchedEngine::FmSynth,
                            "2. FM Synth (2-Operator)",
                        )
                        .clicked()
                    {
                        app.synth_lab.active_tab = SynthEditorTab::FmSynth;
                        app.synth_lab.preset.engine = PitchedEngine::FmSynth;
                        app.set_part_synth_preset(app.synth_lab.target_part_idx, app.synth_lab.preset.clone());
                        commands_to_send.push(SynthCommand::SetPitchedEngine(PitchedEngine::FmSynth));
                        commands_to_send.push(SynthCommand::SetPartEngine {
                            part_idx: app.synth_lab.target_part_idx as u8,
                            engine: PitchedEngine::FmSynth,
                        });
                    }
                });

                ui.add_space(4.0);

                egui::ScrollArea::vertical()
                    .auto_shrink([false, true])
                    .show(ui, |ui| match app.synth_lab.preset.engine {
                        PitchedEngine::PhysicalString => {
                            app.synth_lab.active_tab = SynthEditorTab::PhysicalString;
                            render_unified_string_and_body_panel(ui, app, &mut commands_to_send);
                        }
                        PitchedEngine::FmSynth => {
                            app.synth_lab.active_tab = SynthEditorTab::FmSynth;
                            render_fm_tab(ui, app, &mut commands_to_send);
                        }
                    });
            }
        });

    app.synth_lab.open = open;

    // Dispatch accumulated lock-free commands scoped to the target part
    if !commands_to_send.is_empty() {
        let _ = app.ensure_synth_engine();
        let target_p = app.synth_lab.target_part_idx as u8;
        let active_p = app.selected_part_idx as u8;
        if let Some(engine) = &mut app.synth_engine {
            engine.send(SynthCommand::SetActivePart(target_p));
            for cmd in commands_to_send {
                engine.send(cmd);
            }
            if target_p != active_p {
                engine.send(SynthCommand::SetActivePart(active_p));
            }
        }
    }

    if strum_open_requested {
        strum_current_part_open_strings(app);
    }

    if reconnect_requested {
        app.synth_engine = None;
        let _ = app.ensure_synth_engine();
        app.sync_live_play_config();
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

                    // Save to target part
                    app.set_part_synth_preset(app.synth_lab.target_part_idx, loaded.clone());

                    // Push full update to audio thread
                    let _ = app.ensure_synth_engine();
                    if let Some(engine) = &mut app.synth_engine {
                        engine.send(SynthCommand::SetInstrumentProfile(loaded.body_params.profile_base));
                        engine.send(SynthCommand::SetPitchedEngine(loaded.engine));
                        engine.send(SynthCommand::SetPartEngine {
                            part_idx: app.synth_lab.target_part_idx as u8,
                            engine: loaded.engine,
                        });
                        engine.send(SynthCommand::UpdateStringParams(loaded.string_params));
                        engine.send(SynthCommand::UpdateBodyParams(loaded.body_params));
                        for &role in DrumRole::all() {
                            engine.send(SynthCommand::UpdateDrumParams {
                                drum_role: role,
                                params: loaded.drum_params.params_for_role(role),
                            });
                        }
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
    _is_percussion: bool,
) {
    let factory_presets = SynthPreset::factory_presets();

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Preset:").strong());

        let current_name = app.synth_lab.preset.name.clone();

        egui::ComboBox::from_id_salt("synth_preset_combo")
            .selected_text(&current_name)
            .width(180.0)
            .show_ui(ui, |ui| {
                ui.label(egui::RichText::new("— Factory Presets —").italics().weak());
                for (i, p) in factory_presets.iter().enumerate() {
                    if ui.selectable_label(current_name == p.name, &p.name).clicked() {
                        app.synth_lab.selected_preset_idx = i;
                        app.synth_lab.preset = p.clone();
                        app.synth_lab.profile = p.body_params.profile_base;
                        app.synth_lab.pluck_pos = p.string_params.pluck_pos;
                        app.synth_lab.status_msg = Some(format!("Switched to preset '{}'", p.name));

                        app.set_part_synth_preset(app.synth_lab.target_part_idx, p.clone());

                        commands.push(SynthCommand::SetInstrumentProfile(p.body_params.profile_base));
                        commands.push(SynthCommand::SetPitchedEngine(p.engine));
                        commands.push(SynthCommand::SetPartEngine {
                            part_idx: app.synth_lab.target_part_idx as u8,
                            engine: p.engine,
                        });
                        commands.push(SynthCommand::UpdateStringParams(p.string_params));
                        commands.push(SynthCommand::UpdateBodyParams(p.body_params));
                        for &role in DrumRole::all() {
                            commands.push(SynthCommand::UpdateDrumParams {
                                drum_role: role,
                                params: p.drum_params.params_for_role(role),
                            });
                        }
                        commands.push(SynthCommand::UpdateGmVoiceParams(p.fm_params));
                    }
                }

                if !app.synth_lab.user_presets.is_empty() {
                    ui.separator();
                    ui.label(egui::RichText::new("— User Presets —").italics().weak());
                    let mut chosen_user = None;
                    for (u_i, p) in app.synth_lab.user_presets.iter().enumerate() {
                        let idx = factory_presets.len() + u_i;
                        if ui.selectable_label(app.synth_lab.selected_preset_idx == idx, &p.name).clicked() {
                            chosen_user = Some((idx, u_i));
                        }
                    }
                    if let Some((idx, u_i)) = chosen_user {
                        let p = app.synth_lab.user_presets[u_i].clone();
                        app.synth_lab.selected_preset_idx = idx;
                        app.synth_lab.preset = p.clone();
                        app.synth_lab.profile = p.body_params.profile_base;
                        app.synth_lab.status_msg = Some(format!("Switched to user preset '{}'", p.name));

                        app.set_part_synth_preset(app.synth_lab.target_part_idx, p.clone());

                        commands.push(SynthCommand::SetInstrumentProfile(p.body_params.profile_base));
                        commands.push(SynthCommand::SetPitchedEngine(p.engine));
                        commands.push(SynthCommand::SetPartEngine {
                            part_idx: app.synth_lab.target_part_idx as u8,
                            engine: p.engine,
                        });
                        commands.push(SynthCommand::UpdateStringParams(p.string_params));
                        commands.push(SynthCommand::UpdateBodyParams(p.body_params));
                        for &role in DrumRole::all() {
                            commands.push(SynthCommand::UpdateDrumParams {
                                drum_role: role,
                                params: p.drum_params.params_for_role(role),
                            });
                        }
                        commands.push(SynthCommand::UpdateGmVoiceParams(p.fm_params));
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
            app.set_part_synth_preset(app.synth_lab.target_part_idx, def.clone());

            commands.push(SynthCommand::SetInstrumentProfile(def.body_params.profile_base));
            commands.push(SynthCommand::SetPitchedEngine(def.engine));
            commands.push(SynthCommand::SetPartEngine {
                part_idx: app.synth_lab.target_part_idx as u8,
                engine: def.engine,
            });
            commands.push(SynthCommand::UpdateStringParams(def.string_params));
            commands.push(SynthCommand::UpdateBodyParams(def.body_params));
            for &role in DrumRole::all() {
                commands.push(SynthCommand::UpdateDrumParams {
                    drum_role: role,
                    params: def.drum_params.params_for_role(role),
                });
            }
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
                app.set_part_synth_preset(app.synth_lab.target_part_idx, new_p.clone());
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
            app.set_part_synth_preset(app.synth_lab.target_part_idx, app.synth_lab.preset.clone());
            commands.push(SynthCommand::UpdateBodyParams(app.synth_lab.preset.body_params));
            commands.push(SynthCommand::UpdatePartBodyParams {
                part_idx: app.synth_lab.target_part_idx as u8,
                params: app.synth_lab.preset.body_params,
            });
        }
        if gain_resp.double_clicked() {
            app.synth_lab.master_gain_db = 0.0;
            app.synth_lab.preset.body_params.post_gain_db = 0.0;
            app.set_part_synth_preset(app.synth_lab.target_part_idx, app.synth_lab.preset.clone());
            commands.push(SynthCommand::UpdateBodyParams(app.synth_lab.preset.body_params));
        }
    });
}

fn render_scoped_audition_strip(
    ui: &mut egui::Ui,
    app: &mut MnxEditorApp,
    commands: &mut Vec<SynthCommand>,
    strum_open: &mut bool,
    is_percussion: bool,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("Live Audition:").strong());

        if is_percussion {
            // Percussion Audition Strip dynamically scoped to the current part's mapped kit lines
            let target_part = app
                .parsed_mnx
                .as_ref()
                .and_then(|d| d.parts.get(app.synth_lab.target_part_idx));
            let mut quick_notes: Vec<(String, u8)> = Vec::new();
            if let Some(part) = target_part {
                for (_line, _id, comp) in part.ordered_kit_lines() {
                    let note = comp.sound.unwrap_or(38);
                    if !quick_notes.iter().any(|(_, n)| *n == note) {
                        let short = comp
                            .ext
                            .short_name
                            .as_deref()
                            .filter(|s| !s.trim().is_empty())
                            .or(comp.name.as_deref())
                            .unwrap_or(crate::presets::gm_percussion_name(note));
                        quick_notes.push((format!("{} ({})", short, note), note));
                    }
                    if quick_notes.len() >= 8 {
                        break;
                    }
                }
            }
            if quick_notes.is_empty() {
                quick_notes = vec![
                    ("Kick (36)".into(), 36),
                    ("Snare (38)".into(), 38),
                    ("Stick (37)".into(), 37),
                    ("Closed HH (42)".into(), 42),
                    ("Open HH (46)".into(), 46),
                    ("Crash 1 (49)".into(), 49),
                    ("Ride 1 (51)".into(), 51),
                    ("Bell (53)".into(), 53),
                ];
            }
            for (label, note) in quick_notes {
                if ui.button(label).clicked() {
                    commands.push(SynthCommand::TriggerDrum {
                        gm_drum_note: note,
                        velocity: app.synth_lab.velocity,
                    });
                }
            }
            if ui
                .button("Strum Kit Lines")
                .on_hover_text("Audition all defined kit lines for this percussion part")
                .clicked()
            {
                *strum_open = true;
            }
        } else {
            // Pitched Audition Strip (scoped to active PitchedEngine)
            let is_fm = app.synth_lab.preset.engine == PitchedEngine::FmSynth;

            let trigger_pitch = |cmds: &mut Vec<SynthCommand>, string_idx: u8, freq_hz: f64, vel: f32, pluck_p: f32| {
                cmds.push(SynthCommand::TriggerString {
                    string_idx,
                    freq_hz,
                    velocity: vel,
                    pluck_pos: pluck_p,
                    palm_mute: 0.0,
                    harmonic_node: None,
                });
            };

            if ui
                .button("Low E2 (82 Hz)")
                .on_hover_text("Trigger low string at 82.41 Hz (E2)")
                .clicked()
            {
                trigger_pitch(commands, 1, 82.40689, app.synth_lab.velocity, app.synth_lab.preset.string_params.pluck_pos);
            }

            if ui
                .button("A2 (110 Hz)")
                .on_hover_text("Trigger string at 110.00 Hz (A2)")
                .clicked()
            {
                trigger_pitch(commands, 2, 110.00, app.synth_lab.velocity, app.synth_lab.preset.string_params.pluck_pos);
            }

            if ui
                .button("D3 (147 Hz)")
                .on_hover_text("Trigger string at 146.83 Hz (D3)")
                .clicked()
            {
                trigger_pitch(commands, 3, 146.8324, app.synth_lab.velocity, app.synth_lab.preset.string_params.pluck_pos);
            }

            if ui
                .button("High E4 (330 Hz)")
                .on_hover_text("Trigger high string at 329.63 Hz (E4)")
                .clicked()
            {
                trigger_pitch(commands, 6, 329.6276, app.synth_lab.velocity, app.synth_lab.preset.string_params.pluck_pos);
            }

            if ui
                .button("Hammer-On Slur")
                .on_hover_text("Slur string 2 to C3 (130.81 Hz) using the active engine (Physical String or FM Synth)")
                .clicked()
            {
                commands.push(SynthCommand::SlurString {
                    string_idx: 2,
                    new_freq_hz: 130.8128,
                    hammer_energy: app.synth_lab.velocity * 0.85,
                });
            }

            if ui
                .button("Strum Chord (JI 7-Limit)")
                .on_hover_text("Strum a 4-note pure Just Intonation harmonic chord (1/1, 5/4, 3/2, 7/4)")
                .clicked()
            {
                for (idx, ratio) in [(1, 1.0), (2, 1.25), (3, 1.50), (4, 1.75)] {
                    let f = 196.0 * ratio; // G3 root
                    trigger_pitch(commands, idx, f, app.synth_lab.velocity * 0.9, app.synth_lab.preset.string_params.pluck_pos);
                }
            }

            if !is_fm {
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
            }

            if ui
                .button("Strum Tab Open Strings")
                .on_hover_text("Strum all open strings of the current tab part using exact microtonal tuning")
                .clicked()
            {
                *strum_open = true;
            }
        }
    });
}

fn render_visual_preview_canvas(
    ui: &mut egui::Ui,
    state: &crate::synth::SynthTestLabState,
    sample_rate: f32,
    is_percussion: bool,
) {
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

    if is_percussion {
        paint_drum_envelopes_preview(&painter, rect, &state.preset.drum_params, state.active_drum_role);
    } else {
        match state.preset.engine {
            PitchedEngine::PhysicalString => {
                paint_frequency_response_preview(&painter, rect, &state.preset.body_params, sample_rate);
            }
            PitchedEngine::FmSynth => {
                paint_fm_adsr_preview(&painter, rect, &state.preset.fm_params);
            }
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

/// Paints procedural drum envelopes and waveforms for any of the 28 active drum/percussion roles.
fn paint_drum_envelopes_preview(
    painter: &egui::Painter,
    rect: Rect,
    params: &DrumKitParams,
    active_role: DrumRole,
) {
    let half_w = rect.width() * 0.48;
    let role_name = active_role.display_name();

    match params.params_for_role(active_role) {
        DrumVoiceParams::Kick(k) => {
            let mut pitch_pts: Vec<Pos2> = Vec::with_capacity(41);
            let p_span = k.pitch_start_hz - k.pitch_end_hz;
            for s in 0..=40 {
                let t = (s as f32) / 40.0;
                let env = (-t * (100.0 / k.pitch_decay_ms.max(5.0))).exp();
                let f = k.pitch_end_hz + p_span * env;
                let norm_f = (f - 30.0) / (250.0 - 30.0);
                let px = rect.min.x + t * half_w;
                let py = rect.max.y - (norm_f.clamp(0.0, 1.0) * (rect.height() - 18.0) + 9.0);
                pitch_pts.push(Pos2::new(px, py));
            }
            painter.add(egui::Shape::line(pitch_pts, Stroke::new(2.0, Color32::from_rgb(255, 140, 90))));

            let right_start = rect.min.x + half_w + 10.0;
            let right_w = rect.max.x - right_start;
            let mut body_pts: Vec<Pos2> = Vec::with_capacity(41);
            for s in 0..=40 {
                let t = (s as f32) / 40.0;
                let amp = (-t * (400.0 / k.body_decay_ms.max(10.0))).exp();
                let px = right_start + t * right_w;
                let py = rect.max.y - (amp.clamp(0.0, 1.0) * (rect.height() - 18.0) + 9.0);
                body_pts.push(Pos2::new(px, py));
            }
            painter.add(egui::Shape::line(body_pts, Stroke::new(2.0, Color32::from_rgb(100, 220, 140))));

            painter.text(
                Pos2::new(rect.min.x + 8.0, rect.min.y + 6.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "{}: Pitch {:.0}->{:.0}Hz ({:.0}ms) | Body Decay {:.0}ms | Vol {:.1} dB",
                    role_name, k.pitch_start_hz, k.pitch_end_hz, k.pitch_decay_ms, k.body_decay_ms, k.volume_db
                ),
                egui::FontId::proportional(11.0),
                Color32::from_rgb(255, 200, 160),
            );
        }
        DrumVoiceParams::Snare(s) => {
            let mut tone_pts: Vec<Pos2> = Vec::with_capacity(41);
            let mut wire_pts: Vec<Pos2> = Vec::with_capacity(41);
            for i in 0..=40 {
                let t = (i as f32) / 40.0;
                let amp_t = (-t * (220.0 / s.tone_decay_ms.max(10.0))).exp();
                let amp_w = (-t * (300.0 / s.noise_decay_ms.max(10.0))).exp();
                let px = rect.min.x + t * rect.width();
                tone_pts.push(Pos2::new(px, rect.max.y - (amp_t.clamp(0.0, 1.0) * (rect.height() - 18.0) + 9.0)));
                wire_pts.push(Pos2::new(px, rect.max.y - (amp_w.clamp(0.0, 1.0) * (rect.height() - 18.0) + 9.0)));
            }
            painter.add(egui::Shape::line(wire_pts, Stroke::new(1.8, Color32::from_rgb(120, 190, 255))));
            painter.add(egui::Shape::line(tone_pts, Stroke::new(1.8, Color32::from_rgb(255, 190, 110))));

            painter.text(
                Pos2::new(rect.min.x + 8.0, rect.min.y + 6.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "{}: Modes {:.0}/{:.0}Hz | Wire {:.0}Hz ({:.0}ms) | Vol {:.1} dB",
                    role_name, s.tone_freq_hz, s.mode2_freq_hz, s.wire_cutoff_hz, s.noise_decay_ms, s.volume_db
                ),
                egui::FontId::proportional(11.0),
                Color32::from_rgb(200, 220, 255),
            );
        }
        DrumVoiceParams::HandClap(c) => {
            let mut clap_pts: Vec<Pos2> = Vec::with_capacity(61);
            for i in 0..=60 {
                let t = (i as f32) / 60.0;
                let ms = t * 240.0;
                let burst = if ms < c.clap_spacing_ms * 3.2 {
                    0.45 + 0.55 * ((ms / c.clap_spacing_ms.max(3.0)) * std::f32::consts::TAU).cos().abs()
                } else {
                    (-(ms - c.clap_spacing_ms * 3.2) / c.tail_decay_ms.max(20.0)).exp()
                };
                let px = rect.min.x + t * rect.width();
                let py = rect.max.y - (burst.clamp(0.0, 1.0) * (rect.height() - 18.0) + 9.0);
                clap_pts.push(Pos2::new(px, py));
            }
            painter.add(egui::Shape::line(clap_pts, Stroke::new(2.0, Color32::from_rgb(255, 170, 190))));

            painter.text(
                Pos2::new(rect.min.x + 8.0, rect.min.y + 6.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "{}: Burst {:.1}ms | Formant {:.0}Hz (Q {:.1}) | Tail {:.0}ms | Vol {:.1} dB",
                    role_name, c.clap_spacing_ms, c.bandpass_hz, c.bandpass_q, c.tail_decay_ms, c.volume_db
                ),
                egui::FontId::proportional(11.0),
                Color32::from_rgb(255, 210, 220),
            );
        }
        DrumVoiceParams::Toms(t) => {
            let mut tom_pts: Vec<Pos2> = Vec::with_capacity(41);
            for i in 0..=40 {
                let u = (i as f32) / 40.0;
                let amp = (-u * (400.0 / t.decay_ms.max(10.0))).exp();
                let px = rect.min.x + u * rect.width();
                let py = rect.max.y - (amp.clamp(0.0, 1.0) * (rect.height() - 18.0) + 9.0);
                tom_pts.push(Pos2::new(px, py));
            }
            painter.add(egui::Shape::line(tom_pts, Stroke::new(2.0, Color32::from_rgb(220, 160, 240))));

            painter.text(
                Pos2::new(rect.min.x + 8.0, rect.min.y + 6.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "{}: Pitch {:.2}x | Drop {:.2} | Decay {:.0}ms | Shell {:.2} | Vol {:.1} dB",
                    role_name, t.pitch_scale, t.pitch_drop_ratio, t.decay_ms, t.shell_wood_cavity, t.volume_db
                ),
                egui::FontId::proportional(11.0),
                Color32::from_rgb(235, 205, 250),
            );
        }
        DrumVoiceParams::HiHats(h) => {
            let mut closed_pts: Vec<Pos2> = Vec::with_capacity(41);
            let mut open_pts: Vec<Pos2> = Vec::with_capacity(41);
            for i in 0..=40 {
                let u = (i as f32) / 40.0;
                let amp_c = (-u * (100.0 / h.closed_decay_ms.max(5.0))).exp();
                let amp_o = (-u * (800.0 / h.open_decay_ms.max(10.0))).exp();
                let px = rect.min.x + u * rect.width();
                closed_pts.push(Pos2::new(px, rect.max.y - (amp_c * (rect.height() - 20.0) + 10.0)));
                open_pts.push(Pos2::new(px, rect.max.y - (amp_o * (rect.height() - 20.0) + 10.0)));
            }
            painter.add(egui::Shape::line(open_pts, Stroke::new(1.5, Color32::from_rgb(255, 220, 120))));
            painter.add(egui::Shape::line(closed_pts, Stroke::new(2.0, Color32::from_rgb(140, 240, 220))));

            painter.text(
                Pos2::new(rect.min.x + 8.0, rect.min.y + 6.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "{}: HP {:.0}Hz | Closed {:.0}ms ({:.1}dB) | Open {:.0}ms ({:.1}dB)",
                    role_name, h.highpass_cutoff_hz, h.closed_decay_ms, h.closed_volume_db, h.open_decay_ms, h.open_volume_db
                ),
                egui::FontId::proportional(11.0),
                Color32::from_rgb(240, 240, 180),
            );
        }
        DrumVoiceParams::SingleCymbal(c) => {
            let mut pts: Vec<Pos2> = Vec::with_capacity(41);
            for i in 0..=40 {
                let u = (i as f32) / 40.0;
                let bloom = (u * 12.0).min(1.0);
                let amp = bloom * (-u * (1500.0 / c.decay_ms.max(30.0))).exp();
                let px = rect.min.x + u * rect.width();
                pts.push(Pos2::new(px, rect.max.y - (amp * (rect.height() - 20.0) + 10.0)));
            }
            painter.add(egui::Shape::line(pts, Stroke::new(2.0, Color32::from_rgb(255, 145, 185))));

            painter.text(
                Pos2::new(rect.min.x + 8.0, rect.min.y + 6.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "{}: Size {:.2}x | Cutoff {:.0}Hz | Decay {:.0}ms | Shimmer {:.2} | Vol {:.1} dB",
                    role_name, c.bronze_size_pitch, c.cutoff_hz, c.decay_ms, c.nonlin_shimmer, c.volume_db
                ),
                egui::FontId::proportional(11.0),
                Color32::from_rgb(255, 205, 225),
            );
        }
        DrumVoiceParams::RideCymbal(r) => {
            let mut bow_pts: Vec<Pos2> = Vec::with_capacity(41);
            let mut bell_pts: Vec<Pos2> = Vec::with_capacity(41);
            for i in 0..=40 {
                let u = (i as f32) / 40.0;
                let amp_bow = (-u * (1500.0 / r.bow_decay_ms.max(30.0))).exp();
                let amp_bell = (-u * (1500.0 / r.bell_decay_ms.max(30.0))).exp();
                let px = rect.min.x + u * rect.width();
                bow_pts.push(Pos2::new(px, rect.max.y - (amp_bow * (rect.height() - 20.0) + 10.0)));
                bell_pts.push(Pos2::new(px, rect.max.y - (amp_bell * (rect.height() - 20.0) + 10.0)));
            }
            painter.add(egui::Shape::line(bow_pts, Stroke::new(1.8, Color32::from_rgb(150, 210, 255))));
            painter.add(egui::Shape::line(bell_pts, Stroke::new(1.8, Color32::from_rgb(255, 215, 120))));

            painter.text(
                Pos2::new(rect.min.x + 8.0, rect.min.y + 6.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "{}: Bow {:.0}ms ({:.1}dB) | Bell {:.0}ms ({:.1}dB) | Size {:.2}x",
                    role_name, r.bow_decay_ms, r.bow_volume_db, r.bell_decay_ms, r.bell_volume_db, r.bronze_size_pitch
                ),
                egui::FontId::proportional(11.0),
                Color32::from_rgb(210, 230, 255),
            );
        }
        DrumVoiceParams::PercAux(a) => {
            let mut aux_pts: Vec<Pos2> = Vec::with_capacity(41);
            for i in 0..=40 {
                let u = (i as f32) / 40.0;
                let amp = (-u * (350.0 / a.decay_ms.max(10.0))).exp();
                let px = rect.min.x + u * rect.width();
                aux_pts.push(Pos2::new(px, rect.max.y - (amp * (rect.height() - 20.0) + 10.0)));
            }
            painter.add(egui::Shape::line(aux_pts, Stroke::new(2.0, Color32::from_rgb(255, 215, 110))));

            painter.text(
                Pos2::new(rect.min.x + 8.0, rect.min.y + 6.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "{}: Modes {:.0}Hz + {:.0}Hz (Q {:.1}) | Decay {:.0}ms | Vol {:.1} dB",
                    role_name, a.freq1_hz, a.freq2_hz, a.bandpass_q, a.decay_ms, a.volume_db
                ),
                egui::FontId::proportional(11.0),
                Color32::from_rgb(255, 235, 180),
            );
        }
        DrumVoiceParams::ShakerEffect(s) => {
            let mut pts: Vec<Pos2> = Vec::with_capacity(51);
            for i in 0..=50 {
                let u = (i as f32) / 50.0;
                let env = (-u * (450.0 / s.decay_ms.max(15.0))).exp();
                let mod_w = 0.65 + 0.35 * (u * (8.0 + 18.0 * s.texture_rate) * std::f32::consts::TAU).sin().abs();
                let amp = env * mod_w;
                let px = rect.min.x + u * rect.width();
                pts.push(Pos2::new(px, rect.max.y - (amp.clamp(0.0, 1.0) * (rect.height() - 20.0) + 10.0)));
            }
            painter.add(egui::Shape::line(pts, Stroke::new(2.0, Color32::from_rgb(130, 235, 195))));

            painter.text(
                Pos2::new(rect.min.x + 8.0, rect.min.y + 6.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "{}: Bright {:.0}Hz | Decay {:.0}ms | Pitch {:.2}x | Texture {:.2} | Vol {:.1} dB",
                    role_name, s.brightness_hz, s.decay_ms, s.pitch_scale, s.texture_rate, s.volume_db
                ),
                egui::FontId::proportional(11.0),
                Color32::from_rgb(195, 245, 225),
            );
        }
    }
}

/// Paints the FM Carrier & Modulator ADSR curves side-by-side.
fn paint_fm_adsr_preview(
    painter: &egui::Painter,
    rect: Rect,
    params: &FmVoiceParams,
) {
    let half_w = rect.width() * 0.48;

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
// Inspector Panels: Unified Physical String, Expanded GM Drum Kit, FM Synth
// -----------------------------------------------------------------------------

/// Unified panel merging Extended Karplus-Strong string waveguide and post-string Body/Amp Resonator.
fn render_unified_string_and_body_panel(
    ui: &mut egui::Ui,
    app: &mut MnxEditorApp,
    commands: &mut Vec<SynthCommand>,
) {
    let mut str_changed = false;
    let mut body_changed = false;

    // Archetype Profile Selector
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Instrument Profile:").strong());

            let current_prof = app.synth_lab.preset.body_params.profile_base;
            for prof in [
                InstrumentProfile::AcousticGuitar,
                InstrumentProfile::ElectricClean,
                InstrumentProfile::ElectricOverdrive,
                InstrumentProfile::FingeredBass,
                InstrumentProfile::PickedBass,
                InstrumentProfile::HarpOrLute,
            ] {
                if ui.selectable_label(current_prof == prof, prof.label()).clicked() {
                    app.synth_lab.profile = prof;
                    app.synth_lab.preset.body_params.profile_base = prof;
                    commands.push(SynthCommand::SetInstrumentProfile(prof));
                    body_changed = true;
                }
            }
        });
    });

    ui.add_space(4.0);

    let mut p = app.synth_lab.preset.string_params;
    let mut bp = app.synth_lab.preset.body_params;

    // Two-Column Grid: Left = String & Pluck (EKS), Right = Body & Amp Resonator
    ui.columns(2, |cols| {
        // ---------------------------------------------------------
        // Column 1: String & Pluck (Extended Karplus-Strong)
        // ---------------------------------------------------------
        cols[0].group(|ui| {
            ui.label(egui::RichText::new("String Waveguide (EKS):").strong());
            ui.add_space(2.0);

            egui::Grid::new("unified_string_params_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Pluck Position:");
                    let resp = ui.add(
                        egui::Slider::new(&mut p.pluck_pos, 0.05..=0.50)
                            .text("bridge -> neck"),
                    );
                    if resp.changed() { str_changed = true; }
                    if resp.double_clicked() { p.pluck_pos = 0.18; str_changed = true; }
                    ui.end_row();

                    ui.label("Pick Hardness:");
                    let resp = ui.add(
                        egui::Slider::new(&mut p.pick_hardness, 500.0..=16_000.0)
                            .logarithmic(true)
                            .suffix(" Hz"),
                    );
                    if resp.changed() { str_changed = true; }
                    if resp.double_clicked() { p.pick_hardness = 8_500.0; str_changed = true; }
                    ui.end_row();

                    ui.label("Pluck Noise Mix:");
                    let resp = ui.add(egui::Slider::new(&mut p.noise_mix, 0.0..=1.0));
                    if resp.changed() { str_changed = true; }
                    if resp.double_clicked() { p.noise_mix = 0.65; str_changed = true; }
                    ui.end_row();

                    ui.label("Sustain (T60):");
                    let resp = ui.add(
                        egui::Slider::new(&mut p.t60_decay_sec, 0.1..=10.0)
                            .suffix(" s")
                            .logarithmic(true),
                    );
                    if resp.changed() { str_changed = true; }
                    if resp.double_clicked() { p.t60_decay_sec = 3.5; str_changed = true; }
                    ui.end_row();

                    ui.label("Brightness (S):");
                    let resp = ui.add(egui::Slider::new(&mut p.brightness_s, 0.01..=0.99));
                    if resp.changed() { str_changed = true; }
                    if resp.double_clicked() { p.brightness_s = 0.16; str_changed = true; }
                    ui.end_row();

                    ui.label("Stiffness Dispersion:");
                    let resp = ui.add(egui::Slider::new(&mut p.inharmonicity_dispersion, 0.0..=0.80));
                    if resp.changed() { str_changed = true; }
                    if resp.double_clicked() { p.inharmonicity_dispersion = 0.12; str_changed = true; }
                    ui.end_row();

                    ui.label("Palm Mute Damping:");
                    let resp = ui.add(egui::Slider::new(&mut p.palm_mute_damping, 0.01..=0.80));
                    if resp.changed() { str_changed = true; }
                    if resp.double_clicked() { p.palm_mute_damping = 0.22; str_changed = true; }
                    ui.end_row();

                    ui.label("Harmonic Damping:");
                    let resp = ui.add(
                        egui::Slider::new(&mut p.harmonic_damping_ms, 10.0..=500.0)
                            .suffix(" ms"),
                    );
                    if resp.changed() { str_changed = true; }
                    if resp.double_clicked() { p.harmonic_damping_ms = 30.0; str_changed = true; }
                    ui.end_row();
                });

            ui.add_space(4.0);
            ui.separator();
            ui.label(egui::RichText::new("Physical Modeling Realism:").strong());
            ui.add_space(2.0);

            egui::Grid::new("unified_string_realism_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Dual-Polarization Bloom:")
                        .on_hover_text(format!(
                            "Coupled H/V waveguides with two-stage decay & harmonic bloom (detune {:.2} cents, coupling {:.4})",
                            p.polarization_detune_cents, p.polarization_coupling
                        ));
                    let mut bloom = (p.polarization_coupling / 0.024).clamp(0.0, 1.0);
                    let resp = ui.add(egui::Slider::new(&mut bloom, 0.0..=1.0));
                    if resp.changed() {
                        p.polarization_coupling = bloom * 0.024;
                        p.polarization_detune_cents = bloom * 1.4;
                        str_changed = true;
                    }
                    if resp.double_clicked() {
                        p.polarization_coupling = 0.012;
                        p.polarization_detune_cents = 0.7;
                        str_changed = true;
                    }
                    ui.end_row();

                    ui.label("Tension Pitch Bend:")
                        .on_hover_text("Energy-driven string tension pitch drop over the first 50–100 ms on hard plucks");
                    let resp = ui.add(egui::Slider::new(&mut p.tension_mod_amount, 0.0..=1.0));
                    if resp.changed() { str_changed = true; }
                    if resp.double_clicked() { p.tension_mod_amount = 0.30; str_changed = true; }
                    ui.end_row();

                    ui.label("Fret Buzz Sizzle:")
                        .on_hover_text("Asymmetric fret-clearance soft-clipping barrier triggered only on high-velocity plucks");
                    let resp = ui.add(egui::Slider::new(&mut p.fret_buzz_sizzle, 0.0..=1.0));
                    if resp.changed() { str_changed = true; }
                    if resp.double_clicked() { p.fret_buzz_sizzle = 0.20; str_changed = true; }
                    ui.end_row();

                    ui.label("Sympathetic Resonance:")
                        .on_hover_text(format!(
                            "Passive bridge-bus coupling across unmuted strings for natural chord & open-string bloom (coupling {:.4})",
                            p.sympathetic_coupling
                        ));
                    let mut symp_norm = (p.sympathetic_coupling / 0.010).clamp(0.0, 1.0);
                    let resp = ui.add(egui::Slider::new(&mut symp_norm, 0.0..=1.0));
                    if resp.changed() {
                        p.sympathetic_coupling = symp_norm * 0.010;
                        str_changed = true;
                    }
                    if resp.double_clicked() {
                        p.sympathetic_coupling = 0.0025;
                        str_changed = true;
                    }
                    ui.end_row();
                });
        });

        // ---------------------------------------------------------
        // Column 2: Body Resonator & Cabinet Filter Bank
        // ---------------------------------------------------------
        cols[1].group(|ui| {
            ui.label(egui::RichText::new("Body & Resonator:").strong());
            ui.add_space(2.0);

            egui::Grid::new("unified_body_core_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Wet/Dry Mix:");
                    let resp = ui.add(egui::Slider::new(&mut bp.body_wet_mix, 0.0..=1.0));
                    if resp.changed() { body_changed = true; }
                    if resp.double_clicked() { bp.body_wet_mix = 0.35; body_changed = true; }
                    ui.end_row();

                    ui.label("Wood Modal Knock / Air:")
                        .on_hover_text("12-mode high-Q wood/cabinet resonant bank + 4-tap Householder micro soundboard diffusion (< 2.5 ms)");
                    let resp = ui.add(egui::Slider::new(&mut bp.modal_knock_air, 0.0..=1.0));
                    if resp.changed() { body_changed = true; }
                    if resp.double_clicked() { bp.modal_knock_air = 0.55; body_changed = true; }
                    ui.end_row();

                    ui.label("Overdrive Drive:");
                    let resp = ui.add(egui::Slider::new(&mut bp.pre_drive, 0.0..=5.0));
                    if resp.changed() { body_changed = true; }
                    if resp.double_clicked() { bp.pre_drive = 1.0; body_changed = true; }
                    ui.end_row();

                    ui.label("Makeup Gain:");
                    let resp = ui.add(
                        egui::Slider::new(&mut bp.post_gain_db, -18.0..=12.0)
                            .suffix(" dB"),
                    );
                    if resp.changed() { body_changed = true; }
                    if resp.double_clicked() { bp.post_gain_db = 0.0; body_changed = true; }
                    ui.end_row();
                });

            ui.add_space(2.0);
            ui.label(egui::RichText::new("Parametric 4-Band EQ:").strong());

            let band_labels = [
                "B1: Helmholtz (~98Hz)",
                "B2: Wood Dipole (~205Hz)",
                "B3: Pickup Peak (~2.8kHz)",
                "B4: Cabinet Cone (~5kHz)",
            ];

            for (idx, b) in bp.bands.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut b.enabled, "").changed() {
                        body_changed = true;
                    }
                    ui.label(egui::RichText::new(band_labels[idx]).size(11.0));
                });
                if b.enabled {
                    ui.horizontal(|ui| {
                        ui.label("F:");
                        if ui
                            .add(egui::DragValue::new(&mut b.freq_hz).speed(2.0).range(20.0..=18_000.0).suffix("Hz"))
                            .changed()
                        {
                            body_changed = true;
                        }
                        ui.label("Q:");
                        if ui
                            .add(egui::DragValue::new(&mut b.q).speed(0.05).range(0.1..=15.0))
                            .changed()
                        {
                            body_changed = true;
                        }
                        ui.label("G:");
                        if ui
                            .add(egui::DragValue::new(&mut b.gain_db).speed(0.2).range(-24.0..=24.0).suffix("dB"))
                            .changed()
                        {
                            body_changed = true;
                        }
                    });
                }
            }
        });
    });

    if str_changed {
        app.synth_lab.preset.string_params = p;
        app.set_part_synth_preset(app.synth_lab.target_part_idx, app.synth_lab.preset.clone());
        commands.push(SynthCommand::UpdateStringParams(p));
        commands.push(SynthCommand::UpdatePartStringParams {
            part_idx: app.synth_lab.target_part_idx as u8,
            params: p,
        });
    }

    if body_changed {
        app.synth_lab.preset.body_params = bp;
        app.set_part_synth_preset(app.synth_lab.target_part_idx, app.synth_lab.preset.clone());
        commands.push(SynthCommand::UpdateBodyParams(bp));
        commands.push(SynthCommand::UpdatePartBodyParams {
            part_idx: app.synth_lab.target_part_idx as u8,
            params: bp,
        });
    }
}

/// A single entry in the Drum Tone Editor's navigable instrument/group palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrumPaletteEntry {
    pub role: DrumRole,
    /// Distinct GM MIDI notes belonging to this role that are mapped on the current percussion part.
    pub mapped_notes: Vec<u8>,
    /// Human-readable tab line/symbol bindings for this role (e.g., `"L1 CC (x)"`).
    pub tab_bindings: Vec<String>,
}

impl DrumPaletteEntry {
    /// Returns the preferred GM note to audition when stepping onto this role.
    pub fn primary_note(&self) -> u8 {
        self.mapped_notes
            .first()
            .copied()
            .unwrap_or_else(|| self.role.default_note())
    }
}

/// Discovers the set of editable drum/percussion roles from the active percussion part's tab kit
/// mapping (`comp.sound` + all `comp.ext.symbol_sounds`).
///
/// When `show_all_gm` is `false` (default) and the part has mapped kit lines, returns ONLY the
/// roles present in the kit's palette (e.g., hiding Agogo/Cuica/Whistle on a rock kit).
/// When `show_all_gm` is `true` (or no kit lines are mapped), returns all 28 GM roles in canonical
/// step-through order while still annotating mapped roles with their tab line/symbol bindings.
///
/// Returns `(visible_entries, total_mapped_roles_in_kit)`.
pub fn collect_part_drum_palette(
    part: Option<&crate::mnx::Part>,
    show_all_gm: bool,
) -> (Vec<DrumPaletteEntry>, usize) {
    let mut role_notes: std::collections::HashMap<DrumRole, Vec<u8>> =
        std::collections::HashMap::new();
    let mut role_bindings: std::collections::HashMap<DrumRole, Vec<String>> =
        std::collections::HashMap::new();

    if let Some(p) = part {
        if p.is_percussion() {
            for (line_num, _id, comp) in p.ordered_kit_lines() {
                let short = comp
                    .ext
                    .short_name
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .or(comp.name.as_deref())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("L{}", line_num));

                let mut register_note = |note: u8, sym: &str| {
                    let role = DrumRole::from_gm_note(note);
                    let notes = role_notes.entry(role).or_default();
                    if !notes.contains(&note) {
                        notes.push(note);
                    }
                    let binding = format!("L{} {} ({}→{})", line_num, short, sym, note);
                    let binds = role_bindings.entry(role).or_default();
                    if !binds.contains(&binding) {
                        binds.push(binding);
                    }
                };

                let def_note = comp.sound.unwrap_or(38);
                register_note(def_note, comp.effective_default_symbol());
                for (sym, note) in &comp.ext.symbol_sounds {
                    register_note(*note, sym);
                }
            }
        }
    }

    // Sort mapped notes within each role by GM note number for deterministic display
    for notes in role_notes.values_mut() {
        notes.sort_unstable();
    }

    let mapped_role_count = role_notes.len();
    let include_unmapped = show_all_gm || mapped_role_count == 0;

    let mut entries = Vec::new();
    for &role in DrumRole::all() {
        let mapped_notes = role_notes.remove(&role).unwrap_or_default();
        let tab_bindings = role_bindings.remove(&role).unwrap_or_default();
        if include_unmapped || !mapped_notes.is_empty() {
            entries.push(DrumPaletteEntry {
                role,
                mapped_notes,
                tab_bindings,
            });
        }
    }

    (entries, mapped_role_count)
}

/// Returns `true` if a drum role is completely muted (`<= -47.9 dB` across all its output channels).
fn drum_role_is_muted(dp: &DrumKitParams, role: DrumRole) -> bool {
    match dp.params_for_role(role) {
        DrumVoiceParams::Kick(k) => k.volume_db <= -47.9,
        DrumVoiceParams::Snare(s) => {
            if role == DrumRole::Snare {
                s.volume_db <= -47.9 && s.rim_volume_db <= -47.9
            } else {
                s.volume_db <= -47.9
            }
        }
        DrumVoiceParams::HandClap(c) => c.volume_db <= -47.9,
        DrumVoiceParams::Toms(t) => t.volume_db <= -47.9,
        DrumVoiceParams::HiHats(h) => {
            h.closed_volume_db <= -47.9
                && h.pedal_volume_db <= -47.9
                && h.open_volume_db <= -47.9
        }
        DrumVoiceParams::SingleCymbal(c) => c.volume_db <= -47.9,
        DrumVoiceParams::RideCymbal(r) => r.bow_volume_db <= -47.9 && r.bell_volume_db <= -47.9,
        DrumVoiceParams::PercAux(a) => a.volume_db <= -47.9,
        DrumVoiceParams::ShakerEffect(s) => s.volume_db <= -47.9,
    }
}

/// Compact helper for adding a `-48.0..=+12.0 dB` absolute volume slider row to an `egui::Grid`.
fn add_drum_volume_slider_row(
    ui: &mut egui::Ui,
    label: &str,
    tooltip: &str,
    value_db: &mut f32,
    changed: &mut bool,
) {
    ui.label(egui::RichText::new(label).strong())
        .on_hover_text(tooltip);
    let r = ui.add(
        egui::Slider::new(value_db, -48.0..=12.0)
            .suffix(" dB")
            .fixed_decimals(1)
            .clamping(egui::SliderClamping::Always),
    );
    if r.changed() {
        *changed = true;
    }
    if r.double_clicked() {
        *value_db = 0.0;
        *changed = true;
    }
    ui.end_row();
}

/// Compact helper for adding the logarithmic `Hit Variation` slider row to an `egui::Grid`.
fn add_hit_variation_slider_row(
    ui: &mut egui::Ui,
    value: &mut f32,
    tooltip: &str,
    changed: &mut bool,
) {
    ui.label("Hit Variation (Vel & Strike):").on_hover_text(tooltip);
    let r = ui.add(
        egui::Slider::new(value, 0.0..=1.0)
            .logarithmic(true)
            .smallest_positive(0.005),
    );
    if r.changed() {
        *changed = true;
    }
    if r.double_clicked() {
        *value = 0.10;
        *changed = true;
    }
    ui.end_row();
}

/// Kit-scoped Master-Detail Drum & Percussion Tone Editor (28 logical roles covering all 47 GM notes).
fn render_drum_kit_tab(ui: &mut egui::Ui, app: &mut MnxEditorApp, commands: &mut Vec<SynthCommand>) {
    let mut dp = app.synth_lab.preset.drum_params;
    let target_idx = app.synth_lab.target_part_idx;

    let target_part = app
        .parsed_mnx
        .as_ref()
        .and_then(|d| d.parts.get(target_idx));
    let (palette, mapped_role_count) =
        collect_part_drum_palette(target_part, app.synth_lab.drum_show_all_gm);

    if palette.is_empty() {
        ui.label("No drum voices available.");
        return;
    }

    // Keep active_drum_role clamped to the currently visible palette
    let mut active_idx = palette
        .iter()
        .position(|e| e.role == app.synth_lab.active_drum_role)
        .unwrap_or(0);
    if palette[active_idx].role != app.synth_lab.active_drum_role {
        app.synth_lab.active_drum_role = palette[active_idx].role;
        app.synth_lab.active_drum_note = palette[active_idx].primary_note();
    }

    let select_role = |app: &mut MnxEditorApp,
                       entry: &DrumPaletteEntry,
                       cmds: &mut Vec<SynthCommand>,
                       force_audition: bool| {
        app.synth_lab.active_drum_role = entry.role;
        let note = entry.primary_note();
        app.synth_lab.active_drum_note = note;
        if force_audition || app.synth_lab.drum_auto_audition_on_step {
            cmds.push(SynthCommand::TriggerDrum {
                gm_drum_note: note,
                velocity: app.synth_lab.velocity,
            });
        }
    };

    // Two-Pane Master-Detail Layout: Left Kit Navigator Rail | Right Active Voice Inspector
    ui.horizontal_top(|ui| {
        // -----------------------------------------------------------------
        // LEFT RAIL: Kit-Scoped Instrument Navigator & Step-Through Controls
        // -----------------------------------------------------------------
        ui.vertical(|ui| {
            ui.set_min_width(218.0);
            ui.set_max_width(228.0);

            // Scope toggle pills: Kit Palette vs All GM (28)
            ui.horizontal(|ui| {
                if mapped_role_count > 0 {
                    let kit_label = format!("Kit Palette ({})", mapped_role_count);
                    if ui
                        .selectable_label(!app.synth_lab.drum_show_all_gm, kit_label)
                        .on_hover_text("Show only drum & percussion instruments mapped in this tab's kit")
                        .clicked()
                    {
                        app.synth_lab.drum_show_all_gm = false;
                    }
                    if ui
                        .selectable_label(app.synth_lab.drum_show_all_gm, "All GM (28)")
                        .on_hover_text("Show all 28 GM drum & percussion groups (notes 35–81)")
                        .clicked()
                    {
                        app.synth_lab.drum_show_all_gm = true;
                    }
                } else {
                    ui.label(egui::RichText::new("All GM Percussion (28)").strong());
                }
            });

            ui.add_space(3.0);

            // Step-through navigation bar: < Prev | idx / total | Next >
            ui.horizontal(|ui| {
                if ui
                    .button("< Prev")
                    .on_hover_text("Step to previous instrument in palette")
                    .clicked()
                {
                    let next_idx = if active_idx == 0 {
                        palette.len() - 1
                    } else {
                        active_idx - 1
                    };
                    active_idx = next_idx;
                    select_role(app, &palette[next_idx], commands, false);
                }

                ui.label(
                    egui::RichText::new(format!("{} / {}", active_idx + 1, palette.len()))
                        .monospace()
                        .strong(),
                );

                if ui
                    .button("Next >")
                    .on_hover_text("Step to next instrument in palette")
                    .clicked()
                {
                    let next_idx = (active_idx + 1) % palette.len();
                    active_idx = next_idx;
                    select_role(app, &palette[next_idx], commands, false);
                }
            });

            ui.checkbox(
                &mut app.synth_lab.drum_auto_audition_on_step,
                "Auto-audition on step",
            )
            .on_hover_text("Automatically trigger a preview hit when stepping Prev/Next or selecting a voice");

            ui.separator();

            // Scrollable Family-Grouped Instrument List
            egui::ScrollArea::vertical()
                .id_salt("drum_kit_palette_rail_scroll")
                .max_height(430.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for &family in DrumFamily::all() {
                        let family_items: Vec<(usize, &DrumPaletteEntry)> = palette
                            .iter()
                            .enumerate()
                            .filter(|(_, e)| e.role.family() == family)
                            .collect();
                        if family_items.is_empty() {
                            continue;
                        }

                        ui.add_space(3.0);
                        ui.label(
                            egui::RichText::new(family.label())
                                .size(10.5)
                                .strong()
                                .color(Color32::from_rgb(135, 165, 200)),
                        );

                        for (idx, entry) in family_items {
                            let selected = idx == active_idx;
                            let is_muted = drum_role_is_muted(&dp, entry.role);
                            let is_custom = dp.is_role_customized(entry.role);
                            let in_kit = !entry.mapped_notes.is_empty();

                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;

                                // Inline quick audition play button
                                if ui
                                    .small_button("▶")
                                    .on_hover_text(format!(
                                        "Audition {} (GM {})",
                                        entry.role.display_name(),
                                        entry.primary_note()
                                    ))
                                    .clicked()
                                {
                                    commands.push(SynthCommand::TriggerDrum {
                                        gm_drum_note: entry.primary_note(),
                                        velocity: app.synth_lab.velocity,
                                    });
                                }

                                // Compact note numbers badge
                                let notes_to_show = if !entry.mapped_notes.is_empty()
                                    && !app.synth_lab.drum_show_all_gm
                                {
                                    entry.mapped_notes.as_slice()
                                } else {
                                    entry.role.gm_notes()
                                };
                                let notes_badge = if notes_to_show.len() <= 3 {
                                    notes_to_show
                                        .iter()
                                        .map(|n| n.to_string())
                                        .collect::<Vec<_>>()
                                        .join(",")
                                } else {
                                    format!(
                                        "{}..{}",
                                        notes_to_show[0],
                                        notes_to_show[notes_to_show.len() - 1]
                                    )
                                };

                                let prefix = if is_muted {
                                    "[M] "
                                } else if is_custom {
                                    "• "
                                } else {
                                    ""
                                };
                                let row_text =
                                    format!("{}{} ({})", prefix, entry.role.short_name(), notes_badge);
                                let mut rich = egui::RichText::new(row_text);
                                if is_muted {
                                    rich = rich.color(Color32::from_rgb(230, 110, 110));
                                } else if app.synth_lab.drum_show_all_gm && !in_kit && mapped_role_count > 0 {
                                    rich = rich.weak();
                                }

                                if ui.selectable_label(selected, rich).clicked() {
                                    active_idx = idx;
                                    select_role(app, entry, commands, false);
                                }
                            });
                        }
                    }
                });
        });

        ui.separator();

        // -----------------------------------------------------------------
        // RIGHT PANE: Active Voice Inspector, Audition Strip & Sliders
        // -----------------------------------------------------------------
        let active_entry = palette[active_idx].clone();
        let active_role = active_entry.role;
        // Ensure active_drum_note belongs to active_role
        if !active_role.gm_notes().contains(&app.synth_lab.active_drum_note) {
            app.synth_lab.active_drum_note = active_entry.primary_note();
        }
        let active_note = app.synth_lab.active_drum_note;

        ui.vertical(|ui| {
            // Voice Title Header + Per-Voice Reset
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(active_role.display_name())
                        .size(15.0)
                        .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("Reset Voice")
                        .on_hover_text("Reset only this instrument/group back to its factory acoustic default")
                        .clicked()
                    {
                        dp.reset_role(active_role);
                        app.synth_lab.preset.drum_params = dp;
                        app.set_part_synth_preset(target_idx, app.synth_lab.preset.clone());
                        let updated = dp.params_for_role(active_role);
                        commands.push(SynthCommand::UpdateDrumParams {
                            drum_role: active_role,
                            params: updated,
                        });
                        commands.push(SynthCommand::UpdatePartDrumParams {
                            part_idx: target_idx as u8,
                            drum_role: active_role,
                            params: updated,
                        });
                        app.synth_lab.status_msg =
                            Some(format!("Reset {} to default", active_role.display_name()));
                    }
                });
            });

            // Tab Mapping Binding Summary
            if !active_entry.tab_bindings.is_empty() {
                ui.label(
                    egui::RichText::new(format!("Tab Mapping: {}", active_entry.tab_bindings.join("  |  ")))
                        .size(11.0)
                        .color(Color32::from_rgb(130, 195, 165)),
                );
            } else if mapped_role_count > 0 {
                ui.label(
                    egui::RichText::new("Not mapped on current tab kit (All GM mode)")
                        .size(11.0)
                        .italics()
                        .weak(),
                );
            }

            ui.add_space(4.0);

            // Dedicated Trigger & Group Note Quick-Audition Strip
            ui.group(|ui| {
                ui.horizontal_wrapped(|ui| {
                    let trigger_label = format!(
                        "▶ Trigger {} ({})",
                        crate::presets::gm_percussion_name(active_note),
                        active_note
                    );
                    if ui.button(egui::RichText::new(trigger_label).strong()).clicked() {
                        commands.push(SynthCommand::TriggerDrum {
                            gm_drum_note: active_note,
                            velocity: app.synth_lab.velocity,
                        });
                    }

                    // Show individual note audition buttons for grouped roles (>1 note)
                    let group_notes: Vec<u8> =
                        if !app.synth_lab.drum_show_all_gm && !active_entry.mapped_notes.is_empty() {
                            active_entry.mapped_notes.clone()
                        } else {
                            active_role.gm_notes().to_vec()
                        };

                    if group_notes.len() > 1 {
                        ui.separator();
                        for note in group_notes {
                            let label =
                                format!("{}: {}", note, crate::presets::gm_percussion_name(note));
                            let is_cur = note == app.synth_lab.active_drum_note;
                            if ui.selectable_label(is_cur, label).clicked() {
                                app.synth_lab.active_drum_note = note;
                                commands.push(SynthCommand::TriggerDrum {
                                    gm_drum_note: note,
                                    velocity: app.synth_lab.velocity,
                                });
                            }
                        }
                    }
                });
            });

            ui.add_space(4.0);

            // Scrollable Parameter Controls for the Active Drum Role
            let mut changed_payload: Option<DrumVoiceParams> = None;
            let audition_vel = app.synth_lab.velocity;
            let show_all_gm = app.synth_lab.drum_show_all_gm;

            egui::ScrollArea::vertical()
                .id_salt("drum_voice_inspector_scroll")
                .max_height(370.0)
                .auto_shrink([false, false])
                .show(ui, |ui| match dp.params_for_role(active_role) {
                    DrumVoiceParams::Kick(mut k) => {
                        if render_kick_params_grid(ui, active_role, &mut k) {
                            changed_payload = Some(DrumVoiceParams::Kick(k));
                        }
                    }
                    DrumVoiceParams::Snare(mut s) => {
                        let include_side_stick = active_role == DrumRole::Snare;
                        if render_snare_params_grid(ui, active_role, include_side_stick, &mut s) {
                            changed_payload = Some(DrumVoiceParams::Snare(s));
                        }
                    }
                    DrumVoiceParams::HandClap(mut c) => {
                        if render_hand_clap_params_grid(ui, &mut c) {
                            changed_payload = Some(DrumVoiceParams::HandClap(c));
                        }
                    }
                    DrumVoiceParams::Toms(mut t) => {
                        let available_notes: Vec<u8> = if !show_all_gm
                            && !active_entry.mapped_notes.is_empty()
                        {
                            active_entry.mapped_notes.clone()
                        } else {
                            active_role.gm_notes().to_vec()
                        };
                        if render_membrane_group_params_grid(
                            ui,
                            active_role,
                            &available_notes,
                            &mut t,
                            audition_vel,
                            commands,
                        ) {
                            changed_payload = Some(DrumVoiceParams::Toms(t));
                        }
                    }
                    DrumVoiceParams::HiHats(mut h) => {
                        if render_hihat_params_grid(ui, &mut h) {
                            changed_payload = Some(DrumVoiceParams::HiHats(h));
                        }
                    }
                    DrumVoiceParams::SingleCymbal(mut c) => {
                        if render_single_cymbal_params_grid(ui, active_role, &mut c) {
                            changed_payload = Some(DrumVoiceParams::SingleCymbal(c));
                        }
                    }
                    DrumVoiceParams::RideCymbal(mut r) => {
                        if render_ride_and_bell_params_grid(ui, &mut r) {
                            changed_payload = Some(DrumVoiceParams::RideCymbal(r));
                        }
                    }
                    DrumVoiceParams::PercAux(mut a) => {
                        if render_perc_aux_params_grid(ui, active_role, &mut a) {
                            changed_payload = Some(DrumVoiceParams::PercAux(a));
                        }
                    }
                    DrumVoiceParams::ShakerEffect(mut s) => {
                        if render_shaker_effect_params_grid(ui, active_role, &mut s) {
                            changed_payload = Some(DrumVoiceParams::ShakerEffect(s));
                        }
                    }
                });

            if let Some(payload) = changed_payload {
                dp.set_params_for_role(active_role, payload);
                app.synth_lab.preset.drum_params = dp;
                app.set_part_synth_preset(target_idx, app.synth_lab.preset.clone());
                commands.push(SynthCommand::UpdateDrumParams {
                    drum_role: active_role,
                    params: payload,
                });
                commands.push(SynthCommand::UpdatePartDrumParams {
                    part_idx: target_idx as u8,
                    drum_role: active_role,
                    params: payload,
                });
            }
        });
    });
}

fn render_kick_params_grid(ui: &mut egui::Ui, role: DrumRole, k: &mut KickParams) -> bool {
    let mut changed = false;
    let def = if role == DrumRole::AcousticKick {
        KickParams::acoustic_kick_default()
    } else {
        KickParams::default()
    };

    ui.group(|ui| {
        egui::Grid::new(("kick_params_grid", role))
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                add_drum_volume_slider_row(
                    ui,
                    &format!("{} Volume:", role.short_name()),
                    "Absolute output level (-48 dB = Mute, double-click resets to 0 dB).",
                    &mut k.volume_db,
                    &mut changed,
                );

                ui.label("Pitch Envelope Start:");
                let r1 = ui.add(egui::Slider::new(&mut k.pitch_start_hz, 80.0..=250.0).suffix(" Hz"));
                if r1.changed() { changed = true; }
                if r1.double_clicked() { k.pitch_start_hz = def.pitch_start_hz; changed = true; }
                ui.end_row();

                ui.label("Resting Sub-Bass Pitch:");
                let r2 = ui.add(egui::Slider::new(&mut k.pitch_end_hz, 30.0..=65.0).suffix(" Hz"));
                if r2.changed() { changed = true; }
                if r2.double_clicked() { k.pitch_end_hz = def.pitch_end_hz; changed = true; }
                ui.end_row();

                ui.label("Pitch Drop Time:");
                let r3 = ui.add(egui::Slider::new(&mut k.pitch_decay_ms, 10.0..=80.0).suffix(" ms"));
                if r3.changed() { changed = true; }
                if r3.double_clicked() { k.pitch_decay_ms = def.pitch_decay_ms; changed = true; }
                ui.end_row();

                ui.label("Shell Body Decay:");
                let r4 = ui.add(egui::Slider::new(&mut k.body_decay_ms, 50.0..=600.0).suffix(" ms"));
                if r4.changed() { changed = true; }
                if r4.double_clicked() { k.body_decay_ms = def.body_decay_ms; changed = true; }
                ui.end_row();

                ui.label("Click Beater Level:");
                let r5 = ui.add(egui::Slider::new(&mut k.click_level, 0.0..=1.0));
                if r5.changed() { changed = true; }
                if r5.double_clicked() { k.click_level = def.click_level; changed = true; }
                ui.end_row();

                ui.label("Head Tension Mod (Pitch Bend):")
                    .on_hover_text("Non-linear circular membrane tension pitch stretch on hard hits (30–80 ms)");
                let r6 = ui.add(egui::Slider::new(&mut k.head_tension_mod, 0.0..=1.0));
                if r6.changed() { changed = true; }
                if r6.double_clicked() { k.head_tension_mod = def.head_tension_mod; changed = true; }
                ui.end_row();

                ui.label("Strike Position (Center vs Edge):")
                    .on_hover_text("0.0 = dead-center punch (1.00, 2.29 modes), 1.0 = rim/edge ring (1.59, 2.13, 2.65 modes)");
                let r7 = ui.add(egui::Slider::new(&mut k.strike_position, 0.0..=1.0));
                if r7.changed() { changed = true; }
                if r7.double_clicked() { k.strike_position = def.strike_position; changed = true; }
                ui.end_row();

                ui.label("Overtones Ring (Bessel Decay):")
                    .on_hover_text("Controls how long upper Bessel membrane modes ring relative to the (0,1) fundamental");
                let r8 = ui.add(egui::Slider::new(&mut k.overtones_ring, 0.0..=1.0));
                if r8.changed() { changed = true; }
                if r8.double_clicked() { k.overtones_ring = def.overtones_ring; changed = true; }
                ui.end_row();

                ui.label("Shell Wood Cavity:")
                    .on_hover_text("Cylindrical wooden drum shell air-cavity mode & < 1.5 ms 2-tap micro-reflection body");
                let r9 = ui.add(egui::Slider::new(&mut k.shell_wood_cavity, 0.0..=1.0));
                if r9.changed() { changed = true; }
                if r9.double_clicked() { k.shell_wood_cavity = def.shell_wood_cavity; changed = true; }
                ui.end_row();

                add_hit_variation_slider_row(
                    ui,
                    &mut k.hit_variation,
                    "Logarithmic humanization of hit velocity & strike position (0.0 = exact/identical, 1.0 = heavy variation)",
                    &mut changed,
                );
            });
    });
    changed
}

fn render_snare_params_grid(
    ui: &mut egui::Ui,
    role: DrumRole,
    include_side_stick: bool,
    s: &mut SnareParams,
) -> bool {
    let mut changed = false;
    let def = if include_side_stick {
        SnareParams::default()
    } else {
        SnareParams::electric_snare_default()
    };

    ui.group(|ui| {
        egui::Grid::new(("snare_params_grid", role))
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                add_drum_volume_slider_row(
                    ui,
                    if include_side_stick {
                        "Snare Volume (38):"
                    } else {
                        "Electric Snare Volume (40):"
                    },
                    "Absolute output level (-48 dB = Mute, double-click resets to 0 dB).",
                    &mut s.volume_db,
                    &mut changed,
                );

                if include_side_stick {
                    add_drum_volume_slider_row(
                        ui,
                        "Side Stick Volume (37):",
                        "Absolute output level for Side Stick / Cross-Stick (GM 37), sharing this snare's shell size & wood cavity.",
                        &mut s.rim_volume_db,
                        &mut changed,
                    );
                }

                ui.label("Mode 1 Tone Freq:");
                let r1 = ui.add(egui::Slider::new(&mut s.tone_freq_hz, 120.0..=350.0).suffix(" Hz"));
                if r1.changed() { changed = true; }
                if r1.double_clicked() { s.tone_freq_hz = def.tone_freq_hz; changed = true; }
                ui.end_row();

                ui.label("Mode 2 Resonance:");
                let r2 = ui.add(egui::Slider::new(&mut s.mode2_freq_hz, 200.0..=600.0).suffix(" Hz"));
                if r2.changed() { changed = true; }
                if r2.double_clicked() { s.mode2_freq_hz = def.mode2_freq_hz; changed = true; }
                ui.end_row();

                ui.label("Shell Tone Decay:");
                let r3 = ui.add(egui::Slider::new(&mut s.tone_decay_ms, 20.0..=200.0).suffix(" ms"));
                if r3.changed() { changed = true; }
                if r3.double_clicked() { s.tone_decay_ms = def.tone_decay_ms; changed = true; }
                ui.end_row();

                ui.label("Wire Noise Cutoff:");
                let r4 = ui.add(egui::Slider::new(&mut s.wire_cutoff_hz, 1000.0..=5000.0).suffix(" Hz"));
                if r4.changed() { changed = true; }
                if r4.double_clicked() { s.wire_cutoff_hz = def.wire_cutoff_hz; changed = true; }
                ui.end_row();

                ui.label("Wire Noise Decay:");
                let r5 = ui.add(egui::Slider::new(&mut s.noise_decay_ms, 50.0..=400.0).suffix(" ms"));
                if r5.changed() { changed = true; }
                if r5.double_clicked() { s.noise_decay_ms = def.noise_decay_ms; changed = true; }
                ui.end_row();

                ui.label("Noise / Tone Mix:");
                let r6 = ui.add(egui::Slider::new(&mut s.noise_mix, 0.0..=1.0));
                if r6.changed() { changed = true; }
                if r6.double_clicked() { s.noise_mix = def.noise_mix; changed = true; }
                ui.end_row();

                ui.label("Head Tension Mod (Pitch Bend):")
                    .on_hover_text("Non-linear circular membrane tension pitch stretch on hard snare hits");
                let r8 = ui.add(egui::Slider::new(&mut s.head_tension_mod, 0.0..=1.0));
                if r8.changed() { changed = true; }
                if r8.double_clicked() { s.head_tension_mod = def.head_tension_mod; changed = true; }
                ui.end_row();

                ui.label("Strike Position (Center vs Edge):")
                    .on_hover_text("0.0 = dead-center punch (1.00, 2.29 modes), 1.0 = rim/edge overtone ring (1.59, 2.13, 2.65)");
                let r9 = ui.add(egui::Slider::new(&mut s.strike_position, 0.0..=1.0));
                if r9.changed() { changed = true; }
                if r9.double_clicked() { s.strike_position = def.strike_position; changed = true; }
                ui.end_row();

                ui.label("Overtones Ring (Bessel Decay):")
                    .on_hover_text("Higher Bessel drumhead overtone ring duration relative to the fundamental");
                let r10 = ui.add(egui::Slider::new(&mut s.overtones_ring, 0.0..=1.0));
                if r10.changed() { changed = true; }
                if r10.double_clicked() { s.overtones_ring = def.overtones_ring; changed = true; }
                ui.end_row();

                ui.label("Shell Wood Cavity:")
                    .on_hover_text("Wooden snare shell air-cavity mode & < 1.5 ms 2-tap micro-reflection body (also shapes Side Stick)");
                let r11 = ui.add(egui::Slider::new(&mut s.shell_wood_cavity, 0.0..=1.0));
                if r11.changed() { changed = true; }
                if r11.double_clicked() { s.shell_wood_cavity = def.shell_wood_cavity; changed = true; }
                ui.end_row();

                ui.label("Snare Wire Tension / Buzz:")
                    .on_hover_text("Non-linear membrane-coupled snare strainer threshold (0.0 = loose buzz, 1.0 = tight snap)");
                let r12 = ui.add(egui::Slider::new(&mut s.wire_tension, 0.0..=1.0));
                if r12.changed() { changed = true; }
                if r12.double_clicked() { s.wire_tension = def.wire_tension; changed = true; }
                ui.end_row();

                add_hit_variation_slider_row(
                    ui,
                    &mut s.hit_variation,
                    "Logarithmic humanization of hit velocity & strike position (0.0 = exact/identical, 1.0 = heavy variation)",
                    &mut changed,
                );
            });
    });
    changed
}

fn render_hand_clap_params_grid(ui: &mut egui::Ui, c: &mut HandClapParams) -> bool {
    let mut changed = false;
    let def = HandClapParams::default();

    ui.group(|ui| {
        egui::Grid::new("hand_clap_params_grid")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                add_drum_volume_slider_row(
                    ui,
                    "Hand Clap Volume (39):",
                    "Absolute output level for Hand Clap (GM 39). -48 dB = Mute, double-click resets to 0 dB.",
                    &mut c.volume_db,
                    &mut changed,
                );

                ui.label("Multi-Clap Burst Spacing:")
                    .on_hover_text("Timing stagger between layered hand-clap transients (4–35 ms)");
                let r1 = ui.add(egui::Slider::new(&mut c.clap_spacing_ms, 4.0..=35.0).suffix(" ms"));
                if r1.changed() { changed = true; }
                if r1.double_clicked() { c.clap_spacing_ms = def.clap_spacing_ms; changed = true; }
                ui.end_row();

                ui.label("Formant Bandpass Freq:")
                    .on_hover_text("Center frequency of cupped-hand acoustic resonance (600–3600 Hz)");
                let r2 = ui.add(egui::Slider::new(&mut c.bandpass_hz, 600.0..=3600.0).suffix(" Hz"));
                if r2.changed() { changed = true; }
                if r2.double_clicked() { c.bandpass_hz = def.bandpass_hz; changed = true; }
                ui.end_row();

                ui.label("Formant Resonance Q:");
                let r3 = ui.add(egui::Slider::new(&mut c.bandpass_q, 0.6..=4.5));
                if r3.changed() { changed = true; }
                if r3.double_clicked() { c.bandpass_q = def.bandpass_q; changed = true; }
                ui.end_row();

                ui.label("Clap Tail Decay:");
                let r4 = ui.add(egui::Slider::new(&mut c.tail_decay_ms, 40.0..=500.0).suffix(" ms"));
                if r4.changed() { changed = true; }
                if r4.double_clicked() { c.tail_decay_ms = def.tail_decay_ms; changed = true; }
                ui.end_row();

                ui.label("Room Reflection Mix:");
                let r5 = ui.add(egui::Slider::new(&mut c.room_mix, 0.0..=1.0));
                if r5.changed() { changed = true; }
                if r5.double_clicked() { c.room_mix = def.room_mix; changed = true; }
                ui.end_row();

                add_hit_variation_slider_row(
                    ui,
                    &mut c.hit_variation,
                    "Logarithmic humanization of clap velocity (0.0 = identical, 1.0 = heavy variation)",
                    &mut changed,
                );
            });
    });
    changed
}

fn render_membrane_group_params_grid(
    ui: &mut egui::Ui,
    role: DrumRole,
    available_notes: &[u8],
    t: &mut TomParams,
    audition_vel: f32,
    commands: &mut Vec<SynthCommand>,
) -> bool {
    let mut changed = false;
    let def = match role {
        DrumRole::Bongos => TomParams::bongos_default(),
        DrumRole::Congas => TomParams::congas_default(),
        DrumRole::Timbales => TomParams::timbales_default(),
        _ => TomParams::default(),
    };
    let attack_label = match role {
        DrumRole::Bongos | DrumRole::Congas => "Hand Slap Attack:",
        _ => "Stick Attack Level:",
    };
    let shell_label = match role {
        DrumRole::Timbales => "Shallow Shell Cavity:",
        _ => "Shell Wood Cavity:",
    };

    ui.group(|ui| {
        egui::Grid::new(("membrane_group_params_grid", role))
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                add_drum_volume_slider_row(
                    ui,
                    &format!("{} Volume:", role.short_name()),
                    "Absolute output level for this drum group (-48 dB = Mute, double-click resets to 0 dB).",
                    &mut t.volume_db,
                    &mut changed,
                );

                ui.label("Master Pitch Scale:");
                let r1 = ui.add(egui::Slider::new(&mut t.pitch_scale, 0.5..=2.0));
                if r1.changed() { changed = true; }
                if r1.double_clicked() { t.pitch_scale = def.pitch_scale; changed = true; }
                ui.end_row();

                ui.label("Pitch Drop Sweep Ratio:");
                let r2 = ui.add(egui::Slider::new(&mut t.pitch_drop_ratio, 1.05..=3.0));
                if r2.changed() { changed = true; }
                if r2.double_clicked() { t.pitch_drop_ratio = def.pitch_drop_ratio; changed = true; }
                ui.end_row();

                ui.label("Drumhead Decay Time:");
                let r3 = ui.add(egui::Slider::new(&mut t.decay_ms, 50.0..=800.0).suffix(" ms"));
                if r3.changed() { changed = true; }
                if r3.double_clicked() { t.decay_ms = def.decay_ms; changed = true; }
                ui.end_row();

                ui.label(attack_label);
                let r4 = ui.add(egui::Slider::new(&mut t.stick_attack, 0.0..=1.0));
                if r4.changed() { changed = true; }
                if r4.double_clicked() { t.stick_attack = def.stick_attack; changed = true; }
                ui.end_row();

                ui.label("Head Tension Mod (Pitch Bend):")
                    .on_hover_text("Non-linear circular membrane tension pitch stretch on hard hits (30–80 ms)");
                let r5 = ui.add(egui::Slider::new(&mut t.head_tension_mod, 0.0..=1.0));
                if r5.changed() { changed = true; }
                if r5.double_clicked() { t.head_tension_mod = def.head_tension_mod; changed = true; }
                ui.end_row();

                ui.label("Strike Position (Center vs Edge):")
                    .on_hover_text("0.0 = dead-center punch (1.00, 2.29 modes), 1.0 = rim/edge overtone ring (1.59, 2.13, 2.65)");
                let r6 = ui.add(egui::Slider::new(&mut t.strike_position, 0.0..=1.0));
                if r6.changed() { changed = true; }
                if r6.double_clicked() { t.strike_position = def.strike_position; changed = true; }
                ui.end_row();

                ui.label("Overtones Ring (Bessel Decay):")
                    .on_hover_text("Higher Bessel drumhead overtone ring duration relative to the fundamental");
                let r7 = ui.add(egui::Slider::new(&mut t.overtones_ring, 0.0..=1.0));
                if r7.changed() { changed = true; }
                if r7.double_clicked() { t.overtones_ring = def.overtones_ring; changed = true; }
                ui.end_row();

                ui.label(shell_label)
                    .on_hover_text("Cylindrical drum shell air-cavity mode & < 1.5 ms 2-tap micro-reflection body");
                let r8 = ui.add(egui::Slider::new(&mut t.shell_wood_cavity, 0.0..=1.0));
                if r8.changed() { changed = true; }
                if r8.double_clicked() { t.shell_wood_cavity = def.shell_wood_cavity; changed = true; }
                ui.end_row();

                add_hit_variation_slider_row(
                    ui,
                    &mut t.hit_variation,
                    "Logarithmic humanization of hit velocity & strike position (0.0 = exact/identical, 1.0 = heavy variation)",
                    &mut changed,
                );
            });
    });

    // Individual Per-Drum Tuning section (only showing drums available in the active palette!)
    let tunable_notes: Vec<(u8, usize, f32)> = available_notes
        .iter()
        .filter_map(|&note| {
            let slot = TomParams::tuning_slot_for_note(note)?;
            let base_hz = match note {
                41 => 78.0,
                43 => 95.0,
                45 => 118.0,
                47 => 145.0,
                48 => 172.0,
                50 => 205.0,
                60 => 315.0,
                61 => 220.0,
                62 => 245.0,
                63 => 235.0,
                64 => 165.0,
                65 => 290.0,
                66 => 212.0,
                _ => 120.0,
            };
            Some((note, slot, base_hz))
        })
        .collect();

    if !tunable_notes.is_empty() {
        ui.add_space(4.0);
        ui.group(|ui| {
            ui.label(
                egui::RichText::new("Individual Drum Tuning (Semitones — Double-Click to Reset):")
                    .strong(),
            );
            egui::Grid::new(("membrane_per_drum_tuning_grid", role))
                .num_columns(3)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    for (note, slot, std_hz) in tunable_notes {
                        let eff_hz = std_hz
                            * t.pitch_scale
                            * 2.0_f32.powf(t.tom_tuning_semitones[slot] / 12.0);
                        ui.label(format!(
                            "{}: {} ({:.0} Hz)",
                            note,
                            crate::presets::gm_percussion_name(note),
                            eff_hz
                        ));
                        let r = ui.add(
                            egui::Slider::new(&mut t.tom_tuning_semitones[slot], -12.0..=12.0)
                                .suffix(" st")
                                .fixed_decimals(1),
                        );
                        if r.changed() {
                            changed = true;
                        }
                        if r.double_clicked() {
                            t.tom_tuning_semitones[slot] = 0.0;
                            changed = true;
                        }
                        if ui
                            .small_button("▶")
                            .on_hover_text(format!("Audition GM {}", note))
                            .clicked()
                        {
                            commands.push(SynthCommand::TriggerDrum {
                                gm_drum_note: note,
                                velocity: audition_vel,
                            });
                        }
                        ui.end_row();
                    }
                });
        });
    }

    changed
}

fn render_hihat_params_grid(ui: &mut egui::Ui, h: &mut HiHatParams) -> bool {
    let mut changed = false;
    let def = HiHatParams::default();

    ui.group(|ui| {
        egui::Grid::new("hihat_params_grid")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                add_drum_volume_slider_row(
                    ui,
                    "Closed Hat Volume (42):",
                    "Absolute output level for Closed Hi-Hat (GM 42). -48 dB = Mute, double-click resets to 0 dB.",
                    &mut h.closed_volume_db,
                    &mut changed,
                );
                add_drum_volume_slider_row(
                    ui,
                    "Pedal Hat Volume (44):",
                    "Absolute output level for Pedal Hi-Hat (GM 44). -48 dB = Mute, double-click resets to 0 dB.",
                    &mut h.pedal_volume_db,
                    &mut changed,
                );
                add_drum_volume_slider_row(
                    ui,
                    "Open Hat Volume (46):",
                    "Absolute output level for Open Hi-Hat (GM 46). -48 dB = Mute, double-click resets to 0 dB.",
                    &mut h.open_volume_db,
                    &mut changed,
                );

                ui.label("Metallic Matrix Spread:");
                let r1 = ui.add(egui::Slider::new(&mut h.spread, 0.5..=2.0));
                if r1.changed() { changed = true; }
                if r1.double_clicked() { h.spread = def.spread; changed = true; }
                ui.end_row();

                ui.label("Highpass Cutoff:");
                let r2 = ui.add(egui::Slider::new(&mut h.highpass_cutoff_hz, 3000.0..=12000.0).suffix(" Hz"));
                if r2.changed() { changed = true; }
                if r2.double_clicked() { h.highpass_cutoff_hz = def.highpass_cutoff_hz; changed = true; }
                ui.end_row();

                ui.label("Closed Hat Decay:");
                let r3 = ui.add(egui::Slider::new(&mut h.closed_decay_ms, 15.0..=150.0).suffix(" ms"));
                if r3.changed() { changed = true; }
                if r3.double_clicked() { h.closed_decay_ms = def.closed_decay_ms; changed = true; }
                ui.end_row();

                ui.label("Pedal Hat Decay:");
                let r4 = ui.add(egui::Slider::new(&mut h.pedal_decay_ms, 20.0..=200.0).suffix(" ms"));
                if r4.changed() { changed = true; }
                if r4.double_clicked() { h.pedal_decay_ms = def.pedal_decay_ms; changed = true; }
                ui.end_row();

                ui.label("Open Hat Decay:");
                let r5 = ui.add(egui::Slider::new(&mut h.open_decay_ms, 100.0..=2000.0).suffix(" ms"));
                if r5.changed() { changed = true; }
                if r5.double_clicked() { h.open_decay_ms = def.open_decay_ms; changed = true; }
                ui.end_row();

                ui.label("Bronze Size / Pitch:")
                    .on_hover_text("Scales B20 bronze waveguide delay lengths and modal tuning (0.5 = large/dark, 2.0 = small/bright)");
                let r6 = ui.add(egui::Slider::new(&mut h.bronze_size_pitch, 0.5..=2.0));
                if r6.changed() { changed = true; }
                if r6.double_clicked() { h.bronze_size_pitch = def.bronze_size_pitch; changed = true; }
                ui.end_row();

                ui.label("Plate Dispersion (Stick Ping):")
                    .on_hover_text("1st-order all-pass plate dispersion depth inside the 4-line Hadamard waveguide mesh");
                let r7 = ui.add(egui::Slider::new(&mut h.plate_dispersion, 0.0..=1.0));
                if r7.changed() { changed = true; }
                if r7.double_clicked() { h.plate_dispersion = def.plate_dispersion; changed = true; }
                ui.end_row();

                ui.label("Non-Linear Shimmer Bloom:")
                    .on_hover_text("Signal-modulated all-pass dispersion energy cascade into 4–15 kHz bronze shimmer");
                let r8 = ui.add(egui::Slider::new(&mut h.nonlin_shimmer, 0.0..=1.0));
                if r8.changed() { changed = true; }
                if r8.double_clicked() { h.nonlin_shimmer = def.nonlin_shimmer; changed = true; }
                ui.end_row();

                ui.label("Bell Level:")
                    .on_hover_text("High-Q cast-bronze bell / cup modal resonance emphasis");
                let r9 = ui.add(egui::Slider::new(&mut h.bell_level, 0.0..=1.0));
                if r9.changed() { changed = true; }
                if r9.double_clicked() { h.bell_level = def.bell_level; changed = true; }
                ui.end_row();

                ui.label("Stick Hardness:")
                    .on_hover_text("Stick tip lowpass brightness (soft wood tip vs. bright nylon tip)");
                let r10 = ui.add(egui::Slider::new(&mut h.stick_hardness, 0.0..=1.0));
                if r10.changed() { changed = true; }
                if r10.double_clicked() { h.stick_hardness = def.stick_hardness; changed = true; }
                ui.end_row();

                add_hit_variation_slider_row(
                    ui,
                    &mut h.hit_variation,
                    "Logarithmic humanization of hit velocity & stick strike character (0.0 = exact/identical, 1.0 = heavy variation)",
                    &mut changed,
                );
            });
    });
    changed
}

fn render_single_cymbal_params_grid(
    ui: &mut egui::Ui,
    role: DrumRole,
    c: &mut SingleCymbalParams,
) -> bool {
    let mut changed = false;
    let def = match role {
        DrumRole::Crash2 => SingleCymbalParams::crash2_default(),
        DrumRole::Ride2 => SingleCymbalParams::ride2_default(),
        DrumRole::Splash => SingleCymbalParams::splash_default(),
        DrumRole::China => SingleCymbalParams::china_default(),
        _ => SingleCymbalParams::crash1_default(),
    };

    ui.group(|ui| {
        egui::Grid::new(("single_cymbal_params_grid", role))
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                add_drum_volume_slider_row(
                    ui,
                    &format!("{} Volume:", role.short_name()),
                    "Absolute output level for this cymbal (-48 dB = Mute, double-click resets to 0 dB).",
                    &mut c.volume_db,
                    &mut changed,
                );

                ui.label("Ring / Wash Decay:");
                let r1 = ui.add(egui::Slider::new(&mut c.decay_ms, 180.0..=3500.0).suffix(" ms"));
                if r1.changed() { changed = true; }
                if r1.double_clicked() { c.decay_ms = def.decay_ms; changed = true; }
                ui.end_row();

                ui.label("Bronze Size / Pitch:")
                    .on_hover_text("Scales B20 bronze waveguide delay lengths and modal tuning (0.5 = large/dark, 2.0 = small/bright)");
                let r2 = ui.add(egui::Slider::new(&mut c.bronze_size_pitch, 0.5..=2.0));
                if r2.changed() { changed = true; }
                if r2.double_clicked() { c.bronze_size_pitch = def.bronze_size_pitch; changed = true; }
                ui.end_row();

                ui.label("Highpass / Brightness Cutoff:");
                let r3 = ui.add(egui::Slider::new(&mut c.cutoff_hz, 1800.0..=8000.0).suffix(" Hz"));
                if r3.changed() { changed = true; }
                if r3.double_clicked() { c.cutoff_hz = def.cutoff_hz; changed = true; }
                ui.end_row();

                ui.label("Shimmer Matrix Spread:");
                let r4 = ui.add(egui::Slider::new(&mut c.matrix_spread, 0.5..=2.0));
                if r4.changed() { changed = true; }
                if r4.double_clicked() { c.matrix_spread = def.matrix_spread; changed = true; }
                ui.end_row();

                ui.label("Plate Dispersion (Flex):")
                    .on_hover_text("1st-order all-pass plate dispersion depth inside the 4-line Hadamard waveguide mesh");
                let r5 = ui.add(egui::Slider::new(&mut c.plate_dispersion, 0.0..=1.0));
                if r5.changed() { changed = true; }
                if r5.double_clicked() { c.plate_dispersion = def.plate_dispersion; changed = true; }
                ui.end_row();

                ui.label("Non-Linear Shimmer Bloom:")
                    .on_hover_text("Signal-modulated all-pass dispersion energy cascade into 4–15 kHz cast-bronze shimmer");
                let r6 = ui.add(egui::Slider::new(&mut c.nonlin_shimmer, 0.0..=1.0));
                if r6.changed() { changed = true; }
                if r6.double_clicked() { c.nonlin_shimmer = def.nonlin_shimmer; changed = true; }
                ui.end_row();

                ui.label("Bell / Cup Presence:");
                let r7 = ui.add(egui::Slider::new(&mut c.bell_level, 0.0..=1.0));
                if r7.changed() { changed = true; }
                if r7.double_clicked() { c.bell_level = def.bell_level; changed = true; }
                ui.end_row();

                ui.label("Stick Hardness:")
                    .on_hover_text("Stick tip lowpass brightness (soft felt/wood tip vs. crisp nylon tip)");
                let r8 = ui.add(egui::Slider::new(&mut c.stick_hardness, 0.0..=1.0));
                if r8.changed() { changed = true; }
                if r8.double_clicked() { c.stick_hardness = def.stick_hardness; changed = true; }
                ui.end_row();

                add_hit_variation_slider_row(
                    ui,
                    &mut c.hit_variation,
                    "Logarithmic humanization of hit velocity & stick strike character (0.0 = exact/identical, 1.0 = heavy variation)",
                    &mut changed,
                );
            });
    });
    changed
}

fn render_ride_and_bell_params_grid(ui: &mut egui::Ui, r: &mut RideCymbalParams) -> bool {
    let mut changed = false;
    let def = RideCymbalParams::default();

    ui.group(|ui| {
        egui::Grid::new("ride_and_bell_params_grid")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                add_drum_volume_slider_row(
                    ui,
                    "Ride Bow Volume (51):",
                    "Absolute output level for Ride Cymbal 1 bow (GM 51). -48 dB = Mute, double-click resets to 0 dB.",
                    &mut r.bow_volume_db,
                    &mut changed,
                );
                add_drum_volume_slider_row(
                    ui,
                    "Ride Bell Volume (53):",
                    "Absolute output level for Ride Bell cup (GM 53). -48 dB = Mute, double-click resets to 0 dB.",
                    &mut r.bell_volume_db,
                    &mut changed,
                );

                ui.label("Ride Bow Decay:");
                let r1 = ui.add(egui::Slider::new(&mut r.bow_decay_ms, 300.0..=3500.0).suffix(" ms"));
                if r1.changed() { changed = true; }
                if r1.double_clicked() { r.bow_decay_ms = def.bow_decay_ms; changed = true; }
                ui.end_row();

                ui.label("Ride Bell Decay:");
                let r2 = ui.add(egui::Slider::new(&mut r.bell_decay_ms, 200.0..=3000.0).suffix(" ms"));
                if r2.changed() { changed = true; }
                if r2.double_clicked() { r.bell_decay_ms = def.bell_decay_ms; changed = true; }
                ui.end_row();

                ui.label("Bell Cup Presence:")
                    .on_hover_text("High-Q singing cast-bronze bell cup emphasis (accentuated on GM 53)");
                let r3 = ui.add(egui::Slider::new(&mut r.bell_peak_level, 0.0..=1.0));
                if r3.changed() { changed = true; }
                if r3.double_clicked() { r.bell_peak_level = def.bell_peak_level; changed = true; }
                ui.end_row();

                ui.label("Bronze Size / Pitch:")
                    .on_hover_text("Shared ride plate diameter & modal tuning (affects both bow 51 and bell 53)");
                let r4 = ui.add(egui::Slider::new(&mut r.bronze_size_pitch, 0.5..=2.0));
                if r4.changed() { changed = true; }
                if r4.double_clicked() { r.bronze_size_pitch = def.bronze_size_pitch; changed = true; }
                ui.end_row();

                ui.label("Highpass / Brightness Cutoff:");
                let r5 = ui.add(egui::Slider::new(&mut r.cutoff_hz, 1800.0..=8000.0).suffix(" Hz"));
                if r5.changed() { changed = true; }
                if r5.double_clicked() { r.cutoff_hz = def.cutoff_hz; changed = true; }
                ui.end_row();

                ui.label("Shimmer Matrix Spread:");
                let r6 = ui.add(egui::Slider::new(&mut r.matrix_spread, 0.5..=2.0));
                if r6.changed() { changed = true; }
                if r6.double_clicked() { r.matrix_spread = def.matrix_spread; changed = true; }
                ui.end_row();

                ui.label("Plate Dispersion (Stick Ping):");
                let r7 = ui.add(egui::Slider::new(&mut r.plate_dispersion, 0.0..=1.0));
                if r7.changed() { changed = true; }
                if r7.double_clicked() { r.plate_dispersion = def.plate_dispersion; changed = true; }
                ui.end_row();

                ui.label("Non-Linear Shimmer Wash:");
                let r8 = ui.add(egui::Slider::new(&mut r.nonlin_shimmer, 0.0..=1.0));
                if r8.changed() { changed = true; }
                if r8.double_clicked() { r.nonlin_shimmer = def.nonlin_shimmer; changed = true; }
                ui.end_row();

                ui.label("Stick Hardness:");
                let r9 = ui.add(egui::Slider::new(&mut r.stick_hardness, 0.0..=1.0));
                if r9.changed() { changed = true; }
                if r9.double_clicked() { r.stick_hardness = def.stick_hardness; changed = true; }
                ui.end_row();

                add_hit_variation_slider_row(
                    ui,
                    &mut r.hit_variation,
                    "Logarithmic humanization of hit velocity & stick strike character",
                    &mut changed,
                );
            });
    });
    changed
}

fn render_perc_aux_params_grid(ui: &mut egui::Ui, role: DrumRole, a: &mut PercAuxParams) -> bool {
    let mut changed = false;
    let (def, f1_range, f2_range, decay_range) = match role {
        DrumRole::Agogo => (
            PercAuxParams::agogo_default(),
            400.0..=2200.0,
            600.0..=3200.0,
            40.0..=800.0,
        ),
        DrumRole::Triangle => (
            PercAuxParams::triangle_default(),
            1200.0..=4500.0,
            1800.0..=6800.0,
            100.0..=2000.0,
        ),
        DrumRole::Claves => (
            PercAuxParams::claves_default(),
            1200.0..=3800.0,
            1800.0..=5200.0,
            15.0..=200.0,
        ),
        DrumRole::WoodBlocks => (
            PercAuxParams::wood_blocks_default(),
            350.0..=1800.0,
            550.0..=2800.0,
            15.0..=240.0,
        ),
        _ => (
            PercAuxParams::cowbell_default(),
            300.0..=1200.0,
            400.0..=1800.0,
            20.0..=500.0,
        ),
    };

    ui.group(|ui| {
        egui::Grid::new(("perc_aux_params_grid", role))
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                add_drum_volume_slider_row(
                    ui,
                    &format!("{} Volume:", role.short_name()),
                    "Absolute output level (-48 dB = Mute, double-click resets to 0 dB).",
                    &mut a.volume_db,
                    &mut changed,
                );

                ui.label("Fundamental Mode 1:");
                let r1 = ui.add(egui::Slider::new(&mut a.freq1_hz, f1_range).suffix(" Hz"));
                if r1.changed() { changed = true; }
                if r1.double_clicked() { a.freq1_hz = def.freq1_hz; changed = true; }
                ui.end_row();

                ui.label("Harmonic Mode 2:");
                let r2 = ui.add(egui::Slider::new(&mut a.freq2_hz, f2_range).suffix(" Hz"));
                if r2.changed() { changed = true; }
                if r2.double_clicked() { a.freq2_hz = def.freq2_hz; changed = true; }
                ui.end_row();

                ui.label("Detune / Spread:");
                let r3 = ui.add(egui::Slider::new(&mut a.detune, -80.0..=80.0).suffix(" Hz"));
                if r3.changed() { changed = true; }
                if r3.double_clicked() { a.detune = def.detune; changed = true; }
                ui.end_row();

                ui.label("Resonator Bandpass Q:");
                let r4 = ui.add(egui::Slider::new(&mut a.bandpass_q, 0.5..=12.0));
                if r4.changed() { changed = true; }
                if r4.double_clicked() { a.bandpass_q = def.bandpass_q; changed = true; }
                ui.end_row();

                ui.label("Ring Decay Time:");
                let r5 = ui.add(egui::Slider::new(&mut a.decay_ms, decay_range).suffix(" ms"));
                if r5.changed() { changed = true; }
                if r5.double_clicked() { a.decay_ms = def.decay_ms; changed = true; }
                ui.end_row();

                add_hit_variation_slider_row(
                    ui,
                    &mut a.hit_variation,
                    "Logarithmic humanization of strike velocity",
                    &mut changed,
                );
            });
    });
    changed
}

fn render_shaker_effect_params_grid(
    ui: &mut egui::Ui,
    role: DrumRole,
    s: &mut ShakerAndEffectParams,
) -> bool {
    let mut changed = false;
    let (def, bright_label, bright_range, texture_label, decay_range) = match role {
        DrumRole::Tambourine => (
            ShakerAndEffectParams::tambourine_default(),
            "Jingle Brightness:",
            3000.0..=12000.0,
            "Jingle Flutter Rate:",
            35.0..=500.0,
        ),
        DrumRole::Cabasa => (
            ShakerAndEffectParams::cabasa_default(),
            "Steel Bead Brightness:",
            2500.0..=11000.0,
            "Bead Grain Rate:",
            30.0..=400.0,
        ),
        DrumRole::Maracas => (
            ShakerAndEffectParams::maracas_default(),
            "Seed Tone Cutoff:",
            2000.0..=10500.0,
            "Seed Swish Density:",
            25.0..=300.0,
        ),
        DrumRole::Guiro => (
            ShakerAndEffectParams::guiro_default(),
            "Gourd Resonance Freq:",
            800.0..=5500.0,
            "Notch Scrape Rate:",
            60.0..=750.0,
        ),
        DrumRole::Whistle => (
            ShakerAndEffectParams::whistle_default(),
            "Whistle Base Pitch:",
            1500.0..=5200.0,
            "Pea Trill Rate / Depth:",
            80.0..=1000.0,
        ),
        DrumRole::Vibraslap => (
            ShakerAndEffectParams::vibraslap_default(),
            "Wood Box Resonance:",
            600.0..=4200.0,
            "Wood Ball Rattle Rate:",
            120.0..=1200.0,
        ),
        _ => (
            ShakerAndEffectParams::cuica_default(),
            "Friction Pitch Center:",
            220.0..=1200.0,
            "Pitch Sweep Inflection:",
            60.0..=600.0,
        ),
    };

    ui.group(|ui| {
        egui::Grid::new(("shaker_effect_params_grid", role))
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                add_drum_volume_slider_row(
                    ui,
                    &format!("{} Volume:", role.short_name()),
                    "Absolute output level (-48 dB = Mute, double-click resets to 0 dB).",
                    &mut s.volume_db,
                    &mut changed,
                );

                ui.label("Pitch / Size Scale:");
                let r1 = ui.add(egui::Slider::new(&mut s.pitch_scale, 0.5..=2.0));
                if r1.changed() { changed = true; }
                if r1.double_clicked() { s.pitch_scale = def.pitch_scale; changed = true; }
                ui.end_row();

                ui.label(bright_label);
                let r2 = ui.add(egui::Slider::new(&mut s.brightness_hz, bright_range).suffix(" Hz"));
                if r2.changed() { changed = true; }
                if r2.double_clicked() { s.brightness_hz = def.brightness_hz; changed = true; }
                ui.end_row();

                ui.label("Duration / Decay:");
                let r3 = ui.add(egui::Slider::new(&mut s.decay_ms, decay_range).suffix(" ms"));
                if r3.changed() { changed = true; }
                if r3.double_clicked() { s.decay_ms = def.decay_ms; changed = true; }
                ui.end_row();

                ui.label(texture_label);
                let r4 = ui.add(egui::Slider::new(&mut s.texture_rate, 0.0..=1.0));
                if r4.changed() { changed = true; }
                if r4.double_clicked() { s.texture_rate = def.texture_rate; changed = true; }
                ui.end_row();

                ui.label("Body Resonance Q:");
                let r5 = ui.add(egui::Slider::new(&mut s.resonance_q, 0.5..=10.0));
                if r5.changed() { changed = true; }
                if r5.double_clicked() { s.resonance_q = def.resonance_q; changed = true; }
                ui.end_row();

                add_hit_variation_slider_row(
                    ui,
                    &mut s.hit_variation,
                    "Logarithmic humanization of hit velocity",
                    &mut changed,
                );
            });
    });
    changed
}

/// 2-Operator FM Melodic Backing Synthesizer Tab.
fn render_fm_tab(ui: &mut egui::Ui, app: &mut MnxEditorApp, commands: &mut Vec<SynthCommand>) {
    let mut changed = false;
    let mut fmp = app.synth_lab.preset.fm_params;
    let target_idx = app.synth_lab.target_part_idx;

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
        app.synth_lab.preset.fm_params = fmp;
        app.set_part_synth_preset(target_idx, app.synth_lab.preset.clone());
        commands.push(SynthCommand::UpdateGmVoiceParams(fmp));
        commands.push(SynthCommand::UpdatePartFmParams {
            part_idx: target_idx as u8,
            params: fmp,
        });
    }
}

/// Helper to strum the open strings of the active tab part using microtonal tuning.
fn strum_current_part_open_strings(app: &mut MnxEditorApp) {
    let _ = app.ensure_synth_engine();
    let target_p = app.synth_lab.target_part_idx as u8;
    let active_p = app.selected_part_idx as u8;

    if let Some(doc) = &app.parsed_mnx {
        if let Some(part) = doc
            .parts
            .get(app.synth_lab.target_part_idx)
            .or_else(|| doc.parts.first())
        {
            if part.is_percussion() {
                if let Some(engine) = &mut app.synth_engine {
                    engine.send(SynthCommand::SetActivePart(target_p));
                    for (line, _id, comp) in part.ordered_kit_lines() {
                        engine.send(SynthCommand::TriggerDrum {
                            gm_drum_note: comp.sound.unwrap_or(38).min(127),
                            velocity: app.synth_lab.velocity,
                        });
                        let _ = line;
                    }
                    if target_p != active_p {
                        engine.send(SynthCommand::SetActivePart(active_p));
                    }
                }
            } else {
                let table = crate::tuning_table::FretboardFrequencyTable::from_doc_part(doc, part);
                let n_str = part.num_strings() as u8;
                let pluck_pos = app.synth_lab.preset.string_params.pluck_pos;
                let palm_mute = app.synth_lab.palm_mute;
                let velocity = app.synth_lab.velocity;

                if let Some(engine) = &mut app.synth_engine {
                    engine.send(SynthCommand::SetActivePart(target_p));
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
                    if target_p != active_p {
                        engine.send(SynthCommand::SetActivePart(active_p));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod drum_palette_tests {
    use super::*;
    use crate::mnx::Part;
    use crate::presets::{builtin_drum_kit_presets, line_configs_to_kit, standard_6_line_kit_configs};

    fn make_perc_part(lines: &[crate::presets::PercussionLineConfig]) -> Part {
        let mut p = Part::default();
        p.kit = Some(line_configs_to_kit(lines));
        p
    }

    #[test]
    fn test_drum_palette_scoping_matches_tab_kit_lines() {
        // 1. Standard 6-Line Drum Kit exposes ONLY its mapped roles (no Agogo/Congas/Cuica/Whistle)
        let rock_part = make_perc_part(&standard_6_line_kit_configs());
        let (rock_palette, mapped_count) = collect_part_drum_palette(Some(&rock_part), false);
        let roles: Vec<DrumRole> = rock_palette.iter().map(|e| e.role).collect();

        assert_eq!(rock_palette.len(), mapped_count);
        assert!(roles.contains(&DrumRole::Kick));
        assert!(roles.contains(&DrumRole::Snare)); // includes both 38 and 37 Side Stick
        assert!(roles.contains(&DrumRole::Toms));
        assert!(roles.contains(&DrumRole::HiHats));
        assert!(roles.contains(&DrumRole::Crash1));
        assert!(roles.contains(&DrumRole::Splash));
        assert!(roles.contains(&DrumRole::China));
        assert!(roles.contains(&DrumRole::Ride1AndBell)); // 53 mapped on Snare 'b' symbol
        assert!(!roles.contains(&DrumRole::Agogo));
        assert!(!roles.contains(&DrumRole::Congas));
        assert!(!roles.contains(&DrumRole::Cuica));
        assert!(!roles.contains(&DrumRole::Whistle));
        assert!(!roles.contains(&DrumRole::AcousticKick));
        assert!(!roles.contains(&DrumRole::ElectricSnare));

        // Verify Toms entry exposes ONLY the mapped toms (43 High Floor, 48 Hi-Mid) for individual tuning!
        let toms_entry = rock_palette.iter().find(|e| e.role == DrumRole::Toms).unwrap();
        assert_eq!(toms_entry.mapped_notes, vec![43, 48]);

        // Verify Snare entry groups 37 (Side Stick) and 38 (Acoustic Snare) together!
        let snare_entry = rock_palette.iter().find(|e| e.role == DrumRole::Snare).unwrap();
        assert_eq!(snare_entry.mapped_notes, vec![37, 38]);

        // 2. Toggle show_all_gm = true exposes all 28 GM roles while preserving mapped_count
        let (all_palette, all_mapped_count) = collect_part_drum_palette(Some(&rock_part), true);
        assert_eq!(all_palette.len(), 28);
        assert_eq!(all_mapped_count, mapped_count);

        // 3. Latin / Aux Percussion (6-Line) kit exposes Cowbell, Agogo, Timbales, Bongos, Congas, Claves, Maracas, Tambourine, Side Stick
        let presets = builtin_drum_kit_presets();
        let (_, latin_lines) = presets
            .iter()
            .find(|(name, _)| name.contains("Latin"))
            .expect("Latin kit preset should exist");
        let latin_part = make_perc_part(latin_lines);
        let (latin_palette, _) = collect_part_drum_palette(Some(&latin_part), false);
        let latin_roles: Vec<DrumRole> = latin_palette.iter().map(|e| e.role).collect();

        assert!(latin_roles.contains(&DrumRole::Cowbell));
        assert!(latin_roles.contains(&DrumRole::Agogo));
        assert!(latin_roles.contains(&DrumRole::Timbales));
        assert!(latin_roles.contains(&DrumRole::Bongos));
        assert!(latin_roles.contains(&DrumRole::Congas));
        assert!(latin_roles.contains(&DrumRole::Claves));
        assert!(latin_roles.contains(&DrumRole::Maracas));
        assert!(latin_roles.contains(&DrumRole::Tambourine));
        assert!(!latin_roles.contains(&DrumRole::Kick));
        assert!(!latin_roles.contains(&DrumRole::HiHats));
        assert!(!latin_roles.contains(&DrumRole::Crash1));

        // Verify Congas entry groups 62, 63, 64 together
        let congas_entry = latin_palette.iter().find(|e| e.role == DrumRole::Congas).unwrap();
        assert_eq!(congas_entry.mapped_notes, vec![62, 63, 64]);
    }
}
