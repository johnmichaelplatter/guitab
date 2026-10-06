use std::collections::HashMap;
use super::*;

impl MnxEditorApp {
    pub fn extract_selected_items(&self) -> Option<(Vec<crate::mnx::SequenceItem>, Option<crate::mnx::TimeSignature>, Vec<crate::mnx::Arpeggio>)> {
        let cursor = self.selected_event_idx?;
        let anchor = self.selection_anchor.unwrap_or(cursor);
        let min_idx = cursor.min(anchor);
        let max_idx = cursor.max(anchor);

        let doc = self.parsed_mnx.as_ref()?;
        let part = doc.parts.first()?;
        
        let mut items = Vec::new();
        let mut cur_e = 0;
        let mut captured_ts = None;

        for (m_idx, m) in part.measures.iter().enumerate() {
            let measure_start_e = cur_e;
            for seq in &m.sequences {
                for item in &seq.content {
                    let item_ev_count = match item {
                        crate::mnx::SequenceItem::Event(_) => 1,
                        crate::mnx::SequenceItem::Tuplet(t) => t.content.len(),
                    };
                    
                    let item_start = cur_e;
                    let item_end = cur_e + item_ev_count - 1;
                    
                    if item_start <= max_idx && item_end >= min_idx {
                        if items.is_empty() && item_start == measure_start_e {
                            if let Some(gm) = doc.global.measures.get(m_idx) {
                                captured_ts = gm.time.clone();
                            }
                        }
                        items.push(item.clone());
                    }
                    
                    cur_e += item_ev_count;
                }
            }
        }
        
        let mut all_selected_note_ids = std::collections::HashSet::new();
        fn extract_note_ids(item: &crate::mnx::SequenceItem, ids: &mut std::collections::HashSet<String>) {
            match item {
                crate::mnx::SequenceItem::Event(ev) => {
                    for n in &ev.notes { if let Some(id) = &n.id { ids.insert(id.clone()); } }
                },
                crate::mnx::SequenceItem::Tuplet(t) => {
                    for i in &t.content { extract_note_ids(i, ids); }
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
                    if all_selected_note_ids.contains(&arp.span.start) && all_selected_note_ids.contains(&arp.span.end) {
                        copied_arpeggios.push(arp.clone());
                    }
                }
            }
        }
        
        if items.is_empty() { None } else { Some((items, captured_ts, copied_arpeggios)) }
    }

    pub fn action_copy(&mut self) {
        if let Some((items, ts, arps)) = self.extract_selected_items() {
            self.clipboard = Some(ClipboardData {
                items,
                time_signature: ts,
                arpeggios: arps,
            });
        }
    }

