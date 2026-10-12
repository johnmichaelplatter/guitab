use super::*;
use std::collections::HashMap;

impl MnxEditorApp {
    pub fn extract_selected_items(
        &self,
    ) -> Option<(
        Vec<crate::mnx::SequenceItem>,
        Option<crate::mnx::TimeSignature>,
        Vec<crate::mnx::Arpeggio>,
        Vec<crate::mnx::Tempo>,
        Vec<crate::mnx::TempoChange>,
    )> {
        let cursor = self.selected_event_idx?;
        let anchor = self.selection_anchor.unwrap_or(cursor);
        let min_idx = cursor.min(anchor);
        let max_idx = cursor.max(anchor);

        let doc = self.parsed_mnx.as_ref()?;
        let part = doc.parts.first()?;

        let mut items = Vec::new();
        let mut cur_e = 0;
        let mut captured_ts = None;
        let mut copied_tempos = Vec::new();
        let mut copied_tcs = Vec::new();

        let parse_loc = |loc: &Option<String>| -> f64 {
            if let Some(s) = loc {
                if let Some((n, d)) = s.split_once('/') {
                    if let (Ok(num), Ok(den)) = (n.parse::<f64>(), d.parse::<f64>()) {
                        if den > 0.0 { return (num / den) * 4.0; }
                    }
                }
            }
            0.0
        };

        let mut global_measure_beat = 0.0;
        for (m_idx, m) in part.measures.iter().enumerate() {
            let measure_start_e = cur_e;
            let mut measure_beats = 0.0;
            for seq in &m.sequences {
                let mut current_beat = 0.0;
                for item in &seq.content {
                    let item_ev_count = match item {
                        crate::mnx::SequenceItem::Event(_) => 1,
                        crate::mnx::SequenceItem::Tuplet(t) => t.content.len(),
                        crate::mnx::SequenceItem::Grace(g) => g.content.len(),
                    };
                    
                    let item_beats = item.total_beats();

                    let item_start = cur_e;
                    let item_end = cur_e + item_ev_count - 1;

                    if let crate::mnx::SequenceItem::Grace(g) = item {
                        let mut selected_sub = Vec::new();
                        for (k, sub) in g.content.iter().enumerate() {
                            let sub_idx = cur_e + k;
                            if sub_idx >= min_idx && sub_idx <= max_idx {
                                selected_sub.push(sub.clone());
                            }
                        }
                        if !selected_sub.is_empty() {
                            items.push(crate::mnx::SequenceItem::Grace(crate::mnx::Grace {
                                item_type: g.item_type.clone(),
                                content: selected_sub,
                            }));
                        }
                    } else if item_start <= max_idx && item_end >= min_idx {
                        let is_first = items.is_empty();
                        if is_first && item_start == measure_start_e {
                            if let Some(gm) = doc.global.measures.get(m_idx) {
                                captured_ts = gm.time.clone();
                            }
                        }
                        items.push(item.clone());

                        if let Some(gm) = doc.global.measures.get(m_idx) {
                            for tempo in &gm.tempos {
                                let offset_beats = parse_loc(&tempo.location);
                                if offset_beats >= current_beat - 0.001 && offset_beats < current_beat + item_beats - 0.001 {
                                    copied_tempos.push((tempo.clone(), global_measure_beat + offset_beats));
                                }
                            }
                            for tc in &gm.tempo_changes {
                                let offset_beats = parse_loc(&tc.location);
                                if offset_beats >= current_beat - 0.001 && offset_beats < current_beat + item_beats - 0.001 {
                                    copied_tcs.push((tc.clone(), global_measure_beat + offset_beats));
                                }
                            }
                        }
                    }

                    current_beat += item_beats;
                    cur_e += item_ev_count;
                }
                measure_beats = f64::max(measure_beats, current_beat);
            }
            global_measure_beat += measure_beats;
        }

        let mut all_selected_note_ids = std::collections::HashSet::new();
        fn extract_note_ids(
            item: &crate::mnx::SequenceItem,
            ids: &mut std::collections::HashSet<String>,
        ) {
            match item {
                crate::mnx::SequenceItem::Event(ev) => {
                    for n in &ev.notes {
                        if let Some(id) = &n.id {
                            ids.insert(id.clone());
                        }
                    }
                }
                crate::mnx::SequenceItem::Tuplet(t) => {
                    for i in &t.content {
                        extract_note_ids(i, ids);
                    }
                }
                crate::mnx::SequenceItem::Grace(g) => {
                    for i in &g.content {
                        extract_note_ids(i, ids);
                    }
                }
            }
        }
        for item in &items {
            extract_note_ids(item, &mut all_selected_note_ids);
        }

        let mut copied_arpeggios = Vec::new();
        for m in &part.measures {
            if let Some(arps) = &m.arpeggios {
                for arp in arps {
                    if all_selected_note_ids.contains(&arp.span.start)
                        && all_selected_note_ids.contains(&arp.span.end)
                    {
                        copied_arpeggios.push(arp.clone());
                    }
                }
            }
        }

        // Calculate start beat of selection to make locations relative
        let mut selection_start_beat = 0.0;
        let mut cur_beat = 0.0;
        let mut cur_e2 = 0;
        'outer: for m in &part.measures {
            for seq in &m.sequences {
                for item in &seq.content {
                    let item_ev_count = match item {
                        crate::mnx::SequenceItem::Event(_) => 1,
                        crate::mnx::SequenceItem::Tuplet(t) => t.content.len(),
                        crate::mnx::SequenceItem::Grace(g) => g.content.len(),
                    };
                    if cur_e2 == min_idx {
                        selection_start_beat = cur_beat;
                        break 'outer;
                    }
                    cur_beat += item.total_beats();
                    cur_e2 += item_ev_count;
                }
            }
        }
        
