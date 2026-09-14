use crate::mnx::{MnxDocument, Pitch};
use crate::edo::EdoTuning;
use std::collections::{HashMap, HashSet};

#[derive(Debug)]
pub struct ValidationError {
    pub measure_idx: usize,
    pub message: String,
}

pub fn validate_mnx(doc: &MnxDocument) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    for part in &doc.parts {
        let mut tuning_map: HashMap<u8, f64> = HashMap::new();
        
        // Fret math configuration
        let divisions = part.instrument.as_ref().and_then(|i| i.micro_fret_divisions).unwrap_or(12);
        let step_size = 12.0 / (divisions as f64);
        
        // Capos shift pitch by physical frets, meaning they respect the step_size
        let capo_fret = part.instrument.as_ref().and_then(|i| i.capo).unwrap_or(0);
        let capo_offset = (capo_fret as f64) * step_size;

        if let Some(inst) = &part.instrument {
            if let Some(tuning) = &inst.tuning {
                for t in tuning {
                    tuning_map.insert(t.string, t.pitch.to_midi_note());
                }
            }
        }

        // Standard 12-TET E-tuning fallback
        let default_tuning = [
            (1, Pitch { step: "E".into(), alter: None, octave: 4 }.to_midi_note()),
            (2, Pitch { step: "B".into(), alter: None, octave: 3 }.to_midi_note()),
            (3, Pitch { step: "G".into(), alter: None, octave: 3 }.to_midi_note()),
            (4, Pitch { step: "D".into(), alter: None, octave: 3 }.to_midi_note()),
            (5, Pitch { step: "A".into(), alter: None, octave: 2 }.to_midi_note()),
            (6, Pitch { step: "E".into(), alter: None, octave: 2 }.to_midi_note()),
        ];
        
        for (s, midi) in default_tuning {
            tuning_map.entry(s).or_insert(midi);
        }

        for (m_idx, measure) in part.measures.iter().enumerate() {
            for seq in &measure.sequences {
                for event in &seq.content {
                    let mut active_strings = HashSet::new();

                    for note in &event.notes {
                        if let (Some(string), Some(fret)) = (note.string, note.fret) {
                            
                            // Rule 2: Concurrency check
                            if !active_strings.insert(string) {
                                errors.push(ValidationError {
                                    measure_idx: m_idx + 1,
                                    message: format!("Concurrency violation: String {} is plucked multiple times.", string),
                                });
                            }

                            // Extract the tuning details for the current string from your MNX Instrument mapping
                            // (Assuming string_tune is a reference to the StringTuning struct for this string)
                            let string_step = string_tune.pitch.step.chars().next().unwrap_or('C');
                            let string_octave = string_tune.pitch.octave;

                            let note_step = note.pitch.step.chars().next().unwrap_or('C');
                            let note_octave = note.pitch.octave;
                            let note_alter_semitones = note.pitch.alter.as_ref().map(|a| a.as_semitones()).unwrap_or(0.0);

                            let edo = instrument.micro_fret_divisions.unwrap_or(12) as u32;

                            // Default to 1 step per fret if the property is missing
                            let step_multiplier = instrument.fret_step_multiplier.unwrap_or(1) as i32; 

                            let tuning = EdoTuning::new(edo);
                            let e = edo as i32;

                            // 1. Calculate physical steps
                            let string_base = tuning.nominal_step(string_step) + string_octave * e;

                            // Multiply both the capo position and the played fret by the multiplier
                            let physical_capo_steps = (capo_fret as i32) * step_multiplier;
                            let physical_fret_steps = (fret as i32) * step_multiplier;

                            let played_steps = string_base + physical_capo_steps + physical_fret_steps;

                            // 2. Calculate semantic steps 
                            let expected_base = tuning.nominal_step(note_step) + (note_octave * e);
                            let alter_steps = (note_alter_semitones / tuning.step_semitones).round() as i32;
                            let expected_steps = expected_base + alter_steps;

                            // 3. Strict Integer Check 
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
                        }
                    }
                }
            }
        }
    }
    errors
}