use eframe::egui::{self, Color32};

use std::fs;
use crate::mnx::MnxDocument;
use crate::validate;
use crate::render;

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
    selected_note_idx_in_event: Option<usize>,     // Selected sub-note within chord/event (None = all)
    is_editing_fret: bool,                         // In-place fret number editing mode
    editing_fret_buf: String,                      // Text buffer for the new fret number
    editing_string: Option<u8>,                    // Currently focused string row in edit mode
    duration_textures: Option<DurationTextures>,   // Note duration icons
    bpm_buf: String,                               // Text buffer for BPM input
    midi_ports: Vec<crate::midi::MidiPortInfo>,    // Available MIDI output devices
    selected_midi_port: Option<usize>,             // Selected MIDI port id
    pitch_bend_range: u8,                          // Pitchbend range (1-48, default 2)
    string_channel_map: std::collections::HashMap<u8, u8>, // String (1..n) -> MIDI channel (1..16)
    is_mpe_mode: bool,                             // Toggle button for multi-timbral vs MPE output
    midi_player: crate::midi::MidiPlayer,          // Real-time MIDI sequencer playback engine
    midi_status_msg: Option<String>,               // Status or error message from MIDI playback
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
        };
        
        app.process_and_render();
        app
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EventRange {
    pub total_range: std::ops::Range<usize>,
    pub note_ranges: Vec<std::ops::Range<usize>>,
}

/// Locates character byte ranges for all events (and their individual sub-notes) under `content:` blocks in the YAML text.
pub fn find_all_event_ranges(yaml: &str) -> Vec<EventRange> {
    let mut results = Vec::new();
    let lines: Vec<&str> = yaml.lines().collect();
    if lines.is_empty() {
        return results;
    }

    let mut line_starts = Vec::with_capacity(lines.len());
    let mut offset = 0;
    for line in yaml.split_inclusive('\n') {
        line_starts.push(offset);
        offset += line.len();
    }
    while line_starts.len() < lines.len() {
        let last_start = *line_starts.last().unwrap_or(&0);
        let prev_len = lines[line_starts.len() - 1].len();
        line_starts.push(last_start + prev_len);
    }

    let mut in_content = false;
    let mut content_indent = 0;
    let mut current_event_start_line: Option<usize> = None;
    let mut event_indent = 0;

    let finish_event = |start_line: usize, end_line: usize, results: &mut Vec<EventRange>| {
        let total_start = line_starts[start_line];
        let total_end = line_starts[end_line] + lines[end_line].len();

        let mut note_ranges = Vec::new();
        let mut in_notes = false;
        let mut notes_indent = 0;
        let mut current_note_start: Option<usize> = None;
        let mut note_indent = 0;

        for line_num in start_line..=end_line {
            let line = lines[line_num];
            let trimmed = line.trim();
            let indent = line.len() - line.trim_start().len();

            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if trimmed.starts_with("notes:") || trimmed.ends_with("notes:") {
                in_notes = true;
                notes_indent = indent;
                continue;
            }

            if in_notes {
                if indent <= notes_indent && !trimmed.starts_with('-') {
                    if let Some(n_start) = current_note_start {
                        let prev = line_num.saturating_sub(1);
                        let n_end = line_starts[prev] + lines[prev].len();
                        note_ranges.push(n_start..n_end);
                        current_note_start = None;
                    }
                    in_notes = false;
                    continue;
                }

                if trimmed.starts_with('-') {
                    if let Some(n_start) = current_note_start {
                        if indent <= note_indent {
                            let prev = line_num.saturating_sub(1);
                            let n_end = line_starts[prev] + lines[prev].len();
                            note_ranges.push(n_start..n_end);
                            current_note_start = Some(line_starts[line_num]);
                            note_indent = indent;
                        }
                    } else {
                        current_note_start = Some(line_starts[line_num]);
                        note_indent = indent;
                    }
                }
            }
        }

        if let Some(n_start) = current_note_start {
            let n_end = line_starts[end_line] + lines[end_line].len();
            note_ranges.push(n_start..n_end);
        }

        results.push(EventRange {
            total_range: total_start..total_end,
            note_ranges,
        });
    };

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with("content:") || trimmed.ends_with("content:") {
            if let Some(start_line) = current_event_start_line {
                let end_line = i.saturating_sub(1);
                finish_event(start_line, end_line, &mut results);
                current_event_start_line = None;
            }
            in_content = true;
            content_indent = indent;
            continue;
        }

        if in_content {
            if indent <= content_indent
                && (trimmed.starts_with("measures:")
                    || trimmed.starts_with("sequences:")
                    || trimmed.starts_with("parts:")
                    || trimmed.starts_with("- sequences:")
                    || trimmed.starts_with("- measures:"))
            {
                if let Some(start_line) = current_event_start_line {
                    let end_line = i.saturating_sub(1);
                    finish_event(start_line, end_line, &mut results);
                    current_event_start_line = None;
                }
                in_content = false;
                continue;
            }

            if trimmed.starts_with('-') {
                if let Some(start_line) = current_event_start_line {
                    if indent <= event_indent {
                        let end_line = i.saturating_sub(1);
                        finish_event(start_line, end_line, &mut results);
                        current_event_start_line = Some(i);
                        event_indent = indent;
                    }
                } else {
                    current_event_start_line = Some(i);
                    event_indent = indent;
                }
            }
        }
    }

    if let Some(start_line) = current_event_start_line {
        let end_line = lines.len().saturating_sub(1);
        finish_event(start_line, end_line, &mut results);
    }

    results
}

fn format_pitch_yaml(pitch: &crate::mnx::Pitch) -> String {
    if let Some(alter) = &pitch.alter {
        let alter_str = match alter {
            crate::mnx::Alteration::Fraction(s) => format!("'{}'", s),
            crate::mnx::Alteration::Value(v) => format!("{}", v),
        };
        format!("pitch: {{ step: {}, alter: {}, octave: {} }}", pitch.step, alter_str, pitch.octave)
    } else {
        format!("pitch: {{ step: {}, octave: {} }}", pitch.step, pitch.octave)
    }
}

