use crate::mnx::{Part, Pitch};
use crate::validate::ValidationError;
use eframe::egui::{Color32, FontId};

fn format_pitch(pitch: &Pitch) -> String {
    // ... (Keep your existing format_pitch exactly as it is)
    let mut prefix = String::new();
    let mut suffix = String::new();
    if let Some(alter) = &pitch.alter {
        let alter_str = alter.to_string();
        if alter_str.starts_with("-1\\") || alter_str.starts_with("-1/") {
            prefix.push('v');
        } else if alter_str.starts_with("1\\") || alter_str.starts_with("1/") {
            prefix.push('^');
        } else {
            suffix = format!("({})", alter_str);
        }
    }
    format!("{}{}{}", prefix, pitch.step, suffix)
}

use eframe::egui::{self, Pos2, Stroke, Vec2};

pub fn draw_tablature(ui: &mut egui::Ui, part: &Part, errors: &[ValidationError]) -> egui::Response {
    let num_strings = part.num_strings();
    if num_strings == 0 {
        return ui.label("No strings found.");
    }
    
    // Count events to determine width
    let mut total_events = 0;
    for measure in &part.measures {
        if let Some(seq) = measure.sequences.first() {
            total_events += seq.content.len();
        }
    }
    
    let font_id = FontId::monospace(14.0);
    
    let mut prefixes: Vec<String> = (1..=num_strings).map(|i| i.to_string()).collect();
    if let Some(inst) = &part.instrument {
        if let Some(tuning) = &inst.tuning {
            for t in tuning {
                if t.string >= 1 && (t.string as usize) <= num_strings {
                    prefixes[(t.string - 1) as usize] = format_pitch(&t.pitch);
                }
            }
        }
    }

    let max_prefix_width = prefixes
        .iter()
        .map(|p| ui.painter().layout_no_wrap(p.clone(), font_id.clone(), Color32::WHITE).size().x)
        .fold(0.0f32, f32::max);

    let line_spacing = 24.0;
    let event_spacing = 30.0;
    let measure_spacing = 20.0;
    let margin_left = 10.0;
    let label_gap = 8.0;
    let first_barline_offset = margin_left + max_prefix_width + label_gap;
    let first_bar_padding = 25.0;
    
    let start_offset_x = first_barline_offset + first_bar_padding;
    let total_width = start_offset_x + (total_events as f32) * event_spacing + (part.measures.len() as f32) * measure_spacing + 20.0;
    let total_height = (num_strings as f32 - 1.0).max(0.0) * line_spacing + 40.0;
    
    let (response, painter) = ui.allocate_painter(Vec2::new(total_width, total_height), egui::Sense::hover());
    let rect = response.rect;
    
    let first_barline_x = rect.min.x + first_barline_offset;
    let label_x = first_barline_x - label_gap;

    // Draw string lines and prefixes
    let mut string_y_coords = Vec::new();
    for i in 0..num_strings {
        let y = rect.min.y + 20.0 + (i as f32) * line_spacing;
        string_y_coords.push(y);
        
        // The line itself starts at the first barline
        painter.line_segment(
            [Pos2::new(first_barline_x, y), Pos2::new(rect.max.x - 10.0, y)],
            Stroke::new(1.0, Color32::from_gray(100)),
        );
        
        // String label right-aligned next to the first barline
        painter.text(
            Pos2::new(label_x, y),
            egui::Align2::RIGHT_CENTER,
            &prefixes[i],
            font_id.clone(),
            Color32::DARK_GRAY,
        );
    }
    
    let error_coords: std::collections::HashSet<_> = errors
        .iter()
        .map(|err| (err.measure_idx, err.event_idx, err.string, err.fret))
        .collect();

    // Initial measure bar line
    painter.line_segment(
        [Pos2::new(first_barline_x, string_y_coords[0]), Pos2::new(first_barline_x, string_y_coords[num_strings - 1])],
        Stroke::new(2.0, Color32::from_gray(150)),
    );
    
    let mut current_x = rect.min.x + start_offset_x;
    let bg_color = ui.visuals().panel_fill;

    for (m_idx, measure) in part.measures.iter().enumerate() {
        if let Some(seq) = measure.sequences.first() {
            for (e_idx, event) in seq.content.iter().enumerate() {
                
                let mut step_frets: Vec<Option<u8>> = vec![None; num_strings];
                for note in &event.notes {
                    if let (Some(s), Some(f)) = (note.string, note.fret) {
                        if s >= 1 && (s as usize) <= num_strings {
                            step_frets[(s - 1) as usize] = Some(f);
                        }
                    }
                }
                
                for i in 0..num_strings {
                    if let Some(fret) = step_frets[i] {
                        let string_num = (i + 1) as u8;
                        let is_err = error_coords.contains(&(m_idx + 1, e_idx, string_num, fret));
                        let text_color = if is_err { Color32::RED } else { Color32::BLACK };
                        let y = string_y_coords[i];
                        
                        let text = fret.to_string();
                        let galley = painter.layout_no_wrap(text, font_id.clone(), text_color);
                        let text_rect = egui::Rect::from_center_size(
                            Pos2::new(current_x, y),
                            galley.size(),
                        );
                        
                        // Background rectangle to mask the line behind the number
                        painter.rect_filled(
                            text_rect.expand(2.0),
                            0.0,
                            bg_color,
                        );
                        
                        // The actual fret number
                        painter.galley(
                            text_rect.min,
                            galley,
                            text_color
                        );
                    }
                }
                
                current_x += event_spacing;
            }
        }
        
        // Draw measure bar line
        painter.line_segment(
            [Pos2::new(current_x, string_y_coords[0]), Pos2::new(current_x, string_y_coords[num_strings - 1])],
            Stroke::new(1.0, Color32::from_gray(150)),
        );
        current_x += measure_spacing;
    }

    response
}