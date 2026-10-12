use super::*;
use crate::yaml_editor::*;
use eframe::egui::{self};

impl MnxEditorApp {

    pub(crate) fn action_remove_tempo_change(&mut self, target_id: &str) {
        if let Ok(mut doc) = serde_saphyr::from_str::<crate::mnx::MnxDocument>(&self.yaml_text) {
            for measure in &mut doc.global.measures {
                measure.tempo_changes.retain(|tc| tc.target.as_deref() != Some(target_id));
                measure.tempos.retain(|t| t.id.as_deref() != Some(target_id));
            }
            if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
                self.update_yaml(new_yaml);
            }
        }
    }

    pub(crate) fn action_insert_tempo_change(&mut self, event_idx: usize, end_bpm: f64, dur_beats: f64, bend_str: &str, existing_target_id: Option<String>) {
        if let Some(tid) = existing_target_id {
            self.action_remove_tempo_change(&tid);
        }
        if let Ok(mut doc) = serde_saphyr::from_str::<crate::mnx::MnxDocument>(&self.yaml_text) {
            let mut start_m_idx = 0;
            let mut start_offset = 0.0;
            let mut found = false;
            
            // Find start
            if let Some(part) = doc.parts.first() {
                let mut current_idx = 0;
                for (m_idx, measure) in part.measures.iter().enumerate() {
                    if let Some(seq) = measure.sequences.first() {
                        let mut events_scaled = Vec::new();
                        crate::midi::walk_events_with_scaling(&seq.content, &mut events_scaled, 1.0);
                        start_offset = 0.0;
                        for (e, scale) in events_scaled {
                            if current_idx == event_idx {
                                start_m_idx = m_idx;
                                found = true;
                                break;
                            }
                            let beats = crate::validate::duration_to_quarter_beats(&e.duration);
                            start_offset += beats * scale;
                            current_idx += 1;
                        }
                        if found { break; }
                    }
                }
            }
            if !found && event_idx > 0 {
                start_m_idx = doc.parts.first().map(|p| p.measures.len().saturating_sub(1)).unwrap_or(0);
                start_offset = 0.0;
            }

            // Calculate location fraction
            let get_fraction = |offset: f64| -> Option<String> {
                if offset > 0.001 {
                    let num = (offset * 1000.0).round() as u32;
                    let den = 4000;
                    let mut a = num;
                    let mut b = den;
                    while b != 0 { let t = b; b = a % b; a = t; }
                    Some(format!("{}/{}", num / a, den / a))
                } else {
                    None
                }
            };

            let start_location = get_fraction(start_offset);

            // Now find end location by accumulating beats
            let mut end_m_idx = start_m_idx;
            let mut end_offset = start_offset + dur_beats;
            
            // We need to advance measures if end_offset > measure duration.
            // But we might not know measure duration unless we parse the time signature.
            // For simplicity, we can calculate the duration of the current measure by summing its events.
            if let Some(part) = doc.parts.first() {
                while end_m_idx < part.measures.len() {
                    let measure = &part.measures[end_m_idx];
                    let mut m_duration = 0.0;
                    if let Some(seq) = measure.sequences.first() {
                        let mut events_scaled = Vec::new();
                        crate::midi::walk_events_with_scaling(&seq.content, &mut events_scaled, 1.0);
                        for (e, scale) in events_scaled {
                            m_duration += crate::validate::duration_to_quarter_beats(&e.duration) * scale;
                        }
                    }
                    if m_duration == 0.0 { m_duration = 4.0; } // fallback
                    
                    if end_offset <= m_duration + 0.001 {
                        // It ends in this measure
                        break;
                    } else {
                        end_offset -= m_duration;
                        end_m_idx += 1;
                    }
                }
            }

            let end_location = get_fraction(end_offset);
            
            // Ensure global measures exist up to end_m_idx
            while doc.global.measures.len() <= end_m_idx.max(start_m_idx) {
                doc.global.measures.push(crate::mnx::GlobalMeasure {
                    time: None,
                    repeat_start: None,
                    repeat_end: None,
                    barline: None,
                    tempos: Vec::new(),
                    tempo_changes: Vec::new(),
                });
            }

            // Create target tempo object
            let tempo_id = format!("tempo{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
            
            // Wait, we need to know what 'duration_value' is for the end_bpm.
            // We can just use "quarter" as a standard default for the target tempo.
            let target_tempo = crate::mnx::Tempo {
                id: Some(tempo_id.clone()),
                bpm: end_bpm,
                value: crate::mnx::Duration {
                    base: "quarter".to_string(),
                    dots: None,
                    multiple: None,
                },
                location: end_location,
            };
            
            doc.global.measures[end_m_idx].tempos.push(target_tempo);
            
            // Create tempoChange object
            let tempo_change = crate::mnx::TempoChange {
                location: start_location,
                bend: Some(bend_str.to_string()),
                target: Some(tempo_id),
            };
            
            doc.global.measures[start_m_idx].tempo_changes.push(tempo_change);

            let new_yaml = serde_saphyr::to_string(&doc).unwrap();
            self.update_yaml(new_yaml);
        }
    }

    pub(crate) fn insert_tempo(&mut self, bpm: f64, duration_value: &str, dots: u32, event_idx: usize) {
        if let Ok(mut doc) = serde_saphyr::from_str::<crate::mnx::MnxDocument>(&self.yaml_text) {
            let mut target_m_idx = 0;
            let mut sum_quarter_beats = 0.0;
            let mut found = false;
            
            if let Some(part) = doc.parts.first() {
                let mut current_idx = 0;
                for (m_idx, measure) in part.measures.iter().enumerate() {
                    if let Some(seq) = measure.sequences.first() {
                        let mut events_scaled = Vec::new();
                        crate::midi::walk_events_with_scaling(&seq.content, &mut events_scaled, 1.0);
                        
                        sum_quarter_beats = 0.0;
                        
                        for (e, scale) in events_scaled {
                            if current_idx == event_idx {
                                target_m_idx = m_idx;
                                found = true;
                                break;
                            }
                            let beats = crate::validate::duration_to_quarter_beats(&e.duration);
                            sum_quarter_beats += beats * scale;
                            current_idx += 1;
                        }
                        if found { break; }
                    }
                }
            }
            
            if !found && event_idx > 0 {
                target_m_idx = doc.parts.first().map(|p| p.measures.len().saturating_sub(1)).unwrap_or(0);
                sum_quarter_beats = 0.0;
            }

            while doc.global.measures.len() <= target_m_idx {
                doc.global.measures.push(crate::mnx::GlobalMeasure {
                    time: None,
                    repeat_start: None,
                    repeat_end: None,
                    barline: None,
                    tempos: Vec::new(),
                    tempo_changes: Vec::new(),
                });
            }

            let location = if sum_quarter_beats > 0.001 {
                let num = (sum_quarter_beats * 1000.0).round() as u32;
                let den = 4000;
                
                let mut a = num;
                let mut b = den;
                while b != 0 {
                    let t = b;
                    b = a % b;
                    a = t;
                }
                let s_num = num / a;
                let s_den = den / a;
                Some(format!("{}/{}", s_num, s_den))
            } else {
                None
            };
            
            let tempo = crate::mnx::Tempo {
                id: None,
                bpm,
                value: crate::mnx::Duration {
                    base: duration_value.to_string(),
                    dots: if dots > 0 { Some(dots) } else { None },
                    multiple: None,
                },
                location: location.clone(),
            };
            
            let tempos = &mut doc.global.measures[target_m_idx].tempos;
            if let Some(pos) = tempos.iter().position(|t| t.location == tempo.location) {
                tempos[pos] = tempo;
            } else {
                tempos.push(tempo);
            }
            
            if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
                self.update_yaml(new_yaml);
            }
        }
    }


    


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

    pub fn is_selected_note_bent(&self) -> bool {
        if let Some(doc) = &self.parsed_mnx {
            if let Some(part) = doc.parts.first() {
                if let Some(e_idx) = self.selected_event_idx {
                    if let Some(n_idx) = self.selected_note_idx_in_event {
                        let mut count = 0;
                        for m in &part.measures {
                            for seq in &m.sequences {
                                for ev in seq.flattened_events() {
                                    if count == e_idx {
                                        if let Some(note) = ev.notes.get(n_idx) {
                                            return note.bend.is_some();
                                        }
                                    }
                                    count += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        false
    }

    pub fn action_edit_bend(&mut self) {
        if let Some(e_idx) = self.selected_event_idx {
            if let Some(n_idx) = self.selected_note_idx_in_event {
                let mut existing_bend = None;
                let mut target_str = 1;

                if let Some(doc) = &self.parsed_mnx {
                    if let Some(part) = doc.parts.first() {
                        let mut count = 0;
                        for m in &part.measures {
                            for seq in &m.sequences {
                                for ev in seq.flattened_events() {
                                    if count == e_idx {
                                        if let Some(note) = ev.notes.get(n_idx) {
                                            existing_bend = note.bend.clone();
                                            target_str = note.string.unwrap_or(1);
                                        }
                                    }
                                    count += 1;
                                }
                            }
                        }
                    }
                }

                let rect =
                    egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(300.0, 150.0));
                let editor = if let Some(bend_str) = existing_bend {
                    crate::curve_editor::parse_bend_string(&bend_str, rect)
                } else {
                    crate::curve_editor::MonotonicCurveEditor::new(rect)
                };
                let max_steps = self.pitch_bend_range as u32;

                self.active_dialog = ActiveDialog::BendEditor {
                    event_idx: e_idx,
                    string: target_str,
                    curve_editor: editor,
                    bend_range_steps: max_steps,
                };
            }
        }
    }

    pub fn is_selected_note_slurred(&self) -> bool {
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
                        if let Some(slurs) = &ev.slurs {
                            if let Some(n_idx) = self.selected_note_idx_in_event {
                                if let Some(n) = ev.notes.get(n_idx) {
                                    if let Some(id) = &n.id {
                                        return slurs
                                            .iter()
                                            .any(|s| s.start_note.as_ref() == Some(id));
                                    }
                                }
                            } else {
                                // Blue mode: if any slur starts on this event
                                return !slurs.is_empty();
                            }
                        }
                        return false;
                    }
                    cur_e += 1;
                }
            }
        }
        false
    }
    pub fn is_selected_event_tied(&self) -> bool {
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
                        if let Some(n_idx) = self.selected_note_idx_in_event {
                            if let Some(n) = ev.notes.get(n_idx) {
                                return n.ties.as_ref().map_or(false, |t| !t.is_empty());
                            }
                        }
                        return ev
                            .notes
                            .iter()
                            .any(|n| n.ties.as_ref().map_or(false, |t| !t.is_empty()));
                    }
                    cur_e += 1;
                }
            }
        }
        false
    }

    pub fn is_selected_note_sliding(&self) -> bool {
        let e_idx = match self.selected_event_idx {
            Some(i) => i,
            None => return false,
        };
        let doc = match &self.parsed_mnx {
            Some(d) => d,
            None => return false,
        };
        let flat = doc.get_flattened_event_paths();
        if e_idx >= flat.len() {
            return false;
        }
        let ev = match doc.get_event_at_path(flat[e_idx]) {
            Some(e) => e,
            None => return false,
        };
        if ev.notes.is_empty() {
            return false;
        }
        if let Some(n_idx) = self.selected_note_idx_in_event {
            if n_idx < ev.notes.len() {
                return ev.notes[n_idx].slide.is_some();
            }
        }
        false
    }

    pub fn action_toggle_slide(&mut self) {
        if self.is_editing_fret && !self.editing_fret_buf.trim().is_empty() {
            self.commit_fret_edit();
            self.editing_fret_buf.clear();
        }

        let event_idx = match self.selected_event_idx {
            Some(idx) => idx,
            None => return,
        };
        let mut doc = match self.parsed_mnx.clone() {
            Some(d) => d,
            None => return,
        };

        let flattened_events = doc.get_flattened_event_paths();
        if event_idx >= flattened_events.len() {
            return;
        }

        let cur_path = flattened_events[event_idx];
        let ev_cur_clone = match doc.get_event_at_path(cur_path) {
            Some(e) => e.clone(),
            None => return,
        };

        if ev_cur_clone.notes.is_empty() {
            return;
        }

        let note_idx = match self.selected_note_idx_in_event {
            Some(idx) => {
                if idx < ev_cur_clone.notes.len() {
                    idx
                } else if ev_cur_clone.notes.len() == 1 {
                    0
                } else {
                    return;
                }
            }
            None => {
                if let Some(target_str) = self.editing_string {
                    let mut found = None;
                    for (i, n) in ev_cur_clone.notes.iter().enumerate() {
                        if n.string == Some(target_str) {
                            found = Some(i);
                            break;
                        }
                    }
                    match found {
                        Some(idx) => idx,
                        None => {
                            if ev_cur_clone.notes.len() == 1 {
                                0
                            } else {
                                return;
                            }
                        }
                    }
                } else if ev_cur_clone.notes.len() == 1 {
                    0
                } else {
                    return;
                }
            }
        };

        let mut is_removing = false;
        if let Some(note) = ev_cur_clone.notes.get(note_idx) {
            is_removing = note.slide.is_some();
        }

        if !is_removing {
            let mut slide_valid = false;
            if let Some(s) = ev_cur_clone.notes[note_idx].string {
                if event_idx + 1 < flattened_events.len() {
                    if let Some(next_ev) = doc.get_event_at_path(flattened_events[event_idx + 1]) {
                        if next_ev.notes.iter().any(|n| n.string == Some(s)) {
                            slide_valid = true;
                        }
                    }
                }
            }
            if !slide_valid {
                return;
            }
        }

        if let Some(ev) = doc.get_event_mut_at_path(cur_path) {
            if let Some(note) = ev.notes.get_mut(note_idx) {
                if note.slide.is_some() {
                    note.slide = None;
                } else {
                    note.slide = Some(crate::mnx::Slide {});
                }
            }
        }

        self.selected_note_idx_in_event = Some(note_idx);
        if let Some(s) = ev_cur_clone.notes[note_idx].string {
            self.editing_string = Some(s);
        }

        if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
            self.update_yaml(new_yaml);
            self.process_and_render();
        }
    }

    pub fn is_selected_note_tapped(&self) -> bool {
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
                        if let Some(n_idx) = self.selected_note_idx_in_event {
                            if let Some(n) = ev.notes.get(n_idx) {
                                return n.tap.is_some();
                            }
                        } else if let Some(cur_str) = self.editing_string {
                            return ev
                                .notes
                                .iter()
                                .any(|n| n.string == Some(cur_str) && n.tap.is_some());
                        }
                        return ev.notes.iter().any(|n| n.tap.is_some());
                    }
                    cur_e += 1;
                }
            }
        }
        false
    }

    pub fn update_yaml_and_clean_ties(&mut self, new_yaml: String) {
        if let Ok(mut doc) = serde_saphyr::from_str::<crate::mnx::MnxDocument>(&new_yaml) {
            doc.clean_dangling_ties();
            if let Ok(clean_yaml) = serde_saphyr::to_string(&doc) {
                self.update_yaml(clean_yaml);
            } else {
                self.yaml_text = new_yaml;
            }
        } else {
            self.yaml_text = new_yaml;
        }
        self.process_and_render();
    }

    pub fn convert_selected_to_rest(&mut self) {
        if let Some(e_idx) = self.selected_event_idx {
            if let Some(new_yaml) =
                crate::yaml_editor::convert_event_to_rest(&self.yaml_text, e_idx)
            {
                self.update_yaml_and_clean_ties(new_yaml);
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
                        self.update_yaml(new_yaml);
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
                self.update_yaml(new_yaml);
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
                self.update_yaml(new_yaml);
                self.process_and_render();
            }
        }
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

        println!("Adding measure to yaml...");
        if let Some(new_yaml) = add_measure_to_yaml(&self.yaml_text, &last_duration) {
            println!("Updating yaml...");
            self.update_yaml(new_yaml);
            println!("Process and render...");
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

    pub fn insert_time_signature(&mut self, m_idx: usize, count: u32, unit: u32, rebar_mode: u8) {
        let mut doc = match self.parsed_mnx.as_ref() {
            Some(d) => d.clone(),
            None => return,
        };

        while doc.global.measures.len() <= m_idx {
            let ts = doc
                .global
                .measures
                .last()
                .and_then(|gm| gm.time.clone())
                .unwrap_or(crate::mnx::TimeSignature { count: 4, unit: 4 });
            doc.global.measures.push(crate::mnx::GlobalMeasure {
                time: Some(ts),
                repeat_start: None, repeat_end: None, barline: None, tempos: Vec::new(),
                    tempo_changes: Vec::new(),
            });
        }

        doc.global.measures[m_idx].time = Some(crate::mnx::TimeSignature { count, unit });

        if rebar_mode == 0 {
            // Re-bar all following
            for i in (m_idx + 1)..doc.global.measures.len() {
                doc.global.measures[i].time = None;
            }
        } else if rebar_mode == 1 {
            // Truncate content in this bar only
            let capacity = (count as f64) * 4.0 / (unit as f64);
            let part = doc.parts.first_mut().unwrap();
            if let Some(measure) = part.measures.get_mut(m_idx) {
                let mut new_seq = Vec::new();
                let mut current_beats = 0.0;

                for seq in &measure.sequences {
                    for item in &seq.content {
                        let item_beats = match item {
                            crate::mnx::SequenceItem::Event(ev) => {
                                crate::validate::duration_to_quarter_beats(&ev.duration)
                            }
                            crate::mnx::SequenceItem::Tuplet(t) => {
                                crate::validate::duration_to_quarter_beats(&t.outer.duration)
                                    * (t.outer.multiple as f64)
                            }
                            crate::mnx::SequenceItem::Grace(_) => 0.0,
                        };

                        if current_beats + item_beats <= capacity + 0.001 {
                            new_seq.push(item.clone());
                            current_beats += item_beats;
                        } else if current_beats < capacity - 0.001 {
                            let excess = current_beats + item_beats - capacity;
                            let keep_beats = item_beats - excess;
                            match item {
                                crate::mnx::SequenceItem::Event(ev) => {
                                    let chunks = crate::rebar::greedy_split_event_duration(
                                        ev.clone(),
                                        keep_beats,
                                    );
                                    for c in chunks {
                                        new_seq.push(crate::mnx::SequenceItem::Event(c));
                                    }
                                }
                                crate::mnx::SequenceItem::Tuplet(_t) => {
                                    new_seq.push(item.clone());
                                }
                                crate::mnx::SequenceItem::Grace(_) => {
                                    new_seq.push(item.clone());
                                }
                            }
                            current_beats = capacity;
                        }
                    }
                }
                if let Some(seq) = measure.sequences.first_mut() {
                    seq.content = new_seq;
                }
            }
        }

        let part = doc.parts.first_mut().unwrap();
        crate::rebar::rebar_part(part, &mut doc.global.measures);

        if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
            self.update_yaml(new_yaml);
            self.process_and_render();
        }
    }

    pub fn is_selected_event_strummed(&self, dir: &str) -> bool {
        if let Some(doc) = &self.parsed_mnx {
            if let Some(idx) = self.selected_event_idx {
                let flattened = doc.get_flattened_event_paths();
                if idx < flattened.len() {
                    if let Some(ev) = doc.get_event_at_path(flattened[idx]) {
                        if let Some(markings) = &ev.markings {
                            if let Some(bow) = &markings.bow_direction {
                                return bow.direction == dir;
                            }
                        }
                    }
                }
            }
        }
        false
    }

    pub fn is_selected_event_accented(&self) -> bool {
        if let Some(doc) = &self.parsed_mnx {
            if let Some(idx) = self.selected_event_idx {
                let flattened = doc.get_flattened_event_paths();
                if idx < flattened.len() {
                    if let Some(ev) = doc.get_event_at_path(flattened[idx]) {
                        if let Some(markings) = &ev.markings {
                            return markings.accent.is_some();
                        }
                    }
                }
            }
        }
        false
    }

    pub fn action_toggle_strum(&mut self, dir: &str) {
        let event_idx = match self.selected_event_idx {
            Some(idx) => idx,
            None => return,
        };
        let mut doc = match self.parsed_mnx.clone() {
            Some(d) => d,
            None => return,
        };
        let flattened = doc.get_flattened_event_paths();
        if event_idx >= flattened.len() {
            return;
        }
        if let Some(ev) = doc.get_event_mut_at_path(flattened[event_idx]) {
            let mut cur_dir = None;
            if let Some(m) = &ev.markings {
                if let Some(b) = &m.bow_direction {
                    cur_dir = Some(b.direction.clone());
                }
            }
            if cur_dir.as_deref() == Some(dir) {
                if let Some(m) = &mut ev.markings {
                    m.bow_direction = None;
                    if m.accent.is_none() {
                        ev.markings = None;
                    }
                }
            } else {
                if ev.markings.is_none() {
                    ev.markings = Some(crate::mnx::EventMarkings {
                        accent: None,
                        bow_direction: None,
                    });
                }
                if let Some(m) = &mut ev.markings {
                    m.bow_direction = Some(crate::mnx::BowDirection {
                        direction: dir.to_string(),
                    });
                }
            }
        }
        if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
            self.update_yaml(new_yaml);
            self.process_and_render();
        }
    }

    pub fn action_toggle_accent(&mut self) {
        let event_idx = match self.selected_event_idx {
            Some(idx) => idx,
            None => return,
        };
        let mut doc = match self.parsed_mnx.clone() {
            Some(d) => d,
            None => return,
        };
        let flattened = doc.get_flattened_event_paths();
        if event_idx >= flattened.len() {
            return;
        }
        if let Some(ev) = doc.get_event_mut_at_path(flattened[event_idx]) {
            let has_accent = ev.markings.as_ref().map_or(false, |m| m.accent.is_some());
            if has_accent {
                if let Some(m) = &mut ev.markings {
                    m.accent = None;
                    if m.bow_direction.is_none() {
                        ev.markings = None;
                    }
                }
            } else {
                if ev.markings.is_none() {
                    ev.markings = Some(crate::mnx::EventMarkings {
                        accent: None,
                        bow_direction: None,
                    });
                }
                if let Some(m) = &mut ev.markings {
                    m.accent = Some(crate::mnx::Accent {});
                }
            }
        }
        if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
            self.update_yaml(new_yaml);
            self.process_and_render();
        }
    }
    pub fn action_add_slur(&mut self) {
        let event_idx = match self.selected_event_idx {
            Some(idx) => idx,
            None => return,
        };
        let mut doc = match self.parsed_mnx.clone() {
            Some(d) => d,
            None => return,
        };

        let flattened_events = doc.get_flattened_event_paths();
        if event_idx >= flattened_events.len() {
            return;
        }

        let cur_path = flattened_events[event_idx];
        let ev_cur_clone = match doc.get_event_at_path(cur_path) {
            Some(e) => e.clone(),
            None => return,
        };

        // Determine current string (fallback to only note if blue mode)
        let cur_string = match self.editing_string {
            Some(s) => s,
            None => {
                if ev_cur_clone.notes.len() == 1 {
                    ev_cur_clone.notes[0].string.unwrap_or(1)
                } else {
                    return;
                }
            }
        };

        // Find the note on cur_string
        let note_idx = match ev_cur_clone
            .notes
            .iter()
            .position(|n| n.string == Some(cur_string))
        {
            Some(idx) => idx,
            None => return,
        };

        let mut toggled_off = false;
        if let Some(ev) = doc.get_event_mut_at_path(cur_path) {
            if let Some(slurs) = &mut ev.slurs {
                let note_cur_id = ev.notes[note_idx].id.clone();
                let original_len = slurs.len();
                if let Some(id) = &note_cur_id {
                    slurs.retain(|s| s.start_note.as_ref() != Some(id));
                } else {
                    // if it has no ID, it couldn't have had a slur
                }
                if slurs.len() < original_len {
                    toggled_off = true;
                }
                if slurs.is_empty() {
                    ev.slurs = None;
                }
            }
        }

        if toggled_off {
            if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
                self.update_yaml(new_yaml);
                self.process_and_render();
            }
            return;
        }

        // Search forward for the next note on the same string
        let mut next_target = None;
        for j in (event_idx + 1)..flattened_events.len() {
            let path_j = flattened_events[j];
            let ev_j = doc.get_event_at_path(path_j).cloned().unwrap();
            if let Some(n_idx) = ev_j.notes.iter().position(|n| n.string == Some(cur_string)) {
                next_target = Some((j, path_j, ev_j, n_idx));
                break;
            }
        }

        if let Some((_, next_path, _, next_n_idx)) = next_target {
            let mut target_ev_id = None;
            let mut end_note_id = None;

            if let Some(ev_next) = doc.get_event_mut_at_path(next_path) {
                if ev_next.id.is_none() {
                    ev_next.id = Some(format!("ev_tgt_{}", event_idx));
                }
                target_ev_id = ev_next.id.clone();

                let n_tgt = &mut ev_next.notes[next_n_idx];
                if n_tgt.id.is_none() {
                    n_tgt.id = Some(format!("note_tgt_{}_{}", event_idx, next_n_idx));
                }
                end_note_id = n_tgt.id.clone();
            }

            if let Some(ev_cur) = doc.get_event_mut_at_path(cur_path) {
                let n_cur = &mut ev_cur.notes[note_idx];
                if n_cur.id.is_none() {
                    n_cur.id = Some(format!("note_src_{}_{}", event_idx, note_idx));
                }
                let start_note_id = n_cur.id.clone();

                if ev_cur.slurs.is_none() {
                    ev_cur.slurs = Some(Vec::new());
                }
                ev_cur.slurs.as_mut().unwrap().push(crate::mnx::Slur {
                    target: target_ev_id.unwrap(),
                    side: Some("up".to_string()),
                    start_note: start_note_id,
                    end_note: end_note_id,
                });
            }

            if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
                self.update_yaml(new_yaml);
                self.process_and_render();
            }
        } else {
            // No notes following on same string.
            self.pending_slur = Some((event_idx, cur_string));
            self.advance_to_next_note_in_edit_mode(Some(cur_string), flattened_events.len());
        }
    }
    pub fn action_add_tie(&mut self) {
        let event_idx = match self.selected_event_idx {
            Some(idx) => idx,
            None => return,
        };
        let mut doc = match self.parsed_mnx.clone() {
            Some(d) => d,
            None => return,
        };

        let flattened_events = doc.get_flattened_event_paths();
        if event_idx >= flattened_events.len() {
            return;
        }

        let cur_path = flattened_events[event_idx];
        let ev_cur_clone = match doc.get_event_at_path(cur_path) {
            Some(e) => e.clone(),
            None => return,
        };

        let (cur_m, cur_s, cur_i) = match cur_path {
            crate::mnx::AstPath::TopLevel(m, s, i) => (m, s, i),
            crate::mnx::AstPath::InTuplet(m, s, i, _) => (m, s, i),
        };

        let note_idx = match self.selected_note_idx_in_event {
            Some(idx) => idx,
            None => {
                if let Some(target_str) = self.editing_string {
                    let mut found = None;
                    for (i, n) in ev_cur_clone.notes.iter().enumerate() {
                        if n.string == Some(target_str) {
                            found = Some(i);
                            break;
                        }
                    }
                    match found {
                        Some(idx) => idx,
                        None => {
                            if ev_cur_clone.notes.len() == 1 {
                                0
                            } else {
                                return;
                            }
                        }
                    }
                } else {
                    if ev_cur_clone.notes.len() == 1 {
                        0
                    } else {
                        return;
                    }
                }
            }
        };

        let mut toggled_off = false;
        if let Some(ev) = doc.get_event_mut_at_path(cur_path) {
            let note_cur = &mut ev.notes[note_idx];
            if let Some(ties) = &note_cur.ties {
                if !ties.is_empty() {
                    note_cur.ties = None;
                    toggled_off = true;
                }
            }
        }

        if toggled_off {
            if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
                self.update_yaml(new_yaml);
                self.process_and_render();
            }
            return;
        }

        let cur_string = match ev_cur_clone.notes.get(note_idx) {
            Some(n) => match n.string {
                Some(s) => s,
                None => return,
            },
            None => return,
        };
        let cur_fret = ev_cur_clone.notes[note_idx].fret;

        let next_event_info = flattened_events.get(event_idx + 1).copied();

        if let Some(next_path) = next_event_info {
            let mut target_id = None;
            let mut has_same_string = false;
            let mut diff_fret = false;

            if let Some(ev_next) = doc.get_event_mut_at_path(next_path) {
                for (n_i, n) in ev_next.notes.iter_mut().enumerate() {
                    if n.string == Some(cur_string) {
                        has_same_string = true;
                        if n.fret == cur_fret {
                            if n.id.is_none() {
                                n.id = Some(format!("note_tgt_{}_{}_{}", event_idx, note_idx, n_i));
                            }
                            target_id = n.id.clone();
                        } else {
                            diff_fret = true;
                        }
                        break;
                    }
                }

                if !has_same_string {
                    if ev_next.id.is_none() {
                        ev_next.id = Some(format!("ev_tgt_{}", event_idx));
                    }
                    target_id = ev_next.id.clone();
                }
            }

            if diff_fret {
                return;
            }

            if let Some(tid) = target_id {
                if let Some(ev_cur) = doc.get_event_mut_at_path(cur_path) {
                    let note_cur = &mut ev_cur.notes[note_idx];
                    if note_cur.ties.is_none() {
                        note_cur.ties = Some(Vec::new());
                    }
                    if !note_cur
                        .ties
                        .as_ref()
                        .unwrap()
                        .iter()
                        .any(|t| t.target == tid)
                    {
                        note_cur
                            .ties
                            .as_mut()
                            .unwrap()
                            .push(crate::mnx::Tie { target: tid });
                    }
                }

                if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
                    self.update_yaml(new_yaml);
                    self.process_and_render();
                }
                return;
            }
        }

        // Rule 5: end of tab create notes
        let cur_dur = ev_cur_clone.duration;
        let cur_pitch = ev_cur_clone.notes[note_idx].pitch.clone();
        let new_note_duration_beats = crate::validate::duration_to_quarter_beats(&cur_dur);

        let mut used_beats = 0.0;
        for i in 0..=cur_i {
            match &doc.parts[0].measures[cur_m].sequences[cur_s].content[i] {
                crate::mnx::SequenceItem::Event(e) => {
                    used_beats += crate::validate::duration_to_quarter_beats(&e.duration);
                }
                crate::mnx::SequenceItem::Tuplet(tu) => {
                    used_beats += crate::validate::duration_to_quarter_beats(&tu.outer.duration)
                        * (tu.outer.multiple as f64);
                }
                crate::mnx::SequenceItem::Grace(_) => {
                    // Grace notes have 0.0 beats duration
                }
            }
        }

        let time_sig_count = doc
            .global
            .measures
            .get(cur_m)
            .and_then(|gm| gm.time.as_ref())
            .map(|t| t.count)
            .unwrap_or(4);
        let time_sig_unit = doc
            .global
            .measures
            .get(cur_m)
            .and_then(|gm| gm.time.as_ref())
            .map(|t| t.unit)
            .unwrap_or(4);
        let measure_total_beats = (time_sig_count as f64) * (4.0 / (time_sig_unit as f64));

        let available_beats = (measure_total_beats - used_beats).max(0.0);
        let eps = 0.001;

        if let Some(ev_cur) = doc.get_event_mut_at_path(cur_path) {
            let note_cur = &mut ev_cur.notes[note_idx];
            if note_cur.id.is_none() {
                note_cur.id = Some(format!("note_src_{}", event_idx));
            }
        }

        let next_target_id;

        if available_beats >= new_note_duration_beats - eps {
            let id = format!("note_new_1_{}", event_idx);
            let new_ev = crate::mnx::Event {
                id: None,
                duration: cur_dur,
                notes: vec![crate::mnx::Note {
                    id: Some(id.clone()),
                    pitch: cur_pitch,
                    string: Some(cur_string),
                    fret: cur_fret,
                    ties: None,
                    accidental: None,
                    tap: None,
                    bend: None,
                    muted: None,
                    slide: None,
                    harmonic: None,
                }],
                rest: None,
                slurs: None,
                markings: None,
            };
            next_target_id = id;
            doc.parts[0].measures[cur_m].sequences[cur_s]
                .content
                .insert(cur_i + 1, crate::mnx::SequenceItem::Event(new_ev));
        } else if available_beats > eps {
            let first_dur = match crate::validate::quarter_beats_to_duration(available_beats) {
                Some(d) => d,
                None => return,
            };
            let second_dur = match crate::validate::quarter_beats_to_duration(
                new_note_duration_beats - available_beats,
            ) {
                Some(d) => d,
                None => return,
            };

            let id1 = format!("note_new_split1_{}", event_idx);
            let id2 = format!("note_new_split2_{}", event_idx);

            let ev1 = crate::mnx::Event {
                id: None,
                duration: first_dur,
                notes: vec![crate::mnx::Note {
                    id: Some(id1.clone()),
                    pitch: cur_pitch.clone(),
                    string: Some(cur_string),
                    fret: cur_fret,
                    ties: Some(vec![crate::mnx::Tie {
                        target: id2.clone(),
                    }]),
                    accidental: None,
                    tap: None,
                    bend: None,
                    muted: None,
                    slide: None,
                    harmonic: None,
                }],
                rest: None,
                slurs: None,
                markings: None,
            };

            let ev2 = crate::mnx::Event {
                id: None,
                duration: second_dur,
                notes: vec![crate::mnx::Note {
                    id: Some(id2.clone()),
                    pitch: cur_pitch,
                    string: Some(cur_string),
                    fret: cur_fret,
                    ties: None,
                    accidental: None,
                    tap: None,
                    bend: None,
                    muted: None,
                    slide: None,
                    harmonic: None,
                }],
                rest: None,
                slurs: None,
                markings: None,
            };

            next_target_id = id1;

            doc.parts[0].measures[cur_m].sequences[cur_s]
                .content
                .insert(cur_i + 1, crate::mnx::SequenceItem::Event(ev1));

            let next_m = cur_m + 1;
            if next_m >= doc.parts[0].measures.len() {
                let mut new_m = crate::mnx::Measure {
                    arpeggios: None,
                    sequences: vec![crate::mnx::Sequence {
                        content: Vec::new(),
                        beams: None,
                    }],
                    beams: None,
                };
                new_m.sequences[0]
                    .content
                    .push(crate::mnx::SequenceItem::Event(ev2));
                doc.parts[0].measures.push(new_m);

                if next_m >= doc.global.measures.len() {
                    let gm = crate::mnx::GlobalMeasure {
                        time: Some(crate::mnx::TimeSignature {
                            count: time_sig_count,
                            unit: time_sig_unit,
                        }),
                        repeat_start: None, repeat_end: None, barline: None, tempos: Vec::new(),
                    tempo_changes: Vec::new(),
                    };
                    doc.global.measures.push(gm);
                }
            } else {
                doc.parts[0].measures[next_m].sequences[0]
                    .content
                    .insert(0, crate::mnx::SequenceItem::Event(ev2));
            }
        } else {
            let id2 = format!("note_new_split2_{}", event_idx);
            let ev2 = crate::mnx::Event {
                id: None,
                duration: cur_dur,
                notes: vec![crate::mnx::Note {
                    id: Some(id2.clone()),
                    pitch: cur_pitch,
                    string: Some(cur_string),
                    fret: cur_fret,
                    ties: None,
                    accidental: None,
                    tap: None,
                    bend: None,
                    muted: None,
                    slide: None,
                    harmonic: None,
                }],
                rest: None,
                slurs: None,
                markings: None,
            };

            next_target_id = id2;
            let next_m = cur_m + 1;
            if next_m >= doc.parts[0].measures.len() {
                let mut new_m = crate::mnx::Measure {
                    arpeggios: None,
                    sequences: vec![crate::mnx::Sequence {
                        content: Vec::new(),
                        beams: None,
                    }],
                    beams: None,
                };
                new_m.sequences[0]
                    .content
                    .push(crate::mnx::SequenceItem::Event(ev2));
                doc.parts[0].measures.push(new_m);

                if next_m >= doc.global.measures.len() {
                    let gm = crate::mnx::GlobalMeasure {
                        time: Some(crate::mnx::TimeSignature {
                            count: time_sig_count,
                            unit: time_sig_unit,
                        }),
                        repeat_start: None, repeat_end: None, barline: None, tempos: Vec::new(),
                    tempo_changes: Vec::new(),
                    };
                    doc.global.measures.push(gm);
                }
            } else {
                doc.parts[0].measures[next_m].sequences[0]
                    .content
                    .insert(0, crate::mnx::SequenceItem::Event(ev2));
            }
        }

        if let Some(ev_cur) = doc.get_event_mut_at_path(cur_path) {
            let note_cur = &mut ev_cur.notes[note_idx];
            if note_cur.ties.is_none() {
                note_cur.ties = Some(Vec::new());
            }
            note_cur.ties.as_mut().unwrap().push(crate::mnx::Tie {
                target: next_target_id,
            });
        }

        if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
            self.update_yaml(new_yaml);
            self.process_and_render();
        }
    }

    pub fn action_toggle_tap(&mut self) {
        if self.is_editing_fret && !self.editing_fret_buf.trim().is_empty() {
            self.commit_fret_edit();
            self.editing_fret_buf.clear();
        }

        let event_idx = match self.selected_event_idx {
            Some(idx) => idx,
            None => return,
        };
        let mut doc = match self.parsed_mnx.clone() {
            Some(d) => d,
            None => return,
        };

        let flattened_events = doc.get_flattened_event_paths();
        if event_idx >= flattened_events.len() {
            return;
        }

        let cur_path = flattened_events[event_idx];
        let ev_cur_clone = match doc.get_event_at_path(cur_path) {
            Some(e) => e.clone(),
            None => return,
        };

        if ev_cur_clone.notes.is_empty() {
            return;
        }

        let note_idx = match self.selected_note_idx_in_event {
            Some(idx) => {
                if idx < ev_cur_clone.notes.len() {
                    idx
                } else if ev_cur_clone.notes.len() == 1 {
                    0
                } else {
                    return;
                }
            }
            None => {
                if let Some(target_str) = self.editing_string {
                    let mut found = None;
                    for (i, n) in ev_cur_clone.notes.iter().enumerate() {
                        if n.string == Some(target_str) {
                            found = Some(i);
                            break;
                        }
                    }
                    match found {
                        Some(idx) => idx,
                        None => {
                            if ev_cur_clone.notes.len() == 1 {
                                0
                            } else {
                                return;
                            }
                        }
                    }
                } else if ev_cur_clone.notes.len() == 1 {
                    0
                } else {
                    return;
                }
            }
        };

        if let Some(ev) = doc.get_event_mut_at_path(cur_path) {
            if let Some(note) = ev.notes.get_mut(note_idx) {
                if note.tap.is_some() {
                    note.tap = None;
                } else {
                    note.tap = Some(crate::mnx::Tap {});
                }
            }
        }

        self.selected_note_idx_in_event = Some(note_idx);
        if let Some(s) = ev_cur_clone.notes[note_idx].string {
            self.editing_string = Some(s);
        }

        if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
            self.update_yaml(new_yaml);
            self.process_and_render();
        }
    }
}

