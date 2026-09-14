use crate::mnx::MnxDocument;
use crate::edo::EdoTuning;
use std::collections::HashSet;

#[derive(Debug)]
pub struct ValidationError {
    pub measure_idx: usize,
    pub message: String,
}

pub fn validate_mnx(doc: &MnxDocument) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    for part in &doc.parts {
        // Extract instrument properties safely (with defaults if missing)
        let inst = part.instrument.as_ref();
        let capo_fret = inst.and_then(|i| i.capo).unwrap_or(0);
        let edo = inst.and_then(|i| i.micro_fret_divisions).unwrap_or(12) as u32;
        let step_multiplier = inst.and_then(|i| i.fret_step_multiplier).unwrap_or(1) as i32;
        
        let tuning_calc = EdoTuning::new(edo);
        let e = edo as i32;

        for (m_idx, measure) in part.measures.iter().enumerate() {
            for seq in &measure.sequences {
                for event in &seq.content {
                    let mut active_strings = HashSet::new();

                    for note in &event.notes {
                        if let (Some(string), Some(fret)) = (note.string, note.fret) {
                            
                            // Rule 2: Concurrency check (one note per string per event)
                            if !active_strings.insert(string) {
                                errors.push(ValidationError {
                                    measure_idx: m_idx + 1,
                                    message: format!("Concurrency violation: String {} is plucked multiple times.", string),
                                });
                            }

                            // 1. Find the tuning for this specific string number
                            let mut string_step = 'C';
                            let mut string_octave = 0;
                            let mut string_alter_semitones = 0.0; // <-- NEW: Track open string alterations
                            let mut found_tuning = false;

                            // First, check the MNX instrument definition
                            if let Some(instrument) = inst {
                                if let Some(tunings) = &instrument.tuning {
                                    if let Some(t) = tunings.iter().find(|t| t.string == string) {
                                        string_step = t.pitch.step.chars().next().unwrap_or('C');
                                        string_octave = t.pitch.octave;
                                        // <-- NEW: Extract the alter from the open string
                                        string_alter_semitones = t.pitch.alter.as_ref().map(|a| a.as_semitones()).unwrap_or(0.0);
                                        found_tuning = true;
                                    }
                                }
                            }

                            // If not found in MNX, fallback to default Standard E
                            if !found_tuning {
                                match string {
                                    1 => { string_step = 'E'; string_octave = 4; found_tuning = true; }
                                    2 => { string_step = 'B'; string_octave = 3; found_tuning = true; }
                                    3 => { string_step = 'G'; string_octave = 3; found_tuning = true; }
                                    4 => { string_step = 'D'; string_octave = 3; found_tuning = true; }
                                    5 => { string_step = 'A'; string_octave = 2; found_tuning = true; }
                                    6 => { string_step = 'E'; string_octave = 2; found_tuning = true; }
                                    _ => {}
                                }
                            }

                            if found_tuning {
                                let note_step = note.pitch.step.chars().next().unwrap_or('C');
                                let note_octave = note.pitch.octave;
                                let note_alter_semitones = note.pitch.alter.as_ref().map(|a| a.as_semitones()).unwrap_or(0.0);

                                // 2. Calculate physical steps
                                // <-- NEW: Convert the open string's alter semitones into discrete EDO steps
                                let string_alter_steps = (string_alter_semitones / tuning_calc.step_semitones).round() as i32;
                                
                                // <-- NEW: Apply string_alter_steps to the base calculation
                                let string_base = tuning_calc.nominal_step(string_step) + (string_octave * e) + string_alter_steps;
                                
                                let physical_capo_steps = (capo_fret as i32) * step_multiplier;
                                let physical_fret_steps = (fret as i32) * step_multiplier;
                                
                                let played_steps = string_base + physical_capo_steps + physical_fret_steps;

                                // 3. Calculate semantic steps
                                let expected_base = tuning_calc.nominal_step(note_step) + (note_octave * e);
                                let alter_steps = (note_alter_semitones / tuning_calc.step_semitones).round() as i32;
                                let expected_steps = expected_base + alter_steps;

                                // 4. Strict Integer Check!
                                if played_steps != expected_steps {
                                    let alter_str = note.pitch.alter.as_ref().map(|a| format!(" alter {}", a)).unwrap_or_default();
                                    
                                    errors.push(ValidationError {
                                        measure_idx: m_idx + 1,
                                        message: format!(
                                            "Geometry mismatch on String {} Fret {}. \
                                            Physically produces step {} in {}-EDO. \
                                            MNX data expects {}{}{} (step {}).",
                                            string, fret, 
                                            played_steps, edo,
                                            note.pitch.step, note.pitch.octave, alter_str, expected_steps
                                        ),
                                    });
                                }
                            } else {
                                // Handles strings like 7 or 8 if no custom tuning was provided
                                errors.push(ValidationError {
                                    measure_idx: m_idx + 1,
                                    message: format!("Note placed on string {}, but no tuning is defined for that string.", string),
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    errors
}