        let mut final_copied_tempos: Vec<crate::mnx::Tempo> = copied_tempos.into_iter().map(|(mut t, abs_beat)| {
            let rel = abs_beat - selection_start_beat;
            t.location = Some(format!("{}/16", (rel * 4.0).round() as i32));
            t
        }).collect();
        
        let mut final_copied_tcs: Vec<crate::mnx::TempoChange> = copied_tcs.into_iter().map(|(mut tc, abs_beat)| {
            let rel = abs_beat - selection_start_beat;
            tc.location = Some(format!("{}/16", (rel * 4.0).round() as i32));
            tc
        }).collect();

        // Enforce constraint on tempo and tempoChange pairing
        final_copied_tcs.retain(|tc| {
            if let Some(t_id) = &tc.target {
                final_copied_tempos.iter().any(|t| t.id.as_ref() == Some(t_id))
            } else {
                false
            }
        });

        let mut all_tcs_in_doc = Vec::new();
        for gm in &doc.global.measures {
            all_tcs_in_doc.extend(gm.tempo_changes.clone());
        }

        final_copied_tempos.retain(|t| {
            if let Some(t_id) = &t.id {
                let is_targeted = all_tcs_in_doc.iter().any(|tc| tc.target.as_ref() == Some(t_id));
                if is_targeted {
                    // It must be in final_copied_tcs
                    final_copied_tcs.iter().any(|tc| tc.target.as_ref() == Some(t_id))
                } else {
                    true
                }
            } else {
                true
            }
        });

