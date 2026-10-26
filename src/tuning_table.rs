use crate::edo::EdoTuning;
use crate::mnx::{Part, Pitch};
use std::collections::HashMap;

/// Configuration for the reference pitch (e.g., A4 = 440 Hz, D4 = 293.66 Hz, etc.).
#[derive(Debug, Clone, PartialEq)]
pub struct ReferencePitch {
    pub pitch: Pitch,
    pub frequency_hz: f64,
}

impl Default for ReferencePitch {
    fn default() -> Self {
        // Standard concert pitch A4 = 440.0 Hz
        Self {
            pitch: Pitch {
                step: "A".to_string(),
                alter: None,
                octave: 4,
            },
            frequency_hz: 440.0,
        }
    }
}

impl ReferencePitch {
    pub fn new_a4_440() -> Self {
        Self::default()
    }

    pub fn new_d4_293_66() -> Self {
        Self {
            pitch: Pitch {
                step: "D".to_string(),
                alter: None,
                octave: 4,
            },
            frequency_hz: 293.664767968, // (440.0 * 2^(-7/12))
        }
    }
}

/// Represents the calculated state for a single (string, fret) position on the fretboard.
#[derive(Debug, Clone, PartialEq)]
pub struct FretboardPositionState {
    pub string: u8,
    pub fret: i32,
    pub played_steps: i32,
    pub pitch: Pitch,
    pub frequency_hz: f64,
    pub midi_note_number: f64,
}

/// A comprehensive lookup table for all string and fret positions on the fretboard.
#[derive(Debug, Clone)]
pub struct FretboardFrequencyTable {
    pub edo: u32,
    pub step_multiplier: i32,
    pub capo: u8,
    pub num_strings: usize,
    pub max_fret: u8,
    pub reference_pitch: ReferencePitch,
    pub ref_steps: i32,
    /// Quick lookup mapped by (string, fret)
    pub table: HashMap<(u8, i32), FretboardPositionState>,
}

impl FretboardFrequencyTable {
    /// Builds the fretboard frequency state table from a `Part` and a `ReferencePitch` across frets `0..=max_fret`.
    ///
    /// Inputs leveraged from `part.instrument`:
    /// - `microFretDivisions` (`part.edo()`)
    /// - `fretStepMultiplier` (`part.step_multiplier()`)
    /// - `capo` (`part.capo_fret()`)
    /// - open string tunings (`part.get_string_tuning(string)`)
    pub fn from_part(part: &Part, reference_pitch: ReferencePitch, max_fret: u8) -> Self {
        let edo = part.edo();
        let step_multiplier = part.step_multiplier();
        let capo = part.capo_fret();
        let num_strings = part.num_strings();
        let tuning_calc = EdoTuning::new(edo);

        let ref_steps = tuning_calc.pitch_to_steps(&reference_pitch.pitch);
        let mut table = HashMap::new();

        for string_idx in 1..=(num_strings as u8) {
            if let Some(open_pitch) = part.get_string_tuning(string_idx) {
                let string_base_steps = tuning_calc.pitch_to_steps(&open_pitch);

                for fret in 0..=max_fret {
                    let physical_capo_steps = (capo as i32) * step_multiplier;
                    let physical_fret_steps = (fret as i32) * step_multiplier;
                    let played_steps =
                        string_base_steps + physical_capo_steps + physical_fret_steps;

                    // Standard EDO frequency math:
                    // f = f_ref * 2 ^ ((played_steps - ref_steps) / edo)
                    let step_delta = (played_steps - ref_steps) as f64;
                    let frequency_hz =
                        reference_pitch.frequency_hz * (2.0_f64).powf(step_delta / (edo as f64));

                    // MIDI continuous note number based on 12-TET A440 (where A4 = 440 Hz = MIDI 69)
                    let midi_note_number = 69.0 + 12.0 * (frequency_hz / 440.0).log2();

                    // Resolve the standard MNX Pitch representation
                    let pitch = tuning_calc.steps_to_pitch(played_steps);

                    let state = FretboardPositionState {
                        string: string_idx,
                        fret: fret as i32,
                        played_steps,
                        pitch,
                        frequency_hz,
                        midi_note_number,
                    };

                    table.insert((string_idx, fret as i32), state);
                }
            }
        }

        Self {
            edo,
            step_multiplier,
            capo,
            num_strings,
            max_fret,
            reference_pitch,
            ref_steps,
            table,
        }
    }

