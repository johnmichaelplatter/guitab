pub mod display_list;
pub mod sizing;
pub mod breaking;
pub mod justification;

pub use display_list::{DisplayList, DrawItem, DrawKind};
pub use sizing::{compute_measure_metrics, MeasureMetrics};
pub use breaking::{break_measures_into_systems, pack_systems_into_pages, PageLayout, SystemLayout};
pub use justification::justify_systems;

use crate::mnx::{MnxDocument, PageMargins};

pub const DEFAULT_LAYOUT_BASE_SPACING: f32 = 16.0;

/// Computes the complete paginated and justified layout for a document.
pub fn compute_score_layout(doc: &MnxDocument, base_spacing: f32) -> Vec<PageLayout> {
    let layout_cfg = doc.layout.as_ref();
    let page_size_str = layout_cfg.map(|l| l.page_size.as_str()).unwrap_or("letter");
    let (page_w, page_h) = match page_size_str.to_lowercase().as_str() {
        "a4" => (794.0, 1123.0), // A4 in standard points @ 96DPI approx
        _ => (816.0, 1056.0),    // US Letter @ 96DPI (8.5 x 11 in)
    };

    let margins = layout_cfg
        .map(|l| l.margins.clone())
        .unwrap_or_else(PageMargins::default);
    let justify_last = layout_cfg.map(|l| l.justify_last_system).unwrap_or(false);

    let num_strings = doc.parts.first().map(|p| p.num_strings()).unwrap_or(6);
    let line_spacing = 11.0_f32;
    let staff_height = (num_strings as f32 - 1.0).max(0.0) * line_spacing;

    let max_lyric_lines = doc.parts.first().map(|p| {
        p.measures.iter().map(|m| {
            m.sequences.iter().map(|s| {
                s.flattened_events().iter().map(|e| {
                    e.lyrics.as_ref().map(|l| {
                        l.lines.keys().filter_map(|k| k.parse::<usize>().ok()).max().unwrap_or(l.lines.len())
                    }).unwrap_or(0)
                }).max().unwrap_or(0)
            }).max().unwrap_or(0)
        }).max().unwrap_or(0)
    }).unwrap_or(0);

    let lyrics_extra = if max_lyric_lines > 0 { (max_lyric_lines as f32) * 14.0_f32 } else { 0.0_f32 };
    let system_height = staff_height + 56.0 + lyrics_extra;
    let system_gap = 14.0_f32;
    let first_page_header_height = 75.0_f32;

    let effective_base_spacing = if base_spacing > 0.0 { base_spacing } else { DEFAULT_LAYOUT_BASE_SPACING };
    let metrics = compute_measure_metrics(doc, effective_base_spacing);
    let content_w = (page_w - margins.left - margins.right).max(200.0);
    let locked_systems = doc.layout.as_ref().map(|l| l.locked_systems.as_slice()).unwrap_or(&[]);
    let system_breaks = break_measures_into_systems(&metrics, content_w, locked_systems);
    let mut pages = pack_systems_into_pages(
        system_breaks,
        page_w,
        page_h,
        &margins,
        system_height,
        system_gap,
        first_page_header_height,
    );
    justify_systems(&mut pages, &metrics, justify_last);

    pages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::sizing::MeasureMetrics;
    use crate::mnx::PageMargins;

    fn dummy_metrics(idx: usize, width: f32, beats: f64) -> MeasureMetrics {
        MeasureMetrics {
            measure_idx: idx,
            measure_number: (idx + 1) as u32,
            min_width: width,
            spring_stiffness: beats,
            has_system_break: false,
            has_page_break: false,
            time_signature_width: 0.0,
            has_repeat_start: false,
            has_repeat_end: false,
            ending_numbers: None,
        }
    }

    #[test]
    fn test_system_breaker_auto_flow() {
        // Content width 500.0. 4 measures with min_width 150.0 each.
        // Measure 0,1,2 = 450.0 fits on system 1. Measure 3 wraps to system 2.
        let measures = vec![
            dummy_metrics(0, 150.0, 4.0),
            dummy_metrics(1, 150.0, 4.0),
            dummy_metrics(2, 150.0, 4.0),
            dummy_metrics(3, 150.0, 4.0),
        ];

        let breaks = break_measures_into_systems(&measures, 500.0, &[]);
        assert_eq!(breaks.len(), 2);
        assert_eq!(breaks[0].measure_range, 0..3);
        assert_eq!(breaks[1].measure_range, 3..4);
    }

    #[test]
    fn test_system_breaker_hard_breaks() {
        let mut measures = vec![
            dummy_metrics(0, 100.0, 4.0),
            dummy_metrics(1, 100.0, 4.0),
            dummy_metrics(2, 100.0, 4.0),
            dummy_metrics(3, 100.0, 4.0),
        ];
        measures[1].has_system_break = true;
        measures[2].has_page_break = true;

        let breaks = break_measures_into_systems(&measures, 1000.0, &[]);
        assert_eq!(breaks.len(), 3);
        assert_eq!(breaks[0].measure_range, 0..2);
        assert!(!breaks[0].is_page_break);
        assert_eq!(breaks[1].measure_range, 2..3);
        assert!(breaks[1].is_page_break);
        assert_eq!(breaks[2].measure_range, 3..4);
    }

    #[test]
    fn test_locked_system_spans_and_overrides_intermediate_system_break() {
        let mut measures = vec![
            dummy_metrics(0, 100.0, 4.0),
            dummy_metrics(1, 100.0, 4.0),
            dummy_metrics(2, 100.0, 4.0),
            dummy_metrics(3, 100.0, 4.0),
        ];
        // Measure 1 previously had a system break
        measures[1].has_system_break = true;

        // When locked as system [0, 2], the intermediate system break at measure 1 is overridden
        let breaks = break_measures_into_systems(&measures, 1000.0, &[[0, 2]]);
        assert_eq!(breaks.len(), 2);
        assert_eq!(breaks[0].measure_range, 0..3);
        assert!(!breaks[0].is_page_break);
        assert_eq!(breaks[1].measure_range, 3..4);
    }

    #[test]
    fn test_system_justification_right_margin_alignment() {
        let measures = vec![
            dummy_metrics(0, 150.0, 4.0),
            dummy_metrics(1, 150.0, 4.0),
            dummy_metrics(2, 150.0, 4.0),
        ];

        let page_w = 600.0;
        let margins = PageMargins { top: 40.0, bottom: 40.0, left: 50.0, right: 50.0 };
        let content_w = page_w - margins.left - margins.right; // 500.0

        let breaks = break_measures_into_systems(&measures, content_w, &[]);
        let mut pages = pack_systems_into_pages(breaks, page_w, 800.0, &margins, 100.0, 20.0, 0.0);

        // Justify system 0
        justify_systems(&mut pages, &measures, true);

        let system = &pages[0].systems[0];
        let total_system_width: f32 = system.measure_widths.iter().sum();
        let right_edge = system.measure_x_positions[0] + total_system_width;

        // Right edge must align precisely with page_w - margins.right (550.0)
        assert!((right_edge - (page_w - margins.right)).abs() < 1e-3);
    }

    #[test]
    fn test_element_offset_application() {
        use crate::mnx::Offset2D;
        use eframe::egui::{Pos2, Stroke, Color32};
        use std::collections::HashMap;

        let mut list = DisplayList::new();
        list.push(DrawItem {
            id: Some("tempo_1".to_string()),
            kind: DrawKind::Line {
                from: Pos2::new(10.0, 20.0),
                to: Pos2::new(50.0, 20.0),
                stroke: Stroke::new(1.0, Color32::BLACK),
            },
            z_index: 0,
        });

        let mut offsets = HashMap::new();
        offsets.insert("tempo_1".to_string(), Offset2D { dx: 15.0, dy: -5.0 });

        list.apply_offsets(&offsets);

        if let DrawKind::Line { from, to, .. } = list.items[0].kind {
            assert_eq!(from, Pos2::new(25.0, 15.0));
            assert_eq!(to, Pos2::new(65.0, 15.0));
        } else {
            panic!("Expected line item");
        }
    }
}
