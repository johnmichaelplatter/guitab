use crate::edo::EdoTuning;
use crate::mnx::{Alteration, Event, Harmonic, MnxDocument, Note, Part, Pitch, SequenceItem};
use crate::tuning_table::ReferencePitch;
use std::collections::HashMap;
use std::fmt;

/// Represents a single acoustic harmonic node for a given string.
#[derive(Debug, Clone, PartialEq)]
pub struct HarmonicNode {
    /// Harmonic number n (integer from 2 to 11)
    pub harmonic_number: u32,
    /// Node spot index k (integer from 1 to n - 1)
    pub node_index: u32,
    /// Fraction (k, n) along the vibrating string length
    pub fraction: (u32, u32),
    /// Unrounded exact fret position
    pub fret_exact: f64,
    /// Touch fret position rounded to the nearest tenth of a fret
    pub touch_fret: f64,
    /// Acoustic sounding frequency (n * f_open) in Hz
    pub sounding_frequency_hz: f64,
    /// Sounding pitch (step, alter, octave)
    pub sounding_pitch: Pitch,
}

/// A lookup table of valid harmonic nodes across all strings of an instrument setup.
#[derive(Debug, Clone)]
pub struct HarmonicLookupTable {
    pub edo: u32,
    pub step_multiplier: i32,
    pub capo: u8,
    pub reference_pitch: ReferencePitch,
    /// Mapped by string number (1-based) to the list of harmonic nodes
    pub nodes_by_string: HashMap<u8, Vec<HarmonicNode>>,
}

/// Calculates the exact touch fret position for harmonic `n` and node `k`.
///
/// Formula:
/// F = capo + (edo / step_multiplier) * log2(n / (n - k))
pub fn calculate_touch_fret_exact(
    n: u32,
    k: u32,
    edo: u32,
    step_multiplier: i32,
    capo: u8,
) -> f64 {
    let capo_f = capo as f64;
    let edo_f = edo as f64;
    let step_mult_f = step_multiplier as f64;
    let ratio = (n as f64) / ((n - k) as f64);
    capo_f + (edo_f / step_mult_f) * ratio.log2()
}

/// Calculates the touch fret position rounded to the nearest tenth of a fret.
pub fn calculate_touch_fret(
    n: u32,
    k: u32,
    edo: u32,
    step_multiplier: i32,
    capo: u8,
) -> f64 {
    let exact = calculate_touch_fret_exact(n, k, edo, step_multiplier, capo);
    (exact * 10.0).round() / 10.0
}

/// Computes the open string frequency in Hz.
pub fn calculate_open_string_frequency(
    open_pitch: &Pitch,
    edo: u32,
    step_multiplier: i32,
    capo: u8,
    ref_pitch: &ReferencePitch,
) -> f64 {
    let tuning_calc = EdoTuning::new(edo);
    let ref_steps = tuning_calc.pitch_to_steps(&ref_pitch.pitch);
    let string_base_steps = tuning_calc.pitch_to_steps(open_pitch);
    let capo_steps = (capo as i32) * step_multiplier;
    let open_steps = string_base_steps + capo_steps;
    ref_pitch.frequency_hz * 2.0_f64.powf((open_steps - ref_steps) as f64 / (edo as f64))
}

/// Computes the sounding frequency in Hz for harmonic `n` (f_sounding = n * f_open).
pub fn calculate_sounding_frequency(
    open_pitch: &Pitch,
    n: u32,
    edo: u32,
    step_multiplier: i32,
    capo: u8,
    ref_pitch: &ReferencePitch,
) -> f64 {
    let f_open = calculate_open_string_frequency(open_pitch, edo, step_multiplier, capo, ref_pitch);
    (n as f64) * f_open
}