    /// Convenience constructor with default A4 = 440 Hz and dynamic max fret (2 * EDO).
    pub fn from_part_default(part: &Part) -> Self {
        let max_fret = (part.edo() * 2).min(255) as u8;
        Self::from_part(part, ReferencePitch::default(), max_fret)
    }

    /// Query the state for a given string and fret.
    pub fn get(&self, string: u8, fret: i32) -> Option<&FretboardPositionState> {
        self.table.get(&(string, fret))
    }

    /// Query the Hz for a given string and fret.
    pub fn get_hz(&self, string: u8, fret: i32) -> Option<f64> {
        self.get(string, fret).map(|s| s.frequency_hz)
    }

    /// Compute frequency in Hz directly for any pitch in this EDO space using the reference pitch,
    /// accurately handling microtonal and decimal alterations.
    pub fn pitch_to_hz(&self, pitch: &Pitch) -> f64 {
        let tuning_calc = EdoTuning::new(self.edo);
        let step_char = pitch
            .step
            .chars()
            .next()
            .unwrap_or('C')
            .to_ascii_uppercase();
        let base_steps = tuning_calc.nominal_step(step_char) + (pitch.octave * (self.edo as i32));
        let step_delta = (base_steps - self.ref_steps) as f64;
        let f_base =
            self.reference_pitch.frequency_hz * (2.0_f64).powf(step_delta / (self.edo as f64));
        let alter_semitones = pitch
            .alter
            .as_ref()
            .map(|a| a.as_semitones())
            .unwrap_or(0.0);
        f_base * (2.0_f64).powf(alter_semitones / 12.0)
    }

    /// Compute continuous MIDI note number (e.g. 69.0 for A440) for any pitch in this EDO space.
    pub fn pitch_to_midi_note_number(&self, pitch: &Pitch) -> f64 {
        let hz = self.pitch_to_hz(pitch);
        69.0 + 12.0 * (hz / 440.0).log2()
    }

