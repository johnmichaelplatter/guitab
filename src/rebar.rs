use crate::mnx::{Duration, Event, Measure, Part, Sequence, SequenceItem, Tie, TimeSignature};
use crate::validate::{duration_to_quarter_beats, quarter_beats_to_duration};

pub fn generate_uuid() -> String {
    let random_number: u32 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    format!("id-{:x}", random_number)
}

pub fn rebar_part(part: &mut Part, global_measures: &mut Vec<crate::mnx::GlobalMeasure>) {
    let mut flat_items = Vec::new();
    for measure in &part.measures {
        for seq in &measure.sequences {
            flat_items.extend(seq.content.clone());
        }
    }
    part.measures.clear();

    let eps = 0.001;
    let mut item_queue = std::collections::VecDeque::from(flat_items);
    let mut m_idx = 0;

    if item_queue.is_empty() {
        part.measures.push(Measure {
            beams: None,
            arpeggios: None,
            sequences: vec![Sequence {
                content: Vec::new(),
                beams: None,
            }],
        });
        global_measures.truncate(1);
        return;
    }

    let mut current_ts = TimeSignature { count: 4, unit: 4 };
    while !item_queue.is_empty() {
        // Find the most recent explicit TS up to m_idx
        for i in (0..=m_idx).rev() {
            if let Some(gm) = global_measures.get(i) {
                if let Some(t) = &gm.time {
                    current_ts = t.clone();
                    break;
                }
            }
        }
        let ts = current_ts.clone();

        if m_idx >= global_measures.len() {
            global_measures.push(crate::mnx::GlobalMeasure {
                time: Some(ts.clone()),
                repeat_start: None, repeat_end: None, barline: None, tempos: Vec::new(),
                            tempo_changes: Vec::new(),
            });
        }

        let mut remaining_capacity = (ts.count as f64) * 4.0 / (ts.unit as f64);
        let mut current_measure_items = Vec::new();

        while remaining_capacity > eps {
            if let Some(item) = item_queue.pop_front() {
                let item_beats = match &item {
                    SequenceItem::Event(ev) => duration_to_quarter_beats(&ev.duration),
                    SequenceItem::Tuplet(t) => {
                        duration_to_quarter_beats(&t.outer.duration) * (t.outer.multiple as f64)
                    },
                    SequenceItem::Grace(_) => 0.0,
                };

                if item_beats <= remaining_capacity + eps {
                    current_measure_items.push(item);
                    remaining_capacity -= item_beats;
                } else {
                    match item {
                        SequenceItem::Event(ev) => {
                            let (chunks1, chunks2) =
                                split_event_for_capacity(ev, remaining_capacity);
                            for c in chunks1 {
                                current_measure_items.push(SequenceItem::Event(c));
                            }
                            remaining_capacity = 0.0;

                            for rem in chunks2.into_iter().rev() {
                                item_queue.push_front(SequenceItem::Event(rem));
                            }
                        }
                        SequenceItem::Tuplet(t) => {
                            current_measure_items.push(SequenceItem::Tuplet(t));
                            remaining_capacity -= item_beats;
                        }
                        SequenceItem::Grace(g) => {
                            current_measure_items.push(SequenceItem::Grace(g));
                            remaining_capacity -= item_beats;
                        }
                    }
                }
            } else {
                break;
            }
        }

        part.measures.push(Measure {
            beams: None,
            arpeggios: None,
            sequences: vec![Sequence {
                content: current_measure_items,
                beams: None,
            }],
        });

        m_idx += 1;
    }

    global_measures.truncate(m_idx);
}

pub fn split_event_for_capacity(ev: Event, capacity: f64) -> (Vec<Event>, Vec<Event>) {
    let eps = 0.001;
    let total_beats = duration_to_quarter_beats(&ev.duration);

    let beats1 = total_beats.min(capacity);
    let beats2 = total_beats - beats1;

    if beats2 < eps {
        // Fits entirely (should have been caught earlier, but just in case)
        let chunks1 = greedy_split_event_duration(ev, beats1);
        return (chunks1, vec![]);
    }

    // Split into chunks1 + chunks2, all tied together.

    // We MUST split `beats1` and `beats2` independently, but chain their ties!
    let mut chunks1 = greedy_split_event_duration(ev.clone(), beats1);

    // We need to tie the LAST chunk of chunks1 to the FIRST chunk of chunks2
    let mut template_for_2 = ev;
    if let Some(last_c1) = chunks1.last_mut() {
        for (n_c1, n_template) in last_c1
            .notes
            .iter_mut()
            .zip(template_for_2.notes.iter_mut())
        {
            let new_id = generate_uuid();
            if n_c1.ties.is_none() {
                n_c1.ties = Some(Vec::new());
            }
            n_c1.ties.as_mut().unwrap().push(Tie {
                target: new_id.clone(),
            });
            n_template.id = Some(new_id);
        }
    }

    let chunks2 = greedy_split_event_duration(template_for_2, beats2);

    (chunks1, chunks2)
}

pub fn greedy_split_event_duration(mut ev: Event, mut total_beats: f64) -> Vec<Event> {
    let mut result = Vec::new();
    let eps = 0.001;
    while total_beats > eps {
        if let Some((valid_dur, valid_beats)) = largest_valid_duration(total_beats) {
            let mut chunk = ev.clone();
            chunk.duration = valid_dur;
            total_beats -= valid_beats;

            if total_beats > eps {
                // Generate tie to next chunk
                for n in chunk.notes.iter_mut() {
                    let new_id = generate_uuid();
                    if n.ties.is_none() {
                        n.ties = Some(Vec::new());
                    }
                    n.ties.as_mut().unwrap().push(Tie {
                        target: new_id.clone(),
                    });
                }
                // Assign those target IDs to the ev template for the NEXT chunk
                for (n_chunk, n_ev) in chunk.notes.iter().zip(ev.notes.iter_mut()) {
                    n_ev.id = Some(
                        n_chunk
                            .ties
                            .as_ref()
                            .unwrap()
                            .last()
                            .unwrap()
                            .target
                            .clone(),
                    );
                }
            }

            result.push(chunk);
        } else {
            break;
        }
    }
    result
}

fn largest_valid_duration(beats: f64) -> Option<(Duration, f64)> {
    let eps = 0.001;
    let candidates = [
        4.0, 3.0, 2.0, 1.5, 1.0, 0.75, 0.5, 0.375, 0.25, 0.1875, 0.125, 0.0625,
    ];
    for &c in &candidates {
        if c <= beats + eps {
            if let Some(dur) = quarter_beats_to_duration(c) {
                return Some((dur, c));
            }
        }
    }
    None
}
