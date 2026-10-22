use super::*;
use crate::yaml_editor::*;
use eframe::egui::{self};

impl MnxEditorApp {
    pub(crate) fn select_note_on_string(&mut self, e_idx: usize, string_num: u8) {
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

    pub(crate) fn commit_fret_edit(&mut self) -> bool {
        let mut committed = false;
        if let Some(e_idx) = self.selected_event_idx {
            if let Some(cur_str) = self.editing_string {
                let is_x = self.editing_fret_buf.to_lowercase() == "x";
                let parsed = if is_x {
                    Ok(0)
                } else {
                    self.editing_fret_buf.parse::<i32>()
                };
                if let Ok(new_fret) = parsed {
                    let new_pitch = self
                        .parsed_mnx
                        .as_ref()
                        .and_then(|d| d.parts.first())
                        .and_then(|part| calculate_pitch_for_string_fret(part, cur_str, new_fret));

                    let res = self.mutate_doc(|doc| {
                        if set_or_add_note_on_string_in_doc(
                            doc,
                            e_idx,
                            cur_str,
                            new_fret,
                            new_pitch.as_ref(),
                        ) {
                            Some(())
                        } else {
                            None
                        }
                    });

                    if res.is_some() {
                        self.select_note_on_string(e_idx, cur_str);
                        if !is_x {
                            self.preview_selected_sound();
                        }
                        if self.pending_slur.is_some() {
                            self.resolve_pending_slur();
                        }
                        if is_x {
                            self.action_toggle_mute();
                        }
                        committed = true;
                    }
                }
            }
        }
        committed
    }

    pub(crate) fn handle_tuplet_right_arrow(
        &mut self,
        e_idx: usize,
        _total_events: usize,
    ) -> Option<usize> {
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
            if let Some(new_idx) = self.mutate_doc(|doc| {
                crate::yaml_editor::insert_blank_note_after_tuplet_in_doc(
                    doc,
                    e_idx,
                    &final_note_base,
                )
            }) {
                return Some(new_idx);
            }
        } else if is_last_measure {
            // Measure is full, and it's the last measure: add a new measure with blank note
            if self
                .mutate_doc(|doc| {
                    if crate::yaml_editor::add_measure_to_doc(doc, &dur) {
                        Some(())
                    } else {
                        None
                    }
                })
                .is_some()
            {
                let new_total = self.cached_event_ranges.len();
                return Some(new_total.saturating_sub(1));
            }
        } else {
            // Measure is full, but next measure exists: move to next measure's first event
            return Some(info.event_range.end);
        }

        None
    }

