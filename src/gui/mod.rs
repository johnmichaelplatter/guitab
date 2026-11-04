//! GUI Module for MNX Tablature Editor
pub mod actions;
pub mod clipboard;
pub mod dialogs;
pub mod input;
pub mod panels;
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

pub struct MnxEditorApp {
    pub needs_scroll: bool,
    pub(crate) yaml_text: String,
    pub(crate) saved_yaml: String,
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
    pub(crate) part_midi_ports: std::collections::HashMap<usize, usize>, // Per-part MIDI port id
    pub(crate) part_string_channel_maps:
        std::collections::HashMap<usize, std::collections::HashMap<u8, u8>>, // Per-part string -> channel
    pub(crate) string_channel_dialog_part_idx: usize, // Selected part in String Channels popup
    pub(crate) is_mpe_mode: bool, // Toggle button for multi-timbral vs MPE output
    pub(crate) midi_player: crate::midi::MidiPlayer, // Real-time MIDI sequencer playback engine
    pub(crate) midi_status_msg: Option<String>, // Status or error message from MIDI playback
    pub active_dialog: ActiveDialog, // Active modal dialog
    pub(crate) instrument_setup_name_buf: String, // Name buffer for InstrumentSetup dialog
    pub(crate) is_adding_instrument_in_dialog: bool, // True when InstrumentSetup is adding a new part
    pub(crate) show_string_channels_popup: bool, // Toggle state for String Channels dropdown panel
    pub(crate) show_grid_controllers_popup: bool, // Toggle state for Grid Controllers dropdown panel
    pub(crate) undo_stack: Vec<String>,
    pub(crate) redo_stack: Vec<String>,
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
    pub(crate) cached_event_ranges: Vec<crate::yaml_editor::EventRange>,
    pub(crate) current_grid: String,
    pub(crate) instrument_presets: Vec<crate::presets::InstrumentPreset>,
    pub(crate) preset_status_msg: Option<String>,
    pub(crate) midi_input_ports: Vec<crate::midi::MidiPortInfo>,
    pub(crate) grid_input: crate::grid_input::GridInputManager,
    pub(crate) active_grid_live_notes:
        std::collections::HashMap<(u8, i32), (midly::num::u4, midly::num::u7)>,
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
    InstrumentSetup {
        num_strings: u8,
        edo: u8,
        step_multiplier: u8,
        tuning_mode: InstrumentTuningMode,
        tunings_text: Vec<String>, // In EDO mode: step name; in RatioCents mode: ratio or cents (e.g. "4/3" or "498.0")
        tunings_name: Vec<String>, // 53-EDO recommended or user-specified ups/downs name (e.g. "G")
        tunings_octave: Vec<i32>,  // octave
        show_custom_fretboard: bool,
        fretboard_editor: crate::fretboard_editor::FretboardEditorState,
        is_new_tab: bool,
    },
    SaveInstrumentPreset {
        preset_name: String,
        error_msg: Option<String>,
        num_strings: u8,
        edo: u8,
        step_multiplier: u8,
        tuning_mode: InstrumentTuningMode,
        tunings_text: Vec<String>,
        tunings_name: Vec<String>,
        tunings_octave: Vec<i32>,
        show_custom_fretboard: bool,
        fretboard_editor: crate::fretboard_editor::FretboardEditorState,
        is_new_tab: bool,
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
        let tunings = vec![
            crate::mnx::StringTuning {
                string: 1,
                pitch: crate::mnx::Pitch {
                    step: "E".to_string(),
                    octave: 4,
                    alter: None,
                    ..Default::default()
                },
            },
            crate::mnx::StringTuning {
                string: 2,
                pitch: crate::mnx::Pitch {
                    step: "B".to_string(),
                    octave: 3,
                    alter: None,
                    ..Default::default()
                },
            },
            crate::mnx::StringTuning {
                string: 3,
                pitch: crate::mnx::Pitch {
                    step: "G".to_string(),
                    octave: 3,
                    alter: None,
                    ..Default::default()
                },
            },
            crate::mnx::StringTuning {
                string: 4,
                pitch: crate::mnx::Pitch {
                    step: "D".to_string(),
                    octave: 3,
                    alter: None,
                    ..Default::default()
                },
            },
            crate::mnx::StringTuning {
                string: 5,
                pitch: crate::mnx::Pitch {
                    step: "A".to_string(),
                    octave: 2,
                    alter: None,
                    ..Default::default()
                },
            },
            crate::mnx::StringTuning {
                string: 6,
                pitch: crate::mnx::Pitch {
                    step: "E".to_string(),
                    octave: 2,
                    alter: None,
                    ..Default::default()
                },
            },
        ];

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
            part_midi_ports: std::collections::HashMap::new(),
            part_string_channel_maps: std::collections::HashMap::new(),
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
            is_adding_instrument_in_dialog: false,
            show_string_channels_popup: false,
            show_grid_controllers_popup: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
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
            cached_event_ranges: Vec::new(),
            current_grid: String::from("quarter"),
            instrument_presets: crate::presets::load_presets_with_user_config(),
            preset_status_msg: None,
            midi_input_ports: input_ports,
            grid_input: crate::grid_input::GridInputManager::default(),
            active_grid_live_notes: std::collections::HashMap::new(),
        };