/// Returns the diatonic step offset from the open string note for harmonic `n` (2..=11).
///
/// - n = 2, 4, 8: Octaves (offset 0: unison/octave)
/// - n = 9: Major Second / Ninth (offset 1)
/// - n = 5, 10: Major Third / Tenth (offset 2)
/// - n = 11: Eleventh / Tritone (offset 3: fourth)
/// - n = 3, 6: Perfect Fifth (offset 4)
/// - n = 7: Harmonic Seventh (offset 6)
pub fn diatonic_offset_for_harmonic(n: u32) -> usize {
    match n {
        2 | 4 | 8 => 0,
        9 => 1,
        5 | 10 => 2,
        11 => 3,
        3 | 6 => 4,
        7 => 6,
        _ => 0,
    }
}

/// Calculates the sounding pitch (step, alter, octave) for harmonic `n` on an open string.
///
/// `.alter` is rounded to the thousandths place (0.001) with respect to the frequency
/// of the base pitch in the current EDO.
pub fn calculate_sounding_pitch(
    open_pitch: &Pitch,
    n: u32,
    edo: u32,
    step_multiplier: i32,
    capo: u8,
    ref_pitch: &ReferencePitch,
) -> Pitch {
    let tuning_calc = EdoTuning::new(edo);
    let f_sounding =
        calculate_sounding_frequency(open_pitch, n, edo, step_multiplier, capo, ref_pitch);
    let ref_steps = tuning_calc.pitch_to_steps(&ref_pitch.pitch);

    let open_step = open_pitch
        .step
        .chars()
        .next()
        .unwrap_or('C')
        .to_ascii_uppercase();
    let nominals = ['C', 'D', 'E', 'F', 'G', 'A', 'B'];
    let open_idx = nominals.iter().position(|&c| c == open_step).unwrap_or(0);
    let diatonic_offset = diatonic_offset_for_harmonic(n);
    let target_step_char = nominals[(open_idx + diatonic_offset) % 7];

    // Frequency of base pitch (target step) at octave 0 with alter = 0 in current EDO:
    let base_steps_oct0 = tuning_calc.nominal_step(target_step_char);
    let f_base_oct0 = ref_pitch.frequency_hz
        * 2.0_f64.powf((base_steps_oct0 - ref_steps) as f64 / (edo as f64));

    // Choose the octave that minimizes alteration distance (|alter| <= 6 semitones):
    let octave = (f_sounding / f_base_oct0).log2().round() as i32;

    // Frequency of base pitch at the chosen octave:
    let base_steps = base_steps_oct0 + octave * (edo as i32);
    let f_base =
        ref_pitch.frequency_hz * 2.0_f64.powf((base_steps - ref_steps) as f64 / (edo as f64));

    // Alteration in standard semitones (12.0 * log2(f_sounding / f_base)):
    let raw_alter = 12.0 * (f_sounding / f_base).log2();
    let rounded_alter = (raw_alter * 1000.0).round() / 1000.0;

    let alter = if rounded_alter.abs() < 1e-4 {
        None
    } else {
        Some(Alteration::Value(rounded_alter))
    };

    Pitch {
        step: target_step_char.to_string(),
        alter,
        octave,
    }
}

impl HarmonicLookupTable {
    /// Constructs a lookup table for the specified instrument parameters and tunings.
    /// Loops through integers n = 2..=11 and k = 1..n - 1.
    pub fn new(
        edo: u32,
        step_multiplier: i32,
        capo: u8,
        tunings: &[(u8, Pitch)],
        ref_pitch: Option<ReferencePitch>,
    ) -> Self {
        let reference_pitch = ref_pitch.unwrap_or_default();
        let mut nodes_by_string = HashMap::new();

        for (string_idx, open_pitch) in tunings {
            let mut nodes = Vec::new();
            for n in 2..=11 {
                let sounding_pitch = calculate_sounding_pitch(
                    open_pitch,
                    n,
                    edo,
                    step_multiplier,
                    capo,
                    &reference_pitch,
                );
                let sounding_freq = calculate_sounding_frequency(
                    open_pitch,
                    n,
                    edo,
                    step_multiplier,
                    capo,
                    &reference_pitch,
                );

                for k in 1..n {
                    let fret_exact =
                        calculate_touch_fret_exact(n, k, edo, step_multiplier, capo);
                    let touch_fret = calculate_touch_fret(n, k, edo, step_multiplier, capo);

                    nodes.push(HarmonicNode {
                        harmonic_number: n,
                        node_index: k,
                        fraction: (k, n),
                        fret_exact,
                        touch_fret,
                        sounding_frequency_hz: sounding_freq,
                        sounding_pitch: sounding_pitch.clone(),
                    });
                }
            }
            nodes_by_string.insert(*string_idx, nodes);
        }

        Self {
            edo,
            step_multiplier,
            capo,
            reference_pitch,
            nodes_by_string,
        }
    }

