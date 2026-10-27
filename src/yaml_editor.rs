#[derive(Clone, Debug, PartialEq)]
pub struct EventRange {
    pub total_range: std::ops::Range<usize>,
    pub note_ranges: Vec<std::ops::Range<usize>>,
}

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

        let event_text = &yaml[total_start..total_end];
        if event_text.contains("type: tuplet")
            || event_text.contains("type: \"tuplet\"")
            || event_text.contains("type: 'tuplet'")
            || event_text.contains("type: grace")
            || event_text.contains("type: \"grace\"")
            || event_text.contains("type: 'grace'")
            || (event_text.contains("inner:") && event_text.contains("outer:"))
        {
            return;
        }

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
                notes_indent = line.find("notes:").unwrap_or(indent);
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
            if !in_content {
                in_content = true;
                content_indent = line.find("content:").unwrap_or(indent);
            }
            continue;
        }

        if in_content {
            if (trimmed.starts_with('-') && indent < content_indent)
                || (!trimmed.starts_with('-') && indent <= content_indent)
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

/// Calculates the exact microtonal Pitch for a given string and fret (including custom fretboards).
pub fn calculate_pitch_for_string_fret(
    part: &crate::mnx::Part,
    string: u8,
    fret: i32,
) -> Option<crate::mnx::Pitch> {
    calculate_pitch_for_string_decimal_fret(part, string, fret as f64)
}

/// Calculates the exact microtonal or custom-fretboard Pitch for a given string and decimal fret (e.g. `1.5`).
pub fn calculate_pitch_for_string_decimal_fret(
    part: &crate::mnx::Part,
    string: u8,
    decimal_fret: f64,
) -> Option<crate::mnx::Pitch> {
    let string_pitch = part.get_string_tuning(string)?;

    if let Some(custom_frets) = &part.ext.custom_frets {
        if !custom_frets.is_empty() {
            if decimal_fret.abs() < 1e-4 {
                return Some(string_pitch);
            }
            let mode =
                crate::fretboard_editor::classify_fretboard_alter_mode(custom_frets, 261.626);
            if let Some(cf) = custom_frets
                .iter()
                .find(|f| (f.fret_number - decimal_fret).abs() < 0.05 && f.strings.contains(&string))
            {
                return Some(crate::fretboard_editor::calculate_custom_fret_pitch(
                    &string_pitch,
                    Some(cf),
                    &mode,
                ));
            }
        }
    }

    let capo_fret = part.capo_fret();
    let edo = part.edo();
    let step_multiplier = part.step_multiplier();
    let tuning_calc = crate::edo::EdoTuning::new(edo);
    let string_base = tuning_calc.pitch_to_steps(&string_pitch);
    let physical_capo_steps = (capo_fret as i32) * step_multiplier;
    let physical_fret_steps = (decimal_fret.round() as i32) * step_multiplier;
    let played_steps = string_base + physical_capo_steps + physical_fret_steps;
    Some(tuning_calc.steps_to_pitch(played_steps))
}

/// Helper to parse the `string: <n>` value out of an individual note YAML block text.
pub fn get_note_string_from_text(note_text: &str) -> Option<u8> {
    if let Some(pos) = note_text.find("string:") {
        let after = note_text[pos + "string:".len()..].trim_start();
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(s) = digits.parse::<u8>() {
            return Some(s);
        }
    }
    None
}

fn new_blank_event(duration: crate::mnx::Duration) -> crate::mnx::Event {
    crate::mnx::Event {
        duration,
        ..Default::default()
    }
}

fn new_rest_event(duration: crate::mnx::Duration) -> crate::mnx::Event {
    crate::mnx::Event {
        duration,
        rest: Some(crate::mnx::Rest {}),
        ..Default::default()
    }
}

/// Replaces the fret number and pitch of the given note inside `doc`.
pub fn update_note_in_doc(
    doc: &mut crate::mnx::MnxDocument,
    event_idx: usize,
    note_idx: usize,
    new_fret: i32,
    new_pitch: Option<&crate::mnx::Pitch>,
) -> bool {
    let paths = doc.get_flattened_event_paths();
    let Some(&path) = paths.get(event_idx) else {
        return false;
    };
    let Some(ev) = doc.get_event_mut_at_path(path) else {
        return false;
    };
    let Some(note) = ev.notes.get_mut(note_idx) else {
        return false;
    };
    note.fret = Some(new_fret);
    note.decimal_fret = None;
    note.muted = None;
    if let Some(pitch) = new_pitch {
        note.pitch = pitch.clone();
    }
    true
}

/// Replaces the fret number and pitch of the given note inside yaml_text and returns the updated YAML string.
pub fn update_note_in_yaml(
    yaml: &str,
    event_idx: usize,
    note_idx: usize,
    new_fret: i32,
    new_pitch: Option<&crate::mnx::Pitch>,
) -> Option<String> {
    let mut doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml).ok()?;
    if !update_note_in_doc(&mut doc, event_idx, note_idx, new_fret, new_pitch) {
        return None;
    }
    serde_saphyr::to_string(&doc).ok()
}

