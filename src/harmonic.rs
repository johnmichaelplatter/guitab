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
        ext: crate::mnx::PitchExt::default(),
    }
}

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

/// Computes the exact open string frequency in Hz accurately for standard EDO pitches,
/// microtonal alterations, ratio steps (e.g. 4/3), and cents steps (e.g. 498.0).
pub fn calculate_open_string_frequency_accurate(
    open_pitch: &Pitch,
    ref_pitch: &ReferencePitch,
) -> f64 {
    if let Some(open_cents) = open_pitch.step_cents_above_reference() {
        let oct_delta = open_pitch.octave - ref_pitch.pitch.octave;
        let alter_semitones = open_pitch
            .alter
            .as_ref()
            .map(|a| a.as_semitones())
            .unwrap_or(0.0);
        ref_pitch.frequency_hz
            * 2.0_f64.powf((open_cents / 1200.0) + (oct_delta as f64) + (alter_semitones / 12.0))
    } else {
        let tuning_calc = EdoTuning::new(12);
        let step_char = crate::fretboard_editor::extract_nominal_from_note_name(&open_pitch.step);
        let base_steps =
            tuning_calc.nominal_step(step_char) + (open_pitch.octave * 12);
        let ref_steps = tuning_calc.pitch_to_steps(&ref_pitch.pitch);
        let step_delta = (base_steps - ref_steps) as f64;
        let f_base = ref_pitch.frequency_hz * 2.0_f64.powf(step_delta / 12.0);
        let alter_semitones = open_pitch
            .alter
            .as_ref()
            .map(|a| a.as_semitones())
            .unwrap_or(0.0);
        f_base * 2.0_f64.powf(alter_semitones / 12.0)
    }
}

/// Calculates the sounding pitch (step, alter, octave, custom name) for a given sounding frequency
/// produced by harmonic `n` on `open_pitch`.
pub fn calculate_sounding_pitch_for_frequency(
    f_sounding: f64,
    open_pitch: &Pitch,
    n: u32,
    ref_pitch: &ReferencePitch,
) -> Pitch {
    let open_step_char = crate::fretboard_editor::extract_nominal_from_note_name(&open_pitch.step);
    let nominals = ['C', 'D', 'E', 'F', 'G', 'A', 'B'];
    let open_idx = nominals.iter().position(|&c| c == open_step_char).unwrap_or(0);
    let diatonic_offset = diatonic_offset_for_harmonic(n);
    let target_step_char = nominals[(open_idx + diatonic_offset) % 7];

    let tuning_calc = EdoTuning::new(12);
    let ref_steps = tuning_calc.pitch_to_steps(&ref_pitch.pitch);
    let base_steps_oct0 = tuning_calc.nominal_step(target_step_char);
    let f_base_oct0 = ref_pitch.frequency_hz
        * 2.0_f64.powf((base_steps_oct0 - ref_steps) as f64 / 12.0);

    let octave = (f_sounding / f_base_oct0).log2().round() as i32;
    let base_steps = base_steps_oct0 + octave * 12;
    let f_base =
        ref_pitch.frequency_hz * 2.0_f64.powf((base_steps - ref_steps) as f64 / 12.0);
    let raw_alter = 12.0 * (f_sounding / f_base).log2();
    let rounded_alter = (raw_alter * 1000.0).round() / 1000.0;

    let alter = if rounded_alter.abs() < 1e-4 {
        None
    } else {
        Some(Alteration::Value(rounded_alter))
    };

    let mut ext = crate::mnx::PitchExt::default();
    if ref_pitch.frequency_hz > 0.0 {
        let cents_above_ref = 1200.0 * (f_sounding / ref_pitch.frequency_hz).log2();
        let ref_nominal =
            crate::fretboard_editor::extract_nominal_from_note_name(&ref_pitch.pitch.step);
        let rec_name = crate::fretboard_editor::recommend_53edo_name_from_reference(
            &ref_nominal.to_string(),
            cents_above_ref,
        );
        ext.name = Some(rec_name);
    }

    Pitch {
        step: target_step_char.to_string(),
        alter,
        octave,
        ext,
    }
}

