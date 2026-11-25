//! GUI Module for MNX Tablature Editor
pub mod actions;
pub mod clipboard;
pub mod dialogs;
pub mod input;
pub mod panels;
pub mod synth_editor;
#[cfg(test)]
mod tests;

use crate::yaml_editor::*;
use eframe::egui::{self, include_image};

use crate::mnx::MnxDocument;
use crate::render;
use crate::validate;
use std::fs;

const WHOLE_PNG: &[u8] = include_bytes!("../../assets/whole.png");
const HALF_PNG: &[u8] = include_bytes!("../../assets/half.png");
const QUARTER_PNG: &[u8] = include_bytes!("../../assets/quarter.png");
const EIGHTH_PNG: &[u8] = include_bytes!("../../assets/eighth.png");
const SIXTEENTH_PNG: &[u8] = include_bytes!("../../assets/16th.png");
const THIRTYSECOND_PNG: &[u8] = include_bytes!("../../assets/32nd.png");
const DOT_PNG: &[u8] = include_bytes!("../../assets/dot.png");

pub struct DurationTextures {
    pub whole: egui::TextureHandle,
    pub half: egui::TextureHandle,
    pub quarter: egui::TextureHandle,
    pub eighth: egui::TextureHandle,
    pub sixteenth: egui::TextureHandle,
    pub thirty_second: egui::TextureHandle,
    pub dot: egui::TextureHandle,
}

fn load_texture_from_bytes(ctx: &egui::Context, name: &str, bytes: &[u8]) -> egui::TextureHandle {
    let img = image::load_from_memory(bytes).expect("Failed to decode embedded png icon");
    let rgba = img.to_rgba8();
    let (orig_w, orig_h) = (rgba.width(), rgba.height());
    let max_dim = orig_w.max(orig_h).max(200);

    let mut square_img = image::RgbaImage::new(max_dim, max_dim);
    let offset_x = (max_dim - orig_w) / 2;
    let offset_y = (max_dim - orig_h) / 2;

    for y in 0..orig_h {
        for x in 0..orig_w {
            let pixel = rgba.get_pixel(x, y);
            // Convert to white alpha mask so tinting works adaptively in both light and dark mode
            let alpha = pixel[3];
            let white_pixel = image::Rgba([255, 255, 255, alpha]);
            square_img.put_pixel(offset_x + x, offset_y + y, white_pixel);
        }
    }

    let size = [max_dim as usize, max_dim as usize];
    let pixels = square_img.into_raw();
    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
    ctx.load_texture(name, color_image, egui::TextureOptions::LINEAR)
}

impl DurationTextures {
    pub fn load(ctx: &egui::Context) -> Self {
        Self {
            whole: load_texture_from_bytes(ctx, "icon_whole", WHOLE_PNG),
            half: load_texture_from_bytes(ctx, "icon_half", HALF_PNG),
            quarter: load_texture_from_bytes(ctx, "icon_quarter", QUARTER_PNG),
            eighth: load_texture_from_bytes(ctx, "icon_eighth", EIGHTH_PNG),
            sixteenth: load_texture_from_bytes(ctx, "icon_sixteenth", SIXTEENTH_PNG),
            thirty_second: load_texture_from_bytes(ctx, "icon_thirty_second", THIRTYSECOND_PNG),
            dot: load_texture_from_bytes(ctx, "icon_dot", DOT_PNG),
        }
    }
}

/// Maximum number of undo/redo states retained in memory.
pub const MAX_UNDO_HISTORY: usize = 150;

/// In-memory undo/redo snapshot storing the typed [`MnxDocument`] AST as the primary source of
/// truth (falling back to reference-counted YAML text only when snapshotting unparseable text).
#[derive(Clone, Debug)]
pub(crate) struct UndoSnapshot {
    pub doc: Option<MnxDocument>,
    pub yaml_fallback: Option<std::sync::Arc<str>>,
}

/// Per-part runtime audio/MIDI routing and cached synth state, kept aligned 1:1 with `MnxDocument::parts`.
#[derive(Clone, Debug, Default)]
pub struct PartRuntimeState {
    pub midi_port: Option<usize>,
    pub string_channel_map: std::collections::HashMap<u8, u8>,
    pub mpe_mode: Option<bool>,
    pub pitch_bend_range: Option<u8>,
    pub synth_preset: Option<crate::synth::SynthPreset>,
}