/// Updates an existing note on `target_string` in `event_idx`, or appends a new note on `target_string` to `event_idx` if none exists.
pub fn set_or_add_note_on_string_in_doc(
    doc: &mut crate::mnx::MnxDocument,
    event_idx: usize,
    target_string: u8,
    new_fret: i32,
    new_pitch: Option<&crate::mnx::Pitch>,
) -> bool {
    set_or_add_decimal_note_on_string_in_doc(doc, event_idx, target_string, new_fret as f64, new_pitch)
}

/// Updates an existing note on `target_string` in `event_idx` with a whole or decimal fret (e.g. `1.5`), or appends a new note.
pub fn set_or_add_decimal_note_on_string_in_doc(
    doc: &mut crate::mnx::MnxDocument,
    event_idx: usize,
    target_string: u8,
    decimal_fret: f64,
    new_pitch: Option<&crate::mnx::Pitch>,
) -> bool {
    let paths = doc.get_flattened_event_paths();
    let Some(&path) = paths.get(event_idx) else {
        return false;
    };
    let Some(ev) = doc.get_event_mut_at_path(path) else {
        return false;
    };
    ev.rest = None;

    let int_fret = decimal_fret.round() as i32;
    let is_decimal = (decimal_fret - (int_fret as f64)).abs() >= 0.05;
    let dec_opt = if is_decimal {
        Some((decimal_fret * 10.0).round() / 10.0)
    } else {
        None
    };

    if let Some(existing) = ev.notes.iter_mut().find(|n| n.string == Some(target_string)) {
        existing.fret = Some(int_fret);
        existing.decimal_fret = dec_opt;
        existing.muted = None;
        if let Some(p) = new_pitch {
            existing.pitch = p.clone();
        }
    } else {
        let pitch = new_pitch.cloned().unwrap_or(crate::mnx::Pitch {
            step: "C".to_string(),
            alter: None,
            octave: 4,
            ext: crate::mnx::PitchExt::default(),
        });
        ev.notes.push(crate::mnx::Note {
            pitch,
            ext: crate::mnx::NoteExt {
                string: Some(target_string),
                fret: Some(int_fret),
                decimal_fret: dec_opt,
                ..Default::default()
            },
            ..Default::default()
        });
    }
    true
}

/// Replaces all notes in `event_idx` with the given `string -> fret` chord map (used by Grid Controller Input Mode).
pub fn set_chord_on_event_in_doc(
    doc: &mut crate::mnx::MnxDocument,
    event_idx: usize,
    chord: &std::collections::BTreeMap<u8, i32>,
) -> bool {
    let part_clone = match doc.parts.first().cloned() {
        Some(p) => p,
        None => return false,
    };
    let paths = doc.get_flattened_event_paths();
    let Some(&path) = paths.get(event_idx) else {
        return false;
    };
    let Some(ev) = doc.get_event_mut_at_path(path) else {
        return false;
    };
    ev.rest = None;
    ev.notes.clear();

    for (&string_num, &fret_val) in chord {
        let pitch = calculate_pitch_for_string_fret(&part_clone, string_num, fret_val).unwrap_or(
            crate::mnx::Pitch {
                step: "C".to_string(),
                alter: None,
                octave: 4,
                ext: crate::mnx::PitchExt::default(),
            },
        );
        ev.notes.push(crate::mnx::Note {
            pitch,
            ext: crate::mnx::NoteExt {
                string: Some(string_num),
                fret: Some(fret_val),
                ..Default::default()
            },
            ..Default::default()
        });
    }
    true
}

