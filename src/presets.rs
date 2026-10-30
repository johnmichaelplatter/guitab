use crate::edo::{EdoTuning, generate_edo_scale};
use crate::mnx::{Alteration, Pitch, ReferenceNoteSpec, StringTuning};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstrumentPreset {
    pub name: String,
    #[serde(
        rename = "microFretDivisions",
        alias = "edo",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub edo: Option<u8>,
    #[serde(
        rename = "fretStepMultiplier",
        alias = "step_multiplier",
        alias = "stepMultiplier",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub step_multiplier: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capo: Option<u8>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "customFrets",
        alias = "custom_frets"
    )]
    pub custom_frets: Option<Vec<crate::fretboard_editor::CustomFret>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "referenceNote",
        alias = "reference_note"
    )]
    pub reference_note: Option<ReferenceNoteSpec>,
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
    pub fn edo(&self) -> u8 {
        self.edo.unwrap_or(12)
    }

    pub fn step_multiplier(&self) -> u8 {
        self.step_multiplier.unwrap_or(1)
    }

    pub fn has_custom_frets(&self) -> bool {
        self.custom_frets
            .as_ref()
            .map(|f| !f.is_empty())
            .unwrap_or(false)
    }

    pub fn has_non_edo_tuning(&self) -> bool {
        self.tuning.iter().any(|st| {
            crate::fretboard_editor::parse_ratio_or_cents_to_cents(&st.pitch.step).is_some()
        })
    }

    pub fn display_label(&self) -> String {
        if self.has_custom_frets() {
            format!("Custom — {} ({}-str)", self.name, self.tuning.len())
        } else if self.has_non_edo_tuning() {
            format!("JI / Cents — {} ({}-str)", self.name, self.tuning.len())
        } else {
            format!(
                "{} EDO — {} ({}-str)",
                self.edo(),
                self.name,
                self.tuning.len()
            )
        }
    }

    /// Construct an `InstrumentPreset` from the full fields of `ActiveDialog::InstrumentSetup`.
    pub fn from_dialog_state(
        name: &str,
        num_strings: u8,
        edo: u8,
        step_multiplier: u8,
        tuning_mode: crate::gui::InstrumentTuningMode,
        tunings_text: &[String],
        tunings_name: &[String],
        tunings_octave: &[i32],
        fretboard_editor: &crate::fretboard_editor::FretboardEditorState,
    ) -> Result<Self, String> {
        let clean_name = name.trim();
        if clean_name.is_empty() {
            return Err("Preset name cannot be empty.".to_string());
        }

        let mut tuning = Vec::with_capacity(num_strings as usize);
        if tuning_mode == crate::gui::InstrumentTuningMode::RatioCents {
            for s in 0..(num_strings as usize) {
                let text = tunings_text
                    .get(s)
                    .ok_or_else(|| format!("Missing tuning text for string {}", s + 1))?;
                let Some((norm_step, _)) =
                    crate::fretboard_editor::parse_ratio_or_cents_to_cents(text)
                else {
                    return Err(format!(
                        "String {} must specify a valid ratio (e.g. 4/3) or cents (e.g. 498.0).",
                        s + 1
                    ));
                };
                let p_name = tunings_name.get(s).map(|s| s.trim()).unwrap_or("");
                if !crate::fretboard_editor::is_valid_ups_downs_name(p_name) {
                    return Err(format!(
                        "String {} must have a valid ups/downs pitch name (e.g. G, vB).",
                        s + 1
                    ));
                }
                let octave = tunings_octave.get(s).copied().unwrap_or(4);
                tuning.push(StringTuning {
                    string: (s + 1) as u8,
                    pitch: Pitch {
                        step: norm_step,
                        alter: None,
                        octave,
                        ext: crate::mnx::PitchExt {
                            name: Some(p_name.to_string()),
                        },
                    },
                });
            }
        } else {
            let tuning_calc = EdoTuning::new(edo as u32);
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
        }

        let has_custom = !fretboard_editor.frets.is_empty();
        let custom_frets = if has_custom {
            Some(fretboard_editor.frets.clone())
        } else {
            None
        };

        let need_ref_note = has_custom || tuning_mode == crate::gui::InstrumentTuningMode::RatioCents;
        let reference_note = if need_ref_note {
            let ref_step = if fretboard_editor.reference_note_step.trim().is_empty() {
                "D".to_string()
            } else {
                fretboard_editor.reference_note_step.clone()
            };
            Some(ReferenceNoteSpec {
                pitch: Pitch {
                    step: ref_step,
                    alter: None,
                    octave: fretboard_editor.reference_note_octave,
                    ext: crate::mnx::PitchExt::default(),
                },
                frequency_hz: fretboard_editor.reference_frequency_hz,
            })
        } else {
            None
        };

        let (saved_edo, saved_step_multiplier) =
            if has_custom || tuning_mode == crate::gui::InstrumentTuningMode::RatioCents {
                (None, None)
            } else {
                (Some(edo), Some(step_multiplier.max(1)))
            };

        Ok(Self {
            name: clean_name.to_string(),
            edo: saved_edo,
            step_multiplier: saved_step_multiplier,
            capo: None,
            custom_frets,
            reference_note,
            tuning,
        })
    }

    /// Construct an `InstrumentPreset` from basic EDO dialog fields (convenience helper).
    pub fn from_edo_dialog_state(
        name: &str,
        num_strings: u8,
        edo: u8,
        step_multiplier: u8,
        tunings_text: &[String],
        tunings_octave: &[i32],
    ) -> Result<Self, String> {
        let dummy_editor = crate::fretboard_editor::FretboardEditorState::new(
            num_strings,
            Vec::new(),
            "D".to_string(),
            4,
            293.66,
        );
        Self::from_dialog_state(
            name,
            num_strings,
            edo,
            step_multiplier,
            crate::gui::InstrumentTuningMode::Edo,
            tunings_text,
            tunings_text,
            tunings_octave,
            &dummy_editor,
        )
    }

    /// Apply this preset's parameters into the `InstrumentSetup` dialog buffers.
    pub fn apply_to_dialog_state(
        &self,
        num_strings: &mut u8,
        edo: &mut u8,
        step_multiplier: &mut u8,
        tuning_mode: &mut crate::gui::InstrumentTuningMode,
        tunings_text: &mut Vec<String>,
        tunings_name: &mut Vec<String>,
        tunings_octave: &mut Vec<i32>,
        fretboard_editor: &mut crate::fretboard_editor::FretboardEditorState,
        show_custom_fretboard: &mut bool,
    ) {
        *num_strings = (self.tuning.len().clamp(1, 15)) as u8;
        if let Some(e) = self.edo {
            *edo = e.clamp(1, 100);
        }
        if let Some(m) = self.step_multiplier {
            *step_multiplier = m.clamp(1, 10);
        }

        if tunings_text.len() < 15 {
            tunings_text.resize(15, "E".to_string());
        }
        if tunings_name.len() < 15 {
            tunings_name.resize(15, "E".to_string());
        }
        if tunings_octave.len() < 15 {
            tunings_octave.resize(15, 2);
        }

        let has_custom = self.has_custom_frets();
        let has_ji = self.has_non_edo_tuning();

        if let Some(ref custom_frets) = self.custom_frets {
            fretboard_editor.frets = custom_frets.clone();
            crate::fretboard_editor::recompute_fret_numbers(
                &mut fretboard_editor.frets,
                fretboard_editor.num_strings,
            );
        } else {
            fretboard_editor.frets.clear();
        }

        if let Some(ref rn) = self.reference_note {
            fretboard_editor.reference_note_step = rn.pitch.step.clone();
            fretboard_editor.reference_note_octave = rn.pitch.octave;
            fretboard_editor.reference_frequency_hz = rn.frequency_hz;
        }

        *show_custom_fretboard = has_custom || has_ji;

        if has_ji {
            *tuning_mode = crate::gui::InstrumentTuningMode::RatioCents;
            for st in &self.tuning {
                let idx = (st.string.saturating_sub(1)) as usize;
                if idx < 15 {
                    tunings_text[idx] = st.pitch.step.clone();
                    let name = st
                        .pitch
                        .ext
                        .name
                        .as_ref()
                        .cloned()
                        .unwrap_or_else(|| st.pitch.step.clone());
                    tunings_name[idx] = name;
                    tunings_octave[idx] = st.pitch.octave;
                }
            }
        } else {
            *tuning_mode = crate::gui::InstrumentTuningMode::Edo;
            let edo_val = self.edo() as u32;
            let edo_scale = generate_edo_scale(edo_val);
            let tuning_calc = EdoTuning::new(edo_val);

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
                    tunings_text[idx] = text.clone();
                    tunings_name[idx] = text;
                    tunings_octave[idx] = st.pitch.octave;
                }
            }
        }

        fretboard_editor.sync_num_strings(*num_strings);
    }

    /// Apply this preset's parameters into the EDO-only dialog buffers (convenience helper).
    pub fn apply_to_edo_dialog_state(
        &self,
        num_strings: &mut u8,
        edo: &mut u8,
        step_multiplier: &mut u8,
        tunings_text: &mut Vec<String>,
        tunings_octave: &mut Vec<i32>,
    ) {
        let mut mode = crate::gui::InstrumentTuningMode::Edo;
        let mut names = tunings_text.clone();
        let mut fb = crate::fretboard_editor::FretboardEditorState::new(
            *num_strings,
            Vec::new(),
            "D".to_string(),
            4,
            293.66,
        );
        let mut show_fb = false;
        self.apply_to_dialog_state(
            num_strings,
            edo,
            step_multiplier,
            &mut mode,
            tunings_text,
            &mut names,
            tunings_octave,
            &mut fb,
            &mut show_fb,
        );
    }
}