    /// Compute frequency in Hz directly for any played step integer in this EDO space.
    pub fn steps_to_hz(&self, steps: i32) -> f64 {
        let step_delta = (steps - self.ref_steps) as f64;
        self.reference_pitch.frequency_hz * (2.0_f64).powf(step_delta / (self.edo as f64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mnx::MnxDocument;

    #[test]
    fn test_standard_guitar_12edo() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures: []
parts:
  - instrument:
      name: "Standard Guitar"
      microFretDivisions: 12
      fretStepMultiplier: 1
      capo: 0
    measures: []
"#;
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let part = &doc.parts[0];
        let table = FretboardFrequencyTable::from_part_default(part);

        // A4 is String 1 (E4) Fret 5
        let a4_state = table.get(1, 5).expect("String 1 Fret 5 should exist");
        assert!(
            (a4_state.frequency_hz - 440.0).abs() < 1e-4,
            "A4 should be 440 Hz, got {}",
            a4_state.frequency_hz
        );
        assert!(
            (a4_state.midi_note_number - 69.0).abs() < 1e-4,
            "A4 should be MIDI 69"
        );

        // Open string 1 E4 is ~329.6276 Hz (MIDI 64)
        let e4_state = table.get(1, 0).expect("String 1 Fret 0 should exist");
        assert!(
            (e4_state.frequency_hz - 329.6276).abs() < 0.01,
            "E4 should be ~329.63 Hz, got {}",
            e4_state.frequency_hz
        );
        assert!(
            (e4_state.midi_note_number - 64.0).abs() < 1e-4,
            "E4 should be MIDI 64"
        );

        // String 5 Open is A2 = 110 Hz (MIDI 45)
        let a2_state = table.get(5, 0).expect("String 5 Fret 0 should exist");
        assert!(
            (a2_state.frequency_hz - 110.0).abs() < 1e-4,
            "A2 should be 110 Hz, got {}",
            a2_state.frequency_hz
        );
        assert!(
            (a2_state.midi_note_number - 45.0).abs() < 1e-4,
            "A2 should be MIDI 45"
        );

        // String 5 Fret 12 is A3 = 220 Hz (MIDI 57)
        let a3_state = table.get(5, 12).expect("String 5 Fret 12 should exist");
        assert!(
            (a3_state.frequency_hz - 220.0).abs() < 1e-4,
            "A3 should be 220 Hz, got {}",
            a3_state.frequency_hz
        );
    }

    #[test]
    fn test_kite_guitar_41edo_frequency_table() {
        let yaml = include_str!("../examples/example.yaml");
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let part = &doc.parts[0];
        let table = FretboardFrequencyTable::from_part(part, ReferencePitch::default(), 24);

        assert_eq!(table.edo, 41);
        assert_eq!(table.step_multiplier, 2);
        assert_eq!(table.num_strings, 8);
        assert_eq!(table.max_fret, 24);

        // String 6 Open is vA2 (A2 alter -1\41).
        // Let's verify Hz calculation with EDO formula:
        // A4 = 440 Hz (step = 3 * generator + 4 * 41 = 3 * 24 + 164 = 236 in 41-EDO)
        // String 6 open step = nominal_step('A') + 2 * 41 - 1 = 72 + 82 - 1 = 153.
        // delta = 153 - 236 = -83 steps.
        // f = 440.0 * 2^(-83 / 41) = 440.0 * 2^(-2.02439024) = 108.069 Hz.
        let string6_fret0 = table.get(6, 0).expect("String 6 Fret 0 should exist");
        let expected_f = 440.0 * (2.0_f64).powf(-83.0 / 41.0);
        assert!((string6_fret0.frequency_hz - expected_f).abs() < 1e-6);

        // Geometry match check with example note: String 7 Fret 12
        let string7_fret12 = table.get(7, 12).expect("String 7 Fret 12 should exist");
        assert_eq!(string7_fret12.pitch.step, "C");
        assert_eq!(string7_fret12.pitch.octave, 3);
        assert!(string7_fret12.pitch.alter.is_none());
    }

    #[test]
    fn test_capo_and_custom_reference_frequency() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures: []
parts:
  - name: "Capo Guitar"
    _x:
      microFretDivisions: 12
      fretStepMultiplier: 1
      capo: 2
    measures: []
"#;
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let part = &doc.parts[0];

        // Use custom reference frequency: A4 = 432 Hz
        let ref_pitch = ReferencePitch {
            pitch: Pitch {
                step: "A".to_string(),
                alter: None,
                octave: 4,
            },
            frequency_hz: 432.0,
        };
        let table = FretboardFrequencyTable::from_part(part, ref_pitch, 12);

        // With capo 2 on standard guitar:
        // String 5 open pitch is A2. With capo 2 + fret 0 -> effectively B2 (2 semitones up from A2).
        // f = 432.0 * 2^((nominal_step('B') + 24 - (nominal_step('A') + 48)) / 12) = 432.0 * 2^(-22 / 12)
        let s5_f0 = table.get(5, 0).expect("S5 F0");
        let expected_f = 432.0 * (2.0_f64).powf(-22.0 / 12.0);
        assert!((s5_f0.frequency_hz - expected_f).abs() < 1e-6);
        assert_eq!(s5_f0.pitch.step, "B");
        assert_eq!(s5_f0.pitch.octave, 2);
    }

    #[test]
    fn test_pitch_to_midi_note_number() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures: []
parts:
  - instrument:
      name: "Standard Guitar"
      microFretDivisions: 12
      fretStepMultiplier: 1
    measures: []
"#;
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let part = &doc.parts[0];
        let table = FretboardFrequencyTable::from_part_default(part);

        let a4 = Pitch {
            step: "A".to_string(),
            alter: None,
            octave: 4,
        };
        assert!((table.pitch_to_midi_note_number(&a4) - 69.0).abs() < 1e-4);

        let e4 = Pitch {
            step: "E".to_string(),
            alter: None,
            octave: 4,
        };
        assert!((table.pitch_to_midi_note_number(&e4) - 64.0).abs() < 1e-4);

        let b5_altered = Pitch {
            step: "B".to_string(),
            alter: Some(crate::mnx::Alteration::Value(0.020)),
            octave: 5,
        };
        assert!((table.pitch_to_midi_note_number(&b5_altered) - 83.020).abs() < 1e-4);
    }
}
