use crate::yaml_editor::*;
use eframe::egui::{self, Color32};

use crate::mnx::MnxDocument;
use crate::render;
use crate::validate;
use std::fs;

const WHOLE_PNG: &[u8] = include_bytes!("../assets/whole.png");
const HALF_PNG: &[u8] = include_bytes!("../assets/half.png");
const QUARTER_PNG: &[u8] = include_bytes!("../assets/quarter.png");
const EIGHTH_PNG: &[u8] = include_bytes!("../assets/eighth.png");
const SIXTEENTH_PNG: &[u8] = include_bytes!("../assets/16th.png");
const THIRTYSECOND_PNG: &[u8] = include_bytes!("../assets/32nd.png");
const DOT_PNG: &[u8] = include_bytes!("../assets/dot.png");

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
    yaml_text: String,
    parsed_mnx: Option<MnxDocument>,
    parsed_errors: Vec<validate::ValidationError>,
    validation_errors: Vec<String>,
    current_file_path: Option<std::path::PathBuf>, // Tracks the active file
    selected_event_idx: Option<usize>,             // Selected note/beat index
    selected_note_idx_in_event: Option<usize>, // Selected sub-note within chord/event (None = all)
    is_editing_fret: bool,                     // In-place fret number editing mode
    editing_fret_buf: String,                  // Text buffer for the new fret number
    editing_string: Option<u8>,                // Currently focused string row in edit mode
    duration_textures: Option<DurationTextures>, // Note duration icons
    bpm_buf: String,                           // Text buffer for BPM input
    midi_ports: Vec<crate::midi::MidiPortInfo>, // Available MIDI output devices
    selected_midi_port: Option<usize>,         // Selected MIDI port id
    pitch_bend_range: u8,                      // Pitchbend range (1-48, default 2)
    string_channel_map: std::collections::HashMap<u8, u8>, // String (1..n) -> MIDI channel (1..16)
    is_mpe_mode: bool,                         // Toggle button for multi-timbral vs MPE output
    midi_player: crate::midi::MidiPlayer,      // Real-time MIDI sequencer playback engine
    midi_status_msg: Option<String>,           // Status or error message from MIDI playback
    pub active_dialog: ActiveDialog,           // Active modal dialog
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActiveDialog {
    None,
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
}

impl Default for MnxEditorApp {
    fn default() -> Self {
        let initial_yaml = fs::read_to_string("example.yaml")
            .unwrap_or_else(|_| String::from("# Could not load example.yaml"));

        let ports = crate::midi::get_midi_output_ports();
        let selected_port = ports.first().map(|p| p.id);

        let mut app = Self {
            yaml_text: initial_yaml,
            parsed_mnx: None,
            parsed_errors: Vec::new(),
            validation_errors: Vec::new(),
            current_file_path: None,
            selected_event_idx: None,
            selected_note_idx_in_event: None,
            is_editing_fret: false,
            editing_fret_buf: String::new(),
            editing_string: None,
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
        };

        app.process_and_render();
        app
    }
}

impl MnxEditorApp {
    pub fn get_selected_event_duration(&self) -> Option<crate::mnx::Duration> {
        let e_idx = self.selected_event_idx?;
        let doc = self.parsed_mnx.as_ref()?;
        let part = doc.parts.first()?;
        let mut cur_e = 0;
        for m in &part.measures {
            for seq in &m.sequences {
                for ev in seq.flattened_events() {
                    if cur_e == e_idx {
                        return Some(ev.duration.clone());
                    }
                    cur_e += 1;
                }
            }
        }
        None
    }

    pub fn is_selected_event_rest(&self) -> bool {
        let e_idx = match self.selected_event_idx {
            Some(i) => i,
            None => return false,
        };
        let doc = match &self.parsed_mnx {
            Some(d) => d,
            None => return false,
        };
        let part = match doc.parts.first() {
            Some(p) => p,
            None => return false,
        };
        let mut cur_e = 0;
        for m in &part.measures {
            for seq in &m.sequences {
                for ev in seq.flattened_events() {
                    if cur_e == e_idx {
                        return ev.rest.is_some();
                    }
                    cur_e += 1;
                }
            }
        }
        false
    }

    pub fn is_selected_event_in_tuplet(&self) -> bool {
        let e_idx = match self.selected_event_idx {
            Some(i) => i,
            None => return false,
        };
        let doc = match &self.parsed_mnx {
            Some(d) => d,
            None => return false,
        };
        doc.find_event_tuplet_info(e_idx).is_some()
    }

    pub fn convert_selected_to_rest(&mut self) {
        if let Some(e_idx) = self.selected_event_idx {
            if let Some(new_yaml) =
                crate::yaml_editor::convert_event_to_rest(&self.yaml_text, e_idx)
            {
                self.yaml_text = new_yaml;
                self.process_and_render();
            }
            if self.is_editing_fret {
                self.is_editing_fret = false;
                self.editing_fret_buf.clear();
            }
        }
    }

    pub fn handle_tuplet_button_click(&mut self) {
        if let Some(e_idx) = self.selected_event_idx {
            let is_in_tuplet = self.is_selected_event_in_tuplet();
            if !is_in_tuplet {
                let current_dur = self.get_selected_event_duration();
                let base = current_dur
                    .map(|d| d.base)
                    .unwrap_or_else(|| "eighth".to_string());
                let from_select_mode = !self.is_editing_fret;
                self.active_dialog = ActiveDialog::CreateTuplet {
                    inner_multiple: 3,
                    outer_multiple: 2,
                    base,
                    from_select_mode,
                    event_idx: e_idx,
                };
            } else {
                if self.is_editing_fret {
                    if let Some((new_yaml, new_idx)) =
                        crate::yaml_editor::fill_tuplet_rests_and_add_blank(&self.yaml_text, e_idx)
                    {
                        self.yaml_text = new_yaml;
                        self.process_and_render();
                        self.selected_event_idx = Some(new_idx);
                        self.is_editing_fret = true;
                        self.editing_fret_buf.clear();
                        if let Some(s) = self.editing_string {
                            self.select_note_on_string(new_idx, s);
                        }
                    }
                } else {
                    self.active_dialog = ActiveDialog::ConfirmRemoveTuplet { event_idx: e_idx };
                }
            }
        }
    }

    pub fn change_selected_duration_base(&mut self, new_base: &str) {
        if let Some(e_idx) = self.selected_event_idx {
            let cur_dur =
                self.get_selected_event_duration()
                    .unwrap_or_else(|| crate::mnx::Duration {
                        multiple: None,
                        base: "quarter".to_string(),
                        dots: None,
                    });
            let new_duration = crate::mnx::Duration {
                multiple: None,
                base: new_base.to_string(),
                dots: cur_dur.dots,
            };
            if let Some(new_yaml) =
                update_event_duration_in_yaml(&self.yaml_text, e_idx, &new_duration)
            {
                self.yaml_text = new_yaml;
                self.process_and_render();
            }
        }
    }

