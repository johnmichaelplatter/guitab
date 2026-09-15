use crate::mnx::MnxDocument;
use crate::edo::EdoTuning;
use std::collections::HashSet;

#[derive(Debug)]
pub struct ValidationError {
    pub measure_idx: usize,
    pub event_idx: usize, // NEW: Tracks the exact column/timestep
    pub string: u8,       // NEW: Tracks the exact row
    pub fret: u8,         // NEW: Tracks the exact fret value
    pub message: String,
}

pub fn validate_mnx(doc: &MnxDocument) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    for part in &doc.parts {
        let capo_fret = part.capo_fret();
        let edo = part.edo();
        let step_multiplier = part.step_multiplier();
        
        let tuning_calc = EdoTuning::new(edo);

        for (m_idx, measure) in part.measures.iter().enumerate() {
            for seq in &measure.sequences {
                // NEW: Enumerate over the events to get the event_idx
                for (e_idx, event) in seq.content.iter().enumerate() { 
                    let mut active_strings = HashSet::new();

                    for note in &event.notes {
                        if let (Some(string), Some(fret)) = (note.string, note.fret) {
                            
                            if !active_strings.insert(string) {
                                errors.push(ValidationError {
                                    measure_idx: m_idx + 1,
                                    event_idx: e_idx,
                                    string,
                                    fret,
                                    message: format!("Concurrency violation: String {} is plucked multiple times.", string),
                                });
                            }

                            if let Some(string_pitch) = part.get_string_tuning(string) {
                                let string_base = tuning_calc.pitch_to_steps(&string_pitch);
                                
                                let physical_capo_steps = (capo_fret as i32) * step_multiplier;
                                let physical_fret_steps = (fret as i32) * step_multiplier;
                                let played_steps = string_base + physical_capo_steps + physical_fret_steps;

                                let expected_steps = tuning_calc.pitch_to_steps(&note.pitch);

                                if played_steps != expected_steps {
                                    let alter_str = note.pitch.alter.as_ref().map(|a| format!(" alter {}", a)).unwrap_or_default();
                                    
                                    errors.push(ValidationError {
                                        measure_idx: m_idx + 1,
                                        event_idx: e_idx,
                                        string,
                                        fret,
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
                                errors.push(ValidationError {
                                    measure_idx: m_idx + 1,
                                    event_idx: e_idx,
                                    string,
                                    fret,
                                    message: format!("Note placed on string {}, but no tuning is defined.", string),
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