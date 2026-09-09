use crate::mnx::Part;

pub fn render_part_to_tab(part: &Part) -> String {
    // Array representing strings 1 (High) through 6 (Low)
    let mut lines: Vec<String> = vec![
        String::from("1|"),
        String::from("2|"),
        String::from("3|"),
        String::from("4|"),
        String::from("5|"),
        String::from("6|"),
    ];

    // Inject custom tuning prefixes if present in the MNX file
    if let Some(inst) = &part.instrument {
        if let Some(tuning) = &inst.tuning {
            for t in tuning {
                if t.string >= 1 && t.string <= 6 {
                    let idx = (t.string - 1) as usize;
                    lines[idx] = format!("{}|", t.pitch.step);
                }
            }
        }
    }

    for measure in &part.measures {
        if let Some(seq) = measure.sequences.first() {
            for event in &seq.content {
                
                // Collect frets to play at this timestep
                let mut step_frets: [Option<u8>; 6] = [None; 6];
                for note in &event.notes {
                    if let (Some(s), Some(f)) = (note.string, note.fret) {
                        if s >= 1 && s <= 6 {
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
                for i in 0..6 {
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
        for i in 0..6 {
            lines[i].push('|');
        }
    }

    lines.join("\n")
}