    pub fn toggle_selected_duration_dot(&mut self) {
        if let Some(e_idx) = self.selected_event_idx {
            let cur_dur =
                self.get_selected_event_duration()
                    .unwrap_or_else(|| crate::mnx::Duration {
                        multiple: None,
                        base: "quarter".to_string(),
                        dots: None,
                    });
            let is_dotted = cur_dur.dots.unwrap_or(0) > 0
                || cur_dur.base.contains("dotted")
                || cur_dur.base.contains('.');
            let clean_base = cur_dur
                .base
                .replace("dotted-", "")
                .replace("-dotted", "")
                .replace("dotted", "")
                .replace('.', "");
            let new_duration = crate::mnx::Duration {
                multiple: None,
                base: if clean_base.is_empty() {
                    "quarter".to_string()
                } else {
                    clean_base
                },
                dots: if is_dotted { None } else { Some(1) },
            };
            if let Some(new_yaml) =
                update_event_duration_in_yaml(&self.yaml_text, e_idx, &new_duration)
            {
                self.yaml_text = new_yaml;
                self.process_and_render();
            }
        }
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

        let freq_table = crate::tuning_table::FretboardFrequencyTable::from_part_default(part);
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
            if let (Some(string), Some(fret)) = (note.string, note.fret) {
                let default_ch = if self.is_mpe_mode { string + 1 } else { string };
                let ch_1_based = self
                    .string_channel_map
                    .get(&string)
                    .copied()
                    .unwrap_or(default_ch);
                let ch_0_based = (ch_1_based.saturating_sub(1)) % 16;
                let channel = midly::num::u4::from(ch_0_based);

                if let Some(pos_state) = freq_table.get(string, fret) {
                    let midi_note_frac = pos_state.midi_note_number;
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

    fn select_note_on_string(&mut self, e_idx: usize, string_num: u8) {
        self.selected_note_idx_in_event = None;
        if let Some(doc) = &self.parsed_mnx {
            if let Some(part) = doc.parts.first() {
                let mut cur_e = 0;
                'find_n: for m in &part.measures {
                    for seq in &m.sequences {
                        for ev in seq.flattened_events() {
                            if cur_e == e_idx {
                                for (n_i, n) in ev.notes.iter().enumerate() {
                                    if n.string == Some(string_num) {
                                        self.selected_note_idx_in_event = Some(n_i);
                                        break;
                                    }
                                }
                                break 'find_n;
                            }
                            cur_e += 1;
                        }
                    }
                }
            }
        }
    }

    fn commit_fret_edit(&mut self) -> bool {
        let mut committed = false;
        if let Some(e_idx) = self.selected_event_idx {
            if let Some(cur_str) = self.editing_string {
                if let Ok(new_fret) = self.editing_fret_buf.trim().parse::<u8>() {
                    let new_pitch = self
                        .parsed_mnx
                        .as_ref()
                        .and_then(|d| d.parts.first())
                        .and_then(|part| calculate_pitch_for_string_fret(part, cur_str, new_fret));

                    if let Some(new_yaml) = set_or_add_note_on_string(
                        &self.yaml_text,
                        e_idx,
                        cur_str,
                        new_fret,
                        new_pitch.as_ref(),
                    ) {
                        self.yaml_text = new_yaml;
                        self.process_and_render();
                        self.select_note_on_string(e_idx, cur_str);
                        self.preview_selected_sound();
                        committed = true;
                    }
                }
            }
        }
        committed
    }

    fn handle_tuplet_right_arrow(&mut self, e_idx: usize, _total_events: usize) -> Option<usize> {
        let doc = self.parsed_mnx.as_ref()?;
        let info = doc.find_event_tuplet_info(e_idx)?;

        // Must be the final event of the tuplet
        if e_idx != info.event_range.end - 1 {
            return None;
        }

        // Must fill the final rhythmic duration (has notes or is a rest)
        let part = doc.parts.first()?;
        let flat_events = part
            .measures
            .iter()
            .flat_map(|m| m.sequences.iter())
            .flat_map(|s| s.flattened_events())
            .collect::<Vec<_>>();
        let cur_ev = flat_events.get(e_idx)?;
        if cur_ev.notes.is_empty() && cur_ev.rest.is_none() {
            return None;
        }

        let final_note_base = cur_ev.duration.base.clone();

        // Check if there is already an event after this tuplet in the same measure
        let mut cur_e = 0;
        let mut measure_idx = 0;
        let mut tuplet_is_last_in_measure = false;
        for (m_i, measure) in part.measures.iter().enumerate() {
            if let Some(seq) = measure.sequences.first() {
                let count = seq.flattened_events().len();
                if info.event_range.end <= cur_e + count {
                    measure_idx = m_i;
                    tuplet_is_last_in_measure = info.event_range.end == cur_e + count;
                    break;
                }
                cur_e += count;
            }
        }

        if !tuplet_is_last_in_measure {
            // There is already an event in this measure after the tuplet, move to it
            return Some(info.event_range.end);
        }

        // Tuplet is the last item in this measure.
        // Check measure capacity:
        let dur = crate::mnx::Duration {
            base: final_note_base.clone(),
            dots: None,
            multiple: None,
        };
        let dur_beats = crate::validate::duration_to_quarter_beats(&dur);
        let time_sig = crate::validate::get_time_signature_for_measure(doc, measure_idx);
        let max_beats = (time_sig.count as f64) * (4.0 / time_sig.unit as f64);
        let seq_beats = part.measures[measure_idx]
            .sequences
            .first()
            .map(|s| s.total_beats())
            .unwrap_or(0.0);

        let is_last_measure = measure_idx == part.measures.len().saturating_sub(1);

        if seq_beats + dur_beats <= max_beats + 1e-4 {
            // Measure has room: insert new non-tuple blank note after the tuplet in this measure
            if let Some((new_yaml, new_idx)) = crate::yaml_editor::insert_blank_note_after_tuplet(
                &self.yaml_text,
                e_idx,
                &final_note_base,
            ) {
                self.yaml_text = new_yaml;
                self.process_and_render();
                return Some(new_idx);
            }
        } else if is_last_measure {
            // Measure is full, and it's the last measure: add a new measure with blank note
            if let Some(new_yaml) = crate::yaml_editor::add_measure_to_yaml(&self.yaml_text, &dur) {
                self.yaml_text = new_yaml;
                self.process_and_render();
                let new_total = find_all_event_ranges(&self.yaml_text).len();
                return Some(new_total.saturating_sub(1));
            }
        } else {
            // Measure is full, but next measure exists: move to next measure's first event
            return Some(info.event_range.end);
        }

        None
    }

    fn advance_to_next_note_in_edit_mode(&mut self, cur_str: Option<u8>, total_events: usize) {
        if let Some(e_idx) = self.selected_event_idx {
            let next_idx = if let Some(tuplet_next) =
                self.handle_tuplet_right_arrow(e_idx, total_events)
            {
                tuplet_next
            } else {
                let at_final_event = e_idx == total_events.saturating_sub(1);
                if at_final_event {
                    let mut added_idx = None;
                    if let Some(doc) = &self.parsed_mnx {
                        if let Some(part) = doc.parts.first() {
                            if let Some(last_measure) = part.measures.last() {
                                if let Some(last_seq) = last_measure.sequences.first() {
                                    if let Some(last_event) =
                                        last_seq.flattened_events().last().copied()
                                    {
                                        let m_idx = part.measures.len() - 1;
                                        let final_dur = &last_event.duration;
                                        let dur_beats =
                                            validate::duration_to_quarter_beats(final_dur);
                                        let time_sig =
                                            validate::get_time_signature_for_measure(doc, m_idx);
                                        let max_beats =
                                            (time_sig.count as f64) * (4.0 / time_sig.unit as f64);
                                        let curr_beats: f64 = last_seq.total_beats();

                                        if curr_beats + dur_beats <= max_beats + 1e-4 {
                                            if let Some(new_yaml) = add_blank_note_to_last_measure(
                                                &self.yaml_text,
                                                final_dur,
                                            ) {
                                                self.yaml_text = new_yaml;
                                                self.process_and_render();
                                                let new_total =
                                                    find_all_event_ranges(&self.yaml_text).len();
                                                if new_total > 0 {
                                                    added_idx = Some(new_total - 1);
                                                }
                                            }
                                        } else {
                                            self.add_measure_to_end();
                                            let new_total =
                                                find_all_event_ranges(&self.yaml_text).len();
                                            if new_total > 0 {
                                                added_idx = Some(new_total - 1);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    added_idx.unwrap_or(e_idx)
                } else {
                    (e_idx + 1).min(total_events.saturating_sub(1))
                }
            };

            self.selected_event_idx = Some(next_idx);
            self.editing_string = cur_str;
            self.editing_fret_buf.clear();
            self.is_editing_fret = true;
            if let Some(s) = cur_str {
                self.select_note_on_string(next_idx, s);
            }
        }
    }

    fn retreat_to_prev_note_in_edit_mode(&mut self, cur_str: Option<u8>) {
        if let Some(e_idx) = self.selected_event_idx {
            if e_idx > 0 {
                let prev_idx = e_idx - 1;
                self.selected_event_idx = Some(prev_idx);
                self.editing_string = cur_str;
                self.editing_fret_buf.clear();
                self.is_editing_fret = true;
                if let Some(s) = cur_str {
                    self.select_note_on_string(prev_idx, s);
                }
            }
        }
    }

    fn get_string_for_note(&self, e_idx: usize, note_idx: usize) -> Option<u8> {
        let part = self.parsed_mnx.as_ref()?.parts.first()?;
        let mut cur_e = 0;
        for m in &part.measures {
            for seq in &m.sequences {
                for ev in seq.flattened_events() {
                    if cur_e == e_idx {
                        return ev.notes.get(note_idx).and_then(|n| n.string);
                    }
                    cur_e += 1;
                }
            }
        }
        None
    }

    fn find_primary_string_for_event(&self, e_idx: usize) -> Option<u8> {
        let part = self.parsed_mnx.as_ref()?.parts.first()?;
        let mut cur_e = 0;
        for m in &part.measures {
            for seq in &m.sequences {
                for ev in seq.flattened_events() {
                    if cur_e == e_idx {
                        return ev.notes.first().and_then(|n| n.string);
                    }
                    cur_e += 1;
                }
            }
        }
        None
    }

    fn event_has_note_on_string(&self, e_idx: usize, string_num: u8) -> bool {
        if let Some(doc) = &self.parsed_mnx {
            if let Some(part) = doc.parts.first() {
                let mut cur_e = 0;
                for m in &part.measures {
                    for seq in &m.sequences {
                        for ev in seq.flattened_events() {
                            if cur_e == e_idx {
                                return ev.notes.iter().any(|n| n.string == Some(string_num));
                            }
                            cur_e += 1;
                        }
                    }
                }
            }
        }
        false
    }

    fn event_has_any_notes(&self, e_idx: usize) -> bool {
        if let Some(doc) = &self.parsed_mnx {
            if let Some(part) = doc.parts.first() {
                let mut cur_e = 0;
                for m in &part.measures {
                    for seq in &m.sequences {
                        for ev in seq.flattened_events() {
                            if cur_e == e_idx {
                                return !ev.notes.is_empty();
                            }
                            cur_e += 1;
                        }
                    }
                }
            }
        }
        false
    }

    pub fn add_measure_to_end(&mut self) {
        let last_duration = self
            .parsed_mnx
            .as_ref()
            .and_then(|d| d.parts.first())
            .and_then(|p| p.measures.last())
            .and_then(|m| m.sequences.first())
            .and_then(|s| s.flattened_events().last().copied())
            .map(|e| e.duration.clone())
            .unwrap_or_else(|| crate::mnx::Duration {
                multiple: None,
                base: "quarter".to_string(),
                dots: None,
            });

        if let Some(new_yaml) = add_measure_to_yaml(&self.yaml_text, &last_duration) {
            self.yaml_text = new_yaml;
            self.process_and_render();

            let total_events = find_all_event_ranges(&self.yaml_text).len();
            if total_events > 0 {
                self.selected_event_idx = Some(total_events - 1);
            }
            self.selected_note_idx_in_event = None;
            self.is_editing_fret = false;
            self.editing_fret_buf.clear();
            self.editing_string = None;
        }
    }

    fn process_and_render(&mut self) {
        match serde_saphyr::from_str::<MnxDocument>(&self.yaml_text) {
            Ok(doc) => {
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
                    .flat_map(|m| m.directions.iter())
                    .find_map(|d| d.tempo.as_ref().map(|t| t.bpm));
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

                self.parsed_errors = errors;
                self.parsed_mnx = Some(doc);
            }
            Err(e) => {
                self.parsed_mnx = None;
                self.parsed_errors = Vec::new();
                self.validation_errors = vec![format!("YAML PARSE ERROR:\n{}", e)];
            }
        }
    }
    fn open_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("YAML Documents", &["yaml", "yml"])
            .pick_file()
        {
            match fs::read_to_string(&path) {
                Ok(content) => {
                    self.yaml_text = content;
                    self.current_file_path = Some(path);
                    self.selected_event_idx = None;
                    self.selected_note_idx_in_event = None;
                    self.is_editing_fret = false;
                    self.editing_fret_buf.clear();
                    self.process_and_render();
                }
                Err(e) => {
                    self.validation_errors = vec![format!("Failed to read file: {}", e)];
                }
            }
        }
    }

    fn save_file(&mut self) {
        // If we have a known file path, save to it. Otherwise, trigger "Save As".
        if let Some(path) = &self.current_file_path {
            if let Err(e) = fs::write(path, &self.yaml_text) {
                self.validation_errors = vec![format!("Failed to save file: {}", e)];
            }
        } else {
            self.save_as_file();
        }
    }

    fn save_as_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("YAML Documents", &["yaml", "yml"])
            .set_file_name("untitled.yaml")
            .save_file()
        {
            if let Err(e) = fs::write(&path, &self.yaml_text) {
                self.validation_errors = vec![format!("Failed to save file: {}", e)];
            } else {
                // Update the current path so subsequent "Saves" go here
                self.current_file_path = Some(path);
            }
        }
    }

    fn render_top_panel(
        &mut self,
        ui: &mut egui::Ui,
        event_ranges: &[EventRange],
        total_events: usize,
    ) {
        egui::Panel::top("top_panel").show(ui, |ui| {
            ui.horizontal(|ui| {
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

                if let Some(textures) = &self.duration_textures {
                    let icon_size = egui::vec2(22.0, 22.0);
                    let btn_size = egui::vec2(28.0, 28.0);

                    let make_icon_btn = |ui: &mut egui::Ui, texture: &egui::TextureHandle, is_active: bool, tooltip: &str| -> bool {
                        let img = egui::Image::new(texture).fit_to_exact_size(icon_size);
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
                            self.yaml_text = set_bpm_in_yaml(&self.yaml_text, new_bpm);
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

    fn render_error_panel(&mut self, ui: &mut egui::Ui) {
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

    fn render_yaml_panel(
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

    fn render_central_panel(&mut self, ui: &mut egui::Ui) {
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
                                selected_note_in_event: self.selected_note_idx_in_event,
                                is_editing_fret: self.is_editing_fret,
                                editing_fret_text: &self.editing_fret_buf,
                                editing_string: self.editing_string,
                            },
                        );
                        if add_bar_clicked {
                            self.add_measure_to_end();
                        } else if let Some((clicked_event, clicked_note)) = clicked {
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
                                    .map(|e| e.notes.is_empty())
                                    .unwrap_or(false);

                            if is_final_blank && clicked_event < total_events.saturating_sub(1) {
                                if let Some(new_yaml) = remove_last_blank_event(&self.yaml_text) {
                                    self.yaml_text = new_yaml;
                                    self.process_and_render();
                                }
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

    fn handle_keyboard_input(
        &mut self,
        ui: &mut egui::Ui,
        editor_focused: bool,
        event_ranges: &[EventRange],
        total_events: usize,
    ) {
        if self.active_dialog != ActiveDialog::None {
            return;
        }

        // In-place Fret Editing Key Handling
        if self.is_editing_fret {
            let esc_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));
            let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
            let backspace_pressed = ui.input(|i| i.key_pressed(egui::Key::Backspace));
            let delete_pressed = ui.input(|i| i.key_pressed(egui::Key::Delete));
            let up_pressed = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
            let down_pressed = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));
            let left_pressed = ui.input(|i| i.key_pressed(egui::Key::ArrowLeft));
            let right_pressed = ui.input(|i| i.key_pressed(egui::Key::ArrowRight));
            let r_pressed = ui.input(|i| i.key_pressed(egui::Key::R));

            let num_strings = self
                .parsed_mnx
                .as_ref()
                .and_then(|d| d.parts.first())
                .map(|p| p.num_strings())
                .unwrap_or(6) as u8;

            if r_pressed && self.selected_event_idx.is_some() {
                self.convert_selected_to_rest();
            } else if esc_pressed {
                self.is_editing_fret = false;
                self.editing_fret_buf.clear();
            } else if enter_pressed {
                self.commit_fret_edit();
                self.is_editing_fret = false;
                self.editing_fret_buf.clear();
            } else if left_pressed || right_pressed {
                let cur_str = self.editing_string;
                let e_idx_opt = self.selected_event_idx;
                let is_current_str_existing = if let (Some(e_idx), Some(s)) = (e_idx_opt, cur_str) {
                    self.event_has_note_on_string(e_idx, s)
                } else {
                    false
                };

                if !self.editing_fret_buf.trim().is_empty() {
                    self.commit_fret_edit();
                    self.editing_fret_buf.clear();

                    if right_pressed {
                        self.advance_to_next_note_in_edit_mode(cur_str, total_events);
                    } else if left_pressed {
                        self.retreat_to_prev_note_in_edit_mode(cur_str);
                    }
                } else if is_current_str_existing {
                    if let (Some(e_idx), Some(s)) = (e_idx_opt, cur_str) {
                        self.select_note_on_string(e_idx, s);
                        self.preview_selected_sound();
                    }
                    self.editing_fret_buf.clear();

                    if right_pressed {
                        self.advance_to_next_note_in_edit_mode(cur_str, total_events);
                    } else if left_pressed {
                        self.retreat_to_prev_note_in_edit_mode(cur_str);
                    }
                } else if right_pressed {
                    self.editing_fret_buf.clear();
                    if let Some(e_idx) = e_idx_opt {
                        if e_idx + 1 < total_events && self.event_has_any_notes(e_idx + 1) {
                            let next_idx = e_idx + 1;
                            self.selected_event_idx = Some(next_idx);
                            self.editing_string = cur_str;
                            self.is_editing_fret = true;
                            if let Some(s) = cur_str {
                                self.select_note_on_string(next_idx, s);
                            }
                        }
                    }
                } else if left_pressed {
                    self.editing_fret_buf.clear();
                    if let Some(e_idx) = e_idx_opt {
                        let at_final_event = e_idx == total_events.saturating_sub(1);
                        let is_final_blank = at_final_event
                            && self
                                .parsed_mnx
                                .as_ref()
                                .and_then(|d| d.parts.first())
                                .and_then(|p| p.measures.last())
                                .and_then(|m| m.sequences.first())
                                .and_then(|s| s.flattened_events().last().copied())
                                .map(|e| e.notes.is_empty())
                                .unwrap_or(false);

                        if is_final_blank {
                            if let Some(new_yaml) = remove_last_blank_event(&self.yaml_text) {
                                self.yaml_text = new_yaml;
                                self.process_and_render();
                                let new_total = find_all_event_ranges(&self.yaml_text).len();
                                if new_total > 0 {
                                    let prev_idx = new_total - 1;
                                    self.selected_event_idx = Some(prev_idx);
                                    self.editing_string = cur_str;
                                    self.is_editing_fret = true;
                                    if let Some(s) = cur_str {
                                        self.select_note_on_string(prev_idx, s);
                                    }
                                }
                            }
                        } else if e_idx > 0 {
                            self.retreat_to_prev_note_in_edit_mode(cur_str);
                        }
                    }
                }
            } else if up_pressed || down_pressed {
                // If the user typed digits before moving string, commit them to the current string first
                self.commit_fret_edit();
                self.editing_fret_buf.clear();

                // Move string Up (s - 1) or Down (s + 1)
                if let Some(cur_str) = self.editing_string {
                    if up_pressed && cur_str > 1 {
                        self.editing_string = Some(cur_str - 1);
                    } else if down_pressed && cur_str < num_strings {
                        self.editing_string = Some(cur_str + 1);
                    }
                } else {
                    self.editing_string = Some(1);
                }

                // Update selected_note_idx_in_event to the note on the new string if one exists
                if let Some(e_idx) = self.selected_event_idx {
                    if let Some(cur_str) = self.editing_string {
                        self.select_note_on_string(e_idx, cur_str);
                    }
                }
            } else if backspace_pressed {
                if !self.editing_fret_buf.is_empty() {
                    self.editing_fret_buf.pop();
                } else if let (Some(e_idx), Some(cur_str)) =
                    (self.selected_event_idx, self.editing_string)
                {
                    if let Some(new_yaml) = remove_note_on_string(&self.yaml_text, e_idx, cur_str) {
                        self.yaml_text = new_yaml;
                        self.process_and_render();
                        self.selected_note_idx_in_event = None;
                    }
                }
            } else if delete_pressed {
                if let (Some(e_idx), Some(cur_str)) = (self.selected_event_idx, self.editing_string)
                {
                    if let Some(new_yaml) = remove_note_on_string(&self.yaml_text, e_idx, cur_str) {
                        self.yaml_text = new_yaml;
                        self.process_and_render();
                        self.editing_fret_buf.clear();
                        self.selected_note_idx_in_event = None;
                    }
                }
            } else {
                let events = ui.input(|i| i.events.clone());
                for ev in events {
                    if let egui::Event::Text(t) = ev {
                        for c in t.chars() {
                            if c.is_ascii_digit() && self.editing_fret_buf.len() < 3 {
                                self.editing_fret_buf.push(c);
                            }
                        }
                    }
                }
            }
        } else if !editor_focused {
            let shift_b_pressed = ui.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::B));
            if shift_b_pressed {
                self.add_measure_to_end();
            }

            // Duration keyboard shortcuts: 8 (whole), 7 (half), 6 (quarter), 5 (eighth), 4 (16th), 3 (32nd), . (dot)
            let key_8 = ui.input(|i| i.key_pressed(egui::Key::Num8));
            let key_7 = ui.input(|i| i.key_pressed(egui::Key::Num7));
            let key_6 = ui.input(|i| i.key_pressed(egui::Key::Num6));
            let key_5 = ui.input(|i| i.key_pressed(egui::Key::Num5));
            let key_4 = ui.input(|i| i.key_pressed(egui::Key::Num4));
            let key_3 = ui.input(|i| i.key_pressed(egui::Key::Num3));
            let key_dot = ui.input(|i| i.key_pressed(egui::Key::Period));

            if key_8 {
                self.change_selected_duration_base("whole");
            } else if key_7 {
                self.change_selected_duration_base("half");
            } else if key_6 {
                self.change_selected_duration_base("quarter");
            } else if key_5 {
                self.change_selected_duration_base("eighth");
            } else if key_4 {
                self.change_selected_duration_base("16th");
            } else if key_3 {
                self.change_selected_duration_base("32nd");
            } else if key_dot {
                self.toggle_selected_duration_dot();
            }

            let p_pressed = ui.input(|i| i.key_pressed(egui::Key::P));
            let shift_down = ui.input(|i| i.modifiers.shift);
            let space_pressed = ui.input(|i| i.key_pressed(egui::Key::Space));
            let esc_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));

            if self.midi_player.is_playing() && (space_pressed || esc_pressed) {
                self.midi_player.stop();
                self.midi_status_msg = Some("Playback stopped.".to_string());
            } else if p_pressed {
                if shift_down {
                    self.start_midi_playback(false);
                } else {
                    self.start_midi_playback(true);
                }
            }

            let backspace_pressed = ui.input(|i| i.key_pressed(egui::Key::Backspace));
            let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
            let left_pressed = ui.input(|i| i.key_pressed(egui::Key::ArrowLeft));
            let right_pressed = ui.input(|i| i.key_pressed(egui::Key::ArrowRight));
            let up_pressed = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
            let down_pressed = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));
            let r_pressed = ui.input(|i| i.key_pressed(egui::Key::R));

            if r_pressed && self.selected_event_idx.is_some() {
                self.convert_selected_to_rest();
            } else if backspace_pressed && self.selected_event_idx.is_some() {
                let e_idx = self.selected_event_idx.unwrap();
                let at_final_event = e_idx == total_events.saturating_sub(1);
                let is_final_blank = at_final_event
                    && self
                        .parsed_mnx
                        .as_ref()
                        .and_then(|d| d.parts.first())
                        .and_then(|p| p.measures.last())
                        .and_then(|m| m.sequences.first())
                        .and_then(|s| s.flattened_events().last().copied())
                        .map(|e| e.notes.is_empty())
                        .unwrap_or(false);

                if is_final_blank {
                    if let Some(new_yaml) = remove_last_blank_event(&self.yaml_text) {
                        self.yaml_text = new_yaml;
                        self.process_and_render();
                        let new_total = find_all_event_ranges(&self.yaml_text).len();
                        if new_total > 0 {
                            let prior_idx = new_total - 1;
                            let prior_str =
                                self.find_primary_string_for_event(prior_idx).unwrap_or(1);
                            self.selected_event_idx = Some(prior_idx);
                            self.editing_string = Some(prior_str);
                            self.select_note_on_string(prior_idx, prior_str);
                            self.editing_fret_buf.clear();
                            self.is_editing_fret = true;
                        } else {
                            self.selected_event_idx = None;
                            self.is_editing_fret = false;
                        }
                    }
                } else {
                    let target_str = self
                        .selected_note_idx_in_event
                        .and_then(|n_i| self.get_string_for_note(e_idx, n_i))
                        .or_else(|| self.find_primary_string_for_event(e_idx));

                    if let Some(s) = target_str {
                        if let Some(new_yaml) = remove_note_on_string(&self.yaml_text, e_idx, s) {
                            self.yaml_text = new_yaml;
                            self.process_and_render();
                        }
                    }

                    if e_idx > 0 {
                        let prior_idx = e_idx - 1;
                        let prior_str = target_str
                            .filter(|&ts| self.event_has_note_on_string(prior_idx, ts))
                            .or_else(|| self.find_primary_string_for_event(prior_idx))
                            .unwrap_or(target_str.unwrap_or(1));

                        self.selected_event_idx = Some(prior_idx);
                        self.editing_string = Some(prior_str);
                        self.select_note_on_string(prior_idx, prior_str);
                        self.editing_fret_buf.clear();
                        self.is_editing_fret = true;
                    } else {
                        let current_str = target_str.unwrap_or(1);
                        self.selected_event_idx = Some(0);
                        self.editing_string = Some(current_str);
                        self.select_note_on_string(0, current_str);
                        self.editing_fret_buf.clear();
                        self.is_editing_fret = true;
                    }
                }
            } else if enter_pressed && self.selected_event_idx.is_some() {
                let e_idx = self.selected_event_idx.unwrap();
                let mut initial_string = 1;
                if let Some(doc) = &self.parsed_mnx {
                    if let Some(part) = doc.parts.first() {
                        let mut cur_e = 0;
                        'find_init: for m in &part.measures {
                            for seq in &m.sequences {
                                for ev in seq.flattened_events() {
                                    if cur_e == e_idx {
                                        if let Some(n_idx) = self.selected_note_idx_in_event {
                                            if let Some(n) = ev.notes.get(n_idx) {
                                                if let Some(s) = n.string {
                                                    initial_string = s;
                                                }
                                            }
                                        } else if let Some(first_note) = ev.notes.first() {
                                            if let Some(s) = first_note.string {
                                                initial_string = s;
                                            }
                                        }
                                        break 'find_init;
                                    }
                                    cur_e += 1;
                                }
                            }
                        }
                    }
                }
                self.editing_string = Some(initial_string);
                self.editing_fret_buf.clear();
                self.is_editing_fret = true;
            } else if left_pressed || right_pressed || up_pressed || down_pressed {
                // Left / Right arrow navigation: moves between events, always selecting all notes in target event
                if left_pressed {
                    let at_final_event =
                        self.selected_event_idx == Some(total_events.saturating_sub(1));
                    let is_final_blank = at_final_event
                        && self
                            .parsed_mnx
                            .as_ref()
                            .and_then(|d| d.parts.first())
                            .and_then(|p| p.measures.last())
                            .and_then(|m| m.sequences.first())
                            .and_then(|s| s.flattened_events().last().copied())
                            .map(|e| e.notes.is_empty())
                            .unwrap_or(false);

                    if is_final_blank {
                        // If nothing is entered into this blank note, shifting focus back to the left removes it
                        if let Some(new_yaml) = remove_last_blank_event(&self.yaml_text) {
                            self.yaml_text = new_yaml;
                            self.process_and_render();
                            let new_total = find_all_event_ranges(&self.yaml_text).len();
                            if new_total > 0 {
                                self.selected_event_idx = Some(new_total.saturating_sub(1));
                            } else {
                                self.selected_event_idx = None;
                            }
                            self.selected_note_idx_in_event = None;
                            self.editing_string = None;
                            self.preview_selected_sound();
                        }
                    } else {
                        self.selected_event_idx = match self.selected_event_idx {
                            Some(idx) => Some(idx.saturating_sub(1)),
                            None => Some(total_events.saturating_sub(1)),
                        };
                        self.selected_note_idx_in_event = None;
                        self.editing_string = None;
                        self.preview_selected_sound();
                    }
                }
                if right_pressed {
                    let cur_e_idx = self.selected_event_idx;
                    let tuplet_next =
                        cur_e_idx.and_then(|e_i| self.handle_tuplet_right_arrow(e_i, total_events));

                    if let Some(next_idx) = tuplet_next {
                        self.selected_event_idx = Some(next_idx);
                        self.selected_note_idx_in_event = None;
                        self.editing_string = None;
                        self.preview_selected_sound();
                    } else {
                        let at_final_event =
                            self.selected_event_idx == Some(total_events.saturating_sub(1));

                        if at_final_event {
                            let is_final_blank = self
                                .parsed_mnx
                                .as_ref()
                                .and_then(|d| d.parts.first())
                                .and_then(|p| p.measures.last())
                                .and_then(|m| m.sequences.first())
                                .and_then(|s| s.flattened_events().last().copied())
                                .map(|e| e.notes.is_empty())
                                .unwrap_or(false);

                            if is_final_blank {
                                // Rule 3: Moving further to the right of a final blank note has no effect.
                            } else if let Some(doc) = &self.parsed_mnx {
                                if let Some(part) = doc.parts.first() {
                                    if let Some(last_measure) = part.measures.last() {
                                        if let Some(last_seq) = last_measure.sequences.first() {
                                            if let Some(last_event) =
                                                last_seq.flattened_events().last().copied()
                                            {
                                                let m_idx = part.measures.len() - 1;
                                                let final_dur = &last_event.duration;
                                                let dur_beats =
                                                    validate::duration_to_quarter_beats(final_dur);
                                                let time_sig =
                                                    validate::get_time_signature_for_measure(
                                                        doc, m_idx,
                                                    );
                                                let max_beats = (time_sig.count as f64)
                                                    * (4.0 / time_sig.unit as f64);
                                                let curr_beats: f64 = last_seq.total_beats();

                                                if curr_beats + dur_beats <= max_beats + 1e-4 {
                                                    // Rule 1: Measure would NOT be completed by additional note -> add blank note to current measure
                                                    if let Some(new_yaml) =
                                                        add_blank_note_to_last_measure(
                                                            &self.yaml_text,
                                                            final_dur,
                                                        )
                                                    {
                                                        self.yaml_text = new_yaml;
                                                        self.process_and_render();
                                                        let new_total =
                                                            find_all_event_ranges(&self.yaml_text)
                                                                .len();
                                                        if new_total > 0 {
                                                            self.selected_event_idx =
                                                                Some(new_total - 1);
                                                        }
                                                        self.selected_note_idx_in_event = None;
                                                        self.editing_string = None;
                                                        self.preview_selected_sound();
                                                    }
                                                } else {
                                                    // Rule 2: Bar has been filled up -> add new measure with blank note
                                                    self.add_measure_to_end();
                                                    self.preview_selected_sound();
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            // Standard Right Arrow navigation
                            self.selected_event_idx = match self.selected_event_idx {
                                Some(idx) => Some((idx + 1).min(total_events - 1)),
                                None => Some(0),
                            };
                            self.selected_note_idx_in_event = None;
                            self.editing_string = None;
                            self.preview_selected_sound();
                        }
                    }
                }

                // Up / Down arrow navigation: selects individual notes in a chord
                if up_pressed || down_pressed {
                    if let Some(e_idx) = self.selected_event_idx {
                        if let Some(er) = event_ranges.get(e_idx) {
                            let note_count = er.note_ranges.len();
                            if note_count > 1 {
                                // Order notes visually from top string to bottom string (string 1..N)
                                let mut note_order: Vec<usize> = (0..note_count).collect();
                                if let Some(doc) = &self.parsed_mnx {
                                    if let Some(part) = doc.parts.first() {
                                        let mut cur_e = 0;
                                        'find_ev: for m in &part.measures {
                                            for seq in &m.sequences {
                                                for ev in seq.flattened_events() {
                                                    if cur_e == e_idx {
                                                        note_order.sort_by_key(|&n_i| {
                                                            ev.notes
                                                                .get(n_i)
                                                                .and_then(|n| n.string)
                                                                .unwrap_or(255)
                                                        });
                                                        break 'find_ev;
                                                    }
                                                    cur_e += 1;
                                                }
                                            }
                                        }
                                    }
                                }

                                if up_pressed {
                                    // Up: move to higher string (earlier in note_order)
                                    self.selected_note_idx_in_event =
                                        match self.selected_note_idx_in_event {
                                            None => Some(note_order[0]),
                                            Some(cur) => {
                                                let cur_pos = note_order
                                                    .iter()
                                                    .position(|&x| x == cur)
                                                    .unwrap_or(0);
                                                let new_pos = cur_pos.saturating_sub(1);
                                                Some(note_order[new_pos])
                                            }
                                        };
                                }

                                if down_pressed {
                                    // Down: move to lower string (later in note_order)
                                    self.selected_note_idx_in_event =
                                        match self.selected_note_idx_in_event {
                                            None => Some(note_order[0]),
                                            Some(cur) => {
                                                let cur_pos = note_order
                                                    .iter()
                                                    .position(|&x| x == cur)
                                                    .unwrap_or(0);
                                                let new_pos = (cur_pos + 1).min(note_count - 1);
                                                Some(note_order[new_pos])
                                            }
                                        };
                                }

                                self.preview_selected_sound();
                            }
                        }
                    }
                }
            }
        }
    }

    fn render_dialogs(&mut self, ctx: &egui::Context) {
        enum DialogAction {
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
        }

        let mut close_dialog = false;
        let mut apply_action = None;

        match &mut self.active_dialog {
            ActiveDialog::None => {}
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
        }

        if close_dialog {
            self.active_dialog = ActiveDialog::None;
        }

        if let Some(action) = apply_action {
            match action {
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
                        self.yaml_text = new_yaml;
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
                        self.yaml_text = new_yaml;
                        self.process_and_render();
                        self.selected_event_idx = Some(event_idx);
                    }
                }
            }
        }
    }
}

impl eframe::App for MnxEditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.duration_textures.is_none() {
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

        let event_ranges = find_all_event_ranges(&self.yaml_text);
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

        let editor_focused = ui.memory(|m| m.has_focus(text_edit_id));

        self.handle_keyboard_input(ui, editor_focused, &event_ranges, total_events);

        // 1. Top Menu Bar (Updated with file operations & Duration Toolbar)
        self.render_top_panel(ui, &event_ranges, total_events);

        // 2. Bottom Panel for Errors (only shown if errors exist)
        self.render_error_panel(ui);

        // 3. Right Pane: YAML Text Editor
        self.render_yaml_panel(ui, text_edit_id, &event_ranges);

        // 4. Left Pane: Render Output
        self.render_central_panel(ui);

        // 5. Modal Dialogs (Create Tuplet, Confirm Remove Tuplet)
        self.render_dialogs(ui.ctx());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_all_event_ranges_example_yaml() {
        let yaml = include_str!("../example.yaml");
        let ranges = find_all_event_ranges(yaml);
        assert_eq!(ranges.len(), 20);
        for er in &ranges {
            let snippet = &yaml[er.total_range.clone()];
            assert!(snippet.contains("duration"));
            assert!(snippet.contains("notes"));
            assert_eq!(er.note_ranges.len(), 1);
        }
    }

    #[test]
    fn test_find_all_event_ranges_chord_yaml() {
        let yaml = r#"
parts:
  - measures:
      - sequences:
          - content:
              - duration: { base: quarter }
                notes:
                  - pitch: { step: C, octave: 3 }
                    string: 5
                    fret: 3
                  - pitch: { step: E, octave: 3 }
                    string: 4
                    fret: 2
                  - pitch: { step: G, octave: 3 }
                    string: 3
                    fret: 0
"#;
        let ranges = find_all_event_ranges(yaml);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].note_ranges.len(), 3);
        assert!(yaml[ranges[0].note_ranges[0].clone()].contains("step: C"));
        assert!(yaml[ranges[0].note_ranges[1].clone()].contains("step: E"));
        assert!(yaml[ranges[0].note_ranges[2].clone()].contains("step: G"));
    }

    #[test]
    fn test_update_note_in_yaml() {
        let yaml = include_str!("../example.yaml");
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let part = &doc.parts[0];
        let tuning_calc = crate::edo::EdoTuning::new(part.edo());

        // Note 0 is string 7, fret 12. Let's change it to fret 14.
        let string_pitch = part.get_string_tuning(7).unwrap();
        let string_base = tuning_calc.pitch_to_steps(&string_pitch);
        let played_steps = string_base + 14 * (part.step_multiplier() as i32);
        let new_pitch = tuning_calc.steps_to_pitch(played_steps);

        let updated = update_note_in_yaml(yaml, 0, 0, 14, Some(&new_pitch)).expect("Should update");
        assert!(updated.contains("fret: 14"));

        let updated_doc =
            serde_saphyr::from_str::<MnxDocument>(&updated).expect("Updated doc should parse");
        let errors = crate::validate::validate_mnx(&updated_doc);
        assert_eq!(
            errors.len(),
            0,
            "There should be no validation errors after updating fret and pitch: {:?}",
            errors
        );
    }

    #[test]
    fn test_edit_string4_fret12() {
        let yaml = include_str!("../example.yaml");
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let part = &doc.parts[0];

        // Edit Event 1 to put String 4 Fret 12
        let new_pitch = calculate_pitch_for_string_fret(part, 4, 12);
        let updated = set_or_add_note_on_string(yaml, 1, 4, 12, new_pitch.as_ref())
            .expect("Should update note");

        let updated_doc =
            serde_saphyr::from_str::<MnxDocument>(&updated).expect("Updated doc should parse");
        let errors = crate::validate::validate_mnx(&updated_doc);
        assert_eq!(
            errors.len(),
            0,
            "Validation should pass with 0 errors for String 4 Fret 12: {:?}",
            errors
        );
    }

    #[test]
    fn test_set_or_add_note_on_string_creates_chord() {
        let yaml = include_str!("../example.yaml");
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let part = &doc.parts[0];

        // Event 0 starts with String 7 Fret 12. Add String 6 Fret 9 (D3) to make it a 2-note chord.
        let new_pitch = calculate_pitch_for_string_fret(part, 6, 9);
        let updated =
            set_or_add_note_on_string(yaml, 0, 6, 9, new_pitch.as_ref()).expect("Should add note");

        let updated_doc =
            serde_saphyr::from_str::<MnxDocument>(&updated).expect("Updated doc should parse");
        let ev0_notes = &updated_doc.parts[0].measures[0].sequences[0].flattened_events()[0].notes;
        assert_eq!(ev0_notes.len(), 2);
        assert_eq!(ev0_notes[0].string, Some(7));
        assert_eq!(ev0_notes[0].fret, Some(12));
        assert_eq!(ev0_notes[1].string, Some(6));
        assert_eq!(ev0_notes[1].fret, Some(9));

        let errors = crate::validate::validate_mnx(&updated_doc);
        assert_eq!(
            errors.len(),
            0,
            "Validation should pass with 0 errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_remove_note_on_string() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - instrument:
      name: "Guitar"
    measures:
      - sequences:
          - content:
              - duration: { base: whole }
                notes:
                  - pitch: { step: C, octave: 3 }
                    string: 5
                    fret: 3
                  - pitch: { step: E, octave: 3 }
                    string: 4
                    fret: 2
"#;
        // Remove note on string 4
        let updated = remove_note_on_string(yaml, 0, 4).expect("Should remove note on string 4");
        let updated_doc =
            serde_saphyr::from_str::<MnxDocument>(&updated).expect("Updated doc should parse");
        let notes = &updated_doc.parts[0].measures[0].sequences[0].flattened_events()[0].notes;
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].string, Some(5));

        // Remove note on string 5 (only note left)
        let updated2 = remove_note_on_string(&updated, 0, 5).expect("Should remove last note");
        let updated2_doc =
            serde_saphyr::from_str::<MnxDocument>(&updated2).expect("Updated doc should parse");
        let notes2 = &updated2_doc.parts[0].measures[0].sequences[0].flattened_events()[0].notes;
        assert_eq!(notes2.len(), 0);
    }

    #[test]
    fn test_add_measure_to_yaml() {
        let yaml = include_str!("../example.yaml");
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let initial_measures = doc.parts[0].measures.len();
        assert_eq!(initial_measures, 3);

        let final_dur = doc.parts[0]
            .measures
            .last()
            .unwrap()
            .sequences
            .first()
            .unwrap()
            .flattened_events()
            .last()
            .unwrap()
            .duration
            .clone();
        assert_eq!(final_dur.base, "quarter");

        let updated = add_measure_to_yaml(yaml, &final_dur).expect("Should add measure");
        let updated_doc =
            serde_saphyr::from_str::<MnxDocument>(&updated).expect("Updated doc should parse");
        assert_eq!(updated_doc.global.measures.len(), 4);
        let part = &updated_doc.parts[0];
        assert_eq!(part.measures.len(), 4);

        // Check the newly added measure
        let new_measure = &part.measures[3];
        let new_seq = &new_measure.sequences[0];
        assert_eq!(new_seq.flattened_events().len(), 1);
        let new_event = &new_seq.flattened_events()[0];
        assert_eq!(new_event.duration.base, "quarter");
        assert_eq!(new_event.notes.len(), 0);

        // Validation should pass with 0 errors
        let errors = crate::validate::validate_mnx(&updated_doc);
        assert_eq!(
            errors.len(),
            0,
            "Validation should have 0 errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_add_blank_note_to_last_measure_and_remove() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - instrument:
      name: "Guitar"
    measures:
      - sequences:
          - content:
              - duration: { base: quarter }
                notes:
                  - pitch: { step: C, octave: 3 }
                    string: 5
                    fret: 3
"#;
        let dur = crate::mnx::Duration {
            multiple: None,
            base: "quarter".to_string(),
            dots: None,
        };
        let with_blank = add_blank_note_to_last_measure(yaml, &dur).expect("Should add blank note");
        let doc_with_blank =
            serde_saphyr::from_str::<MnxDocument>(&with_blank).expect("Should parse");
        assert_eq!(
            doc_with_blank.parts[0].measures[0].sequences[0]
                .flattened_events()
                .len(),
            2
        );
        assert_eq!(
            doc_with_blank.parts[0].measures[0].sequences[0].flattened_events()[1]
                .notes
                .len(),
            0
        );

        // Now remove the blank note (since nothing was entered into it)
        let cleaned = remove_last_blank_event(&with_blank).expect("Should remove blank note");
        let cleaned_doc = serde_saphyr::from_str::<MnxDocument>(&cleaned).expect("Should parse");
        assert_eq!(
            cleaned_doc.parts[0].measures[0].sequences[0]
                .flattened_events()
                .len(),
            1
        );
        assert_eq!(
            cleaned_doc.parts[0].measures[0].sequences[0].flattened_events()[0]
                .notes
                .len(),
            1
        );
    }

    #[test]
    fn test_add_measure_and_remove_blank_measure() {
        let yaml = include_str!("../example.yaml");
        let dur = crate::mnx::Duration {
            multiple: None,
            base: "quarter".to_string(),
            dots: None,
        };
        let with_new_measure = add_measure_to_yaml(yaml, &dur).expect("Should add measure");
        let doc1 = serde_saphyr::from_str::<MnxDocument>(&with_new_measure).expect("Should parse");
        assert_eq!(doc1.parts[0].measures.len(), 4);
        assert_eq!(doc1.global.measures.len(), 4);

        // When shifting focus back to the left without entering anything, the empty measure is removed
        let cleaned =
            remove_last_blank_event(&with_new_measure).expect("Should remove empty measure");
        let doc2 = serde_saphyr::from_str::<MnxDocument>(&cleaned).expect("Should parse");
        assert_eq!(doc2.parts[0].measures.len(), 3);
        assert_eq!(doc2.global.measures.len(), 3);
    }

    #[test]
    fn test_right_arrow_rules_and_lifecycle() {
        let mut app = MnxEditorApp::default();
        // App starts with example.yaml (15 events across 2 measures)
        let total_initial = find_all_event_ranges(&app.yaml_text).len();
        assert_eq!(total_initial, 20);

        // Select the final event (event 19, in Measure 2 which is full)
        app.selected_event_idx = Some(19);

        // Right arrow on full final bar -> Rule 2: creates Measure 3 with a blank note
        // (Simulate right arrow logic)
        let final_dur = app
            .parsed_mnx
            .as_ref()
            .and_then(|d| d.parts.first())
            .and_then(|p| p.measures.last())
            .and_then(|m| m.sequences.first())
            .and_then(|s| s.flattened_events().last().copied())
            .map(|e| e.duration.clone())
            .unwrap();

        let updated = add_measure_to_yaml(&app.yaml_text, &final_dur).unwrap();
        app.yaml_text = updated;
        app.process_and_render();
        let total_after_r2 = find_all_event_ranges(&app.yaml_text).len();
        assert_eq!(total_after_r2, 21);
        app.selected_event_idx = Some(20);

        // Rule 3: Moving further to the right of a final blank note has no effect
        let is_final_blank = app
            .parsed_mnx
            .as_ref()
            .and_then(|d| d.parts.first())
            .and_then(|p| p.measures.last())
            .and_then(|m| m.sequences.first())
            .and_then(|s| s.flattened_events().last().copied())
            .map(|e| e.notes.is_empty())
            .unwrap();
        assert!(is_final_blank);

        // Shifting focus back to the left -> blank note and measure disappear
        let cleaned = remove_last_blank_event(&app.yaml_text).unwrap();
        app.yaml_text = cleaned;
        app.process_and_render();
        let total_after_left = find_all_event_ranges(&app.yaml_text).len();
        assert_eq!(total_after_left, 20);
    }

    #[test]
    fn test_update_event_duration_in_yaml() {
        let yaml = include_str!("../example.yaml");
        let dur_quarter = crate::mnx::Duration {
            multiple: None,
            base: "quarter".to_string(),
            dots: None,
        };
        let updated =
            update_event_duration_in_yaml(yaml, 0, &dur_quarter).expect("Should update duration");
        let doc = serde_saphyr::from_str::<MnxDocument>(&updated).expect("Should parse");
        assert_eq!(
            doc.parts[0].measures[0].sequences[0].flattened_events()[0]
                .duration
                .base,
            "quarter"
        );
        assert_eq!(
            doc.parts[0].measures[0].sequences[0].flattened_events()[0]
                .duration
                .dots,
            None
        );

        // Test dotted
        let dur_dotted_half = crate::mnx::Duration {
            multiple: None,
            base: "half".to_string(),
            dots: Some(1),
        };
        let updated_dot = update_event_duration_in_yaml(&updated, 0, &dur_dotted_half)
            .expect("Should update to dotted");
        let doc_dot = serde_saphyr::from_str::<MnxDocument>(&updated_dot).expect("Should parse");
        assert_eq!(
            doc_dot.parts[0].measures[0].sequences[0].flattened_events()[0]
                .duration
                .base,
            "half"
        );
        assert_eq!(
            doc_dot.parts[0].measures[0].sequences[0].flattened_events()[0]
                .duration
                .dots,
            Some(1)
        );
    }

    #[test]
    fn test_change_and_toggle_duration() {
        let mut app = MnxEditorApp::default();
        app.selected_event_idx = Some(0);

        // Change base to whole
        app.change_selected_duration_base("whole");
        assert_eq!(app.get_selected_event_duration().unwrap().base, "whole");

        // Toggle dot on
        app.toggle_selected_duration_dot();
        let dur = app.get_selected_event_duration().unwrap();
        assert_eq!(dur.base, "whole");
        assert_eq!(dur.dots, Some(1));

        // Toggle dot off
        app.toggle_selected_duration_dot();
        let dur2 = app.get_selected_event_duration().unwrap();
        assert_eq!(dur2.base, "whole");
        assert_eq!(dur2.dots, None);

        // Change base to 16th
        app.change_selected_duration_base("16th");
        assert_eq!(app.get_selected_event_duration().unwrap().base, "16th");
    }
    #[test]
    fn test_set_bpm_in_yaml_insert() {
        let yaml = include_str!("../example.yaml");
        let result = set_bpm_in_yaml(yaml, 140.0);
        println!("RESULT YAML:\n{}", &result[..result.len().min(400)]);
        let doc =
            serde_saphyr::from_str::<MnxDocument>(&result).expect("Should parse after BPM insert");
        println!("DOC GLOBAL: {:?}", doc.global);
        let bpm = doc
            .global
            .measures
            .iter()
            .flat_map(|m| m.directions.iter())
            .find_map(|d| d.tempo.as_ref().map(|t| t.bpm));
        assert_eq!(bpm, Some(140.0), "BPM should be 140 after insert");
    }

    #[test]
    fn test_set_bpm_in_yaml_update() {
        let yaml = include_str!("../example.yaml");
        let with_bpm = set_bpm_in_yaml(yaml, 140.0);
        let updated = set_bpm_in_yaml(&with_bpm, 200.0);
        let doc =
            serde_saphyr::from_str::<MnxDocument>(&updated).expect("Should parse after BPM update");
        let bpm = doc
            .global
            .measures
            .iter()
            .flat_map(|m| m.directions.iter())
            .find_map(|d| d.tempo.as_ref().map(|t| t.bpm));
        assert_eq!(bpm, Some(200.0), "BPM should be updated to 200");
        let tempo_count = doc
            .global
            .measures
            .iter()
            .flat_map(|m| m.directions.iter())
            .filter(|d| d.tempo.is_some())
            .count();
        assert_eq!(tempo_count, 1, "Should only have one tempo directive");
    }

    #[test]
    fn test_edit_mode_arrow_commit_and_advance() {
        let mut app = MnxEditorApp::default();
        app.selected_event_idx = Some(0);
        app.is_editing_fret = true;
        app.editing_string = Some(7);
        app.editing_fret_buf = "14".to_string();

        let total_events = find_all_event_ranges(&app.yaml_text).len();
        let cur_str = app.editing_string;
        assert!(app.commit_fret_edit());
        app.editing_fret_buf.clear();
        app.advance_to_next_note_in_edit_mode(cur_str, total_events);

        // Check event 0 was updated to fret 14 on string 7
        let doc = serde_saphyr::from_str::<MnxDocument>(&app.yaml_text).unwrap();
        let note = &doc.parts[0].measures[0].sequences[0].flattened_events()[0].notes[0];
        assert_eq!(note.string, Some(7));
        assert_eq!(note.fret, Some(14));

        // Check app moved to event 1 and is in edit mode on string 7
        assert_eq!(app.selected_event_idx, Some(1));
        assert!(app.is_editing_fret);
        assert_eq!(app.editing_string, Some(7));
        assert!(app.editing_fret_buf.is_empty());

        // Now edit event 1 and retreat left
        app.editing_fret_buf = "10".to_string();
        assert!(app.commit_fret_edit());
        app.editing_fret_buf.clear();
        app.retreat_to_prev_note_in_edit_mode(cur_str);

        let doc2 = serde_saphyr::from_str::<MnxDocument>(&app.yaml_text).unwrap();
        let note2 = doc2.parts[0].measures[0].sequences[0].flattened_events()[1]
            .notes
            .iter()
            .find(|n| n.string == Some(7))
            .expect("Should have note on string 7");
        assert_eq!(note2.fret, Some(10));

        assert_eq!(app.selected_event_idx, Some(0));
        assert!(app.is_editing_fret);
        assert_eq!(app.editing_string, Some(7));
    }

    #[test]
    fn test_blue_mode_backspace_removes_note_and_opens_orange_edit() {
        let mut app = MnxEditorApp::default();
        // Start in blue select mode at event 1
        app.selected_event_idx = Some(1);
        app.is_editing_fret = false;

        let _total_events = find_all_event_ranges(&app.yaml_text).len();
        let e_idx = 1;

        let target_str = app
            .selected_note_idx_in_event
            .and_then(|n_i| app.get_string_for_note(e_idx, n_i))
            .or_else(|| app.find_primary_string_for_event(e_idx));
        assert!(target_str.is_some());

        if let Some(s) = target_str {
            if let Some(new_yaml) = remove_note_on_string(&app.yaml_text, e_idx, s) {
                app.yaml_text = new_yaml;
                app.process_and_render();
            }
        }

        let prior_idx = e_idx - 1;
        let prior_str = target_str
            .filter(|&ts| app.event_has_note_on_string(prior_idx, ts))
            .or_else(|| app.find_primary_string_for_event(prior_idx))
            .unwrap_or(target_str.unwrap_or(1));

        app.selected_event_idx = Some(prior_idx);
        app.editing_string = Some(prior_str);
        app.select_note_on_string(prior_idx, prior_str);
        app.editing_fret_buf.clear();
        app.is_editing_fret = true;

        // Verify event 1 note is removed (notes is empty)
        let doc = serde_saphyr::from_str::<MnxDocument>(&app.yaml_text).unwrap();
        assert!(
            doc.parts[0].measures[0].sequences[0].flattened_events()[1]
                .notes
                .is_empty()
        );

        // Verify orange edit mode is open on prior note (event 0)
        assert_eq!(app.selected_event_idx, Some(0));
        assert!(app.is_editing_fret);
        assert_eq!(app.editing_string, Some(prior_str));
    }

    #[test]
    fn test_edit_mode_navigate_existing_note_without_typing() {
        let mut app = MnxEditorApp::default();
        // Start in edit mode on event 0, string 7 without typing anything (buffer is empty)
        app.selected_event_idx = Some(0);
        app.is_editing_fret = true;
        app.editing_string = Some(7);
        app.editing_fret_buf.clear();

        let total_events = find_all_event_ranges(&app.yaml_text).len();
        let cur_str = app.editing_string;

        // Pressing right arrow should navigate to event 1 on string 7 in edit mode
        app.advance_to_next_note_in_edit_mode(cur_str, total_events);
        assert_eq!(app.selected_event_idx, Some(1));
        assert!(app.is_editing_fret);
        assert_eq!(app.editing_string, Some(7));
        assert!(app.editing_fret_buf.is_empty());

        // Pressing left arrow should navigate back to event 0 on string 7 in edit mode
        app.retreat_to_prev_note_in_edit_mode(cur_str);
        assert_eq!(app.selected_event_idx, Some(0));
        assert!(app.is_editing_fret);
        assert_eq!(app.editing_string, Some(7));
    }

    #[test]
    fn test_edit_mode_blank_note_left_arrow_removes_blank() {
        let mut app = MnxEditorApp::default();
        let total_initial = find_all_event_ranges(&app.yaml_text).len();
        assert_eq!(total_initial, 20);

        // Select the final event (event 14) and advance right to create a new blank note in edit mode
        app.selected_event_idx = Some(19);
        app.is_editing_fret = true;
        app.editing_string = Some(4);
        app.advance_to_next_note_in_edit_mode(Some(4), total_initial);

        let total_after_advance = find_all_event_ranges(&app.yaml_text).len();
        assert_eq!(total_after_advance, 21);
        assert_eq!(app.selected_event_idx, Some(20));
        assert!(app.is_editing_fret);
        assert_eq!(app.editing_string, Some(4));

        // Now, while on the blank note with empty buffer, heading left should remove the blank note
        let cur_str = app.editing_string;
        let e_idx_opt = app.selected_event_idx;
        if let Some(e_idx) = e_idx_opt {
            let total_events = find_all_event_ranges(&app.yaml_text).len();
            let at_final_event = e_idx == total_events.saturating_sub(1);
            let is_final_blank = at_final_event
                && app
                    .parsed_mnx
                    .as_ref()
                    .and_then(|d| d.parts.first())
                    .and_then(|p| p.measures.last())
                    .and_then(|m| m.sequences.first())
                    .and_then(|s| s.flattened_events().last().copied())
                    .map(|e| e.notes.is_empty())
                    .unwrap_or(false);

            assert!(is_final_blank);
            if let Some(new_yaml) = remove_last_blank_event(&app.yaml_text) {
                app.yaml_text = new_yaml;
                app.process_and_render();
                let new_total = find_all_event_ranges(&app.yaml_text).len();
                assert_eq!(new_total, 20);
                let prev_idx = new_total - 1;
                app.selected_event_idx = Some(prev_idx);
                app.editing_string = cur_str;
                app.is_editing_fret = true;
                if let Some(s) = cur_str {
                    app.select_note_on_string(prev_idx, s);
                }
            }
        }

        assert_eq!(app.selected_event_idx, Some(19));
        assert!(app.is_editing_fret);
        assert_eq!(app.editing_string, Some(4));
    }

    #[test]
    fn test_edit_mode_navigate_right_on_blank_string_when_next_event_has_notes() {
        let mut app = MnxEditorApp::default();
        // In example.yaml, event 0 has a note on string 7, but string 1 is blank at event 0.
        // Event 1 has a note on string 7.
        app.selected_event_idx = Some(0);
        app.is_editing_fret = true;
        app.editing_string = Some(1);
        app.editing_fret_buf.clear();

        // Check string 1 is indeed blank at event 0
        assert!(!app.event_has_note_on_string(0, 1));
        // Check event 1 has notes
        assert!(app.event_has_any_notes(1));

        let total_events = find_all_event_ranges(&app.yaml_text).len();
        let cur_str = app.editing_string;
        let e_idx = 0;

        if e_idx + 1 < total_events && app.event_has_any_notes(e_idx + 1) {
            let next_idx = e_idx + 1;
            app.selected_event_idx = Some(next_idx);
            app.editing_string = cur_str;
            app.is_editing_fret = true;
            if let Some(s) = cur_str {
                app.select_note_on_string(next_idx, s);
            }
        }

        assert_eq!(app.selected_event_idx, Some(1));
        assert!(app.is_editing_fret);
        assert_eq!(app.editing_string, Some(1));
    }
}

#[cfg(test)]
mod rest_tests {
    use super::*;

    #[test]
    fn test_convert_event_to_rest_and_back() {
        let mut app = MnxEditorApp::default();
        let _total_initial = crate::yaml_editor::find_all_event_ranges(&app.yaml_text).len();

        // Select event 0 (which has notes)
        app.selected_event_idx = Some(0);

        // Convert to rest
        if let Some(new_yaml) = crate::yaml_editor::convert_event_to_rest(&app.yaml_text, 0) {
            app.yaml_text = new_yaml;
            app.process_and_render();
        }

        let doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(&app.yaml_text).unwrap();
        let ev = &doc.parts[0].measures[0].sequences[0].flattened_events()[0];
        assert!(ev.rest.is_some());
        assert!(ev.notes.is_empty());

        // Convert back to note by typing a number
        app.is_editing_fret = true;
        app.editing_string = Some(3); // String 3
        app.editing_fret_buf = "5".to_string();
        app.commit_fret_edit();
        app.process_and_render();

        let doc2 = serde_saphyr::from_str::<crate::mnx::MnxDocument>(&app.yaml_text).unwrap();
        let ev2 = &doc2.parts[0].measures[0].sequences[0].flattened_events()[0];
        assert!(ev2.rest.is_none());
        assert_eq!(ev2.notes.len(), 1);
        assert_eq!(ev2.notes[0].string, Some(3));
        assert_eq!(ev2.notes[0].fret, Some(5));
    }
}

#[cfg(test)]
mod tuplet_button_tests {
    use super::*;

    #[test]
    fn test_tuplet_state_behaviors_and_yaml_lifecycle() {
        let mut app = MnxEditorApp::default();

        // 1. Select mode on non-tuple event (event 0)
        app.selected_event_idx = Some(0);
        app.is_editing_fret = false;
        app.handle_tuplet_button_click();

        // Should open CreateTuplet dialog with from_select_mode = true
        assert!(matches!(
            app.active_dialog,
            ActiveDialog::CreateTuplet {
                inner_multiple: 3,
                outer_multiple: 2,
                from_select_mode: true,
                ..
            }
        ));

        // Create the tuplet in YAML
        let new_yaml = crate::yaml_editor::create_tuplet_in_yaml(&app.yaml_text, 0, 3, 2, "eighth")
            .expect("Should create tuplet");
        app.yaml_text = new_yaml;
        app.process_and_render();
        app.selected_event_idx = Some(0);
        app.is_editing_fret = true; // enters edit mode

        let doc = app.parsed_mnx.as_ref().unwrap();
        let tuplet_info = doc
            .find_event_tuplet_info(0)
            .expect("Event 0 should be in tuplet");
        assert_eq!(tuplet_info.tuplet.inner.multiple, 3);
        assert_eq!(tuplet_info.tuplet.outer.multiple, 2);
        assert_eq!(tuplet_info.event_range, 0..3);

        // Events 1 and 2 in the tuplet should be rests
        let seq = &doc.parts[0].measures[0].sequences[0];
        let flat = seq.flattened_events();
        assert!(flat[0].rest.is_none());
        assert!(flat[1].rest.is_some());
        assert!(flat[2].rest.is_some());

        // 2. In edit mode within a tuple figure:
        // Pressing tuplet button fills remainder with rests and appends blank note after tuplet
        app.selected_event_idx = Some(0);
        app.is_editing_fret = true;
        app.active_dialog = ActiveDialog::None;

        app.handle_tuplet_button_click();
        assert_eq!(app.active_dialog, ActiveDialog::None);
        // A new blank note should follow the end of the tuple duration (index 3)
        assert_eq!(app.selected_event_idx, Some(3));
        assert!(app.is_editing_fret);

        let doc_after_fill = app.parsed_mnx.as_ref().unwrap();
        let flat_after = doc_after_fill.parts[0].measures[0].sequences[0].flattened_events();
        assert_eq!(flat_after[3].notes.len(), 0);

        // 3. In select mode within a tuple figure:
        // Select event 1 (inside the tuplet)
        app.selected_event_idx = Some(1);
        app.is_editing_fret = false;
        app.handle_tuplet_button_click();

        // Should open ConfirmRemoveTuplet dialog
        assert!(matches!(
            app.active_dialog,
            ActiveDialog::ConfirmRemoveTuplet { event_idx: 1 }
        ));

        // When OK is confirmed, remove tuplet unwraps events
        let unwrapped_yaml = crate::yaml_editor::remove_tuplet_in_yaml(&app.yaml_text, 1)
            .expect("Should remove tuplet");
        app.yaml_text = unwrapped_yaml;
        app.process_and_render();
        app.selected_event_idx = Some(1);
        app.active_dialog = ActiveDialog::None;

        let doc_unwrapped = app.parsed_mnx.as_ref().unwrap();
        assert!(
            doc_unwrapped.find_event_tuplet_info(1).is_none(),
            "Tuplet wrapper should be removed"
        );
    }

    #[test]
    fn test_non_tuple_edit_mode_dialog() {
        let mut app = MnxEditorApp::default();
        app.selected_event_idx = Some(0);
        app.is_editing_fret = true;
        app.handle_tuplet_button_click();

        assert!(matches!(
            app.active_dialog,
            ActiveDialog::CreateTuplet {
                inner_multiple: 3,
                outer_multiple: 2,
                from_select_mode: false,
                ..
            }
        ));
    }

    #[test]
    fn test_tuplet_right_arrow_creates_non_tuple_blank_note() {
        let mut app = MnxEditorApp::default();
        let new_yaml = crate::yaml_editor::create_tuplet_in_yaml(&app.yaml_text, 0, 3, 2, "eighth")
            .expect("Should create tuplet");
        app.yaml_text = new_yaml;
        app.process_and_render();

        // Event 0: note, Event 1: rest, Event 2: rest.
        // Event 2 is the final note/event in this tuplet.
        app.selected_event_idx = Some(2);
        let total = find_all_event_ranges(&app.yaml_text).len();

        // 1. In select mode
        let next_idx = app.handle_tuplet_right_arrow(2, total);
        assert!(next_idx.is_some());
        let new_e = next_idx.unwrap();
        assert_eq!(new_e, 3);

        let doc = app.parsed_mnx.as_ref().unwrap();
        // Crucial requirement: the new note must be NON-TUPLE
        assert!(
            doc.find_event_tuplet_info(new_e).is_none(),
            "New note must be non-tuple"
        );
        let ev = &doc.parts[0].measures[0].sequences[0].flattened_events()[new_e];
        // Must match the base duration of the final note inside the tuple ("eighth")
        assert_eq!(ev.duration.base, "eighth");

        // 2. In edit mode
        app.selected_event_idx = Some(2);
        app.is_editing_fret = true;
        app.editing_string = Some(1);
        let total_now = find_all_event_ranges(&app.yaml_text).len();
        app.advance_to_next_note_in_edit_mode(Some(1), total_now);

        assert_eq!(app.selected_event_idx, Some(3));
        assert!(app.is_editing_fret);
        let doc2 = app.parsed_mnx.as_ref().unwrap();
        assert!(
            doc2.find_event_tuplet_info(3).is_none(),
            "Must be non-tuple in edit mode"
        );
    }
}