/// Updates an existing note on `target_string` in `event_idx`, or appends a new note on `target_string` to `event_idx` if none exists.
pub fn set_or_add_note_on_string(
    yaml: &str,
    event_idx: usize,
    target_string: u8,
    new_fret: i32,
    new_pitch: Option<&crate::mnx::Pitch>,
) -> Option<String> {
    let mut doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml).ok()?;
    if !set_or_add_note_on_string_in_doc(&mut doc, event_idx, target_string, new_fret, new_pitch) {
        return None;
    }
    serde_saphyr::to_string(&doc).ok()
}

/// Removes the note on `target_string` from the event at `event_idx` in `doc`.
pub fn remove_note_on_string_in_doc(
    doc: &mut crate::mnx::MnxDocument,
    event_idx: usize,
    target_string: u8,
) -> bool {
    let paths = doc.get_flattened_event_paths();
    let Some(&path) = paths.get(event_idx) else {
        return false;
    };
    let Some(ev) = doc.get_event_mut_at_path(path) else {
        return false;
    };
    let initial_len = ev.notes.len();
    ev.notes.retain(|n| n.string != Some(target_string));
    ev.notes.len() != initial_len
}

/// Removes the note on `target_string` from the event at `event_idx`.
pub fn remove_note_on_string(yaml: &str, event_idx: usize, target_string: u8) -> Option<String> {
    let mut doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml).ok()?;
    if !remove_note_on_string_in_doc(&mut doc, event_idx, target_string) {
        return None;
    }
    serde_saphyr::to_string(&doc).ok()
}

/// Replaces the `duration` property of the event at `event_idx` with `new_duration` in `doc`.
pub fn update_event_duration_in_doc(
    doc: &mut crate::mnx::MnxDocument,
    event_idx: usize,
    new_duration: &crate::mnx::Duration,
) -> bool {
    let paths = doc.get_flattened_event_paths();
    let Some(&path) = paths.get(event_idx) else {
        return false;
    };
    let Some(ev) = doc.get_event_mut_at_path(path) else {
        return false;
    };
    ev.duration = new_duration.clone();
    true
}

/// Replaces the `duration` property of the event at `event_idx` with `new_duration`.
pub fn update_event_duration_in_yaml(
    yaml: &str,
    event_idx: usize,
    new_duration: &crate::mnx::Duration,
) -> Option<String> {
    let mut doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml).ok()?;
    if !update_event_duration_in_doc(&mut doc, event_idx, new_duration) {
        return None;
    }
    serde_saphyr::to_string(&doc).ok()
}

/// Updates an existing `bpm:` entry inside the first global measure's `tempos` list,
/// or inserts a `tempos:` block into the first global measure if none is present.
pub fn set_bpm_in_doc(doc: &mut crate::mnx::MnxDocument, new_bpm: f64) {
    if doc.global.measures.is_empty() {
        doc.global.measures.push(crate::mnx::GlobalMeasure {
            time: Some(crate::mnx::TimeSignature { count: 4, unit: 4 }),
            ..Default::default()
        });
    }

    if let Some(tempo) = doc
        .global
        .measures
        .iter_mut()
        .flat_map(|m| m.tempos.iter_mut())
        .next()
    {
        tempo.bpm = new_bpm;
    } else {
        doc.global.measures[0].tempos.push(crate::mnx::Tempo {
            id: None,
            bpm: new_bpm,
            value: crate::mnx::Duration {
                base: "quarter".to_string(),
                dots: None,
                multiple: None,
            },
            location: None,
        });
    }
}

pub fn set_bpm_in_yaml(yaml: &str, new_bpm: f64) -> String {
    let Ok(mut doc) = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml) else {
        return yaml.to_string();
    };
    set_bpm_in_doc(&mut doc, new_bpm);
    serde_saphyr::to_string(&doc).unwrap_or_else(|_| yaml.to_string())
}