    /// Constructs a harmonic lookup table directly from a `Part`.
    pub fn from_part(part: &Part) -> Self {
        let edo = part.edo();
        let step_multiplier = part.step_multiplier();
        let capo = part.capo_fret();
        let num_strings = part.num_strings();

        let mut tunings = Vec::new();
        for s in 1..=(num_strings as u8) {
            if let Some(open_pitch) = part.get_string_tuning(s) {
                tunings.push((s, open_pitch));
            }
        }

        Self::new(edo, step_multiplier, capo, &tunings, None)
    }

    /// Returns all harmonic nodes for a given string.
    pub fn get_nodes_for_string(&self, string: u8) -> Option<&[HarmonicNode]> {
        self.nodes_by_string.get(&string).map(|v| v.as_slice())
    }

    /// Finds all harmonic nodes matching a given touch fret on a string (within 0.05 fret tolerance).
    pub fn find_nodes_at_touch(&self, string: u8, touch: f64) -> Vec<&HarmonicNode> {
        if let Some(nodes) = self.nodes_by_string.get(&string) {
            nodes
                .iter()
                .filter(|node| (node.touch_fret - touch).abs() < 0.05)
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Validates a `Harmonic` definition on a specific string.
    ///
    /// Returns `Ok(&HarmonicNode)` if valid, or a descriptive `HarmonicValidationError` on failure.
    pub fn validate_harmonic(
        &self,
        string: u8,
        harmonic: &Harmonic,
    ) -> Result<&HarmonicNode, HarmonicValidationError> {
        let nodes = match self.nodes_by_string.get(&string) {
            Some(n) => n,
            None => {
                return Err(HarmonicValidationError {
                    measure_idx: None,
                    event_idx: None,
                    string: Some(string),
                    touch: Some(harmonic.touch),
                    message: format!("No tuning defined for String {}.", string),
                });
            }
        };

        let matching_nodes: Vec<&HarmonicNode> = nodes
            .iter()
            .filter(|node| (node.touch_fret - harmonic.touch).abs() < 0.05)
            .collect();

        if matching_nodes.is_empty() {
            return Err(HarmonicValidationError {
                measure_idx: None,
                event_idx: None,
                string: Some(string),
                touch: Some(harmonic.touch),
                message: format!(
                    "Touch fret {} on String {} does not correspond to any harmonic node (n = 2..11).",
                    harmonic.touch, string
                ),
            });
        }

        let actual_sounding_pitch = &harmonic.sounding_note.pitch;
        let actual_alter = actual_sounding_pitch
            .alter
            .as_ref()
            .map(|a| a.as_semitones())
            .unwrap_or(0.0);

        for node in &matching_nodes {
            let expected_pitch = &node.sounding_pitch;
            let expected_alter = expected_pitch
                .alter
                .as_ref()
                .map(|a| a.as_semitones())
                .unwrap_or(0.0);

            let step_match = expected_pitch
                .step
                .eq_ignore_ascii_case(&actual_sounding_pitch.step);
            let octave_match = expected_pitch.octave == actual_sounding_pitch.octave;
            let alter_match = (expected_alter - actual_alter).abs() < 0.0015;

            if step_match && octave_match && alter_match {
                return Ok(node);
            }
        }

        // None matched: build an informative error message showing the expected pitches
        let candidates: Vec<String> = matching_nodes
            .iter()
            .map(|node| {
                let alter_str = node
                    .sounding_pitch
                    .alter
                    .as_ref()
                    .map(|a| format!(" alter {:.3}", a.as_semitones()))
                    .unwrap_or_default();
                format!(
                    "{}{}{} (harmonic n={}, {}/{})",
                    node.sounding_pitch.step,
                    node.sounding_pitch.octave,
                    alter_str,
                    node.harmonic_number,
                    node.node_index,
                    node.harmonic_number
                )
            })
            .collect();

        let actual_alter_str = actual_sounding_pitch
            .alter
            .as_ref()
            .map(|a| format!(" alter {:.3}", a.as_semitones()))
            .unwrap_or_default();

        Err(HarmonicValidationError {
            measure_idx: None,
            event_idx: None,
            string: Some(string),
            touch: Some(harmonic.touch),
            message: format!(
                "Sounding note mismatch on String {} at touch fret {}. Found {}{}{}, but expected one of: [{}].",
                string,
                harmonic.touch,
                actual_sounding_pitch.step,
                actual_sounding_pitch.octave,
                actual_alter_str,
                candidates.join(", ")
            ),
        })
    }
}

/// Represents an error detected during harmonic validation.
#[derive(Debug, Clone, PartialEq)]
pub struct HarmonicValidationError {
    pub measure_idx: Option<usize>, // 1-indexed
    pub event_idx: Option<usize>,   // 0-indexed
    pub string: Option<u8>,
    pub touch: Option<f64>,
    pub message: String,
}

impl fmt::Display for HarmonicValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut loc = Vec::new();
        if let Some(m) = self.measure_idx {
            loc.push(format!("Measure {}", m));
        }
        if let Some(e) = self.event_idx {
            loc.push(format!("Event {}", e));
        }
        if let Some(s) = self.string {
            loc.push(format!("String {}", s));
        }
        if let Some(t) = self.touch {
            loc.push(format!("Touch {}", t));
        }

        if loc.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "[{}] {}", loc.join(", "), self.message)
        }
    }
}