/// Calculates the exact microtonal Pitch for a given string and fret.
pub fn calculate_pitch_for_string_fret(
    part: &crate::mnx::Part,
    string: u8,
    fret: u8,
) -> Option<crate::mnx::Pitch> {
    let capo_fret = part.capo_fret();
    let edo = part.edo();
    let step_multiplier = part.step_multiplier();
    let tuning_calc = crate::edo::EdoTuning::new(edo);
    let string_pitch = part.get_string_tuning(string)?;
    let string_base = tuning_calc.pitch_to_steps(&string_pitch);
    let physical_capo_steps = (capo_fret as i32) * step_multiplier;
    let physical_fret_steps = (fret as i32) * step_multiplier;
    let played_steps = string_base + physical_capo_steps + physical_fret_steps;
    Some(tuning_calc.steps_to_pitch(played_steps))
}

/// Helper to parse the `string: <n>` value out of an individual note YAML block text.
pub fn get_note_string_from_text(note_text: &str) -> Option<u8> {
    for line in note_text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("string:") {
            let num_str = rest.trim();
            let digits: String = num_str.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(s) = digits.parse::<u8>() {
                return Some(s);
            }
        }
    }
    None
}

/// Replaces the fret number and pitch of the given note inside yaml_text and returns the updated YAML string.
pub fn update_note_in_yaml(
    yaml: &str,
    event_idx: usize,
    note_idx: usize,
    new_fret: u8,
    new_pitch: Option<&crate::mnx::Pitch>,
) -> Option<String> {
    let event_ranges = find_all_event_ranges(yaml);
    let er = event_ranges.get(event_idx)?;
    let target_range = er.note_ranges.get(note_idx).unwrap_or(&er.total_range);
    let note_text = yaml.get(target_range.clone())?;

    let mut result = yaml.to_string();

    // 1. Update fret
    if let Some(pos) = note_text.find("fret:") {
        let after_fret = &note_text[pos + 5..];
        let num_start_rel = after_fret.find(|c: char| c.is_ascii_digit())?;
        let num_end_rel = after_fret[num_start_rel..]
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after_fret[num_start_rel..].len());

        let abs_start = target_range.start + pos + 5 + num_start_rel;
        let abs_end = abs_start + num_end_rel;

        result = format!("{}{}{}", &yaml[..abs_start], new_fret, &yaml[abs_end..]);
    }

    // 2. Update pitch if provided
    if let Some(pitch) = new_pitch {
        let event_ranges = find_all_event_ranges(&result);
        if let Some(er) = event_ranges.get(event_idx) {
            let target_range = er.note_ranges.get(note_idx).unwrap_or(&er.total_range);
            if let Some(cur_note_text) = result.get(target_range.clone()) {
                if let Some(p_start_rel) = cur_note_text.find("pitch:") {
                    let after_p = &cur_note_text[p_start_rel..];
                    let p_len = if let Some(brace_end) = after_p.find('}') {
                        brace_end + 1
                    } else if let Some(nl) = after_p.find('\n') {
                        nl
                    } else {
                        after_p.len()
                    };

                    let abs_p_start = target_range.start + p_start_rel;
                    let abs_p_end = abs_p_start + p_len;

                    let pitch_yaml_str = format_pitch_yaml(pitch);
                    result = format!("{}{}{}", &result[..abs_p_start], pitch_yaml_str, &result[abs_p_end..]);
                }
            }
        }
    }

    Some(result)
}