/// Appends a new measure to `global/measures` and to the first part in `doc`, containing a single blank note with `duration`.
pub fn add_measure_to_doc(
    doc: &mut crate::mnx::MnxDocument,
    duration: &crate::mnx::Duration,
) -> bool {
    if doc.global.measures.is_empty() {
        let last_time = crate::mnx::TimeSignature { count: 4, unit: 4 };
        doc.global.measures.push(crate::mnx::GlobalMeasure {
            time: Some(last_time),
            ..Default::default()
        });
    } else {
        doc.global.measures.push(crate::mnx::GlobalMeasure::default());
    }

    let Some(part) = doc.parts.first_mut() else {
        return false;
    };
    part.measures.push(crate::mnx::Measure {
        comment: None,
        beams: None,
        arpeggios: None,
        sequences: vec![crate::mnx::Sequence {
            comment: None,
            content: vec![crate::mnx::SequenceItem::Event(new_blank_event(
                duration.clone(),
            ))],
        }],
    });
    true
}

/// Appends a new measure to `global/measures` and to the first part in `yaml_text`, containing a single blank note with `duration`.
pub fn add_measure_to_yaml(yaml: &str, duration: &crate::mnx::Duration) -> Option<String> {
    let mut doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml).ok()?;
    if !add_measure_to_doc(&mut doc, duration) {
        return None;
    }
    serde_saphyr::to_string(&doc).ok()
}

/// Appends a new blank note (notes: []) to the last measure of the first part in `doc`.
pub fn add_blank_note_to_last_measure_in_doc(
    doc: &mut crate::mnx::MnxDocument,
    duration: &crate::mnx::Duration,
) -> bool {
    let Some(seq) = doc
        .parts
        .first_mut()
        .and_then(|p| p.measures.last_mut())
        .and_then(|m| m.sequences.first_mut())
    else {
        return false;
    };
    seq.content
        .push(crate::mnx::SequenceItem::Event(new_blank_event(
            duration.clone(),
        )));
    true
}

/// Appends a new blank note (notes: []) to the last measure of the first part.
pub fn add_blank_note_to_last_measure(
    yaml: &str,
    duration: &crate::mnx::Duration,
) -> Option<String> {
    let mut doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml).ok()?;
    if !add_blank_note_to_last_measure_in_doc(&mut doc, duration) {
        return None;
    }
    serde_saphyr::to_string(&doc).ok()
}

pub fn insert_blank_note_after_tuplet_in_doc(
    doc: &mut crate::mnx::MnxDocument,
    event_idx: usize,
    base: &str,
) -> Option<usize> {
    let info = doc.find_event_tuplet_info(event_idx)?;
    let tuplet_end = info.event_range.end;
    let paths = doc.get_flattened_event_paths();
    let crate::mnx::AstPath::InTuplet(m_idx, s_idx, i_idx, _) = *paths.get(event_idx)? else {
        return None;
    };

    let dur = crate::mnx::Duration {
        base: base.to_string(),
        dots: None,
        multiple: None,
    };
    let seq = doc
        .parts
        .first_mut()?
        .measures
        .get_mut(m_idx)?
        .sequences
        .get_mut(s_idx)?;
    seq.content
        .insert(i_idx + 1, crate::mnx::SequenceItem::Event(new_blank_event(dur)));
    Some(tuplet_end)
}

pub fn insert_blank_note_after_tuplet(
    yaml: &str,
    event_idx: usize,
    base: &str,
) -> Option<(String, usize)> {
    let mut doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml).ok()?;
    let tuplet_end = insert_blank_note_after_tuplet_in_doc(&mut doc, event_idx, base)?;
    let new_yaml = serde_saphyr::to_string(&doc).ok()?;
    Some((new_yaml, tuplet_end))
}

