use crate::layout::sizing::MeasureMetrics;
use crate::mnx::PageMargins;

#[derive(Debug, Clone, PartialEq)]
pub struct SystemBreak {
    pub measure_range: std::ops::Range<usize>, // [start_m, end_m)
    pub is_page_break: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SystemLayout {
    pub system_idx: usize,
    pub measure_range: std::ops::Range<usize>,
    pub y_offset: f32,
    pub height: f32,
    pub measure_widths: Vec<f32>,
    pub measure_x_positions: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PageLayout {
    pub page_number: usize,
    pub systems: Vec<SystemLayout>,
    pub width: f32,
    pub height: f32,
    pub margins: PageMargins,
}

/// Breaks a sequence of measures into systems based on content width, automatic wrapping,
/// and explicit system/page breaks.
pub fn break_measures_into_systems(
    measures: &[MeasureMetrics],
    content_width: f32,
) -> Vec<SystemBreak> {
    let mut systems = Vec::new();
    if measures.is_empty() {
        return systems;
    }

    let mut system_start = 0;
    let mut current_width = 0.0;

    for (i, m) in measures.iter().enumerate() {
        let is_first_in_system = i == system_start;
        let would_overflow = !is_first_in_system && (current_width + m.min_width > content_width);

        if would_overflow {
            systems.push(SystemBreak {
                measure_range: system_start..i,
                is_page_break: false,
            });
            system_start = i;
            current_width = m.min_width;
        } else {
            current_width += m.min_width;
        }

        // Hard break after this measure
        if m.has_page_break {
            systems.push(SystemBreak {
                measure_range: system_start..(i + 1),
                is_page_break: true,
            });
            system_start = i + 1;
            current_width = 0.0;
        } else if m.has_system_break {
            systems.push(SystemBreak {
                measure_range: system_start..(i + 1),
                is_page_break: false,
            });
            system_start = i + 1;
            current_width = 0.0;
        }
    }

    if system_start < measures.len() {
        systems.push(SystemBreak {
            measure_range: system_start..measures.len(),
            is_page_break: false,
        });
    }

    systems
}

/// Packs systems into paginated sheets based on page height, margins, and explicit page breaks.
pub fn pack_systems_into_pages(
    system_breaks: Vec<SystemBreak>,
    page_width: f32,
    page_height: f32,
    margins: &PageMargins,
    system_height: f32,
    system_gap: f32,
) -> Vec<PageLayout> {
    let mut pages = Vec::new();
    if system_breaks.is_empty() {
        return pages;
    }

    let content_height = (page_height - margins.top - margins.bottom).max(100.0);
    let mut current_page_systems = Vec::new();
    let mut current_y = margins.top;
    let mut page_number = 1;
    let mut global_system_idx = 0;

    for s_break in system_breaks {
        let would_overflow = !current_page_systems.is_empty()
            && (current_y + system_height > margins.top + content_height);

        if would_overflow {
            pages.push(PageLayout {
                page_number,
                systems: current_page_systems,
                width: page_width,
                height: page_height,
                margins: margins.clone(),
            });
            page_number += 1;
            current_page_systems = Vec::new();
            current_y = margins.top;
        }

        current_page_systems.push(SystemLayout {
            system_idx: global_system_idx,
            measure_range: s_break.measure_range.clone(),
            y_offset: current_y,
            height: system_height,
            measure_widths: Vec::new(),
            measure_x_positions: Vec::new(),
        });

        global_system_idx += 1;
        current_y += system_height + system_gap;

        if s_break.is_page_break {
            pages.push(PageLayout {
                page_number,
                systems: current_page_systems,
                width: page_width,
                height: page_height,
                margins: margins.clone(),
            });
            page_number += 1;
            current_page_systems = Vec::new();
            current_y = margins.top;
        }
    }

    if !current_page_systems.is_empty() {
        pages.push(PageLayout {
            page_number,
            systems: current_page_systems,
            width: page_width,
            height: page_height,
            margins: margins.clone(),
        });
    }

    pages
}
