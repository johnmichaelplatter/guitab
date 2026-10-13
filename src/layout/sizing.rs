use crate::mnx::MnxDocument;
use crate::render::{calculate_event_spacing, LayoutItem};

#[derive(Debug, Clone, PartialEq)]
pub struct MeasureMetrics {
    pub measure_idx: usize,
    pub measure_number: u32,
    pub min_width: f32,
    pub spring_stiffness: f64,
    pub has_system_break: bool,
    pub has_page_break: bool,
    pub time_signature_width: f32,
    pub has_repeat_start: bool,
    pub has_repeat_end: bool,
    pub ending_numbers: Option<Vec<u32>>,
}

/// Computes the spring and rod sizing metrics for each measure in a score.
pub fn compute_measure_metrics(doc: &MnxDocument, base_spacing: f32) -> Vec<MeasureMetrics> {
    let mut metrics = Vec::new();
    let part = match doc.parts.first() {
        Some(p) => p,
        None => return metrics,
    };

    let mut current_measure_num = 1;
    let mut current_ts: Option<crate::mnx::TimeSignature> = None;

    for (m_idx, measure) in part.measures.iter().enumerate() {
        let gm = doc.global.measures.get(m_idx);
        let m_num = gm.and_then(|g| g.number).unwrap_or(current_measure_num);
        current_measure_num = m_num + 1;

        let has_system_break = gm.and_then(|g| g.system_break).unwrap_or(false);
        let has_page_break = gm.and_then(|g| g.page_break).unwrap_or(false);
        let has_repeat_start = gm.and_then(|g| g.repeat_start.as_ref()).is_some();
        let has_repeat_end = gm.and_then(|g| g.repeat_end.as_ref()).is_some();
        let ending_numbers = gm.and_then(|g| g.ending.as_ref()).map(|e| e.numbers.clone());

        let m_ts = gm.and_then(|g| g.time);
        let shows_ts = if m_idx == 0 {
            m_ts.is_some()
        } else if let Some(ts) = m_ts {
            current_ts != Some(ts)
        } else {
            false
        };

        if let Some(ts) = m_ts {
            current_ts = Some(ts);
        }

        let ts_width = if shows_ts { 45.0 } else { 0.0 };

        let mut min_width = 15.0 + ts_width;
        if has_repeat_start {
            min_width += 14.0;
        }

        let mut measure_beats = 0.0;
        if let Some(seq) = measure.sequences.first() {
            let mut layout_items = Vec::new();
            for item in &seq.content {
                item.to_layout_items(&mut layout_items);
            }

            let mut in_grace = false;
            for (item_idx, l_item) in layout_items.iter().enumerate() {
                match l_item {
                    LayoutItem::GraceStart => in_grace = true,
                    LayoutItem::GraceEnd => in_grace = false,
                    LayoutItem::Event(ev) => {
                        let mut next_ev_info = None;
                        let mut look_grace = in_grace;
                        for next_item in &layout_items[item_idx + 1..] {
                            match next_item {
                                LayoutItem::GraceStart => look_grace = true,
                                LayoutItem::GraceEnd => look_grace = false,
                                LayoutItem::Event(ne) => {
                                    next_ev_info = Some((*ne, look_grace));
                                    break;
                                }
                                _ => {}
                            }
                        }

                        let ev_w = calculate_event_spacing(
                            ev,
                            in_grace,
                            base_spacing,
                            next_ev_info,
                            None,
                            None,
                        );
                        min_width += ev_w;

                        if !in_grace {
                            measure_beats += crate::validate::duration_to_quarter_beats(&ev.duration);
                        }
                    }
                    _ => {}
                }
            }
        }

        if has_repeat_end {
            min_width += 14.0;
        }

        // Clamp minimum measure width
        min_width = min_width.max(65.0);
        let spring_stiffness = if measure_beats > 0.0 {
            measure_beats
        } else {
            current_ts.map(|ts| (ts.count as f64) * 4.0 / (ts.unit as f64)).unwrap_or(4.0)
        };

        metrics.push(MeasureMetrics {
            measure_idx: m_idx,
            measure_number: m_num,
            min_width,
            spring_stiffness,
            has_system_break,
            has_page_break,
            time_signature_width: ts_width,
            has_repeat_start,
            has_repeat_end,
            ending_numbers,
        });
    }

    metrics
}
