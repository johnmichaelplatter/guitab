use crate::edo::EdoTuning;
use crate::mnx::{Duration, MnxDocument, TimeSignature};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub struct ValidationError {
    pub measure_idx: usize,
    pub event_idx: usize, // 0 for measure-level errors
    pub string: u8,       // 0 for measure-level errors
    pub fret: i32,        // 0 for measure-level errors
    pub message: String,
}

/// Converts an MNX Duration to duration in quarter-note beats.
pub fn duration_to_quarter_beats(duration: &Duration) -> f64 {
    let base_str = duration.base.trim().to_lowercase();
    let is_string_dotted = base_str.contains("dotted") || base_str.contains('.');
    let dot_count = duration
        .dots
        .unwrap_or(if is_string_dotted { 1 } else { 0 });

    let raw_beats = if base_str.contains("breve") || base_str.contains("double-whole") {
        8.0
    } else if base_str.contains("whole") || base_str == "1" {
        4.0
    } else if base_str.contains("half") || base_str == "1/2" {
        2.0
    } else if base_str.contains("32nd")
        || base_str.contains("thirtysecond")
        || base_str.contains("thirty-second")
        || base_str == "1/32"
    {
        0.125
    } else if base_str.contains("64th")
        || base_str.contains("sixtyfourth")
        || base_str.contains("sixty-fourth")
        || base_str == "1/64"
    {
        0.0625
    } else if base_str.contains("16th") || base_str.contains("sixteenth") || base_str == "1/16" {
        0.25
    } else if base_str.contains("eighth") || base_str.contains("8th") || base_str == "1/8" {
        0.5
    } else if base_str.contains("quarter") || base_str.contains("4th") || base_str == "1/4" {
        1.0
    } else {
        1.0
    };

    let mut multiplier = 1.0;
    let mut add = 0.5;
    for _ in 0..dot_count {
        multiplier += add;
        add /= 2.0;
    }

    raw_beats * multiplier
}

/// Finds the applicable TimeSignature for a measure index by inspecting global.measures up to measure_idx,
/// defaulting to 4/4 if not specified.
pub fn get_time_signature_for_measure(doc: &MnxDocument, measure_idx: usize) -> TimeSignature {
    let max_check = measure_idx.min(doc.global.measures.len().saturating_sub(1));
    if !doc.global.measures.is_empty() {
        for i in (0..=max_check).rev() {
            if let Some(time) = doc.global.measures[i].time {
                return time;
            }
        }
    }
    TimeSignature { count: 4, unit: 4 }
}

fn format_beats(beats: f64) -> String {
    if beats.fract().abs() < 1e-4 {
        format!("{:.0}", beats)
    } else {
        let s = format!("{:.3}", beats);
        let trimmed = s.trim_end_matches('0').trim_end_matches('.');
        trimmed.to_string()
    }
}