/// If the final event in the last measure is blank (notes: []), removes it from `doc`.
/// If it was the only event in that measure, removes the entire measure (and the corresponding global measure if applicable).
pub fn remove_last_blank_event_in_doc(doc: &mut crate::mnx::MnxDocument) -> bool {
    if doc.parts.is_empty() || doc.parts[0].measures.is_empty() {
        return false;
    }

    let last_measure_idx = doc.parts[0].measures.len().saturating_sub(1);
    let measure = &mut doc.parts[0].measures[last_measure_idx];
    if measure.sequences.is_empty() {
        return false;
    }

    let seq = &mut measure.sequences[0];
    if seq.content.is_empty() {
        return false;
    }

    fn remove_last_blank(items: &mut Vec<crate::mnx::SequenceItem>) -> bool {
        if let Some(last_item) = items.last_mut() {
            match last_item {
                crate::mnx::SequenceItem::Event(e) => {
                    if e.notes.is_empty() {
                        items.pop();
                        return true;
                    }
                    return false;
                }
                crate::mnx::SequenceItem::Tuplet(t) => {
                    let removed = remove_last_blank(&mut t.content);
                    if t.content.is_empty() {
                        items.pop();
                    }
                    return removed;
                }
                crate::mnx::SequenceItem::Grace(g) => {
                    let removed = remove_last_blank(&mut g.content);
                    if g.content.is_empty() {
                        items.pop();
                    }
                    return removed;
                }
            }
        }
        false
    }

    if !remove_last_blank(&mut seq.content) {
        return false;
    }

    if seq.content.is_empty() {
        doc.parts[0].measures.pop();
        if doc.global.measures.len() > last_measure_idx {
            doc.global.measures.remove(last_measure_idx);
        }
    }
    true
}

/// If the final event in the last measure is blank (notes: []), removes it from the YAML.
/// If it was the only event in that measure, removes the entire measure (and the corresponding global measure if applicable).
pub fn remove_last_blank_event(yaml: &str) -> Option<String> {
    let mut doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml).ok()?;
    if !remove_last_blank_event_in_doc(&mut doc) {
        return None;
    }
    serde_saphyr::to_string(&doc).ok()
}

pub fn convert_rest_to_blank_in_doc(doc: &mut crate::mnx::MnxDocument, event_idx: usize) -> bool {
    let paths = doc.get_flattened_event_paths();
    let Some(&path) = paths.get(event_idx) else {
        return false;
    };
    let Some(ev) = doc.get_event_mut_at_path(path) else {
        return false;
    };
    if ev.rest.is_none() {
        return false;
    }
    ev.rest = None;
    true
}

pub fn convert_rest_to_blank(yaml: &str, event_idx: usize) -> Option<String> {
    let mut doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml).ok()?;
    if !convert_rest_to_blank_in_doc(&mut doc, event_idx) {
        return None;
    }
    serde_saphyr::to_string(&doc).ok()
}

pub fn convert_event_to_rest_in_doc(doc: &mut crate::mnx::MnxDocument, event_idx: usize) -> bool {
    let paths = doc.get_flattened_event_paths();
    let Some(&path) = paths.get(event_idx) else {
        return false;
    };
    let Some(ev) = doc.get_event_mut_at_path(path) else {
        return false;
    };
    if ev.rest.is_some() && ev.notes.is_empty() {
        return false;
    }
    ev.notes.clear();
    ev.rest = Some(crate::mnx::Rest {});
    true
}

pub fn convert_event_to_rest(yaml: &str, event_idx: usize) -> Option<String> {
    let mut doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml).ok()?;
    if !convert_event_to_rest_in_doc(&mut doc, event_idx) {
        return None;
    }
    serde_saphyr::to_string(&doc).ok()
}

pub fn create_tuplet_in_doc(
    doc: &mut crate::mnx::MnxDocument,
    event_idx: usize,
    inner_multiple: u32,
    outer_multiple: u32,
    base: &str,
) -> bool {
    let paths = doc.get_flattened_event_paths();
    let Some(&crate::mnx::AstPath::TopLevel(m_idx, s_idx, i_idx)) = paths.get(event_idx) else {
        return false;
    };

    let Some(seq) = doc
        .parts
        .first_mut()
        .and_then(|p| p.measures.get_mut(m_idx))
        .and_then(|m| m.sequences.get_mut(s_idx))
    else {
        return false;
    };

    let Some(item) = seq.content.get_mut(i_idx) else {
        return false;
    };
    let crate::mnx::SequenceItem::Event(mut ev) = item.clone() else {
        return false;
    };

    let base_dur = crate::mnx::Duration {
        base: base.to_string(),
        dots: None,
        multiple: None,
    };
    ev.duration = base_dur.clone();

    let mut content = Vec::with_capacity(inner_multiple as usize);
    content.push(crate::mnx::SequenceItem::Event(ev));
    for _ in 1..inner_multiple {
        content.push(crate::mnx::SequenceItem::Event(new_rest_event(
            base_dur.clone(),
        )));
    }

    *item = crate::mnx::SequenceItem::Tuplet(crate::mnx::Tuplet {
        item_type: Some("tuplet".to_string()),
        inner: crate::mnx::TupletDuration {
            duration: base_dur.clone(),
            multiple: inner_multiple,
        },
        outer: crate::mnx::TupletDuration {
            duration: base_dur,
            multiple: outer_multiple,
        },
        content,
    });
    true
}

