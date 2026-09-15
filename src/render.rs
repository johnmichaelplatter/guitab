use crate::mnx::{Part, Pitch};
use crate::validate::ValidationError;
use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{Color32, FontId, FontFamily};

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

pub fn render_part_to_tab(part: &Part, errors: &[ValidationError]) -> LayoutJob {
    let mut job = LayoutJob::default();
    let font = FontId::new(16.0, FontFamily::Monospace);
    
    // Define our styling
    let default_fmt = TextFormat::simple(font.clone(), Color32::LIGHT_GRAY);
    let error_fmt = TextFormat::simple(font.clone(), Color32::RED);

    let num_strings = part.num_strings();

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

    let max_prefix_len = prefixes.iter().map(|p| p.len()).max().unwrap_or(1);
    
    // Create a 2D grid holding (String, TextFormat)
    let mut string_spans: Vec<Vec<(String, TextFormat)>> = vec![Vec::new(); num_strings];

    for (i, p) in prefixes.iter().enumerate() {
        let padded = format!("{:>width$}|", p, width = max_prefix_len);
        string_spans[i].push((padded, default_fmt.clone()));
    }

    let error_coords: std::collections::HashSet<_> = errors
        .iter()
        .map(|err| (err.measure_idx, err.event_idx, err.string, err.fret))
        .collect();

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

                let max_width = step_frets.iter().flatten()
                    .map(|f| f.to_string().len()).max().unwrap_or(1);

                for i in 0..num_strings {
                    let string_num = (i + 1) as u8;
                    if let Some(fret) = step_frets[i] {
                        
                        let is_err = error_coords.contains(&(m_idx + 1, e_idx, string_num, fret));

                        let fmt = if is_err { &error_fmt } else { &default_fmt };
                        let fret_str = fret.to_string();
                        let padding = "-".repeat(max_width - fret_str.len());

                        // Append the segments with individual coloring
                        string_spans[i].push(("-".to_string(), default_fmt.clone()));
                        string_spans[i].push((fret_str, fmt.clone()));
                        string_spans[i].push((format!("{}-", padding), default_fmt.clone()));
                        
                    } else {
                        string_spans[i].push(("-".repeat(max_width + 2), default_fmt.clone()));
                    }
                }
            }
        }
        for i in 0..num_strings {
            string_spans[i].push(("|".to_string(), default_fmt.clone()));
        }
    }

    // Assemble the grid row by row into the LayoutJob
    for (i, spans) in string_spans.iter().enumerate() {
        for (text, format) in spans {
            job.append(text, 0.0, format.clone());
        }
        if i < num_strings - 1 {
            job.append("\n", 0.0, default_fmt.clone());
        }
    }

    job
}