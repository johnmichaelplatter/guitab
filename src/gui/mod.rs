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
            square_img.put_pixel(offset_x + x, offset_y + y, *pixel);
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
    pub(crate) selected_event_idx: Option<usize>,             // Selected note/beat index
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
    pub(crate) is_mpe_mode: bool, // Toggle button for multi-timbral vs MPE output
    pub(crate) midi_player: crate::midi::MidiPlayer, // Real-time MIDI sequencer playback engine
    pub(crate) midi_status_msg: Option<String>, // Status or error message from MIDI playback
    pub active_dialog: ActiveDialog, // Active modal dialog
    pub(crate) undo_stack: Vec<String>,
    pub(crate) redo_stack: Vec<String>,
    pub(crate) clipboard: Option<ClipboardData>,
    pub is_editing_lyric: bool,
    pub editing_lyric_buf: String,
    pub view_mode: ViewMode,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Continuous,
    Layout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscardTarget {
    NewProject,
    OpenFile,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClipboardData {
    pub items: Vec<crate::mnx::SequenceItem>,
    pub time_signature: Option<crate::mnx::TimeSignature>,
    pub arpeggios: Vec<crate::mnx::Arpeggio>,
    pub tempos: Vec<crate::mnx::Tempo>,
    pub tempo_changes: Vec<crate::mnx::TempoChange>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActiveDialog {
    None,
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
    },
    Message {
        title: String,
        text: String,
    },
    ConfirmDiscardChanges {
        target: DiscardTarget,
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
        tunings_text: Vec<String>, // text input for step
        tunings_octave: Vec<i32>,  // octave
        is_new_tab: bool,
    },
    SaveInstrumentPreset {
        preset_name: String,
        error_msg: Option<String>,
        num_strings: u8,
        edo: u8,
        step_multiplier: u8,
        tunings_text: Vec<String>,
        tunings_octave: Vec<i32>,
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
    GridControllerSetup,
    CustomFretboardEditor(crate::fretboard_editor::FretboardEditorState),
}

impl Default for MnxEditorApp {
    fn default() -> Self {
        let initial_yaml = fs::read_to_string("examples/example.yaml")
            .unwrap_or_else(|_| String::from("# Could not load example.yaml"));

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
            selected_event_idx: None,
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
            is_mpe_mode: false,
            midi_player: crate::midi::MidiPlayer::new(),
            midi_status_msg: None,
            active_dialog: ActiveDialog::None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            clipboard: None,
            is_editing_lyric: false,
            editing_lyric_buf: String::new(),
            view_mode: ViewMode::Continuous,
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
        };

        app.process_and_render();
        app
    }
}

impl MnxEditorApp {
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
            let flattened = doc.get_flattened_event_paths();
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
        if let Some(doc) = &self.parsed_mnx {
            if let Some(port_idx) = self.selected_midi_port {
                let mut string_to_channel = std::collections::HashMap::new();
                let num_strings = doc.parts.first().map(|p| p.num_strings()).unwrap_or(6) as u8;
                for s in 1..=num_strings {
                    let default_ch = if self.is_mpe_mode { s + 1 } else { s };
                    let ch_1_based = self
                        .string_channel_map
                        .get(&s)
                        .copied()
                        .unwrap_or(default_ch);
                    let ch_0_based = (ch_1_based.saturating_sub(1)) % 16;
                    string_to_channel.insert(s, midly::num::u4::from(ch_0_based));
                }

                let start_event = if start_from_selected {
                    self.selected_event_idx
                } else {
                    Some(0)
                };

                let options = crate::midi::MidiOptions {
                    pitch_bend_range: self.pitch_bend_range,
                    string_to_channel,
                    default_bpm: 120.0,
                    ppq: midly::num::u15::from(480),
                    start_event_idx: start_event,
                    is_mpe: self.is_mpe_mode,
                };

                match self.midi_player.play(doc, options, port_idx) {
                    Ok(_) => {
                        let port_name = self
                            .midi_ports
                            .iter()
                            .find(|p| p.id == port_idx)
                            .map(|p| p.name.as_str())
                            .unwrap_or("MIDI device");
                        let mode_str = if self.is_mpe_mode { " (MPE)" } else { "" };
                        if let Some(start_idx) = start_event {
                            if start_idx > 0 {
                                self.midi_status_msg = Some(format!(
                                    "Playing from note {}{} on {}",
                                    start_idx + 1,
                                    mode_str,
                                    port_name
                                ));
                            } else {
                                self.midi_status_msg = Some(format!(
                                    "Playing from start{} on {}",
                                    mode_str, port_name
                                ));
                            }
                        } else {
                            self.midi_status_msg =
                                Some(format!("Playing{} on {}", mode_str, port_name));
                        }
                    }
                    Err(e) => {
                        self.midi_status_msg = Some(format!("Playback error: {}", e));
                    }
                }
            } else {
                self.midi_status_msg = Some("No MIDI output device selected.".to_string());
            }
        } else {
            self.midi_status_msg = Some("No valid document parsed to play.".to_string());
        }
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
        let part = match doc.parts.first() {
            Some(p) => p,
            None => return,
        };
        let port_idx = match self.selected_midi_port {
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

    pub fn sync_from_parsed_doc(&mut self, doc: MnxDocument) {
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

        // Sync pitch bend range from YAML if stored on the part
        if let Some(pb) = doc
            .parts
            .first()
            .and_then(|p| p.pitch_bend_range)
        {
            self.pitch_bend_range = pb.clamp(1, 48);
        }

        self.parsed_errors = errors;
        self.parsed_mnx = Some(doc);
    }

    pub fn apply_mutated_doc(&mut self, doc: MnxDocument) {
        if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
            self.update_yaml(new_yaml);
            self.cached_event_ranges = find_all_event_ranges(&self.yaml_text);
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
        self.cached_event_ranges = find_all_event_ranges(&self.yaml_text);
        match serde_saphyr::from_str::<MnxDocument>(&self.yaml_text) {
            Ok(doc) => {
                self.sync_from_parsed_doc(doc);
            }
            Err(e) => {
                self.parsed_mnx = None;
                self.parsed_errors = Vec::new();
                self.validation_errors = vec![format!("YAML PARSE ERROR:\n{}", e)];
            }
        }
    }

    pub fn set_pitch_bend_range_in_yaml(&mut self, pb_range: u8) {
        let clamped = pb_range.clamp(1, 48);
        self.pitch_bend_range = clamped;
        self.mutate_doc(|doc| {
            let part = doc.parts.first_mut()?;
            part.pitch_bend_range = Some(clamped);
            if part.name.is_none() {
                part.name = Some("Guitar".to_string());
            }
            Some(())
        });
    }

    pub fn is_modified(&self) -> bool {
        self.yaml_text != self.saved_yaml
    }

    pub fn prompt_new_tab_instrument_setup(&mut self) {
        let mut tunings_text = Vec::new();
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
            tunings_octave.push(octave);
        }
        self.active_dialog = ActiveDialog::InstrumentSetup {
            num_strings: 6,
            edo: 12,
            step_multiplier: 1,
            tunings_text,
            tunings_octave,
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
        self.selected_event_idx = Some(0);
        self.selected_note_idx_in_event = None;
        self.is_editing_fret = false;
        self.editing_fret_buf.clear();
        self.editing_string = None;
        self.cached_event_ranges = find_all_event_ranges(&self.yaml_text);
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

    pub(crate) fn perform_open_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("YAML Documents", &["yaml", "yml"])
            .pick_file()
        {
            match fs::read_to_string(&path) {
                Ok(content) => {
                    self.update_yaml(content.clone());
                    self.saved_yaml = content;
                    self.current_file_path = Some(path);
                    self.selected_event_idx = None;
                    self.selected_note_idx_in_event = None;
                    self.is_editing_fret = false;
                    self.editing_fret_buf.clear();
                    self.editing_string = None;
                    self.process_and_render();
                }
                Err(e) => {
                    self.validation_errors = vec![format!("Failed to read file: {}", e)];
                }
            }
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
            if let Some(doc) = &self.parsed_mnx {
                let pages = crate::layout::compute_score_layout(doc, 0.0);
                if let Err(e) = crate::pdf_export::export_score_to_pdf(doc, &pages, &path) {
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
            let paths = doc.get_flattened_event_paths();
            if let Some(&path) = paths.get(event_idx) {
                if let Some(ev) = doc.get_event_at_path(path) {
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
            .parsed_mnx
            .as_ref()
            .and_then(|d| d.parts.first())
            .map(|p| p.num_strings() as u8)
            .unwrap_or(6);
        let any_grid_connected = self.grid_input.active_controller_count() > 0;
        if any_grid_connected
            || self.grid_input.release_latch_start.is_some()
            || matches!(self.active_dialog, ActiveDialog::GridControllerSetup)
        {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(20));
        }

        let was_grid_mode = self.grid_input.is_grid_input_mode;
        let (latest_chord, should_finalize, _cal_done) =
            self.grid_input.poll(num_strings, std::time::Instant::now());
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
            self.selected_event_idx = Some(e_idx);
            self.selected_note_idx_in_event = None;
            if self
                .mutate_doc(|doc| {
                    if crate::yaml_editor::set_chord_on_event_in_doc(doc, e_idx, &chord) {
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