        app.process_and_render();
        app
    }
}

impl MnxEditorApp {
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
        self.part_string_channel_maps
            .insert(self.selected_part_idx, self.string_channel_map.clone());
        if let Some(p) = self.selected_midi_port {
            self.part_midi_ports.entry(self.selected_part_idx).or_insert(p);
        }

        if self.selected_part_idx != new_idx {
            self.selected_part_idx = new_idx;
            self.selected_note_idx_in_event = None;
            self.is_editing_fret = false;
            self.editing_fret_buf.clear();
            self.editing_string = None;
            self.pending_slur = None;
        }

        self.string_channel_map = self
            .part_string_channel_maps
            .get(&self.selected_part_idx)
            .cloned()
            .unwrap_or_default();
        if let Some(&port) = self.part_midi_ports.get(&self.selected_part_idx) {
            self.selected_midi_port = Some(port);
        }
        if let Some(pb) = self
            .parsed_mnx
            .as_ref()
            .and_then(|d| d.parts.get(self.selected_part_idx))
            .and_then(|p| p.pitch_bend_range)
        {
            self.pitch_bend_range = pb.clamp(1, 48);
        }

        self.cached_event_ranges =
            find_event_ranges_for_part(&self.yaml_text, self.selected_part_idx);
        let total_events = self.cached_event_ranges.len();
        if let Some(ev_idx) = self.selected_event_idx {
            if total_events == 0 {
                self.selected_event_idx = None;
            } else if ev_idx >= total_events {
                self.selected_event_idx = Some(total_events - 1);
            }
        }
        self.selection_anchor = None;
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

    pub fn push_undo_state(&mut self) {
        if self.undo_stack.last() != Some(&self.yaml_text) {
            self.undo_stack.push(self.yaml_text.clone());
            self.redo_stack.clear();
        }
    }