impl MnxEditorApp {
    

}

impl crate::gui::MnxEditorApp {
    pub fn action_toggle_mute(&mut self) {
        let event_idx = match self.selected_event_idx {
            Some(idx) => idx,
            None => return,
        };
        let note_idx = match self.selected_note_idx_in_event {
            Some(idx) => idx,
            None => return,
        };
        let mut doc = match self.parsed_mnx.clone() {
            Some(d) => d,
            None => return,
        };
        let flattened = doc.get_flattened_event_paths();
        if event_idx >= flattened.len() {
            return;
        }
        if let Some(ev) = doc.get_event_mut_at_path(flattened[event_idx]) {
            if note_idx < ev.notes.len() {
                let note = &mut ev.notes[note_idx];
                let is_muted = note.muted.unwrap_or(false);
                note.muted = if is_muted { None } else { Some(true) };
            }
        }
        if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
            self.update_yaml(new_yaml);
            self.process_and_render();
        }
    }
}

impl crate::gui::MnxEditorApp {
    #[allow(dead_code)]
    pub fn event_has_any_notes(&self, event_idx: usize) -> bool {
        let doc = match self.parsed_mnx.as_ref() {
            Some(d) => d,
            None => return false,
        };
        let flattened = doc.get_flattened_event_paths();
        if event_idx >= flattened.len() {
            return false;
        }
        if let Some(ev) = doc.get_event_at_path(flattened[event_idx]) {
            return !ev.notes.is_empty();
        }
        false
    }
}

