use crate::mnx::{Part, Pitch};

/// Translates the Pitch and Alteration enum into a tablature-friendly string
fn format_pitch(pitch: &Pitch) -> String {
    let mut prefix = String::new();
    let mut suffix = String::new();
    
    if let Some(alter) = &pitch.alter {
        let alter_str = alter.to_string();
        
        // Map fractional notation to Kite arrows
        if alter_str.starts_with("-1\\") || alter_str.starts_with("-1/") {
            prefix.push('v');
        } else if alter_str.starts_with("1\\") || alter_str.starts_with("1/") {
            prefix.push('^');
        } else {
            // Fallback for custom floats or other unmapped fractional steps
            suffix = format!("({})", alter_str);
        }
    }
    
    format!("{}{}{}", prefix, pitch.step, suffix)
}

pub fn render_part_to_tab(part: &Part) -> String {
    // 1. Determine number of strings (default 6, or max string defined in the tuning array)
    let num_strings = part
        .instrument
        .as_ref()
        .and_then(|inst| inst.tuning.as_ref())
        .map(|tuning| tuning.iter().map(|t| t.string).max().unwrap_or(6))
        .unwrap_or(6)
        .max(1) as usize;

    // 2. Build tuning prefixes
    let mut prefixes: Vec<String> = (1..=num_strings).map(|i| i.to_string()).collect();

    if let Some(inst) = &part.instrument {
        if let Some(tuning) = &inst.tuning {
            for t in tuning {
                if t.string >= 1 && (t.string as usize) <= num_strings {
                    let idx = (t.string - 1) as usize;
                    prefixes[idx] = format_pitch(&t.pitch);
                }
            }
        }
    }

    // Align the vertical bar by finding the longest prefix character count
    let max_prefix_len = prefixes.iter().map(|p| p.len()).max().unwrap_or(1);
    
    let mut lines: Vec<String> = prefixes
        .iter()
        .map(|p| format!("{:>width$}|", p, width = max_prefix_len))
        .collect();

    // 3. Render measures
    for measure in &part.measures {
        if let Some(seq) = measure.sequences.first() {
            for event in &seq.content {
                
                // Collect frets to play at this timestep
                let mut step_frets: Vec<Option<u8>> = vec![None; num_strings];
                for note in &event.notes {
                    if let (Some(s), Some(f)) = (note.string, note.fret) {
                        if s >= 1 && (s as usize) <= num_strings {
                            step_frets[(s - 1) as usize] = Some(f);
                        }
                    }
                }

                // Find the max character width for proper vertical alignment padding
                let max_width = step_frets
                    .iter()
                    .flatten()
                    .map(|f| f.to_string().len())
                    .max()
                    .unwrap_or(1);

                // Append the step to the grid
                for i in 0..num_strings {
                    if let Some(fret) = step_frets[i] {
                        let fret_str = fret.to_string();
                        let padding = "-".repeat(max_width - fret_str.len());
                        lines[i].push_str(&format!("-{}{}-", fret_str, padding));
                    } else {
                        lines[i].push_str(&"-".repeat(max_width + 2));
                    }
                }
            }
        }
        
        // Cap the measure
        for i in 0..num_strings {
            lines[i].push('|');
        }
    }

    lines.join("\n")
}