    pub fn update_yaml(&mut self, new_yaml: String) {
        if self.yaml_text != new_yaml {
            self.push_undo_state();
            self.yaml_text = new_yaml;
        }
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack.push(self.yaml_text.clone());
            self.yaml_text = prev;
            self.process_and_render();
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(self.yaml_text.clone());
            self.yaml_text = next;
            self.process_and_render();
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
                        for ev in seq.flattened_events() {
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
        if let Some(doc) = &self.parsed_mnx {
            let default_port_opt = self.selected_midi_port;
            if default_port_opt.is_none() && self.part_midi_ports.is_empty() {
                self.midi_status_msg = Some("No MIDI output device selected.".to_string());
                return;
            }

            let mut part_configs = Vec::with_capacity(doc.parts.len());
            let mut port_channel_cursor: std::collections::HashMap<usize, u8> =
                std::collections::HashMap::new();

            for (p_idx, part) in doc.parts.iter().enumerate() {
                let Some(port_idx) = self
                    .part_midi_ports
                    .get(&p_idx)
                    .copied()
                    .or(default_port_opt)
                else {
                    continue;
                };

                let num_strings = part.num_strings() as u8;
                let base_offset = *port_channel_cursor.entry(port_idx).or_insert(0);
                let mut string_to_channel = std::collections::HashMap::new();

                let custom_map = if p_idx == self.selected_part_idx {
                    Some(&self.string_channel_map)
                } else {
                    self.part_string_channel_maps.get(&p_idx)
                };

                for s in 1..=num_strings {
                    let default_ch = if self.is_mpe_mode {
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

                let pb_range = part.pitch_bend_range.unwrap_or(self.pitch_bend_range);
                part_configs.push(crate::midi::PartMidiConfig {
                    part_idx: p_idx,
                    port_index: port_idx,
                    pitch_bend_range: pb_range,
                    string_to_channel,
                    is_mpe: self.is_mpe_mode,
                });
            }

            if part_configs.is_empty() {
                self.midi_status_msg = Some("No MIDI output device selected.".to_string());
                return;
            }

            let start_event = if start_from_selected {
                self.selected_event_idx
            } else {
                Some(0)
            };

            let primary_port = part_configs[0].port_index;
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
            match self.midi_player.play_multi_part(
                doc,
                part_configs,
                120.0,
                midly::num::u15::from(480),
                self.selected_part_idx,
                start_event,
            ) {
                Ok(_) => {
                    let port_name = self
                        .midi_ports
                        .iter()
                        .find(|p| p.id == primary_port)
                        .map(|p| p.name.as_str())
                        .unwrap_or("MIDI device");
                    let mode_str = if self.is_mpe_mode { " (MPE)" } else { "" };
                    if let Some(start_idx) = start_event {
                        if start_idx > 0 {
                            self.midi_status_msg = Some(format!(
                                "Playing{} from note {}{} on {}",
                                solo_prefix,
                                start_idx + 1,
                                mode_str,
                                port_name
                            ));
                        } else {
                            self.midi_status_msg = Some(format!(
                                "Playing{} from start{} on {}",
                                solo_prefix, mode_str, port_name
                            ));
                        }
                    } else {
                        self.midi_status_msg =
                            Some(format!("Playing{}{} on {}", solo_prefix, mode_str, port_name));
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
                    let old_total = self.cached_event_ranges.len();
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
                    let new_total = self.cached_event_ranges.len();
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
        let new_total = self.cached_event_ranges.len();
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

    pub fn preview_selected_sound(&mut self) {
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
        let port_idx = match self
            .part_midi_ports
            .get(&self.selected_part_idx)
            .copied()
            .or(self.selected_midi_port)
        {
            Some(p) => p,
            None => return,
        };

        // Find the event
        let mut cur_e = 0;
        let mut target_event = None;
        'find_ev: for m in &part.measures {
            for seq in &m.sequences {
                for ev in seq.flattened_events() {
                    if cur_e == e_idx {
                        target_event = Some(ev);
                        break 'find_ev;
                    }
                    cur_e += 1;
                }
            }
        }

        let event = match target_event {
            Some(ev) => ev,
            None => return,
        };

        if event.notes.is_empty() {
            self.midi_player.stop();
            return;
        }

        let freq_table = crate::tuning_table::FretboardFrequencyTable::from_doc_part(doc, part);
        let mut notes_to_play = Vec::new();

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
                let default_ch = if self.is_mpe_mode { string + 1 } else { string };
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
                        crate::midi::calculate_pitch_bend(semitone_diff, self.pitch_bend_range);

                    notes_to_play.push(crate::midi::NotePreview {
                        channel,
                        key: midly::num::u7::from(base_note.min(127)),
                        pitch_bend: Some(pitch_bend_val),
                        velocity: midly::num::u7::from(100),
                    });
                }
            }
        }

        if !notes_to_play.is_empty() {
            let num_strings = part.num_strings() as u8;
            let _ = self.midi_player.play_preview(
                notes_to_play,
                port_idx,
                400,
                self.is_mpe_mode,
                num_strings,
                self.pitch_bend_range,
            );
        }
    }

    /// Handles live note-on/note-off events from grid controllers in "Just Play" mode,
    /// looking up the exact pitch via `FretboardFrequencyTable` and streaming to MIDI output.
    pub fn handle_grid_live_play_events(
        &mut self,
        events: Vec<crate::grid_input::GridLivePlayEvent>,
    ) {
        let port_idx = match self
            .part_midi_ports
            .get(&self.selected_part_idx)
            .copied()
            .or(self.selected_midi_port)
        {
            Some(p) => p,
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

        let freq_table = crate::tuning_table::FretboardFrequencyTable::from_doc_part(doc, part);
        let num_strings = part.num_strings() as u8;

        for ev in events {
            let default_ch = if self.is_mpe_mode {
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
                if let Some(midi_note_frac) =
                    freq_table.string_fret_to_midi_note_number(part, ev.string, ev.fret)
                {
                    let base_note = midi_note_frac.round() as u8;
                    let semitone_diff = (midi_note_frac - (base_note as f64)) as f32;
                    let pitch_bend_val = crate::midi::calculate_pitch_bend(
                        semitone_diff,
                        self.pitch_bend_range,
                    );
                    let key = midly::num::u7::from(base_note.min(127));
                    let vel = midly::num::u7::from(ev.velocity.clamp(1, 127));

                    let _ = self.midi_player.send_live_note_on(
                        port_idx,
                        channel,
                        key,
                        vel,
                        Some(pitch_bend_val),
                        self.is_mpe_mode,
                        num_strings,
                        self.pitch_bend_range,
                    );
                    self.active_grid_live_notes
                        .insert((ev.string, ev.fret), (channel, key));
                }
            } else {
                if let Some((ch, key)) = self.active_grid_live_notes.remove(&(ev.string, ev.fret)) {
                    let _ = self.midi_player.send_live_note_off(port_idx, ch, key);
                } else if let Some(midi_note_frac) =
                    freq_table.string_fret_to_midi_note_number(part, ev.string, ev.fret)
                {
                    let base_note = midi_note_frac.round() as u8;
                    let key = midly::num::u7::from(base_note.min(127));
                    let _ = self.midi_player.send_live_note_off(port_idx, channel, key);
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

        // Sync pitch bend range from YAML if stored on the active part
        if let Some(pb) = doc
            .parts
            .get(self.selected_part_idx)
            .or_else(|| doc.parts.first())
            .and_then(|p| p.pitch_bend_range)
        {
            self.pitch_bend_range = pb.clamp(1, 48);
        }

        self.parsed_errors = errors;
        self.parsed_mnx = Some(doc);
        self.cached_event_ranges =
            find_event_ranges_for_part(&self.yaml_text, self.selected_part_idx);
    }

    pub fn apply_mutated_doc(&mut self, mut doc: MnxDocument) {
        doc.ensure_measure_parity();
        doc.clean_unneeded_fret_fields();
        doc.clean_dangling_slides();
        if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
            self.update_yaml(new_yaml);
            self.sync_from_parsed_doc(doc);
        }
    }

    pub fn apply_mutated_doc_and_clean_ties(&mut self, mut doc: MnxDocument) {
        doc.clean_dangling_ties();
        self.apply_mutated_doc(doc);
    }

    pub fn mutate_doc<F, R>(&mut self, f: F) -> Option<R>
    where
        F: FnOnce(&mut MnxDocument) -> Option<R>,
    {
        let mut doc = self
            .parsed_mnx
            .take()
            .or_else(|| serde_saphyr::from_str::<MnxDocument>(&self.yaml_text).ok())?;
        match f(&mut doc) {
            Some(res) => {
                self.apply_mutated_doc(doc);
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
        let mut doc = self
            .parsed_mnx
            .take()
            .or_else(|| serde_saphyr::from_str::<MnxDocument>(&self.yaml_text).ok())?;
        match f(&mut doc) {
            Some(res) => {
                doc.clean_dangling_ties();
                self.apply_mutated_doc(doc);
                Some(res)
            }
            None => {
                self.parsed_mnx = Some(doc);
                None
            }
        }
    }

    pub fn process_and_render(&mut self) {
        match serde_saphyr::from_str::<MnxDocument>(&self.yaml_text) {
            Ok(mut doc) => {
                doc.ensure_measure_parity();
                let had_missing_edo = doc.parts.iter().any(|p| {
                    p.ext.micro_fret_divisions.is_none() && p.detect_tuning_edo().is_some()
                });
                doc.clean_unneeded_fret_fields();
                if had_missing_edo {
                    if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
                        self.update_yaml(new_yaml);
                    }
                }
                self.sync_from_parsed_doc(doc);
            }
            Err(e) => {
                self.cached_event_ranges =
                    find_event_ranges_for_part(&self.yaml_text, self.selected_part_idx);
                self.parsed_mnx = None;
                self.parsed_errors = Vec::new();
                self.validation_errors = vec![format!("YAML PARSE ERROR:\n{}", e)];
            }
        }
    }

    pub fn set_pitch_bend_range_in_yaml(&mut self, pb_range: u8) {
        let clamped = pb_range.clamp(1, 48);
        self.pitch_bend_range = clamped;
        let part_idx = self.selected_part_idx;
        self.mutate_doc(|doc| {
            let part = doc.parts.get_mut(part_idx)?;
            part.pitch_bend_range = Some(clamped);
            if part.name.is_none() {
                part.name = Some(part.display_name(part_idx));
            }
            Some(())
        });
    }

    pub fn is_modified(&self) -> bool {
        self.yaml_text != self.saved_yaml
    }

    pub fn open_instrument_setup_dialog(&mut self, show_custom_fretboard: bool) {
        self.is_adding_instrument_in_dialog = false;
        let part_idx = self.selected_part_idx;
        let doc = self.parsed_mnx.as_ref();
        let part = doc.and_then(|d| d.parts.get(part_idx).or_else(|| d.parts.first()));
        self.instrument_setup_name_buf = part
            .map(|p| p.display_name(part_idx))
            .unwrap_or_else(|| "Guitar".to_string());
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
                    _ => ("G", 0),
                };
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

        self.active_dialog = ActiveDialog::InstrumentSetup {
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
        };
    }

    pub fn open_add_instrument_dialog(&mut self) {
        self.is_adding_instrument_in_dialog = true;
        let next_num = self
            .parsed_mnx
            .as_ref()
            .map(|d| d.parts.len() + 1)
            .unwrap_or(2);
        self.instrument_setup_name_buf = format!("Guitar {}", next_num);

        let mut tunings_text = Vec::new();
        let mut tunings_name = Vec::new();
        let mut tunings_octave = Vec::new();
        for s in 1..=15 {
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
                _ => ("G", 0),
            };
            tunings_text.push(step.to_string());
            tunings_name.push(step.to_string());
            tunings_octave.push(octave);
        }

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

        self.active_dialog = ActiveDialog::InstrumentSetup {
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
            is_new_tab: false,
        };
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
                let ts = crate::render::get_time_signature_for_measure(Some(&doc.global), m_idx);
                let expected_beats = (ts.count as f64) * (4.0 / (ts.unit as f64));
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
                ext: crate::mnx::PartExt {
                    micro_fret_divisions: Some(edo),
                    fret_step_multiplier: Some(step_multiplier),
                    pitch_bend_range: None,
                    capo: None,
                    tuning: Some(tunings),
                    custom_frets,
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
        self.part_midi_ports.remove(&part_idx);
        self.part_string_channel_maps.remove(&part_idx);

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
        self.select_part(next_sel);
    }

    pub fn prompt_new_tab_instrument_setup(&mut self) {
        self.is_adding_instrument_in_dialog = false;
        self.instrument_setup_name_buf = "Guitar".to_string();
        let mut tunings_text = Vec::new();
        let mut tunings_name = Vec::new();
        let mut tunings_octave = Vec::new();
        for s in 1..=15 {
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
                _ => ("G", 0),
            };
            tunings_text.push(step.to_string());
            tunings_name.push(step.to_string());
            tunings_octave.push(octave);
        }
        self.active_dialog = ActiveDialog::InstrumentSetup {
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
                "D".to_string(),
                4,
                293.67,
            ),
            is_new_tab: true,
        };
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

        let new_yaml = serde_saphyr::to_string(&doc).unwrap_or_default();

        self.update_yaml(new_yaml.clone());
        self.saved_yaml = new_yaml;
        self.current_file_path = None;
        self.selected_part_idx = 0;
        self.layout_view_tab = LayoutViewTab::Ensemble;
        self.ensemble_visible_parts = vec![true];
        self.selected_event_idx = Some(0);
        self.selected_note_idx_in_event = None;
        self.is_editing_fret = false;
        self.editing_fret_buf.clear();
        self.editing_string = None;
        self.sync_from_parsed_doc(doc);
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

    pub(crate) fn perform_open_file(&mut self) -> bool {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("YAML Documents", &["yaml", "yml"])
            .pick_file()
        {
            match fs::read_to_string(&path) {
                Ok(content) => {
                    self.update_yaml(content.clone());
                    self.saved_yaml = content;
                    self.current_file_path = Some(path);
                    self.selected_part_idx = 0;
                    self.layout_view_tab = LayoutViewTab::Ensemble;
                    self.selected_event_idx = None;
                    self.selected_note_idx_in_event = None;
                    self.is_editing_fret = false;
                    self.editing_fret_buf.clear();
                    self.editing_string = None;
                    self.process_and_render();
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
            if let Err(e) = fs::write(&path, &self.yaml_text) {
                self.validation_errors = vec![format!("Failed to save file: {}", e)];
                false
            } else {
                self.saved_yaml = self.yaml_text.clone();
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
            if let Err(e) = fs::write(&path, &self.yaml_text) {
                self.validation_errors = vec![format!("Failed to save file: {}", e)];
                false
            } else {
                // Update the current path so subsequent "Saves" go here
                self.current_file_path = Some(path);
                self.saved_yaml = self.yaml_text.clone();
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
            match crate::mnx::export_yaml_to_mnx_json(&self.yaml_text) {
                Ok(json_str) => {
                    if let Err(e) = fs::write(&path, json_str) {
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

        let event_ranges = self.cached_event_ranges.clone();
        let total_events = event_ranges.len();
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

        // Poll Grid Controller MIDI Input
        let num_strings = self
            .active_part()
            .map(|p| p.num_strings() as u8)
            .unwrap_or(6);
        let any_grid_connected = self.grid_input.active_controller_count() > 0;
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
            if self
                .mutate_doc(|doc| {
                    if crate::yaml_editor::set_chord_on_event_in_doc_part(doc, p_idx, e_idx, &chord)
                    {
                        Some(())
                    } else {
                        None
                    }
                })
                .is_some()
            {
                self.preview_selected_sound();
            }
        }
        if should_finalize {
            let cur_total = self.cached_event_ranges.len();
            self.advance_to_next_note_in_grid_mode(cur_total);
        }

        let event_ranges = self.cached_event_ranges.clone();
        let total_events = event_ranges.len();

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