/// Given a cents position `cents` above open string, returns the exact fret coordinate along the neck
/// defined by `points` where each point is `(cents, fret_number)` sorted by ascending cents.
pub fn cents_to_fret_exact(cents: f64, points: &[(f64, f64)]) -> f64 {
    if points.is_empty() || cents <= 0.0 {
        return 0.0;
    }

    if let Some(pos) = points.iter().position(|&(c, _)| c >= cents) {
        if pos == 0 {
            return points[0].1;
        }
        let (c_left, f_left) = points[pos - 1];
        let (c_right, f_right) = points[pos];
        if c_right > c_left + 1e-6 {
            let frac = (cents - c_left) / (c_right - c_left);
            return f_left + frac * (f_right - f_left);
        } else {
            return f_left;
        }
    }

    // Beyond highest point: extrapolate using last interval or average
    let n = points.len();
    if n >= 2 {
        let (c_prev, f_prev) = points[n - 2];
        let (c_last, f_last) = points[n - 1];
        if c_last > c_prev + 1e-6 {
            let delta_c = c_last - c_prev;
            let delta_f = f_last - f_prev;
            return f_last + ((cents - c_last) / delta_c) * delta_f;
        }
    }
    let (c_last, f_last) = points[n - 1];
    if c_last > 1e-6 {
        cents * (f_last / c_last)
    } else {
        cents / 100.0
    }
}