pub fn create_tuplet_in_yaml(
    yaml: &str,
    event_idx: usize,
    inner_multiple: u32,
    outer_multiple: u32,
    base: &str,
) -> Option<String> {
    let mut doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml).ok()?;
    if !create_tuplet_in_doc(&mut doc, event_idx, inner_multiple, outer_multiple, base) {
        return None;
    }
    serde_saphyr::to_string(&doc).ok()
}

pub fn remove_tuplet_in_doc(doc: &mut crate::mnx::MnxDocument, event_idx: usize) -> bool {
    if doc.find_event_tuplet_info(event_idx).is_none() {
        return false;
    }
    let paths = doc.get_flattened_event_paths();
    let Some(&crate::mnx::AstPath::InTuplet(m_idx, s_idx, i_idx, _)) = paths.get(event_idx) else {
        return false;
    };

    let Some(seq) = doc
        .parts
        .first_mut()
        .and_then(|p| p.measures.get_mut(m_idx))
        .and_then(|m| m.sequences.get_mut(s_idx))
    else {
        return false;
    };

    if let crate::mnx::SequenceItem::Tuplet(tuplet) = seq.content.remove(i_idx) {
        seq.content.splice(i_idx..i_idx, tuplet.content);
        true
    } else {
        false
    }
}

pub fn remove_tuplet_in_yaml(yaml: &str, event_idx: usize) -> Option<String> {
    let mut doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml).ok()?;
    if !remove_tuplet_in_doc(&mut doc, event_idx) {
        return None;
    }
    serde_saphyr::to_string(&doc).ok()
}

pub fn fill_tuplet_rests_and_add_blank_in_doc(
    doc: &mut crate::mnx::MnxDocument,
    event_idx: usize,
) -> Option<usize> {
    let info = doc.find_event_tuplet_info(event_idx)?;
    let tuplet_end = info.event_range.end;
    let outer_base = info.tuplet.outer.duration.base.clone();

    let paths = doc.get_flattened_event_paths();
    let crate::mnx::AstPath::InTuplet(m_idx, s_idx, i_idx, _) = *paths.get(event_idx)? else {
        return None;
    };

    let cur_is_blank = doc
        .get_event_at_path(paths[event_idx])
        .map(|e| e.notes.is_empty() && e.rest.is_none())
        .unwrap_or(false);

    let convert_start = if cur_is_blank {
        event_idx
    } else {
        event_idx + 1
    };

    for idx in convert_start..tuplet_end {
        if let Some(ev) = doc.get_event_mut_at_path(paths[idx]) {
            ev.notes.clear();
            ev.rest = Some(crate::mnx::Rest {});
        }
    }

    let outer_dur = crate::mnx::Duration {
        base: outer_base,
        dots: None,
        multiple: None,
    };

    let part_measures_len = doc.parts.first()?.measures.len();
    let is_last_measure = m_idx == part_measures_len.saturating_sub(1);
    let seq_items_len = doc.parts[0].measures[m_idx]
        .sequences
        .get(s_idx)
        .map(|s| s.content.len())
        .unwrap_or(0);
    let is_last_item_in_measure = i_idx + 1 == seq_items_len;

    let dur_beats = crate::validate::duration_to_quarter_beats(&outer_dur);
    let time_sig = crate::validate::get_time_signature_for_measure(doc, m_idx);
    let max_beats = (time_sig.count as f64) * (4.0 / time_sig.unit as f64);
    let seq_beats = doc.parts[0].measures[m_idx]
        .sequences
        .get(s_idx)
        .map(|s| s.total_beats())
        .unwrap_or(0.0);

    if is_last_measure && is_last_item_in_measure && (seq_beats + dur_beats > max_beats + 1e-4) {
        if add_measure_to_doc(doc, &outer_dur) {
            let new_total = doc.get_flattened_event_paths().len();
            return Some(new_total.saturating_sub(1));
        }
        return None;
    }

    doc.parts[0].measures[m_idx].sequences[s_idx]
        .content
        .insert(
            i_idx + 1,
            crate::mnx::SequenceItem::Event(new_blank_event(outer_dur)),
        );

    Some(tuplet_end)
}