/// Sets or adds a note on `target_string` in the specified event with `new_fret` and `new_pitch`.
pub fn set_or_add_note_on_string(
    yaml: &str,
    event_idx: usize,
    target_string: u8,
    new_fret: u8,
    new_pitch: Option<&crate::mnx::Pitch>,
) -> Option<String> {
    let event_ranges = find_all_event_ranges(yaml);
    let er = event_ranges.get(event_idx)?;

    // 1. Check if a note already exists on target_string
    for (note_idx, nr) in er.note_ranges.iter().enumerate() {
        if let Some(note_text) = yaml.get(nr.clone()) {
            if get_note_string_from_text(note_text) == Some(target_string) {
                return update_note_in_yaml(yaml, event_idx, note_idx, new_fret, new_pitch);
            }
        }
    }

    // 2. No note on target_string: Add a new note to this event
    let pitch_str = if let Some(p) = new_pitch {
        format_pitch_yaml(p)
    } else {
        "pitch: { step: C, octave: 4 }".to_string()
    };

    if !er.note_ranges.is_empty() {
        // Append after the last existing note in this event
        let last_range = er.note_ranges.last().unwrap();
        let first_note_text = &yaml[er.note_ranges[0].clone()];
        let leading_spaces = first_note_text.len() - first_note_text.trim_start().len();
        let indent = " ".repeat(leading_spaces);
        let sub_indent = " ".repeat(leading_spaces + 2);

        let insert_text = format!(
            "\n{}- {}\n{}string: {}\n{}fret: {}",
            indent, pitch_str, sub_indent, target_string, sub_indent, new_fret
        );

        let insert_pos = last_range.end;
        let mut res = yaml.to_string();
        res.insert_str(insert_pos, &insert_text);
        Some(res)
    } else {
        // notes: list is empty or notes: []
        let event_text = yaml.get(er.total_range.clone())?;
        if let Some(notes_pos) = event_text.find("notes:") {
            let abs_notes_pos = er.total_range.start + notes_pos;
            let after_notes = &yaml[abs_notes_pos + 6..];
            let line_end_rel = after_notes.find('\n').unwrap_or(after_notes.len());

            // Find indent of notes: line
            let line_start = yaml[..abs_notes_pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
            let notes_indent_len = abs_notes_pos - line_start;
            let indent = " ".repeat(notes_indent_len + 2);
            let sub_indent = " ".repeat(notes_indent_len + 4);

            let new_notes_block = format!(
                "notes:\n{}- {}\n{}string: {}\n{}fret: {}",
                indent, pitch_str, sub_indent, target_string, sub_indent, new_fret
            );

            let abs_replace_end = abs_notes_pos + 6 + line_end_rel;
            let mut res = yaml.to_string();
            res.replace_range(abs_notes_pos..abs_replace_end, &new_notes_block);
            Some(res)
        } else {
            None
        }
    }
}

/// Removes the note on `target_string` from the specified event in YAML.
pub fn remove_note_on_string(
    yaml: &str,
    event_idx: usize,
    target_string: u8,
) -> Option<String> {
    let event_ranges = find_all_event_ranges(yaml);
    let er = event_ranges.get(event_idx)?;

    let mut target_range: Option<std::ops::Range<usize>> = None;
    for nr in &er.note_ranges {
        if let Some(note_text) = yaml.get(nr.clone()) {
            if get_note_string_from_text(note_text) == Some(target_string) {
                target_range = Some(nr.clone());
                break;
            }
        }
    }

    let nr = target_range?;
    let mut res = yaml.to_string();

    if er.note_ranges.len() == 1 {
        // Only note in the event: replace entire notes block with notes: []
        let event_text = yaml.get(er.total_range.clone())?;
        if let Some(notes_pos) = event_text.find("notes:") {
            let abs_notes_pos = er.total_range.start + notes_pos;
            let replace_end = nr.end;
            res.replace_range(abs_notes_pos..replace_end, "notes: []");
        }
    } else {
        // Chord with multiple notes: remove this specific note range
        let mut start = nr.start;
        let mut end = nr.end;
        if start > 0 && res.as_bytes()[start - 1] == b'\n' {
            start -= 1;
        } else if end < res.len() && res.as_bytes()[end] == b'\n' {
            end += 1;
        }
        res.replace_range(start..end, "");
    }
    Some(res)
}

/// Replaces the duration of the specified event inside yaml_text and returns the updated YAML string.
pub fn update_event_duration_in_yaml(
    yaml: &str,
    event_idx: usize,
    new_duration: &crate::mnx::Duration,
) -> Option<String> {
    let event_ranges = find_all_event_ranges(yaml);
    let er = event_ranges.get(event_idx)?;
    let event_text = yaml.get(er.total_range.clone())?;

    let dur_rel = event_text.find("duration:")?;
    let abs_dur_start = er.total_range.start + dur_rel;
    let after_dur = &yaml[abs_dur_start + 9..];

    let dur_str = if let Some(dots) = new_duration.dots {
        if dots > 0 {
            format!("{{ base: {}, dots: {} }}", new_duration.base, dots)
        } else {
            format!("{{ base: {} }}", new_duration.base)
        }
    } else {
        format!("{{ base: {} }}", new_duration.base)
    };

    let replacement = format!("duration: {}", dur_str);

    let abs_dur_end = if let Some(brace_start) = after_dur.find('{') {
        let before_brace = &after_dur[..brace_start];
        if !before_brace.contains("notes:") && !before_brace.contains('\n') {
            if let Some(brace_end) = after_dur.find('}') {
                abs_dur_start + 9 + brace_end + 1
            } else {
                abs_dur_start + 9 + after_dur.find('\n').unwrap_or(after_dur.len())
            }
        } else if let Some(notes_rel) = event_text.find("notes:") {
            let notes_abs = er.total_range.start + notes_rel;
            let before_notes = &yaml[..notes_abs];
            before_notes.rfind('\n').map(|p| p + 1).unwrap_or(notes_abs)
        } else {
            abs_dur_start + 9 + after_dur.find('\n').unwrap_or(after_dur.len())
        }
    } else if let Some(notes_rel) = event_text.find("notes:") {
        let notes_abs = er.total_range.start + notes_rel;
        let before_notes = &yaml[..notes_abs];
        before_notes.rfind('\n').map(|p| p + 1).unwrap_or(notes_abs)
    } else {
        abs_dur_start + 9 + after_dur.find('\n').unwrap_or(after_dur.len())
    };

    let mut result = yaml.to_string();
    result.replace_range(abs_dur_start..abs_dur_end, &replacement);
    Some(result)
}

/// Sets or updates the BPM tempo directive in the first global measure's directions block.
///
/// If `global/measures[0]/directions:` with a `tempo:` entry already exists, it updates the
/// `bpm:` field in-place.  Otherwise it inserts the `directions:` block under the first
/// global measure entry (after the `time:` line if present, otherwise right after the `- ` list
/// marker of the first measure entry).
pub fn set_bpm_in_yaml(yaml: &str, bpm: f64) -> String {
    let bpm_str = if bpm.fract() == 0.0 {
        format!("{:.0}", bpm)
    } else {
        format!("{}", bpm)
    };

    let lines: Vec<&str> = yaml.lines().collect();

    // 1. Try to find an existing `bpm:` line inside a tempo directive and update it in-place.
    {
        let mut in_global_measures = false;
        let mut first_measure_seen = false;
        let mut in_directions = false;
        let mut in_tempo = false;
        let mut base_indent = 0usize;

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            let indent = line.len() - line.trim_start().len();

            if trimmed == "global:" {
                in_global_measures = false;
                first_measure_seen = false;
            }
            if trimmed == "measures:" && !in_global_measures {
                in_global_measures = true;
                continue;
            }
            if in_global_measures && !first_measure_seen && trimmed.starts_with("- ") {
                first_measure_seen = true;
                base_indent = indent;
                in_directions = false;
                in_tempo = false;
            }
            if first_measure_seen && indent <= base_indent && trimmed.starts_with("- ") && !trimmed.starts_with("- time") && !trimmed.starts_with("- directions") {
                // We've moved past the first measure entry without finding BPM.
                break;
            }
            if first_measure_seen && trimmed.starts_with("directions:") {
                in_directions = true;
            }
            if in_directions && (trimmed.starts_with("- tempo:") || trimmed.starts_with("tempo:")) {
                if trimmed.contains("bpm:") {
                    // Inline format like `- tempo: { bpm: 140 }` or `tempo: { bpm: 140 }`
                    let leading = &line[..indent];
                    let mut result = lines[..i].join("\n");
                    if !result.is_empty() { result.push('\n'); }
                    // Recreate inline tempo entry preserving '-' if present
                    let prefix = if trimmed.starts_with("- ") { "- " } else { "" };
                    result.push_str(&format!("{}{}{}tempo: {{ bpm: {} }}", leading, prefix, if prefix.is_empty() { "" } else { "" }, bpm_str));
                    result.push('\n');
                    for later in &lines[i + 1..] {
                        result.push_str(later);
                        result.push('\n');
                    }
                    if !yaml.ends_with('\n') {
                        result = result.trim_end_matches('\n').to_string();
                    }
                    return result;
                }
                in_tempo = true;
            }
            if in_tempo && trimmed.starts_with("bpm:") {
                // Found existing bpm line – replace it.
                let leading = &line[..indent];
                let mut result = lines[..i].join("\n");
                if !result.is_empty() { result.push('\n'); }
                result.push_str(&format!("{}bpm: {}", leading, bpm_str));
                result.push('\n');
                for later in &lines[i + 1..] {
                    result.push_str(later);
                    result.push('\n');
                }
                // Trim trailing newline added if original didn't have one
                if !yaml.ends_with('\n') {
                    result = result.trim_end_matches('\n').to_string();
                }
                return result;
            }
        }
    }

    // 2. No existing bpm: found. Find the first global measure list item and append directions block.
    let mut in_global_section = false;
    let mut in_global_measures = false;
    let mut first_measure_line: Option<usize> = None;
    let mut first_measure_indent = 0usize;
    // We'll insert after the `time:` line within that entry, or after the `- ` marker itself.
    let mut insert_after_line: Option<usize> = None;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();

        if trimmed == "global:" {
            in_global_section = true;
            in_global_measures = false;
        } else if in_global_section && trimmed == "measures:" {
            in_global_measures = true;
        } else if in_global_measures && first_measure_line.is_none() && trimmed.starts_with("- ") {
            first_measure_line = Some(i);
            first_measure_indent = indent;
            insert_after_line = Some(i); // default: right after the `- …` line
        } else if first_measure_line.is_some() {
            // Track the last line belonging to the first measure (i.e. before the indent drops or next - item appears)
            if indent > first_measure_indent || (indent == first_measure_indent && !trimmed.starts_with("- ")) {
                insert_after_line = Some(i); // extend insertion point
            } else {
                break;
            }
        } else if in_global_measures && first_measure_line.is_some() {
            break;
        } else if !in_global_section && !in_global_measures {
            // Left global section
            if in_global_section {
                break;
            }
        }
    }

    if let Some(insert_after) = insert_after_line {
        // Determine indentation of child fields within the first measure (2 extra spaces past its `- ` indent).
        let child_indent = " ".repeat(first_measure_indent + 2);

        let directions_block = format!(
            "{}directions:\n{}  - tempo: {{ bpm: {} }}",
            child_indent,
            child_indent,
            bpm_str
        );

        let mut result = lines[..=insert_after].join("\n");
        result.push('\n');
        result.push_str(&directions_block);
        result.push('\n');
        for later in &lines[insert_after + 1..] {
            result.push_str(later);
            result.push('\n');
        }
        if !yaml.ends_with('\n') {
            result = result.trim_end_matches('\n').to_string();
        }
        return result;
    }

    // Fallback: return unchanged
    yaml.to_string()
}