impl crate::gui::MnxEditorApp {
    pub fn is_selected_event_arpeggiated(&self, dir: &str) -> bool {
        if let Some(doc) = &self.parsed_mnx {
            if let Some(idx) = self.selected_event_idx {
                let flattened = doc.get_flattened_event_paths();
                if idx < flattened.len() {
                    let path = flattened[idx];
                    let m_idx = path.m_idx();
                    if let Some(part) = doc.parts.first() {
                        if let Some(m) = part.measures.get(m_idx) {
                            if let Some(ev) = doc.get_event_at_path(path) {
                                let first_id =
                                    ev.notes.first().and_then(|n| n.id.as_deref()).unwrap_or("");
                                let last_id =
                                    ev.notes.last().and_then(|n| n.id.as_deref()).unwrap_or("");
                                if let Some(arps) = &m.arpeggios {
                                    for arp in arps {
                                        if arp.span.start == first_id && arp.span.end == last_id {
                                            return arp.direction.as_deref() == Some(dir);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        false
    }

    pub fn action_toggle_arpeggio(&mut self, dir: &str) {
        let event_idx = match self.selected_event_idx {
            Some(idx) => idx,
            None => return,
        };
        let mut doc = match self.parsed_mnx.clone() {
            Some(d) => d,
            None => return,
        };
        let flattened = doc.get_flattened_event_paths();
        if event_idx >= flattened.len() {
            return;
        }
        let path = flattened[event_idx];
        let m_idx = path.m_idx();

        let mut first_id = String::new();
        let mut last_id = String::new();
        if let Some(ev) = doc.get_event_mut_at_path(path) {
            if ev.notes.len() < 2 {
                return;
            }
            let f_note = ev.notes.first_mut().unwrap();
            if f_note.id.is_none() {
                f_note.id = Some(format!("arp_{}_start", event_idx));
            }
            first_id = f_note.id.clone().unwrap();

            let l_note = ev.notes.last_mut().unwrap();
            if l_note.id.is_none() {
                l_note.id = Some(format!("arp_{}_end", event_idx));
            }
            last_id = l_note.id.clone().unwrap();
        }

        let part = doc.parts.first_mut().unwrap();
        let measure = part.measures.get_mut(m_idx).unwrap();
        let mut existing_idx = None;
        if let Some(arps) = &mut measure.arpeggios {
            for (i, arp) in arps.iter().enumerate() {
                if arp.span.start == first_id && arp.span.end == last_id {
                    existing_idx = Some(i);
                    break;
                }
            }
        }

        if let Some(idx) = existing_idx {
            let arps = measure.arpeggios.as_mut().unwrap();
            if arps[idx].direction.as_deref() == Some(dir) {
                arps.remove(idx);
                if arps.is_empty() {
                    measure.arpeggios = None;
                }
            } else {
                arps[idx].direction = Some(dir.to_string());
            }
        } else {
            if measure.arpeggios.is_none() {
                measure.arpeggios = Some(Vec::new());
            }
            measure
                .arpeggios
                .as_mut()
                .unwrap()
                .push(crate::mnx::Arpeggio {
                    position: None,
                    span: crate::mnx::Span {
                        start: first_id,
                        end: last_id,
                    },
                    direction: Some(dir.to_string()),
                    arrow: true,
                });
        }

        if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
            self.update_yaml(new_yaml);
            self.process_and_render();
        }
    }
}

impl crate::gui::MnxEditorApp {
    pub fn action_toggle_grace(&mut self) {
        if let (Some(e_idx), Some(doc)) = (self.selected_event_idx, &self.parsed_mnx) {
            if let Some(info) = doc.find_event_grace_info(e_idx) {
                // We are in a grace block. Pop out.
                // Move selection to the event after the grace block.
                self.selected_event_idx = Some(info.event_range.end);
            } else {
                // Not in a grace block.
                // If the immediately preceding event is a grace block leading into this note,
                // toggle back into that grace block instead of inserting a duplicate!
                if e_idx > 0 {
                    if let Some(prev_info) = doc.find_event_grace_info(e_idx - 1) {
                        if prev_info.event_range.end == e_idx {
                            self.selected_event_idx = Some(prev_info.event_range.end - 1);
                            return;
                        }
                    }
                }
                if let Some(new_yaml) = crate::yaml_editor::insert_grace_before_in_yaml(&self.yaml_text, e_idx) {
                    self.update_yaml(new_yaml);
                    self.process_and_render();
                }
            }
        }
    }

    pub fn action_delete_grace_event(&mut self, e_idx: usize) {
        let mut doc = match self.parsed_mnx.clone() {
            Some(d) => d,
            None => return,
        };
        let info = match doc.find_event_grace_info(e_idx) {
            Some(i) => i,
            None => return,
        };

        let target_start = info.event_range.start;
        let inner_idx = info.inner_event_idx;
        let count = info.event_range.end - target_start;

        let part = match doc.parts.first_mut() {
            Some(p) => p,
            None => return,
        };

        let mut cur_e = 0;
        let mut removed = false;
        'outer: for m in &mut part.measures {
            for seq in &mut m.sequences {
                let mut i = 0;
                while i < seq.content.len() {
                    let item = &mut seq.content[i];
                    if let crate::mnx::SequenceItem::Grace(g) = item {
                        let g_count = g.content.len();
                        if cur_e == target_start {
                            if count <= 1 || g.content.len() <= 1 {
                                seq.content.remove(i);
                            } else if inner_idx < g.content.len() {
                                g.content.remove(inner_idx);
                            }
                            removed = true;
                            break 'outer;
                        }
                        cur_e += g_count;
                        i += 1;
                    } else {
                        let item_ev_count = match item {
                            crate::mnx::SequenceItem::Event(_) => 1,
                            crate::mnx::SequenceItem::Tuplet(t) => t.content.len(),
                            _ => 1,
                        };
                        cur_e += item_ev_count;
                        i += 1;
                    }
                }
            }
        }

        if !removed {
            return;
        }

        doc.clean_dangling_ties();

        if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
            self.update_yaml(new_yaml);
            self.process_and_render();

            let total_events = crate::yaml_editor::find_all_event_ranges(&self.yaml_text).len();
            if total_events == 0 {
                self.selected_event_idx = None;
            } else if e_idx < total_events {
                self.selected_event_idx = Some(e_idx);
            } else {
                self.selected_event_idx = Some(total_events - 1);
            }

            self.is_editing_fret = false;
            self.editing_fret_buf.clear();
            self.selected_note_idx_in_event = None;
            self.editing_string = None;
            self.selection_anchor = None;
        }
    }
}