impl std::error::Error for HarmonicValidationError {}

/// Validates a single `Note` if it contains a `harmonic`.
pub fn validate_note_harmonic(
    note: &Note,
    table: &HarmonicLookupTable,
    measure_idx: Option<usize>,
    event_idx: Option<usize>,
) -> Option<HarmonicValidationError> {
    let harmonic = note.harmonic.as_ref()?;
    let string = match note.string {
        Some(s) => s,
        None => {
            return Some(HarmonicValidationError {
                measure_idx,
                event_idx,
                string: None,
                touch: Some(harmonic.touch),
                message: format!(
                    "Note with harmonic touch {} has no string specified.",
                    harmonic.touch
                ),
            });
        }
    };

    match table.validate_harmonic(string, harmonic) {
        Ok(_) => None,
        Err(mut err) => {
            err.measure_idx = measure_idx;
            err.event_idx = event_idx;
            Some(err)
        }
    }
}

/// Validates all harmonics in an `Event`.
pub fn validate_event_harmonics(
    event: &Event,
    table: &HarmonicLookupTable,
    measure_idx: Option<usize>,
    event_idx: Option<usize>,
) -> Vec<HarmonicValidationError> {
    let mut errors = Vec::new();
    for note in &event.notes {
        if let Some(err) = validate_note_harmonic(note, table, measure_idx, event_idx) {
            errors.push(err);
        }
    }
    errors
}