/// Given a fret coordinate `fret`, returns the cents position above open string
/// defined by `points` where each point is `(cents, fret_number)` sorted by ascending fret_number.
pub fn fret_to_cents(fret: f64, points: &[(f64, f64)]) -> f64 {
    if points.is_empty() || fret <= 0.0 {
        return 0.0;
    }

    if let Some(pos) = points.iter().position(|&(_, f)| f >= fret) {
        if pos == 0 {
            return points[0].0;
        }
        let (c_left, f_left) = points[pos - 1];
        let (c_right, f_right) = points[pos];
        if f_right > f_left + 1e-6 {
            let frac = (fret - f_left) / (f_right - f_left);
            return c_left + frac * (c_right - c_left);
        } else {
            return c_left;
        }
    }

    let n = points.len();
    if n >= 2 {
        let (c_prev, f_prev) = points[n - 2];
        let (c_last, f_last) = points[n - 1];
        if f_last > f_prev + 1e-6 {
            let delta_c = c_last - c_prev;
            let delta_f = f_last - f_prev;
            return c_last + ((fret - f_last) / delta_f) * delta_c;
        }
    }
    let (c_last, f_last) = points[n - 1];
    if f_last > 1e-6 {
        fret * (c_last / f_last)
    } else {
        fret * 100.0
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

    fn new_custom_fretboard(
        part: &Part,
        custom_frets: &[crate::fretboard_editor::CustomFret],
        tunings: &[(u8, Pitch)],
        reference_pitch: ReferencePitch,
    ) -> Self {
        let mut nodes_by_string = HashMap::new();

        for (string_idx, open_pitch) in tunings {
            let mut nodes = Vec::new();
            let f_open = calculate_open_string_frequency_accurate(open_pitch, &reference_pitch);

            // Filter to custom frets installed and playable on this specific string
            let mut playable_frets: Vec<&crate::fretboard_editor::CustomFret> = custom_frets
                .iter()
                .filter(|cf| cf.has_string(*string_idx))
                .collect();
            playable_frets.sort_by(|a, b| {
                a.cents()
                    .partial_cmp(&b.cents())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            // Control points along the string: Nut at (0.0 cents, 0.0 fret), followed by installed frets
            let mut points: Vec<(f64, f64)> = vec![(0.0, 0.0)];
            for cf in &playable_frets {
                points.push((cf.cents(), cf.fret_number));
            }
            points.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-4);

            // Acoustic natural harmonic nodes n = 2..=11, k = 1..n
            for n in 2..=11 {
                for k in 1..n {
                    if gcd(n, k) != 1 {
                        continue;
                    }
                    let ratio = (n as f64) / ((n - k) as f64);
                    let c_node = 1200.0 * ratio.log2();

                    let sounding_freq = (n as f64) * f_open;
                    let sounding_pitch = calculate_sounding_pitch_for_frequency(
                        sounding_freq,
                        open_pitch,
                        n,
                        &reference_pitch,
                    );

                    let f_exact = cents_to_fret_exact(c_node, &points);
                    let f_primary = (f_exact * 10.0).round() / 10.0;

                    let mut candidate_touches: Vec<f64> = Vec::new();

                    // 1. Primary decimal fret location (rounded to nearest tenth)
                    let c_primary = fret_to_cents(f_primary, &points);
                    if (c_primary - c_node).abs() <= 18.0 {
                        candidate_touches.push(f_primary);
                    }

                    // 2. Neighboring decimal tenths splitting the fret that are close enough to ring
                    for delta in [-0.1, 0.1] {
                        let t = ((f_primary + delta) * 10.0).round() / 10.0;
                        if t > 0.0 && (t - t.round()).abs() > 1e-4 {
                            let c_t = fret_to_cents(t, &points);
                            if (c_t - c_node).abs() <= 18.0 {
                                candidate_touches.push(t);
                            }
                        }
                    }

                    // 3. Installed custom frets on this string matching this node within 8.0 cents
                    for cf in &playable_frets {
                        if (cf.cents() - c_node).abs() <= 8.0 {
                            candidate_touches.push(cf.fret_number);
                        }
                    }

                    candidate_touches
                        .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    candidate_touches.dedup_by(|a, b| (*a - *b).abs() < 1e-4);

                    for touch in candidate_touches {
                        nodes.push(HarmonicNode {
                            harmonic_number: n,
                            node_index: k,
                            fraction: (k, n),
                            fret_exact: f_exact,
                            touch_fret: touch,
                            sounding_frequency_hz: sounding_freq,
                            sounding_pitch: sounding_pitch.clone(),
                        });
                    }
                }
            }

            // Sort nodes by touch fret
            nodes.sort_by(|a, b| {
                a.touch_fret
                    .partial_cmp(&b.touch_fret)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            nodes_by_string.insert(*string_idx, nodes);
        }

        Self {
            edo: 12,
            step_multiplier: 1,
            capo: part.capo_fret(),
            reference_pitch,
            nodes_by_string,
        }
    }

    /// Constructs a harmonic lookup table directly from a `Part`.
    pub fn from_part(part: &Part) -> Self {
        Self::from_part_and_ref(part, None)
    }

    /// Constructs a harmonic lookup table directly from a `Part` and optional `ReferencePitch`.
    pub fn from_part_and_ref(part: &Part, ref_pitch: Option<ReferencePitch>) -> Self {
        let reference_pitch = ref_pitch.unwrap_or_default();
        let num_strings = part.num_strings();

        let mut tunings = Vec::new();
        for s in 1..=(num_strings as u8) {
            if let Some(open_pitch) = part.get_string_tuning(s) {
                tunings.push((s, open_pitch));
            }
        }

        if let Some(custom_frets) = &part.ext.custom_frets {
            if !custom_frets.is_empty() {
                return Self::new_custom_fretboard(part, custom_frets, &tunings, reference_pitch);
            }
        }

        let edo = part.edo();
        let step_multiplier = part.step_multiplier();
        let capo = part.capo_fret();
        Self::new(edo, step_multiplier, capo, &tunings, Some(reference_pitch))
    }

    /// Constructs a harmonic lookup table using `doc.global._x.tuning.referenceNote` when present.
    pub fn from_doc_part(doc: &crate::mnx::MnxDocument, part: &Part) -> Self {
        let ref_pitch = doc
            .global
            .ext
            .tuning
            .as_ref()
            .and_then(|t| t.reference_note.as_ref())
            .map(|rn| ReferencePitch {
                pitch: rn.pitch.clone(),
                frequency_hz: rn.frequency_hz,
            });
        Self::from_part_and_ref(part, ref_pitch)
    }

    /// Returns all harmonic nodes for a given string.
    pub fn get_nodes_for_string(&self, string: u8) -> Option<&[HarmonicNode]> {
        self.nodes_by_string.get(&string).map(|v| v.as_slice())
    }

    /// Finds all harmonic nodes matching a given touch fret on a string.
    /// Matches direct touch within 0.05 fret tolerance, or within 0.12 for decimal fret locations.
    pub fn find_nodes_at_touch(&self, string: u8, touch: f64) -> Vec<&HarmonicNode> {
        let nodes = match self.nodes_by_string.get(&string) {
            Some(n) => n,
            None => return Vec::new(),
        };

        let direct: Vec<&HarmonicNode> = nodes
            .iter()
            .filter(|node| (node.touch_fret - touch).abs() < 0.05)
            .collect();
        if !direct.is_empty() {
            return direct;
        }

        // If touch is a decimal fret, check if it is within 0.12 of a node
        // (allowing neighboring tenths that split the fret into 10ths)
        if (touch - touch.round()).abs() > 1e-4 {
            let close: Vec<&HarmonicNode> = nodes
                .iter()
                .filter(|node| {
                    (node.fret_exact - touch).abs() <= 0.12
                        || (node.touch_fret - touch).abs() <= 0.12
                })
                .collect();
            if !close.is_empty() {
                return close;
            }
        }

        Vec::new()
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
            let alter_match = (expected_alter - actual_alter).abs() < 0.005;

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
            ..Default::default()
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
                    ..Default::default()
                },
            ),
            (
                2,
                Pitch {
                    step: "B".to_string(),
                    alter: None,
                    octave: 3,
                    ..Default::default()
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
                    ..Default::default()
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
                    ..Default::default()
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
                    ..Default::default()
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
        let yaml_str = include_str!("../examples/example_harmonics.yaml");
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

    #[test]
    fn test_custom_fretboard_natural_harmonics_and_yaml_cleanup() {
        use crate::fretboard_editor::{CustomFret, FretLocationSpec};
        use crate::mnx::{Harmonic, Note, NoteExt, Part, PartExt, Pitch, PitchExt, StringTuning};

        // Construct a part with String 1 = G4 (open), String 2 = D4 (open)
        let tunings = vec![
            StringTuning {
                string: 1,
                pitch: Pitch {
                    step: "G".to_string(),
                    alter: None,
                    octave: 4,
                    ext: PitchExt::default(),
                },
            },
            StringTuning {
                string: 2,
                pitch: Pitch {
                    step: "D".to_string(),
                    alter: None,
                    octave: 4,
                    ext: PitchExt::default(),
                },
            },
        ];

        // Frets:
        // Fret 1.5: 9/8 ratio, installed on strings 1 and 2
        // Fret 6: 5/4 ratio, installed ONLY on string 1
        // Fret 7: 4/3 ratio, installed on strings 1 and 2
        let frets = vec![
            CustomFret {
                fret_number: 1.5,
                manual_number: None,
                spec: FretLocationSpec::Ratio { num: 9, den: 8 },
                strings: vec![1, 2],
                note_name: "9/8".to_string(),
            },
            CustomFret {
                fret_number: 6.0,
                manual_number: None,
                spec: FretLocationSpec::Ratio { num: 5, den: 4 },
                strings: vec![1],
                note_name: "5/4".to_string(),
            },
            CustomFret {
                fret_number: 7.0,
                manual_number: None,
                spec: FretLocationSpec::Ratio { num: 4, den: 3 },
                strings: vec![1, 2],
                note_name: "4/3".to_string(),
            },
        ];

        let mut part = Part {
            id: None,
            name: Some("Custom JI Guitar".to_string()),
            short_name: None,
            comment: None,
            ext: PartExt {
                micro_fret_divisions: Some(12),
                fret_step_multiplier: Some(1),
                pitch_bend_range: None,
                capo: None,
                tuning: Some(tunings),
                custom_frets: Some(frets),
            },
            measures: Vec::new(),
        };

        // 1. Build lookup table for this part
        let table = HarmonicLookupTable::from_part(&part);

        // On String 1: fret 6 (5/4 node: n=5, k=1) is an installed fret
        let nodes_s1_f6 = table.find_nodes_at_touch(1, 6.0);
        assert!(!nodes_s1_f6.is_empty(), "String 1 should have a harmonic at installed fret 6.0");
        let n6 = nodes_s1_f6[0];
        assert_eq!(n6.harmonic_number, 5);
        assert_eq!(n6.node_index, 1);
        assert_eq!(n6.touch_fret, 6.0);
        assert_eq!(n6.sounding_pitch.step, "B");
        assert_eq!(n6.sounding_pitch.octave, 6);

        // On String 2: fret 6 is NOT installed on string 2
        // Node 5/4 falls between fret 1.5 (203.9 cents) and fret 7.0 (498.0 cents),
        // which maps to decimal fret location 4.9 (tenths interpolation).
        let nodes_s2_f6 = table.find_nodes_at_touch(2, 6.0);
        assert!(
            nodes_s2_f6.is_empty(),
            "String 2 should not have harmonic at fret 6.0 because fret 6 is not on String 2"
        );
        let nodes_s2_f4_9 = table.find_nodes_at_touch(2, 4.9);
        assert!(
            !nodes_s2_f4_9.is_empty(),
            "String 2 should have 5th harmonic at decimal fret location 4.9"
        );
        assert_eq!(nodes_s2_f4_9[0].harmonic_number, 5);

        // Decimal fretlet 1.5 (9/8 node: n=9, k=1) is installed on both strings
        let nodes_s1_f1_5 = table.find_nodes_at_touch(1, 1.5);
        assert_eq!(nodes_s1_f1_5.len(), 1, "String 1 should have harmonic at fretlet 1.5");
        assert_eq!(nodes_s1_f1_5[0].harmonic_number, 9);

        // 2. Test harmonic validation
        let valid_h = Harmonic {
            touch: 6.0,
            sounding_note: Box::new(Note {
                id: None,
                pitch: n6.sounding_pitch.clone(),
                accidental_display: None,
                ties: None,
                comment: None,
                ext: NoteExt::default(),
            }),
        };
        assert!(table.validate_harmonic(1, &valid_h).is_ok());

        // Validate on String 2 should fail since fret 6 is not playable on string 2
        let invalid_on_s2 = table.validate_harmonic(2, &valid_h);
        assert!(invalid_on_s2.is_err());
        assert!(invalid_on_s2.unwrap_err().message.contains("does not correspond to any harmonic node"));

        // Validate decimal fret location 4.9 on String 2 should succeed for sounding F#6
        let valid_s2_pitch = nodes_s2_f4_9[0].sounding_pitch.clone();
        let valid_s2_h = Harmonic {
            touch: 4.9,
            sounding_note: Box::new(Note {
                id: None,
                pitch: valid_s2_pitch,
                accidental_display: None,
                ties: None,
                comment: None,
                ext: NoteExt::default(),
            }),
        };
        assert!(table.validate_harmonic(2, &valid_s2_h).is_ok());

        // 3. Test standard 12-EDO custom fretboard decimal fret locations (e.g. 3.9, 2.7)
        let edo_frets: Vec<CustomFret> = (1..=24)
            .map(|f| CustomFret {
                fret_number: f as f64,
                manual_number: None,
                spec: FretLocationSpec::Cents { cents: (f * 100) as f64 },
                strings: vec![1, 2],
                note_name: format!("Fret {}", f),
            })
            .collect();
        let edo_part = Part {
            id: None,
            name: Some("12-EDO Custom Guitar".to_string()),
            short_name: None,
            comment: None,
            ext: PartExt {
                micro_fret_divisions: None,
                fret_step_multiplier: None,
                pitch_bend_range: None,
                capo: None,
                tuning: Some(vec![StringTuning {
                    string: 1,
                    pitch: Pitch {
                        step: "E".to_string(),
                        alter: None,
                        octave: 4,
                        ext: PitchExt::default(),
                    },
                }]),
                custom_frets: Some(edo_frets),
            },
            measures: Vec::new(),
        };
        let edo_table = HarmonicLookupTable::from_part(&edo_part);
        // Harmonic 5 (1/5 node) maps to decimal fret location 3.9
        let s1_3_9 = edo_table.find_nodes_at_touch(1, 3.9);
        assert!(!s1_3_9.is_empty(), "Decimal fret 3.9 should map to harmonic 5");
        assert_eq!(s1_3_9[0].harmonic_number, 5);
        assert_eq!(s1_3_9[0].sounding_pitch.step, "G");
        assert_eq!(s1_3_9[0].sounding_pitch.octave, 6);

        // Harmonic 7 (1/7 node) maps to decimal fret location 2.7
        let s1_2_7 = edo_table.find_nodes_at_touch(1, 2.7);
        assert!(!s1_2_7.is_empty(), "Decimal fret 2.7 should map to harmonic 7");
        assert_eq!(s1_2_7[0].harmonic_number, 7);

        // Whole fret 4 is not in the table as a harmonic touch point
        assert!(edo_table.find_nodes_at_touch(1, 4.0).is_empty());

        // 4. Test Kite Guitar (41-EDO step mult 2) custom fretboard decimal fret 20.5 for octave harmonic
        let kite_frets: Vec<CustomFret> = (1..=24)
            .map(|f| CustomFret {
                fret_number: f as f64,
                manual_number: None,
                spec: FretLocationSpec::EdoStep { steps: (f * 2) as i32, edo: 41 },
                strings: vec![1],
                note_name: format!("Fret {}", f),
            })
            .collect();
        let kite_part = Part {
            id: None,
            name: Some("Kite Guitar".to_string()),
            short_name: None,
            comment: None,
            ext: PartExt {
                micro_fret_divisions: None,
                fret_step_multiplier: None,
                pitch_bend_range: None,
                capo: None,
                tuning: Some(vec![StringTuning {
                    string: 1,
                    pitch: Pitch {
                        step: "E".to_string(),
                        alter: None,
                        octave: 4,
                        ext: PitchExt::default(),
                    },
                }]),
                custom_frets: Some(kite_frets),
            },
            measures: Vec::new(),
        };
        let kite_table = HarmonicLookupTable::from_part(&kite_part);
        let s1_20_5 = kite_table.find_nodes_at_touch(1, 20.5);
        assert!(!s1_20_5.is_empty(), "Kite guitar decimal fret 20.5 should map to octave harmonic");
        assert_eq!(s1_20_5[0].harmonic_number, 2);
        assert_eq!(s1_20_5[0].sounding_pitch.step, "E");
        assert_eq!(s1_20_5[0].sounding_pitch.octave, 5);

        // 5. Test YAML cleanup
        assert!(part.has_custom_frets());
        assert!(part.ext.micro_fret_divisions.is_some());
        assert!(part.ext.fret_step_multiplier.is_some());

        part.clean_unneeded_fret_fields();
        assert!(part.ext.micro_fret_divisions.is_none());
        assert!(part.ext.fret_step_multiplier.is_none());

        let yaml_in = r#"mnx:
  version: 1
global:
  measures: []
parts:
  - name: "Custom JI Guitar"
    _x:
      microFretDivisions: 12
      fretStepMultiplier: 1
      tuning:
        - string: 1
          pitch: { step: G, octave: 4 }
        - string: 2
          pitch: { step: D, octave: 4 }
      customFrets:
        - fret: 6
          spec: { kind: ratio, num: 5, den: 4 }
          strings: [1]
          noteName: "5/4"
    measures: []
"#;
        let mut doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml_in).expect("parsed doc");
        doc.clean_unneeded_fret_fields();
        let yaml_out = serde_saphyr::to_string(&doc).expect("YAML serialization");
        assert!(
            !yaml_out.contains("microFretDivisions"),
            "YAML should not contain microFretDivisions with custom fretboard"
        );
        assert!(
            !yaml_out.contains("fretStepMultiplier"),
            "YAML should not contain fretStepMultiplier with custom fretboard"
        );
        assert!(
            yaml_out.contains("customFrets"),
            "YAML should contain customFrets"
        );
    }
}
