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
    AdsrEnvelopeParams, DrumKitParams, DrumRole, DrumVoiceParams, FmVoiceParams, PitchedEngine,
    SynthPreset,
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
        .default_width(640.0)
        .default_height(780.0)
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
                        engine.send(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::Kick,
                            params: DrumVoiceParams::Kick(loaded.drum_params.kick),
                        });
                        engine.send(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::Snare,
                            params: DrumVoiceParams::Snare(loaded.drum_params.snare),
                        });
                        engine.send(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::Toms,
                            params: DrumVoiceParams::Toms(loaded.drum_params.toms),
                        });
                        engine.send(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::HiHats,
                            params: DrumVoiceParams::HiHats(loaded.drum_params.hi_hats),
                        });
                        engine.send(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::Cymbals,
                            params: DrumVoiceParams::Cymbals(loaded.drum_params.cymbals),
                        });
                        engine.send(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::PercAux,
                            params: DrumVoiceParams::PercAux(loaded.drum_params.aux),
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
                        commands.push(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::Kick,
                            params: DrumVoiceParams::Kick(p.drum_params.kick),
                        });
                        commands.push(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::Snare,
                            params: DrumVoiceParams::Snare(p.drum_params.snare),
                        });
                        commands.push(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::Toms,
                            params: DrumVoiceParams::Toms(p.drum_params.toms),
                        });
                        commands.push(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::HiHats,
                            params: DrumVoiceParams::HiHats(p.drum_params.hi_hats),
                        });
                        commands.push(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::Cymbals,
                            params: DrumVoiceParams::Cymbals(p.drum_params.cymbals),
                        });
                        commands.push(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::PercAux,
                            params: DrumVoiceParams::PercAux(p.drum_params.aux),
                        });
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
                        commands.push(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::Kick,
                            params: DrumVoiceParams::Kick(p.drum_params.kick),
                        });
                        commands.push(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::Snare,
                            params: DrumVoiceParams::Snare(p.drum_params.snare),
                        });
                        commands.push(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::Toms,
                            params: DrumVoiceParams::Toms(p.drum_params.toms),
                        });
                        commands.push(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::HiHats,
                            params: DrumVoiceParams::HiHats(p.drum_params.hi_hats),
                        });
                        commands.push(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::Cymbals,
                            params: DrumVoiceParams::Cymbals(p.drum_params.cymbals),
                        });
                        commands.push(SynthCommand::UpdateDrumParams {
                            drum_role: DrumRole::PercAux,
                            params: DrumVoiceParams::PercAux(p.drum_params.aux),
                        });
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
            commands.push(SynthCommand::UpdateDrumParams {
                drum_role: DrumRole::Kick,
                params: DrumVoiceParams::Kick(def.drum_params.kick),
            });
            commands.push(SynthCommand::UpdateDrumParams {
                drum_role: DrumRole::Snare,
                params: DrumVoiceParams::Snare(def.drum_params.snare),
            });
            commands.push(SynthCommand::UpdateDrumParams {
                drum_role: DrumRole::Toms,
                params: DrumVoiceParams::Toms(def.drum_params.toms),
            });
            commands.push(SynthCommand::UpdateDrumParams {
                drum_role: DrumRole::HiHats,
                params: DrumVoiceParams::HiHats(def.drum_params.hi_hats),
            });
            commands.push(SynthCommand::UpdateDrumParams {
                drum_role: DrumRole::Cymbals,
                params: DrumVoiceParams::Cymbals(def.drum_params.cymbals),
            });
            commands.push(SynthCommand::UpdateDrumParams {
                drum_role: DrumRole::PercAux,
                params: DrumVoiceParams::PercAux(def.drum_params.aux),
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
            // Percussion Audition Strip
            let quick_drums = [
                ("Kick (36)", 36),
                ("Snare (38)", 38),
                ("Rim (37)", 37),
                ("Clap (39)", 39),
                ("Closed Hat (42)", 42),
                ("Open Hat (46)", 46),
                ("Crash (49)", 49),
                ("Cowbell (56)", 56),
            ];
            for (label, note) in quick_drums {
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

/// Paints procedural drum envelopes and waveforms based on the active drum role.
fn paint_drum_envelopes_preview(
    painter: &egui::Painter,
    rect: Rect,
    params: &DrumKitParams,
    active_role: DrumRole,
) {
    let half_w = rect.width() * 0.48;

    match active_role {
        DrumRole::Kick => {
            // Plot Kick pitch envelope & amplitude decay
            let mut pitch_pts: Vec<Pos2> = Vec::with_capacity(41);
            let p_span = params.kick.pitch_start_hz - params.kick.pitch_end_hz;
            for s in 0..=40 {
                let t = (s as f32) / 40.0;
                let env = (-t * (100.0 / params.kick.pitch_decay_ms.max(5.0))).exp();
                let f = params.kick.pitch_end_hz + p_span * env;
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
                let amp = (-t * (400.0 / params.kick.body_decay_ms.max(10.0))).exp();
                let px = right_start + t * right_w;
                let py = rect.max.y - (amp.clamp(0.0, 1.0) * (rect.height() - 18.0) + 9.0);
                body_pts.push(Pos2::new(px, py));
            }
            painter.add(egui::Shape::line(body_pts, Stroke::new(2.0, Color32::from_rgb(100, 220, 140))));

            painter.text(
                Pos2::new(rect.min.x + 8.0, rect.min.y + 6.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "Kick (35, 36): Pitch {:.0}->{:.0}Hz ({:.0}ms) | Body Decay: {:.0}ms",
                    params.kick.pitch_start_hz, params.kick.pitch_end_hz, params.kick.pitch_decay_ms, params.kick.body_decay_ms
                ),
                egui::FontId::proportional(11.0),
                Color32::from_rgb(255, 200, 160),
            );
        }
        DrumRole::Snare => {
            // Plot Snare tone vs wire noise envelope
            let mut wire_pts: Vec<Pos2> = Vec::with_capacity(41);
            for s in 0..=40 {
                let t = (s as f32) / 40.0;
                let amp = (-t * (300.0 / params.snare.noise_decay_ms.max(10.0))).exp();
                let px = rect.min.x + t * rect.width();
                let py = rect.max.y - (amp.clamp(0.0, 1.0) * (rect.height() - 18.0) + 9.0);
                wire_pts.push(Pos2::new(px, py));
            }
            painter.add(egui::Shape::line(wire_pts, Stroke::new(2.0, Color32::from_rgb(120, 190, 255))));

            painter.text(
                Pos2::new(rect.min.x + 8.0, rect.min.y + 6.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "Snare/Clap (37..40): Modes {:.0}/{:.0}Hz | Wire Cutoff: {:.0}Hz | Noise Decay: {:.0}ms",
                    params.snare.tone_freq_hz, params.snare.mode2_freq_hz, params.snare.wire_cutoff_hz, params.snare.noise_decay_ms
                ),
                egui::FontId::proportional(11.0),
                Color32::from_rgb(200, 220, 255),
            );
        }
        DrumRole::Toms => {
            let mut tom_pts: Vec<Pos2> = Vec::with_capacity(41);
            for s in 0..=40 {
                let t = (s as f32) / 40.0;
                let amp = (-t * (400.0 / params.toms.decay_ms.max(10.0))).exp();
                let px = rect.min.x + t * rect.width();
                let py = rect.max.y - (amp.clamp(0.0, 1.0) * (rect.height() - 18.0) + 9.0);
                tom_pts.push(Pos2::new(px, py));
            }
            painter.add(egui::Shape::line(tom_pts, Stroke::new(2.0, Color32::from_rgb(220, 160, 240))));

            painter.text(
                Pos2::new(rect.min.x + 8.0, rect.min.y + 6.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "Toms (41, 43, 45, 47, 48, 50): Scale {:.2}x | Drop Ratio {:.2} | Head Decay {:.0}ms",
                    params.toms.pitch_scale, params.toms.pitch_drop_ratio, params.toms.decay_ms
                ),
                egui::FontId::proportional(11.0),
                Color32::from_rgb(235, 205, 250),
            );
        }
        DrumRole::HiHats => {
            let mut closed_pts: Vec<Pos2> = Vec::with_capacity(41);
            let mut open_pts: Vec<Pos2> = Vec::with_capacity(41);
            for s in 0..=40 {
                let t = (s as f32) / 40.0;
                let amp_c = (-t * (100.0 / params.hi_hats.closed_decay_ms.max(5.0))).exp();
                let amp_o = (-t * (800.0 / params.hi_hats.open_decay_ms.max(10.0))).exp();
                let px = rect.min.x + t * rect.width();
                closed_pts.push(Pos2::new(px, rect.max.y - (amp_c * (rect.height() - 20.0) + 10.0)));
                open_pts.push(Pos2::new(px, rect.max.y - (amp_o * (rect.height() - 20.0) + 10.0)));
            }
            painter.add(egui::Shape::line(open_pts, Stroke::new(1.5, Color32::from_rgb(255, 220, 120))));
            painter.add(egui::Shape::line(closed_pts, Stroke::new(2.0, Color32::from_rgb(140, 240, 220))));

            painter.text(
                Pos2::new(rect.min.x + 8.0, rect.min.y + 6.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "Hi-Hats (42, 44, 46): HP Cutoff {:.0}Hz | Closed: {:.0}ms | Open: {:.0}ms",
                    params.hi_hats.highpass_cutoff_hz, params.hi_hats.closed_decay_ms, params.hi_hats.open_decay_ms
                ),
                egui::FontId::proportional(11.0),
                Color32::from_rgb(240, 240, 180),
            );
        }
        DrumRole::Cymbals => {
            let mut crash_pts: Vec<Pos2> = Vec::with_capacity(41);
            let mut ride_pts: Vec<Pos2> = Vec::with_capacity(41);
            for s in 0..=40 {
                let t = (s as f32) / 40.0;
                let amp_c = (-t * (1500.0 / params.cymbals.crash_decay_ms.max(20.0))).exp();
                let amp_r = (-t * (1500.0 / params.cymbals.ride_decay_ms.max(20.0))).exp();
                let px = rect.min.x + t * rect.width();
                crash_pts.push(Pos2::new(px, rect.max.y - (amp_c * (rect.height() - 20.0) + 10.0)));
                ride_pts.push(Pos2::new(px, rect.max.y - (amp_r * (rect.height() - 20.0) + 10.0)));
            }
            painter.add(egui::Shape::line(crash_pts, Stroke::new(1.5, Color32::from_rgb(255, 120, 160))));
            painter.add(egui::Shape::line(ride_pts, Stroke::new(2.0, Color32::from_rgb(160, 210, 255))));

            painter.text(
                Pos2::new(rect.min.x + 8.0, rect.min.y + 6.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "Cymbals (49, 51, 52, 53, 55, 57, 59): Cutoff {:.0}Hz | Crash: {:.0}ms | Ride: {:.0}ms",
                    params.cymbals.cutoff_hz, params.cymbals.crash_decay_ms, params.cymbals.ride_decay_ms
                ),
                egui::FontId::proportional(11.0),
                Color32::from_rgb(255, 200, 220),
            );
        }
        DrumRole::PercAux => {
            let mut aux_pts: Vec<Pos2> = Vec::with_capacity(41);
            for s in 0..=40 {
                let t = (s as f32) / 40.0;
                let amp = (-t * (250.0 / params.aux.decay_ms.max(10.0))).exp();
                let px = rect.min.x + t * rect.width();
                aux_pts.push(Pos2::new(px, rect.max.y - (amp * (rect.height() - 20.0) + 10.0)));
            }
            painter.add(egui::Shape::line(aux_pts, Stroke::new(2.0, Color32::from_rgb(255, 215, 110))));

            painter.text(
                Pos2::new(rect.min.x + 8.0, rect.min.y + 6.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "Cowbell & Aux (56): Modes {:.0}Hz + {:.0}Hz (Q {:.1}) | Decay: {:.0}ms",
                    params.aux.freq1_hz, params.aux.freq2_hz, params.aux.bandpass_q, params.aux.decay_ms
                ),
                egui::FontId::proportional(11.0),
                Color32::from_rgb(255, 235, 180),
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

/// Expanded, General MIDI Labeled Drum Kit Editor.
fn render_drum_kit_tab(ui: &mut egui::Ui, app: &mut MnxEditorApp, commands: &mut Vec<SynthCommand>) {
    let mut dp = app.synth_lab.preset.drum_params;
    let target_idx = app.synth_lab.target_part_idx;

    // Drum Role Selector with GM note labels
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Drum Voice:").strong());

        let roles = [
            ("Bass Drums (35, 36)", DrumRole::Kick, 36),
            ("Snares & Claps (37..40)", DrumRole::Snare, 38),
            ("Toms (41..50)", DrumRole::Toms, 45),
            ("Hi-Hats (42, 44, 46)", DrumRole::HiHats, 42),
            ("Cymbals (49..59)", DrumRole::Cymbals, 49),
            ("Cowbell & Aux (56)", DrumRole::PercAux, 56),
        ];

        for (label, role, def_note) in roles {
            if ui.selectable_label(app.synth_lab.active_drum_role == role, label).clicked() {
                app.synth_lab.active_drum_role = role;
                app.synth_lab.active_drum_note = def_note;
            }
        }
    });

    ui.add_space(4.0);

    // Dedicated "Trigger [Selected Drum]" Audition Bar
    ui.group(|ui| {
        ui.horizontal(|ui| {
            let (role_name, active_note) = match app.synth_lab.active_drum_role {
                DrumRole::Kick => ("Kick", app.synth_lab.active_drum_note.clamp(35, 36)),
                DrumRole::Snare => ("Snare", app.synth_lab.active_drum_note.clamp(37, 40)),
                DrumRole::Toms => ("Tom", app.synth_lab.active_drum_note.clamp(41, 50)),
                DrumRole::HiHats => ("Hi-Hat", app.synth_lab.active_drum_note.clamp(42, 46)),
                DrumRole::Cymbals => ("Cymbal", app.synth_lab.active_drum_note.clamp(49, 59)),
                DrumRole::PercAux => ("Cowbell", 56),
            };

            if ui
                .button(egui::RichText::new(format!("Trigger {} (Note {})", role_name, active_note)).strong())
                .clicked()
            {
                commands.push(SynthCommand::TriggerDrum {
                    gm_drum_note: active_note,
                    velocity: app.synth_lab.velocity,
                });
            }

            ui.separator();
            ui.label(egui::RichText::new("Quick Audition:").weak());

            match app.synth_lab.active_drum_role {
                DrumRole::Kick => {
                    for (note_name, note) in [("35: Acoustic Bass", 35), ("36: Electric Bass 1", 36)] {
                        if ui.small_button(note_name).clicked() {
                            app.synth_lab.active_drum_note = note;
                            commands.push(SynthCommand::TriggerDrum { gm_drum_note: note, velocity: app.synth_lab.velocity });
                        }
                    }
                }
                DrumRole::Snare => {
                    for (note_name, note) in [
                        ("37: Side Stick", 37),
                        ("38: Acoustic Snare", 38),
                        ("39: Hand Clap", 39),
                        ("40: Electric Snare", 40),
                    ] {
                        if ui.small_button(note_name).clicked() {
                            app.synth_lab.active_drum_note = note;
                            commands.push(SynthCommand::TriggerDrum { gm_drum_note: note, velocity: app.synth_lab.velocity });
                        }
                    }
                }
                DrumRole::Toms => {
                    for (note_name, note) in [
                        ("41: Low Floor", 41),
                        ("43: High Floor", 43),
                        ("45: Low", 45),
                        ("47: Low-Mid", 47),
                        ("48: Hi-Mid", 48),
                        ("50: High", 50),
                    ] {
                        if ui.small_button(note_name).clicked() {
                            app.synth_lab.active_drum_note = note;
                            commands.push(SynthCommand::TriggerDrum { gm_drum_note: note, velocity: app.synth_lab.velocity });
                        }
                    }
                }
                DrumRole::HiHats => {
                    for (note_name, note) in [
                        ("42: Closed Hat", 42),
                        ("44: Pedal Hat", 44),
                        ("46: Open Hat", 46),
                    ] {
                        if ui.small_button(note_name).clicked() {
                            app.synth_lab.active_drum_note = note;
                            commands.push(SynthCommand::TriggerDrum { gm_drum_note: note, velocity: app.synth_lab.velocity });
                        }
                    }
                }
                DrumRole::Cymbals => {
                    for (note_name, note) in [
                        ("49: Crash 1", 49),
                        ("51: Ride 1", 51),
                        ("52: China", 52),
                        ("53: Ride Bell", 53),
                        ("55: Splash", 55),
                        ("57: Crash 2", 57),
                        ("59: Ride 2", 59),
                    ] {
                        if ui.small_button(note_name).clicked() {
                            app.synth_lab.active_drum_note = note;
                            commands.push(SynthCommand::TriggerDrum { gm_drum_note: note, velocity: app.synth_lab.velocity });
                        }
                    }
                }
                DrumRole::PercAux => {
                    if ui.small_button("56: Cowbell").clicked() {
                        app.synth_lab.active_drum_note = 56;
                        commands.push(SynthCommand::TriggerDrum { gm_drum_note: 56, velocity: app.synth_lab.velocity });
                    }
                }
            }
        });
    });

    ui.add_space(4.0);

    // Active Drum Role Parameter Sliders
    match app.synth_lab.active_drum_role {
        DrumRole::Kick => {
            ui.group(|ui| {
                ui.label(egui::RichText::new("Bass Drum Voice Parameters (GM 35, 36):").strong());
                let mut k_changed = false;
                let k = &mut dp.kick;

                egui::Grid::new("kick_params_grid")
                    .num_columns(2)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Kick Volume:").strong())
                            .on_hover_text("Absolute output level for Kick (GM 35, 36). -48 dB = Mute, double-click resets to 0 dB.");
                        let rv = ui.add(
                            egui::Slider::new(&mut k.volume_db, -48.0..=12.0)
                                .suffix(" dB")
                                .fixed_decimals(1)
                                .clamping(egui::SliderClamping::Always),
                        );
                        if rv.changed() { k_changed = true; }
                        if rv.double_clicked() { k.volume_db = 0.0; k_changed = true; }
                        ui.end_row();

                        ui.label("Pitch Envelope Start:");
                        let r1 = ui.add(egui::Slider::new(&mut k.pitch_start_hz, 80.0..=250.0).suffix(" Hz"));
                        if r1.changed() { k_changed = true; }
                        ui.end_row();

                        ui.label("Resting Sub-Bass Pitch:");
                        let r2 = ui.add(egui::Slider::new(&mut k.pitch_end_hz, 30.0..=65.0).suffix(" Hz"));
                        if r2.changed() { k_changed = true; }
                        ui.end_row();

                        ui.label("Pitch Drop Time:");
                        let r3 = ui.add(egui::Slider::new(&mut k.pitch_decay_ms, 10.0..=80.0).suffix(" ms"));
                        if r3.changed() { k_changed = true; }
                        ui.end_row();

                        ui.label("Shell Body Decay:");
                        let r4 = ui.add(egui::Slider::new(&mut k.body_decay_ms, 50.0..=600.0).suffix(" ms"));
                        if r4.changed() { k_changed = true; }
                        ui.end_row();

                        ui.label("Click Beater Level:");
                        let r5 = ui.add(egui::Slider::new(&mut k.click_level, 0.0..=1.0));
                        if r5.changed() { k_changed = true; }
                        if r5.double_clicked() { k.click_level = 0.65; k_changed = true; }
                        ui.end_row();

                        ui.label("Head Tension Mod (Pitch Bend):")
                            .on_hover_text("Non-linear circular membrane tension pitch stretch on hard hits (30–80 ms)");
                        let r6 = ui.add(egui::Slider::new(&mut k.head_tension_mod, 0.0..=1.0));
                        if r6.changed() { k_changed = true; }
                        if r6.double_clicked() { k.head_tension_mod = 0.55; k_changed = true; }
                        ui.end_row();

                        ui.label("Strike Position (Center vs Edge):")
                            .on_hover_text("0.0 = dead-center punch (1.00, 2.29 modes), 1.0 = rim/edge ring (1.59, 2.13, 2.65 modes)");
                        let r7 = ui.add(egui::Slider::new(&mut k.strike_position, 0.0..=1.0));
                        if r7.changed() { k_changed = true; }
                        if r7.double_clicked() { k.strike_position = 0.15; k_changed = true; }
                        ui.end_row();

                        ui.label("Overtones Ring (Bessel Decay):")
                            .on_hover_text("Controls how long upper Bessel membrane modes ring relative to the (0,1) fundamental");
                        let r8 = ui.add(egui::Slider::new(&mut k.overtones_ring, 0.0..=1.0));
                        if r8.changed() { k_changed = true; }
                        if r8.double_clicked() { k.overtones_ring = 0.30; k_changed = true; }
                        ui.end_row();

                        ui.label("Shell Wood Cavity:")
                            .on_hover_text("Cylindrical wooden drum shell air-cavity mode & < 1.5 ms 2-tap micro-reflection body");
                        let r9 = ui.add(egui::Slider::new(&mut k.shell_wood_cavity, 0.0..=1.0));
                        if r9.changed() { k_changed = true; }
                        if r9.double_clicked() { k.shell_wood_cavity = 0.50; k_changed = true; }
                        ui.end_row();

                        ui.label("Hit Variation (Vel & Strike):")
                            .on_hover_text("Logarithmic humanization of hit velocity & strike position (0.0 = exact/identical, 1.0 = heavy variation)");
                        let r10 = ui.add(
                            egui::Slider::new(&mut k.hit_variation, 0.0..=1.0)
                                .logarithmic(true)
                                .smallest_positive(0.005),
                        );
                        if r10.changed() { k_changed = true; }
                        if r10.double_clicked() { k.hit_variation = 0.10; k_changed = true; }
                        ui.end_row();
                    });

                if k_changed {
                    app.synth_lab.preset.drum_params.kick = *k;
                    app.set_part_synth_preset(target_idx, app.synth_lab.preset.clone());
                    commands.push(SynthCommand::UpdateDrumParams {
                        drum_role: DrumRole::Kick,
                        params: DrumVoiceParams::Kick(*k),
                    });
                }
            });
        }
        DrumRole::Snare => {
            ui.group(|ui| {
                ui.label(egui::RichText::new("Snare, Rim & Clap Voice Parameters (GM 37, 38, 39, 40):").strong());
                let mut s_changed = false;
                let s = &mut dp.snare;

                egui::Grid::new("snare_params_grid")
                    .num_columns(2)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Snare Volume:").strong())
                            .on_hover_text("Absolute output level for Acoustic/Electric Snare (GM 38, 40). -48 dB = Mute, double-click resets to 0 dB.");
                        let rv1 = ui.add(
                            egui::Slider::new(&mut s.volume_db, -48.0..=12.0)
                                .suffix(" dB")
                                .fixed_decimals(1)
                                .clamping(egui::SliderClamping::Always),
                        );
                        if rv1.changed() { s_changed = true; }
                        if rv1.double_clicked() { s.volume_db = 0.0; s_changed = true; }
                        ui.end_row();

                        ui.label(egui::RichText::new("Side Stick Volume:").strong())
                            .on_hover_text("Absolute output level for Side Stick / Rimshot (GM 37). -48 dB = Mute, double-click resets to 0 dB.");
                        let rv2 = ui.add(
                            egui::Slider::new(&mut s.rim_volume_db, -48.0..=12.0)
                                .suffix(" dB")
                                .fixed_decimals(1)
                                .clamping(egui::SliderClamping::Always),
                        );
                        if rv2.changed() { s_changed = true; }
                        if rv2.double_clicked() { s.rim_volume_db = 0.0; s_changed = true; }
                        ui.end_row();

                        ui.label(egui::RichText::new("Hand Clap Volume:").strong())
                            .on_hover_text("Absolute output level for Hand Clap (GM 39). -48 dB = Mute, double-click resets to 0 dB.");
                        let rv3 = ui.add(
                            egui::Slider::new(&mut s.clap_volume_db, -48.0..=12.0)
                                .suffix(" dB")
                                .fixed_decimals(1)
                                .clamping(egui::SliderClamping::Always),
                        );
                        if rv3.changed() { s_changed = true; }
                        if rv3.double_clicked() { s.clap_volume_db = 0.0; s_changed = true; }
                        ui.end_row();

                        ui.label("Mode 1 Tone Freq:");
                        let r1 = ui.add(egui::Slider::new(&mut s.tone_freq_hz, 120.0..=350.0).suffix(" Hz"));
                        if r1.changed() { s_changed = true; }
                        ui.end_row();

                        ui.label("Mode 2 Resonance:");
                        let r2 = ui.add(egui::Slider::new(&mut s.mode2_freq_hz, 200.0..=600.0).suffix(" Hz"));
                        if r2.changed() { s_changed = true; }
                        ui.end_row();

                        ui.label("Shell Tone Decay:");
                        let r3 = ui.add(egui::Slider::new(&mut s.tone_decay_ms, 20.0..=200.0).suffix(" ms"));
                        if r3.changed() { s_changed = true; }
                        ui.end_row();

                        ui.label("Wire Noise Cutoff:");
                        let r4 = ui.add(egui::Slider::new(&mut s.wire_cutoff_hz, 1000.0..=5000.0).suffix(" Hz"));
                        if r4.changed() { s_changed = true; }
                        ui.end_row();

                        ui.label("Wire Noise Decay:");
                        let r5 = ui.add(egui::Slider::new(&mut s.noise_decay_ms, 50.0..=400.0).suffix(" ms"));
                        if r5.changed() { s_changed = true; }
                        ui.end_row();

                        ui.label("Noise / Tone Mix:");
                        let r6 = ui.add(egui::Slider::new(&mut s.noise_mix, 0.0..=1.0));
                        if r6.changed() { s_changed = true; }
                        ui.end_row();

                        ui.label("Clap Burst Spacing:");
                        let r7 = ui.add(egui::Slider::new(&mut s.clap_spacing_ms, 5.0..=30.0).suffix(" ms"));
                        if r7.changed() { s_changed = true; }
                        if r7.double_clicked() { s.clap_spacing_ms = 12.0; s_changed = true; }
                        ui.end_row();

                        ui.label("Head Tension Mod (Pitch Bend):")
                            .on_hover_text("Non-linear circular membrane tension pitch stretch on hard snare hits");
                        let r8 = ui.add(egui::Slider::new(&mut s.head_tension_mod, 0.0..=1.0));
                        if r8.changed() { s_changed = true; }
                        if r8.double_clicked() { s.head_tension_mod = 0.40; s_changed = true; }
                        ui.end_row();

                        ui.label("Strike Position (Center vs Edge):")
                            .on_hover_text("0.0 = dead-center punch (1.00, 2.29 modes), 1.0 = rim/edge overtone ring (1.59, 2.13, 2.65)");
                        let r9 = ui.add(egui::Slider::new(&mut s.strike_position, 0.0..=1.0));
                        if r9.changed() { s_changed = true; }
                        if r9.double_clicked() { s.strike_position = 0.32; s_changed = true; }
                        ui.end_row();

                        ui.label("Overtones Ring (Bessel Decay):")
                            .on_hover_text("Higher Bessel drumhead overtone ring duration relative to the fundamental");
                        let r10 = ui.add(egui::Slider::new(&mut s.overtones_ring, 0.0..=1.0));
                        if r10.changed() { s_changed = true; }
                        if r10.double_clicked() { s.overtones_ring = 0.45; s_changed = true; }
                        ui.end_row();

                        ui.label("Shell Wood Cavity:")
                            .on_hover_text("Wooden snare shell air-cavity mode & < 1.5 ms 2-tap micro-reflection body");
                        let r11 = ui.add(egui::Slider::new(&mut s.shell_wood_cavity, 0.0..=1.0));
                        if r11.changed() { s_changed = true; }
                        if r11.double_clicked() { s.shell_wood_cavity = 0.52; s_changed = true; }
                        ui.end_row();

                        ui.label("Snare Wire Tension / Buzz:")
                            .on_hover_text("Non-linear membrane-coupled snare strainer threshold (0.0 = loose buzz, 1.0 = tight snap)");
                        let r12 = ui.add(egui::Slider::new(&mut s.wire_tension, 0.0..=1.0));
                        if r12.changed() { s_changed = true; }
                        if r12.double_clicked() { s.wire_tension = 0.55; s_changed = true; }
                        ui.end_row();

                        ui.label("Hit Variation (Vel & Strike):")
                            .on_hover_text("Logarithmic humanization of hit velocity & strike position (0.0 = exact/identical, 1.0 = heavy variation)");
                        let r13 = ui.add(
                            egui::Slider::new(&mut s.hit_variation, 0.0..=1.0)
                                .logarithmic(true)
                                .smallest_positive(0.005),
                        );
                        if r13.changed() { s_changed = true; }
                        if r13.double_clicked() { s.hit_variation = 0.10; s_changed = true; }
                        ui.end_row();
                    });

                if s_changed {
                    app.synth_lab.preset.drum_params.snare = *s;
                    app.set_part_synth_preset(target_idx, app.synth_lab.preset.clone());
                    commands.push(SynthCommand::UpdateDrumParams {
                        drum_role: DrumRole::Snare,
                        params: DrumVoiceParams::Snare(*s),
                    });
                }
            });
        }
        DrumRole::Toms => {
            ui.group(|ui| {
                ui.label(egui::RichText::new("Tuned Tom-Toms (GM 41, 43, 45, 47, 48, 50):").strong());
                let mut t_changed = false;
                let t = &mut dp.toms;

                egui::Grid::new("tom_params_grid")
                    .num_columns(2)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Toms Volume:").strong())
                            .on_hover_text("Absolute output level for Toms (GM 41–50). -48 dB = Mute, double-click resets to 0 dB.");
                        let rv = ui.add(
                            egui::Slider::new(&mut t.volume_db, -48.0..=12.0)
                                .suffix(" dB")
                                .fixed_decimals(1)
                                .clamping(egui::SliderClamping::Always),
                        );
                        if rv.changed() { t_changed = true; }
                        if rv.double_clicked() { t.volume_db = 0.0; t_changed = true; }
                        ui.end_row();

                        ui.label("Pitch Scaling Multiplier:");
                        let r1 = ui.add(egui::Slider::new(&mut t.pitch_scale, 0.5..=2.0));
                        if r1.changed() { t_changed = true; }
                        ui.end_row();

                        ui.label("Pitch Drop Sweep Ratio:");
                        let r2 = ui.add(egui::Slider::new(&mut t.pitch_drop_ratio, 1.2..=3.0));
                        if r2.changed() { t_changed = true; }
                        ui.end_row();

                        ui.label("Tomhead Decay Time:");
                        let r3 = ui.add(egui::Slider::new(&mut t.decay_ms, 50.0..=800.0).suffix(" ms"));
                        if r3.changed() { t_changed = true; }
                        ui.end_row();

                        ui.label("Stick Attack Level:");
                        let r4 = ui.add(egui::Slider::new(&mut t.stick_attack, 0.0..=1.0));
                        if r4.changed() { t_changed = true; }
                        if r4.double_clicked() { t.stick_attack = 0.60; t_changed = true; }
                        ui.end_row();

                        ui.label("Head Tension Mod (Pitch Bend):")
                            .on_hover_text("Non-linear circular membrane tension pitch stretch on hard tom hits (30–80 ms)");
                        let r5 = ui.add(egui::Slider::new(&mut t.head_tension_mod, 0.0..=1.0));
                        if r5.changed() { t_changed = true; }
                        if r5.double_clicked() { t.head_tension_mod = 0.50; t_changed = true; }
                        ui.end_row();

                        ui.label("Strike Position (Center vs Edge):")
                            .on_hover_text("0.0 = dead-center punch (1.00, 2.29 modes), 1.0 = rim/edge overtone ring (1.59, 2.13, 2.65)");
                        let r6 = ui.add(egui::Slider::new(&mut t.strike_position, 0.0..=1.0));
                        if r6.changed() { t_changed = true; }
                        if r6.double_clicked() { t.strike_position = 0.28; t_changed = true; }
                        ui.end_row();

                        ui.label("Overtones Ring (Bessel Decay):")
                            .on_hover_text("Higher Bessel drumhead overtone ring duration relative to the fundamental");
                        let r7 = ui.add(egui::Slider::new(&mut t.overtones_ring, 0.0..=1.0));
                        if r7.changed() { t_changed = true; }
                        if r7.double_clicked() { t.overtones_ring = 0.50; t_changed = true; }
                        ui.end_row();

                        ui.label("Shell Wood Cavity:")
                            .on_hover_text("Wooden tom shell air-cavity mode & < 1.5 ms 2-tap micro-reflection body");
                        let r8 = ui.add(egui::Slider::new(&mut t.shell_wood_cavity, 0.0..=1.0));
                        if r8.changed() { t_changed = true; }
                        if r8.double_clicked() { t.shell_wood_cavity = 0.58; t_changed = true; }
                        ui.end_row();

                        ui.label("Hit Variation (Vel & Strike):")
                            .on_hover_text("Logarithmic humanization of hit velocity & strike position (0.0 = exact/identical, 1.0 = heavy variation)");
                        let r9 = ui.add(
                            egui::Slider::new(&mut t.hit_variation, 0.0..=1.0)
                                .logarithmic(true)
                                .smallest_positive(0.005),
                        );
                        if r9.changed() { t_changed = true; }
                        if r9.double_clicked() { t.hit_variation = 0.10; t_changed = true; }
                        ui.end_row();
                    });

                if t_changed {
                    app.synth_lab.preset.drum_params.toms = *t;
                    app.set_part_synth_preset(target_idx, app.synth_lab.preset.clone());
                    commands.push(SynthCommand::UpdateDrumParams {
                        drum_role: DrumRole::Toms,
                        params: DrumVoiceParams::Toms(*t),
                    });
                }
            });
        }
        DrumRole::HiHats => {
            ui.group(|ui| {
                ui.label(egui::RichText::new("Hi-Hats (GM 42, 44, 46 - Closed/Pedal/Open):").strong());
                let mut h_changed = false;
                let h = &mut dp.hi_hats;

                egui::Grid::new("hihat_params_grid")
                    .num_columns(2)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Closed Hat Volume:").strong())
                            .on_hover_text("Absolute output level for Closed Hi-Hat (GM 42). -48 dB = Mute, double-click resets to 0 dB.");
                        let rv1 = ui.add(
                            egui::Slider::new(&mut h.closed_volume_db, -48.0..=12.0)
                                .suffix(" dB")
                                .fixed_decimals(1)
                                .clamping(egui::SliderClamping::Always),
                        );
                        if rv1.changed() { h_changed = true; }
                        if rv1.double_clicked() { h.closed_volume_db = 0.0; h_changed = true; }
                        ui.end_row();

                        ui.label(egui::RichText::new("Pedal Hat Volume:").strong())
                            .on_hover_text("Absolute output level for Pedal Hi-Hat (GM 44). -48 dB = Mute, double-click resets to 0 dB.");
                        let rv2 = ui.add(
                            egui::Slider::new(&mut h.pedal_volume_db, -48.0..=12.0)
                                .suffix(" dB")
                                .fixed_decimals(1)
                                .clamping(egui::SliderClamping::Always),
                        );
                        if rv2.changed() { h_changed = true; }
                        if rv2.double_clicked() { h.pedal_volume_db = 0.0; h_changed = true; }
                        ui.end_row();

                        ui.label(egui::RichText::new("Open Hat Volume:").strong())
                            .on_hover_text("Absolute output level for Open Hi-Hat (GM 46). -48 dB = Mute, double-click resets to 0 dB.");
                        let rv3 = ui.add(
                            egui::Slider::new(&mut h.open_volume_db, -48.0..=12.0)
                                .suffix(" dB")
                                .fixed_decimals(1)
                                .clamping(egui::SliderClamping::Always),
                        );
                        if rv3.changed() { h_changed = true; }
                        if rv3.double_clicked() { h.open_volume_db = 0.0; h_changed = true; }
                        ui.end_row();

                        ui.label("Metallic Matrix Spread:");
                        let r1 = ui.add(egui::Slider::new(&mut h.spread, 0.5..=2.0));
                        if r1.changed() { h_changed = true; }
                        ui.end_row();

                        ui.label("Highpass Cutoff:");
                        let r2 = ui.add(egui::Slider::new(&mut h.highpass_cutoff_hz, 3000.0..=12000.0).suffix(" Hz"));
                        if r2.changed() { h_changed = true; }
                        ui.end_row();

                        ui.label("Closed Hat Decay:");
                        let r3 = ui.add(egui::Slider::new(&mut h.closed_decay_ms, 15.0..=150.0).suffix(" ms"));
                        if r3.changed() { h_changed = true; }
                        ui.end_row();

                        ui.label("Pedal Hat Decay:");
                        let r4 = ui.add(egui::Slider::new(&mut h.pedal_decay_ms, 20.0..=200.0).suffix(" ms"));
                        if r4.changed() { h_changed = true; }
                        ui.end_row();

                        ui.label("Open Hat Decay:");
                        let r5 = ui.add(egui::Slider::new(&mut h.open_decay_ms, 100.0..=2000.0).suffix(" ms"));
                        if r5.changed() { h_changed = true; }
                        if r5.double_clicked() { h.open_decay_ms = 650.0; h_changed = true; }
                        ui.end_row();

                        ui.label("Bronze Size / Pitch:")
                            .on_hover_text("Scales B20 bronze waveguide delay lengths and modal tuning (0.5 = large/dark, 2.0 = small/bright)");
                        let r6 = ui.add(egui::Slider::new(&mut h.bronze_size_pitch, 0.5..=2.0));
                        if r6.changed() { h_changed = true; }
                        if r6.double_clicked() { h.bronze_size_pitch = 1.0; h_changed = true; }
                        ui.end_row();

                        ui.label("Plate Dispersion (Stick Ping):")
                            .on_hover_text("1st-order all-pass plate dispersion depth inside the 4-line Hadamard waveguide mesh");
                        let r7 = ui.add(egui::Slider::new(&mut h.plate_dispersion, 0.0..=1.0));
                        if r7.changed() { h_changed = true; }
                        if r7.double_clicked() { h.plate_dispersion = 0.48; h_changed = true; }
                        ui.end_row();

                        ui.label("Non-Linear Shimmer Bloom:")
                            .on_hover_text("Signal-modulated all-pass dispersion energy cascade into 4–15 kHz bronze shimmer");
                        let r8 = ui.add(egui::Slider::new(&mut h.nonlin_shimmer, 0.0..=1.0));
                        if r8.changed() { h_changed = true; }
                        if r8.double_clicked() { h.nonlin_shimmer = 0.42; h_changed = true; }
                        ui.end_row();

                        ui.label("Bell Level:")
                            .on_hover_text("High-Q cast-bronze bell / cup modal resonance emphasis");
                        let r9 = ui.add(egui::Slider::new(&mut h.bell_level, 0.0..=1.0));
                        if r9.changed() { h_changed = true; }
                        if r9.double_clicked() { h.bell_level = 0.35; h_changed = true; }
                        ui.end_row();

                        ui.label("Stick Hardness:")
                            .on_hover_text("Stick tip lowpass brightness (soft wood tip vs. bright nylon tip)");
                        let r10 = ui.add(egui::Slider::new(&mut h.stick_hardness, 0.0..=1.0));
                        if r10.changed() { h_changed = true; }
                        if r10.double_clicked() { h.stick_hardness = 0.68; h_changed = true; }
                        ui.end_row();

                        ui.label("Hit Variation (Vel & Strike):")
                            .on_hover_text("Logarithmic humanization of hit velocity & stick strike character (0.0 = exact/identical, 1.0 = heavy variation)");
                        let r11 = ui.add(
                            egui::Slider::new(&mut h.hit_variation, 0.0..=1.0)
                                .logarithmic(true)
                                .smallest_positive(0.005),
                        );
                        if r11.changed() { h_changed = true; }
                        if r11.double_clicked() { h.hit_variation = 0.10; h_changed = true; }
                        ui.end_row();
                    });

                if h_changed {
                    app.synth_lab.preset.drum_params.hi_hats = *h;
                    app.set_part_synth_preset(target_idx, app.synth_lab.preset.clone());
                    commands.push(SynthCommand::UpdateDrumParams {
                        drum_role: DrumRole::HiHats,
                        params: DrumVoiceParams::HiHats(*h),
                    });
                }
            });
        }
        DrumRole::Cymbals => {
            ui.group(|ui| {
                ui.label(egui::RichText::new("Crash & Ride Cymbals (GM 49, 51, 52, 53, 55, 57, 59):").strong());
                let mut c_changed = false;
                let c = &mut dp.cymbals;

                egui::Grid::new("cymbals_params_grid")
                    .num_columns(2)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Crash / Splash / China Volume:").strong())
                            .on_hover_text("Absolute output level for Crash, Splash & China cymbals (GM 49, 52, 55, 57). -48 dB = Mute, double-click resets to 0 dB.");
                        let rv1 = ui.add(
                            egui::Slider::new(&mut c.crash_volume_db, -48.0..=12.0)
                                .suffix(" dB")
                                .fixed_decimals(1)
                                .clamping(egui::SliderClamping::Always),
                        );
                        if rv1.changed() { c_changed = true; }
                        if rv1.double_clicked() { c.crash_volume_db = 0.0; c_changed = true; }
                        ui.end_row();

                        ui.label(egui::RichText::new("Ride Bow Volume:").strong())
                            .on_hover_text("Absolute output level for Ride Cymbal bow (GM 51, 59). -48 dB = Mute, double-click resets to 0 dB.");
                        let rv2 = ui.add(
                            egui::Slider::new(&mut c.ride_volume_db, -48.0..=12.0)
                                .suffix(" dB")
                                .fixed_decimals(1)
                                .clamping(egui::SliderClamping::Always),
                        );
                        if rv2.changed() { c_changed = true; }
                        if rv2.double_clicked() { c.ride_volume_db = 0.0; c_changed = true; }
                        ui.end_row();

                        ui.label(egui::RichText::new("Ride Bell Volume:").strong())
                            .on_hover_text("Absolute output level for Ride Bell (GM 53). -48 dB = Mute, double-click resets to 0 dB.");
                        let rv3 = ui.add(
                            egui::Slider::new(&mut c.bell_volume_db, -48.0..=12.0)
                                .suffix(" dB")
                                .fixed_decimals(1)
                                .clamping(egui::SliderClamping::Always),
                        );
                        if rv3.changed() { c_changed = true; }
                        if rv3.double_clicked() { c.bell_volume_db = 0.0; c_changed = true; }
                        ui.end_row();

                        ui.label("Shimmer Matrix Spread:");
                        let r1 = ui.add(egui::Slider::new(&mut c.matrix_spread, 0.5..=2.0));
                        if r1.changed() { c_changed = true; }
                        ui.end_row();

                        ui.label("Bandpass/Highpass Cutoff:");
                        let r2 = ui.add(egui::Slider::new(&mut c.cutoff_hz, 2000.0..=8000.0).suffix(" Hz"));
                        if r2.changed() { c_changed = true; }
                        ui.end_row();

                        ui.label("Bell Level:")
                            .on_hover_text("High-Q cast-bronze bell / cup modal resonance emphasis (accentuated on Ride Bell Note 53)");
                        let r3 = ui.add(egui::Slider::new(&mut c.bell_peak_level, 0.0..=1.0));
                        if r3.changed() { c_changed = true; }
                        if r3.double_clicked() { c.bell_peak_level = 0.50; c_changed = true; }
                        ui.end_row();

                        ui.label("Crash Cymbal Decay:");
                        let r4 = ui.add(egui::Slider::new(&mut c.crash_decay_ms, 300.0..=3000.0).suffix(" ms"));
                        if r4.changed() { c_changed = true; }
                        if r4.double_clicked() { c.crash_decay_ms = 1200.0; c_changed = true; }
                        ui.end_row();

                        ui.label("Ride Cymbal Decay:");
                        let r5 = ui.add(egui::Slider::new(&mut c.ride_decay_ms, 400.0..=3500.0).suffix(" ms"));
                        if r5.changed() { c_changed = true; }
                        if r5.double_clicked() { c.ride_decay_ms = 1300.0; c_changed = true; }
                        ui.end_row();

                        ui.label("Bronze Size / Pitch:")
                            .on_hover_text("Scales B20 bronze waveguide delay lengths and modal tuning (0.5 = large/dark, 2.0 = small/bright)");
                        let r6 = ui.add(egui::Slider::new(&mut c.bronze_size_pitch, 0.5..=2.0));
                        if r6.changed() { c_changed = true; }
                        if r6.double_clicked() { c.bronze_size_pitch = 1.0; c_changed = true; }
                        ui.end_row();

                        ui.label("Plate Dispersion (Stick Ping):")
                            .on_hover_text("1st-order all-pass plate dispersion depth inside the 4-line Hadamard waveguide mesh");
                        let r7 = ui.add(egui::Slider::new(&mut c.plate_dispersion, 0.0..=1.0));
                        if r7.changed() { c_changed = true; }
                        if r7.double_clicked() { c.plate_dispersion = 0.52; c_changed = true; }
                        ui.end_row();

                        ui.label("Non-Linear Shimmer Bloom:")
                            .on_hover_text("Signal-modulated all-pass dispersion energy cascade into 4–15 kHz cast-bronze shimmer");
                        let r8 = ui.add(egui::Slider::new(&mut c.nonlin_shimmer, 0.0..=1.0));
                        if r8.changed() { c_changed = true; }
                        if r8.double_clicked() { c.nonlin_shimmer = 0.58; c_changed = true; }
                        ui.end_row();

                        ui.label("Stick Hardness:")
                            .on_hover_text("Stick tip lowpass brightness (soft felt/wood tip vs. crisp nylon tip)");
                        let r9 = ui.add(egui::Slider::new(&mut c.stick_hardness, 0.0..=1.0));
                        if r9.changed() { c_changed = true; }
                        if r9.double_clicked() { c.stick_hardness = 0.62; c_changed = true; }
                        ui.end_row();

                        ui.label("Hit Variation (Vel & Strike):")
                            .on_hover_text("Logarithmic humanization of hit velocity & stick strike character (0.0 = exact/identical, 1.0 = heavy variation)");
                        let r10 = ui.add(
                            egui::Slider::new(&mut c.hit_variation, 0.0..=1.0)
                                .logarithmic(true)
                                .smallest_positive(0.005),
                        );
                        if r10.changed() { c_changed = true; }
                        if r10.double_clicked() { c.hit_variation = 0.10; c_changed = true; }
                        ui.end_row();
                    });

                if c_changed {
                    app.synth_lab.preset.drum_params.cymbals = *c;
                    app.set_part_synth_preset(target_idx, app.synth_lab.preset.clone());
                    commands.push(SynthCommand::UpdateDrumParams {
                        drum_role: DrumRole::Cymbals,
                        params: DrumVoiceParams::Cymbals(*c),
                    });
                }
            });
        }
        DrumRole::PercAux => {
            ui.group(|ui| {
                ui.label(egui::RichText::new("Cowbell & Auxiliary Percussion (GM 56):").strong());
                let mut a_changed = false;
                let a = &mut dp.aux;

                egui::Grid::new("aux_params_grid")
                    .num_columns(2)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Cowbell / Aux Volume:").strong())
                            .on_hover_text("Absolute output level for Cowbell & auxiliary percussion (GM 56+). -48 dB = Mute, double-click resets to 0 dB.");
                        let rv = ui.add(
                            egui::Slider::new(&mut a.volume_db, -48.0..=12.0)
                                .suffix(" dB")
                                .fixed_decimals(1)
                                .clamping(egui::SliderClamping::Always),
                        );
                        if rv.changed() { a_changed = true; }
                        if rv.double_clicked() { a.volume_db = 0.0; a_changed = true; }
                        ui.end_row();

                        ui.label("Fundamental Mode 1:");
                        let r1 = ui.add(egui::Slider::new(&mut a.freq1_hz, 300.0..=1000.0).suffix(" Hz"));
                        if r1.changed() { a_changed = true; }
                        ui.end_row();

                        ui.label("Harmonic Mode 2:");
                        let r2 = ui.add(egui::Slider::new(&mut a.freq2_hz, 400.0..=1400.0).suffix(" Hz"));
                        if r2.changed() { a_changed = true; }
                        ui.end_row();

                        ui.label("Detune Offset:");
                        let r3 = ui.add(egui::Slider::new(&mut a.detune, -50.0..=50.0).suffix(" Hz"));
                        if r3.changed() { a_changed = true; }
                        ui.end_row();

                        ui.label("Bandpass Q:");
                        let r4 = ui.add(egui::Slider::new(&mut a.bandpass_q, 0.5..=10.0));
                        if r4.changed() { a_changed = true; }
                        ui.end_row();

                        ui.label("Ring Decay Time:");
                        let r5 = ui.add(egui::Slider::new(&mut a.decay_ms, 20.0..=500.0).suffix(" ms"));
                        if r5.changed() { a_changed = true; }
                        ui.end_row();
                    });

                if a_changed {
                    app.synth_lab.preset.drum_params.aux = *a;
                    app.set_part_synth_preset(target_idx, app.synth_lab.preset.clone());
                    commands.push(SynthCommand::UpdateDrumParams {
                        drum_role: DrumRole::PercAux,
                        params: DrumVoiceParams::PercAux(*a),
                    });
                }
            });
        }
    }
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
