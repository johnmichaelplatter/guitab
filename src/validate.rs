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
        // 1. Establish tuning map (String index -> Base MIDI Note)
        let mut tuning_map: HashMap<u8, i32> = HashMap::new();
        let capo_offset = part.instrument.as_ref().and_then(|i| i.capo).unwrap_or(0) as i32;

        if let Some(inst) = &part.instrument {
            if let Some(tuning) = &inst.tuning {
                for t in tuning {
                    tuning_map.insert(t.string, t.pitch.to_midi_note());
                }
            }
        }

        // Standard E tuning fallback if string definitions are missing
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

        // 2. Scan physical constraints measure by measure
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
                                    message: format!("Concurrency violation: String {} is plucked multiple times in the same beat.", string),
                                });
                            }

                            // Rule 1 & 3: Pitch math check (Base + Capo + Fret == MNX Pitch)
                            if let Some(&base_midi) = tuning_map.get(&string) {
                                let physical_midi = base_midi + capo_offset + (fret as i32);
                                let semantic_midi = note.pitch.to_midi_note();

                                if physical_midi != semantic_midi {
                                    let physical_pitch = Pitch::from_midi_note(physical_midi);
                                    let alt_str = physical_pitch.alter.map(|a| if a > 0 { "#" } else { "b" }).unwrap_or("");
                                    
                                    errors.push(ValidationError {
                                        measure_idx: m_idx + 1,
                                        message: format!(
                                            "Geometry mismatch on String {} Fret {}. \
                                            Physically produces {}{}{} (MIDI {}), but MNX semantic data expects {}{} (MIDI {}).",
                                            string, fret, 
                                            physical_pitch.step, alt_str, physical_pitch.octave, physical_midi,
                                            note.pitch.step, note.pitch.octave, semantic_midi
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