pub struct MnxEditorApp {
    pub needs_scroll: bool,
    pub(crate) yaml_text: String,
    pub(crate) saved_yaml: String,
    pub(crate) saved_doc: Option<MnxDocument>,
    pub(crate) yaml_dirty: bool,
    pub(crate) yaml_edit_session_active: bool,
    pub(crate) interactive_gesture_active: bool,
    pub(crate) live_play_dirty: bool,
    pub(crate) last_live_play_just_play: bool,
    pub(crate) last_live_play_controller_count: usize,
    pub(crate) parsed_mnx: Option<MnxDocument>,
    pub(crate) parsed_errors: Vec<validate::ValidationError>,
    pub(crate) validation_errors: Vec<String>,
    pub(crate) current_file_path: Option<std::path::PathBuf>, // Tracks the active file
    pub(crate) selected_part_idx: usize,                      // Active instrument/part index in doc.parts
    pub(crate) selected_event_idx: Option<usize>,             // Selected note/beat index within active part
    pub(crate) selected_tempo: Option<(usize, usize)>,        // Selected (measure_idx, tempo_idx) in continuous mode
    pub(crate) selection_anchor: Option<usize>,               // Anchor for shift-select
    pub(crate) selected_note_idx_in_event: Option<usize>, // Selected sub-note within chord/event (None = all)
    pub(crate) is_editing_fret: bool,                     // In-place fret number editing mode
    pub(crate) editing_fret_buf: String,                  // Text buffer for the new fret number
    pub(crate) editing_string: Option<u8>, // Currently focused string row in edit mode
    pub(crate) pending_slur: Option<(usize, u8)>, // (start_event_idx, string) waiting for note entry
    pub(crate) duration_textures: Option<DurationTextures>, // Note duration icons
    pub(crate) bpm_buf: String,                   // Text buffer for BPM input
    pub(crate) midi_ports: Vec<crate::midi::MidiPortInfo>, // Available MIDI output devices
    pub(crate) selected_midi_port: Option<usize>, // Selected MIDI port id
    pub(crate) pitch_bend_range: u8,              // Pitchbend range (1-48, default 2)
    pub(crate) string_channel_map: std::collections::HashMap<u8, u8>, // String (1..n) -> MIDI channel (1..16)
    pub(crate) parts_runtime: Vec<PartRuntimeState>, // Per-part runtime MIDI/synth settings
    pub(crate) string_channel_dialog_part_idx: usize, // Selected part in String Channels popup
    pub(crate) is_mpe_mode: bool, // Toggle button for multi-timbral vs MPE output
    pub(crate) midi_player: crate::midi::MidiPlayer, // Real-time MIDI sequencer playback engine
    pub(crate) midi_status_msg: Option<String>, // Status or error message from MIDI playback
    pub active_dialog: ActiveDialog, // Active modal dialog
    pub(crate) instrument_setup_name_buf: String, // Name buffer for InstrumentSetup dialog
    pub(crate) instrument_setup_is_percussion: bool, // True when configuring a drum/percussion part
    pub(crate) instrument_setup_percussion_lines: Vec<crate::presets::PercussionLineConfig>, // Per-line config in InstrumentSetup
    pub(crate) instrument_setup_new_sym_bufs: Vec<String>, // Temp text buffers for adding custom symbol overrides
    pub(crate) is_adding_instrument_in_dialog: bool, // True when InstrumentSetup is adding a new part
    pub(crate) show_string_channels_popup: bool, // Toggle state for String Channels dropdown panel
    pub(crate) show_grid_controllers_popup: bool, // Toggle state for Grid Controllers dropdown panel
    pub(crate) undo_stack: std::collections::VecDeque<UndoSnapshot>,
    pub(crate) redo_stack: std::collections::VecDeque<UndoSnapshot>,
    pub(crate) clipboard: Option<ClipboardData>,
    pub is_editing_lyric: bool,
    pub editing_lyric_buf: String,
    pub view_mode: ViewMode,
    pub layout_view_tab: LayoutViewTab,
    pub ensemble_visible_parts: Vec<bool>,
    pub show_yaml_panel: bool,
    pub selected_layout_element_id: Option<String>,
    pub selected_layout_element_ids: std::collections::HashSet<String>,
    pub selected_layout_measures: std::collections::BTreeSet<usize>,
    pub is_dragging_layout_element: bool,
    pub(crate) cached_event_ranges: std::sync::Arc<[crate::yaml_editor::EventRange]>,
    pub(crate) current_grid: String,
    pub(crate) instrument_presets: Vec<crate::presets::InstrumentPreset>,
    pub(crate) drum_kit_presets: Vec<crate::presets::DrumKitPreset>,
    pub(crate) preset_status_msg: Option<String>,
    pub(crate) midi_input_ports: Vec<crate::midi::MidiPortInfo>,
    pub(crate) grid_input: crate::grid_input::GridInputManager,
    pub(crate) active_grid_live_notes:
        std::collections::HashMap<(u8, i32), (midly::num::u4, midly::num::u7)>,
    /// Stack of currently held notes per string for internal synth voice stealing: string (1-based) -> Vec<(fret, freq_hz, velocity)>
    pub(crate) active_grid_synth_notes:
        std::collections::HashMap<u8, Vec<(i32, f64, f32)>>,
    pub(crate) last_synced_part_idx: Option<usize>,
    pub(crate) use_internal_synth: bool,
    pub(crate) use_external_midi: bool,
    pub(crate) synth_engine: Option<crate::synth::SynthEngine>,
    pub(crate) synth_lab: crate::synth::SynthTestLabState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Continuous,
    Layout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayoutViewTab {
    #[default]
    Ensemble,
    Part(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscardTarget {
    NewProject,
    OpenFile,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CopiedMeasureAttrs {
    pub time: Option<crate::mnx::TimeSignature>,
    pub repeat_start: Option<crate::mnx::RepeatStart>,
    pub repeat_end: Option<crate::mnx::RepeatEnd>,
    pub barline: Option<crate::mnx::Barline>,
    pub ending: Option<crate::mnx::Ending>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClipboardData {
    pub items: Vec<crate::mnx::SequenceItem>,
    pub time_signature: Option<crate::mnx::TimeSignature>,
    pub arpeggios: Vec<crate::mnx::Arpeggio>,
    pub tempos: Vec<crate::mnx::Tempo>,
    pub tempo_changes: Vec<crate::mnx::TempoChange>,
    pub measure_attrs: Vec<CopiedMeasureAttrs>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InstrumentTuningMode {
    #[default]
    Edo,
    RatioCents,
}

/// Shared form state for the Instrument Setup dialog and its preset sub-dialogs.
#[derive(Debug, Clone, PartialEq)]
pub struct InstrumentSetupForm {
    pub num_strings: u8,
    pub edo: u8,
    pub step_multiplier: u8,
    pub tuning_mode: InstrumentTuningMode,
    /// In EDO mode: step name; in RatioCents mode: ratio or cents (e.g. `"4/3"` or `"498.0"`).
    pub tunings_text: Vec<String>,
    /// 53-EDO recommended or user-specified ups/downs name (e.g. `"G"`).
    pub tunings_name: Vec<String>,
    pub tunings_octave: Vec<i32>,
    pub show_custom_fretboard: bool,
    pub fretboard_editor: crate::fretboard_editor::FretboardEditorState,
    pub is_new_tab: bool,
}

impl InstrumentSetupForm {
    pub fn default_12edo_6_string(
        ref_step: String,
        ref_oct: i32,
        ref_freq: f64,
        is_new_tab: bool,
    ) -> Self {
        let mut tunings_text = Vec::with_capacity(15);
        let mut tunings_name = Vec::with_capacity(15);
        let mut tunings_octave = Vec::with_capacity(15);
        for &(step, octave) in &crate::mnx::STANDARD_15_STRING_TUNING {
            tunings_text.push(step.to_string());
            tunings_name.push(step.to_string());
            tunings_octave.push(octave);
        }
        Self {
            num_strings: 6,
            edo: 12,
            step_multiplier: 1,
            tuning_mode: InstrumentTuningMode::Edo,
            tunings_text,
            tunings_name,
            tunings_octave,
            show_custom_fretboard: false,
            fretboard_editor: crate::fretboard_editor::FretboardEditorState::new(
                6,
                Vec::new(),
                ref_step,
                ref_oct,
                ref_freq,
            ),
            is_new_tab,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActiveDialog {
    None,
    StartupPrompt,
    Manual,
    About,
    InsertTimeSignature {
        count: u32,
        unit: u32,
        m_idx: usize,
        warning: Option<String>,
    },
    InsertTempo {
        bpm_str: String,
        duration_value: String,
        dots: u32,
        event_idx: usize,
        existing_tempo: Option<(usize, usize)>,
    },
    Message {
        title: String,
        text: String,
    },
    ConfirmDiscardChanges {
        target: DiscardTarget,
    },
    ConfirmRemoveInstrument {
        part_idx: usize,
    },
    ScoreInfo {
        title: String,
        subtitle: String,
        composer: String,
    },
    InsertTempoChange {
        event_idx: usize,
        ending_tempo: String,
        duration_beats: String,
        curve_editor: crate::curve_editor::MonotonicCurveEditor,
        existing_target_id: Option<String>,
    },

    CreateTuplet {
        inner_multiple: u32,
        outer_multiple: u32,
        base: String,
        from_select_mode: bool,
        event_idx: usize,
    },
    ConfirmRemoveTuplet {
        event_idx: usize,
    },
    InstrumentSetup(Box<InstrumentSetupForm>),
    SaveInstrumentPreset {
        preset_name: String,
        error_msg: Option<String>,
        form: Box<InstrumentSetupForm>,
    },
    SaveDrumKitPreset {
        preset_name: String,
        error_msg: Option<String>,
        form: Box<InstrumentSetupForm>,
    },
    ConfirmEdoChange {
        new_num_strings: u8,
        new_edo: u8,
        new_step_multiplier: u8,
        new_tunings: Vec<crate::mnx::StringTuning>,
    },
    ConfirmTuningChange {
        new_num_strings: u8,
        new_edo: u8,
        new_step_multiplier: u8,
        new_tunings: Vec<crate::mnx::StringTuning>,
    },
    BendEditor {
        event_idx: usize,
        string: u8,
        curve_editor: crate::curve_editor::MonotonicCurveEditor,
        bend_range_steps: u32,
    },
    NaturalHarmonicPrompt {
        event_idx: usize,
        string: u8,
        touch_buf: String,
    },
    InsertEnding {
        start_m: usize,
        end_m: usize,
        times_str: String,
    },
    LyricEditor {
        event_idx: usize,
        line_idx: usize,
        syllable_buf: String,
        in_word: bool,
    },
    InsertTextMarking {
        event_idx: usize,
        text_buf: String,
        bold: bool,
        italic: bool,
        underline: bool,
        font_family: String,
        font_size: f32,
        has_existing: bool,
    },
}

impl Default for MnxEditorApp {
    fn default() -> Self {
        Self::new_with_optional_example(fs::read_to_string("examples/example.yaml").ok())
    }
}

impl MnxEditorApp {
    pub fn default_empty_yaml() -> String {
        let tunings: Vec<crate::mnx::StringTuning> = crate::mnx::STANDARD_15_STRING_TUNING[..6]
            .iter()
            .enumerate()
            .map(|(i, &(step, octave))| crate::mnx::StringTuning {
                string: (i + 1) as u8,
                pitch: crate::mnx::Pitch {
                    step: step.to_string(),
                    octave,
                    alter: None,
                    ..Default::default()
                },
            })
            .collect();

        let doc = MnxDocument {
            mnx: crate::mnx::MnxMeta { version: 1 },
            comment: None,
            ext: crate::mnx::DocumentExt::default(),
            global: crate::mnx::Global {
                measures: vec![crate::mnx::GlobalMeasure {
                    time: Some(crate::mnx::TimeSignature { count: 4, unit: 4 }),
                    ..Default::default()
                }],
                ..Default::default()
            },
            parts: vec![crate::mnx::Part {
                id: None,
                name: Some("Guitar".to_string()),
                short_name: None,
                comment: None,
                kit: None,
                ext: crate::mnx::PartExt {
                    micro_fret_divisions: Some(12),
                    fret_step_multiplier: Some(1),
                    pitch_bend_range: None,
                    capo: None,
                    tuning: Some(tunings),
                    ..Default::default()
                },
                measures: vec![crate::mnx::Measure {
                    comment: None,
                    beams: None,
                    arpeggios: None,
                    sequences: vec![crate::mnx::Sequence {
                        comment: None,
                        content: vec![crate::mnx::SequenceItem::Event(crate::mnx::Event {
                            markings: None,
                            id: None,
                            duration: crate::mnx::Duration {
                                base: "quarter".to_string(),
                                dots: None,
                                multiple: None,
                            },
                            notes: Vec::new(),
                            kit_notes: Vec::new(),
                            rest: Some(crate::mnx::Rest {}),
                            slurs: None,
                            lyrics: None,
                            comment: None,
                            ext: crate::mnx::EventExt::default(),
                        })],
                    }],
                }],
            }],
        };

        serde_saphyr::to_string(&doc).unwrap_or_default()
    }

    pub fn new_with_optional_example(loaded_example: Option<String>) -> Self {
        let (initial_yaml, is_example_found) = match loaded_example {
            Some(content) => (content, true),
            None => (Self::default_empty_yaml(), false),
        };

        let ports = if cfg!(test) {
            Vec::new()
        } else {
            crate::midi::get_midi_output_ports()
        };
        let input_ports = if cfg!(test) {
            Vec::new()
        } else {
            crate::grid_input::get_midi_input_ports()
        };
        let selected_port = ports.first().map(|p| p.id);

        let mut app = Self {
            yaml_text: initial_yaml.clone(),
            saved_yaml: initial_yaml,
            saved_doc: None,
            yaml_dirty: false,
            yaml_edit_session_active: false,
            interactive_gesture_active: false,
            live_play_dirty: true,
            last_live_play_just_play: false,
            last_live_play_controller_count: 0,
            parsed_mnx: None,
            parsed_errors: Vec::new(),
            needs_scroll: true,
            validation_errors: Vec::new(),
            current_file_path: None,
            selected_part_idx: 0,
            selected_event_idx: None,
            selected_tempo: None,
            selection_anchor: None,
            selected_note_idx_in_event: None,
            is_editing_fret: false,
            editing_fret_buf: String::new(),
            editing_string: None,
            pending_slur: None,
            duration_textures: None,
            bpm_buf: String::from("120"),
            midi_ports: ports,
            selected_midi_port: selected_port,
            pitch_bend_range: 2,
            string_channel_map: std::collections::HashMap::new(),
            parts_runtime: Vec::new(),
            string_channel_dialog_part_idx: 0,
            is_mpe_mode: false,
            midi_player: crate::midi::MidiPlayer::new(),
            midi_status_msg: None,
            active_dialog: if is_example_found {
                ActiveDialog::None
            } else {
                ActiveDialog::StartupPrompt
            },
            instrument_setup_name_buf: String::from("Guitar"),
            instrument_setup_is_percussion: false,
            instrument_setup_percussion_lines: crate::presets::standard_6_line_kit_configs(),
            instrument_setup_new_sym_bufs: vec![String::new(); 15],
            is_adding_instrument_in_dialog: false,
            show_string_channels_popup: false,
            show_grid_controllers_popup: false,
            undo_stack: std::collections::VecDeque::new(),
            redo_stack: std::collections::VecDeque::new(),
            clipboard: None,
            is_editing_lyric: false,
            editing_lyric_buf: String::new(),
            view_mode: ViewMode::Continuous,
            layout_view_tab: LayoutViewTab::Ensemble,
            ensemble_visible_parts: vec![true],
            show_yaml_panel: true,
            selected_layout_element_id: None,
            selected_layout_element_ids: std::collections::HashSet::new(),
            selected_layout_measures: std::collections::BTreeSet::new(),
            is_dragging_layout_element: false,
            cached_event_ranges: std::sync::Arc::from(&[][..]),
            current_grid: String::from("quarter"),
            instrument_presets: crate::presets::load_presets_with_user_config(),
            drum_kit_presets: crate::presets::load_drum_presets_with_user_config(),
            preset_status_msg: None,
            midi_input_ports: input_ports,
            grid_input: crate::grid_input::GridInputManager::default(),
            active_grid_live_notes: std::collections::HashMap::new(),
            active_grid_synth_notes: std::collections::HashMap::new(),
            last_synced_part_idx: None,
            use_internal_synth: true,
            use_external_midi: true,
            synth_engine: None,
            synth_lab: crate::synth::SynthTestLabState::default(),
        };

        let conn_arc = app.midi_player.connections_arc();
        if let Ok(mut state) = app.grid_input.live_play_state.lock() {
            state.connections = Some(conn_arc);
        }
        app.sync_live_play_config();

        app.process_and_render();
        app.saved_yaml = app.yaml_text.clone();
        app.saved_doc = app.parsed_mnx.clone();
        app.undo_stack.clear();
        app.redo_stack.clear();
        app
    }
}

impl MnxEditorApp {
    pub(crate) fn ensure_parts_runtime_len(&mut self, min_len: usize) {
        if self.parts_runtime.len() < min_len {
            self.parts_runtime
                .resize_with(min_len, PartRuntimeState::default);
        }
    }

    pub(crate) fn part_runtime(&self, part_idx: usize) -> Option<&PartRuntimeState> {
        self.parts_runtime.get(part_idx)
    }

    pub(crate) fn part_runtime_mut(&mut self, part_idx: usize) -> &mut PartRuntimeState {
        self.ensure_parts_runtime_len(part_idx + 1);
        &mut self.parts_runtime[part_idx]
    }

    pub fn get_part_synth_preset(&self, part_idx: usize) -> crate::synth::SynthPreset {
        if let Some(doc) = &self.parsed_mnx {
            if let Some(part) = doc.parts.get(part_idx) {
                if let Some(preset) = &part.ext.synth_preset {
                    return preset.clone();
                }
            }
        }
        if let Some(preset) = self
            .part_runtime(part_idx)
            .and_then(|rt| rt.synth_preset.as_ref())
        {
            preset.clone()
        } else {
            let is_perc = self
                .parsed_mnx
                .as_ref()
                .and_then(|doc| doc.parts.get(part_idx))
                .map(|p| p.is_percussion())
                .unwrap_or(false);
            if is_perc {
                crate::synth::SynthPreset::linn_808_kit()
            } else {
                crate::synth::SynthPreset::acoustic_dreadnought()
            }
        }
    }

    pub fn set_part_synth_preset(&mut self, part_idx: usize, preset: crate::synth::SynthPreset) {
        self.part_runtime_mut(part_idx).synth_preset = Some(preset.clone());
        self.mutate_doc(|doc| {
            let part = doc.parts.get_mut(part_idx)?;
            part.ext.synth_preset = Some(preset.clone());
            Some(())
        });
        self.sync_part_to_synth_engine(part_idx, &preset);
    }

    pub fn sync_part_to_synth_engine(
        &mut self,
        part_idx: usize,
        preset: &crate::synth::SynthPreset,
    ) {
        use crate::synth::command::SynthCommand;
        use crate::synth::patch::DrumRole;

        if let Some(eng) = self.synth_engine.as_mut() {
            let p_u8 = part_idx as u8;
            eng.send(SynthCommand::SetPartEngine {
                part_idx: p_u8,
                engine: preset.engine,
            });
            eng.send(SynthCommand::SetPartProfile {
                part_idx: p_u8,
                profile: preset.body_params.profile_base,
            });
            eng.send(SynthCommand::UpdatePartStringParams {
                part_idx: p_u8,
                params: preset.string_params,
            });
            eng.send(SynthCommand::UpdatePartBodyParams {
                part_idx: p_u8,
                params: preset.body_params,
            });
            eng.send(SynthCommand::UpdatePartFmParams {
                part_idx: p_u8,
                params: preset.fm_params,
            });
            for &role in DrumRole::all() {
                eng.send(SynthCommand::UpdatePartDrumParams {
                    part_idx: p_u8,
                    drum_role: role,
                    params: preset.drum_params.params_for_role(role),
                });
            }
        }
    }

    pub fn sync_all_parts_to_synth_engine(&mut self) {
        let part_count = self
            .parsed_mnx
            .as_ref()
            .map(|d| d.parts.len())
            .unwrap_or(1)
            .max(1);
        for i in 0..part_count {
            let preset = self.get_part_synth_preset(i);
            self.sync_part_to_synth_engine(i, &preset);
        }
        if let Some(eng) = self.synth_engine.as_mut() {
            eng.send(crate::synth::SynthCommand::SetActivePart(
                self.selected_part_idx as u8,
            ));
        }
    }

    pub fn active_part(&self) -> Option<&crate::mnx::Part> {
        let doc = self.parsed_mnx.as_ref()?;
        doc.parts
            .get(self.selected_part_idx)
            .or_else(|| doc.parts.first())
    }

    pub fn select_part(&mut self, part_idx: usize) {
        let part_count = self
            .parsed_mnx
            .as_ref()
            .map(|d| d.parts.len())
            .unwrap_or(1)
            .max(1);
        let new_idx = part_idx.min(part_count - 1);

        // Save current part's MIDI channel map and port before switching
        let cur_idx = self.selected_part_idx;
        let cur_map = self.string_channel_map.clone();
        let cur_port = self.selected_midi_port;
        let cur_mpe = self.is_mpe_mode;
        let cur_pb = self.pitch_bend_range;
        {
            let rt = self.part_runtime_mut(cur_idx);
            rt.string_channel_map = cur_map;
            if let Some(p) = cur_port {
                rt.midi_port = Some(p);
            }
            rt.mpe_mode = Some(cur_mpe);
            rt.pitch_bend_range = Some(cur_pb);
        }

        if self.selected_part_idx != new_idx {
            self.selected_part_idx = new_idx;
            self.string_channel_dialog_part_idx = new_idx;
            self.selected_note_idx_in_event = None;
            self.is_editing_fret = false;
            self.editing_fret_buf.clear();
            self.editing_string = None;
            self.pending_slur = None;
            if let Some(eng) = self.synth_engine.as_mut() {
                eng.send(crate::synth::SynthCommand::SetActivePart(new_idx as u8));
            }
        }

        self.string_channel_map = self
            .part_runtime(self.selected_part_idx)
            .map(|rt| rt.string_channel_map.clone())
            .unwrap_or_default();
        self.selected_midi_port = self.get_part_midi_port(self.selected_part_idx);
        self.pitch_bend_range = self.get_part_pitch_bend_range(self.selected_part_idx);
        self.is_mpe_mode = self.is_part_mpe_mode(self.selected_part_idx);
        self.live_play_dirty = true;

        if self.show_yaml_panel || cfg!(test) {
            self.ensure_yaml_synced();
            self.refresh_cached_event_ranges();
        }
        let total_events = self.active_part_event_count();
        if let Some(ev_idx) = self.selected_event_idx {
            if total_events == 0 {
                self.selected_event_idx = None;
            } else if ev_idx >= total_events {
                self.selected_event_idx = Some(total_events - 1);
            }
        }
        self.selection_anchor = None;
    }

    pub fn active_part_event_count(&self) -> usize {
        if let Some(part) = self.active_part() {
            part.event_count()
        } else {
            self.cached_event_ranges.len()
        }
    }

    pub(crate) fn ensure_global_measure_up_to(doc: &mut MnxDocument, target_m_idx: usize) {
        while doc.global.measures.len() <= target_m_idx {
            if doc.global.measures.is_empty() {
                doc.global.measures.push(crate::mnx::GlobalMeasure {
                    time: Some(crate::mnx::TimeSignature { count: 4, unit: 4 }),
                    ..Default::default()
                });
            } else {
                doc.global
                    .measures
                    .push(crate::mnx::GlobalMeasure::default());
            }
        }
    }

    pub fn refresh_cached_event_ranges(&mut self) {
        self.cached_event_ranges =
            find_event_ranges_for_part(&self.yaml_text, self.selected_part_idx).into();
    }

    /// Flushes any deferred `MnxDocument -> yaml_text` serialization when `yaml_dirty` is set.
    pub fn ensure_yaml_synced(&mut self) {
        if self.yaml_dirty {
            if let Some(doc) = &self.parsed_mnx {
                if let Ok(new_yaml) = serde_saphyr::to_string(doc) {
                    self.yaml_text = new_yaml;
                }
            }
            self.yaml_dirty = false;
            if self.show_yaml_panel || cfg!(test) {
                self.refresh_cached_event_ranges();
            }
        }
    }

    pub fn active_layout_part_indices(&self) -> Vec<usize> {
        let num_parts = self
            .parsed_mnx
            .as_ref()
            .map(|d| d.parts.len())
            .unwrap_or(1)
            .max(1);
        match self.layout_view_tab {
            LayoutViewTab::Part(p_idx) => {
                vec![p_idx.min(num_parts - 1)]
            }
            LayoutViewTab::Ensemble => {
                let mut indices: Vec<usize> = (0..num_parts)
                    .filter(|&i| self.ensemble_visible_parts.get(i).copied().unwrap_or(true))
                    .collect();
                if indices.is_empty() {
                    indices.push(0);
                }
                indices
            }
        }
    }

    pub(crate) fn current_grid_beats(&self) -> f64 {
        match self.current_grid.as_str() {
            "half" => 2.0,
            "quarter" => 1.0,
            "eighth" => 0.5,
            "16th" | "sixteenth" => 0.25,
            _ => 1.0,
        }
    }

    pub(crate) fn current_undo_snapshot(&self) -> UndoSnapshot {
        if let Some(doc) = &self.parsed_mnx {
            UndoSnapshot {
                doc: Some(doc.clone()),
                yaml_fallback: None,
            }
        } else {
            UndoSnapshot {
                doc: None,
                yaml_fallback: Some(std::sync::Arc::from(self.yaml_text.as_str())),
            }
        }
    }

    pub(crate) fn push_undo_snapshot(&mut self, snap: UndoSnapshot) {
        let is_same = self.undo_stack.back().map_or(false, |last| {
            match (&last.doc, &snap.doc) {
                (Some(a), Some(b)) => a == b,
                (None, None) => last.yaml_fallback == snap.yaml_fallback,
                _ => false,
            }
        });
        if !is_same {
            while self.undo_stack.len() >= MAX_UNDO_HISTORY {
                self.undo_stack.pop_front();
            }
            self.undo_stack.push_back(snap);
            self.redo_stack.clear();
        }
    }

    pub fn push_undo_state(&mut self) {
        let snap = self.current_undo_snapshot();
        self.push_undo_snapshot(snap);
    }

    pub fn update_yaml(&mut self, new_yaml: String) {
        self.ensure_yaml_synced();
        if self.yaml_text != new_yaml {
            self.push_undo_state();
            self.yaml_text = new_yaml;
            self.yaml_dirty = false;
        }
    }

    fn restore_undo_snapshot(&mut self, snap: UndoSnapshot) {
        self.interactive_gesture_active = false;
        self.yaml_edit_session_active = false;
        if let Some(doc) = snap.doc {
            self.commit_doc_without_undo(doc);
        } else if let Some(txt) = snap.yaml_fallback {
            self.yaml_text = txt.to_string();
            self.yaml_dirty = false;
            self.process_and_render();
        }
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop_back() {
            let cur = self.current_undo_snapshot();
            while self.redo_stack.len() >= MAX_UNDO_HISTORY {
                self.redo_stack.pop_front();
            }
            self.redo_stack.push_back(cur);
            self.restore_undo_snapshot(prev);
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop_back() {
            let cur = self.current_undo_snapshot();
            while self.undo_stack.len() >= MAX_UNDO_HISTORY {
                self.undo_stack.pop_front();
            }
            self.undo_stack.push_back(cur);
            self.restore_undo_snapshot(next);
        }
    }

    /// Starts a coalesced interactive gesture (e.g. Layout drag or live pad chord roll),
    /// recording at most one undo snapshot for the entire gesture.
    pub fn begin_interactive_gesture(&mut self) {
        if !self.interactive_gesture_active {
            self.push_undo_state();
            self.interactive_gesture_active = true;
        }
    }

    /// Mutates `self.parsed_mnx` directly in memory during a high-frequency gesture (such as
    /// mouse dragging at 60–120 Hz) without re-serializing YAML, scanning ranges, or flooding
    /// `undo_stack`.
    pub fn mutate_doc_interactive<F>(&mut self, f: F) -> bool
    where
        F: FnOnce(&mut MnxDocument) -> bool,
    {
        if self.parsed_mnx.is_none() {
            self.parsed_mnx = serde_saphyr::from_str::<MnxDocument>(&self.yaml_text).ok();
        }
        if self.parsed_mnx.is_none() {
            return false;
        }
        self.begin_interactive_gesture();
        let changed = if let Some(doc) = self.parsed_mnx.as_mut() {
            f(doc)
        } else {
            false
        };
        if changed {
            self.yaml_dirty = true;
            self.live_play_dirty = true;
        }
        changed
    }

    /// Finalizes a coalesced interactive gesture, running normalization and syncing YAML once.
    pub fn end_interactive_gesture(&mut self) {
        if self.interactive_gesture_active {
            self.interactive_gesture_active = false;
            if let Some(mut doc) = self.parsed_mnx.take() {
                doc.normalize(false);
                self.commit_doc_without_undo(doc);
            }
        }
    }

    pub fn parse_measure_from_element_id_prefix(id: &str) -> Option<usize> {
        if let Some(rest) = id.strip_prefix("measure_") {
            return rest.split('_').next()?.parse::<usize>().ok();
        }
        if let Some(rest) = id.strip_prefix("barline_") {
            return rest.split('_').next()?.parse::<usize>().ok();
        }
        if let Some(rest) = id.strip_prefix("sys_barline_") {
            return rest.split('_').next()?.parse::<usize>().ok();
        }
        if let Some(rest) = id.strip_prefix("time_sig_") {
            return rest.split('_').next()?.parse::<usize>().ok();
        }
        if let Some(rest) = id.strip_prefix("ts_") {
            return rest.split('_').next()?.parse::<usize>().ok();
        }
        if let Some(rest) = id.strip_prefix("mn_") {
            return rest.split('_').next()?.parse::<usize>().ok();
        }
        if let Some(rest) = id.strip_prefix("ending_") {
            return rest.split('_').next()?.parse::<usize>().ok();
        }
        if let Some(rest) = id.strip_prefix("tempo_") {
            return rest.split('_').next()?.parse::<usize>().ok();
        }
        if let Some(rest) = id.strip_prefix("tc_") {
            return rest.split('_').next()?.parse::<usize>().ok();
        }
        if let Some(rest) = id.strip_prefix("stem_") {
            return rest.split('_').next()?.parse::<usize>().ok();
        }
        if let Some(rest) = id.strip_prefix("tuplet_") {
            return rest.split('_').next()?.parse::<usize>().ok();
        }
        if let Some(rest) = id.strip_prefix("lyric_hyphen_") {
            return rest.split('_').next()?.parse::<usize>().ok();
        }
        if let Some(rest) = id.strip_prefix("lyric_") {
            return rest.split('_').next()?.parse::<usize>().ok();
        }
        if let Some(rest) = id.strip_prefix("note_") {
            return rest.split('_').next()?.parse::<usize>().ok();
        }
        if let Some(rest) = id.strip_prefix("rest_") {
            return rest.split('_').next()?.parse::<usize>().ok();
        }
        if let Some(rest) = id.strip_prefix("sys_lock_") {
            let mut parts = rest.split('_');
            let _start_m = parts.next()?;
            let end_m = parts.next()?;
            return end_m.parse::<usize>().ok();
        }
        None
    }

    pub fn resolve_layout_element_to_measure(&self, element_id: &str) -> Option<usize> {
        if let Some(m) = Self::parse_measure_from_element_id_prefix(element_id) {
            return Some(m);
        }
        if let Some(doc) = &self.parsed_mnx {
            for part in &doc.parts {
                for (m_idx, m) in part.measures.iter().enumerate() {
                    for seq in &m.sequences {
                        for ev in seq.events() {
                            if ev.id.as_deref() == Some(element_id) {
                                return Some(m_idx);
                            }
                            for note in &ev.notes {
                                if note.id.as_deref() == Some(element_id) {
                                    return Some(m_idx);
                                }
                            }
                        }
                    }
                }
            }
            if let Some(rest) = element_id.strip_prefix("slur_") {
                let parts: Vec<&str> = rest.split('_').collect();
                if let Some(target) = parts.get(1) {
                    if let Some(m) = self.resolve_layout_element_to_measure(target) {
                        return Some(m);
                    }
                }
            }
            if let Some(rest) = element_id.strip_prefix("tie_") {
                let parts: Vec<&str> = rest.split('_').collect();
                if let Some(target) = parts.get(1) {
                    if let Some(m) = self.resolve_layout_element_to_measure(target) {
                        return Some(m);
                    }
                }
            }
        }
        None
    }

    pub fn get_selected_measure_range(&self) -> (usize, usize) {
        if self.view_mode == ViewMode::Layout {
            if !self.selected_layout_measures.is_empty() {
                let start_m = *self.selected_layout_measures.iter().min().unwrap();
                let end_m = *self.selected_layout_measures.iter().max().unwrap();
                return (start_m, end_m);
            }
            if let Some(id) = &self.selected_layout_element_id {
                if let Some(m) = self.resolve_layout_element_to_measure(id) {
                    return (m, m);
                }
            }
            for id in &self.selected_layout_element_ids {
                if let Some(m) = self.resolve_layout_element_to_measure(id) {
                    return (m, m);
                }
            }
        }
        if let Some(doc) = &self.parsed_mnx {
            let flattened = doc.get_flattened_event_paths_for_part(self.selected_part_idx);
            if !flattened.is_empty() {
                let (min_ev, max_ev) = match (self.selected_event_idx, self.selection_anchor) {
                    (Some(curr), Some(anch)) => (curr.min(anch), curr.max(anch)),
                    (Some(curr), None) => (curr, curr),
                    (None, Some(anch)) => (anch, anch),
                    (None, None) => (0, 0),
                };
                let min_ev = min_ev.min(flattened.len().saturating_sub(1));
                let max_ev = max_ev.min(flattened.len().saturating_sub(1));
                let start_m = flattened[min_ev].m_idx();
                let end_m = flattened[max_ev].m_idx();
                return (start_m, end_m);
            }
        }
        (0, 0)
    }

    pub fn get_target_measure_for_layout_or_breaks(&self) -> usize {
        if self.view_mode == ViewMode::Layout {
            let (_, end_m) = self.get_selected_measure_range();
            return end_m;
        }
        self.get_selected_measure_range().0
    }

    pub fn start_midi_playback(&mut self, start_from_selected: bool) {
        self.start_midi_playback_with_options(start_from_selected, false);
    }

    pub fn start_midi_playback_with_options(
        &mut self,
        start_from_selected: bool,
        current_part_only: bool,
    ) {
        if !self.use_internal_synth && (!self.use_external_midi || self.midi_ports.is_empty()) {
            self.midi_status_msg = Some("No audio output enabled. Enable Internal Synth or connect a MIDI output device.".to_string());
            return;
        }

        if self.use_internal_synth {
            let _ = self.ensure_synth_engine();
            self.sync_all_parts_to_synth_engine();
        }
        let synth_tx = if self.use_internal_synth {
            self.synth_engine.as_ref().map(|e| e.sender())
        } else {
            None
        };
        self.midi_player.set_synth_routing(
            synth_tx,
            self.use_external_midi && !self.midi_ports.is_empty(),
        );

        if let Some(doc) = &self.parsed_mnx {

            let mut part_configs = Vec::with_capacity(doc.parts.len());
            let mut port_channel_cursor: std::collections::HashMap<usize, u8> =
                std::collections::HashMap::new();

            for (p_idx, part) in doc.parts.iter().enumerate() {
                let port_idx = self.get_part_midi_port(p_idx).unwrap_or(0);

                if part.is_percussion() {
                    if current_part_only && p_idx != self.selected_part_idx {
                        continue;
                    }
                    let perc_ch = part.percussion_midi_channel().saturating_sub(1).min(15);
                    part_configs.push(crate::midi::PartMidiConfig {
                        part_idx: p_idx,
                        port_index: port_idx,
                        pitch_bend_range: 2,
                        string_to_channel: std::collections::HashMap::new(),
                        is_mpe: false,
                        is_percussion: true,
                        percussion_channel: midly::num::u4::from(perc_ch),
                    });
                    continue;
                }

                let num_strings = part.num_strings() as u8;
                let base_offset = *port_channel_cursor.entry(port_idx).or_insert(0);
                let mut string_to_channel = std::collections::HashMap::new();

                let custom_map = if p_idx == self.selected_part_idx && !self.string_channel_map.is_empty() {
                    Some(&self.string_channel_map)
                } else {
                    self.part_runtime(p_idx)
                        .map(|rt| &rt.string_channel_map)
                        .filter(|m| !m.is_empty())
                };

                let is_mpe = self.is_part_mpe_mode(p_idx);
                let pb_range = self.get_part_pitch_bend_range(p_idx);

                for s in 1..=num_strings {
                    let default_ch = if is_mpe {
                        ((base_offset + s - 1) % 15) + 2
                    } else {
                        ((base_offset + s - 1) % 16) + 1
                    };
                    let ch_1_based = custom_map
                        .and_then(|m| m.get(&s).copied())
                        .unwrap_or(default_ch);
                    let ch_0_based = (ch_1_based.saturating_sub(1)) % 16;
                    string_to_channel.insert(s, midly::num::u4::from(ch_0_based));
                }

                if let Some(cursor) = port_channel_cursor.get_mut(&port_idx) {
                    *cursor = cursor.saturating_add(num_strings);
                }

                if current_part_only && p_idx != self.selected_part_idx {
                    continue;
                }

                part_configs.push(crate::midi::PartMidiConfig {
                    part_idx: p_idx,
                    port_index: port_idx,
                    pitch_bend_range: pb_range,
                    string_to_channel,
                    is_mpe,
                    is_percussion: false,
                    percussion_channel: midly::num::u4::from(9),
                });
            }

            if part_configs.is_empty() {
                self.midi_status_msg = Some("No instrument parts available for playback.".to_string());
                return;
            }

            let start_event = if start_from_selected {
                self.selected_event_idx
            } else {
                Some(0)
            };

            let primary_port = part_configs.first().map(|c| c.port_index).unwrap_or(0);
            let solo_prefix = if current_part_only {
                let pname = doc
                    .parts
                    .get(self.selected_part_idx)
                    .map(|p| p.display_name(self.selected_part_idx))
                    .unwrap_or_else(|| format!("Part {}", self.selected_part_idx + 1));
                format!(" ({} only)", pname)
            } else {
                String::new()
            };
            let port_name = if self.use_external_midi && !self.midi_ports.is_empty() {
                self.midi_ports
                    .iter()
                    .find(|p| p.id == primary_port)
                    .map(|p| p.name.as_str())
                    .unwrap_or("MIDI device")
            } else {
                ""
            };
            let out_desc = match (self.use_internal_synth, self.use_external_midi && !self.midi_ports.is_empty()) {
                (true, true) => format!("Internal Synth + {}", port_name),
                (true, false) => "Internal Synth".to_string(),
                (false, true) => port_name.to_string(),
                (false, false) => "Silent".to_string(),
            };
            match self.midi_player.play_multi_part(
                doc,
                part_configs,
                120.0,
                midly::num::u15::from(480),
                self.selected_part_idx,
                start_event,
            ) {
                Ok(_) => {
                    let mode_str = if self.is_part_mpe_mode(self.selected_part_idx) { " (MPE)" } else { "" };
                    if let Some(start_idx) = start_event {
                        if start_idx > 0 {
                            self.midi_status_msg = Some(format!(
                                "Playing{} from note {}{} via {}",
                                solo_prefix,
                                start_idx + 1,
                                mode_str,
                                out_desc
                            ));
                        } else {
                            self.midi_status_msg = Some(format!(
                                "Playing{} from start{} via {}",
                                solo_prefix, mode_str, out_desc
                            ));
                        }
                    } else {
                        self.midi_status_msg =
                            Some(format!("Playing{}{} via {}", solo_prefix, mode_str, out_desc));
                    }
                }
                Err(e) => {
                    self.midi_status_msg = Some(format!("Playback error: {}", e));
                }
            }
        } else {
            self.midi_status_msg = Some("No valid document parsed to play.".to_string());
        }
    }

    fn part_event_measure_and_beats(part: &crate::mnx::Part) -> Vec<(usize, f64, f64)> {
        fn collect_seq_item_beats(
            items: &[crate::mnx::SequenceItem],
            m_idx: usize,
            scale: f64,
            in_grace: bool,
            cur_beat: &mut f64,
            out: &mut Vec<(usize, f64, f64)>,
        ) {
            for item in items {
                match item {
                    crate::mnx::SequenceItem::Event(ev) => {
                        let dur_beats = if in_grace {
                            0.0
                        } else {
                            crate::validate::duration_to_quarter_beats(&ev.duration) * scale
                        };
                        let start = *cur_beat;
                        let end = start + dur_beats;
                        *cur_beat = end;
                        out.push((m_idx, start, end));
                    }
                    crate::mnx::SequenceItem::Grace(g) => {
                        collect_seq_item_beats(&g.content, m_idx, scale, true, cur_beat, out);
                    }
                    crate::mnx::SequenceItem::Tuplet(t) => {
                        let inner_beats =
                            crate::validate::duration_to_quarter_beats(&t.inner.duration)
                                * (t.inner.multiple as f64);
                        let outer_beats =
                            crate::validate::duration_to_quarter_beats(&t.outer.duration)
                                * (t.outer.multiple as f64);
                        let new_scale = if inner_beats > 0.0 {
                            scale * (outer_beats / inner_beats)
                        } else {
                            scale
                        };
                        collect_seq_item_beats(&t.content, m_idx, new_scale, in_grace, cur_beat, out);
                    }
                }
            }
        }

        let mut out = Vec::new();
        for (m_idx, measure) in part.measures.iter().enumerate() {
            if let Some(seq) = measure.sequences.first() {
                let mut cur_beat = 0.0;
                collect_seq_item_beats(&seq.content, m_idx, 1.0, false, &mut cur_beat, &mut out);
            }
        }
        out
    }

    pub fn find_matching_event_in_part(
        doc: &crate::mnx::MnxDocument,
        from_part_idx: usize,
        from_event_idx: usize,
        to_part_idx: usize,
    ) -> usize {
        let Some(from_part) = doc.parts.get(from_part_idx) else {
            return 0;
        };
        let Some(to_part) = doc.parts.get(to_part_idx) else {
            return 0;
        };
        let from_beats = Self::part_event_measure_and_beats(from_part);
        let to_beats = Self::part_event_measure_and_beats(to_part);
        if to_beats.is_empty() {
            return 0;
        }

        let (from_m_idx, target_beat, _) = from_beats
            .get(from_event_idx)
            .copied()
            .or_else(|| from_beats.last().copied())
            .unwrap_or((0, 0.0, 0.0));
        let target_m_idx = from_m_idx.min(to_part.measures.len().saturating_sub(1));

        // 1. Prefer an event in target_m_idx whose [start_beat, end_beat) span contains target_beat
        let mut best_spanning: Option<(usize, f64)> = None;
        let mut best_closest: Option<(usize, f64)> = None;

        for (ev_idx, &(m_idx, start_beat, end_beat)) in to_beats.iter().enumerate() {
            if m_idx != target_m_idx {
                continue;
            }
            let dist = (start_beat - target_beat).abs();
            if best_closest.map(|(_, d)| dist < d - 1e-6).unwrap_or(true) {
                best_closest = Some((ev_idx, dist));
            }
            if end_beat > start_beat + 1e-6
                && start_beat <= target_beat + 1e-4
                && target_beat < end_beat - 1e-4
            {
                if best_spanning.map(|(_, d)| dist < d - 1e-6).unwrap_or(true) {
                    best_spanning = Some((ev_idx, dist));
                }
            }
        }

        best_spanning
            .or(best_closest)
            .map(|(idx, _)| idx)
            .unwrap_or(0)
    }

    pub fn jump_to_adjacent_part(&mut self, delta: isize) {
        let num_parts = self
            .parsed_mnx
            .as_ref()
            .map(|d| d.parts.len())
            .unwrap_or(0);
        if num_parts <= 1 {
            return;
        }
        let target_part_isize = (self.selected_part_idx as isize) + delta;
        if target_part_isize < 0 || (target_part_isize as usize) >= num_parts {
            return;
        }
        let target_part_idx = target_part_isize as usize;

        let was_editing_fret = self.is_editing_fret;
        let old_editing_string = self.editing_string;
        if was_editing_fret && !self.editing_fret_buf.trim().is_empty() {
            self.commit_fret_edit();
            self.editing_fret_buf.clear();
        }

        let old_part_idx = self.selected_part_idx;
        let mut ref_event_idx = self.selected_event_idx.unwrap_or(0);
        if let Some(old_e_idx) = self.selected_event_idx {
            if let Some(info) = self.find_event_measure_end_info(old_e_idx) {
                if info.is_last_in_measure && info.is_blank {
                    let m_idx = info.measure_idx;
                    let old_total = self.active_part_event_count();
                    let removed = self.mutate_doc(|doc| {
                        if crate::yaml_editor::remove_blank_event_at_end_of_measure_in_doc_part(
                            doc,
                            old_part_idx,
                            m_idx,
                        ) {
                            Some(())
                        } else {
                            None
                        }
                    });
                    let new_total = self.active_part_event_count();
                    if removed.is_some() && new_total < old_total {
                        ref_event_idx = old_e_idx.saturating_sub(1);
                    }
                }
            }
        }

        let target_event_idx = self
            .parsed_mnx
            .as_ref()
            .map(|doc| {
                Self::find_matching_event_in_part(doc, old_part_idx, ref_event_idx, target_part_idx)
            })
            .unwrap_or(0);

        self.select_part(target_part_idx);
        let new_total = self.active_part_event_count();
        if new_total > 0 {
            let clamped_ev = target_event_idx.min(new_total - 1);
            self.selected_event_idx = Some(clamped_ev);
            if was_editing_fret {
                let new_num_strings = self
                    .active_part()
                    .map(|p| p.num_strings() as u8)
                    .unwrap_or(6)
                    .max(1);
                let target_str = old_editing_string.unwrap_or(1).clamp(1, new_num_strings);
                self.is_editing_fret = true;
                self.editing_string = Some(target_str);
                self.editing_fret_buf.clear();
                self.select_note_on_string(clamped_ev, target_str);
            }
            self.preview_selected_sound();
        }
        self.needs_scroll = true;
    }

    /// Ensures the internal real-time `cpal` synth engine is running and returns a mutable reference.
    pub fn ensure_synth_engine(&mut self) -> Option<&mut crate::synth::SynthEngine> {
        if self.synth_engine.is_none() && !cfg!(test) {
            let preferred_dev = self.synth_lab.selected_output_device.clone();
            let preferred_buf = self.synth_lab.selected_buffer_size;
            match crate::synth::SynthEngine::new_with_device_and_buffer(
                preferred_dev.as_deref(),
                preferred_buf,
            ) {
                Ok(eng) => {
                    self.synth_lab.status_msg = Some(format!(
                        "Audio output active: {} ({:.0} Hz, {} ch, {} buffer)",
                        eng.device_name(),
                        eng.sample_rate(),
                        eng.channels(),
                        eng.buffer_size_label()
                    ));
                    self.synth_engine = Some(eng);
                    self.sync_all_parts_to_synth_engine();
                }
                Err(e) => {
                    self.synth_lab.status_msg = Some(format!("Audio init error: {}", e));
                }
            }
        }
        self.synth_engine.as_mut()
    }

    pub fn preview_selected_sound(&mut self) {
        if self.use_internal_synth || self.synth_lab.route_score_preview {
            let _ = self.ensure_synth_engine();
        }

        let e_idx = match self.selected_event_idx {
            Some(i) => i,
            None => return,
        };
        let doc = match &self.parsed_mnx {
            Some(d) => d,
            None => return,
        };
        let part = match doc
            .parts
            .get(self.selected_part_idx)
            .or_else(|| doc.parts.first())
        {
            Some(p) => p,
            None => return,
        };
        let port_idx_opt = self.get_part_midi_port(self.selected_part_idx);
        let is_mpe = self.is_part_mpe_mode(self.selected_part_idx);
        let pb_range = self.get_part_pitch_bend_range(self.selected_part_idx);

        let Some(event) = part.nth_event(e_idx) else {
            return;
        };

        if part.is_percussion() {
            if event.kit_notes.is_empty() {
                self.midi_player.stop();
                if let Some(eng) = self.synth_engine.as_mut() {
                    eng.all_notes_off();
                }
                return;
            }
            let perc_ch = midly::num::u4::from(part.percussion_midi_channel().saturating_sub(1).min(15));
            let target_kns: Vec<&crate::mnx::KitNote> =
                if let Some(n_idx) = self.selected_note_idx_in_event {
                    if let Some(kn) = event.kit_notes.get(n_idx) {
                        vec![kn]
                    } else {
                        event.kit_notes.iter().collect()
                    }
                } else {
                    event.kit_notes.iter().collect()
                };
            let mut notes_to_play = Vec::new();
            let mut synth_drums = Vec::new();
            for kn in target_kns {
                let comp = part.kit.as_ref().and_then(|k| k.get(&kn.kit_component));
                let midi_note = kn
                    .ext
                    .sound_override
                    .unwrap_or_else(|| {
                        comp.map(|c| c.resolve_midi_sound(kn.ext.symbol.as_deref()))
                            .unwrap_or(38)
                    })
                    .min(127);
                let vel = if kn.ext.symbol.as_deref() == Some("g") {
                    50
                } else {
                    100
                };
                synth_drums.push((midi_note, (vel as f32 / 127.0).clamp(0.05, 1.0)));
                notes_to_play.push(crate::midi::NotePreview {
                    channel: perc_ch,
                    key: midly::num::u7::from(midi_note),
                    pitch_bend: None,
                    velocity: midly::num::u7::from(vel),
                });
            }
            if self.use_internal_synth || self.synth_lab.route_score_preview {
                let active_p = self.selected_part_idx as u8;
                if let Some(eng) = self.ensure_synth_engine() {
                    eng.send(crate::synth::SynthCommand::SetActivePart(active_p));
                    for (gm_drum_note, velocity) in synth_drums {
                        eng.send(crate::synth::SynthCommand::TriggerDrum {
                            gm_drum_note,
                            velocity,
                        });
                    }
                }
            }
            if self.use_external_midi {
                if let Some(port_idx) = port_idx_opt {
                    if !notes_to_play.is_empty() {
                        let _ = self.midi_player.play_preview(
                            notes_to_play,
                            port_idx,
                            250,
                            false,
                            1,
                            2,
                        );
                    }
                }
            }
            return;
        }

        if event.notes.is_empty() {
            self.midi_player.stop();
            if let Some(eng) = self.synth_engine.as_mut() {
                eng.all_notes_off();
            }
            return;
        }

        let freq_table = crate::tuning_table::FretboardFrequencyTable::from_doc_part(doc, part);
        let mut notes_to_play = Vec::new();
        let mut synth_notes = Vec::new();

        let target_notes: Vec<&crate::mnx::Note> =
            if let Some(n_idx) = self.selected_note_idx_in_event {
                if let Some(n) = event.notes.get(n_idx) {
                    vec![n]
                } else {
                    event.notes.iter().collect()
                }
            } else {
                event.notes.iter().collect()
            };

        for note in target_notes {
            if note.muted.unwrap_or(false)
                || note
                    .harmonic
                    .as_ref()
                    .and_then(|h| h.sounding_note.muted)
                    .unwrap_or(false)
            {
                continue;
            }
            let string_opt = note
                .string
                .or_else(|| note.harmonic.as_ref().and_then(|h| h.sounding_note.string));
            if let Some(string) = string_opt {
                if let Some(freq_hz) = freq_table.note_to_frequency_hz(part, note) {
                    synth_notes.push((string, freq_hz));
                }

                let default_ch = if is_mpe { string + 1 } else { string };
                let ch_1_based = self
                    .string_channel_map
                    .get(&string)
                    .copied()
                    .unwrap_or(default_ch);
                let ch_0_based = (ch_1_based.saturating_sub(1)) % 16;
                let channel = midly::num::u4::from(ch_0_based);

                let midi_note_frac_opt = freq_table.note_to_midi_note_number(part, note);

                if let Some(midi_note_frac) = midi_note_frac_opt {
                    let base_note = midi_note_frac.round() as u8;
                    let semitone_diff = (midi_note_frac - (base_note as f64)) as f32;

                    let pitch_bend_val =
                        crate::midi::calculate_pitch_bend(semitone_diff, pb_range);

                    notes_to_play.push(crate::midi::NotePreview {
                        channel,
                        key: midly::num::u7::from(base_note.min(127)),
                        pitch_bend: Some(pitch_bend_val),
                        velocity: midly::num::u7::from(100),
                    });
                }
            }
        }

        if self.use_internal_synth || self.synth_lab.route_score_preview {
            let active_p = self.selected_part_idx as u8;
            let part_preset = self.get_part_synth_preset(self.selected_part_idx);
            let pluck_pos = part_preset.string_params.pluck_pos;
            let palm_mute = self.synth_lab.palm_mute;
            let velocity = self.synth_lab.velocity;
            if let Some(eng) = self.synth_engine.as_mut() {
                eng.send(crate::synth::SynthCommand::SetActivePart(active_p));
                for (string_idx, freq_hz) in synth_notes {
                    eng.send(crate::synth::SynthCommand::TriggerString {
                        string_idx,
                        freq_hz,
                        velocity,
                        pluck_pos,
                        palm_mute,
                        harmonic_node: None,
                    });
                }
            }
        }

        if self.use_external_midi {
            if let Some(port_idx) = port_idx_opt {
                if !notes_to_play.is_empty() {
                    let num_strings = part.num_strings() as u8;
                    let _ = self.midi_player.play_preview(
                        notes_to_play,
                        port_idx,
                        400,
                        is_mpe,
                        num_strings,
                        pb_range,
                    );
                }
            }
        }
    }

    /// Synchronizes the active instrument's tuning, frequency table, MIDI port, and percussion line mappings
    /// to the background `GridLivePlayState` for immediate, low-latency background grid controller playback.
    pub fn sync_live_play_config(&mut self) {
        let port_idx = self.get_part_midi_port(self.selected_part_idx);
        let is_mpe = self.is_part_mpe_mode(self.selected_part_idx);
        let pb_range = self.get_part_pitch_bend_range(self.selected_part_idx);
        let part_opt = self.active_part().cloned();
        let is_perc = part_opt.as_ref().map(|p| p.is_percussion()).unwrap_or(false);
        let perc_channel = part_opt.as_ref().map(|p| p.percussion_midi_channel()).unwrap_or(10);
        let num_strings = part_opt.as_ref().map(|p| p.num_strings() as u8).unwrap_or(6);

        let mut perc_lines = std::collections::HashMap::new();
        if let Some(part) = &part_opt {
            if is_perc {
                for (line_num, _id, comp) in part.ordered_kit_lines() {
                    let primary = comp.sound.unwrap_or(38);
                    let sym_sounds = comp.ext.symbol_sounds.iter().map(|(_, s)| *s).collect();
                    perc_lines.insert(line_num, (primary, sym_sounds));
                }
            }
        }

        let freq_table = if !is_perc {
            if let (Some(doc), Some(part)) = (&self.parsed_mnx, &part_opt) {
                Some(crate::tuning_table::FretboardFrequencyTable::from_doc_part(doc, part))
            } else {
                None
            }
        } else {
            None
        };

        let synth_tx = if self.use_internal_synth {
            self.ensure_synth_engine().map(|e| e.sender())
        } else {
            None
        };

        if let Ok(mut state) = self.grid_input.live_play_state.lock() {
            state.synth_sender = synth_tx;
            if state.connections.is_none() {
                state.connections = Some(self.midi_player.connections_arc());
            }
            if self.last_synced_part_idx != Some(self.selected_part_idx) {
                state.silence_all_held_notes();
                self.active_grid_synth_notes.clear();
                self.active_grid_live_notes.clear();
                self.last_synced_part_idx = Some(self.selected_part_idx);
            }
            if state.port_index != port_idx {
                state.silence_all_held_notes();
                state.port_index = port_idx;
                if let (Some(connections), Some(pid)) = (&state.connections, port_idx) {
                    let _ = crate::midi::ensure_cached_connection(connections, pid);
                }
            }
            state.just_play_mode = self.grid_input.just_play_mode;
            state.num_strings = num_strings;
            state.is_percussion = is_perc;
            state.percussion_channel = perc_channel;
            state.percussion_lines = perc_lines;
            state.is_mpe = is_mpe;
            state.pb_range = pb_range;
            state.string_channel_map = self.string_channel_map.clone();
            state.freq_table = freq_table;
            state.part_copy = part_opt;
            state.slots = self.grid_input.slots.clone();
        }
        self.live_play_dirty = false;
        self.last_live_play_just_play = self.grid_input.just_play_mode;
        self.last_live_play_controller_count = self.grid_input.active_controller_count();
    }

    /// Handles live note-on/note-off events from grid controllers in "Just Play" mode,
    /// looking up the exact pitch via `FretboardFrequencyTable` (or percussion line/symbol mapping)
    /// and streaming to MIDI output.
    pub fn handle_grid_live_play_events(
        &mut self,
        events: Vec<crate::grid_input::GridLivePlayEvent>,
    ) {
        if self.use_internal_synth {
            let _ = self.ensure_synth_engine();
        }

        let port_idx_opt = self.get_part_midi_port(self.selected_part_idx);
        let is_mpe = self.is_part_mpe_mode(self.selected_part_idx);
        let pb_range = self.get_part_pitch_bend_range(self.selected_part_idx);
        let doc = match &self.parsed_mnx {
            Some(d) => d,
            None => return,
        };
        let part = match doc
            .parts
            .get(self.selected_part_idx)
            .or_else(|| doc.parts.first())
        {
            Some(p) => p,
            None => return,
        };

        if part.is_percussion() {
            let channel = midly::num::u4::from(part.percussion_midi_channel().saturating_sub(1).min(15));
            for ev in events {
                let Some((_, comp)) = part.kit_component_for_line(ev.string) else {
                    continue;
                };
                let midi_note_opt = if ev.fret <= 0 {
                    Some(comp.sound.unwrap_or(38).min(127))
                } else {
                    let map_idx = (ev.fret - 1) as usize;
                    comp.ext
                        .symbol_sounds
                        .get(map_idx)
                        .map(|(_, note)| (*note).min(127))
                };

                if ev.is_note_on {
                    if let Some(midi_note) = midi_note_opt {
                        let key = midly::num::u7::from(midi_note);
                        let vel = midly::num::u7::from(ev.velocity.clamp(1, 127));
                        if self.use_internal_synth {
                            if let Some(eng) = self.synth_engine.as_mut() {
                                eng.send(crate::synth::SynthCommand::TriggerDrum {
                                    gm_drum_note: midi_note,
                                    velocity: (ev.velocity as f32 / 127.0).clamp(0.05, 1.0),
                                });
                            }
                        }
                        if self.use_external_midi {
                            if let Some(port_idx) = port_idx_opt {
                                let _ = self.midi_player.send_live_note_on(
                                    port_idx,
                                    channel,
                                    key,
                                    vel,
                                    None,
                                    false,
                                    1,
                                    2,
                                );
                            }
                        }
                        self.active_grid_live_notes
                            .insert((ev.string, ev.fret), (channel, key));
                    }
                } else if let Some((ch, key)) =
                    self.active_grid_live_notes.remove(&(ev.string, ev.fret))
                {
                    if self.use_external_midi {
                        if let Some(port_idx) = port_idx_opt {
                            let _ = self.midi_player.send_live_note_off(port_idx, ch, key);
                        }
                    }
                } else if let Some(midi_note) = midi_note_opt {
                    let key = midly::num::u7::from(midi_note);
                    if self.use_external_midi {
                        if let Some(port_idx) = port_idx_opt {
                            let _ = self.midi_player.send_live_note_off(port_idx, channel, key);
                        }
                    }
                }
            }
            return;
        }

        let freq_table = crate::tuning_table::FretboardFrequencyTable::from_doc_part(doc, part);
        let num_strings = part.num_strings() as u8;

        for ev in events {
            let default_ch = if is_mpe {
                ev.string + 1
            } else {
                ev.string
            };
            let ch_1_based = self
                .string_channel_map
                .get(&ev.string)
                .copied()
                .unwrap_or(default_ch);
            let ch_0_based = (ch_1_based.saturating_sub(1)) % 16;
            let channel = midly::num::u4::from(ch_0_based);

            if ev.is_note_on {
                if self.use_internal_synth {
                    if let Some(hz) = freq_table.string_fret_to_frequency_hz(part, ev.string, ev.fret) {
                        let vel_f32 = (ev.velocity as f32 / 127.0).clamp(0.05, 1.0);
                        let stack = self.active_grid_synth_notes.entry(ev.string).or_default();
                        stack.retain(|(f, _, _)| *f != ev.fret);
                        stack.push((ev.fret, hz, vel_f32));

                        if let Some(eng) = self.synth_engine.as_mut() {
                            eng.send(crate::synth::SynthCommand::TriggerString {
                                string_idx: ev.string,
                                freq_hz: hz,
                                velocity: vel_f32,
                                pluck_pos: 0.18,
                                palm_mute: 0.0,
                                harmonic_node: None,
                            });
                        }
                    }
                }
                if self.use_external_midi {
                    if let Some(midi_note_frac) =
                        freq_table.string_fret_to_midi_note_number(part, ev.string, ev.fret)
                    {
                        let base_note = midi_note_frac.round() as u8;
                        let semitone_diff = (midi_note_frac - (base_note as f64)) as f32;
                        let pitch_bend_val = crate::midi::calculate_pitch_bend(
                            semitone_diff,
                            pb_range,
                        );
                        let key = midly::num::u7::from(base_note.min(127));
                        let vel = midly::num::u7::from(ev.velocity.clamp(1, 127));

                        if let Some(port_idx) = port_idx_opt {
                            let _ = self.midi_player.send_live_note_on(
                                port_idx,
                                channel,
                                key,
                                vel,
                                Some(pitch_bend_val),
                                is_mpe,
                                num_strings,
                                pb_range,
                            );
                        }
                        self.active_grid_live_notes
                            .insert((ev.string, ev.fret), (channel, key));
                    }
                }
            } else {
                if self.use_internal_synth {
                    let mut should_mute = true;
                    let mut fallback_note = None;

                    if let Some(stack) = self.active_grid_synth_notes.get_mut(&ev.string) {
                        let was_top = stack.last().map(|(f, _, _)| *f == ev.fret).unwrap_or(false);
                        stack.retain(|(f, _, _)| *f != ev.fret);

                        if stack.is_empty() {
                            should_mute = true;
                        } else if was_top {
                            // Sounding note released, but another note on this string is still held!
                            // Fallback to the previous held note (voice stealing release / pull-off)
                            should_mute = false;
                            let (_, prev_hz, prev_vel) = *stack.last().unwrap();
                            fallback_note = Some((prev_hz, prev_vel));
                        } else {
                            // An older note was released while a newer note is still held and sounding.
                            // Do NOT mute or retrigger the sounding note!
                            should_mute = false;
                        }
                    }

                    if let Some(eng) = self.synth_engine.as_mut() {
                        if should_mute {
                            eng.send(crate::synth::SynthCommand::MuteString {
                                string_idx: ev.string,
                                release_rate: 0.995,
                            });
                        } else if let Some((hz, vel)) = fallback_note {
                            eng.send(crate::synth::SynthCommand::TriggerString {
                                string_idx: ev.string,
                                freq_hz: hz,
                                velocity: vel * 0.9,
                                pluck_pos: 0.18,
                                palm_mute: 0.0,
                                harmonic_node: None,
                            });
                        }
                    }
                }
                if self.use_external_midi {
                    if let Some((ch, key)) = self.active_grid_live_notes.remove(&(ev.string, ev.fret)) {
                        if let Some(port_idx) = port_idx_opt {
                            let _ = self.midi_player.send_live_note_off(port_idx, ch, key);
                        }
                    } else if let Some(midi_note_frac) =
                        freq_table.string_fret_to_midi_note_number(part, ev.string, ev.fret)
                    {
                        let base_note = midi_note_frac.round() as u8;
                        let key = midly::num::u7::from(base_note.min(127));
                        if let Some(port_idx) = port_idx_opt {
                            let _ = self.midi_player.send_live_note_off(port_idx, channel, key);
                        }
                    }
                }
            }
        }
    }

    pub fn sync_from_parsed_doc(&mut self, doc: MnxDocument) {
        let num_parts = doc.parts.len().max(1);
        if self.selected_part_idx >= num_parts {
            self.selected_part_idx = num_parts - 1;
        }
        if self.string_channel_dialog_part_idx >= num_parts {
            self.string_channel_dialog_part_idx = num_parts - 1;
        }
        if let LayoutViewTab::Part(p_idx) = self.layout_view_tab {
            if p_idx >= num_parts {
                self.layout_view_tab = LayoutViewTab::Ensemble;
            }
        }
        if self.ensemble_visible_parts.len() < num_parts {
            self.ensemble_visible_parts.resize(num_parts, true);
        } else if self.ensemble_visible_parts.len() > num_parts {
            self.ensemble_visible_parts.truncate(num_parts);
        }
        if !self.ensemble_visible_parts.iter().any(|&v| v) && !self.ensemble_visible_parts.is_empty()
        {
            self.ensemble_visible_parts[0] = true;
        }

        let errors = validate::validate_mnx(&doc);

        // Store formatted error strings for the bottom panel UI
        self.validation_errors = errors
            .iter()
            .map(|err| {
                if err.string > 0 {
                    format!(
                        "Measure {}, Event {}: {}",
                        err.measure_idx,
                        err.event_idx + 1,
                        err.message
                    )
                } else {
                    format!("Measure {}: {}", err.measure_idx, err.message)
                }
            })
            .collect();

        // Sync BPM from YAML if a tempo directive is present
        let yaml_bpm = doc
            .global
            .measures
            .iter()
            .flat_map(|m| m.tempos.iter())
            .find_map(|t| Some(t.bpm));
        if let Some(bpm) = yaml_bpm {
            let formatted = if bpm.fract() == 0.0 {
                format!("{:.0}", bpm)
            } else {
                format!("{}", bpm)
            };
            // Only update the text buf when it differs in value, to avoid overwriting while user types
            if self.bpm_buf.trim().parse::<f64>().ok() != Some(bpm) {
                self.bpm_buf = formatted;
            }
        }

        // Sync MPE mode, pitch bend range, and per-part synth presets from YAML
        if self.parts_runtime.len() < num_parts {
            self.parts_runtime
                .resize_with(num_parts, PartRuntimeState::default);
        } else if self.parts_runtime.len() > num_parts {
            self.parts_runtime.truncate(num_parts);
        }
        for (i, part) in doc.parts.iter().enumerate() {
            let rt = &mut self.parts_runtime[i];
            rt.mpe_mode = part.mpe;
            rt.pitch_bend_range = part.pitch_bend_range.map(|pb| pb.clamp(1, 48));
            if let Some(sp) = &part.ext.synth_preset {
                rt.synth_preset = Some(sp.clone());
            }
        }
        self.is_mpe_mode = self.is_part_mpe_mode(self.selected_part_idx);
        self.pitch_bend_range = self.get_part_pitch_bend_range(self.selected_part_idx);

        self.parsed_errors = errors;
        self.parsed_mnx = Some(doc);
        if self.synth_lab.target_part_idx >= num_parts {
            self.synth_lab.target_part_idx = 0;
        }
        let target_preset = self.get_part_synth_preset(self.synth_lab.target_part_idx);
        self.synth_lab.preset = target_preset.clone();
        self.synth_lab.profile = target_preset.body_params.profile_base;
        self.live_play_dirty = true;
        if (self.show_yaml_panel || cfg!(test)) && !self.yaml_dirty {
            self.refresh_cached_event_ranges();
        }
    }

    fn commit_doc_without_undo(&mut self, doc: MnxDocument) {
        if self.show_yaml_panel || cfg!(test) {
            match serde_saphyr::to_string(&doc) {
                Ok(new_yaml) => {
                    self.yaml_text = new_yaml;
                    self.yaml_dirty = false;
                    self.sync_from_parsed_doc(doc);
                }
                Err(e) => {
                    self.sync_from_parsed_doc(doc);
                    self.validation_errors
                        .push(format!("YAML SERIALIZE ERROR:\n{}", e));
                }
            }
        } else {
            self.yaml_dirty = true;
            self.sync_from_parsed_doc(doc);
        }
        if self.synth_engine.is_some() {
            self.sync_all_parts_to_synth_engine();
        }
    }

    fn apply_mutated_doc_inner(&mut self, mut doc: MnxDocument, clean_ties: bool) {
        self.interactive_gesture_active = false;
        self.yaml_edit_session_active = false;
        doc.normalize(clean_ties);
        if let Some(prev) = self.parsed_mnx.take() {
            if prev != doc {
                self.push_undo_snapshot(UndoSnapshot {
                    doc: Some(prev),
                    yaml_fallback: None,
                });
            }
        } else {
            self.push_undo_state();
        }
        self.commit_doc_without_undo(doc);
    }

    pub fn apply_mutated_doc(&mut self, doc: MnxDocument) {
        self.apply_mutated_doc_inner(doc, false);
    }

    pub fn apply_mutated_doc_and_clean_ties(&mut self, doc: MnxDocument) {
        self.apply_mutated_doc_inner(doc, true);
    }

    pub fn mutate_doc<F, R>(&mut self, f: F) -> Option<R>
    where
        F: FnOnce(&mut MnxDocument) -> Option<R>,
    {
        let prev_opt = self.parsed_mnx.clone();
        let mut doc = self
            .parsed_mnx
            .take()
            .or_else(|| serde_saphyr::from_str::<MnxDocument>(&self.yaml_text).ok())?;
        match f(&mut doc) {
            Some(res) => {
                self.parsed_mnx = prev_opt;
                self.apply_mutated_doc_inner(doc, false);
                Some(res)
            }
            None => {
                self.parsed_mnx = Some(doc);
                None
            }
        }
    }

    pub fn mutate_doc_and_clean_ties<F, R>(&mut self, f: F) -> Option<R>
    where
        F: FnOnce(&mut MnxDocument) -> Option<R>,
    {
        let prev_opt = self.parsed_mnx.clone();
        let mut doc = self
            .parsed_mnx
            .take()
            .or_else(|| serde_saphyr::from_str::<MnxDocument>(&self.yaml_text).ok())?;
        match f(&mut doc) {
            Some(res) => {
                self.parsed_mnx = prev_opt;
                self.apply_mutated_doc_inner(doc, true);
                Some(res)
            }
            None => {
                self.parsed_mnx = Some(doc);
                None
            }
        }
    }

    /// Parses `self.yaml_text` while the user is actively typing in the YAML `TextEdit`,
    /// updating `parsed_mnx` and validation without overwriting `self.yaml_text` mid-keystroke.
    pub fn parse_yaml_from_editor(&mut self) {
        match serde_saphyr::from_str::<MnxDocument>(&self.yaml_text) {
            Ok(mut doc) => {
                doc.ensure_measure_parity();
                doc.clean_unneeded_fret_fields();
                self.sync_from_parsed_doc(doc);
                if self.synth_engine.is_some() {
                    self.sync_all_parts_to_synth_engine();
                }
            }
            Err(e) => {
                self.refresh_cached_event_ranges();
                self.parsed_mnx = None;
                self.parsed_errors = Vec::new();
                self.validation_errors = vec![format!("YAML PARSE ERROR:\n{}", e)];
            }
        }
    }

    pub fn process_and_render(&mut self) {
        self.yaml_dirty = false;
        match serde_saphyr::from_str::<MnxDocument>(&self.yaml_text) {
            Ok(mut doc) => {
                let healed = doc.normalize(false);
                if healed {
                    if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
                        self.yaml_text = new_yaml;
                    }
                }
                self.sync_from_parsed_doc(doc);
                if self.synth_engine.is_some() {
                    self.sync_all_parts_to_synth_engine();
                }
            }
            Err(e) => {
                self.refresh_cached_event_ranges();
                self.parsed_mnx = None;
                self.parsed_errors = Vec::new();
                self.validation_errors = vec![format!("YAML PARSE ERROR:\n{}", e)];
            }
        }
    }

    pub fn get_part_midi_port(&self, part_idx: usize) -> Option<usize> {
        if let Some(port) = self.part_runtime(part_idx).and_then(|rt| rt.midi_port) {
            if self.midi_ports.iter().any(|p| p.id == port) {
                return Some(port);
            }
        }
        self.midi_ports.first().map(|p| p.id)
    }

    pub fn set_part_midi_port(&mut self, part_idx: usize, port_id: usize) {
        self.part_runtime_mut(part_idx).midi_port = Some(port_id);
        if part_idx == self.selected_part_idx {
            self.selected_midi_port = Some(port_id);
            self.live_play_dirty = true;
        }
    }

    pub fn is_part_mpe_mode(&self, part_idx: usize) -> bool {
        if let Some(mpe) = self.part_runtime(part_idx).and_then(|rt| rt.mpe_mode) {
            return mpe;
        }
        if let Some(mpe) = self
            .parsed_mnx
            .as_ref()
            .and_then(|d| d.parts.get(part_idx))
            .and_then(|p| p.mpe)
        {
            return mpe;
        }
        false
    }

    pub fn get_part_pitch_bend_range(&self, part_idx: usize) -> u8 {
        if let Some(pb) = self
            .part_runtime(part_idx)
            .and_then(|rt| rt.pitch_bend_range)
        {
            return pb;
        }
        if let Some(pb) = self
            .parsed_mnx
            .as_ref()
            .and_then(|d| d.parts.get(part_idx))
            .and_then(|p| p.pitch_bend_range)
        {
            pb.clamp(1, 48)
        } else if self.is_part_mpe_mode(part_idx) {
            48
        } else {
            2
        }
    }

    pub fn set_part_pitch_bend_range(&mut self, part_idx: usize, pb_range: u8) {
        let clamped = pb_range.clamp(1, 48);
        self.part_runtime_mut(part_idx).pitch_bend_range = Some(clamped);
        if part_idx == self.selected_part_idx {
            self.pitch_bend_range = clamped;
        }
        self.mutate_doc(|doc| {
            let part = doc.parts.get_mut(part_idx)?;
            part.pitch_bend_range = Some(clamped);
            if part.name.is_none() {
                part.name = Some(part.display_name(part_idx));
            }
            Some(())
        });
    }

    #[allow(dead_code)]
    pub fn set_pitch_bend_range_in_yaml(&mut self, pb_range: u8) {
        self.set_part_pitch_bend_range(self.selected_part_idx, pb_range);
    }

    pub fn compute_prior_strings_on_port(&self, part_idx: usize, target_port: usize) -> u8 {
        self.parsed_mnx
            .as_ref()
            .map(|d| {
                d.parts
                    .iter()
                    .enumerate()
                    .take(part_idx)
                    .filter(|(pi, _)| {
                        self.get_part_midi_port(*pi).unwrap_or(0) == target_port
                    })
                    .map(|(_, p)| p.num_strings() as u8)
                    .sum()
            })
            .unwrap_or(0)
    }

    pub fn set_part_mpe_mode(&mut self, part_idx: usize, is_mpe: bool) {
        let new_pb = if is_mpe { 48 } else { 2 };
        {
            let rt = self.part_runtime_mut(part_idx);
            rt.mpe_mode = Some(is_mpe);
            rt.pitch_bend_range = Some(new_pb);
        }
        if part_idx == self.selected_part_idx {
            self.is_mpe_mode = is_mpe;
            self.pitch_bend_range = new_pb;
        }

        let num_strings = self
            .parsed_mnx
            .as_ref()
            .and_then(|d| d.parts.get(part_idx))
            .map(|p| p.num_strings())
            .unwrap_or(6) as u8;

        let target_port = self.get_part_midi_port(part_idx).unwrap_or(0);
        let prior_strings_on_port = self.compute_prior_strings_on_port(part_idx, target_port);
        let updated_map = {
            let part_map = &mut self.part_runtime_mut(part_idx).string_channel_map;
            part_map.clear();
            for s in 1..=num_strings {
                let def_ch = if is_mpe {
                    2 + ((prior_strings_on_port + s - 1) % 15)
                } else {
                    1 + ((prior_strings_on_port + s - 1) % 16)
                };
                part_map.insert(s, def_ch);
            }
            part_map.clone()
        };
        if part_idx == self.selected_part_idx {
            self.string_channel_map = updated_map;
        }

        self.mutate_doc(|doc| {
            let part = doc.parts.get_mut(part_idx)?;
            part.mpe = Some(is_mpe);
            part.pitch_bend_range = Some(new_pb);
            if part.name.is_none() {
                part.name = Some(part.display_name(part_idx));
            }
            Some(())
        });
    }

    pub fn is_modified(&self) -> bool {
        if self.yaml_dirty {
            match (&self.parsed_mnx, &self.saved_doc) {
                (Some(cur), Some(saved)) => cur != saved,
                _ => true,
            }
        } else {
            self.yaml_text != self.saved_yaml
        }
    }

    pub fn open_instrument_setup_dialog(&mut self, show_custom_fretboard: bool) {
        self.is_adding_instrument_in_dialog = false;
        let part_idx = self.selected_part_idx;
        let doc = self.parsed_mnx.as_ref();
        let part = doc.and_then(|d| d.parts.get(part_idx).or_else(|| d.parts.first()));
        self.instrument_setup_name_buf = part
            .map(|p| p.display_name(part_idx))
            .unwrap_or_else(|| "Guitar".to_string());
        self.instrument_setup_is_percussion = part.map(|p| p.is_percussion()).unwrap_or(false);
        self.instrument_setup_percussion_lines = part
            .and_then(|p| p.kit.as_ref())
            .map(crate::presets::kit_to_line_configs)
            .filter(|v| !v.is_empty())
            .unwrap_or_else(crate::presets::standard_6_line_kit_configs);
        self.instrument_setup_new_sym_bufs = vec![String::new(); 15];
        let num_strings = part.map(|p| p.num_strings()).unwrap_or(6) as u8;
        let edo = part.map(|p| p.edo()).unwrap_or(12) as u8;
        let step_multiplier = part.map(|p| p.step_multiplier()).unwrap_or(1) as u8;
        let fretboard_editor = doc
            .map(|d| crate::fretboard_editor::FretboardEditorState::from_doc_part(d, part_idx))
            .unwrap_or_else(|| {
                crate::fretboard_editor::FretboardEditorState::new(
                    num_strings,
                    Vec::new(),
                    "D".to_string(),
                    4,
                    293.67,
                )
            });

        let any_ratio_or_cents = part
            .and_then(|p| p.ext.tuning.as_ref())
            .map(|tunings| tunings.iter().any(|t| t.pitch.is_ratio_or_cents_step()))
            .unwrap_or(false);
        let tuning_mode = if any_ratio_or_cents {
            InstrumentTuningMode::RatioCents
        } else {
            InstrumentTuningMode::Edo
        };

        let mut tunings_text = Vec::new();
        let mut tunings_name = Vec::new();
        let mut tunings_octave = Vec::new();
        let edo_scale = crate::edo::generate_edo_scale(edo as u32);
        let tuning_calc = crate::edo::EdoTuning::new(edo as u32);

        for s in 1..=15 {
            if let Some(pitch) = part.and_then(|p| p.get_string_tuning(s)) {
                let resolved_name = crate::fretboard_editor::resolve_open_string_53edo_name(
                    &pitch,
                    edo as u32,
                    Some(&fretboard_editor.reference_note_step),
                );
                if tuning_mode == InstrumentTuningMode::RatioCents {
                    let step_str = if pitch.is_ratio_or_cents_step() {
                        pitch.step.clone()
                    } else {
                        "1/1".to_string()
                    };
                    tunings_text.push(step_str);
                    tunings_name.push(resolved_name);
                } else {
                    let steps = tuning_calc.pitch_to_steps(&pitch);
                    let step_in_oct = steps.rem_euclid(edo as i32);
                    let text = edo_scale
                        .iter()
                        .find(|(st, _, _, _)| *st == step_in_oct)
                        .map(|(_, n, _, _)| n.clone())
                        .unwrap_or_else(|| pitch.step.clone());
                    tunings_text.push(text.clone());
                    tunings_name.push(text);
                }
                tunings_octave.push(pitch.octave);
            } else {
                let (step, octave) = crate::mnx::standard_15_string_tuning(s);
                if tuning_mode == InstrumentTuningMode::RatioCents {
                    tunings_text.push("1/1".to_string());
                    tunings_name.push(fretboard_editor.reference_note_step.clone());
                } else {
                    tunings_text.push(step.to_string());
                    tunings_name.push(step.to_string());
                }
                tunings_octave.push(octave);
            }
        }

        self.active_dialog = ActiveDialog::InstrumentSetup(Box::new(InstrumentSetupForm {
            num_strings,
            edo,
            step_multiplier,
            tuning_mode,
            tunings_text,
            tunings_name,
            tunings_octave,
            show_custom_fretboard,
            fretboard_editor,
            is_new_tab: false,
        }));
    }

    pub fn open_add_instrument_dialog(&mut self) {
        self.is_adding_instrument_in_dialog = true;
        let next_num = self
            .parsed_mnx
            .as_ref()
            .map(|d| d.parts.len() + 1)
            .unwrap_or(2);
        self.instrument_setup_name_buf = format!("Guitar {}", next_num);
        self.instrument_setup_is_percussion = false;
        self.instrument_setup_percussion_lines = crate::presets::standard_6_line_kit_configs();
        self.instrument_setup_new_sym_bufs = vec![String::new(); 15];

        let (ref_step, ref_oct, ref_freq) = self
            .parsed_mnx
            .as_ref()
            .map(|d| {
                let fe = crate::fretboard_editor::FretboardEditorState::from_doc(d);
                (
                    fe.reference_note_step,
                    fe.reference_note_octave,
                    fe.reference_frequency_hz,
                )
            })
            .unwrap_or_else(|| ("D".to_string(), 4, 293.67));

        self.active_dialog = ActiveDialog::InstrumentSetup(Box::new(
            InstrumentSetupForm::default_12edo_6_string(ref_step, ref_oct, ref_freq, false),
        ));
    }

    pub fn add_new_instrument_part(
        &mut self,
        name: String,
        _num_strings: u8,
        edo: u8,
        step_multiplier: u8,
        tunings: Vec<crate::mnx::StringTuning>,
        custom_frets: Option<Vec<crate::fretboard_editor::CustomFret>>,
    ) {
        let trimmed_name = name.trim();
        let part_name = if trimmed_name.is_empty() {
            let next_num = self
                .parsed_mnx
                .as_ref()
                .map(|d| d.parts.len() + 1)
                .unwrap_or(2);
            format!("Guitar {}", next_num)
        } else {
            trimmed_name.to_string()
        };

        let mut new_part_idx = 0;
        self.mutate_doc(|doc| {
            if doc.global.measures.is_empty() {
                doc.global.measures.push(crate::mnx::GlobalMeasure {
                    time: Some(crate::mnx::TimeSignature { count: 4, unit: 4 }),
                    ..Default::default()
                });
            }
            let num_measures = doc.global.measures.len();
            let mut measures = Vec::with_capacity(num_measures);
            for m_idx in 0..num_measures {
                let expected_beats =
                    crate::render::get_time_signature_for_measure(Some(&doc.global), m_idx)
                        .expected_quarter_beats();
                let content = crate::yaml_editor::make_rest_items_for_beats(expected_beats);
                measures.push(crate::mnx::Measure {
                    comment: None,
                    beams: None,
                    arpeggios: None,
                    sequences: vec![crate::mnx::Sequence {
                        comment: None,
                        content,
                    }],
                });
            }

            let new_part = crate::mnx::Part {
                id: None,
                name: Some(part_name),
                short_name: None,
                comment: None,
                kit: None,
                ext: crate::mnx::PartExt {
                    micro_fret_divisions: Some(edo),
                    fret_step_multiplier: Some(step_multiplier),
                    pitch_bend_range: None,
                    capo: None,
                    tuning: Some(tunings),
                    custom_frets,
                    ..Default::default()
                },
                measures,
            };

            doc.parts.push(new_part);
            new_part_idx = doc.parts.len() - 1;
            Some(())
        });

        self.is_adding_instrument_in_dialog = false;
        self.select_part(new_part_idx);
        self.selected_event_idx = Some(0);
    }

    pub fn remove_instrument_part(&mut self, part_idx: usize) {
        let can_remove = self
            .parsed_mnx
            .as_ref()
            .map(|d| d.parts.len() > 1 && part_idx < d.parts.len())
            .unwrap_or(false);
        if !can_remove {
            return;
        }

        if part_idx < self.ensemble_visible_parts.len() {
            self.ensemble_visible_parts.remove(part_idx);
        }
        if part_idx < self.parts_runtime.len() {
            self.parts_runtime.remove(part_idx);
        }

        self.mutate_doc(|doc| {
            if doc.parts.len() > 1 && part_idx < doc.parts.len() {
                doc.parts.remove(part_idx);
                Some(())
            } else {
                None
            }
        });

        let new_count = self
            .parsed_mnx
            .as_ref()
            .map(|d| d.parts.len())
            .unwrap_or(1)
            .max(1);
        let next_sel = self.selected_part_idx.min(new_count - 1);
        self.selected_part_idx = next_sel;
        self.string_channel_dialog_part_idx = next_sel;
        self.string_channel_map = self
            .part_runtime(next_sel)
            .map(|rt| rt.string_channel_map.clone())
            .unwrap_or_default();
        self.selected_midi_port = self.get_part_midi_port(next_sel);
        self.pitch_bend_range = self.get_part_pitch_bend_range(next_sel);
        self.is_mpe_mode = self.is_part_mpe_mode(next_sel);
        self.live_play_dirty = true;
        if self.show_yaml_panel || cfg!(test) {
            self.ensure_yaml_synced();
            self.refresh_cached_event_ranges();
        }
        if self.synth_engine.is_some() {
            self.sync_all_parts_to_synth_engine();
        }
    }

    pub fn prompt_new_tab_instrument_setup(&mut self) {
        self.is_adding_instrument_in_dialog = false;
        self.instrument_setup_name_buf = "Guitar".to_string();
        self.instrument_setup_is_percussion = false;
        self.instrument_setup_percussion_lines = crate::presets::standard_6_line_kit_configs();
        self.instrument_setup_new_sym_bufs = vec![String::new(); 15];
        self.active_dialog = ActiveDialog::InstrumentSetup(Box::new(
            InstrumentSetupForm::default_12edo_6_string("D".to_string(), 4, 293.67, true),
        ));
    }

    pub fn start_new_tab(
        &mut self,
        _num_strings: u8,
        edo: u8,
        step_multiplier: u8,
        tunings: Vec<crate::mnx::StringTuning>,
    ) {
        let part_name = if self.instrument_setup_name_buf.trim().is_empty() {
            "Guitar".to_string()
        } else {
            self.instrument_setup_name_buf.trim().to_string()
        };
        let doc = MnxDocument {
            mnx: crate::mnx::MnxMeta { version: 1 },
            comment: None,
            ext: crate::mnx::DocumentExt::default(),
            global: crate::mnx::Global {
                measures: vec![crate::mnx::GlobalMeasure {
                    time: Some(crate::mnx::TimeSignature { count: 4, unit: 4 }),
                    ..Default::default()
                }],
                ..Default::default()
            },
            parts: vec![crate::mnx::Part {
                id: None,
                name: Some(part_name),
                short_name: None,
                comment: None,
                kit: None,
                ext: crate::mnx::PartExt {
                    micro_fret_divisions: Some(edo),
                    fret_step_multiplier: Some(step_multiplier),
                    pitch_bend_range: if self.pitch_bend_range != 2 {
                        Some(self.pitch_bend_range)
                    } else {
                        None
                    },
                    capo: None,
                    tuning: Some(tunings),
                    custom_frets: None,
                    ..Default::default()
                },
                measures: vec![crate::mnx::Measure {
                    comment: None,
                    beams: None,
                    arpeggios: None,
                    sequences: vec![crate::mnx::Sequence {
                        comment: None,
                        content: vec![crate::mnx::SequenceItem::Event(crate::mnx::Event {
                            markings: None,
                            id: None,
                            duration: crate::mnx::Duration {
                                base: "quarter".to_string(),
                                dots: None,
                                multiple: None,
                            },
                            notes: Vec::new(),
                            kit_notes: Vec::new(),
                            rest: Some(crate::mnx::Rest {}),
                            slurs: None,
                            lyrics: None,
                            comment: None,
                            ext: crate::mnx::EventExt::default(),
                        })],
                    }],
                }],
            }],
        };

        let mut doc = doc;
        doc.normalize(false);
        let new_yaml = serde_saphyr::to_string(&doc).unwrap_or_default();

        self.undo_stack.clear();
        self.redo_stack.clear();
        self.yaml_edit_session_active = false;
        self.interactive_gesture_active = false;
        self.yaml_text = new_yaml.clone();
        self.saved_yaml = new_yaml;
        self.saved_doc = Some(doc.clone());
        self.yaml_dirty = false;
        self.current_file_path = None;
        self.selected_part_idx = 0;
        self.string_channel_dialog_part_idx = 0;
        self.parts_runtime.clear();
        self.layout_view_tab = LayoutViewTab::Ensemble;
        self.ensemble_visible_parts = vec![true];
        self.selected_event_idx = Some(0);
        self.selected_note_idx_in_event = None;
        self.is_editing_fret = false;
        self.editing_fret_buf.clear();
        self.editing_string = None;
        self.sync_from_parsed_doc(doc);
        if self.synth_engine.is_some() {
            self.sync_all_parts_to_synth_engine();
        }
    }

    pub fn start_new_percussion_tab(
        &mut self,
        lines: Vec<crate::presets::PercussionLineConfig>,
    ) {
        let part_name = if self.instrument_setup_name_buf.trim().is_empty()
            || self.instrument_setup_name_buf.trim() == "Guitar"
        {
            "Drum Kit".to_string()
        } else {
            self.instrument_setup_name_buf.trim().to_string()
        };
        let kit = crate::presets::line_configs_to_kit(&lines);
        let doc = MnxDocument {
            mnx: crate::mnx::MnxMeta { version: 1 },
            comment: None,
            ext: crate::mnx::DocumentExt::default(),
            global: crate::mnx::Global {
                measures: vec![crate::mnx::GlobalMeasure {
                    time: Some(crate::mnx::TimeSignature { count: 4, unit: 4 }),
                    ..Default::default()
                }],
                ..Default::default()
            },
            parts: vec![crate::mnx::Part {
                id: None,
                name: Some(part_name),
                short_name: None,
                comment: None,
                kit: Some(kit),
                ext: crate::mnx::PartExt {
                    midi_channel: Some(10),
                    ..Default::default()
                },
                measures: vec![crate::mnx::Measure {
                    comment: None,
                    beams: None,
                    arpeggios: None,
                    sequences: vec![crate::mnx::Sequence {
                        comment: None,
                        content: vec![crate::mnx::SequenceItem::Event(crate::mnx::Event {
                            markings: None,
                            id: None,
                            duration: crate::mnx::Duration {
                                base: "quarter".to_string(),
                                dots: None,
                                multiple: None,
                            },
                            notes: Vec::new(),
                            kit_notes: Vec::new(),
                            rest: Some(crate::mnx::Rest {}),
                            slurs: None,
                            lyrics: None,
                            comment: None,
                            ext: crate::mnx::EventExt::default(),
                        })],
                    }],
                }],
            }],
        };

        let mut doc = doc;
        doc.normalize(false);
        let new_yaml = serde_saphyr::to_string(&doc).unwrap_or_default();

        self.undo_stack.clear();
        self.redo_stack.clear();
        self.yaml_edit_session_active = false;
        self.interactive_gesture_active = false;
        self.yaml_text = new_yaml.clone();
        self.saved_yaml = new_yaml;
        self.saved_doc = Some(doc.clone());
        self.yaml_dirty = false;
        self.current_file_path = None;
        self.selected_part_idx = 0;
        self.string_channel_dialog_part_idx = 0;
        self.parts_runtime.clear();
        self.layout_view_tab = LayoutViewTab::Ensemble;
        self.ensemble_visible_parts = vec![true];
        self.selected_event_idx = Some(0);
        self.selected_note_idx_in_event = None;
        self.is_editing_fret = false;
        self.editing_fret_buf.clear();
        self.editing_string = None;
        self.sync_from_parsed_doc(doc);
        if self.synth_engine.is_some() {
            self.sync_all_parts_to_synth_engine();
        }
    }

    pub fn add_new_percussion_part(
        &mut self,
        name: String,
        lines: Vec<crate::presets::PercussionLineConfig>,
    ) {
        let trimmed_name = name.trim();
        let part_name = if trimmed_name.is_empty() || trimmed_name.starts_with("Guitar ") {
            "Drum Kit".to_string()
        } else {
            trimmed_name.to_string()
        };
        let kit = crate::presets::line_configs_to_kit(&lines);

        let mut new_part_idx = 0;
        self.mutate_doc(|doc| {
            if doc.global.measures.is_empty() {
                doc.global.measures.push(crate::mnx::GlobalMeasure {
                    time: Some(crate::mnx::TimeSignature { count: 4, unit: 4 }),
                    ..Default::default()
                });
            }
            let num_measures = doc.global.measures.len();
            let mut measures = Vec::with_capacity(num_measures);
            for m_idx in 0..num_measures {
                let expected_beats =
                    crate::render::get_time_signature_for_measure(Some(&doc.global), m_idx)
                        .expected_quarter_beats();
                let content = crate::yaml_editor::make_rest_items_for_beats(expected_beats);
                measures.push(crate::mnx::Measure {
                    comment: None,
                    beams: None,
                    arpeggios: None,
                    sequences: vec![crate::mnx::Sequence {
                        comment: None,
                        content,
                    }],
                });
            }

            let new_part = crate::mnx::Part {
                id: None,
                name: Some(part_name),
                short_name: None,
                comment: None,
                kit: Some(kit),
                ext: crate::mnx::PartExt {
                    midi_channel: Some(10),
                    ..Default::default()
                },
                measures,
            };

            doc.parts.push(new_part);
            new_part_idx = doc.parts.len() - 1;
            Some(())
        });

        self.is_adding_instrument_in_dialog = false;
        self.select_part(new_part_idx);
        self.selected_event_idx = Some(0);
    }

    pub fn apply_percussion_setup_to_part(
        &mut self,
        part_idx: usize,
        name: String,
        lines: Vec<crate::presets::PercussionLineConfig>,
    ) {
        let kit = crate::presets::line_configs_to_kit(&lines);
        self.mutate_doc(|doc| {
            let part = doc.parts.get_mut(part_idx)?;
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                part.name = Some(trimmed.to_string());
            }
            part.kit = Some(kit.clone());
            part.ext.tuning = None;
            part.ext.micro_fret_divisions = None;
            part.ext.fret_step_multiplier = None;
            part.ext.custom_frets = None;
            if part.ext.midi_channel.is_none() {
                part.ext.midi_channel = Some(10);
            }
            // Remove any kit_notes referencing deleted component IDs
            for ev in part.events_mut() {
                ev.notes.clear();
                ev.kit_notes.retain(|kn| kit.contains_key(&kn.kit_component));
                if ev.kit_notes.is_empty() {
                    ev.rest = Some(crate::mnx::Rest {});
                }
            }
            Some(())
        });
    }

    pub fn new_file(&mut self) {
        if self.is_modified() {
            self.active_dialog = ActiveDialog::ConfirmDiscardChanges {
                target: DiscardTarget::NewProject,
            };
        } else {
            self.prompt_new_tab_instrument_setup();
        }
    }

    pub(crate) fn open_file(&mut self) {
        if self.is_modified() {
            self.active_dialog = ActiveDialog::ConfirmDiscardChanges {
                target: DiscardTarget::OpenFile,
            };
        } else {
            self.perform_open_file();
        }
    }

    /// Loads `content` as a freshly opened score, clearing prior undo/redo history so `Cmd+Z`
    /// cannot revert into a previously open file and overwrite `file_path`.
    pub fn load_yaml_into_fresh_session(
        &mut self,
        content: String,
        file_path: Option<std::path::PathBuf>,
    ) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.yaml_edit_session_active = false;
        self.interactive_gesture_active = false;
        self.yaml_text = content;
        self.yaml_dirty = false;
        self.current_file_path = file_path;
        self.selected_part_idx = 0;
        self.string_channel_dialog_part_idx = 0;
        self.parts_runtime.clear();
        self.layout_view_tab = LayoutViewTab::Ensemble;
        self.selected_event_idx = None;
        self.selected_note_idx_in_event = None;
        self.is_editing_fret = false;
        self.editing_fret_buf.clear();
        self.editing_string = None;
        self.process_and_render();
        self.saved_yaml = self.yaml_text.clone();
        self.saved_doc = self.parsed_mnx.clone();
    }

    pub(crate) fn perform_open_file(&mut self) -> bool {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("YAML Documents", &["yaml", "yml"])
            .pick_file()
        {
            match fs::read_to_string(&path) {
                Ok(content) => {
                    self.load_yaml_into_fresh_session(content, Some(path));
                    true
                }
                Err(e) => {
                    self.validation_errors = vec![format!("Failed to read file: {}", e)];
                    false
                }
            }
        } else {
            false
        }
    }

    pub(crate) fn save_file(&mut self) -> bool {
        // If we have a known file path, save to it. Otherwise, trigger "Save As".
        if let Some(path) = self.current_file_path.clone() {
            self.ensure_yaml_synced();
            if let Err(e) = crate::rebar::atomic_write_file(&path, self.yaml_text.as_bytes()) {
                self.validation_errors = vec![format!("Failed to save file: {}", e)];
                false
            } else {
                self.saved_yaml = self.yaml_text.clone();
                self.saved_doc = self.parsed_mnx.clone();
                true
            }
        } else {
            self.save_as_file()
        }
    }

    pub(crate) fn save_as_file(&mut self) -> bool {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("YAML Documents", &["yaml", "yml"])
            .set_file_name("untitled.yaml")
            .save_file()
        {
            self.ensure_yaml_synced();
            if let Err(e) = crate::rebar::atomic_write_file(&path, self.yaml_text.as_bytes()) {
                self.validation_errors = vec![format!("Failed to save file: {}", e)];
                false
            } else {
                // Update the current path so subsequent "Saves" go here
                self.current_file_path = Some(path);
                self.saved_yaml = self.yaml_text.clone();
                self.saved_doc = self.parsed_mnx.clone();
                true
            }
        } else {
            false
        }
    }

    pub(crate) fn export_pdf_file(&mut self) -> bool {
        let default_name = self
            .current_file_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .map(|stem| format!("{}.pdf", stem))
            .unwrap_or_else(|| "score.pdf".to_string());

        if let Some(mut path) = rfd::FileDialog::new()
            .add_filter("PDF Document", &["pdf"])
            .set_file_name(&default_name)
            .save_file()
        {
            if path.extension().and_then(|e| e.to_str()) != Some("pdf") {
                path.set_extension("pdf");
            }
            let active_parts = self.active_layout_part_indices();
            if let Some(doc) = &self.parsed_mnx {
                let pages = crate::layout::compute_score_layout_for_parts(doc, 0.0, &active_parts);
                if let Err(e) =
                    crate::pdf_export::export_score_to_pdf_for_parts(doc, &pages, &path, &active_parts)
                {
                    self.validation_errors = vec![format!("Failed to export PDF file: {}", e)];
                    false
                } else {
                    true
                }
            } else {
                false
            }
        } else {
            false
        }
    }

    pub(crate) fn export_mnx_file(&mut self) -> bool {
        let default_name = self
            .current_file_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .map(|stem| format!("{}.mnx", stem))
            .unwrap_or_else(|| "score.mnx".to_string());

        if let Some(mut path) = rfd::FileDialog::new()
            .add_filter("MNX Document", &["mnx"])
            .set_file_name(&default_name)
            .save_file()
        {
            if path.extension().and_then(|e| e.to_str()) != Some("mnx") {
                path.set_extension("mnx");
            }
            let json_res = if let Some(doc) = &self.parsed_mnx {
                doc.to_mnx_export_json()
            } else {
                self.ensure_yaml_synced();
                crate::mnx::export_yaml_to_mnx_json(&self.yaml_text)
            };
            match json_res {
                Ok(json_str) => {
                    if let Err(e) = crate::rebar::atomic_write_file(&path, json_str.as_bytes()) {
                        self.validation_errors = vec![format!("Failed to export .mnx file: {}", e)];
                        false
                    } else {
                        true
                    }
                }
                Err(e) => {
                    self.validation_errors = vec![format!("Failed to export .mnx file: {}", e)];
                    false
                }
            }
        } else {
            false
        }
    }

    pub(crate) fn open_text_marking_dialog(&mut self, event_idx: usize) {
        let mut text_buf = String::new();
        let mut bold = false;
        let mut italic = false;
        let mut underline = false;
        let mut font_family = "Sans-Serif".to_string();
        let mut font_size = 12.0;
        let mut has_existing = false;

        if let Some(doc) = &self.parsed_mnx {
            let paths = doc.get_flattened_event_paths_for_part(self.selected_part_idx);
            if let Some(&path) = paths.get(event_idx) {
                if let Some(ev) = doc.get_event_at_part_path(self.selected_part_idx, path) {
                    if let Some(first_tm) = ev.text_markings.first() {
                        text_buf = first_tm.text.clone();
                        bold = first_tm.bold;
                        italic = first_tm.italic;
                        underline = first_tm.underline;
                        font_family = first_tm
                            .font_family
                            .clone()
                            .unwrap_or_else(|| "Sans-Serif".to_string());
                        font_size = first_tm.font_size.unwrap_or(12.0);
                        has_existing = true;
                    }
                }
            }
        }

        self.active_dialog = ActiveDialog::InsertTextMarking {
            event_idx,
            text_buf,
            bold,
            italic,
            underline,
            font_family,
            font_size,
            has_existing,
        };
    }
}

impl eframe::App for MnxEditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.duration_textures.is_none() {
            egui_extras::install_image_loaders(ui.ctx());
            self.duration_textures = Some(DurationTextures::load(ui.ctx()));
        }

        // Synchronize MIDI playback progress with the score selection bar
        let is_midi_playing = self.midi_player.is_playing();
        if is_midi_playing {
            ui.ctx().request_repaint();
            if let Some(playing_idx) = self.midi_player.current_event_idx() {
                if self.selected_event_idx != Some(playing_idx) {
                    self.selected_event_idx = Some(playing_idx);
                    self.selected_note_idx_in_event = None;
                    self.is_editing_fret = false;
                    self.editing_fret_buf.clear();
                    self.editing_string = None;
                }
            }
        }

        let total_events = self.active_part_event_count();
        let text_edit_id = egui::Id::new("yaml_source_editor");

        // Clamp selection if document changed
        if let Some(idx) = self.selected_event_idx {
            if total_events == 0 {
                self.selected_event_idx = None;
                self.selected_note_idx_in_event = None;
                self.is_editing_fret = false;
            } else if idx >= total_events {
                self.selected_event_idx = Some(total_events - 1);
                self.selected_note_idx_in_event = None;
                self.is_editing_fret = false;
            }
        }

        let editor_focused = self.show_yaml_panel && ui.memory(|m| m.has_focus(text_edit_id));

        let any_grid_connected = self.grid_input.active_controller_count() > 0;
        if self.live_play_dirty
            || self.last_live_play_just_play != self.grid_input.just_play_mode
            || self.last_live_play_controller_count != self.grid_input.active_controller_count()
        {
            self.sync_live_play_config();
        }

        // Poll Grid Controller MIDI Input
        let num_strings = self
            .active_part()
            .map(|p| p.num_strings() as u8)
            .unwrap_or(6);
        if any_grid_connected
            || self.grid_input.release_latch_start.is_some()
            || self.grid_input.active_calibration.is_some()
        {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(20));
        }

        let was_grid_mode = self.grid_input.is_grid_input_mode;
        let (latest_chord, should_finalize, _cal_done, live_events) =
            self.grid_input.poll(num_strings, std::time::Instant::now());
        if !live_events.is_empty() {
            self.handle_grid_live_play_events(live_events);
        }
        if !was_grid_mode && self.grid_input.is_grid_input_mode {
            self.is_editing_fret = false;
            self.editing_fret_buf.clear();
            self.editing_string = None;
            if self.selected_event_idx.is_none() && total_events > 0 {
                self.selected_event_idx = Some(0);
            }
        }
        if let Some(chord) = latest_chord {
            let e_idx = self.selected_event_idx.unwrap_or(0);
            let p_idx = self.selected_part_idx;
            self.selected_event_idx = Some(e_idx);
            self.selected_note_idx_in_event = None;
            if self.mutate_doc_interactive(|doc| {
                crate::yaml_editor::set_chord_on_event_in_doc_part(doc, p_idx, e_idx, &chord)
            }) {
                self.preview_selected_sound();
            }
        }
        if should_finalize {
            self.end_interactive_gesture();
            let cur_total = self.active_part_event_count();
            self.advance_to_next_note_in_grid_mode(cur_total);
        } else if was_grid_mode && !self.grid_input.is_grid_input_mode && self.interactive_gesture_active {
            self.end_interactive_gesture();
        }

        let event_ranges = std::sync::Arc::clone(&self.cached_event_ranges);
        let total_events = self.active_part_event_count();

        self.handle_keyboard_input(ui, editor_focused, &event_ranges, total_events);

        // 1. Top Menu Bar (Updated with file operations & Duration Toolbar)
        self.render_top_panel(ui, &event_ranges, total_events);

        // 2. Bottom Panel for Errors (only shown if errors exist)
        self.render_error_panel(ui);

        // 3. Right Pane: YAML Text Editor (optional)
        if self.show_yaml_panel {
            self.render_yaml_panel(ui, text_edit_id, &event_ranges);
        }

        // 4. Left Pane: Render Output
        self.render_central_panel(ui);

        // 5. Modal Dialogs (Create Tuplet, Confirm Remove Tuplet)
        self.render_dialogs(ui.ctx());
    }
}
