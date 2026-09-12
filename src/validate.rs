use crate::mnx::{MnxDocument, Pitch};
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

                            // Rule 1 & 3: Microtonal Pitch math check
                            if let Some(&base_midi) = tuning_map.get(&string) {
                                let physical_midi = base_midi + capo_offset + ((fret as f64) * step_size);
                                let semantic_midi = note.pitch.to_midi_note();

                                // Allow a margin of error of 0.002 for 3-decimal precision rounding in YAML
                                if (physical_midi - semantic_midi).abs() > 0.002 {
                                    let alter_str = note.pitch.alter.map(|a| format!(" alter {}", a)).unwrap_or_default();
                                    
                                    errors.push(ValidationError {
                                        measure_idx: m_idx + 1,
                                        message: format!(
                                            "Geometry mismatch on String {} Fret {}. \
                                            Physically produces MIDI {:.3} (Base {:.3} + capo {} + fret {} at {:.3} semitones/fret). \
                                            MNX data expects {}{}{} (MIDI {:.3}).",
                                            string, fret, 
                                            physical_midi, base_midi, capo_fret, fret, step_size,
                                            note.pitch.step, note.pitch.octave, alter_str, semantic_midi
                                        ),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    errors
}