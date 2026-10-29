#[derive(Debug, Clone, Copy)]
pub struct EdoTuning {
    pub edo: u32,
    pub generator: i32,
    pub sharp_steps: i32,
    pub step_semitones: f64,
}

fn nominal_generators_from_d(nom: char) -> i32 {
    match nom {
        'F' => -3,
        'C' => -2,
        'G' => -1,
        'D' => 0,
        'A' => 1,
        'E' => 2,
        'B' => 3,
        _ => 0,
    }
}

impl EdoTuning {
    pub fn new(edo: u32) -> Self {
        let e = edo as f64;
        let generator = (e * 1.5_f64.log2()).round() as i32;
        let sharp_steps = 7 * generator - 4 * (edo as i32);

        Self {
            edo,
            generator,
            sharp_steps,
            step_semitones: 12.0 / e,
        }
    }

    pub fn nominal_step(&self, step_char: char) -> i32 {
        let g = self.generator;
        let e = self.edo as i32;

        match step_char {
            'C' => 0,
            'D' => (2 * g).rem_euclid(e),
            'E' => (4 * g).rem_euclid(e),
            'F' => (-g).rem_euclid(e),
            'G' => g.rem_euclid(e),
            'A' => (3 * g).rem_euclid(e),
            'B' => (5 * g).rem_euclid(e),
            _ => 0,
        }
    }

    pub fn pitch_to_steps(&self, pitch: &crate::mnx::Pitch) -> i32 {
        if pitch.is_ratio_or_cents_step() {
            if let Some(name) = &pitch.ext.name {
                if let Some(parsed) = self.parse_pitch_string(name, pitch.octave) {
                    return self.pitch_to_steps(&parsed);
                }
            }
            if let Some(cents) = pitch.step_cents_above_reference() {
                let alter_semitones = pitch
                    .alter
                    .as_ref()
                    .map(|a| a.as_semitones())
                    .unwrap_or(0.0);
                let total_semitones = (cents / 100.0) + alter_semitones;
                let alter_steps = (total_semitones / self.step_semitones).round() as i32;
                return (pitch.octave * (self.edo as i32)) + alter_steps;
            }
        }
        let step = pitch.step.chars().next().unwrap_or('C');
        let octave = pitch.octave;
        let alter_semitones = pitch
            .alter
            .as_ref()
            .map(|a| a.as_semitones())
            .unwrap_or(0.0);

        let alter_steps = (alter_semitones / self.step_semitones).round() as i32;
        self.nominal_step(step) + (octave * (self.edo as i32)) + alter_steps
    }