/// Appends a new measure to `global/measures` and to the first part in `yaml_text`, containing a single blank note with `duration`.
pub fn add_measure_to_yaml(yaml: &str, duration: &crate::mnx::Duration) -> Option<String> {
    let lines: Vec<&str> = yaml.lines().collect();
    if lines.is_empty() {
        return None;
    }

    // 1. Find global/measures section to add a global measure entry
    let mut in_global = false;
    let mut global_measures_idx = None;
    let mut global_measure_item_idx = None;
    let mut global_item_indent = 4;
    let mut last_time_sig_str = "time: { count: 4, unit: 4 }".to_string();

    // 2. Find parts/measures section
    let mut in_parts = false;
    let mut parts_measures_idx = None;
    let mut seq_indent = 6;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();

        if trimmed.starts_with("global:") {
            in_global = true;
            in_parts = false;
            continue;
        }

        if in_global {
            if trimmed.starts_with("measures:") {
                global_measures_idx = Some(i);
                continue;
            }

            if global_measures_idx.is_some() {
                if trimmed.starts_with('-') {
                    global_measure_item_idx = Some(i);
                    global_item_indent = indent;
                    if let Some(pos) = trimmed.find("time:") {
                        last_time_sig_str = trimmed[pos..].to_string();
                    }
                } else if trimmed.starts_with("parts:") || (indent == 0 && !trimmed.is_empty()) {
                    in_global = false;
                }
            }
        }

        if trimmed.starts_with("parts:") {
            in_parts = true;
            in_global = false;
            continue;
        }

        if in_parts {
            if trimmed.starts_with("measures:") {
                parts_measures_idx = Some(i);
                continue;
            }

            if parts_measures_idx.is_some() && (trimmed.starts_with("- sequences:") || trimmed.starts_with("-sequences:")) {
                seq_indent = indent;
            }
        }
    }

    let _ = parts_measures_idx?;

    // Format new measure in global/measures if global/measures exists
    let mut result_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();

    if let Some(last_gm_idx) = global_measure_item_idx {
        // Insert after the last item in global/measures
        let new_gm_line = format!("{}- {}", " ".repeat(global_item_indent), last_time_sig_str);
        result_lines.insert(last_gm_idx + 1, new_gm_line);
    } else if let Some(gm_header_idx) = global_measures_idx {
        let new_gm_line = format!("{}- {}", " ".repeat(4), last_time_sig_str);
        result_lines.insert(gm_header_idx + 1, new_gm_line);
    }

    let intermediate_yaml = result_lines.join("\n");

    let dur_str = if let Some(dots) = duration.dots {
        if dots > 0 {
            format!("{{ base: {}, dots: {} }}", duration.base, dots)
        } else {
            format!("{{ base: {} }}", duration.base)
        }
    } else {
        format!("{{ base: {} }}", duration.base)
    };

    let seq_spaces = " ".repeat(seq_indent);
    let content_spaces = " ".repeat(seq_indent + 4);
    let event_spaces = " ".repeat(seq_indent + 8);
    let notes_spaces = " ".repeat(seq_indent + 10);

    let new_measure_block = format!(
        "{}- sequences:\n{}- content:\n{}- duration: {}\n{}notes: []",
        seq_spaces, content_spaces, event_spaces, dur_str, notes_spaces
    );

    let trimmed_yaml = intermediate_yaml.trim_end();
    Some(format!("{}\n{}\n", trimmed_yaml, new_measure_block))
}

/// Appends a new blank note (notes: []) to the last measure of the first part.
pub fn add_blank_note_to_last_measure(yaml: &str, duration: &crate::mnx::Duration) -> Option<String> {
    let event_ranges = find_all_event_ranges(yaml);
    let last_er = event_ranges.last()?;

    let event_first_line = &yaml[last_er.total_range.start..];
    let indent_len = event_first_line.len() - event_first_line.trim_start().len();
    let indent = " ".repeat(indent_len);
    let sub_indent = " ".repeat(indent_len + 2);

    let dur_str = if let Some(dots) = duration.dots {
        if dots > 0 {
            format!("{{ base: {}, dots: {} }}", duration.base, dots)
        } else {
            format!("{{ base: {} }}", duration.base)
        }
    } else {
        format!("{{ base: {} }}", duration.base)
    };

    let new_event_str = format!(
        "\n{}- duration: {}\n{}notes: []",
        indent, dur_str, sub_indent
    );

    let mut result = yaml.to_string();
    result.insert_str(last_er.total_range.end, &new_event_str);
    Some(result)
}