pub fn fill_tuplet_rests_and_add_blank(yaml: &str, event_idx: usize) -> Option<(String, usize)> {
    let mut doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml).ok()?;
    let target_idx = fill_tuplet_rests_and_add_blank_in_doc(&mut doc, event_idx)?;
    let new_yaml = serde_saphyr::to_string(&doc).ok()?;
    Some((new_yaml, target_idx))
}

#[cfg(test)]
mod tuplet_delete_tests {
    use super::*;

    #[test]
    fn test_remove_last_blank_tuplet_removes_tuplet() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - measures:
      - sequences:
          - content:
              - duration: { base: quarter }
                notes: []
              - type: tuplet
                inner:
                  duration: { base: quarter }
                  multiple: 3
                outer:
                  duration: { base: quarter }
                  multiple: 2
                content:
                  - duration: { base: quarter }
                    notes: []
"#;

        let res = remove_last_blank_event(yaml).unwrap();
        // Since there is a quarter note before, the tuplet is removed but the measure stays
        assert!(!res.contains("tuplet"));
        assert!(res.contains("duration: {base: quarter}"));
    }
}

pub fn set_note_bend_in_doc(
    doc: &mut crate::mnx::MnxDocument,
    event_idx: usize,
    string_val: u8,
    bend_str: &str,
) -> bool {
    let paths = doc.get_flattened_event_paths();
    let Some(&path) = paths.get(event_idx) else {
        return false;
    };
    let Some(ev) = doc.get_event_mut_at_path(path) else {
        return false;
    };
    let target_note = if let Some(idx) = ev.notes.iter().position(|n| n.string == Some(string_val))
    {
        ev.notes.get_mut(idx)
    } else {
        ev.notes.first_mut()
    };
    if let Some(note) = target_note {
        note.bend = if bend_str.is_empty() {
            None
        } else {
            Some(bend_str.to_string())
        };
        true
    } else {
        false
    }
}

pub fn set_note_bend_in_yaml(
    yaml: &str,
    event_idx: usize,
    string_val: u8,
    bend_str: &str,
) -> String {
    let Ok(mut doc) = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml) else {
        return yaml.to_string();
    };
    if !set_note_bend_in_doc(&mut doc, event_idx, string_val, bend_str) {
        return yaml.to_string();
    }
    serde_saphyr::to_string(&doc).unwrap_or_else(|_| yaml.to_string())
}

pub fn set_global_measure_property_in_doc(
    doc: &mut crate::mnx::MnxDocument,
    target_m_idx: usize,
    prop_name: &str,
    prop_value: &str,
) {
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
    let gm = &mut doc.global.measures[target_m_idx];
    match prop_name {
        "repeatStart" => {
            gm.repeat_start = Some(crate::mnx::RepeatStart {});
        }
        "repeatEnd" => {
            gm.repeat_end = serde_saphyr::from_str::<crate::mnx::RepeatEnd>(prop_value)
                .ok()
                .or(Some(crate::mnx::RepeatEnd { times: 2 }));
        }
        "barline" => {
            gm.barline = serde_saphyr::from_str::<crate::mnx::Barline>(prop_value).ok();
        }
        "ending" => {
            gm.ending = serde_saphyr::from_str::<crate::mnx::Ending>(prop_value).ok();
        }
        "systemBreak" => {
            gm.system_break = if prop_value.trim() == "true" {
                Some(true)
            } else {
                None
            };
        }
        "pageBreak" => {
            gm.page_break = if prop_value.trim() == "true" {
                Some(true)
            } else {
                None
            };
        }
        "number" => {
            gm.number = prop_value.trim().parse::<u32>().ok();
        }
        _ => {}
    }
}