    pub(crate) fn advance_to_next_note_in_edit_mode(
        &mut self,
        cur_str: Option<u8>,
        total_events: usize,
    ) {
        if let Some(e_idx) = self.selected_event_idx {
            let mut is_cur_blank = false;
            if let Some(doc) = &self.parsed_mnx {
                if let Some(part) = doc.parts.first() {
                    let mut count = 0;
                    'find_cur: for m in &part.measures {
                        for seq in &m.sequences {
                            for ev in seq.flattened_events() {
                                if count == e_idx {
                                    is_cur_blank = ev.notes.is_empty() && ev.rest.is_none();
                                    break 'find_cur;
                                }
                                count += 1;
                            }
                        }
                    }
                }
            }

            if is_cur_blank {
                return;
            }

            let next_idx = if let Some(tuplet_next) =
                self.handle_tuplet_right_arrow(e_idx, total_events)
            {
                tuplet_next
            } else if let Some(grace_next) = self.handle_grace_right_arrow(e_idx) {
                grace_next
            } else {
                let at_final_event = e_idx >= total_events.saturating_sub(1);
                if at_final_event {
                    let mut added_idx = None;
                    let measure_info = self.parsed_mnx.as_ref().and_then(|doc| {
                        let part = doc.parts.first()?;
                        let last_measure = part.measures.last()?;
                        let last_seq = last_measure.sequences.first()?;
                        let last_event = last_seq.flattened_events().last().copied()?;
                        let m_idx = part.measures.len() - 1;
                        let final_dur = last_event.duration.clone();
                        let dur_beats = validate::duration_to_quarter_beats(&final_dur);
                        let time_sig = validate::get_time_signature_for_measure(doc, m_idx);
                        let max_beats = (time_sig.count as f64) * (4.0 / time_sig.unit as f64);
                        let curr_beats: f64 = last_seq.total_beats();
                        Some((final_dur, curr_beats + dur_beats <= max_beats + 1e-4))
                    });

                    if let Some((final_dur, fits_in_measure)) = measure_info {
                        if fits_in_measure {
                            if self
                                .mutate_doc(|doc| {
                                    if crate::yaml_editor::add_blank_note_to_last_measure_in_doc(
                                        doc, &final_dur,
                                    ) {
                                        Some(())
                                    } else {
                                        None
                                    }
                                })
                                .is_some()
                            {
                                let new_total = self.cached_event_ranges.len();
                                if new_total > 0 {
                                    added_idx = Some(new_total - 1);
                                }
                            }
                        } else {
                            self.add_measure_to_end();
                            let new_total = self.cached_event_ranges.len();
                            if new_total > 0 {
                                added_idx = Some(new_total - 1);
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

    pub(crate) fn retreat_to_prev_note_in_edit_mode(&mut self, cur_str: Option<u8>) {
        if let Some(e_idx) = self.selected_event_idx {
            let total_events = self.cached_event_ranges.len();
            let at_final_event = e_idx >= total_events.saturating_sub(1);
            let is_final_blank = at_final_event
                && self
                    .parsed_mnx
                    .as_ref()
                    .and_then(|d| d.parts.first())
                    .and_then(|p| p.measures.last())
                    .and_then(|m| m.sequences.first())
                    .and_then(|s| s.flattened_events().last().copied())
                    .map(|e| e.notes.is_empty() && e.rest.is_none())
                    .unwrap_or(false);

            if is_final_blank {
                if self
                    .mutate_doc(|doc| {
                        if crate::yaml_editor::remove_last_blank_event_in_doc(doc) {
                            Some(())
                        } else {
                            None
                        }
                    })
                    .is_some()
                {
                    self.selected_event_idx = Some(e_idx.saturating_sub(1));
                    self.editing_string = cur_str;
                    self.editing_fret_buf.clear();
                    self.is_editing_fret = true;
                    if let Some(s) = cur_str {
                        self.select_note_on_string(e_idx.saturating_sub(1), s);
                    }
                    return;
                }
            }

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

    pub(crate) fn get_string_for_note(&self, e_idx: usize, note_idx: usize) -> Option<u8> {
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

    pub(crate) fn find_primary_string_for_event(&self, e_idx: usize) -> Option<u8> {
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

    pub(crate) fn event_has_note_on_string(&self, e_idx: usize, string_num: u8) -> bool {
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

    pub(crate) fn handle_keyboard_input(
        &mut self,
        ui: &mut egui::Ui,
        editor_focused: bool,
        event_ranges: &[EventRange],
        total_events: usize,
    ) {
        if self.active_dialog != ActiveDialog::None {
            return;
        }

        if ui.memory(|m| {
            m.has_focus(egui::Id::new("bpm_input"))
                || m.has_focus(egui::Id::new("yaml_source_editor"))
        }) {
            return;
        }

        let shift_down = ui.input(|i| i.modifiers.shift);
        let command_down = ui.input(|i| i.modifiers.command);
        let ctrl_down = ui.input(|i| i.modifiers.ctrl);
        let cmd = command_down || ctrl_down;

        let z_pressed = ui.input(|i| i.key_pressed(egui::Key::Z));
        let y_pressed = ui.input(|i| i.key_pressed(egui::Key::Y));
        let c_pressed = ui.input(|i| i.key_pressed(egui::Key::C));
        let x_pressed = ui.input(|i| i.key_pressed(egui::Key::X));
        let v_pressed = ui.input(|i| i.key_pressed(egui::Key::V));
        let i_pressed = ui.input(|i| i.key_pressed(egui::Key::I));
        let d_pressed = ui.input(|i| i.key_pressed(egui::Key::D));

        let mut is_copy = false;
        let mut is_cut = false;
        let mut is_paste = false;
        ui.input(|i| {
            for event in &i.events {
                match event {
                    egui::Event::Copy => is_copy = true,
                    egui::Event::Cut => is_cut = true,
                    egui::Event::Paste(_) => is_paste = true,
                    _ => {}
                }
            }
        });

        let mut handled = false;
        if is_copy || (cmd && c_pressed) {
            self.action_copy();
            let cb_after = self
                .clipboard
                .as_ref()
                .map(|cb| cb.items.len())
                .unwrap_or(0);
            println!("action_copy finished. cb_after: {}", cb_after);
            ui.ctx().copy_text("mnx_tab_cli_data".to_string());
            ui.input_mut(|i| i.events.clear());
            handled = true;
        } else if is_cut || (cmd && x_pressed) {
            if shift_down {
                self.action_cut_time();
            } else {
                self.action_cut_overwrite();
            }
            ui.ctx().copy_text("mnx_tab_cli_data".to_string());
            ui.input_mut(|i| i.events.clear());
            handled = true;
        } else if is_paste || (cmd && v_pressed) {
            if shift_down {
                self.action_paste_insert();
            } else {
                self.action_paste_overwrite();
            }
            ui.input_mut(|i| i.events.clear());
            handled = true;
        }

        if handled {
            return;
        }

        if cmd {
            if y_pressed {
                self.redo();
                ui.input_mut(|i| i.events.clear()); // consume
                return;
            } else if z_pressed {
                if shift_down {
                    self.redo();
                } else {
                    self.undo();
                }
                ui.input_mut(|i| i.events.clear()); // consume
                return;
            } else if i_pressed {
                self.action_insert_time();
                ui.input_mut(|i| i.events.clear());
                return;
            } else if d_pressed {
                if shift_down {
                    self.action_duplicate_insert();
                } else {
                    self.action_duplicate_overwrite();
                }
                ui.input_mut(|i| i.events.clear());
                return;
            }
        }

        // In-place Lyric Editing Key Handling
        if self.is_editing_lyric {
            let esc_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));
            let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
            let space_pressed = ui.input(|i| i.key_pressed(egui::Key::Space));
            let backspace_pressed = ui.input(|i| i.key_pressed(egui::Key::Backspace));
            let tab_pressed = ui.input(|i| i.key_pressed(egui::Key::Tab));

            if esc_pressed || enter_pressed {
                if let Some(e_idx) = self.selected_event_idx {
                    let text = self.editing_lyric_buf.clone();
                    self.action_set_event_lyric(e_idx, &text);
                }
                self.is_editing_lyric = false;
                self.editing_lyric_buf.clear();
                ui.input_mut(|i| i.events.clear());
                return;
            } else if space_pressed || tab_pressed {
                if let Some(e_idx) = self.selected_event_idx {
                    let text = self.editing_lyric_buf.clone();
                    self.action_set_event_lyric(e_idx, &text);
                    if e_idx + 1 < total_events {
                        self.selected_event_idx = Some(e_idx + 1);
                        self.load_lyric_buf_for_selected();
                    } else {
                        self.is_editing_lyric = false;
                        self.editing_lyric_buf.clear();
                    }
                }
                ui.input_mut(|i| i.events.clear());
                return;
            } else if backspace_pressed {
                if !self.editing_lyric_buf.is_empty() {
                    self.editing_lyric_buf.pop();
                } else if let Some(e_idx) = self.selected_event_idx {
                    if e_idx > 0 {
                        self.selected_event_idx = Some(e_idx - 1);
                        self.load_lyric_buf_for_selected();
                    }
                }
                ui.input_mut(|i| i.events.clear());
                return;
            } else {
                let events = ui.input(|i| i.events.clone());
                for ev in events {
                    if let egui::Event::Text(t) = ev {
                        for c in t.chars() {
                            if c == '-' {
                                self.editing_lyric_buf.push('-');
                                if let Some(e_idx) = self.selected_event_idx {
                                    let text = self.editing_lyric_buf.clone();
                                    self.action_set_event_lyric(e_idx, &text);
                                    if e_idx + 1 < total_events {
                                        self.selected_event_idx = Some(e_idx + 1);
                                        self.load_lyric_buf_for_selected();
                                    } else {
                                        self.is_editing_lyric = false;
                                        self.editing_lyric_buf.clear();
                                    }
                                }
                            } else if c == '_' {
                                self.editing_lyric_buf.push('_');
                                if let Some(e_idx) = self.selected_event_idx {
                                    let text = self.editing_lyric_buf.clone();
                                    self.action_set_event_lyric(e_idx, &text);
                                    if e_idx + 1 < total_events {
                                        self.selected_event_idx = Some(e_idx + 1);
                                        self.load_lyric_buf_for_selected();
                                    } else {
                                        self.is_editing_lyric = false;
                                        self.editing_lyric_buf.clear();
                                    }
                                }
                            } else if !c.is_control() {
                                self.editing_lyric_buf.push(c);
                            }
                        }
                    }
                }
                ui.input_mut(|i| i.events.clear());
                return;
            }
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
            if up_pressed || down_pressed || left_pressed || right_pressed {
                self.needs_scroll = true;
            }
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

                let mut committed = false;
                if !self.editing_fret_buf.trim().is_empty() {
                    committed = self.commit_fret_edit();
                }

                if !committed {
                    if let (Some(e_idx), Some(s)) = (e_idx_opt, cur_str) {
                        if is_current_str_existing {
                            self.select_note_on_string(e_idx, s);
                            self.preview_selected_sound();
                        }
                    }
                }
                self.editing_fret_buf.clear();

                if right_pressed {
                    self.advance_to_next_note_in_edit_mode(cur_str, total_events);
                } else if left_pressed {
                    self.retreat_to_prev_note_in_edit_mode(cur_str);
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
                    if let Some(doc) = &self.parsed_mnx {
                        if doc.find_event_grace_info(e_idx).is_some() {
                            let note_count = doc
                                .get_flattened_event_paths()
                                .get(e_idx)
                                .and_then(|p| doc.get_event_at_path(*p))
                                .map(|ev| ev.notes.len())
                                .unwrap_or(0);
                            if note_count <= 1 {
                                self.action_delete_grace_event(e_idx);
                                return;
                            }
                        }
                    }
                    if self
                        .mutate_doc_and_clean_ties(|doc| {
                            if crate::yaml_editor::remove_note_on_string_in_doc(
                                doc, e_idx, cur_str,
                            ) {
                                Some(())
                            } else {
                                None
                            }
                        })
                        .is_some()
                    {
                        self.selected_note_idx_in_event = None;
                    }
                }
            } else if delete_pressed {
                if let (Some(e_idx), Some(cur_str)) = (self.selected_event_idx, self.editing_string)
                {
                    if let Some(doc) = &self.parsed_mnx {
                        if doc.find_event_grace_info(e_idx).is_some() {
                            let note_count = doc
                                .get_flattened_event_paths()
                                .get(e_idx)
                                .and_then(|p| doc.get_event_at_path(*p))
                                .map(|ev| ev.notes.len())
                                .unwrap_or(0);
                            if note_count <= 1 {
                                self.action_delete_grace_event(e_idx);
                                return;
                            }
                        }
                    }
                    if self
                        .mutate_doc_and_clean_ties(|doc| {
                            if crate::yaml_editor::remove_note_on_string_in_doc(
                                doc, e_idx, cur_str,
                            ) {
                                Some(())
                            } else {
                                None
                            }
                        })
                        .is_some()
                    {
                        self.editing_fret_buf.clear();
                        self.selected_note_idx_in_event = None;
                    }
                }
            } else {
                let events = ui.input(|i| i.events.clone());
                for ev in events {
                    if let egui::Event::Text(t) = ev {
                        for c in t.chars() {
                            if (c.is_ascii_digit() || c == 'x' || c == 'X')
                                && self.editing_fret_buf.len() < 3
                            {
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

            let shift_l_pressed = ui.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::L));
            if shift_l_pressed {
                let e_idx = self.selected_event_idx.unwrap_or(0);
                self.is_editing_fret = false;
                self.open_lyric_dialog(e_idx, 1);
                ui.input_mut(|i| i.events.clear());
                return;
            }

            // Duration keyboard shortcuts: 8 (whole), 7 (half), 6 (quarter), 5 (eighth), 4 (16th), 3 (32nd), . (dot)
            let key_8 = ui.input(|i| i.key_pressed(egui::Key::Num8));
            let key_7 = ui.input(|i| i.key_pressed(egui::Key::Num7));
            let key_6 = ui.input(|i| i.key_pressed(egui::Key::Num6));
            let key_5 = ui.input(|i| i.key_pressed(egui::Key::Num5));
            let key_4 = ui.input(|i| i.key_pressed(egui::Key::Num4));
            let key_3 = ui.input(|i| i.key_pressed(egui::Key::Num3));
            let key_dot = ui.input(|i| i.key_pressed(egui::Key::Period));
            let key_x = ui.input(|i| i.key_pressed(egui::Key::X));

            if key_8 {
                self.change_selected_duration_base("whole");
                if key_x {
                    self.action_toggle_mute();
                }
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
            if up_pressed || down_pressed || left_pressed || right_pressed {
                self.needs_scroll = true;
            }
            let r_pressed = ui.input(|i| i.key_pressed(egui::Key::R));

            let delete_pressed = ui.input(|i| i.key_pressed(egui::Key::Delete));
            if r_pressed && self.selected_event_idx.is_some() {
                self.convert_selected_to_rest();
            } else if (backspace_pressed || delete_pressed) && self.selected_event_idx.is_some() {
                let e_idx = self.selected_event_idx.unwrap();
                let anchor = self.selection_anchor.unwrap_or(e_idx);
                if e_idx != anchor {
                    self.action_delete_overwrite();
                    return;
                }

                if let Some(doc) = &self.parsed_mnx {
                    if doc.find_event_grace_info(e_idx).is_some() {
                        let note_count = doc
                            .get_flattened_event_paths()
                            .get(e_idx)
                            .and_then(|p| doc.get_event_at_path(*p))
                            .map(|ev| ev.notes.len())
                            .unwrap_or(0);
                        if note_count > 1 && self.selected_note_idx_in_event.is_some() {
                            let n_idx = self.selected_note_idx_in_event.unwrap();
                            if let Some(s) = self.get_string_for_note(e_idx, n_idx) {
                                if self
                                    .mutate_doc_and_clean_ties(|doc| {
                                        if crate::yaml_editor::remove_note_on_string_in_doc(
                                            doc, e_idx, s,
                                        ) {
                                            Some(())
                                        } else {
                                            None
                                        }
                                    })
                                    .is_some()
                                {
                                    self.selected_note_idx_in_event = None;
                                    return;
                                }
                            }
                        }
                        self.action_delete_grace_event(e_idx);
                        return;
                    }
                }

                if let Some(n_idx) = self.selected_note_idx_in_event {
                    if let Some(s) = self.get_string_for_note(e_idx, n_idx) {
                        if self
                            .mutate_doc_and_clean_ties(|doc| {
                                if crate::yaml_editor::remove_note_on_string_in_doc(doc, e_idx, s) {
                                    Some(())
                                } else {
                                    None
                                }
                            })
                            .is_some()
                        {
                            self.selected_note_idx_in_event = None;
                            return;
                        }
                    }
                } else if self.is_selected_event_rest() {
                    if self
                        .mutate_doc_and_clean_ties(|doc| {
                            if crate::yaml_editor::convert_rest_to_blank_in_doc(doc, e_idx) {
                                Some(())
                            } else {
                                None
                            }
                        })
                        .is_some()
                    {
                        return;
                    }
                }

                let at_final_event = e_idx >= total_events.saturating_sub(1);
                let is_final_blank = at_final_event
                    && self
                        .parsed_mnx
                        .as_ref()
                        .and_then(|d| d.parts.first())
                        .and_then(|p| p.measures.last())
                        .and_then(|m| m.sequences.first())
                        .and_then(|s| s.flattened_events().last().copied())
                        .map(|e| e.notes.is_empty() && e.rest.is_none())
                        .unwrap_or(false);

                if is_final_blank {
                    if self
                        .mutate_doc(|doc| {
                            if crate::yaml_editor::remove_last_blank_event_in_doc(doc) {
                                Some(())
                            } else {
                                None
                            }
                        })
                        .is_some()
                    {
                        let new_total = self.cached_event_ranges.len();
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
                        let _ = self.mutate_doc_and_clean_ties(|doc| {
                            if crate::yaml_editor::remove_note_on_string_in_doc(doc, e_idx, s) {
                                Some(())
                            } else {
                                None
                            }
                        });
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
                let (shift_down, alt_down) = ui.input(|i| (i.modifiers.shift, i.modifiers.alt));
                if alt_down && (left_pressed || right_pressed) {
                    if right_pressed {
                        self.action_move_selected_by_grid(1);
                    } else if left_pressed {
                        self.action_move_selected_by_grid(-1);
                    }
                } else if alt_down && (up_pressed || down_pressed) {
                    if up_pressed {
                        self.action_shift_selected_strings(-1);
                    } else if down_pressed {
                        self.action_shift_selected_strings(1);
                    }
                } else if shift_down && (up_pressed || down_pressed) {
                    if up_pressed {
                        self.action_shift_selected_frets(1);
                    } else if down_pressed {
                        self.action_shift_selected_frets(-1);
                    }
                } else {
                    if left_pressed || right_pressed {
                        if shift_down {
                            if self.selection_anchor.is_none() {
                                self.selection_anchor = self.selected_event_idx;
                            }
                        } else {
                            self.selection_anchor = None;
                        }
                    }

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
                                .map(|e| e.notes.is_empty() && e.rest.is_none())
                                .unwrap_or(false);

                        if is_final_blank {
                            // If nothing is entered into this blank note, shifting focus back to the left removes it
                            if self
                                .mutate_doc(|doc| {
                                    if crate::yaml_editor::remove_last_blank_event_in_doc(doc) {
                                        Some(())
                                    } else {
                                        None
                                    }
                                })
                                .is_some()
                            {
                                let new_total = self.cached_event_ranges.len();
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
                        let tuplet_next = cur_e_idx
                            .and_then(|e_i| self.handle_tuplet_right_arrow(e_i, total_events));

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
                                    .map(|e| e.notes.is_empty() && e.rest.is_none())
                                    .unwrap_or(false);

                                if is_final_blank {
                                    // Rule 3: Moving further to the right of a final blank note has no effect.
                                } else if shift_down {
                                    // DO NOT create measures while multi-selecting
                                } else {
                                    let measure_info = self.parsed_mnx.as_ref().and_then(|doc| {
                                        let part = doc.parts.first()?;
                                        let last_measure = part.measures.last()?;
                                        let last_seq = last_measure.sequences.first()?;
                                        let last_event =
                                            last_seq.flattened_events().last().copied()?;
                                        let m_idx = part.measures.len() - 1;
                                        let final_dur = last_event.duration.clone();
                                        let dur_beats =
                                            validate::duration_to_quarter_beats(&final_dur);
                                        let time_sig =
                                            validate::get_time_signature_for_measure(doc, m_idx);
                                        let max_beats =
                                            (time_sig.count as f64) * (4.0 / time_sig.unit as f64);
                                        let curr_beats: f64 = last_seq.total_beats();
                                        Some((final_dur, curr_beats + dur_beats <= max_beats + 1e-4))
                                    });

                                    if let Some((final_dur, fits_in_measure)) = measure_info {
                                        if fits_in_measure {
                                            // Rule 1: Measure would NOT be completed by additional note -> add blank note to current measure
                                            if self
                                                .mutate_doc(|doc| {
                                                    if crate::yaml_editor::add_blank_note_to_last_measure_in_doc(
                                                        doc, &final_dur,
                                                    ) {
                                                        Some(())
                                                    } else {
                                                        None
                                                    }
                                                })
                                                .is_some()
                                            {
                                                let new_total = self.cached_event_ranges.len();
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
    }

    pub(crate) fn resolve_pending_slur(&mut self) {
        let (start_idx, slur_str) = match self.pending_slur {
            Some(s) => s,
            None => return,
        };
        let cur_idx = match self.selected_event_idx {
            Some(i) => i,
            None => return,
        };
        let cur_str = match self.editing_string {
            Some(s) => s,
            None => return,
        };

        if cur_str != slur_str || cur_idx <= start_idx {
            self.pending_slur = None;
            return;
        }

        self.mutate_doc(|doc| {
            let flattened_events = doc.get_flattened_event_paths();

            if start_idx >= flattened_events.len() || cur_idx >= flattened_events.len() {
                return None;
            }

            let start_path = flattened_events[start_idx];
            let cur_path = flattened_events[cur_idx];

            let mut start_note_id = None;
            let mut target_ev_id = None;
            let mut target_note_id = None;

            // Give the target event and note IDs
            if let Some(ev_cur) = doc.get_event_mut_at_path(cur_path) {
                if ev_cur.id.is_none() {
                    ev_cur.id = Some(format!("ev_tgt_{}", start_idx));
                }
                target_ev_id = ev_cur.id.clone();

                if let Some(n) = ev_cur.notes.iter_mut().find(|n| n.string == Some(cur_str)) {
                    if n.id.is_none() {
                        n.id = Some(format!("note_tgt_{}_{}", start_idx, cur_idx));
                    }
                    target_note_id = n.id.clone();
                }
            }

            if target_note_id.is_none() {
                return None;
            }

            // Give the start note an ID and append the slur
            if let Some(ev_start) = doc.get_event_mut_at_path(start_path) {
                if let Some(n) = ev_start
                    .notes
                    .iter_mut()
                    .find(|n| n.string == Some(slur_str))
                {
                    if n.id.is_none() {
                        n.id = Some(format!("note_src_{}_{}", start_idx, cur_idx));
                    }
                    start_note_id = n.id.clone();
                }

                if start_note_id.is_some() {
                    if ev_start.slurs.is_none() {
                        ev_start.slurs = Some(Vec::new());
                    }
                    ev_start.slurs.as_mut().unwrap().push(crate::mnx::Slur {
                        target: target_ev_id.unwrap(),
                        side: Some("up".to_string()),
                        start_note: start_note_id,
                        end_note: target_note_id,
                    });
                }
            }
            Some(())
        });
        self.pending_slur = None;
    }
    
    pub(crate) fn handle_grace_right_arrow(
        &mut self,
        e_idx: usize,
    ) -> Option<usize> {
        let is_eligible = {
            let doc = self.parsed_mnx.as_ref()?;
            let info = doc.find_event_grace_info(e_idx)?;

            // Must be the final event of the grace block
            if e_idx != info.event_range.end - 1 {
                return None;
            }

            let part = doc.parts.first()?;
            let flat_events = part
                .measures
                .iter()
                .flat_map(|m| m.sequences.iter())
                .flat_map(|s| s.flattened_events())
                .collect::<Vec<_>>();
            let cur_ev = flat_events.get(e_idx)?;
            if cur_ev.notes.is_empty() && cur_ev.rest.is_none() {
                return None; // Still blank, don't advance
            }
            true
        };

        if !is_eligible {
            return None;
        }

        // Add blank note to grace block
        if self
            .mutate_doc(|doc| {
                if crate::yaml_editor::append_blank_note_to_grace_block_in_doc(doc, e_idx) {
                    Some(())
                } else {
                    None
                }
            })
            .is_some()
        {
            return Some(e_idx + 1);
        }
        None
    }
}