/// If the final event in the last measure is blank (notes: []), removes it from the YAML.
/// If it was the only event in that measure, removes the entire measure (and the corresponding global measure if applicable).
pub fn remove_last_blank_event(yaml: &str) -> Option<String> {
    let doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml).ok()?;
    let part = doc.parts.first()?;
    let last_measure = part.measures.last()?;
    let last_seq = last_measure.sequences.first()?;
    let last_event = last_seq.content.last()?;

    if !last_event.notes.is_empty() {
        return None;
    }

    if last_seq.content.len() > 1 {
        let event_ranges = find_all_event_ranges(yaml);
        let last_er = event_ranges.last()?;
        let mut start = last_er.total_range.start;
        let end = last_er.total_range.end;
        if start > 0 && yaml.as_bytes()[start - 1] == b'\n' {
            start -= 1;
        }
        let mut result = yaml.to_string();
        result.replace_range(start..end, "");
        return Some(result);
    }

    // Only 1 event in the measure -> remove the last measure entirely
    let lines: Vec<&str> = yaml.lines().collect();
    let mut last_seq_line_idx = None;
    let mut in_parts = false;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("parts:") {
            in_parts = true;
            continue;
        }
        if in_parts && (trimmed.starts_with("- sequences:") || trimmed.starts_with("-sequences:")) {
            last_seq_line_idx = Some(i);
        }
    }

    let seq_line_idx = last_seq_line_idx?;

    // Also identify the corresponding global measure entry if it exists
    let mut in_global = false;
    let mut global_measure_item_idx = None;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();

        if trimmed.starts_with("global:") {
            in_global = true;
            continue;
        }
        if in_global {
            if trimmed.starts_with('-') {
                global_measure_item_idx = Some(i);
            } else if trimmed.starts_with("parts:") || (indent == 0 && !trimmed.is_empty()) {
                in_global = false;
            }
        }
    }

    let mut result_lines = Vec::new();
    for (i, &line) in lines.iter().enumerate() {
        if let Some(gm_idx) = global_measure_item_idx {
            if doc.global.measures.len() == part.measures.len() && i == gm_idx {
                continue;
            }
        }
        if i >= seq_line_idx {
            continue;
        }
        result_lines.push(line);
    }

    let trimmed = result_lines.join("\n");
    Some(format!("{}\n", trimmed.trim_end()))
}

impl MnxEditorApp {
    pub fn get_selected_event_duration(&self) -> Option<crate::mnx::Duration> {
        let e_idx = self.selected_event_idx?;
        let doc = self.parsed_mnx.as_ref()?;
        let part = doc.parts.first()?;
        let mut cur_e = 0;
        for m in &part.measures {
            for seq in &m.sequences {
                for ev in &seq.content {
                    if cur_e == e_idx {
                        return Some(ev.duration.clone());
                    }
                    cur_e += 1;
                }
            }
        }
        None
    }

    pub fn change_selected_duration_base(&mut self, new_base: &str) {
        if let Some(e_idx) = self.selected_event_idx {
            let cur_dur = self.get_selected_event_duration().unwrap_or_else(|| crate::mnx::Duration {
                base: "quarter".to_string(),
                dots: None,
            });
            let new_duration = crate::mnx::Duration {
                base: new_base.to_string(),
                dots: cur_dur.dots,
            };
            if let Some(new_yaml) = update_event_duration_in_yaml(&self.yaml_text, e_idx, &new_duration) {
                self.yaml_text = new_yaml;
                self.process_and_render();
            }
        }
    }

