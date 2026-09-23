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
        if event_text.contains("type: tuplet") {
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
        format!(
            "pitch: {{ step: {}, alter: {}, octave: {} }}",
            pitch.step, alter_str, pitch.octave
        )
    } else {
        format!(
            "pitch: {{ step: {}, octave: {} }}",
            pitch.step, pitch.octave
        )
    }
}

/// Calculates the exact microtonal Pitch for a given string and fret.
pub fn calculate_pitch_for_string_fret(
    part: &crate::mnx::Part,
    string: u8,
    fret: i32,
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
    new_fret: i32,
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
                    result = format!(
                        "{}{}{}",
                        &result[..abs_p_start],
                        pitch_yaml_str,
                        &result[abs_p_end..]
                    );
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
    new_fret: i32,
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
            let line_start = yaml[..abs_notes_pos]
                .rfind('\n')
                .map(|p| p + 1)
                .unwrap_or(0);
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
        } else if let Some(rest_pos) = event_text.find("rest:") {
            let abs_rest_pos = er.total_range.start + rest_pos;
            let after_rest = &yaml[abs_rest_pos..];
            let line_end_rel = after_rest.find('\n').unwrap_or(after_rest.len());

            let line_start = yaml[..abs_rest_pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
            let notes_indent_len = abs_rest_pos - line_start;
            let indent = " ".repeat(notes_indent_len + 2);
            let sub_indent = " ".repeat(notes_indent_len + 4);

            let new_notes_block = format!(
                "notes:\n{}- {}\n{}string: {}\n{}fret: {}",
                indent, pitch_str, sub_indent, target_string, sub_indent, new_fret
            );

            let abs_replace_end = abs_rest_pos + line_end_rel;
            let mut res = yaml.to_string();
            res.replace_range(abs_rest_pos..abs_replace_end, &new_notes_block);
            Some(res)
        } else {
            // Append notes block
            let first_line_start = event_text.find('-').unwrap_or(0);
            let before_dash = &event_text[..first_line_start];
            let indent_len = before_dash
                .rfind('\n')
                .map(|p| before_dash.len() - p - 1)
                .unwrap_or(before_dash.len());
            let indent = " ".repeat(indent_len + 2);
            let sub_indent = " ".repeat(indent_len + 4);

            let new_notes_block = format!(
                "\n{}notes:\n{}- {}\n{}string: {}\n{}fret: {}",
                indent, indent, pitch_str, sub_indent, target_string, sub_indent, new_fret
            );

            let mut res = yaml.to_string();
            res.insert_str(er.total_range.end, &new_notes_block);
            Some(res)
        }
    }
}