pub fn validate_mnx(doc: &MnxDocument) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    for part in &doc.parts {
        let capo_fret = part.capo_fret();
        let edo = part.edo();
        let step_multiplier = part.step_multiplier();

        let tuning_calc = EdoTuning::new(edo);
        let harmonic_table = crate::harmonic::HarmonicLookupTable::from_part(part);
        let num_measures = part.measures.len();

        for (m_idx, measure) in part.measures.iter().enumerate() {
            let is_final_measure = m_idx + 1 == num_measures;
            let time_sig = get_time_signature_for_measure(doc, m_idx);
            let expected_beats = (time_sig.count as f64) * (4.0 / time_sig.unit as f64);

            for seq in &measure.sequences {
                // Validate sequence total duration against measure time signature
                let total_beats: f64 = seq.total_beats();

                // Final measure exception: allowed to be under-capacity while writing,
                // but still flags errors if it overflows the measure.
                // Non-final measures must match the time signature exactly.
                if is_final_measure {
                    if total_beats > expected_beats + 1e-4 {
                        errors.push(ValidationError {
                            measure_idx: m_idx + 1,
                            event_idx: 0,
                            string: 0,
                            fret: 0,
                            message: format!(
                                "Measure duration ({} beats) exceeds time signature {}/{} ({} beats).",
                                format_beats(total_beats), time_sig.count, time_sig.unit, format_beats(expected_beats)
                            ),
                        });
                    }
                } else {
                    if (total_beats - expected_beats).abs() > 1e-4 {
                        let status = if total_beats < expected_beats {
                            "is incomplete"
                        } else {
                            "exceeds"
                        };
                        errors.push(ValidationError {
                            measure_idx: m_idx + 1,
                            event_idx: 0,
                            string: 0,
                            fret: 0,
                            message: format!(
                                "Measure duration ({} beats) {} for time signature {}/{} ({} beats).",
                                format_beats(total_beats), status, time_sig.count, time_sig.unit, format_beats(expected_beats)
                            ),
                        });
                    }
                }

                // Note-level geometry and concurrency checks
                for (e_idx, event) in seq.flattened_events().into_iter().enumerate() {
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

                            if let Some(harmonic) = &note.harmonic {
                                match harmonic_table.validate_harmonic(string, harmonic) {
                                    Ok(_) => {}
                                    Err(h_err) => {
                                        errors.push(ValidationError {
                                            measure_idx: m_idx + 1,
                                            event_idx: e_idx,
                                            string,
                                            fret,
                                            message: h_err.message,
                                        });
                                    }
                                }
                            } else if let Some(string_pitch) = part.get_string_tuning(string) {
                                let string_base = tuning_calc.pitch_to_steps(&string_pitch);

                                let physical_capo_steps = (capo_fret as i32) * step_multiplier;
                                let physical_fret_steps = (fret as i32) * step_multiplier;
                                let played_steps =
                                    string_base + physical_capo_steps + physical_fret_steps;

                                if fret < 0 {
                                    errors.push(ValidationError {
                                        measure_idx: m_idx + 1,
                                        event_idx: e_idx,
                                        string,
                                        fret,
                                        message: format!(
                                            "Negative fret {} on String {} is physically impossible.",
                                            fret, string
                                        ),
                                    });
                                }

                                let expected_steps = tuning_calc.pitch_to_steps(&note.pitch);

                                if played_steps != expected_steps {
                                    let alter_str = note
                                        .pitch
                                        .alter
                                        .as_ref()
                                        .map(|a| format!(" alter {}", a))
                                        .unwrap_or_default();

                                    errors.push(ValidationError {
                                        measure_idx: m_idx + 1,
                                        event_idx: e_idx,
                                        string,
                                        fret,
                                        message: format!(
                                            "Geometry mismatch on String {} Fret {}. \
                                            Physically produces step {} in {}-EDO. \
                                            MNX data expects {}{}{} (step {}).",
                                            string,
                                            fret,
                                            played_steps,
                                            edo,
                                            note.pitch.step,
                                            note.pitch.octave,
                                            alter_str,
                                            expected_steps
                                        ),
                                    });
                                }
                            } else {
                                errors.push(ValidationError {
                                    measure_idx: m_idx + 1,
                                    event_idx: e_idx,
                                    string,
                                    fret,
                                    message: format!(
                                        "Note placed on string {}, but no tuning is defined.",
                                        string
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duration_to_quarter_beats() {
        assert_eq!(
            duration_to_quarter_beats(&Duration {
                multiple: None,
                base: "whole".to_string(),
                dots: None
            }),
            4.0
        );
        assert_eq!(
            duration_to_quarter_beats(&Duration {
                multiple: None,
                base: "half".to_string(),
                dots: None
            }),
            2.0
        );
        assert_eq!(
            duration_to_quarter_beats(&Duration {
                multiple: None,
                base: "quarter".to_string(),
                dots: None
            }),
            1.0
        );
        assert_eq!(
            duration_to_quarter_beats(&Duration {
                multiple: None,
                base: "eighth".to_string(),
                dots: None
            }),
            0.5
        );
        assert_eq!(
            duration_to_quarter_beats(&Duration {
                multiple: None,
                base: "sixteenth".to_string(),
                dots: None
            }),
            0.25
        );
        assert_eq!(
            duration_to_quarter_beats(&Duration {
                multiple: None,
                base: "32nd".to_string(),
                dots: None
            }),
            0.125
        );
        assert_eq!(
            duration_to_quarter_beats(&Duration {
                multiple: None,
                base: "dotted-quarter".to_string(),
                dots: None
            }),
            1.5
        );
        assert_eq!(
            duration_to_quarter_beats(&Duration {
                multiple: None,
                base: "quarter".to_string(),
                dots: Some(1)
            }),
            1.5
        );
        assert_eq!(
            duration_to_quarter_beats(&Duration {
                multiple: None,
                base: "quarter".to_string(),
                dots: Some(2)
            }),
            1.75
        );
    }

    #[test]
    fn test_validate_measure_rhythms() {
        let yaml_valid = r#"
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
    - time: { count: 4, unit: 4 }
parts:
  - instrument:
      name: "Guitar"
    measures:
      - sequences:
          - content:
              - duration: { base: whole }
                notes: []
      - sequences:
          - content:
              - duration: { base: half }
                notes: []
"#;
        let doc: MnxDocument = serde_saphyr::from_str(yaml_valid).unwrap();
        let errors = validate_mnx(&doc);
        // Measure 1 is 4 beats (4/4) -> OK.
        // Measure 2 is 2 beats, but it is the final measure -> OK (under construction).
        assert_eq!(errors.len(), 0);

        // If we add measure 3, now measure 2 (2 beats in 4/4) is non-final and must fail
        let yaml_incomplete_middle = r#"
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - instrument:
      name: "Guitar"
    measures:
      - sequences:
          - content:
              - duration: { base: whole }
                notes: []
      - sequences:
          - content:
              - duration: { base: half }
                notes: []
      - sequences:
          - content:
              - duration: { base: whole }
                notes: []
"#;
        let doc2: MnxDocument = serde_saphyr::from_str(yaml_incomplete_middle).unwrap();
        let errors2 = validate_mnx(&doc2);
        assert_eq!(errors2.len(), 1);
        assert_eq!(errors2[0].measure_idx, 2);
        assert!(errors2[0].message.contains("is incomplete"));

        // If the final measure overflows, it should fail
        let yaml_overfull_final = r#"
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - instrument:
      name: "Guitar"
    measures:
      - sequences:
          - content:
              - duration: { base: whole }
                notes: []
              - duration: { base: quarter }
                notes: []
"#;
        let doc3: MnxDocument = serde_saphyr::from_str(yaml_overfull_final).unwrap();
        let errors3 = validate_mnx(&doc3);
        assert_eq!(errors3.len(), 1);
        assert_eq!(errors3[0].measure_idx, 1);
        assert!(errors3[0].message.contains("exceeds"));
    }

    #[test]
    fn test_validate_harmonics_in_validate_mnx() {
        let yaml_str = include_str!("../example_harmonics.yaml");
        let doc: MnxDocument = serde_saphyr::from_str(yaml_str).unwrap();
        let errors = validate_mnx(&doc);
        assert_eq!(errors.len(), 4);
        for err in &errors {
            assert_eq!(err.measure_idx, 3);
        }
    }
}

pub fn quarter_beats_to_duration(beats: f64) -> Option<Duration> {
    let eps = 0.001;
    if (beats - 4.0).abs() < eps {
        Some(Duration {
            base: "whole".to_string(),
            dots: None,
            multiple: None,
        })
    } else if (beats - 3.0).abs() < eps {
        Some(Duration {
            base: "half".to_string(),
            dots: Some(1),
            multiple: None,
        })
    } else if (beats - 2.0).abs() < eps {
        Some(Duration {
            base: "half".to_string(),
            dots: None,
            multiple: None,
        })
    } else if (beats - 1.5).abs() < eps {
        Some(Duration {
            base: "quarter".to_string(),
            dots: Some(1),
            multiple: None,
        })
    } else if (beats - 1.0).abs() < eps {
        Some(Duration {
            base: "quarter".to_string(),
            dots: None,
            multiple: None,
        })
    } else if (beats - 0.75).abs() < eps {
        Some(Duration {
            base: "eighth".to_string(),
            dots: Some(1),
            multiple: None,
        })
    } else if (beats - 0.5).abs() < eps {
        Some(Duration {
            base: "eighth".to_string(),
            dots: None,
            multiple: None,
        })
    } else if (beats - 0.375).abs() < eps {
        Some(Duration {
            base: "16th".to_string(),
            dots: Some(1),
            multiple: None,
        })
    } else if (beats - 0.25).abs() < eps {
        Some(Duration {
            base: "16th".to_string(),
            dots: None,
            multiple: None,
        })
    } else if (beats - 0.1875).abs() < eps {
        Some(Duration {
            base: "32nd".to_string(),
            dots: Some(1),
            multiple: None,
        })
    } else if (beats - 0.125).abs() < eps {
        Some(Duration {
            base: "32nd".to_string(),
            dots: None,
            multiple: None,
        })
    } else if (beats - 0.0625).abs() < eps {
        Some(Duration {
            base: "64th".to_string(),
            dots: None,
            multiple: None,
        })
    } else {
        None
    }
}