pub fn set_global_measure_property(
    yaml: &str,
    target_m_idx: usize,
    prop_name: &str,
    prop_value: &str,
) -> String {
    let Ok(mut doc) = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml) else {
        return yaml.to_string();
    };
    set_global_measure_property_in_doc(&mut doc, target_m_idx, prop_name, prop_value);
    serde_saphyr::to_string(&doc).unwrap_or_else(|_| yaml.to_string())
}

pub fn insert_grace_before_in_doc(doc: &mut crate::mnx::MnxDocument, event_idx: usize) -> bool {
    if doc.find_event_grace_info(event_idx).is_some() {
        return false;
    }
    if event_idx > 0 {
        if let Some(prev_info) = doc.find_event_grace_info(event_idx - 1) {
            if prev_info.event_range.end == event_idx {
                return false;
            }
        }
    }

    let paths = doc.get_flattened_event_paths();
    let Some(&crate::mnx::AstPath::TopLevel(m_idx, s_idx, i_idx)) = paths.get(event_idx) else {
        return false;
    };

    let grace_dur = crate::mnx::Duration {
        base: "eighth".to_string(),
        dots: None,
        multiple: None,
    };
    let grace = crate::mnx::Grace {
        item_type: "grace".to_string(),
        content: vec![crate::mnx::SequenceItem::Event(new_rest_event(grace_dur))],
    };

    let Some(part) = doc.parts.first_mut() else {
        return false;
    };
    let Some(measure) = part.measures.get_mut(m_idx) else {
        return false;
    };
    let Some(seq) = measure.sequences.get_mut(s_idx) else {
        return false;
    };
    seq.content
        .insert(i_idx, crate::mnx::SequenceItem::Grace(grace));
    true
}

pub fn insert_grace_before_in_yaml(yaml: &str, event_idx: usize) -> Option<String> {
    let mut doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml).ok()?;
    if !insert_grace_before_in_doc(&mut doc, event_idx) {
        return None;
    }
    serde_saphyr::to_string(&doc).ok()
}

pub fn append_blank_note_to_grace_block_in_doc(
    doc: &mut crate::mnx::MnxDocument,
    e_idx: usize,
) -> bool {
    if doc.find_event_grace_info(e_idx).is_none() {
        return false;
    }
    let paths = doc.get_flattened_event_paths();
    let Some(&crate::mnx::AstPath::InTuplet(m_idx, s_idx, i_idx, _)) = paths.get(e_idx) else {
        return false;
    };

    let grace_dur = crate::mnx::Duration {
        base: "eighth".to_string(),
        dots: None,
        multiple: None,
    };
    if let Some(crate::mnx::SequenceItem::Grace(g)) = doc
        .parts
        .first_mut()
        .and_then(|p| p.measures.get_mut(m_idx))
        .and_then(|m| m.sequences.get_mut(s_idx))
        .and_then(|s| s.content.get_mut(i_idx))
    {
        g.content
            .push(crate::mnx::SequenceItem::Event(new_rest_event(grace_dur)));
        true
    } else {
        false
    }
}

pub fn append_blank_note_to_grace_block_in_yaml(yaml: &str, e_idx: usize) -> Option<String> {
    let mut doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml).ok()?;
    if !append_blank_note_to_grace_block_in_doc(&mut doc, e_idx) {
        return None;
    }
    serde_saphyr::to_string(&doc).ok()
}

pub fn set_event_text_markings_in_doc(
    doc: &mut crate::mnx::MnxDocument,
    event_idx: usize,
    markings: Vec<crate::mnx::TextMarking>,
) -> bool {
    let paths = doc.get_flattened_event_paths();
    let Some(&path) = paths.get(event_idx) else {
        return false;
    };
    let Some(ev) = doc.get_event_mut_at_path(path) else {
        return false;
    };
    ev.text_markings = markings
        .into_iter()
        .filter(|m| !m.text.trim().is_empty())
        .collect();
    true
}

pub fn set_event_text_markings_in_yaml(
    yaml: &str,
    event_idx: usize,
    markings: Vec<crate::mnx::TextMarking>,
) -> Option<String> {
    let mut doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml).ok()?;
    if !set_event_text_markings_in_doc(&mut doc, event_idx, markings) {
        return None;
    }
    serde_saphyr::to_string(&doc).ok()
}