    pub fn action_delete_overwrite(&mut self) {
        let cursor = match self.selected_event_idx { Some(c) => c, None => return };
        let anchor = self.selection_anchor.unwrap_or(cursor);
        let min_idx = cursor.min(anchor);
        let max_idx = cursor.max(anchor);
        
        let mut doc = match self.parsed_mnx.as_ref() { Some(d) => d.clone(), None => return };
        let part = match doc.parts.first_mut() { Some(p) => p, None => return };
        
        // Check if all selected events are empty (rests / blanks)
        let mut all_empty = true;
        let mut cur_e = 0;
        for m in &part.measures {
            for seq in &m.sequences {
                for item in &seq.content {
                    let item_ev_count = match item {
                        crate::mnx::SequenceItem::Event(_) => 1,
                        crate::mnx::SequenceItem::Tuplet(t) => t.content.len(),
                    };
                    let item_start = cur_e;
                    let item_end = cur_e + item_ev_count - 1;
                    
                    if item_start <= max_idx && item_end >= min_idx {
                        match item {
                            crate::mnx::SequenceItem::Event(ev) => {
                                if !ev.notes.is_empty() {
                                    all_empty = false;
                                }
                            },
                            crate::mnx::SequenceItem::Tuplet(t) => {
                                for sub_ev in &t.content {
                                    if let crate::mnx::SequenceItem::Event(sev) = sub_ev {
                                        if !sev.notes.is_empty() {
                                            all_empty = false;
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
                        };
                        let item_start = cur_e;
                        let item_end = cur_e + item_ev_count - 1;
                        
                        if !(item_start <= max_idx && item_end >= min_idx) {
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
            sequences: vec![crate::mnx::Sequence { content: flat_items, beams: None }]
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
                for item in &mut seq.content {
                    let item_ev_count = match item {
                        crate::mnx::SequenceItem::Event(_) => 1,
                        crate::mnx::SequenceItem::Tuplet(t) => t.content.len(),
                    };
                    let item_start = cur_e;
                    let item_end = cur_e + item_ev_count - 1;
                    
                    if item_start <= max_idx && item_end >= min_idx {
                        match item {
                            crate::mnx::SequenceItem::Event(ev) => {
                                ev.notes.clear();
                                ev.slurs = None;
                            },
                            crate::mnx::SequenceItem::Tuplet(t) => {
                                for sub_ev in &mut t.content {
                                    if let crate::mnx::SequenceItem::Event(sev) = sub_ev {
                                        sev.notes.clear();
                                        sev.slurs = None;
                                    }
                                }
                            }
                        }
                    }
                    cur_e += item_ev_count;
                }
            }
        }
        
        if let Ok(new_yaml) = serde_saphyr::to_string(&doc) {
            self.update_yaml(new_yaml);
            self.process_and_render();
            self.selection_anchor = None;
            self.selected_event_idx = Some(min_idx);
        }
    }

    pub(crate) fn rebuild_part_with_rebar(part: &mut crate::mnx::Part, global_measures: &mut Vec<crate::mnx::GlobalMeasure>, flat_items: Vec<crate::mnx::SequenceItem>, arpeggios_to_add: Vec<crate::mnx::Arpeggio>) {
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
            sequences: vec![crate::mnx::Sequence { content: flat_items, beams: None }]
        });
        crate::rebar::rebar_part(part, global_measures);

        let mut note_to_measure = std::collections::HashMap::new();
        for (m_idx, m) in part.measures.iter().enumerate() {
            fn map_notes(items: &[crate::mnx::SequenceItem], m_idx: usize, map: &mut std::collections::HashMap<String, usize>) {
                for item in items {
                    match item {
                        crate::mnx::SequenceItem::Event(ev) => {
                            for n in &ev.notes {
                                if let Some(id) = &n.id {
                                    map.insert(id.clone(), m_idx);
                                }
                            }
                        },
                        crate::mnx::SequenceItem::Tuplet(t) => {
                            map_notes(&t.content, m_idx, map);
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
                    if m.arpeggios.is_none() { m.arpeggios = Some(Vec::new()); }
                    m.arpeggios.as_mut().unwrap().push(arp);
                }
            }
        }
    }

    pub fn action_cut_overwrite(&mut self) {
        self.action_copy();
        let cb_after = self.clipboard.as_ref().map(|cb| cb.items.len()).unwrap_or(0);
        println!("action_copy finished. cb_after: {}", cb_after);
        
        let cursor = match self.selected_event_idx { Some(c) => c, None => return };
        let anchor = self.selection_anchor.unwrap_or(cursor);
        let min_idx = cursor.min(anchor);
        let max_idx = cursor.max(anchor);
        
        let mut doc = match self.parsed_mnx.as_ref() { Some(d) => d.clone(), None => return };
        let part = match doc.parts.first_mut() { Some(p) => p, None => return };
        
        let mut cur_e = 0;
        for m in &mut part.measures {
            for seq in &mut m.sequences {
                for item in &mut seq.content {
                    let item_ev_count = match item {
                        crate::mnx::SequenceItem::Event(_) => 1,
                        crate::mnx::SequenceItem::Tuplet(t) => t.content.len(),
                    };
                    let item_start = cur_e;
                    let item_end = cur_e + item_ev_count - 1;
                    
                    if item_start <= max_idx && item_end >= min_idx {
                        match item {
                            crate::mnx::SequenceItem::Event(ev) => {
                                ev.notes.clear();
                                ev.slurs = None;
                            },
                            crate::mnx::SequenceItem::Tuplet(t) => {
                                for sub_ev in &mut t.content {
                                    if let crate::mnx::SequenceItem::Event(sev) = sub_ev {
                                        sev.notes.clear();
                                        sev.slurs = None;
                                    }
                                }
                            }
                        }
                    }
                    cur_e += item_ev_count;
                }
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
        let cb_after = self.clipboard.as_ref().map(|cb| cb.items.len()).unwrap_or(0);
        println!("action_copy finished. cb_after: {}", cb_after);
        
        let cursor = match self.selected_event_idx { Some(c) => c, None => return };
        let anchor = self.selection_anchor.unwrap_or(cursor);
        let min_idx = cursor.min(anchor);
        let max_idx = cursor.max(anchor);
        
        let mut doc = match self.parsed_mnx.as_ref() { Some(d) => d.clone(), None => return };
        let part = match doc.parts.first_mut() { Some(p) => p, None => return };
        
        let mut flat_items = Vec::new();
        let mut cur_e = 0;
        
        for m in &part.measures {
            for seq in &m.sequences {
                for item in &seq.content {
                    let item_ev_count = match item {
                        crate::mnx::SequenceItem::Event(_) => 1,
                        crate::mnx::SequenceItem::Tuplet(t) => t.content.len(),
                    };
                    let item_start = cur_e;
                    let item_end = cur_e + item_ev_count - 1;
                    
                    if !(item_start <= max_idx && item_end >= min_idx) {
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

    pub fn prepare_pasted_items(items: &mut [crate::mnx::SequenceItem], arps: &mut [crate::mnx::Arpeggio]) {
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
                        },
                        crate::mnx::SequenceItem::Tuplet(t) => {
                            stack.push(&mut t.content);
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
                    },
                    crate::mnx::SequenceItem::Tuplet(t) => {
                        map_ids(&mut t.content, id_map);
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
                            if slurs.is_empty() { ev.slurs = None; }
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
                                if ties.is_empty() { n.ties = None; }
                            }
                        }
                    },
                    crate::mnx::SequenceItem::Tuplet(t) => {
                        update_refs(&mut t.content, id_map);
                    }
                }
            }
        }
        update_refs(items, &id_map);
        for arp in arps {
            if let Some(new_id) = id_map.get(&arp.span.start) { arp.span.start = new_id.clone(); }
            if let Some(new_id) = id_map.get(&arp.span.end) { arp.span.end = new_id.clone(); }
        }
    }

    pub fn action_paste_insert(&mut self) {
        let cursor = match self.selected_event_idx { Some(c) => c, None => return };
        let cb = match &self.clipboard { Some(c) => c, None => return };
        let mut doc = match self.parsed_mnx.as_ref() { Some(d) => d.clone(), None => return };
        let part = match doc.parts.first_mut() { Some(p) => p, None => return };
        
        let mut flat_items = Vec::new();
        let mut all_pasted_arps: Vec<crate::mnx::Arpeggio> = Vec::new();
        let mut cur_e = 0;
        let mut inserted = false;
        
        for (m_idx, m) in part.measures.iter().enumerate() {
            let measure_start_e = cur_e;
            for seq in &m.sequences {
                for item in &seq.content {
                    if cur_e == cursor {
                        if cur_e == measure_start_e {
                            if let Some(ts) = &cb.time_signature {
                                if let Some(gm) = doc.global.measures.get_mut(m_idx) {
                                    gm.time = Some(ts.clone());
                                }
                            }
                        }
                        
                        let mut pasted_items = cb.items.clone();
                        let mut pasted_arps = cb.arpeggios.clone();
                        Self::prepare_pasted_items(&mut pasted_items, &mut pasted_arps);
                        flat_items.extend(pasted_items);
                        all_pasted_arps.extend(pasted_arps);
                        inserted = true;
                    }
                    flat_items.push(item.clone());
                    let item_ev_count = match item {
                        crate::mnx::SequenceItem::Event(_) => 1,
                        crate::mnx::SequenceItem::Tuplet(t) => t.content.len(),
                    };
                    cur_e += item_ev_count;
                }
            }
        }
        
        if !inserted {
            let mut pasted_items = cb.items.clone();
            let mut pasted_arps = cb.arpeggios.clone();
            Self::prepare_pasted_items(&mut pasted_items, &mut pasted_arps);
            flat_items.extend(pasted_items);
            all_pasted_arps.extend(pasted_arps);
        }
        
        Self::rebuild_part_with_rebar(part, &mut doc.global.measures, flat_items, all_pasted_arps);
        
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
                }
            }
        }
        count
    }

    pub fn action_paste_overwrite(&mut self) {
        let cursor = match self.selected_event_idx { Some(c) => c, None => return };
        let cb = match &self.clipboard { Some(c) => c, None => return };
        let mut doc = match self.parsed_mnx.as_ref() { Some(d) => d.clone(), None => return };
        let part = match doc.parts.first_mut() { Some(p) => p, None => return };
        
        let mut cb_items = cb.items.clone();
        Self::prepare_pasted_items(&mut cb_items, &mut []);
        let mut cb_beats = 0.0;
        for item in &cb_items {
            let beats = match item {
                crate::mnx::SequenceItem::Event(ev) => crate::validate::duration_to_quarter_beats(&ev.duration),
                crate::mnx::SequenceItem::Tuplet(t) => crate::validate::duration_to_quarter_beats(&t.outer.duration) * (t.outer.multiple as f64),
            };
            cb_beats += beats;
        }
        
        let mut flat_items = Vec::new();
        let mut all_pasted_arps: Vec<crate::mnx::Arpeggio> = Vec::new();
        let mut cur_e = 0;
        let mut absorbed_beats = 0.0;
        let mut inserted = false;
        
        for (m_idx, m) in part.measures.iter().enumerate() {
            let measure_start_e = cur_e;
            for seq in &m.sequences {
                for item in &seq.content {
                    if cur_e == cursor && !inserted {
                        if cur_e == measure_start_e {
                            if let Some(ts) = &cb.time_signature {
                                if let Some(gm) = doc.global.measures.get_mut(m_idx) {
                                    gm.time = Some(ts.clone());
                                }
                            }
                        }
                        
                        let mut pasted_items = cb.items.clone();
                        let mut pasted_arps = cb.arpeggios.clone();
                        Self::prepare_pasted_items(&mut pasted_items, &mut pasted_arps);
                        flat_items.extend(pasted_items);
                        all_pasted_arps.extend(pasted_arps);
                        inserted = true;
                    }
                    
                    let item_beats = match item {
                        crate::mnx::SequenceItem::Event(ev) => crate::validate::duration_to_quarter_beats(&ev.duration),
                        crate::mnx::SequenceItem::Tuplet(t) => crate::validate::duration_to_quarter_beats(&t.outer.duration) * (t.outer.multiple as f64),
                    };
                    
                    let item_ev_count = match item {
                        crate::mnx::SequenceItem::Event(_) => 1,
                        crate::mnx::SequenceItem::Tuplet(t) => t.content.len(),
                    };
                    
                    if inserted && absorbed_beats < cb_beats - 0.001 {
                        absorbed_beats += item_beats;
                        if absorbed_beats > cb_beats + 0.001 {
                            let excess = absorbed_beats - cb_beats;
                            match item {
                                crate::mnx::SequenceItem::Event(ev) => {
                                    let remainder_ev = ev.clone();
                                    let chunks = crate::rebar::greedy_split_event_duration(remainder_ev, excess);
                                    for c in chunks {
                                        flat_items.push(crate::mnx::SequenceItem::Event(c));
                                    }
                                },
                                crate::mnx::SequenceItem::Tuplet(_t) => {
                                    flat_items.push(item.clone());
                                }
                            }
                        }
                    } else {
                        flat_items.push(item.clone());
                    }
                    
                    cur_e += item_ev_count;
                }
            }
        }
        
        if !inserted {
            let mut pasted_items = cb.items.clone();
            let mut pasted_arps = cb.arpeggios.clone();
            Self::prepare_pasted_items(&mut pasted_items, &mut pasted_arps);
            flat_items.extend(pasted_items);
            all_pasted_arps.extend(pasted_arps);
        }
        
        Self::rebuild_part_with_rebar(part, &mut doc.global.measures, flat_items, all_pasted_arps);
        
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
        let cursor = match self.selected_event_idx { Some(c) => c, None => return };
        let mut doc = match self.parsed_mnx.as_ref() { Some(d) => d.clone(), None => return };
        let part = match doc.parts.first_mut() { Some(p) => p, None => return };
        
        let mut flat_items = Vec::new();
        let mut cur_e = 0;
        let mut inserted = false;
        for m in &part.measures {
            for seq in &m.sequences {
                for item in &seq.content {
                    if cur_e == cursor && !inserted {
                        let rest_ev = crate::mnx::Event {
                            id: None, duration: crate::mnx::Duration { base: "whole".to_string(), dots: None, multiple: None },
                            notes: vec![],
                            rest: Some(crate::mnx::Rest {}),
                            slurs: None,
                        };
                        flat_items.push(crate::mnx::SequenceItem::Event(rest_ev));
                        inserted = true;
                    }
                    flat_items.push(item.clone());
                    let item_ev_count = match item {
                        crate::mnx::SequenceItem::Event(_) => 1,
                        crate::mnx::SequenceItem::Tuplet(t) => t.content.len(),
                    };
                    cur_e += item_ev_count;
                }
            }
        }
        
        if !inserted {
            let rest_ev = crate::mnx::Event {
                id: None, duration: crate::mnx::Duration { base: "whole".to_string(), dots: None, multiple: None },
                notes: vec![],
                rest: Some(crate::mnx::Rest {}),
                slurs: None,
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
        let cb_after = self.clipboard.as_ref().map(|cb| cb.items.len()).unwrap_or(0);
        println!("action_copy finished. cb_after: {}", cb_after);
        let max_idx = if let Some(cursor) = self.selected_event_idx {
            let anchor = self.selection_anchor.unwrap_or(cursor);
            cursor.max(anchor)
        } else { return };
        self.selected_event_idx = Some(max_idx + 1);
        self.selection_anchor = None;
        self.action_paste_overwrite();
    }

    pub fn action_duplicate_insert(&mut self) {
        let cb_before = self.clipboard.as_ref().map(|cb| cb.items.len()).unwrap_or(0);
        println!("action_duplicate_insert called. cb_before: {}", cb_before);
        self.action_copy();
        let cb_after = self.clipboard.as_ref().map(|cb| cb.items.len()).unwrap_or(0);
        println!("action_copy finished. cb_after: {}", cb_after);
        let max_idx = if let Some(cursor) = self.selected_event_idx {
            let anchor = self.selection_anchor.unwrap_or(cursor);
            cursor.max(anchor)
        } else { return };
        self.selected_event_idx = Some(max_idx + 1);
        self.selection_anchor = None;
        self.action_paste_insert();
    }
}