        if items.is_empty() {
            None
        } else {
            Some((items, captured_ts, copied_arpeggios, final_copied_tempos, final_copied_tcs))
        }
    }

    pub fn action_copy(&mut self) {
        if let Some((items, ts, arps, tempos, tempo_changes)) = self.extract_selected_items() {
            self.clipboard = Some(ClipboardData {
                items,
                time_signature: ts,
                arpeggios: arps,
                tempos,
                tempo_changes,
            });
        }
    }

    pub fn action_delete_overwrite(&mut self) {
        let cursor = match self.selected_event_idx {
            Some(c) => c,
            None => return,
        };
        let anchor = self.selection_anchor.unwrap_or(cursor);
        let min_idx = cursor.min(anchor);
        let max_idx = cursor.max(anchor);

        let mut doc = match self.parsed_mnx.as_ref() {
            Some(d) => d.clone(),
            None => return,
        };
        let part = match doc.parts.first_mut() {
            Some(p) => p,
            None => return,
        };

        // Check if all selected events are empty (rests / blanks)
        let mut all_empty = true;
        let mut cur_e = 0;
        for m in &part.measures {
            for seq in &m.sequences {
                for item in &seq.content {
                    let item_ev_count = match item {
                        crate::mnx::SequenceItem::Event(_) => 1,
                        crate::mnx::SequenceItem::Tuplet(t) => t.content.len(),
                        crate::mnx::SequenceItem::Grace(g) => g.content.len(),
                    };
                    let item_start = cur_e;
                    let item_end = cur_e + item_ev_count - 1;

                    if item_start <= max_idx && item_end >= min_idx {
                        match item {
                            crate::mnx::SequenceItem::Event(ev) => {
                                if !ev.notes.is_empty() {
                                    all_empty = false;
                                }
                            }
                            crate::mnx::SequenceItem::Tuplet(t) => {
                                for sub_ev in &t.content {
                                    if let crate::mnx::SequenceItem::Event(sev) = sub_ev {
                                        if !sev.notes.is_empty() {
                                            all_empty = false;
                                        }
                                    }
                                }
                            }
                            crate::mnx::SequenceItem::Grace(g) => {
                                for (k, sub_ev) in g.content.iter().enumerate() {
                                    let sub_idx = cur_e + k;
                                    if sub_idx >= min_idx && sub_idx <= max_idx {
                                        if let crate::mnx::SequenceItem::Event(sev) = sub_ev {
                                            if !sev.notes.is_empty() {
                                                all_empty = false;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    cur_e += item_ev_count;
                }
            }
        }

        if all_empty {
            // Cut Time equivalent without clipboard
            let mut flat_items = Vec::new();
            let mut cur_e = 0;

            for m in &part.measures {
                for seq in &m.sequences {
                    for item in &seq.content {
                        let item_ev_count = match item {
                            crate::mnx::SequenceItem::Event(_) => 1,
                            crate::mnx::SequenceItem::Tuplet(t) => t.content.len(),
                        crate::mnx::SequenceItem::Grace(g) => g.content.len(),
                        };
                        let item_start = cur_e;
                        let item_end = cur_e + item_ev_count - 1;

                        if item_start <= max_idx && item_end >= min_idx {
                            if let crate::mnx::SequenceItem::Grace(g) = item {
                                let mut remaining = Vec::new();
                                for (k, sub) in g.content.iter().enumerate() {
                                    let sub_idx = cur_e + k;
                                    if sub_idx < min_idx || sub_idx > max_idx {
                                        remaining.push(sub.clone());
                                    }
                                }
                                if !remaining.is_empty() {
                                    flat_items.push(crate::mnx::SequenceItem::Grace(crate::mnx::Grace {
                                        item_type: g.item_type.clone(),
                                        content: remaining,
                                    }));
                                }
                            }
                        } else {
                            flat_items.push(item.clone());
                        }
                        cur_e += item_ev_count;
                    }
                }
            }

            part.measures.clear();
            part.measures.push(crate::mnx::Measure {
                beams: None,
                arpeggios: None,
                sequences: vec![crate::mnx::Sequence {
                    content: flat_items,
                    beams: None,
                }],
            });
            crate::rebar::rebar_part(part, &mut doc.global.measures);

            if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
                self.update_yaml(new_yaml);
                self.process_and_render();
                self.selection_anchor = None;
                self.selected_event_idx = Some(min_idx);
            }
            return;
        }

        let mut cur_e = 0;
        for m in &mut part.measures {
            for seq in &mut m.sequences {
                let mut new_content = Vec::new();
                for item in seq.content.drain(..) {
                    let item_ev_count = match &item {
                        crate::mnx::SequenceItem::Event(_) => 1,
                        crate::mnx::SequenceItem::Tuplet(t) => t.content.len(),
                        crate::mnx::SequenceItem::Grace(g) => g.content.len(),
                    };
                    let item_start = cur_e;
                    let item_end = cur_e + item_ev_count - 1;

                    if item_start <= max_idx && item_end >= min_idx {
                        match item {
                            crate::mnx::SequenceItem::Event(mut ev) => {
                                ev.notes.clear();
                                ev.slurs = None;
                                new_content.push(crate::mnx::SequenceItem::Event(ev));
                            }
                            crate::mnx::SequenceItem::Tuplet(mut t) => {
                                for sub_ev in &mut t.content {
                                    if let crate::mnx::SequenceItem::Event(sev) = sub_ev {
                                        sev.notes.clear();
                                        sev.slurs = None;
                                    }
                                }
                                new_content.push(crate::mnx::SequenceItem::Tuplet(t));
                            }
                            crate::mnx::SequenceItem::Grace(g) => {
                                let mut remaining = Vec::new();
                                for (k, sub_ev) in g.content.into_iter().enumerate() {
                                    let sub_idx = cur_e + k;
                                    if sub_idx < min_idx || sub_idx > max_idx {
                                        remaining.push(sub_ev);
                                    }
                                }
                                if !remaining.is_empty() {
                                    new_content.push(crate::mnx::SequenceItem::Grace(crate::mnx::Grace {
                                        item_type: g.item_type,
                                        content: remaining,
                                    }));
                                }
                            }
                        }
                    } else {
                        new_content.push(item);
                    }
                    cur_e += item_ev_count;
                }
                seq.content = new_content;
            }
        }

        if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
            self.update_yaml(new_yaml);
            self.process_and_render();
            self.selection_anchor = None;
            self.selected_event_idx = Some(min_idx);
        }
    }

    pub(crate) fn rebuild_part_with_rebar(
        part: &mut crate::mnx::Part,
        global_measures: &mut Vec<crate::mnx::GlobalMeasure>,
        flat_items: Vec<crate::mnx::SequenceItem>,
        arpeggios_to_add: Vec<crate::mnx::Arpeggio>,
    ) {
        let mut all_arpeggios = Vec::new();
        for m in &part.measures {
            if let Some(arps) = &m.arpeggios {
                all_arpeggios.extend(arps.clone());
            }
        }
        all_arpeggios.extend(arpeggios_to_add);

        part.measures.clear();
        part.measures.push(crate::mnx::Measure {
            beams: None,
            arpeggios: None,
            sequences: vec![crate::mnx::Sequence {
                content: flat_items,
                beams: None,
            }],
        });
        crate::rebar::rebar_part(part, global_measures);

        let mut note_to_measure = std::collections::HashMap::new();
        for (m_idx, m) in part.measures.iter().enumerate() {
            fn map_notes(
                items: &[crate::mnx::SequenceItem],
                m_idx: usize,
                map: &mut std::collections::HashMap<String, usize>,
            ) {
                for item in items {
                    match item {
                        crate::mnx::SequenceItem::Event(ev) => {
                            for n in &ev.notes {
                                if let Some(id) = &n.id {
                                    map.insert(id.clone(), m_idx);
                                }
                            }
                        }
                        crate::mnx::SequenceItem::Tuplet(t) => {
                            map_notes(&t.content, m_idx, map);
                        }
                        crate::mnx::SequenceItem::Grace(g) => {
                            map_notes(&g.content, m_idx, map);
                        }
                    }
                }
            }
            for seq in &m.sequences {
                map_notes(&seq.content, m_idx, &mut note_to_measure);
            }
        }

        for arp in all_arpeggios {
            if let Some(&m_idx) = note_to_measure.get(&arp.span.start) {
                if note_to_measure.contains_key(&arp.span.end) {
                    let m = &mut part.measures[m_idx];
                    if m.arpeggios.is_none() {
                        m.arpeggios = Some(Vec::new());
                    }
                    m.arpeggios.as_mut().unwrap().push(arp);
                }
            }
        }
    }

    pub fn action_cut_overwrite(&mut self) {
        self.action_copy();
        let cb_after = self
            .clipboard
            .as_ref()
            .map(|cb| cb.items.len())
            .unwrap_or(0);
        println!("action_copy finished. cb_after: {}", cb_after);

        let cursor = match self.selected_event_idx {
            Some(c) => c,
            None => return,
        };
        let anchor = self.selection_anchor.unwrap_or(cursor);
        let min_idx = cursor.min(anchor);
        let max_idx = cursor.max(anchor);

        let mut doc = match self.parsed_mnx.as_ref() {
            Some(d) => d.clone(),
            None => return,
        };
        let part = match doc.parts.first_mut() {
            Some(p) => p,
            None => return,
        };

        let mut cur_e = 0;
        for m in &mut part.measures {
            for seq in &mut m.sequences {
                let mut new_content = Vec::new();
                for item in seq.content.drain(..) {
                    let item_ev_count = match &item {
                        crate::mnx::SequenceItem::Event(_) => 1,
                        crate::mnx::SequenceItem::Tuplet(t) => t.content.len(),
                        crate::mnx::SequenceItem::Grace(g) => g.content.len(),
                    };
                    let item_start = cur_e;
                    let item_end = cur_e + item_ev_count - 1;

                    if item_start <= max_idx && item_end >= min_idx {
                        match item {
                            crate::mnx::SequenceItem::Event(mut ev) => {
                                ev.notes.clear();
                                ev.slurs = None;
                                new_content.push(crate::mnx::SequenceItem::Event(ev));
                            }
                            crate::mnx::SequenceItem::Tuplet(mut t) => {
                                for sub_ev in &mut t.content {
                                    if let crate::mnx::SequenceItem::Event(sev) = sub_ev {
                                        sev.notes.clear();
                                        sev.slurs = None;
                                    }
                                }
                                new_content.push(crate::mnx::SequenceItem::Tuplet(t));
                            }
                            crate::mnx::SequenceItem::Grace(g) => {
                                let mut remaining = Vec::new();
                                for (k, sub_ev) in g.content.into_iter().enumerate() {
                                    let sub_idx = cur_e + k;
                                    if sub_idx < min_idx || sub_idx > max_idx {
                                        remaining.push(sub_ev);
                                    }
                                }
                                if !remaining.is_empty() {
                                    new_content.push(crate::mnx::SequenceItem::Grace(crate::mnx::Grace {
                                        item_type: g.item_type,
                                        content: remaining,
                                    }));
                                }
                            }
                        }
                    } else {
                        new_content.push(item);
                    }
                    cur_e += item_ev_count;
                }
                seq.content = new_content;
            }
        }

        if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
            self.update_yaml(new_yaml);
            self.process_and_render();
            self.process_and_render();
        }
    }

    pub fn action_cut_time(&mut self) {
        self.action_copy();
        let cb_after = self
            .clipboard
            .as_ref()
            .map(|cb| cb.items.len())
            .unwrap_or(0);
        println!("action_copy finished. cb_after: {}", cb_after);

        let cursor = match self.selected_event_idx {
            Some(c) => c,
            None => return,
        };
        let anchor = self.selection_anchor.unwrap_or(cursor);
        let min_idx = cursor.min(anchor);
        let max_idx = cursor.max(anchor);

        let mut doc = match self.parsed_mnx.as_ref() {
            Some(d) => d.clone(),
            None => return,
        };
        let part = match doc.parts.first_mut() {
            Some(p) => p,
            None => return,
        };

        let mut flat_items = Vec::new();
        let mut cur_e = 0;

        for m in &part.measures {
            for seq in &m.sequences {
                for item in &seq.content {
                    let item_ev_count = match item {
                        crate::mnx::SequenceItem::Event(_) => 1,
                        crate::mnx::SequenceItem::Tuplet(t) => t.content.len(),
                        crate::mnx::SequenceItem::Grace(g) => g.content.len(),
                    };
                    let item_start = cur_e;
                    let item_end = cur_e + item_ev_count - 1;

                    if item_start <= max_idx && item_end >= min_idx {
                        if let crate::mnx::SequenceItem::Grace(g) = item {
                            let mut remaining = Vec::new();
                            for (k, sub) in g.content.iter().enumerate() {
                                let sub_idx = cur_e + k;
                                if sub_idx < min_idx || sub_idx > max_idx {
                                    remaining.push(sub.clone());
                                }
                            }
                            if !remaining.is_empty() {
                                flat_items.push(crate::mnx::SequenceItem::Grace(crate::mnx::Grace {
                                    item_type: g.item_type.clone(),
                                    content: remaining,
                                }));
                            }
                        }
                    } else {
                        flat_items.push(item.clone());
                    }
                    cur_e += item_ev_count;
                }
            }
        }

        Self::rebuild_part_with_rebar(part, &mut doc.global.measures, flat_items, vec![]);

        if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
            self.update_yaml(new_yaml);
            self.process_and_render();
            self.process_and_render();
            self.selected_event_idx = Some(min_idx);
            self.selection_anchor = None;
        }
    }

    fn distribute_tempos(
        global_measures: &mut Vec<crate::mnx::GlobalMeasure>,
        part: &crate::mnx::Part,
        pasted_tempos: Vec<(crate::mnx::Tempo, f64)>,
        pasted_tcs: Vec<(crate::mnx::TempoChange, f64)>,
    ) {
        // Calculate measure boundaries
        let mut measure_start_beats = Vec::new();
        let mut cur_beat = 0.0;
        for m in &part.measures {
            measure_start_beats.push(cur_beat);
            let mut measure_beats = 0.0;
            for seq in &m.sequences {
                let mut seq_beats = 0.0;
                for item in &seq.content {
                    seq_beats += item.total_beats();
                }
                measure_beats = f64::max(measure_beats, seq_beats);
            }
            cur_beat += measure_beats;
        }

        // Expand global_measures if needed
        while global_measures.len() < part.measures.len() {
            let last_ts = global_measures.last().and_then(|g| g.time.clone()).unwrap_or(crate::mnx::TimeSignature { count: 4, unit: 4 });
            global_measures.push(crate::mnx::GlobalMeasure {
                time: Some(last_ts),
                repeat_start: None,
                repeat_end: None,
                barline: None,
                ending: None,
                tempos: Vec::new(),
                tempo_changes: Vec::new(),
            });
        }

        for (mut t, abs_beat) in pasted_tempos {
            // Find which measure it belongs to
            let mut m_idx = 0;
            for (i, &start) in measure_start_beats.iter().enumerate() {
                if abs_beat >= start - 0.001 {
                    m_idx = i;
                } else {
                    break;
                }
            }
            let measure_start = measure_start_beats[m_idx];
            let rel = abs_beat - measure_start;
            t.location = Some(format!("{}/16", (rel * 4.0).round() as i32));
            global_measures[m_idx].tempos.push(t);
        }

        for (mut tc, abs_beat) in pasted_tcs {
            let mut m_idx = 0;
            for (i, &start) in measure_start_beats.iter().enumerate() {
                if abs_beat >= start - 0.001 {
                    m_idx = i;
                } else {
                    break;
                }
            }
            let measure_start = measure_start_beats[m_idx];
            let rel = abs_beat - measure_start;
            tc.location = Some(format!("{}/16", (rel * 4.0).round() as i32));
            global_measures[m_idx].tempo_changes.push(tc);
        }
    }

    pub fn prepare_pasted_items(
        items: &mut [crate::mnx::SequenceItem],
        arps: &mut [crate::mnx::Arpeggio],
        tempos: &mut [crate::mnx::Tempo],
        tcs: &mut [crate::mnx::TempoChange],
    ) {
        let mut id_map = HashMap::new();

        // Pass 1: Generate new IDs for everything and build a map
        let _collect_ids = |items: &mut [crate::mnx::SequenceItem]| {
            let mut stack = vec![items];
            while let Some(slice) = stack.pop() {
                for item in slice {
                    match item {
                        crate::mnx::SequenceItem::Event(ev) => {
                            if let Some(old_ev_id) = &ev.id {
                                let new_ev_id = crate::rebar::generate_uuid();
                                id_map.insert(old_ev_id.clone(), new_ev_id.clone());
                                ev.id = Some(new_ev_id);
                            } else {
                                ev.id = Some(crate::rebar::generate_uuid());
                            }

                            for n in ev.notes.iter_mut() {
                                if let Some(old_id) = &n.id {
                                    let new_id = crate::rebar::generate_uuid();
                                    id_map.insert(old_id.clone(), new_id.clone());
                                    n.id = Some(new_id);
                                } else {
                                    n.id = Some(crate::rebar::generate_uuid());
                                }
                            }
                        }
                        crate::mnx::SequenceItem::Tuplet(t) => {
                            stack.push(&mut t.content);
                        }
                        crate::mnx::SequenceItem::Grace(g) => {
                            stack.push(&mut g.content);
                        }
                    }
                }
            }
        };

        // We have to clone to pass to collect_ids since we can't borrow mutably twice
        // Actually we can just write a recursive fn
        fn map_ids(items: &mut [crate::mnx::SequenceItem], id_map: &mut HashMap<String, String>) {
            for item in items.iter_mut() {
                match item {
                    crate::mnx::SequenceItem::Event(ev) => {
                        if let Some(old_ev_id) = &ev.id {
                            let new_ev_id = crate::rebar::generate_uuid();
                            id_map.insert(old_ev_id.clone(), new_ev_id.clone());
                            ev.id = Some(new_ev_id);
                        } else {
                            ev.id = Some(crate::rebar::generate_uuid());
                        }
                        for n in ev.notes.iter_mut() {
                            if let Some(old_id) = &n.id {
                                let new_id = crate::rebar::generate_uuid();
                                id_map.insert(old_id.clone(), new_id.clone());
                                n.id = Some(new_id);
                            } else {
                                n.id = Some(crate::rebar::generate_uuid());
                            }
                        }
                    }
                    crate::mnx::SequenceItem::Tuplet(t) => {
                        map_ids(&mut t.content, id_map);
                    }
                    crate::mnx::SequenceItem::Grace(g) => {
                        map_ids(&mut g.content, id_map);
                    }
                }
            }
        }
        map_ids(items, &mut id_map);

        // Pass 2: Update ties and slurs if they point within the map, otherwise remove them
        fn update_refs(items: &mut [crate::mnx::SequenceItem], id_map: &HashMap<String, String>) {
            for item in items.iter_mut() {
                match item {
                    crate::mnx::SequenceItem::Event(ev) => {
                        if let Some(slurs) = &mut ev.slurs {
                            slurs.retain_mut(|s| {
                                if let Some(new_target) = id_map.get(&s.target) {
                                    s.target = new_target.clone();
                                    if let Some(old_start) = &s.start_note {
                                        if let Some(new_start) = id_map.get(old_start) {
                                            s.start_note = Some(new_start.clone());
                                        }
                                    }
                                    if let Some(old_end) = &s.end_note {
                                        if let Some(new_end) = id_map.get(old_end) {
                                            s.end_note = Some(new_end.clone());
                                        }
                                    }
                                    true
                                } else {
                                    false
                                }
                            });
                            if slurs.is_empty() {
                                ev.slurs = None;
                            }
                        }
                        for n in ev.notes.iter_mut() {
                            if let Some(ties) = &mut n.ties {
                                ties.retain_mut(|t| {
                                    if let Some(new_target) = id_map.get(&t.target) {
                                        t.target = new_target.clone();
                                        true
                                    } else {
                                        false
                                    }
                                });
                                if ties.is_empty() {
                                    n.ties = None;
                                }
                            }
                        }
                    }
                    crate::mnx::SequenceItem::Tuplet(t) => {
                        update_refs(&mut t.content, id_map);
                    }
                    crate::mnx::SequenceItem::Grace(g) => {
                        update_refs(&mut g.content, id_map);
                    }
                }
            }
        }
        update_refs(items, &id_map);
        for arp in arps {
            if let Some(new_id) = id_map.get(&arp.span.start) {
                arp.span.start = new_id.clone();
            }
            if let Some(new_id) = id_map.get(&arp.span.end) {
                arp.span.end = new_id.clone();
            }
        }

        // Map tempo IDs
        let mut tempo_id_map = HashMap::new();
        for t in tempos.iter_mut() {
            if let Some(old_id) = &t.id {
                let new_id = crate::rebar::generate_uuid();
                tempo_id_map.insert(old_id.clone(), new_id.clone());
                t.id = Some(new_id);
            }
        }
        for tc in tcs.iter_mut() {
            if let Some(old_target) = &tc.target {
                if let Some(new_target) = tempo_id_map.get(old_target) {
                    tc.target = Some(new_target.clone());
                }
            }
        }
    }

    pub fn action_paste_insert(&mut self) {
        let cursor = match self.selected_event_idx {
            Some(c) => c,
            None => return,
        };
        let cb = match &self.clipboard {
            Some(c) => c,
            None => return,
        };
        let mut doc = match self.parsed_mnx.as_ref() {
            Some(d) => d.clone(),
            None => return,
        };
        let part = match doc.parts.first_mut() {
            Some(p) => p,
            None => return,
        };

        let mut flat_items = Vec::new();
        let mut all_pasted_arps: Vec<crate::mnx::Arpeggio> = Vec::new();
        let mut cur_e = 0;
        let mut inserted = false;
        
        let mut dest_beat = 0.0;
        let mut found_dest_beat = false;
        let mut cur_beat = 0.0;

        let mut pasted_tempos = cb.tempos.clone();
        let mut pasted_tcs = cb.tempo_changes.clone();
        let mut pasted_items = cb.items.clone();
        let mut pasted_arps = cb.arpeggios.clone();
        Self::prepare_pasted_items(&mut pasted_items, &mut pasted_arps, &mut pasted_tempos, &mut pasted_tcs);

        for (m_idx, m) in part.measures.iter().enumerate() {
            let measure_start_e = cur_e;
            let mut measure_beats = 0.0;
            for seq in &m.sequences {
                let mut seq_beat = 0.0;
                for item in &seq.content {
                    if cur_e == cursor && !found_dest_beat {
                        dest_beat = cur_beat + seq_beat;
                        found_dest_beat = true;
                    }
                    if cur_e == cursor {
                        if cur_e == measure_start_e {
                            if let Some(ts) = &cb.time_signature {
                                if let Some(gm) = doc.global.measures.get_mut(m_idx) {
                                    gm.time = Some(ts.clone());
                                }
                            }
                        }

                        flat_items.extend(pasted_items.clone());
                        all_pasted_arps.extend(pasted_arps.clone());
                        inserted = true;
                    }
                    flat_items.push(item.clone());
                    let item_ev_count = match item {
                        crate::mnx::SequenceItem::Event(_) => 1,
                        crate::mnx::SequenceItem::Tuplet(t) => t.content.len(),
                        crate::mnx::SequenceItem::Grace(g) => g.content.len(),
                    };
                    cur_e += item_ev_count;
                    seq_beat += item.total_beats();
                }
                measure_beats = f64::max(measure_beats, seq_beat);
            }
            cur_beat += measure_beats;
        }

        if !inserted {
            dest_beat = cur_beat;
            flat_items.extend(pasted_items.clone());
            all_pasted_arps.extend(pasted_arps.clone());
        }

        Self::rebuild_part_with_rebar(part, &mut doc.global.measures, flat_items, all_pasted_arps);
        
        let parse_loc = |loc: &Option<String>| -> f64 {
            if let Some(s) = loc {
                if let Some((n, d)) = s.split_once('/') {
                    if let (Ok(num), Ok(den)) = (n.parse::<f64>(), d.parse::<f64>()) {
                        if den > 0.0 { return (num / den) * 4.0; }
                    }
                }
            }
            0.0
        };

        let pasted_tempos_abs: Vec<_> = pasted_tempos.into_iter().map(|t| {
            let rel = parse_loc(&t.location);
            (t, dest_beat + rel)
        }).collect();
        let pasted_tcs_abs: Vec<_> = pasted_tcs.into_iter().map(|tc| {
            let rel = parse_loc(&tc.location);
            (tc, dest_beat + rel)
        }).collect();

        Self::distribute_tempos(&mut doc.global.measures, part, pasted_tempos_abs, pasted_tcs_abs);

        let cb_count = self.clipboard_event_count();
        if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
            self.update_yaml(new_yaml);
            self.process_and_render();
            self.process_and_render();

            if cb_count > 0 {
                self.selection_anchor = Some(cursor);
                self.selected_event_idx = Some(cursor + cb_count - 1);
            }
        }
    }

    fn clipboard_event_count(&self) -> usize {
        let mut count = 0;
        if let Some(cb) = &self.clipboard {
            for item in &cb.items {
                match item {
                    crate::mnx::SequenceItem::Event(_) => count += 1,
                    crate::mnx::SequenceItem::Tuplet(t) => count += t.content.len(),
                    crate::mnx::SequenceItem::Grace(g) => count += g.content.len(),
                }
            }
        }
        count
    }

    pub fn action_paste_overwrite(&mut self) {
        println!("action_paste_overwrite CALLED! cursor: {:?}, cb_is_some: {}", self.selected_event_idx, self.clipboard.is_some());
        let cursor = match self.selected_event_idx {
            Some(c) => c,
            None => return,
        };
        let cb = match &self.clipboard {
            Some(c) => c,
            None => return,
        };
        let mut doc = match self.parsed_mnx.as_ref() {
            Some(d) => d.clone(),
            None => return,
        };
        let part = match doc.parts.first_mut() {
            Some(p) => p,
            None => return,
        };

        let mut pasted_tempos = cb.tempos.clone();
        let mut pasted_tcs = cb.tempo_changes.clone();
        let mut cb_items = cb.items.clone();
        Self::prepare_pasted_items(&mut cb_items, &mut [], &mut pasted_tempos, &mut pasted_tcs);
        let mut cb_beats = 0.0;
        for item in &cb_items {
            let beats = match item {
                crate::mnx::SequenceItem::Event(ev) => {
                    crate::validate::duration_to_quarter_beats(&ev.duration)
                }
                crate::mnx::SequenceItem::Tuplet(t) => {
                    crate::validate::duration_to_quarter_beats(&t.outer.duration)
                        * (t.outer.multiple as f64)
                }
                crate::mnx::SequenceItem::Grace(_) => 0.0,
            };
            cb_beats += beats;
        }

        let mut flat_items = Vec::new();
        let mut all_pasted_arps: Vec<crate::mnx::Arpeggio> = Vec::new();
        let mut cur_e = 0;
        let mut absorbed_beats = 0.0;
        let mut inserted = false;
        
        let mut dest_beat = 0.0;
        let mut cur_beat = 0.0;
        let mut found_dest_beat = false;
        let mut pasted_arps = cb.arpeggios.clone();
        Self::prepare_pasted_items(&mut [], &mut pasted_arps, &mut [], &mut []);

        for (m_idx, m) in part.measures.iter().enumerate() {
            let measure_start_e = cur_e;
            let mut measure_beats = 0.0;
            for seq in &m.sequences {
                let mut seq_beat = 0.0;
                for item in &seq.content {
                    if cur_e == cursor && !found_dest_beat {
                        dest_beat = cur_beat + seq_beat;
                        found_dest_beat = true;
                    }
                    if cur_e == cursor && !inserted {
                        if cur_e == measure_start_e {
                            if let Some(ts) = &cb.time_signature {
                                if let Some(gm) = doc.global.measures.get_mut(m_idx) {
                                    gm.time = Some(ts.clone());
                                }
                            }
                        }

                        flat_items.extend(cb_items.clone());
                        all_pasted_arps.extend(pasted_arps.clone());
                        inserted = true;
                    }

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

                    let item_ev_count = match item {
                        crate::mnx::SequenceItem::Event(_) => 1,
                        crate::mnx::SequenceItem::Tuplet(t) => t.content.len(),
                        crate::mnx::SequenceItem::Grace(g) => g.content.len(),
                    };

                    if inserted && absorbed_beats < cb_beats - 0.001 {
                        absorbed_beats += item_beats;
                        if absorbed_beats > cb_beats + 0.001 {
                            let excess = absorbed_beats - cb_beats;
                            match item {
                                crate::mnx::SequenceItem::Event(ev) => {
                                    let remainder_ev = ev.clone();
                                    let chunks = crate::rebar::greedy_split_event_duration(
                                        remainder_ev,
                                        excess,
                                    );
                                    for c in chunks {
                                        flat_items.push(crate::mnx::SequenceItem::Event(c));
                                    }
                                }
                                crate::mnx::SequenceItem::Tuplet(_t) => {
                                    flat_items.push(item.clone());
                                }
                                crate::mnx::SequenceItem::Grace(_g) => {
                                    flat_items.push(item.clone());
                                }
                            }
                        }
                    } else {
                        flat_items.push(item.clone());
                    }

                    cur_e += item_ev_count;
                    seq_beat += item_beats;
                }
                measure_beats = f64::max(measure_beats, seq_beat);
            }
            cur_beat += measure_beats;
        }

        if !inserted {
            dest_beat = cur_beat;
            flat_items.extend(cb_items.clone());
            all_pasted_arps.extend(pasted_arps.clone());
        }

        Self::rebuild_part_with_rebar(part, &mut doc.global.measures, flat_items, all_pasted_arps);
        
        let parse_loc = |loc: &Option<String>| -> f64 {
            if let Some(s) = loc {
                if let Some((n, d)) = s.split_once('/') {
                    if let (Ok(num), Ok(den)) = (n.parse::<f64>(), d.parse::<f64>()) {
                        if den > 0.0 { return (num / den) * 4.0; }
                    }
                }
            }
            0.0
        };

        let pasted_tempos_abs: Vec<_> = pasted_tempos.into_iter().map(|t| {
            let rel = parse_loc(&t.location);
            (t, dest_beat + rel)
        }).collect();
        let pasted_tcs_abs: Vec<_> = pasted_tcs.into_iter().map(|tc| {
            let rel = parse_loc(&tc.location);
            (tc, dest_beat + rel)
        }).collect();

        Self::distribute_tempos(&mut doc.global.measures, part, pasted_tempos_abs, pasted_tcs_abs);

        let cb_count = self.clipboard_event_count();
        if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
            self.update_yaml(new_yaml);
            self.process_and_render();
            self.process_and_render();

            if cb_count > 0 {
                self.selection_anchor = Some(cursor);
                self.selected_event_idx = Some(cursor + cb_count - 1);
            }
        }
    }

    pub fn action_insert_time(&mut self) {
        let cursor = match self.selected_event_idx {
            Some(c) => c,
            None => return,
        };
        let mut doc = match self.parsed_mnx.as_ref() {
            Some(d) => d.clone(),
            None => return,
        };
        let part = match doc.parts.first_mut() {
            Some(p) => p,
            None => return,
        };

        let mut flat_items = Vec::new();
        let mut cur_e = 0;
        let mut inserted = false;
        for m in &part.measures {
            for seq in &m.sequences {
                for item in &seq.content {
                    if cur_e == cursor && !inserted {
                        let rest_ev = crate::mnx::Event {
                            id: None,
                            duration: crate::mnx::Duration {
                                base: "whole".to_string(),
                                dots: None,
                                multiple: None,
                            },
                            notes: vec![],
                            rest: Some(crate::mnx::Rest {}),
                            slurs: None,
                            markings: None,
                        };
                        flat_items.push(crate::mnx::SequenceItem::Event(rest_ev));
                        inserted = true;
                    }
                    flat_items.push(item.clone());
                    let item_ev_count = match item {
                        crate::mnx::SequenceItem::Event(_) => 1,
                        crate::mnx::SequenceItem::Tuplet(t) => t.content.len(),
                        crate::mnx::SequenceItem::Grace(g) => g.content.len(),
                    };
                    cur_e += item_ev_count;
                }
            }
        }

        if !inserted {
            let rest_ev = crate::mnx::Event {
                id: None,
                duration: crate::mnx::Duration {
                    base: "whole".to_string(),
                    dots: None,
                    multiple: None,
                },
                notes: vec![],
                rest: Some(crate::mnx::Rest {}),
                slurs: None,
                markings: None,
            };
            flat_items.push(crate::mnx::SequenceItem::Event(rest_ev));
        }

        Self::rebuild_part_with_rebar(part, &mut doc.global.measures, flat_items, vec![]);

        let cb_count = self.clipboard_event_count();
        if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
            self.update_yaml(new_yaml);
            self.process_and_render();
            self.process_and_render();

            if cb_count > 0 {
                self.selection_anchor = Some(cursor);
                self.selected_event_idx = Some(cursor + cb_count - 1);
            }
        }
    }

    pub fn action_duplicate_overwrite(&mut self) {
        self.action_copy();
        let cb_after = self
            .clipboard
            .as_ref()
            .map(|cb| cb.items.len())
            .unwrap_or(0);
        println!("action_copy finished. cb_after: {}", cb_after);
        let max_idx = if let Some(cursor) = self.selected_event_idx {
            let anchor = self.selection_anchor.unwrap_or(cursor);
            cursor.max(anchor)
        } else {
            return;
        };
        self.selected_event_idx = Some(max_idx + 1);
        self.selection_anchor = None;
        self.action_paste_overwrite();
    }

    pub fn action_duplicate_insert(&mut self) {
        let cb_before = self
            .clipboard
            .as_ref()
            .map(|cb| cb.items.len())
            .unwrap_or(0);
        println!("action_duplicate_insert called. cb_before: {}", cb_before);
        self.action_copy();
        let cb_after = self
            .clipboard
            .as_ref()
            .map(|cb| cb.items.len())
            .unwrap_or(0);
        println!("action_copy finished. cb_after: {}", cb_after);
        let max_idx = if let Some(cursor) = self.selected_event_idx {
            let anchor = self.selection_anchor.unwrap_or(cursor);
            cursor.max(anchor)
        } else {
            return;
        };
        self.selected_event_idx = Some(max_idx + 1);
        self.selection_anchor = None;
        self.action_paste_insert();
    }
}