/// Validates all harmonics present in a `Part`.
pub fn validate_harmonics_in_part(part: &Part) -> Vec<HarmonicValidationError> {
    let table = HarmonicLookupTable::from_part(part);
    let mut errors = Vec::new();

    for (m_idx, measure) in part.measures.iter().enumerate() {
        for sequence in &measure.sequences {
            let mut event_idx = 0;
            for item in &sequence.content {
                match item {
                    SequenceItem::Event(event) => {
                        errors.extend(validate_event_harmonics(
                            event,
                            &table,
                            Some(m_idx + 1),
                            Some(event_idx),
                        ));
                        event_idx += 1;
                    }
                    SequenceItem::Tuplet(tuplet) => {
                        for inner in &tuplet.content {
                            if let SequenceItem::Event(event) = inner {
                                errors.extend(validate_event_harmonics(
                                    event,
                                    &table,
                                    Some(m_idx + 1),
                                    Some(event_idx),
                                ));
                                event_idx += 1;
                            }
                        }
                    }
                    SequenceItem::Grace(grace) => {
                        for inner in &grace.content {
                            if let SequenceItem::Event(event) = inner {
                                errors.extend(validate_event_harmonics(
                                    event,
                                    &table,
                                    Some(m_idx + 1),
                                    Some(event_idx),
                                ));
                                event_idx += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    errors
}

/// Validates all harmonics present across all parts of an `MnxDocument`.
pub fn validate_harmonics_in_doc(doc: &MnxDocument) -> Vec<HarmonicValidationError> {
    let mut errors = Vec::new();
    for part in &doc.parts {
        errors.extend(validate_harmonics_in_part(part));
    }
    errors
}

/// Validates all harmonics in a YAML document string.
pub fn validate_harmonics_in_yaml(yaml_str: &str) -> Result<Vec<HarmonicValidationError>, String> {
    let doc: MnxDocument =
        serde_saphyr::from_str(yaml_str).map_err(|e| format!("YAML parse error: {}", e))?;
    Ok(validate_harmonics_in_doc(&doc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_guitar_harmonics_math() {
        let e4 = Pitch {
            step: "E".to_string(),
            alter: None,
            octave: 4,
        };
        let ref_pitch = ReferencePitch::default();
        let edo = 12;
        let step_mult = 1;
        let capo = 0;

        // n = 2, k = 1 (1/2): Fret 12.0 -> Sounding E5 (alter None)
        let fret_2 = calculate_touch_fret(2, 1, edo, step_mult, capo);
        assert_eq!(fret_2, 12.0);
        let pitch_2 = calculate_sounding_pitch(&e4, 2, edo, step_mult, capo, &ref_pitch);
        assert_eq!(pitch_2.step, "E");
        assert_eq!(pitch_2.octave, 5);
        assert_eq!(pitch_2.alter, None);

        // n = 3, k = 1 (1/3): Fret 7.0 -> Sounding B5 (alter +0.02)
        let fret_3_1 = calculate_touch_fret(3, 1, edo, step_mult, capo);
        assert_eq!(fret_3_1, 7.0);
        let pitch_3 = calculate_sounding_pitch(&e4, 3, edo, step_mult, capo, &ref_pitch);
        assert_eq!(pitch_3.step, "B");
        assert_eq!(pitch_3.octave, 5);
        assert_eq!(pitch_3.alter, Some(Alteration::Value(0.02)));

        // n = 3, k = 2 (2/3): Fret 19.0 -> Sounding B5 (alter +0.02)
        let fret_3_2 = calculate_touch_fret(3, 2, edo, step_mult, capo);
        assert_eq!(fret_3_2, 19.0);

        // n = 4, k = 1 (1/4): Fret 5.0 -> Sounding E6
        let fret_4_1 = calculate_touch_fret(4, 1, edo, step_mult, capo);
        assert_eq!(fret_4_1, 5.0);
        let pitch_4 = calculate_sounding_pitch(&e4, 4, edo, step_mult, capo, &ref_pitch);
        assert_eq!(pitch_4.step, "E");
        assert_eq!(pitch_4.octave, 6);
        assert_eq!(pitch_4.alter, None);

        // n = 5, k = 1 (1/5): Fret 3.9 -> Sounding G6 (alter +0.863)
        let fret_5_1 = calculate_touch_fret(5, 1, edo, step_mult, capo);
        assert_eq!(fret_5_1, 3.9);
        let pitch_5 = calculate_sounding_pitch(&e4, 5, edo, step_mult, capo, &ref_pitch);
        assert_eq!(pitch_5.step, "G");
        assert_eq!(pitch_5.octave, 6);
        assert_eq!(pitch_5.alter, Some(Alteration::Value(0.863)));

        // n = 7, k = 1 (1/7): Fret 2.7 -> Sounding D7 (alter -0.312)
        let fret_7_1 = calculate_touch_fret(7, 1, edo, step_mult, capo);
        assert_eq!(fret_7_1, 2.7);
        let pitch_7 = calculate_sounding_pitch(&e4, 7, edo, step_mult, capo, &ref_pitch);
        assert_eq!(pitch_7.step, "D");
        assert_eq!(pitch_7.octave, 7);
        assert_eq!(pitch_7.alter, Some(Alteration::Value(-0.312)));

        // n = 9, k = 1 (1/9): Fret 2.0 -> Sounding F7 (alter +1.039)
        let fret_9_1 = calculate_touch_fret(9, 1, edo, step_mult, capo);
        assert_eq!(fret_9_1, 2.0);
        let pitch_9 = calculate_sounding_pitch(&e4, 9, edo, step_mult, capo, &ref_pitch);
        assert_eq!(pitch_9.step, "F");
        assert_eq!(pitch_9.octave, 7);
        assert_eq!(pitch_9.alter, Some(Alteration::Value(1.039)));
    }

    #[test]
    fn test_lookup_table_validation() {
        let tunings = vec![
            (
                1,
                Pitch {
                    step: "E".to_string(),
                    alter: None,
                    octave: 4,
                },
            ),
            (
                2,
                Pitch {
                    step: "B".to_string(),
                    alter: None,
                    octave: 3,
                },
            ),
        ];
        let table = HarmonicLookupTable::new(12, 1, 0, &tunings, None);

        // Valid harmonic at 12.0
        let h_valid = Harmonic {
            touch: 12.0,
            sounding_note: Box::new(Note {
                id: None,
                pitch: Pitch {
                    step: "E".to_string(),
                    alter: None,
                    octave: 5,
                },
                accidental_display: None,
                ties: None,
                comment: None,
                ext: crate::mnx::NoteExt {
                    string: Some(1),
                    fret: Some(12),
                    ..Default::default()
                },
            }),
        };
        assert!(table.validate_harmonic(1, &h_valid).is_ok());

        // Invalid touch fret
        let h_invalid_touch = Harmonic {
            touch: 4.5,
            sounding_note: Box::new(Note {
                id: None,
                pitch: Pitch {
                    step: "E".to_string(),
                    alter: None,
                    octave: 5,
                },
                accidental_display: None,
                ties: None,
                comment: None,
                ext: crate::mnx::NoteExt {
                    string: Some(1),
                    fret: Some(5),
                    ..Default::default()
                },
            }),
        };
        assert!(table.validate_harmonic(1, &h_invalid_touch).is_err());

        // Invalid sounding pitch at 12.0
        let h_invalid_pitch = Harmonic {
            touch: 12.0,
            sounding_note: Box::new(Note {
                id: None,
                pitch: Pitch {
                    step: "C".to_string(),
                    alter: None,
                    octave: 5,
                },
                accidental_display: None,
                ties: None,
                comment: None,
                ext: crate::mnx::NoteExt {
                    string: Some(1),
                    fret: Some(12),
                    ..Default::default()
                },
            }),
        };
        assert!(table.validate_harmonic(1, &h_invalid_pitch).is_err());
    }

    #[test]
    fn test_validate_example_harmonics_yaml_file() {
        let yaml_str = include_str!("../example_harmonics.yaml");
        let doc: MnxDocument = serde_saphyr::from_str(yaml_str).expect("Valid YAML syntax");
        assert_eq!(doc.parts.len(), 1);
        let part = &doc.parts[0];
        assert_eq!(part.measures.len(), 3);

        let table = HarmonicLookupTable::from_part(part);

        // Measure 1 (index 0): All valid harmonics
        let mut m1_errors = Vec::new();
        for seq in &part.measures[0].sequences {
            for (e_idx, item) in seq.content.iter().enumerate() {
                if let SequenceItem::Event(e) = item {
                    m1_errors.extend(validate_event_harmonics(&e, &table, Some(1), Some(e_idx)));
                }
            }
        }
        assert!(
            m1_errors.is_empty(),
            "Measure 1 should have 0 harmonic errors, found: {:?}",
            m1_errors
        );

        // Measure 2 (index 1): All valid harmonics
        let mut m2_errors = Vec::new();
        for seq in &part.measures[1].sequences {
            for (e_idx, item) in seq.content.iter().enumerate() {
                if let SequenceItem::Event(e) = item {
                    m2_errors.extend(validate_event_harmonics(&e, &table, Some(2), Some(e_idx)));
                }
            }
        }
        assert!(
            m2_errors.is_empty(),
            "Measure 2 should have 0 harmonic errors, found: {:?}",
            m2_errors
        );

        // Measure 3 (index 2): Exactly 4 invalid harmonics
        let mut m3_errors = Vec::new();
        for seq in &part.measures[2].sequences {
            for (e_idx, item) in seq.content.iter().enumerate() {
                if let SequenceItem::Event(e) = item {
                    m3_errors.extend(validate_event_harmonics(&e, &table, Some(3), Some(e_idx)));
                }
            }
        }
        assert_eq!(
            m3_errors.len(),
            4,
            "Measure 3 should have exactly 4 invalid harmonics errors, found: {:?}",
            m3_errors
        );

        // Check each error in Measure 3
        assert!(
            m3_errors[0].message.contains("does not correspond to any harmonic node"),
            "Error 1 should be invalid touch fret: {}",
            m3_errors[0].message
        );
        assert_eq!(m3_errors[0].touch, Some(4.5));

        assert!(
            m3_errors[1].message.contains("Sounding note mismatch"),
            "Error 2 should be step mismatch: {}",
            m3_errors[1].message
        );
        assert!(m3_errors[1].message.contains("Found C5"));

        assert!(
            m3_errors[2].message.contains("Sounding note mismatch"),
            "Error 3 should be alter mismatch: {}",
            m3_errors[2].message
        );
        assert!(m3_errors[2].message.contains("0.500"));

        assert!(
            m3_errors[3].message.contains("Sounding note mismatch"),
            "Error 4 should be octave mismatch: {}",
            m3_errors[3].message
        );
        assert!(m3_errors[3].message.contains("Found E4"));

        // Test validate_harmonics_in_yaml wrapper
        let doc_errors = validate_harmonics_in_yaml(yaml_str).expect("Parsed YAML");
        assert_eq!(doc_errors.len(), 4);
    }

    #[test]
    fn test_capo_and_step_multiplier() {
        // Capo at fret 2, 12-EDO, step_multiplier = 1
        // At n = 2, k = 1 (1/2): F = capo + 12 * log2(2) = 2 + 12 = 14.0 fret
        let fret_capo = calculate_touch_fret(2, 1, 12, 1, 2);
        assert_eq!(fret_capo, 14.0);

        // Kite Guitar: 41-EDO, step_multiplier = 2, capo = 0
        // At n = 2, k = 1: F = 0 + (41/2) * log2(2) = 20.5 fret
        let fret_kite = calculate_touch_fret(2, 1, 41, 2, 0);
        assert_eq!(fret_kite, 20.5);
    }
}