/// Sort presets primarily by EDO ascending, then case-insensitively by name.
pub fn sort_presets(presets: &mut [InstrumentPreset]) {
    presets.sort_by(|a, b| {
        let key_a = (a.has_custom_frets() || a.has_non_edo_tuning(), a.edo.unwrap_or(255));
        let key_b = (b.has_custom_frets() || b.has_non_edo_tuning(), b.edo.unwrap_or(255));
        key_a
            .cmp(&key_b)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

/// Append a preset (or update if same EDO/custom frets and case-insensitive name) and re-sort.
pub fn add_or_update_preset(presets: &mut Vec<InstrumentPreset>, new_preset: InstrumentPreset) {
    if let Some(existing) = presets.iter_mut().find(|p| {
        p.edo == new_preset.edo
            && p.has_custom_frets() == new_preset.has_custom_frets()
            && p.name.eq_ignore_ascii_case(&new_preset.name)
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
            edo: Some(12),
            step_multiplier: Some(1),
            capo: None,
            custom_frets: None,
            reference_note: None,
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
            edo: Some(12),
            step_multiplier: Some(1),
            capo: None,
            custom_frets: None,
            reference_note: None,
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
            edo: Some(12),
            step_multiplier: Some(1),
            capo: None,
            custom_frets: None,
            reference_note: None,
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
            edo: Some(12),
            step_multiplier: Some(1),
            capo: None,
            custom_frets: None,
            reference_note: None,
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
            edo: Some(12),
            step_multiplier: Some(1),
            capo: None,
            custom_frets: None,
            reference_note: None,
            tuning: make_tunings(&[
                ("G", None, 2),
                ("D", None, 2),
                ("A", None, 1),
                ("E", None, 1),
            ]),
        },
        InstrumentPreset {
            name: "19-EDO Guitar".to_string(),
            edo: Some(19),
            step_multiplier: Some(1),
            capo: None,
            custom_frets: None,
            reference_note: None,
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
            edo: Some(22),
            step_multiplier: Some(1),
            capo: None,
            custom_frets: None,
            reference_note: None,
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
            edo: Some(31),
            step_multiplier: Some(1),
            capo: None,
            custom_frets: None,
            reference_note: None,
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
            edo: Some(41),
            step_multiplier: Some(2),
            capo: None,
            custom_frets: None,
            reference_note: None,
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
            let key0 = (
                w[0].has_custom_frets() || w[0].has_non_edo_tuning(),
                w[0].edo.unwrap_or(255),
            );
            let key1 = (
                w[1].has_custom_frets() || w[1].has_non_edo_tuning(),
                w[1].edo.unwrap_or(255),
            );
            assert!(
                key0 < key1
                    || (key0 == key1
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
            edo: Some(24),
            step_multiplier: Some(1),
            capo: None,
            custom_frets: None,
            reference_note: None,
            tuning: presets[0].tuning.clone(),
        };
        add_or_update_preset(&mut presets, custom_24);

        let edos: Vec<u8> = presets.iter().map(|p| p.edo()).collect();
        let mut sorted_edos = edos.clone();
        sorted_edos.sort();
        assert_eq!(edos, sorted_edos);
        assert!(presets.iter().any(|p| p.name == "Quarter-Tone Guitar" && p.edo == Some(24)));
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
            .find(|p| p.edo == Some(41))
            .expect("Kite preset exists");

        let mut num_strings = 6;
        let mut edo = 12;
        let mut step_mult = 1;
        let mut texts = vec!["E".to_string(); 15];
        let mut octaves = vec![4; 15];

        kite.apply_to_edo_dialog_state(
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

        let reconstructed = InstrumentPreset::from_edo_dialog_state(
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

    #[test]
    fn test_custom_fretboard_and_ji_preset_export_import_roundtrip() {
        use crate::fretboard_editor::{CustomFret, FretLocationSpec, FretboardEditorState};
        use crate::gui::InstrumentTuningMode;

        let custom_frets = vec![
            CustomFret {
                fret_number: 1.0,
                manual_number: None,
                spec: FretLocationSpec::Ratio { num: 22, den: 21 },
                strings: vec![1, 2, 3, 4, 5, 6],
                note_name: "Db".to_string(),
            },
            CustomFret {
                fret_number: 2.0,
                manual_number: None,
                spec: FretLocationSpec::Ratio { num: 13, den: 12 },
                strings: vec![1, 2, 3, 4, 5, 6],
                note_name: "^C#".to_string(),
            },
            CustomFret {
                fret_number: 3.0,
                manual_number: None,
                spec: FretLocationSpec::Ratio { num: 9, den: 8 },
                strings: vec![1, 2, 3, 4, 5, 6],
                note_name: "D".to_string(),
            },
        ];

        let ji_tunings = vec![
            StringTuning {
                string: 1,
                pitch: Pitch {
                    step: "4/3".to_string(),
                    alter: None,
                    octave: 4,
                    ext: crate::mnx::PitchExt {
                        name: Some("G".to_string()),
                    },
                },
            },
            StringTuning {
                string: 2,
                pitch: Pitch {
                    step: "5/3".to_string(),
                    alter: None,
                    octave: 4,
                    ext: crate::mnx::PitchExt {
                        name: Some("vB".to_string()),
                    },
                },
            },
            StringTuning {
                string: 3,
                pitch: Pitch {
                    step: "4/3".to_string(),
                    alter: None,
                    octave: 3,
                    ext: crate::mnx::PitchExt {
                        name: Some("G".to_string()),
                    },
                },
            },
            StringTuning {
                string: 4,
                pitch: Pitch {
                    step: "1/1".to_string(),
                    alter: None,
                    octave: 3,
                    ext: crate::mnx::PitchExt {
                        name: Some("D".to_string()),
                    },
                },
            },
            StringTuning {
                string: 5,
                pitch: Pitch {
                    step: "4/3".to_string(),
                    alter: None,
                    octave: 2,
                    ext: crate::mnx::PitchExt {
                        name: Some("G".to_string()),
                    },
                },
            },
            StringTuning {
                string: 6,
                pitch: Pitch {
                    step: "1/1".to_string(),
                    alter: None,
                    octave: 2,
                    ext: crate::mnx::PitchExt {
                        name: Some("D".to_string()),
                    },
                },
            },
        ];

        let ref_note = ReferenceNoteSpec {
            pitch: Pitch {
                step: "D".to_string(),
                alter: None,
                octave: 4,
                ext: Default::default(),
            },
            frequency_hz: 293.66,
        };

        let preset = InstrumentPreset {
            name: "11-Limit JI Fretless Guitar".to_string(),
            edo: None,
            step_multiplier: None,
            capo: None,
            custom_frets: Some(custom_frets.clone()),
            reference_note: Some(ref_note),
            tuning: ji_tunings,
        };

        assert!(preset.has_custom_frets());
        assert!(preset.has_non_edo_tuning());
        assert_eq!(preset.display_label(), "Custom — 11-Limit JI Fretless Guitar (6-str)");

        // 1. Serialize to YAML and inspect
        let yaml = serialize_preset_to_yaml(&preset).expect("serialization succeeds");
        assert!(yaml.contains("customFrets:"));
        assert!(yaml.contains("referenceNote:"));
        assert!(yaml.contains("frequencyHz: 293.66"));
        assert!(yaml.contains("step: 4/3"));
        assert!(yaml.contains("name: vB"));
        assert!(!yaml.contains("microFretDivisions"));
        assert!(!yaml.contains("fretStepMultiplier"));

        // 2. Parse back from YAML
        let parsed = parse_presets_from_yaml(&yaml).expect("deserialization succeeds");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0], preset);

        // 3. Apply to dialog state
        let mut num_strings = 4;
        let mut edo = 12;
        let mut step_mult = 1;
        let mut tuning_mode = InstrumentTuningMode::Edo;
        let mut tunings_text = vec!["E".to_string(); 15];
        let mut tunings_name = vec!["E".to_string(); 15];
        let mut tunings_octave = vec![4; 15];
        let mut fb_editor = FretboardEditorState::new(4, Vec::new(), "E".to_string(), 4, 329.63);
        let mut show_fb = false;

        preset.apply_to_dialog_state(
            &mut num_strings,
            &mut edo,
            &mut step_mult,
            &mut tuning_mode,
            &mut tunings_text,
            &mut tunings_name,
            &mut tunings_octave,
            &mut fb_editor,
            &mut show_fb,
        );

        assert_eq!(num_strings, 6);
        assert_eq!(tuning_mode, InstrumentTuningMode::RatioCents);
        assert!(show_fb);
        assert_eq!(tunings_text[0], "4/3");
        assert_eq!(tunings_name[0], "G");
        assert_eq!(tunings_text[1], "5/3");
        assert_eq!(tunings_name[1], "vB");
        assert_eq!(tunings_text[2], "4/3");
        assert_eq!(tunings_name[2], "G");
        assert_eq!(tunings_text[3], "1/1");
        assert_eq!(tunings_name[3], "D");
        assert_eq!(fb_editor.frets.len(), 3);
        assert_eq!(fb_editor.frets[0].strings, vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(fb_editor.reference_note_step, "D");
        assert_eq!(fb_editor.reference_note_octave, 4);
        assert_eq!(fb_editor.reference_frequency_hz, 293.66);

        // 4. Reconstruct from dialog state
        let reconstructed = InstrumentPreset::from_dialog_state(
            "11-Limit JI Fretless Guitar",
            num_strings,
            edo,
            step_mult,
            tuning_mode,
            &tunings_text,
            &tunings_name,
            &tunings_octave,
            &fb_editor,
        )
        .expect("reconstruction from dialog succeeds");

        assert_eq!(reconstructed, preset);
    }
}