/// Removes the note on `target_string` from the specified event in YAML.
pub fn remove_note_on_string(yaml: &str, event_idx: usize, target_string: u8) -> Option<String> {
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
            if first_measure_seen
                && indent <= base_indent
                && trimmed.starts_with("- ")
                && !trimmed.starts_with("- time")
                && !trimmed.starts_with("- directions")
            {
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
                    if !result.is_empty() {
                        result.push('\n');
                    }
                    // Recreate inline tempo entry preserving '-' if present
                    let prefix = if trimmed.starts_with("- ") { "- " } else { "" };
                    result.push_str(&format!(
                        "{}{}{}tempo: {{ bpm: {} }}",
                        leading,
                        prefix,
                        if prefix.is_empty() { "" } else { "" },
                        bpm_str
                    ));
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
                if !result.is_empty() {
                    result.push('\n');
                }
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
            if indent > first_measure_indent
                || (indent == first_measure_indent && !trimmed.starts_with("- "))
            {
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
            child_indent, child_indent, bpm_str
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
    let mut global_end_idx = None;
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
                        let extracted = trimmed[pos..].trim();
                        if extracted == "time:" {
                            last_time_sig_str = "time: { count: 4, unit: 4 }".to_string();
                        } else {
                            last_time_sig_str = extracted.to_string();
                        }
                    }
                } else if trimmed.starts_with("parts:") || (indent == 0 && !trimmed.is_empty()) {
                    in_global = false;
                    global_end_idx = Some(i);
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

            if parts_measures_idx.is_some()
                && (trimmed.starts_with("- sequences:") || trimmed.starts_with("-sequences:"))
            {
                seq_indent = indent;
            }
        }
    }

    let _ = parts_measures_idx?;

    // Format new measure in global/measures if global/measures exists
    let mut result_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();

    if let Some(end_idx) = global_end_idx {
        // Insert at the end of the global block (just before parts)
        let new_gm_line = format!("{}- {}", " ".repeat(global_item_indent), last_time_sig_str);
        result_lines.insert(end_idx, new_gm_line);
    } else if let Some(last_gm_idx) = global_measure_item_idx {
        // Fallback: Insert after the last item in global/measures
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
pub fn add_blank_note_to_last_measure(
    yaml: &str,
    duration: &crate::mnx::Duration,
) -> Option<String> {
    let event_ranges = find_all_event_ranges(yaml);
    let last_er = event_ranges.last()?;

    let doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml).ok();
    let total_events = event_ranges.len();
    let tuplet_info = doc
        .as_ref()
        .and_then(|d| d.find_event_tuplet_info(total_events.saturating_sub(1)));

    let indent = if tuplet_info.is_some() {
        let before_first = &yaml[..last_er.total_range.start];
        let tuplet_pos = before_first
            .rfind("- type: tuplet")
            .or_else(|| before_first.rfind("- type: \"tuplet\""))
            .or_else(|| before_first.rfind("- type: 'tuplet'"))?;
        let tuplet_line_start = yaml[..tuplet_pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let tuplet_indent_len = tuplet_pos - tuplet_line_start;
        " ".repeat(tuplet_indent_len)
    } else {
        let event_first_line = &yaml[last_er.total_range.start..];
        let indent_len = event_first_line.len() - event_first_line.trim_start().len();
        " ".repeat(indent_len)
    };
    let sub_indent = format!("{}  ", indent);

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

pub fn insert_blank_note_after_tuplet(
    yaml: &str,
    event_idx: usize,
    base: &str,
) -> Option<(String, usize)> {
    let doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml).ok()?;
    let info = doc.find_event_tuplet_info(event_idx)?;
    let event_ranges = find_all_event_ranges(yaml);

    let tuplet_end = info.event_range.end;
    let last_er = event_ranges.get(tuplet_end.saturating_sub(1))?;

    let before_first = &yaml[..last_er.total_range.start];
    let tuplet_pos = before_first
        .rfind("- type: tuplet")
        .or_else(|| before_first.rfind("- type: \"tuplet\""))
        .or_else(|| before_first.rfind("- type: 'tuplet'"))?;
    let tuplet_line_start = yaml[..tuplet_pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let tuplet_indent_len = tuplet_pos - tuplet_line_start;
    let dash_indent = " ".repeat(tuplet_indent_len);

    let insert_text = format!(
        "\n{}- duration: {{ base: {} }}\n{}  notes: []",
        dash_indent, base, dash_indent
    );
    let insert_pos = last_er.total_range.end;

    let mut result = yaml.to_string();
    result.insert_str(insert_pos, &insert_text);

    let new_event_idx = tuplet_end;
    Some((result, new_event_idx))
}

/// If the final event in the last measure is blank (notes: []), removes it from the YAML.
/// If it was the only event in that measure, removes the entire measure (and the corresponding global measure if applicable).
pub fn remove_last_blank_event(yaml: &str) -> Option<String> {
    let doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml).ok()?;
    let part = doc.parts.first()?;
    let last_measure = part.measures.last()?;
    let last_seq = last_measure.sequences.first()?;
    let last_event = last_seq.flattened_events().last().copied()?;

    if !last_event.notes.is_empty() {
        return None;
    }

    if last_seq.flattened_events().len() > 1 {
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

pub fn convert_event_to_rest(yaml: &str, event_idx: usize) -> Option<String> {
    let event_ranges = find_all_event_ranges(yaml);
    let er = event_ranges.get(event_idx)?;
    let event_text = yaml.get(er.total_range.clone())?;

    // Find where notes: block starts, or if we have notes at all
    let mut result = yaml.to_string();

    // Find the indent of the event's first line
    let first_line_start = event_text.find('-').unwrap_or(0);
    let before_dash = &event_text[..first_line_start];
    let indent_len = before_dash
        .rfind('\n')
        .map(|p| before_dash.len() - p - 1)
        .unwrap_or(before_dash.len());
    let indent = " ".repeat(indent_len + 2); // properties are usually indented 2 spaces from the dash

    let mut to_replace_start = None;
    let mut to_replace_end = None;

    if let Some(notes_pos) = event_text.find("notes:") {
        let abs_start = er.total_range.start + notes_pos;
        to_replace_start = Some(abs_start);

        // Find end of notes block
        let block_end = if !er.note_ranges.is_empty() {
            let last_note_range = er.note_ranges.last().unwrap();
            let after_last_note = &yaml[last_note_range.end..er.total_range.end];
            if let Some(nl) = after_last_note.find('\n') {
                last_note_range.end + nl
            } else {
                er.total_range.end
            }
        } else {
            let after_notes = &event_text[notes_pos..];
            if let Some(nl) = after_notes.find('\n') {
                abs_start + nl
            } else {
                er.total_range.end
            }
        };
        to_replace_end = Some(block_end);
    } else if event_text.contains("rest:") {
        // Already a rest
        return None;
    }

    let rest_line = format!("rest: {{}}");

    if let (Some(start), Some(end)) = (to_replace_start, to_replace_end) {
        result.replace_range(start..end, &rest_line);
    } else {
        // Just append rest: {} to the end of the event
        result.insert_str(er.total_range.end, &format!("\n{}{}", indent, rest_line));
    }

    Some(result)
}

fn replace_event_duration_base(event_text: &str, new_base: &str) -> String {
    let lines: Vec<&str> = event_text.lines().collect();
    let mut new_lines = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with("- duration:") {
            let indent = line.len() - line.trim_start().len();
            new_lines.push(format!(
                "{}- duration: {{ base: {} }}",
                " ".repeat(indent),
                new_base
            ));
        } else if trimmed.starts_with("duration:") {
            let indent = line.len() - line.trim_start().len();
            new_lines.push(format!(
                "{}duration: {{ base: {} }}",
                " ".repeat(indent),
                new_base
            ));
        } else {
            new_lines.push(line.to_string());
        }
    }
    new_lines.join("\n")
}

pub fn create_tuplet_in_yaml(
    yaml: &str,
    event_idx: usize,
    inner_multiple: u32,
    outer_multiple: u32,
    base: &str,
) -> Option<String> {
    let event_ranges = find_all_event_ranges(yaml);
    let er = event_ranges.get(event_idx)?;
    let event_text = yaml.get(er.total_range.clone())?;

    let first_line_start = event_text.find('-').unwrap_or(0);
    let before_dash = &event_text[..first_line_start];
    let indent_len = before_dash
        .rfind('\n')
        .map(|p| before_dash.len() - p - 1)
        .unwrap_or(before_dash.len());
    let dash_indent = " ".repeat(indent_len);
    let prop_indent = " ".repeat(indent_len + 2);
    let inner_dash_indent = " ".repeat(indent_len + 4);

    let mut tuplet_yaml = String::new();
    tuplet_yaml.push_str(&format!("{}- type: tuplet\n", dash_indent));
    tuplet_yaml.push_str(&format!("{}inner:\n", prop_indent));
    tuplet_yaml.push_str(&format!(
        "{}duration: {{ base: {} }}\n",
        inner_dash_indent, base
    ));
    tuplet_yaml.push_str(&format!(
        "{}multiple: {}\n",
        inner_dash_indent, inner_multiple
    ));
    tuplet_yaml.push_str(&format!("{}outer:\n", prop_indent));
    tuplet_yaml.push_str(&format!(
        "{}duration: {{ base: {} }}\n",
        inner_dash_indent, base
    ));
    tuplet_yaml.push_str(&format!(
        "{}multiple: {}\n",
        inner_dash_indent, outer_multiple
    ));
    tuplet_yaml.push_str(&format!("{}content:\n", prop_indent));

    let updated_event_text = replace_event_duration_base(event_text, base);
    for line in updated_event_text.lines() {
        tuplet_yaml.push_str(&format!("    {}\n", line));
    }

    for _ in 1..inner_multiple {
        tuplet_yaml.push_str(&format!(
            "{}- duration: {{ base: {} }}\n",
            inner_dash_indent, base
        ));
        tuplet_yaml.push_str(&format!("{}  rest: {{}}\n", inner_dash_indent));
    }

    let mut result = yaml.to_string();
    result.replace_range(er.total_range.clone(), tuplet_yaml.trim_end());
    Some(result)
}

pub fn remove_tuplet_in_yaml(yaml: &str, event_idx: usize) -> Option<String> {
    let doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml).ok()?;
    let info = doc.find_event_tuplet_info(event_idx)?;
    let event_ranges = find_all_event_ranges(yaml);

    let first_er = event_ranges.get(info.event_range.start)?;
    let last_er = event_ranges.get(info.event_range.end.saturating_sub(1))?;

    let before_first = &yaml[..first_er.total_range.start];
    let tuplet_pos = before_first
        .rfind("- type: tuplet")
        .or_else(|| before_first.rfind("- type: \"tuplet\""))
        .or_else(|| before_first.rfind("- type: 'tuplet'"))?;

    let tuplet_line_start = yaml[..tuplet_pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let tuplet_indent = tuplet_pos - tuplet_line_start;

    let inner_content_text = &yaml[first_er.total_range.start..last_er.total_range.end];

    let first_inner_dash = inner_content_text.find('-').unwrap_or(0);
    let inner_indent_len = inner_content_text[..first_inner_dash]
        .rfind('\n')
        .map(|p| inner_content_text[..first_inner_dash].len() - p - 1)
        .unwrap_or(inner_content_text[..first_inner_dash].len());

    let de_indent = inner_indent_len.saturating_sub(tuplet_indent);

    let mut unwrapped_lines = Vec::new();
    for line in inner_content_text.lines() {
        if line.len() >= de_indent && line[..de_indent].trim().is_empty() {
            unwrapped_lines.push(&line[de_indent..]);
        } else {
            unwrapped_lines.push(line.trim_start());
        }
    }
    let unwrapped_text = unwrapped_lines.join("\n");

    let mut result = yaml.to_string();
    result.replace_range(tuplet_line_start..last_er.total_range.end, &unwrapped_text);
    Some(result)
}

pub fn fill_tuplet_rests_and_add_blank(yaml: &str, event_idx: usize) -> Option<(String, usize)> {
    let mut current_yaml = yaml.to_string();
    let doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(&current_yaml).ok()?;
    let info = doc.find_event_tuplet_info(event_idx)?;

    let tuplet_end = info.event_range.end;
    let outer_base = info.tuplet.outer.duration.base.clone();

    // Check if the current event is blank (has no notes and is not a rest)
    let flat_events = doc
        .parts
        .first()?
        .measures
        .iter()
        .flat_map(|m| m.sequences.iter())
        .flat_map(|s| s.flattened_events())
        .collect::<Vec<_>>();
    let cur_is_blank = flat_events
        .get(event_idx)
        .map(|e| e.notes.is_empty() && e.rest.is_none())
        .unwrap_or(false);

    let convert_start = if cur_is_blank {
        event_idx
    } else {
        event_idx + 1
    };

    // Fill remaining events in tuplet with rests in reverse order so indices remain stable
    for idx in (convert_start..tuplet_end).rev() {
        if let Some(updated) = convert_event_to_rest(&current_yaml, idx) {
            current_yaml = updated;
        }
    }

    // Now insert a new blank note after the tuplet
    let event_ranges = find_all_event_ranges(&current_yaml);
    let last_er = event_ranges.get(tuplet_end.saturating_sub(1))?;

    // Determine indentation for top-level sequence items from the tuplet header
    let before_first = &current_yaml[..last_er.total_range.start];
    let tuplet_pos = before_first
        .rfind("- type: tuplet")
        .or_else(|| before_first.rfind("- type: \"tuplet\""))
        .or_else(|| before_first.rfind("- type: 'tuplet'"))?;
    let tuplet_line_start = current_yaml[..tuplet_pos]
        .rfind('\n')
        .map(|p| p + 1)
        .unwrap_or(0);
    let tuplet_indent_len = tuplet_pos - tuplet_line_start;
    let dash_indent = " ".repeat(tuplet_indent_len);

    // Check if measure capacity allows adding note or requires a new measure
    let current_doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(&current_yaml).ok()?;
    let part = current_doc.parts.first()?;

    // Find which measure contains the tuplet
    let mut cur_e = 0;
    let mut measure_idx = 0;
    let mut is_last_event_in_measure = false;
    for (m_i, measure) in part.measures.iter().enumerate() {
        if let Some(seq) = measure.sequences.first() {
            let count = seq.flattened_events().len();
            if tuplet_end <= cur_e + count {
                measure_idx = m_i;
                is_last_event_in_measure = tuplet_end == cur_e + count;
                break;
            }
            cur_e += count;
        }
    }

    let is_last_measure = measure_idx == part.measures.len().saturating_sub(1);
    let outer_dur = crate::mnx::Duration {
        base: outer_base.clone(),
        dots: None,
        multiple: None,
    };
    let dur_beats = crate::validate::duration_to_quarter_beats(&outer_dur);
    let time_sig = crate::validate::get_time_signature_for_measure(&current_doc, measure_idx);
    let max_beats = (time_sig.count as f64) * (4.0 / time_sig.unit as f64);
    let seq_beats = part.measures[measure_idx]
        .sequences
        .first()
        .map(|s| s.total_beats())
        .unwrap_or(0.0);

    if is_last_measure && is_last_event_in_measure && (seq_beats + dur_beats > max_beats + 1e-4) {
        // Measure is full, add a new measure
        if let Some(new_yaml) = add_measure_to_yaml(&current_yaml, &outer_dur) {
            let new_total = find_all_event_ranges(&new_yaml).len();
            return Some((new_yaml, new_total.saturating_sub(1)));
        }
    }

    // Insert blank note right after the tuplet
    let insert_text = format!(
        "\n{}- duration: {{ base: {} }}\n{}  notes: []",
        dash_indent, outer_base, dash_indent
    );
    let insert_pos = last_er.total_range.end;
    current_yaml.insert_str(insert_pos, &insert_text);

    let new_event_idx = tuplet_end;
    Some((current_yaml, new_event_idx))
}

/// Post-processes YAML generated by `serde_saphyr::to_string` to format `duration:` and `pitch:`
/// blocks in a single-line flow style (e.g. `duration: { base: quarter }`), mimicking the
/// handwritten style.
pub fn post_process_generated_yaml(yaml: &str) -> String {
    let mut lines = yaml.lines().peekable();
    let mut out = String::new();

    while let Some(line) = lines.next() {
        let trimmed = line.trim_end();
        if trimmed.ends_with("duration:")
            || trimmed.ends_with("pitch:")
            || trimmed.ends_with("time:")
        {
            let indent = trimmed
                .chars()
                .take_while(|c| *c == ' ' || *c == '-')
                .count();
            let mut attrs = Vec::new();

            while let Some(&next_line) = lines.peek() {
                if next_line.trim().is_empty() {
                    break;
                }
                let next_indent = next_line.chars().take_while(|c| *c == ' ').count();
                if next_indent > indent {
                    let part = next_line.trim();
                    if part.starts_with("-") {
                        break;
                    }
                    if let Some((k, v)) = part.split_once(':') {
                        let k = k.trim();
                        let v = v.trim();
                        if v == "null" {
                            // Ignore null values like `alter: null`
                        } else if k == "alter" && !v.starts_with('\'') && !v.starts_with('"') {
                            attrs.push(format!("{}: '{}'", k, v));
                        } else {
                            attrs.push(format!("{}: {}", k, v));
                        }
                    }
                    lines.next();
                } else {
                    break;
                }
            }

            if !attrs.is_empty() {
                out.push_str(&format!("{} {{ {} }}\n", trimmed, attrs.join(", ")));
            } else {
                out.push_str(trimmed);
                out.push('\n');
            }
        } else if trimmed.ends_with("events:") || trimmed.ends_with("events: ") {
            let indent = trimmed
                .chars()
                .take_while(|c| *c == ' ' || *c == '-')
                .count();
            let mut list_items = Vec::new();

            while let Some(&next_line) = lines.peek() {
                if next_line.trim().is_empty() {
                    break;
                }
                let next_indent = next_line.chars().take_while(|c| *c == ' ').count();
                if next_indent >= indent {
                    let part = next_line.trim();
                    if part.starts_with("- ") {
                        list_items.push(part[2..].trim().to_string());
                        lines.next();
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }

            if !list_items.is_empty() {
                out.push_str(&format!("{} [{}]\n", trimmed, list_items.join(", ")));
            } else {
                out.push_str(trimmed);
                out.push('\n');
            }
        } else {
            out.push_str(trimmed);
            out.push('\n');
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_post_process_generated_yaml() {
        let doc = crate::mnx::MnxDocument {
            mnx: crate::mnx::MnxMeta { version: 1 },
            global: crate::mnx::Global {
                measures: vec![crate::mnx::GlobalMeasure {
                    time: Some(crate::mnx::TimeSignature { count: 4, unit: 4 }),
                    directions: vec![],
                }],
            },
            parts: vec![crate::mnx::Part {
                instrument: Some(crate::mnx::Instrument {
                    name: "Guitar".to_string(),
                    tuning: Some(vec![]),
                    capo: None,
                    micro_fret_divisions: Some(12),
                    fret_step_multiplier: Some(1),
                }),
                measures: vec![crate::mnx::Measure {
                    beams: Some(vec![crate::mnx::BeamGroup {
                        events: vec!["ev1".to_string(), "ev2".to_string()],
                        beams: None,
                        direction: None,
                    }]),
                    sequences: vec![crate::mnx::Sequence {
                        content: vec![],
                        beams: None,
                    }],
                }],
            }],
        };
        let yaml = serde_saphyr::to_string(&doc).unwrap();

        // Assert that the processed yaml contains the single line formats
        let processed = post_process_generated_yaml(&yaml);
        assert!(processed.contains("time: { count: 4, unit: 4 }"));
        assert!(processed.contains("events: [ev1, ev2]"));
    }
}