    pub fn toggle_selected_duration_dot(&mut self) {
        if let Some(e_idx) = self.selected_event_idx {
            let cur_dur = self.get_selected_event_duration().unwrap_or_else(|| crate::mnx::Duration {
                base: "quarter".to_string(),
                dots: None,
            });
            let is_dotted = cur_dur.dots.unwrap_or(0) > 0 || cur_dur.base.contains("dotted") || cur_dur.base.contains('.');
            let clean_base = cur_dur.base
                .replace("dotted-", "")
                .replace("-dotted", "")
                .replace("dotted", "")
                .replace('.', "");
            let new_duration = crate::mnx::Duration {
                base: if clean_base.is_empty() { "quarter".to_string() } else { clean_base },
                dots: if is_dotted { None } else { Some(1) },
            };
            if let Some(new_yaml) = update_event_duration_in_yaml(&self.yaml_text, e_idx, &new_duration) {
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
                    let ch_1_based = self.string_channel_map.get(&s).copied().unwrap_or(default_ch);
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
                        let port_name = self.midi_ports.iter().find(|p| p.id == port_idx)
                            .map(|p| p.name.as_str()).unwrap_or("MIDI device");
                        let mode_str = if self.is_mpe_mode { " (MPE)" } else { "" };
                        if let Some(start_idx) = start_event {
                            if start_idx > 0 {
                                self.midi_status_msg = Some(format!("Playing from note {}{} on {}", start_idx + 1, mode_str, port_name));
                            } else {
                                self.midi_status_msg = Some(format!("Playing from start{} on {}", mode_str, port_name));
                            }
                        } else {
                            self.midi_status_msg = Some(format!("Playing{} on {}", mode_str, port_name));
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
                for ev in &seq.content {
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

        let target_notes: Vec<&crate::mnx::Note> = if let Some(n_idx) = self.selected_note_idx_in_event {
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
                let ch_1_based = self.string_channel_map.get(&string).copied().unwrap_or(default_ch);
                let ch_0_based = (ch_1_based.saturating_sub(1)) % 16;
                let channel = midly::num::u4::from(ch_0_based);

                if let Some(pos_state) = freq_table.get(string, fret) {
                    let midi_note_frac = pos_state.midi_note_number;
                    let base_note = midi_note_frac.round() as u8;
                    let semitone_diff = (midi_note_frac - (base_note as f64)) as f32;

                    let pitch_bend_val = crate::midi::calculate_pitch_bend(semitone_diff, self.pitch_bend_range);

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

    pub fn add_measure_to_end(&mut self) {
        let last_duration = self.parsed_mnx
            .as_ref()
            .and_then(|d| d.parts.first())
            .and_then(|p| p.measures.last())
            .and_then(|m| m.sequences.first())
            .and_then(|s| s.content.last())
            .map(|e| e.duration.clone())
            .unwrap_or_else(|| crate::mnx::Duration {
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
                self.validation_errors = errors.iter()
                    .map(|err| {
                        if err.string > 0 {
                            format!("Measure {}, Event {}: {}", err.measure_idx, err.event_idx + 1, err.message)
                        } else {
                            format!("Measure {}: {}", err.measure_idx, err.message)
                        }
                    })
                    .collect();

                // Sync BPM from YAML if a tempo directive is present
                let yaml_bpm = doc.global.measures.iter()
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

        // In-place Fret Editing Key Handling
        if self.is_editing_fret {
            let esc_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));
            let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
            let backspace_pressed = ui.input(|i| i.key_pressed(egui::Key::Backspace));
            let delete_pressed = ui.input(|i| i.key_pressed(egui::Key::Delete));
            let up_pressed = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
            let down_pressed = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));

            let num_strings = self.parsed_mnx.as_ref()
                .and_then(|d| d.parts.first())
                .map(|p| p.num_strings())
                .unwrap_or(6) as u8;

            if esc_pressed {
                self.is_editing_fret = false;
                self.editing_fret_buf.clear();
            } else if enter_pressed {
                if let Some(e_idx) = self.selected_event_idx {
                    if let Some(cur_str) = self.editing_string {
                        if let Ok(new_fret) = self.editing_fret_buf.trim().parse::<u8>() {
                            let new_pitch = self.parsed_mnx.as_ref()
                                .and_then(|d| d.parts.first())
                                .and_then(|part| calculate_pitch_for_string_fret(part, cur_str, new_fret));

                            if let Some(new_yaml) = set_or_add_note_on_string(
                                &self.yaml_text, e_idx, cur_str, new_fret, new_pitch.as_ref()
                            ) {
                                self.yaml_text = new_yaml;
                                self.process_and_render();
                            }
                        }
                    }
                }
                self.is_editing_fret = false;
                self.editing_fret_buf.clear();
            } else if up_pressed || down_pressed {
                // If the user typed digits before moving string, commit them to the current string first
                if let Some(e_idx) = self.selected_event_idx {
                    if let Some(cur_str) = self.editing_string {
                        if let Ok(new_fret) = self.editing_fret_buf.trim().parse::<u8>() {
                            let new_pitch = self.parsed_mnx.as_ref()
                                .and_then(|d| d.parts.first())
                                .and_then(|part| calculate_pitch_for_string_fret(part, cur_str, new_fret));

                            if let Some(new_yaml) = set_or_add_note_on_string(
                                &self.yaml_text, e_idx, cur_str, new_fret, new_pitch.as_ref()
                            ) {
                                self.yaml_text = new_yaml;
                                self.process_and_render();
                            }
                        }
                    }
                }
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
                        self.selected_note_idx_in_event = None;
                        if let Some(doc) = &self.parsed_mnx {
                            if let Some(part) = doc.parts.first() {
                                let mut cur_e = 0;
                                'find_n: for m in &part.measures {
                                    for seq in &m.sequences {
                                        for ev in &seq.content {
                                            if cur_e == e_idx {
                                                for (n_i, n) in ev.notes.iter().enumerate() {
                                                    if n.string == Some(cur_str) {
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
                }
            } else if backspace_pressed {
                if !self.editing_fret_buf.is_empty() {
                    self.editing_fret_buf.pop();
                } else if let (Some(e_idx), Some(cur_str)) = (self.selected_event_idx, self.editing_string) {
                    if let Some(new_yaml) = remove_note_on_string(&self.yaml_text, e_idx, cur_str) {
                        self.yaml_text = new_yaml;
                        self.process_and_render();
                        self.selected_note_idx_in_event = None;
                    }
                }
            } else if delete_pressed {
                if let (Some(e_idx), Some(cur_str)) = (self.selected_event_idx, self.editing_string) {
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

            let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
            let left_pressed = ui.input(|i| i.key_pressed(egui::Key::ArrowLeft));
            let right_pressed = ui.input(|i| i.key_pressed(egui::Key::ArrowRight));
            let up_pressed = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
            let down_pressed = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));

            if enter_pressed && self.selected_event_idx.is_some() {
                let e_idx = self.selected_event_idx.unwrap();
                let mut initial_string = 1;
                if let Some(doc) = &self.parsed_mnx {
                    if let Some(part) = doc.parts.first() {
                        let mut cur_e = 0;
                        'find_init: for m in &part.measures {
                            for seq in &m.sequences {
                                for ev in &seq.content {
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
                    let at_final_event = self.selected_event_idx == Some(total_events.saturating_sub(1));
                    let is_final_blank = at_final_event && self.parsed_mnx.as_ref()
                        .and_then(|d| d.parts.first())
                        .and_then(|p| p.measures.last())
                        .and_then(|m| m.sequences.first())
                        .and_then(|s| s.content.last())
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
                    let at_final_event = self.selected_event_idx == Some(total_events.saturating_sub(1));

                    if at_final_event {
                        let is_final_blank = self.parsed_mnx.as_ref()
                            .and_then(|d| d.parts.first())
                            .and_then(|p| p.measures.last())
                            .and_then(|m| m.sequences.first())
                            .and_then(|s| s.content.last())
                            .map(|e| e.notes.is_empty())
                            .unwrap_or(false);

                        if is_final_blank {
                            // Rule 3: Moving further to the right of a final blank note has no effect.
                        } else if let Some(doc) = &self.parsed_mnx {
                            if let Some(part) = doc.parts.first() {
                                if let Some(last_measure) = part.measures.last() {
                                    if let Some(last_seq) = last_measure.sequences.first() {
                                        if let Some(last_event) = last_seq.content.last() {
                                            let m_idx = part.measures.len() - 1;
                                            let final_dur = &last_event.duration;
                                            let dur_beats = validate::duration_to_quarter_beats(final_dur);
                                            let time_sig = validate::get_time_signature_for_measure(doc, m_idx);
                                            let max_beats = (time_sig.count as f64) * (4.0 / time_sig.unit as f64);
                                            let curr_beats: f64 = last_seq.content.iter().map(|e| validate::duration_to_quarter_beats(&e.duration)).sum();

                                            if curr_beats + dur_beats <= max_beats + 1e-4 {
                                                // Rule 1: Measure would NOT be completed by additional note -> add blank note to current measure
                                                if let Some(new_yaml) = add_blank_note_to_last_measure(&self.yaml_text, final_dur) {
                                                    self.yaml_text = new_yaml;
                                                    self.process_and_render();
                                                    let new_total = find_all_event_ranges(&self.yaml_text).len();
                                                    if new_total > 0 {
                                                        self.selected_event_idx = Some(new_total - 1);
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
                                                for ev in &seq.content {
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
                                    self.selected_note_idx_in_event = match self.selected_note_idx_in_event {
                                        None => Some(note_order[0]),
                                        Some(cur) => {
                                            let cur_pos = note_order.iter().position(|&x| x == cur).unwrap_or(0);
                                            let new_pos = cur_pos.saturating_sub(1);
                                            Some(note_order[new_pos])
                                        }
                                    };
                                }

                                if down_pressed {
                                    // Down: move to lower string (later in note_order)
                                    self.selected_note_idx_in_event = match self.selected_note_idx_in_event {
                                        None => Some(note_order[0]),
                                        Some(cur) => {
                                            let cur_pos = note_order.iter().position(|&x| x == cur).unwrap_or(0);
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

        // 1. Top Menu Bar (Updated with file operations & Duration Toolbar)
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
                }

                if let Some(action) = clicked_action {
                    match action {
                        DurationAction::Base(b) => self.change_selected_duration_base(b),
                        DurationAction::ToggleDot => self.toggle_selected_duration_dot(),
                    }
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

        // 2. Bottom Panel for Errors (only shown if errors exist)
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

        // 3. Right Pane: YAML Text Editor
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

                    let mut layouter = |ui: &egui::Ui, text_buf: &dyn egui::TextBuffer, wrap_width: f32| {
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

        // 4. Left Pane: Render Output
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::both().show(ui, |ui| {
                if let Some(doc) = &self.parsed_mnx {
                    if let Some(part) = doc.parts.first() {
                        let (_resp, clicked, add_bar_clicked) = render::draw_tablature(
                            ui,
                            part,
                            &self.parsed_errors,
                            self.selected_event_idx,
                            self.selected_note_idx_in_event,
                            self.is_editing_fret,
                            &self.editing_fret_buf,
                            self.editing_string,
                        );
                        if add_bar_clicked {
                            self.add_measure_to_end();
                        } else if let Some((clicked_event, clicked_note)) = clicked {
                            let total_events = find_all_event_ranges(&self.yaml_text).len();
                            let was_at_final = self.selected_event_idx == Some(total_events.saturating_sub(1));
                            let is_final_blank = was_at_final && self.parsed_mnx.as_ref()
                                .and_then(|d| d.parts.first())
                                .and_then(|p| p.measures.last())
                                .and_then(|m| m.sequences.first())
                                .and_then(|s| s.content.last())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_all_event_ranges_example_yaml() {
        let yaml = include_str!("../example.yaml");
        let ranges = find_all_event_ranges(yaml);
        assert_eq!(ranges.len(), 15);
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

        let updated_doc = serde_saphyr::from_str::<MnxDocument>(&updated).expect("Updated doc should parse");
        let errors = crate::validate::validate_mnx(&updated_doc);
        assert_eq!(errors.len(), 0, "There should be no validation errors after updating fret and pitch: {:?}", errors);
    }

    #[test]
    fn test_edit_string4_fret12() {
        let yaml = include_str!("../example.yaml");
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let part = &doc.parts[0];

        // Edit Event 1 to put String 4 Fret 12
        let new_pitch = calculate_pitch_for_string_fret(part, 4, 12);
        let updated = set_or_add_note_on_string(yaml, 1, 4, 12, new_pitch.as_ref()).expect("Should update note");

        let updated_doc = serde_saphyr::from_str::<MnxDocument>(&updated).expect("Updated doc should parse");
        let errors = crate::validate::validate_mnx(&updated_doc);
        assert_eq!(errors.len(), 0, "Validation should pass with 0 errors for String 4 Fret 12: {:?}", errors);
    }

    #[test]
    fn test_set_or_add_note_on_string_creates_chord() {
        let yaml = include_str!("../example.yaml");
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let part = &doc.parts[0];

        // Event 0 starts with String 7 Fret 12. Add String 6 Fret 9 (D3) to make it a 2-note chord.
        let new_pitch = calculate_pitch_for_string_fret(part, 6, 9);
        let updated = set_or_add_note_on_string(yaml, 0, 6, 9, new_pitch.as_ref()).expect("Should add note");

        let updated_doc = serde_saphyr::from_str::<MnxDocument>(&updated).expect("Updated doc should parse");
        let ev0_notes = &updated_doc.parts[0].measures[0].sequences[0].content[0].notes;
        assert_eq!(ev0_notes.len(), 2);
        assert_eq!(ev0_notes[0].string, Some(7));
        assert_eq!(ev0_notes[0].fret, Some(12));
        assert_eq!(ev0_notes[1].string, Some(6));
        assert_eq!(ev0_notes[1].fret, Some(9));

        let errors = crate::validate::validate_mnx(&updated_doc);
        assert_eq!(errors.len(), 0, "Validation should pass with 0 errors: {:?}", errors);
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
        let updated_doc = serde_saphyr::from_str::<MnxDocument>(&updated).expect("Updated doc should parse");
        let notes = &updated_doc.parts[0].measures[0].sequences[0].content[0].notes;
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].string, Some(5));

        // Remove note on string 5 (only note left)
        let updated2 = remove_note_on_string(&updated, 0, 5).expect("Should remove last note");
        let updated2_doc = serde_saphyr::from_str::<MnxDocument>(&updated2).expect("Updated doc should parse");
        let notes2 = &updated2_doc.parts[0].measures[0].sequences[0].content[0].notes;
        assert_eq!(notes2.len(), 0);
    }

    #[test]
    fn test_add_measure_to_yaml() {
        let yaml = include_str!("../example.yaml");
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let initial_measures = doc.parts[0].measures.len();
        assert_eq!(initial_measures, 2);

        let final_dur = doc.parts[0].measures.last()
            .unwrap().sequences.first()
            .unwrap().content.last()
            .unwrap().duration.clone();
        assert_eq!(final_dur.base, "quarter");

        let updated = add_measure_to_yaml(yaml, &final_dur).expect("Should add measure");
        let updated_doc = serde_saphyr::from_str::<MnxDocument>(&updated).expect("Updated doc should parse");
        assert_eq!(updated_doc.global.measures.len(), 3);
        let part = &updated_doc.parts[0];
        assert_eq!(part.measures.len(), 3);

        // Check the newly added measure
        let new_measure = &part.measures[2];
        let new_seq = &new_measure.sequences[0];
        assert_eq!(new_seq.content.len(), 1);
        let new_event = &new_seq.content[0];
        assert_eq!(new_event.duration.base, "quarter");
        assert_eq!(new_event.notes.len(), 0);

        // Validation should pass with 0 errors
        let errors = crate::validate::validate_mnx(&updated_doc);
        assert_eq!(errors.len(), 0, "Validation should have 0 errors: {:?}", errors);
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
        let dur = crate::mnx::Duration { base: "quarter".to_string(), dots: None };
        let with_blank = add_blank_note_to_last_measure(yaml, &dur).expect("Should add blank note");
        let doc_with_blank = serde_saphyr::from_str::<MnxDocument>(&with_blank).expect("Should parse");
        assert_eq!(doc_with_blank.parts[0].measures[0].sequences[0].content.len(), 2);
        assert_eq!(doc_with_blank.parts[0].measures[0].sequences[0].content[1].notes.len(), 0);

        // Now remove the blank note (since nothing was entered into it)
        let cleaned = remove_last_blank_event(&with_blank).expect("Should remove blank note");
        let cleaned_doc = serde_saphyr::from_str::<MnxDocument>(&cleaned).expect("Should parse");
        assert_eq!(cleaned_doc.parts[0].measures[0].sequences[0].content.len(), 1);
        assert_eq!(cleaned_doc.parts[0].measures[0].sequences[0].content[0].notes.len(), 1);
    }

    #[test]
    fn test_add_measure_and_remove_blank_measure() {
        let yaml = include_str!("../example.yaml");
        let dur = crate::mnx::Duration { base: "quarter".to_string(), dots: None };
        let with_new_measure = add_measure_to_yaml(yaml, &dur).expect("Should add measure");
        let doc1 = serde_saphyr::from_str::<MnxDocument>(&with_new_measure).expect("Should parse");
        assert_eq!(doc1.parts[0].measures.len(), 3);
        assert_eq!(doc1.global.measures.len(), 3);

        // When shifting focus back to the left without entering anything, the empty measure is removed
        let cleaned = remove_last_blank_event(&with_new_measure).expect("Should remove empty measure");
        let doc2 = serde_saphyr::from_str::<MnxDocument>(&cleaned).expect("Should parse");
        assert_eq!(doc2.parts[0].measures.len(), 2);
        assert_eq!(doc2.global.measures.len(), 2);
    }

    #[test]
    fn test_right_arrow_rules_and_lifecycle() {
        let mut app = MnxEditorApp::default();
        // App starts with example.yaml (15 events across 2 measures)
        let total_initial = find_all_event_ranges(&app.yaml_text).len();
        assert_eq!(total_initial, 15);

        // Select the final event (event 14, in Measure 2 which is full)
        app.selected_event_idx = Some(14);

        // Right arrow on full final bar -> Rule 2: creates Measure 3 with a blank note
        // (Simulate right arrow logic)
        let final_dur = app.parsed_mnx.as_ref()
            .and_then(|d| d.parts.first())
            .and_then(|p| p.measures.last())
            .and_then(|m| m.sequences.first())
            .and_then(|s| s.content.last())
            .map(|e| e.duration.clone()).unwrap();

        let updated = add_measure_to_yaml(&app.yaml_text, &final_dur).unwrap();
        app.yaml_text = updated;
        app.process_and_render();
        let total_after_r2 = find_all_event_ranges(&app.yaml_text).len();
        assert_eq!(total_after_r2, 16);
        app.selected_event_idx = Some(15);

        // Rule 3: Moving further to the right of a final blank note has no effect
        let is_final_blank = app.parsed_mnx.as_ref()
            .and_then(|d| d.parts.first())
            .and_then(|p| p.measures.last())
            .and_then(|m| m.sequences.first())
            .and_then(|s| s.content.last())
            .map(|e| e.notes.is_empty()).unwrap();
        assert!(is_final_blank);

        // Shifting focus back to the left -> blank note and measure disappear
        let cleaned = remove_last_blank_event(&app.yaml_text).unwrap();
        app.yaml_text = cleaned;
        app.process_and_render();
        let total_after_left = find_all_event_ranges(&app.yaml_text).len();
        assert_eq!(total_after_left, 15);
    }

    #[test]
    fn test_update_event_duration_in_yaml() {
        let yaml = include_str!("../example.yaml");
        let dur_quarter = crate::mnx::Duration { base: "quarter".to_string(), dots: None };
        let updated = update_event_duration_in_yaml(yaml, 0, &dur_quarter).expect("Should update duration");
        let doc = serde_saphyr::from_str::<MnxDocument>(&updated).expect("Should parse");
        assert_eq!(doc.parts[0].measures[0].sequences[0].content[0].duration.base, "quarter");
        assert_eq!(doc.parts[0].measures[0].sequences[0].content[0].duration.dots, None);

        // Test dotted
        let dur_dotted_half = crate::mnx::Duration { base: "half".to_string(), dots: Some(1) };
        let updated_dot = update_event_duration_in_yaml(&updated, 0, &dur_dotted_half).expect("Should update to dotted");
        let doc_dot = serde_saphyr::from_str::<MnxDocument>(&updated_dot).expect("Should parse");
        assert_eq!(doc_dot.parts[0].measures[0].sequences[0].content[0].duration.base, "half");
        assert_eq!(doc_dot.parts[0].measures[0].sequences[0].content[0].duration.dots, Some(1));
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
        let doc = serde_saphyr::from_str::<MnxDocument>(&result).expect("Should parse after BPM insert");
        println!("DOC GLOBAL: {:?}", doc.global);
        let bpm = doc.global.measures.iter()
            .flat_map(|m| m.directions.iter())
            .find_map(|d| d.tempo.as_ref().map(|t| t.bpm));
        assert_eq!(bpm, Some(140.0), "BPM should be 140 after insert");
    }

    #[test]
    fn test_set_bpm_in_yaml_update() {
        let yaml = include_str!("../example.yaml");
        let with_bpm = set_bpm_in_yaml(yaml, 140.0);
        let updated = set_bpm_in_yaml(&with_bpm, 200.0);
        let doc = serde_saphyr::from_str::<MnxDocument>(&updated).expect("Should parse after BPM update");
        let bpm = doc.global.measures.iter()
            .flat_map(|m| m.directions.iter())
            .find_map(|d| d.tempo.as_ref().map(|t| t.bpm));
        assert_eq!(bpm, Some(200.0), "BPM should be updated to 200");
        let tempo_count = doc.global.measures.iter()
            .flat_map(|m| m.directions.iter())
            .filter(|d| d.tempo.is_some())
            .count();
        assert_eq!(tempo_count, 1, "Should only have one tempo directive");
    }
}
