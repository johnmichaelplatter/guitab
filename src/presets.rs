use crate::edo::{EdoTuning, generate_edo_scale};
use crate::mnx::{Alteration, Pitch, StringTuning};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

fn default_step_mult() -> u8 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstrumentPreset {
    pub name: String,
    #[serde(rename = "microFretDivisions", alias = "edo")]
    pub edo: u8,
    #[serde(
        rename = "fretStepMultiplier",
        alias = "step_multiplier",
        alias = "stepMultiplier",
        default = "default_step_mult"
    )]
    pub step_multiplier: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capo: Option<u8>,
    pub tuning: Vec<StringTuning>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PresetListWrapper {
    pub presets: Vec<InstrumentPreset>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
enum PresetYamlFormat {
    WrappedList(PresetListWrapper),
    BareList(Vec<InstrumentPreset>),
    Single(InstrumentPreset),
}

impl InstrumentPreset {
    pub fn display_label(&self) -> String {
        format!(
            "{} EDO — {} ({}-str)",
            self.edo,
            self.name,
            self.tuning.len()
        )
    }

    /// Construct an `InstrumentPreset` from the fields of `ActiveDialog::InstrumentSetup`.
    pub fn from_dialog_state(
        name: &str,
        num_strings: u8,
        edo: u8,
        step_multiplier: u8,
        tunings_text: &[String],
        tunings_octave: &[i32],
    ) -> Result<Self, String> {
        let clean_name = name.trim();
        if clean_name.is_empty() {
            return Err("Preset name cannot be empty.".to_string());
        }
        let tuning_calc = EdoTuning::new(edo as u32);
        let mut tuning = Vec::with_capacity(num_strings as usize);
        for s in 0..(num_strings as usize) {
            let text = tunings_text
                .get(s)
                .ok_or_else(|| format!("Missing tuning text for string {}", s + 1))?;
            let octave = tunings_octave.get(s).copied().unwrap_or(4);
            let pitch = tuning_calc
                .parse_pitch_string(text, octave)
                .ok_or_else(|| format!("String {} has an invalid pitch name '{}'.", s + 1, text))?;
            tuning.push(StringTuning {
                string: (s + 1) as u8,
                pitch,
            });
        }
        Ok(Self {
            name: clean_name.to_string(),
            edo,
            step_multiplier: step_multiplier.max(1),
            capo: None,
            tuning,
        })
    }

    /// Apply this preset's parameters into the `InstrumentSetup` dialog buffers.
    pub fn apply_to_dialog_state(
        &self,
        num_strings: &mut u8,
        edo: &mut u8,
        step_multiplier: &mut u8,
        tunings_text: &mut Vec<String>,
        tunings_octave: &mut Vec<i32>,
    ) {
        *num_strings = (self.tuning.len().clamp(1, 15)) as u8;
        *edo = self.edo.clamp(1, 100);
        *step_multiplier = self.step_multiplier.clamp(1, 10);

        let edo_val = *edo as u32;
        let edo_scale = generate_edo_scale(edo_val);
        let tuning_calc = EdoTuning::new(edo_val);

        if tunings_text.len() < 15 {
            tunings_text.resize(15, "E".to_string());
        }
        if tunings_octave.len() < 15 {
            tunings_octave.resize(15, 2);
        }

        for st in &self.tuning {
            let idx = (st.string.saturating_sub(1)) as usize;
            if idx < 15 {
                let steps = tuning_calc.pitch_to_steps(&st.pitch);
                let step_in_oct = steps.rem_euclid(edo_val as i32);
                let text = edo_scale
                    .iter()
                    .find(|(s_step, _, _, _)| *s_step == step_in_oct)
                    .map(|(_, n, _, _)| n.clone())
                    .unwrap_or_else(|| st.pitch.step.clone());
                tunings_text[idx] = text;
                tunings_octave[idx] = st.pitch.octave;
            }
        }
    }
}

/// Sort presets primarily by EDO ascending, then case-insensitively by name.
pub fn sort_presets(presets: &mut [InstrumentPreset]) {
    presets.sort_by(|a, b| {
        a.edo
            .cmp(&b.edo)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

/// Append a preset (or update if same EDO and case-insensitive name) and re-sort by EDO.
pub fn add_or_update_preset(presets: &mut Vec<InstrumentPreset>, new_preset: InstrumentPreset) {
    if let Some(existing) = presets.iter_mut().find(|p| {
        p.edo == new_preset.edo && p.name.eq_ignore_ascii_case(&new_preset.name)
    }) {
        *existing = new_preset;
    } else {
        presets.push(new_preset);
    }
    sort_presets(presets);
}

/// Parse one or more presets from a YAML string (supports single preset, `{ presets: [...] }`, or bare list `[...]`).
pub fn parse_presets_from_yaml(yaml: &str) -> Result<Vec<InstrumentPreset>, String> {
    let parsed: PresetYamlFormat = serde_saphyr::from_str(yaml)
        .map_err(|e| format!("Failed to parse preset YAML: {}", e))?;
    let mut list = match parsed {
        PresetYamlFormat::WrappedList(w) => w.presets,
        PresetYamlFormat::BareList(v) => v,
        PresetYamlFormat::Single(p) => vec![p],
    };
    if list.is_empty() {
        return Err("No presets found in YAML file.".to_string());
    }
    sort_presets(&mut list);
    Ok(list)
}

/// Serialize a single preset to YAML.
pub fn serialize_preset_to_yaml(preset: &InstrumentPreset) -> Result<String, String> {
    serde_saphyr::to_string(preset).map_err(|e| format!("Failed to serialize preset: {}", e))
}

/// Serialize a list of presets to YAML, sorted by EDO.
pub fn serialize_presets_list_to_yaml(presets: &[InstrumentPreset]) -> Result<String, String> {
    let mut sorted = presets.to_vec();
    sort_presets(&mut sorted);
    let wrapper = PresetListWrapper { presets: sorted };
    serde_saphyr::to_string(&wrapper).map_err(|e| format!("Failed to serialize presets: {}", e))
}

/// Built-in default presets ordered by EDO.
pub fn builtin_presets() -> Vec<InstrumentPreset> {
    let pitch = |step: &str, alter: Option<Alteration>, octave: i32| Pitch {
        step: step.to_string(),
        alter,
        octave,
        ..Default::default()
    };
    let make_tunings = |specs: &[(&str, Option<Alteration>, i32)]| -> Vec<StringTuning> {
        specs
            .iter()
            .enumerate()
            .map(|(i, (s, a, o))| StringTuning {
                string: (i + 1) as u8,
                pitch: pitch(s, a.clone(), *o),
            })
            .collect()
    };

    let mut presets = vec![
        InstrumentPreset {
            name: "Standard Guitar".to_string(),
            edo: 12,
            step_multiplier: 1,
            capo: None,
            tuning: make_tunings(&[
                ("E", None, 4),
                ("B", None, 3),
                ("G", None, 3),
                ("D", None, 3),
                ("A", None, 2),
                ("E", None, 2),
            ]),
        },
        InstrumentPreset {
            name: "Drop D Guitar".to_string(),
            edo: 12,
            step_multiplier: 1,
            capo: None,
            tuning: make_tunings(&[
                ("E", None, 4),
                ("B", None, 3),
                ("G", None, 3),
                ("D", None, 3),
                ("A", None, 2),
                ("D", None, 2),
            ]),
        },
        InstrumentPreset {
            name: "DADGAD Guitar".to_string(),
            edo: 12,
            step_multiplier: 1,
            capo: None,
            tuning: make_tunings(&[
                ("D", None, 4),
                ("A", None, 3),
                ("G", None, 3),
                ("D", None, 3),
                ("A", None, 2),
                ("D", None, 2),
            ]),
        },
        InstrumentPreset {
            name: "7-String Guitar".to_string(),
            edo: 12,
            step_multiplier: 1,
            capo: None,
            tuning: make_tunings(&[
                ("E", None, 4),
                ("B", None, 3),
                ("G", None, 3),
                ("D", None, 3),
                ("A", None, 2),
                ("E", None, 2),
                ("B", None, 1),
            ]),
        },
        InstrumentPreset {
            name: "Bass (4-String Standard)".to_string(),
            edo: 12,
            step_multiplier: 1,
            capo: None,
            tuning: make_tunings(&[
                ("G", None, 2),
                ("D", None, 2),
                ("A", None, 1),
                ("E", None, 1),
            ]),
        },
        InstrumentPreset {
            name: "19-EDO Guitar".to_string(),
            edo: 19,
            step_multiplier: 1,
            capo: None,
            tuning: make_tunings(&[
                ("E", None, 4),
                ("B", None, 3),
                ("G", None, 3),
                ("D", None, 3),
                ("A", None, 2),
                ("E", None, 2),
            ]),
        },
        InstrumentPreset {
            name: "22-EDO Guitar".to_string(),
            edo: 22,
            step_multiplier: 1,
            capo: None,
            tuning: make_tunings(&[
                ("E", None, 4),
                ("B", None, 3),
                ("G", None, 3),
                ("D", None, 3),
                ("A", None, 2),
                ("E", None, 2),
            ]),
        },
        InstrumentPreset {
            name: "31-EDO Guitar".to_string(),
            edo: 31,
            step_multiplier: 1,
            capo: None,
            tuning: make_tunings(&[
                ("E", None, 4),
                ("B", None, 3),
                ("G", None, 3),
                ("D", None, 3),
                ("A", None, 2),
                ("E", None, 2),
            ]),
        },
        InstrumentPreset {
            name: "Kite Guitar (8-String)".to_string(),
            edo: 41,
            step_multiplier: 2,
            capo: None,
            tuning: make_tunings(&[
                ("E", Some(Alteration::Fraction("-1\\41".to_string())), 4),
                ("C", None, 4),
                ("G", None, 3),
                ("E", Some(Alteration::Fraction("-1\\41".to_string())), 3),
                ("C", None, 3),
                ("A", Some(Alteration::Fraction("-1\\41".to_string())), 2),
                ("F", None, 2),
                ("C", None, 2),
            ]),
        },
    ];

    sort_presets(&mut presets);
    presets
}

pub fn user_presets_file_path() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("HOME") {
        let dir = Path::new(&home).join(".config").join("guitab");
        return Some(dir.join("presets.yaml"));
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        let dir = Path::new(&appdata).join("guitab");
        return Some(dir.join("presets.yaml"));
    }
    None
}

pub fn load_presets_with_user_config() -> Vec<InstrumentPreset> {
    let mut presets = builtin_presets();
    if cfg!(test) {
        return presets;
    }
    if let Some(path) = user_presets_file_path() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(loaded) = parse_presets_from_yaml(&content) {
                for p in loaded {
                    add_or_update_preset(&mut presets, p);
                }
            }
        }
    }
    presets
}

pub fn save_presets_to_user_config(presets: &[InstrumentPreset]) -> Result<(), String> {
    if cfg!(test) {
        return Ok(());
    }
    if let Some(path) = user_presets_file_path() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }
        let yaml = serialize_presets_list_to_yaml(presets)?;
        fs::write(&path, yaml).map_err(|e| format!("Failed to write presets file: {}", e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_presets_sorted_by_edo() {
        let presets = builtin_presets();
        assert!(!presets.is_empty());
        for w in presets.windows(2) {
            assert!(
                w[0].edo < w[1].edo
                    || (w[0].edo == w[1].edo
                        && w[0].name.to_lowercase() <= w[1].name.to_lowercase()),
                "Presets not sorted by EDO then name: {:?} vs {:?}",
                w[0],
                w[1]
            );
        }
    }

    #[test]
    fn test_add_or_update_preset_maintains_edo_order() {
        let mut presets = builtin_presets();
        let custom_24 = InstrumentPreset {
            name: "Quarter-Tone Guitar".to_string(),
            edo: 24,
            step_multiplier: 1,
            capo: None,
            tuning: presets[0].tuning.clone(),
        };
        add_or_update_preset(&mut presets, custom_24);

        let edos: Vec<u8> = presets.iter().map(|p| p.edo).collect();
        let mut sorted_edos = edos.clone();
        sorted_edos.sort();
        assert_eq!(edos, sorted_edos);
        assert!(presets.iter().any(|p| p.name == "Quarter-Tone Guitar" && p.edo == 24));
    }

    #[test]
    fn test_single_and_list_yaml_roundtrip() {
        let presets = builtin_presets();
        let single_yaml = serialize_preset_to_yaml(&presets[0]).unwrap();
        let parsed_single = parse_presets_from_yaml(&single_yaml).unwrap();
        assert_eq!(parsed_single.len(), 1);
        assert_eq!(parsed_single[0], presets[0]);

        let list_yaml = serialize_presets_list_to_yaml(&presets).unwrap();
        let parsed_list = parse_presets_from_yaml(&list_yaml).unwrap();
        assert_eq!(parsed_list.len(), presets.len());
        assert_eq!(parsed_list, presets);
    }

    #[test]
    fn test_dialog_state_roundtrip() {
        let kite = builtin_presets()
            .into_iter()
            .find(|p| p.edo == 41)
            .expect("Kite preset exists");

        let mut num_strings = 6;
        let mut edo = 12;
        let mut step_mult = 1;
        let mut texts = vec!["E".to_string(); 15];
        let mut octaves = vec![4; 15];

        kite.apply_to_dialog_state(
            &mut num_strings,
            &mut edo,
            &mut step_mult,
            &mut texts,
            &mut octaves,
        );

        assert_eq!(num_strings, 8);
        assert_eq!(edo, 41);
        assert_eq!(step_mult, 2);
        assert_eq!(texts[0], "vE");
        assert_eq!(octaves[0], 4);

        let reconstructed = InstrumentPreset::from_dialog_state(
            "Kite Guitar (8-String)",
            num_strings,
            edo,
            step_mult,
            &texts,
            &octaves,
        )
        .unwrap();
        assert_eq!(reconstructed, kite);
    }
}