    pub fn steps_to_pitch(&self, played_steps: i32) -> crate::mnx::Pitch {
        let e = self.edo as i32;
        let step_in_octave = played_steps.rem_euclid(e);

        let nominals = ['C', 'D', 'E', 'F', 'G', 'A', 'B'];
        let mut best_nom = 'C';
        let mut best_alter_steps = 0;
        let mut best_score = i32::MAX;

        for &nom_char in &nominals {
            let base_step = self.nominal_step(nom_char);
            let mut dist = (step_in_octave - base_step).rem_euclid(e);
            if dist > e / 2 {
                dist -= e;
            }

            for sharps in -2..=2 {
                let ups = dist - sharps * self.sharp_steps;
                let gen_coord = nominal_generators_from_d(nom_char) + sharps * 7;
                // We use a Manhattan-like distance on the (generator, ups) lattice.
                // We weight `ups` by a factor (e.g. 5) to penalize microtonal accidentals slightly
                // compared to moving along the circle of fifths, but still allow `vE` to beat `Fb` in 41 EDO.
                // If sharp_steps is 1 (like 12 EDO, 19 EDO, etc...), we want standard notation to always win.
                let up_weight = if self.sharp_steps.abs() == 1 { 1000 } else { 5 };
                let score = gen_coord.abs() + ups.abs() * up_weight;

                if score < best_score {
                    best_score = score;
                    best_nom = nom_char;
                    best_alter_steps = sharps * self.sharp_steps + ups;
                }
            }
        }

        let octave = (played_steps - self.nominal_step(best_nom) - best_alter_steps).div_euclid(e);

        let alter = if best_alter_steps == 0 {
            None
        } else if self.edo == 12 {
            Some(crate::mnx::Alteration::Value(best_alter_steps as f64))
        } else {
            Some(crate::mnx::Alteration::Fraction(format!(
                "{}\\{}",
                best_alter_steps, self.edo
            )))
        };

        crate::mnx::Pitch {
            step: best_nom.to_string(),
            alter,
            octave,
            ext: crate::mnx::PitchExt::default(),
        }
    }
    pub fn parse_pitch_string(&self, s: &str, octave: i32) -> Option<crate::mnx::Pitch> {
        let mut ups = 0;
        let mut downs = 0;
        let mut sharps = 0;
        let mut flats = 0;
        let mut nom = None;

        for c in s.chars() {
            match c {
                '^' => ups += 1,
                'v' => downs += 1,
                '#' => sharps += 1,
                'b' => flats += 1,
                'A'..='G' => nom = Some(c),
                _ => return None,
            }
        }

        let nom = nom?;
        if ups > 3 || downs > 3 || sharps > 2 || flats > 2 {
            return None;
        }
        if ups > 0 && downs > 0 {
            return None;
        }
        if sharps > 0 && flats > 0 {
            return None;
        }

        let total_sharps = sharps - flats;
        let total_ups = ups - downs;

        let alter_steps = total_sharps * self.sharp_steps + total_ups;
        let alter = if alter_steps == 0 {
            None
        } else if self.edo == 12 {
            Some(crate::mnx::Alteration::Value(alter_steps as f64))
        } else {
            Some(crate::mnx::Alteration::Fraction(format!(
                "{}\\{}",
                alter_steps, self.edo
            )))
        };

        Some(crate::mnx::Pitch {
            step: nom.to_string(),
            alter,
            octave,
            ext: crate::mnx::PitchExt::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_steps_to_pitch_roundtrip() {
        for edo in [12, 19, 22, 31, 41, 53] {
            let tuning = EdoTuning::new(edo);
            for steps in -200..=500 {
                let pitch = tuning.steps_to_pitch(steps);
                let recovered = tuning.pitch_to_steps(&pitch);
                assert_eq!(
                    steps, recovered,
                    "Roundtrip failed for {}-EDO at step {}: pitch={:?}",
                    edo, steps, pitch
                );
            }
        }
    }

    #[test]
    fn test_kite_guitar_string4_fret12() {
        // String 4 on Kite Guitar is vE3 (alter -1\41).
        // In 41-EDO: nominal E = 14, octave 3 = 123, alter = -1 -> base step 136.
        // Fret 12 with multiplier 2 produces 136 + 24 = step 160.
        let tuning = EdoTuning::new(41);
        let pitch = tuning.steps_to_pitch(160);
        assert_eq!(pitch.step, "B");
        assert_eq!(pitch.octave, 3);
        assert_eq!(
            pitch.alter.as_ref().map(|a| a.to_string()),
            Some("-1\\41".to_string())
        );
        assert_eq!(tuning.pitch_to_steps(&pitch), 160);
    }
}

#[allow(dead_code)]
pub fn generate_edo_scale(edo: u32) -> Vec<(i32, String, f64, f64)> {
    let tuning = EdoTuning::new(edo);
    let e = edo as i32;
    let mut scale = Vec::new();

    let nominals = ['C', 'D', 'E', 'F', 'G', 'A', 'B'];

    for step in 0..e {
        let mut best_name = String::new();
        let mut best_score = i32::MAX;
        let mut best_alter = 0.0;

        for nom_char in &nominals {
            let base_step = tuning.nominal_step(*nom_char);

            let mut dist = (step - base_step).rem_euclid(e);
            if dist > e / 2 {
                dist -= e;
            }

            for sharps in -2..=2 {
                let ups = dist - sharps * tuning.sharp_steps;
                let gen_coord = nominal_generators_from_d(*nom_char) + sharps * 7;
                let up_weight = if tuning.sharp_steps.abs() == 1 {
                    1000
                } else {
                    5
                };
                let score = gen_coord.abs() + ups.abs() * up_weight;

                if score < best_score {
                    best_score = score;

                    let mut name = String::new();
                    if ups > 0 {
                        name.push_str(&"^".repeat(ups as usize));
                    }
                    if ups < 0 {
                        name.push_str(&"v".repeat((-ups) as usize));
                    }

                    name.push(*nom_char);

                    if sharps > 0 {
                        name.push_str(&"#".repeat(sharps as usize));
                    }
                    if sharps < 0 {
                        name.push_str(&"b".repeat((-sharps) as usize));
                    }

                    best_name = name;

                    let alter_steps = sharps * tuning.sharp_steps + ups;
                    best_alter = (alter_steps as f64) * tuning.step_semitones;
                }
            }
        }

        let midi_note = 60.0 + (step as f64) * tuning.step_semitones;
        scale.push((step, best_name, best_alter, midi_note));
    }

    scale
}
