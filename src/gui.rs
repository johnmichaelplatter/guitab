use eframe::egui::{self, Color32};

use std::fs;
use crate::mnx::MnxDocument;
use crate::validate;
use crate::render;

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
}

impl Default for MnxEditorApp {
    fn default() -> Self {
        let initial_yaml = fs::read_to_string("example.yaml")
            .unwrap_or_else(|_| String::from("# Could not load example.yaml"));
        
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

impl MnxEditorApp {
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
        } else if !editor_focused && total_events > 0 {
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
                    self.selected_event_idx = match self.selected_event_idx {
                        Some(idx) => Some(idx.saturating_sub(1)),
                        None => Some(total_events.saturating_sub(1)),
                    };
                    self.selected_note_idx_in_event = None;
                    self.editing_string = None;
                }
                if right_pressed {
                    self.selected_event_idx = match self.selected_event_idx {
                        Some(idx) => Some((idx + 1).min(total_events - 1)),
                        None => Some(0),
                    };
                    self.selected_note_idx_in_event = None;
                    self.editing_string = None;
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
                            }
                        }
                    }
                }
            }
        }

        // 1. Top Menu Bar (Updated with file operations)
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
            ui.heading("Tablature Render");
            ui.separator();
            egui::ScrollArea::both().show(ui, |ui| {
                if let Some(doc) = &self.parsed_mnx {
                    if let Some(part) = doc.parts.first() {
                        let (_resp, clicked) = render::draw_tablature(
                            ui,
                            part,
                            &self.parsed_errors,
                            self.selected_event_idx,
                            self.selected_note_idx_in_event,
                            self.is_editing_fret,
                            &self.editing_fret_buf,
                            self.editing_string,
                        );
                        if let Some((clicked_event, clicked_note)) = clicked {
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
}