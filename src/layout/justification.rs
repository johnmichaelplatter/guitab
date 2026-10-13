use crate::layout::breaking::PageLayout;
use crate::layout::sizing::MeasureMetrics;

/// Distributes slack space across measures in each system to horizontally justify
/// the measures against page margins.
pub fn justify_systems(
    pages: &mut [PageLayout],
    measures: &[MeasureMetrics],
    justify_last_system: bool,
) {
    let total_systems: usize = pages.iter().map(|p| p.systems.len()).sum();
    let mut current_global_system = 0;

    for page in pages.iter_mut() {
        let content_width = page.width - page.margins.left - page.margins.right;

        for system in &mut page.systems {
            current_global_system += 1;
            let is_last_system = current_global_system == total_systems;

            let m_indices: Vec<usize> = system.measure_range.clone().collect();
            if m_indices.is_empty() {
                continue;
            }

            let system_measures: Vec<&MeasureMetrics> = m_indices
                .iter()
                .filter_map(|&idx| measures.get(idx))
                .collect();

            let min_system_width: f32 = system_measures.iter().map(|m| m.min_width).sum();
            let total_spring_stiffness: f64 = system_measures.iter().map(|m| m.spring_stiffness).sum();
            let slack = content_width - min_system_width;

            let mut widths = Vec::with_capacity(system_measures.len());
            let mut x_positions = Vec::with_capacity(system_measures.len());

            let should_justify = (!is_last_system || justify_last_system) && slack > 0.0;

            if should_justify && total_spring_stiffness > 0.0 {
                let mut accumulated_slack = 0.0_f32;
                for (i, m) in system_measures.iter().enumerate() {
                    let is_last_measure = i == system_measures.len() - 1;
                    let measure_slack = if is_last_measure {
                        // Absorb remaining slack to ensure exact pixel flush alignment
                        slack - accumulated_slack
                    } else {
                        let ratio = (m.spring_stiffness / total_spring_stiffness) as f32;
                        let w_slack = (slack * ratio).round();
                        accumulated_slack += w_slack;
                        w_slack
                    };
                    widths.push(m.min_width + measure_slack);
                }
            } else {
                for m in &system_measures {
                    widths.push(m.min_width);
                }
            }

            let mut current_x = page.margins.left;
            for &w in &widths {
                x_positions.push(current_x);
                current_x += w;
            }

            system.measure_widths = widths;
            system.measure_x_positions = x_positions;
        }
    }
}
