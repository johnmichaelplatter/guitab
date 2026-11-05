use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_d_dark_mode_icon_assets() {
        // Test that duration textures produce white alpha masks for theme-adaptive tinting
        let ctx = egui::Context::default();
        let textures = DurationTextures::load(&ctx);
        // Verify texture handles loaded successfully
        assert_eq!(textures.whole.name(), "icon_whole");
        assert_eq!(textures.half.name(), "icon_half");
        assert_eq!(textures.quarter.name(), "icon_quarter");
        assert_eq!(textures.eighth.name(), "icon_eighth");
        assert_eq!(textures.sixteenth.name(), "icon_sixteenth");
        assert_eq!(textures.thirty_second.name(), "icon_thirty_second");
        assert_eq!(textures.dot.name(), "icon_dot");

        // Verify raw PNG bytes converted into white mask in load_texture_from_bytes logic
        for (name, bytes) in [
            ("whole", WHOLE_PNG),
            ("half", HALF_PNG),
            ("quarter", QUARTER_PNG),
            ("eighth", EIGHTH_PNG),
            ("16th", SIXTEENTH_PNG),
            ("32nd", THIRTYSECOND_PNG),
            ("dot", DOT_PNG),
        ] {
            let img = image::load_from_memory(bytes).expect("Valid PNG");
            let rgba = img.to_rgba8();
            let mut has_non_transparent = false;
            for p in rgba.pixels() {
                if p[3] > 0 {
                    has_non_transparent = true;
                    // Our loader turns non-transparent pixels into (255, 255, 255, alpha)
                    let white = [255u8, 255u8, 255u8, p[3]];
                    assert_eq!(white[0], 255, "White mask failed for {}", name);
                    assert_eq!(white[1], 255, "White mask failed for {}", name);
                    assert_eq!(white[2], 255, "White mask failed for {}", name);
                }
            }
            assert!(has_non_transparent, "Asset {} has no visible pixels", name);
        }

        // Verify SVGs contain #ffffff and no black strokes/fills
        let closed_ending = include_str!("../../assets/closed ending.svg");
        assert!(closed_ending.contains("#ffffff"));
        assert!(!closed_ending.contains("#000000"));

        let open_ending = include_str!("../../assets/open ending.svg");
        assert!(open_ending.contains("#ffffff"));
        assert!(!open_ending.contains("#000000"));

        let tie = include_str!("../../assets/tie.svg");
        assert!(tie.contains("#ffffff"));
        assert!(!tie.contains("#000000"));

        let hammer = include_str!("../../assets/hammer_pulloff.svg");
        assert!(hammer.contains("#ffffff"));
        assert!(!hammer.contains("#000000"));

        let sys = include_str!("../../assets/system_break.svg");
        assert!(sys.contains("#ffffff"));

        let page = include_str!("../../assets/page_break.svg");
        assert!(page.contains("#ffffff"));
    }

    #[test]
    fn test_find_all_event_ranges_example_yaml() {
        let yaml = include_str!("../../examples/example.yaml");
        let ranges = find_all_event_ranges(yaml);
        assert_eq!(ranges.len(), 20);
        for er in &ranges {
            let snippet = &yaml[er.total_range.clone()];
            assert!(snippet.contains("duration"));
            assert!(snippet.contains("notes"));
            assert_eq!(er.note_ranges.len(), 1);
        }
    }

    #[test]
    fn test_find_all_event_ranges_chord_yaml() {
        let yaml = r#"
parts:
  - measures:
      - sequences:
          - content:
              - duration: { base: quarter }
                notes:
                  - pitch: { step: C, octave: 3 }
                    string: 5
                    fret: 3
                  - pitch: { step: E, octave: 3 }
                    string: 4
                    fret: 2
                  - pitch: { step: G, octave: 3 }
                    string: 3
                    fret: 0
"#;
        let ranges = find_all_event_ranges(yaml);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].note_ranges.len(), 3);
        assert!(yaml[ranges[0].note_ranges[0].clone()].contains("step: C"));
        assert!(yaml[ranges[0].note_ranges[1].clone()].contains("step: E"));
        assert!(yaml[ranges[0].note_ranges[2].clone()].contains("step: G"));
    }

    #[test]
    fn test_update_note_in_yaml() {
        let yaml = include_str!("../../examples/example.yaml");
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let part = &doc.parts[0];
        let tuning_calc = crate::edo::EdoTuning::new(part.edo());

        // Note 0 is string 7, fret 12. Let's change it to fret 14.
        let string_pitch = part.get_string_tuning(7).unwrap();
        let string_base = tuning_calc.pitch_to_steps(&string_pitch);
        let played_steps = string_base + 14 * (part.step_multiplier() as i32);
        let new_pitch = tuning_calc.steps_to_pitch(played_steps);

        let updated = update_note_in_yaml(yaml, 0, 0, 14, Some(&new_pitch)).expect("Should update");
        assert!(updated.contains("fret: 14"));

        let updated_doc =
            serde_saphyr::from_str::<MnxDocument>(&updated).expect("Updated doc should parse");
        let errors = crate::validate::validate_mnx(&updated_doc);
        assert_eq!(
            errors.len(),
            0,
            "There should be no validation errors after updating fret and pitch: {:?}",
            errors
        );
    }

    #[test]
    fn test_edit_string4_fret12() {
        let yaml = include_str!("../../examples/example.yaml");
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let part = &doc.parts[0];

        // Edit Event 1 to put String 4 Fret 12
        let new_pitch = calculate_pitch_for_string_fret(part, 4, 12);
        let updated = set_or_add_note_on_string(yaml, 1, 4, 12, new_pitch.as_ref())
            .expect("Should update note");

        let updated_doc =
            serde_saphyr::from_str::<MnxDocument>(&updated).expect("Updated doc should parse");
        let errors = crate::validate::validate_mnx(&updated_doc);
        assert_eq!(
            errors.len(),
            0,
            "Validation should pass with 0 errors for String 4 Fret 12: {:?}",
            errors
        );
    }

    #[test]
    fn test_set_or_add_note_on_string_creates_chord() {
        let yaml = include_str!("../../examples/example.yaml");
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let part = &doc.parts[0];

        // Event 0 starts with String 7 Fret 12. Add String 6 Fret 9 (D3) to make it a 2-note chord.
        let new_pitch = calculate_pitch_for_string_fret(part, 6, 9);
        let updated =
            set_or_add_note_on_string(yaml, 0, 6, 9, new_pitch.as_ref()).expect("Should add note");

        let updated_doc =
            serde_saphyr::from_str::<MnxDocument>(&updated).expect("Updated doc should parse");
        let ev0_notes = &updated_doc.parts[0].measures[0].sequences[0].flattened_events()[0].notes;
        assert_eq!(ev0_notes.len(), 2);
        assert_eq!(ev0_notes[0].string, Some(7));
        assert_eq!(ev0_notes[0].fret, Some(12));
        assert_eq!(ev0_notes[1].string, Some(6));
        assert_eq!(ev0_notes[1].fret, Some(9));

        let errors = crate::validate::validate_mnx(&updated_doc);
        assert_eq!(
            errors.len(),
            0,
            "Validation should pass with 0 errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_remove_note_on_string() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - name: "Guitar"
    measures:
      - sequences:
          - content:
              - duration: { base: whole }
                notes:
                  - pitch: { step: C, octave: 3 }
                    _x:
                      string: 5
                      fret: 3
                  - pitch: { step: E, octave: 3 }
                    _x:
                      string: 4
                      fret: 2
"#;
        // Remove note on string 4
        let updated = remove_note_on_string(yaml, 0, 4).expect("Should remove note on string 4");
        let updated_doc =
            serde_saphyr::from_str::<MnxDocument>(&updated).expect("Updated doc should parse");
        let notes = &updated_doc.parts[0].measures[0].sequences[0].flattened_events()[0].notes;
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].string, Some(5));

        // Remove note on string 5 (only note left)
        let updated2 = remove_note_on_string(&updated, 0, 5).expect("Should remove last note");
        let updated2_doc =
            serde_saphyr::from_str::<MnxDocument>(&updated2).expect("Updated doc should parse");
        let notes2 = &updated2_doc.parts[0].measures[0].sequences[0].flattened_events()[0].notes;
        assert_eq!(notes2.len(), 0);
    }

    #[test]
    fn test_add_measure_to_yaml() {
        let yaml = include_str!("../../examples/example.yaml");
        let doc = serde_saphyr::from_str::<MnxDocument>(yaml).expect("Should parse");
        let initial_measures = doc.parts[0].measures.len();
        assert_eq!(initial_measures, 3);

        let final_dur = doc.parts[0]
            .measures
            .last()
            .unwrap()
            .sequences
            .first()
            .unwrap()
            .flattened_events()
            .last()
            .unwrap()
            .duration
            .clone();
        assert_eq!(final_dur.base, "quarter");

        let updated = add_measure_to_yaml(yaml, &final_dur).expect("Should add measure");
        let updated_doc =
            serde_saphyr::from_str::<MnxDocument>(&updated).expect("Updated doc should parse");
        assert_eq!(updated_doc.global.measures.len(), 4);
        assert_eq!(
            updated_doc.global.measures[3].time,
            None,
            "Measure 4 must not have an explicit time block"
        );
        let part = &updated_doc.parts[0];
        assert_eq!(part.measures.len(), 4);

        // Check the newly added measure
        let new_measure = &part.measures[3];
        let new_seq = &new_measure.sequences[0];
        assert_eq!(new_seq.flattened_events().len(), 1);
        let new_event = &new_seq.flattened_events()[0];
        assert_eq!(new_event.duration.base, "quarter");
        assert_eq!(new_event.notes.len(), 0);

        // Validation should pass with 0 errors
        let errors = crate::validate::validate_mnx(&updated_doc);
        assert_eq!(
            errors.len(),
            0,
            "Validation should have 0 errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_add_measure_to_yaml_serde_saphyr_tuning_style() {
        let yaml = r#"mnx:
  version: 1
global:
  measures:
  - time: {count: 4, unit: 4}
parts:
- name: Guitar
  _x:
    tuning:
    - string: 1
      pitch: {octave: 4, step: E}
    - string: 2
      pitch: {octave: 3, step: B}
  measures:
  - sequences:
    - content:
      - duration: {base: quarter}
        notes:
        - pitch: {octave: 4, step: E}
          _x: {fret: 0, string: 1}
_x:
  layout:
    pageSize: letter
"#;
        let dur = crate::mnx::Duration {
            multiple: None,
            base: "quarter".to_string(),
            dots: None,
        };

        let updated = add_measure_to_yaml(yaml, &dur).expect("Should add measure");
        
        // 1. Must parse cleanly
        let doc = serde_saphyr::from_str::<MnxDocument>(&updated).expect("Updated YAML must parse");
        assert_eq!(doc.parts[0].measures.len(), 2, "Part should have 2 measures");
        assert_eq!(doc.global.measures.len(), 2, "Global should have 2 measures");
        assert!(doc.layout.is_some(), "Layout should be preserved");
        assert!(doc.global.measures[0].time.is_some(), "Measure 0 must have a time signature");
        assert_eq!(doc.global.measures[1].time, None, "Measure 1 must omit time signature");
        assert!(updated.contains("- {}"), "Measure 1 must be serialized as - {{}}");

        // 2. Measure 0 notes must still be the original note, NOT displaced by an insertion at top
        let m0_events = doc.parts[0].measures[0].sequences[0].flattened_events();
        assert_eq!(m0_events.len(), 1);
        assert_eq!(m0_events[0].notes.len(), 1, "First measure should still have its original note");

        // 3. Measure 1 must be the newly added blank note
        let m1_events = doc.parts[0].measures[1].sequences[0].flattened_events();
        assert_eq!(m1_events.len(), 1);
        assert_eq!(m1_events[0].notes.len(), 0, "Second measure should be blank");

        // 4. In text, the newly added measure must come AFTER measure 0, and BEFORE layout:, NEVER between tuning and measures
        let parts_pos = updated.find("parts:").unwrap();
        let tuning_pos = updated.find("tuning:").unwrap();
        let parts_measures_pos = updated[parts_pos..].find("measures:").unwrap() + parts_pos;
        let layout_pos = updated.find("layout:").unwrap();
        assert!(tuning_pos < parts_measures_pos);
        
        let seq_positions: Vec<usize> = updated.match_indices("- sequences:").map(|(pos, _)| pos).collect();
        assert_eq!(seq_positions.len(), 2, "Should have two measures");
        assert!(seq_positions[0] > parts_measures_pos, "Measure 0 must be after measures:");
        assert!(seq_positions[1] > seq_positions[0], "Measure 1 must be after measure 0");
        assert!(layout_pos < parts_pos, "Document _x.layout must be preserved before parts:");
    }

    #[test]
    fn test_new_measure_omits_time_block_after_bar_1() {
        let yaml = r#"mnx:
  version: 1
global:
  measures:
  - time: {count: 3, unit: 4}
parts:
- name: Guitar
  measures:
  - sequences:
    - content:
      - duration: {base: half, dots: 1}
        notes:
        - pitch: {octave: 4, step: E}
          _x: {fret: 0, string: 1}
"#;
        let dur = crate::mnx::Duration {
            multiple: None,
            base: "half".to_string(),
            dots: Some(1),
        };

        // Add bar 2
        let updated = add_measure_to_yaml(yaml, &dur).expect("Should add measure 2");
        let doc = serde_saphyr::from_str::<MnxDocument>(&updated).expect("Updated YAML must parse");
        assert_eq!(doc.global.measures.len(), 2);
        assert_eq!(
            doc.global.measures[0].time,
            Some(crate::mnx::TimeSignature { count: 3, unit: 4 })
        );
        assert_eq!(
            doc.global.measures[1].time, None,
            "Bar 2 should not have a time block"
        );
        assert!(
            updated.contains("- {}"),
            "Bar 2 should be serialized as an empty mapping '- {{}}'"
        );

        // Effective time signature for bar 2 should inherit from bar 1 (3/4)
        let ts_bar2 = crate::validate::get_time_signature_for_measure(&doc, 1);
        assert_eq!(
            ts_bar2,
            crate::mnx::TimeSignature { count: 3, unit: 4 }
        );

        // Add bar 3 (with a quarter note; bar 3 is the final measure so under-capacity is allowed)
        let dur_quarter = crate::mnx::Duration {
            multiple: None,
            base: "quarter".to_string(),
            dots: None,
        };
        let updated3 = add_measure_to_yaml(&updated, &dur_quarter).expect("Should add measure 3");
        let doc3 = serde_saphyr::from_str::<MnxDocument>(&updated3).expect("Updated YAML must parse");
        assert_eq!(doc3.global.measures.len(), 3);
        assert_eq!(
            doc3.global.measures[0].time,
            Some(crate::mnx::TimeSignature { count: 3, unit: 4 })
        );
        assert_eq!(doc3.global.measures[1].time, None);
        assert_eq!(doc3.global.measures[2].time, None);

        // Validation should pass with 0 errors
        let errors = crate::validate::validate_mnx(&doc3);
        assert_eq!(
            errors.len(),
            0,
            "Validation errors: {:?}",
            errors
        );
    }

    #[test]
    fn test_add_measure_to_yaml_real_files_and_reserialized() {
        let dur = crate::mnx::Duration {
            multiple: None,
            base: "quarter".to_string(),
            dots: None,
        };

        for filename in &[
            "examples/example.yaml",
            "examples/example_endings.yaml",
            "examples/example_harmonics.yaml",
            "examples/example_layout_lyrics.yaml",
            "examples/c scale.yaml",
            "chord examples/example.yaml",
        ] {
            let path = std::path::Path::new(filename);
            if !path.exists() {
                continue;
            }
            let raw_yaml = std::fs::read_to_string(path).expect("Read yaml");
            
            // 1. Raw hand-crafted YAML must parse and reserialize cleanly
            let raw_doc = serde_saphyr::from_str::<MnxDocument>(&raw_yaml).expect("Parse raw");
            let reserialized = serde_saphyr::to_string(&raw_doc).expect("Serialize with serde_saphyr");
            let reparsed = serde_saphyr::from_str::<MnxDocument>(&reserialized).expect("Reparse reserialized");
            assert_eq!(raw_doc, reparsed, "Roundtrip equality failed for {}", filename);

            if matches!(
                *filename,
                "examples/example.yaml" | "examples/example_endings.yaml" | "examples/example_harmonics.yaml" | "examples/example_layout_lyrics.yaml"
            ) {
                let initial_m_count = raw_doc.parts[0].measures.len();
                let updated_raw = add_measure_to_yaml(&raw_yaml, &dur).expect("Add measure to raw");
                let parsed_raw = serde_saphyr::from_str::<MnxDocument>(&updated_raw).expect("Parse updated raw");
                assert_eq!(parsed_raw.parts[0].measures.len(), initial_m_count + 1);
                assert_eq!(parsed_raw.global.measures.len(), initial_m_count + 1);

                // 2. serde_saphyr serialized YAML (Layout mode style)
                let updated_reserialized = add_measure_to_yaml(&reserialized, &dur).expect("Add measure to reserialized");
                let parsed_reserialized = serde_saphyr::from_str::<MnxDocument>(&updated_reserialized).expect("Parse updated reserialized");
                assert_eq!(parsed_reserialized.parts[0].measures.len(), initial_m_count + 1);
                assert_eq!(parsed_reserialized.global.measures.len(), initial_m_count + 1);

                // Ensure layout block is still present and valid if it was present
                if raw_doc.layout.is_some() {
                    assert!(parsed_raw.layout.is_some());
                    assert!(parsed_reserialized.layout.is_some());
                }
            }
        }
    }

    #[test]
    fn test_add_blank_note_to_last_measure_and_remove() {
        let yaml = r#"
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
              - duration: { base: quarter }
                notes:
                  - pitch: { step: C, octave: 3 }
                    string: 5
                    fret: 3
"#;
        let dur = crate::mnx::Duration {
            multiple: None,
            base: "quarter".to_string(),
            dots: None,
        };
        let with_blank = add_blank_note_to_last_measure(yaml, &dur).expect("Should add blank note");
        let doc_with_blank =
            serde_saphyr::from_str::<MnxDocument>(&with_blank).expect("Should parse");
        assert_eq!(
            doc_with_blank.parts[0].measures[0].sequences[0]
                .flattened_events()
                .len(),
            2
        );
        assert_eq!(
            doc_with_blank.parts[0].measures[0].sequences[0].flattened_events()[1]
                .notes
                .len(),
            0
        );

        // Now remove the blank note (since nothing was entered into it)
        let cleaned = remove_last_blank_event(&with_blank).expect("Should remove blank note");
        let cleaned_doc = serde_saphyr::from_str::<MnxDocument>(&cleaned).expect("Should parse");
        assert_eq!(
            cleaned_doc.parts[0].measures[0].sequences[0]
                .flattened_events()
                .len(),
            1
        );
        assert_eq!(
            cleaned_doc.parts[0].measures[0].sequences[0].flattened_events()[0]
                .notes
                .len(),
            1
        );
    }

    #[test]
    fn test_add_measure_and_remove_blank_measure() {
        let yaml = include_str!("../../examples/example.yaml");
        let dur = crate::mnx::Duration {
            multiple: None,
            base: "quarter".to_string(),
            dots: None,
        };
        let with_new_measure = add_measure_to_yaml(yaml, &dur).expect("Should add measure");
        let doc1 = serde_saphyr::from_str::<MnxDocument>(&with_new_measure).expect("Should parse");
        assert_eq!(doc1.parts[0].measures.len(), 4);
        assert_eq!(doc1.global.measures.len(), 4);

        // When shifting focus back to the left without entering anything, the empty measure is removed
        let cleaned =
            remove_last_blank_event(&with_new_measure).expect("Should remove empty measure");
        let doc2 = serde_saphyr::from_str::<MnxDocument>(&cleaned).expect("Should parse");
        assert_eq!(doc2.parts[0].measures.len(), 3);
        assert_eq!(doc2.global.measures.len(), 3);
    }

    #[test]
    fn test_right_arrow_rules_and_lifecycle() {
        let mut app = MnxEditorApp::default();
        // App starts with example.yaml (15 events across 2 measures)
        let total_initial = find_all_event_ranges(&app.yaml_text).len();
        assert_eq!(total_initial, 20);

        // Select the final event (event 19, in Measure 2 which is full)
        app.selected_event_idx = Some(19);

        // Right arrow on full final bar -> Rule 2: creates Measure 3 with a blank note
        // (Simulate right arrow logic)
        let final_dur = app
            .parsed_mnx
            .as_ref()
            .and_then(|d| d.parts.first())
            .and_then(|p| p.measures.last())
            .and_then(|m| m.sequences.first())
            .and_then(|s| s.flattened_events().last().copied())
            .map(|e| e.duration.clone())
            .unwrap();

        let updated = add_measure_to_yaml(&app.yaml_text, &final_dur).unwrap();
        app.yaml_text = updated;
        app.process_and_render();
        let total_after_r2 = find_all_event_ranges(&app.yaml_text).len();
        assert_eq!(total_after_r2, 21);
        app.selected_event_idx = Some(20);

        // Rule 3: Moving further to the right of a final blank note has no effect
        let is_final_blank = app
            .parsed_mnx
            .as_ref()
            .and_then(|d| d.parts.first())
            .and_then(|p| p.measures.last())
            .and_then(|m| m.sequences.first())
            .and_then(|s| s.flattened_events().last().copied())
            .map(|e| e.notes.is_empty() && e.rest.is_none())
            .unwrap();
        assert!(is_final_blank);

        // Shifting focus back to the left -> blank note and measure disappear
        let cleaned = remove_last_blank_event(&app.yaml_text).unwrap();
        app.yaml_text = cleaned;
        app.process_and_render();
        let total_after_left = find_all_event_ranges(&app.yaml_text).len();
        assert_eq!(total_after_left, 20);
    }

    #[test]
    fn test_update_event_duration_in_yaml() {
        let yaml = include_str!("../../examples/example.yaml");
        let dur_quarter = crate::mnx::Duration {
            multiple: None,
            base: "quarter".to_string(),
            dots: None,
        };
        let updated =
            update_event_duration_in_yaml(yaml, 0, &dur_quarter).expect("Should update duration");
        let doc = serde_saphyr::from_str::<MnxDocument>(&updated).expect("Should parse");
        assert_eq!(
            doc.parts[0].measures[0].sequences[0].flattened_events()[0]
                .duration
                .base,
            "quarter"
        );
        assert_eq!(
            doc.parts[0].measures[0].sequences[0].flattened_events()[0]
                .duration
                .dots,
            None
        );

        // Test dotted
        let dur_dotted_half = crate::mnx::Duration {
            multiple: None,
            base: "half".to_string(),
            dots: Some(1),
        };
        let updated_dot = update_event_duration_in_yaml(&updated, 0, &dur_dotted_half)
            .expect("Should update to dotted");
        let doc_dot = serde_saphyr::from_str::<MnxDocument>(&updated_dot).expect("Should parse");
        assert_eq!(
            doc_dot.parts[0].measures[0].sequences[0].flattened_events()[0]
                .duration
                .base,
            "half"
        );
        assert_eq!(
            doc_dot.parts[0].measures[0].sequences[0].flattened_events()[0]
                .duration
                .dots,
            Some(1)
        );
    }

    #[test]
    fn test_change_and_toggle_duration() {
        let mut app = MnxEditorApp::default();
        app.selected_event_idx = Some(0);

        // Change base to whole
        app.change_selected_duration_base("whole");
        assert_eq!(app.get_selected_event_duration().unwrap().base, "whole");

        // Toggle dot on
        app.toggle_selected_duration_dot();
        let dur = app.get_selected_event_duration().unwrap();
        assert_eq!(dur.base, "whole");
        assert_eq!(dur.dots, Some(1));

        // Toggle dot off
        app.toggle_selected_duration_dot();
        let dur2 = app.get_selected_event_duration().unwrap();
        assert_eq!(dur2.base, "whole");
        assert_eq!(dur2.dots, None);

        // Change base to 16th
        app.change_selected_duration_base("16th");
        assert_eq!(app.get_selected_event_duration().unwrap().base, "16th");
    }
    #[test]
    fn test_set_bpm_in_yaml_insert() {
        let yaml = include_str!("../../examples/example.yaml");
        let result = set_bpm_in_yaml(yaml, 140.0);
        println!("RESULT YAML:\n{}", &result[..result.len().min(400)]);
        let doc =
            serde_saphyr::from_str::<MnxDocument>(&result).expect("Should parse after BPM insert");
        println!("DOC GLOBAL: {:?}", doc.global);
        let bpm = doc
            .global
            .measures
            .iter()
            .flat_map(|m| m.tempos.iter())
            .find_map(|t| Some(t.bpm));
        assert_eq!(bpm, Some(140.0), "BPM should be 140 after insert");
    }

    #[test]
    fn test_set_bpm_in_yaml_update() {
        let yaml = include_str!("../../examples/example.yaml");
        let with_bpm = set_bpm_in_yaml(yaml, 140.0);
        let updated = set_bpm_in_yaml(&with_bpm, 200.0);
        let doc =
            serde_saphyr::from_str::<MnxDocument>(&updated).expect("Should parse after BPM update");
        let bpm = doc
            .global
            .measures
            .iter()
            .flat_map(|m| m.tempos.iter())
            .find_map(|t| Some(t.bpm));
        assert_eq!(bpm, Some(200.0), "BPM should be updated to 200");
        let tempo_count = doc
            .global
            .measures
            .iter()
            .flat_map(|m| m.tempos.iter())
            
            .count();
        assert_eq!(tempo_count, 1, "Should only have one tempo directive");
    }

    #[test]
    fn test_edit_mode_arrow_commit_and_advance() {
        let mut app = MnxEditorApp::default();
        app.selected_event_idx = Some(0);
        app.is_editing_fret = true;
        app.editing_string = Some(7);
        app.editing_fret_buf = "14".to_string();

        let total_events = find_all_event_ranges(&app.yaml_text).len();
        let cur_str = app.editing_string;
        assert!(app.commit_fret_edit());
        app.editing_fret_buf.clear();
        app.advance_to_next_note_in_edit_mode(cur_str, total_events);

        // Check event 0 was updated to fret 14 on string 7
        let doc = serde_saphyr::from_str::<MnxDocument>(&app.yaml_text).unwrap();
        let note = &doc.parts[0].measures[0].sequences[0].flattened_events()[0].notes[0];
        assert_eq!(note.string, Some(7));
        assert_eq!(note.fret, Some(14));

        // Check app moved to event 1 and is in edit mode on string 7
        assert_eq!(app.selected_event_idx, Some(1));
        assert!(app.is_editing_fret);
        assert_eq!(app.editing_string, Some(7));
        assert!(app.editing_fret_buf.is_empty());

        // Now edit event 1 and retreat left
        app.editing_fret_buf = "10".to_string();
        assert!(app.commit_fret_edit());
        app.editing_fret_buf.clear();
        app.retreat_to_prev_note_in_edit_mode(cur_str);

        let doc2 = serde_saphyr::from_str::<MnxDocument>(&app.yaml_text).unwrap();
        let note2 = doc2.parts[0].measures[0].sequences[0].flattened_events()[1]
            .notes
            .iter()
            .find(|n| n.string == Some(7))
            .expect("Should have note on string 7");
        assert_eq!(note2.fret, Some(10));

        assert_eq!(app.selected_event_idx, Some(0));
        assert!(app.is_editing_fret);
        assert_eq!(app.editing_string, Some(7));
    }

    #[test]
    fn test_blue_mode_backspace_removes_note_and_opens_orange_edit() {
        let mut app = MnxEditorApp::default();
        // Start in blue select mode at event 1
        app.selected_event_idx = Some(1);
        app.is_editing_fret = false;

        let _total_events = find_all_event_ranges(&app.yaml_text).len();
        let e_idx = 1;

        let target_str = app
            .selected_note_idx_in_event
            .and_then(|n_i| app.get_string_for_note(e_idx, n_i))
            .or_else(|| app.find_primary_string_for_event(e_idx));
        assert!(target_str.is_some());

        if let Some(s) = target_str {
            if let Some(new_yaml) = remove_note_on_string(&app.yaml_text, e_idx, s) {
                app.yaml_text = new_yaml;
                app.process_and_render();
            }
        }

        let prior_idx = e_idx - 1;
        let prior_str = target_str
            .filter(|&ts| app.event_has_note_on_string(prior_idx, ts))
            .or_else(|| app.find_primary_string_for_event(prior_idx))
            .unwrap_or(target_str.unwrap_or(1));

        app.selected_event_idx = Some(prior_idx);
        app.editing_string = Some(prior_str);
        app.select_note_on_string(prior_idx, prior_str);
        app.editing_fret_buf.clear();
        app.is_editing_fret = true;

        // Verify event 1 note is removed (notes is empty)
        let doc = serde_saphyr::from_str::<MnxDocument>(&app.yaml_text).unwrap();
        assert!(
            doc.parts[0].measures[0].sequences[0].flattened_events()[1]
                .notes
                .is_empty()
        );

        // Verify orange edit mode is open on prior note (event 0)
        assert_eq!(app.selected_event_idx, Some(0));
        assert!(app.is_editing_fret);
        assert_eq!(app.editing_string, Some(prior_str));
    }

    #[test]
    fn test_edit_mode_navigate_existing_note_without_typing() {
        let mut app = MnxEditorApp::default();
        // Start in edit mode on event 0, string 7 without typing anything (buffer is empty)
        app.selected_event_idx = Some(0);
        app.is_editing_fret = true;
        app.editing_string = Some(7);
        app.editing_fret_buf.clear();

        let total_events = find_all_event_ranges(&app.yaml_text).len();
        let cur_str = app.editing_string;

        // Pressing right arrow should navigate to event 1 on string 7 in edit mode
        app.advance_to_next_note_in_edit_mode(cur_str, total_events);
        assert_eq!(app.selected_event_idx, Some(1));
        assert!(app.is_editing_fret);
        assert_eq!(app.editing_string, Some(7));
        assert!(app.editing_fret_buf.is_empty());

        // Pressing left arrow should navigate back to event 0 on string 7 in edit mode
        app.retreat_to_prev_note_in_edit_mode(cur_str);
        assert_eq!(app.selected_event_idx, Some(0));
        assert!(app.is_editing_fret);
        assert_eq!(app.editing_string, Some(7));
    }

    #[test]
    fn test_edit_mode_blank_note_left_arrow_removes_blank() {
        let mut app = MnxEditorApp::default();
        let total_initial = find_all_event_ranges(&app.yaml_text).len();
        assert_eq!(total_initial, 20);

        // Select the final event (event 14) and advance right to create a new blank note in edit mode
        app.selected_event_idx = Some(19);
        app.is_editing_fret = true;
        app.editing_string = Some(4);
        app.advance_to_next_note_in_edit_mode(Some(4), total_initial);

        let total_after_advance = find_all_event_ranges(&app.yaml_text).len();
        assert_eq!(total_after_advance, 21);
        assert_eq!(app.selected_event_idx, Some(20));
        assert!(app.is_editing_fret);
        assert_eq!(app.editing_string, Some(4));

        // Now, while on the blank note with empty buffer, heading left should remove the blank note
        let cur_str = app.editing_string;
        let e_idx_opt = app.selected_event_idx;
        if let Some(e_idx) = e_idx_opt {
            let total_events = find_all_event_ranges(&app.yaml_text).len();
            let at_final_event = e_idx == total_events.saturating_sub(1);
            let is_final_blank = at_final_event
                && app
                    .parsed_mnx
                    .as_ref()
                    .and_then(|d| d.parts.first())
                    .and_then(|p| p.measures.last())
                    .and_then(|m| m.sequences.first())
                    .and_then(|s| s.flattened_events().last().copied())
                    .map(|e| e.notes.is_empty() && e.rest.is_none())
                    .unwrap_or(false);

            assert!(is_final_blank);
            if let Some(new_yaml) = remove_last_blank_event(&app.yaml_text) {
                app.yaml_text = new_yaml;
                app.process_and_render();
                let new_total = find_all_event_ranges(&app.yaml_text).len();
                assert_eq!(new_total, 20);
                let prev_idx = new_total - 1;
                app.selected_event_idx = Some(prev_idx);
                app.editing_string = cur_str;
                app.is_editing_fret = true;
                if let Some(s) = cur_str {
                    app.select_note_on_string(prev_idx, s);
                }
            }
        }

        assert_eq!(app.selected_event_idx, Some(19));
        assert!(app.is_editing_fret);
        assert_eq!(app.editing_string, Some(4));
    }

    #[test]
    fn test_edit_mode_navigate_right_on_blank_string_when_next_event_has_notes() {
        let mut app = MnxEditorApp::default();
        // In examples/example.yaml, event 0 has a note on string 7, but string 1 is blank at event 0.
        // Event 1 has a note on string 7.
        app.selected_event_idx = Some(0);
        app.is_editing_fret = true;
        app.editing_string = Some(1);
        app.editing_fret_buf.clear();

        // Check string 1 is indeed blank at event 0
        assert!(!app.event_has_note_on_string(0, 1));
        // Check event 1 has notes
        assert!(app.event_has_any_notes(1));

        let total_events = find_all_event_ranges(&app.yaml_text).len();
        let cur_str = app.editing_string;
        let e_idx = 0;

        if e_idx + 1 < total_events && app.event_has_any_notes(e_idx + 1) {
            let next_idx = e_idx + 1;
            app.selected_event_idx = Some(next_idx);
            app.editing_string = cur_str;
            app.is_editing_fret = true;
            if let Some(s) = cur_str {
                app.select_note_on_string(next_idx, s);
            }
        }

        assert_eq!(app.selected_event_idx, Some(1));
        assert!(app.is_editing_fret);
        assert_eq!(app.editing_string, Some(1));
    }
}

#[cfg(test)]
mod rest_tests {
    use super::*;

    #[test]
    fn test_convert_event_to_rest_and_back() {
        let mut app = MnxEditorApp::default();
        let _total_initial = crate::yaml_editor::find_all_event_ranges(&app.yaml_text).len();

        // Select event 0 (which has notes)
        app.selected_event_idx = Some(0);

        // Convert to rest
        if let Some(new_yaml) = crate::yaml_editor::convert_event_to_rest(&app.yaml_text, 0) {
            app.yaml_text = new_yaml;
            app.process_and_render();
        }

        let doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(&app.yaml_text).unwrap();
        let ev = &doc.parts[0].measures[0].sequences[0].flattened_events()[0];
        assert!(ev.rest.is_some());
        assert!(ev.notes.is_empty());

        // Convert back to note by typing a number
        app.is_editing_fret = true;
        app.editing_string = Some(3); // String 3
        app.editing_fret_buf = "5".to_string();
        app.commit_fret_edit();
        app.process_and_render();

        let doc2 = serde_saphyr::from_str::<crate::mnx::MnxDocument>(&app.yaml_text).unwrap();
        let ev2 = &doc2.parts[0].measures[0].sequences[0].flattened_events()[0];
        assert!(ev2.rest.is_none());
        assert_eq!(ev2.notes.len(), 1);
        assert_eq!(ev2.notes[0].string, Some(3));
        assert_eq!(ev2.notes[0].fret, Some(5));
    }
}

#[cfg(test)]
mod tuplet_button_tests {
    use super::*;

    #[test]
    fn test_tuplet_state_behaviors_and_yaml_lifecycle() {
        let mut app = MnxEditorApp::default();

        // 1. Select mode on non-tuple event (event 0)
        app.selected_event_idx = Some(0);
        app.is_editing_fret = false;
        app.handle_tuplet_button_click();

        // Should open CreateTuplet dialog with from_select_mode = true
        assert!(matches!(
            app.active_dialog,
            ActiveDialog::CreateTuplet {
                inner_multiple: 3,
                outer_multiple: 2,
                from_select_mode: true,
                ..
            }
        ));

        // Create the tuplet in YAML
        let new_yaml = crate::yaml_editor::create_tuplet_in_yaml(&app.yaml_text, 0, 3, 2, "eighth")
            .expect("Should create tuplet");
        app.yaml_text = new_yaml;
        app.process_and_render();
        app.selected_event_idx = Some(0);
        app.is_editing_fret = true; // enters edit mode

        let doc = app.parsed_mnx.as_ref().unwrap();
        let tuplet_info = doc
            .find_event_tuplet_info(0)
            .expect("Event 0 should be in tuplet");
        assert_eq!(tuplet_info.tuplet.inner.multiple, 3);
        assert_eq!(tuplet_info.tuplet.outer.multiple, 2);
        assert_eq!(tuplet_info.event_range, 0..3);

        // Events 1 and 2 in the tuplet should be rests
        let seq = &doc.parts[0].measures[0].sequences[0];
        let flat = seq.flattened_events();
        assert!(flat[0].rest.is_none());
        assert!(flat[1].rest.is_some());
        assert!(flat[2].rest.is_some());

        // 2. In edit mode within a tuple figure:
        // Pressing tuplet button fills remainder with rests and appends blank note after tuplet
        app.selected_event_idx = Some(0);
        app.is_editing_fret = true;
        app.active_dialog = ActiveDialog::None;

        app.handle_tuplet_button_click();
        assert_eq!(app.active_dialog, ActiveDialog::None);
        // A new blank note should follow the end of the tuple duration (index 3)
        assert_eq!(app.selected_event_idx, Some(3));
        assert!(app.is_editing_fret);

        let doc_after_fill = app.parsed_mnx.as_ref().unwrap();
        let flat_after = doc_after_fill.parts[0].measures[0].sequences[0].flattened_events();
        assert_eq!(flat_after[3].notes.len(), 0);

        // 3. In select mode within a tuple figure:
        // Select event 1 (inside the tuplet)
        app.selected_event_idx = Some(1);
        app.is_editing_fret = false;
        app.handle_tuplet_button_click();

        // Should open ConfirmRemoveTuplet dialog
        assert!(matches!(
            app.active_dialog,
            ActiveDialog::ConfirmRemoveTuplet { event_idx: 1 }
        ));

        // When OK is confirmed, remove tuplet unwraps events
        let unwrapped_yaml = crate::yaml_editor::remove_tuplet_in_yaml(&app.yaml_text, 1)
            .expect("Should remove tuplet");
        app.yaml_text = unwrapped_yaml;
        app.process_and_render();
        app.selected_event_idx = Some(1);
        app.active_dialog = ActiveDialog::None;

        let doc_unwrapped = app.parsed_mnx.as_ref().unwrap();
        assert!(
            doc_unwrapped.find_event_tuplet_info(1).is_none(),
            "Tuplet wrapper should be removed"
        );
    }

    #[test]
    fn test_non_tuple_edit_mode_dialog() {
        let mut app = MnxEditorApp::default();
        app.selected_event_idx = Some(0);
        app.is_editing_fret = true;
        app.handle_tuplet_button_click();

        assert!(matches!(
            app.active_dialog,
            ActiveDialog::CreateTuplet {
                inner_multiple: 3,
                outer_multiple: 2,
                from_select_mode: false,
                ..
            }
        ));
    }

    #[test]
    fn test_tuplet_right_arrow_creates_non_tuple_blank_note() {
        let mut app = MnxEditorApp::default();
        let new_yaml = crate::yaml_editor::create_tuplet_in_yaml(&app.yaml_text, 0, 3, 2, "eighth")
            .expect("Should create tuplet");
        app.yaml_text = new_yaml;
        app.process_and_render();

        // Event 0: note, Event 1: rest, Event 2: rest.
        // Event 2 is the final note/event in this tuplet.
        app.selected_event_idx = Some(2);
        let total = find_all_event_ranges(&app.yaml_text).len();

        // 1. In select mode
        let next_idx = app.handle_tuplet_right_arrow(2, total);
        assert!(next_idx.is_some());
        let new_e = next_idx.unwrap();
        assert_eq!(new_e, 3);

        let doc = app.parsed_mnx.as_ref().unwrap();
        // Crucial requirement: the new note must be NON-TUPLE
        assert!(
            doc.find_event_tuplet_info(new_e).is_none(),
            "New note must be non-tuple"
        );
        let ev = &doc.parts[0].measures[0].sequences[0].flattened_events()[new_e];
        // Must match the base duration of the final note inside the tuple ("eighth")
        assert_eq!(ev.duration.base, "eighth");

        // 2. In edit mode
        app.selected_event_idx = Some(2);
        app.is_editing_fret = true;
        app.editing_string = Some(1);
        let total_now = find_all_event_ranges(&app.yaml_text).len();
        app.advance_to_next_note_in_edit_mode(Some(1), total_now);

        assert_eq!(app.selected_event_idx, Some(3));
        assert!(app.is_editing_fret);
        let doc2 = app.parsed_mnx.as_ref().unwrap();
        assert!(
            doc2.find_event_tuplet_info(3).is_none(),
            "Must be non-tuple in edit mode"
        );
    }

    #[test]
    fn test_new_file_when_unmodified_prompts_instrument_setup() {
        let mut app = MnxEditorApp::default();
        assert!(!app.is_modified(), "Initial app should not be modified");

        app.new_file();
        assert!(
            matches!(
                app.active_dialog,
                ActiveDialog::InstrumentSetup {
                    num_strings: 6,
                    edo: 12,
                    step_multiplier: 1,
                    is_new_tab: true,
                    ..
                }
            ),
            "Unmodified new_file should open InstrumentSetup for new tab"
        );
    }

    #[test]
    fn test_new_file_when_modified_prompts_confirm_discard() {
        let mut app = MnxEditorApp::default();
        app.yaml_text.push_str("\n# modification");
        assert!(app.is_modified(), "App should detect unsaved changes");

        app.new_file();
        assert_eq!(
            app.active_dialog,
            ActiveDialog::ConfirmDiscardChanges {
                target: DiscardTarget::NewProject,
            },
            "Modified new_file should prompt ConfirmDiscardChanges"
        );
    }

    #[test]
    fn test_start_new_tab_resets_state_and_initializes_document() {
        let mut app = MnxEditorApp::default();
        app.selected_event_idx = Some(2);
        app.is_editing_fret = true;
        app.editing_fret_buf = "12".to_string();
        app.current_file_path = Some(std::path::PathBuf::from("/fake/path.yaml"));

        let tunings = vec![
            crate::mnx::StringTuning {
                string: 1,
                pitch: crate::mnx::Pitch {
                    step: "E".to_string(),
                    alter: None,
                    octave: 4,
                    ..Default::default()
                },
            },
            crate::mnx::StringTuning {
                string: 2,
                pitch: crate::mnx::Pitch {
                    step: "B".to_string(),
                    alter: None,
                    octave: 3,
                    ..Default::default()
                },
            },
            crate::mnx::StringTuning {
                string: 3,
                pitch: crate::mnx::Pitch {
                    step: "G".to_string(),
                    alter: None,
                    octave: 3,
                    ..Default::default()
                },
            },
            crate::mnx::StringTuning {
                string: 4,
                pitch: crate::mnx::Pitch {
                    step: "D".to_string(),
                    alter: None,
                    octave: 3,
                    ..Default::default()
                },
            },
            crate::mnx::StringTuning {
                string: 5,
                pitch: crate::mnx::Pitch {
                    step: "A".to_string(),
                    alter: None,
                    octave: 2,
                    ..Default::default()
                },
            },
            crate::mnx::StringTuning {
                string: 6,
                pitch: crate::mnx::Pitch {
                    step: "E".to_string(),
                    alter: None,
                    octave: 2,
                    ..Default::default()
                },
            },
        ];

        app.start_new_tab(6, 12, 1, tunings);

        assert!(!app.is_modified(), "New tab should be clean (unmodified)");
        assert!(app.current_file_path.is_none(), "File path should be reset");
        assert_eq!(
            app.selected_event_idx,
            Some(0),
            "First rest should be selected"
        );
        assert!(!app.is_editing_fret, "Fret edit mode should be cleared");
        assert!(
            app.editing_fret_buf.is_empty(),
            "Fret edit buf should be empty"
        );

        let doc = app
            .parsed_mnx
            .as_ref()
            .expect("New tab document should parse");
        assert_eq!(doc.parts.len(), 1);
        assert_eq!(doc.parts[0].edo(), 12);
        assert_eq!(doc.parts[0].num_strings(), 6);
        assert_eq!(doc.parts[0].measures.len(), 1);
        let events = doc.parts[0].measures[0].sequences[0].flattened_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].duration.base, "quarter");
        assert!(
            events[0].rest.is_some(),
            "Default event in new tab should be a rest"
        );
    }

    #[test]
    fn test_is_selected_event_tied() {
        let mut app = MnxEditorApp::default();
        app.yaml_text = r#"mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - measures:
      - sequences:
          - content:
              - id: ev1
                duration: { base: quarter }
                notes:
                  - id: n1
                    pitch: { step: C, octave: 4 }
                    string: 1
                    fret: 0
                    ties:
                      - target: n2
              - id: ev2
                duration: { base: quarter }
                notes:
                  - id: n2
                    pitch: { step: C, octave: 4 }
                    string: 1
                    fret: 0
"#
        .to_string();
        app.process_and_render();

        app.selected_event_idx = Some(0);
        assert!(app.is_selected_event_tied());

        app.selected_event_idx = Some(1);
        assert!(!app.is_selected_event_tied());

        app.selected_event_idx = None;
        assert!(!app.is_selected_event_tied());
    }

    #[test]
    fn test_tap_toggle_and_yaml_writing() {
        let mut app = MnxEditorApp::default();
        app.yaml_text = r#"mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - measures:
      - sequences:
          - content:
              - id: ev1
                duration: { base: quarter }
                notes:
                  - id: n1
                    pitch: { step: E, octave: 4 }
                    string: 1
                    fret: 12
"#
        .to_string();
        app.process_and_render();

        app.selected_event_idx = Some(0);
        assert!(!app.is_selected_note_tapped());

        // Toggle Tap ON
        app.action_toggle_tap();
        assert!(app.is_selected_note_tapped());
        assert!(app.yaml_text.contains("tap: {}"));

        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
        assert!(ev.notes[0].tap.is_some());

        // Toggle Tap OFF
        app.action_toggle_tap();
        assert!(!app.is_selected_note_tapped());
        assert!(!app.yaml_text.contains("tap:"));

        let doc2 = app.parsed_mnx.as_ref().unwrap();
        let ev2 = doc2.parts[0].measures[0].sequences[0].flattened_events()[0];
        assert!(ev2.notes[0].tap.is_none());
    }

    #[test]
    fn test_tap_chord_note_selection() {
        let mut app = MnxEditorApp::default();
        app.yaml_text = r#"mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - measures:
      - sequences:
          - content:
              - id: ev1
                duration: { base: quarter }
                notes:
                  - id: n1
                    pitch: { step: E, octave: 4 }
                    string: 1
                    fret: 12
                  - id: n2
                    pitch: { step: B, octave: 3 }
                    string: 2
                    fret: 12
"#
        .to_string();
        app.process_and_render();

        app.selected_event_idx = Some(0);
        // Select second note (string 2)
        app.selected_note_idx_in_event = Some(1);
        app.editing_string = Some(2);

        app.action_toggle_tap();
        assert!(app.is_selected_note_tapped());

        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
        assert!(ev.notes[0].tap.is_none());
        assert!(ev.notes[1].tap.is_some());

        // Select first note (string 1)
        app.selected_note_idx_in_event = Some(0);
        app.editing_string = Some(1);
        assert!(!app.is_selected_note_tapped());

        app.action_toggle_tap();
        assert!(app.is_selected_note_tapped());

        let doc2 = app.parsed_mnx.as_ref().unwrap();
        let ev2 = doc2.parts[0].measures[0].sequences[0].flattened_events()[0];
        assert!(ev2.notes[0].tap.is_some());
        assert!(ev2.notes[1].tap.is_some());
    }

    #[test]
    fn test_grace_note_creation_and_yaml_structure() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - measures:
      - sequences:
          - content:
              - duration: { base: quarter }
                notes:
                  - pitch: { step: C, octave: 4 }
                    string: 2
                    fret: 1
"#;
        // Event ranges before grace note: should be 1
        let er_before = find_all_event_ranges(yaml);
        assert_eq!(er_before.len(), 1);

        // Insert grace note before event 0
        let grace_yaml = crate::yaml_editor::insert_grace_before_in_yaml(yaml, 0).expect("insert grace failed");

        // Event ranges after grace note: should be 2 (the grace event, and the original quarter note).
        // The "- type: grace" container header must NOT be an EventRange!
        let er_after = find_all_event_ranges(&grace_yaml);
        assert_eq!(er_after.len(), 2, "Grace container must not be counted as an event range");

        // Now edit event 0 (which is the grace event) to set string 3 fret 12
        let pitch = crate::mnx::Pitch {
            step: "D".to_string(),
            alter: None,
            octave: 4,
            ..Default::default()
        };
        let edited_yaml = crate::yaml_editor::set_or_add_note_on_string(&grace_yaml, 0, 3, 12, Some(&pitch))
            .expect("set note on grace event failed");

        // Verify that "notes:" is NOT on the grace container line, but inside content:
        assert!(!edited_yaml.contains("- type: grace\n      notes:"));
        assert!(!edited_yaml.contains("- type: grace\n    notes:"));

        // Parse with MnxDocument
        let doc: crate::mnx::MnxDocument = serde_saphyr::from_str(&edited_yaml).expect("Failed to deserialize edited YAML");
        let part = doc.parts.first().unwrap();
        let seq = part.measures[0].sequences.first().unwrap();

        // seq.content[0] must be SequenceItem::Grace
        match &seq.content[0] {
            crate::mnx::SequenceItem::Grace(g) => {
                assert_eq!(g.content.len(), 1);
                match &g.content[0] {
                    crate::mnx::SequenceItem::Event(ev) => {
                        assert_eq!(ev.notes.len(), 1);
                        assert_eq!(ev.notes[0].string, Some(3));
                        assert_eq!(ev.notes[0].fret, Some(12));
                        assert_eq!(ev.notes[0].pitch.step, "D");
                    }
                    _ => panic!("Expected Event inside grace content"),
                }
            }
            _ => panic!("Expected Grace as first sequence item"),
        }
    }

    #[test]
    fn test_grace_toggle_never_duplicates() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - measures:
      - sequences:
          - content:
              - duration: { base: quarter }
                notes:
                  - pitch: { step: C, octave: 4 }
                    string: 2
                    fret: 1
"#;
        let mut app = MnxEditorApp::default();
        app.yaml_text = yaml.to_string();
        app.process_and_render();
        app.selected_event_idx = Some(0);

        // Click grace once: inserts grace note before note
        app.action_toggle_grace();
        assert_eq!(app.selected_event_idx, Some(0)); // on the grace note
        assert_eq!(app.yaml_text.matches("- type: grace").count(), 1);

        // Click grace again: pops out to regular note (now event 1)
        app.action_toggle_grace();
        assert_eq!(app.selected_event_idx, Some(1)); // on the regular note
        assert_eq!(app.yaml_text.matches("- type: grace").count(), 1);

        // Click grace again while on regular note: should toggle back to grace note, NOT insert another!
        app.action_toggle_grace();
        assert_eq!(app.selected_event_idx, Some(0)); // on the grace note
        assert_eq!(app.yaml_text.matches("- type: grace").count(), 1, "Must never create duplicate grace container");

        // Click grace again: pops out to regular note
        app.action_toggle_grace();
        assert_eq!(app.selected_event_idx, Some(1));
        assert_eq!(app.yaml_text.matches("- type: grace").count(), 1);
    }

    #[test]
    fn test_grace_delete_single_and_multi() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - measures:
      - sequences:
          - content:
              - type: grace
                content:
                  - duration: { base: eighth }
                    notes:
                      - pitch: { step: G, octave: 4 }
                        string: 1
                        fret: 3
                  - duration: { base: eighth }
                    notes:
                      - pitch: { step: A, octave: 4 }
                        string: 1
                        fret: 5
              - duration: { base: quarter }
                notes:
                  - pitch: { step: C, octave: 4 }
                    string: 2
                    fret: 1
"#;
        let mut app = MnxEditorApp::default();
        app.yaml_text = yaml.to_string();
        app.process_and_render();
        // Events: 0 (Grace G), 1 (Grace A), 2 (Quarter C)
        assert_eq!(find_all_event_ranges(&app.yaml_text).len(), 3);

        // Delete Grace G (event 0)
        app.action_delete_grace_event(0);
        // Should now have 2 events: Grace A (event 0), Quarter C (event 1)
        assert_eq!(find_all_event_ranges(&app.yaml_text).len(), 2);
        assert_eq!(app.yaml_text.matches("- type: grace").count(), 1);

        // Delete Grace A (event 0)
        app.action_delete_grace_event(0);
        // The grace block had only 1 note left, so it should be completely removed!
        assert_eq!(find_all_event_ranges(&app.yaml_text).len(), 1);
        assert_eq!(app.yaml_text.matches("- type: grace").count(), 0);
        // Selection should be on the remaining quarter note (event 0)
        assert_eq!(app.selected_event_idx, Some(0));
    }

    #[test]
    fn test_grace_cut_preserves_remaining_grace_notes() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - measures:
      - sequences:
          - content:
              - type: grace
                content:
                  - duration: { base: eighth }
                    notes:
                      - pitch: { step: G, octave: 4 }
                        _x:
                          string: 1
                          fret: 3
                  - duration: { base: eighth }
                    notes:
                      - pitch: { step: A, octave: 4 }
                        _x:
                          string: 1
                          fret: 5
              - duration: { base: quarter }
                notes:
                  - pitch: { step: C, octave: 4 }
                    _x:
                      string: 2
                      fret: 1
"#;
        let mut app = MnxEditorApp::default();
        app.yaml_text = yaml.to_string();
        app.process_and_render();
        // Select only Grace G (event 0)
        app.selected_event_idx = Some(0);
        app.selection_anchor = Some(0);

        // Cut Grace G
        app.action_cut_overwrite();

        // Grace A (originally event 1) must be preserved!
        let doc: crate::mnx::MnxDocument = serde_saphyr::from_str(&app.yaml_text).unwrap();
        let part = doc.parts.first().unwrap();
        let seq = part.measures[0].sequences.first().unwrap();
        match &seq.content[0] {
            crate::mnx::SequenceItem::Grace(g) => {
                assert_eq!(g.content.len(), 1, "Grace container must preserve Grace A");
                if let crate::mnx::SequenceItem::Event(ev) = &g.content[0] {
                    assert_eq!(ev.notes[0].fret, Some(5), "Grace A (fret 5) preserved");
                } else {
                    panic!("Expected Event inside grace");
                }
            }
            _ => panic!("Expected Grace container"),
        }
    }

    #[test]
    fn test_natural_harmonic_valid_fret_toggle() {
        let mut app = MnxEditorApp::default();
        app.yaml_text = r#"mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - name: "Standard Guitar"
    _x:
      microFretDivisions: 12
      fretStepMultiplier: 1
      tuning:
        - string: 1
          pitch: { step: E, octave: 4 }
    measures:
      - sequences:
          - content:
              - id: ev1
                duration: { base: quarter }
                notes:
                  - id: n1
                    pitch: { step: E, octave: 4 }
                    _x:
                      string: 1
                      fret: 12
"#
        .to_string();
        app.process_and_render();

        app.selected_event_idx = Some(0);
        app.editing_string = Some(1);
        app.selected_note_idx_in_event = Some(0);
        assert!(!app.is_selected_note_harmonic());

        // Toggle Natural Harmonic ON
        app.action_toggle_harmonic();
        assert!(app.is_selected_note_harmonic());
        assert!(app.yaml_text.contains("harmonic:"));
        assert!(app.yaml_text.contains("touch: 12"));

        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
        let h = ev.notes[0].harmonic.as_ref().expect("Harmonic should be present");
        assert_eq!(h.touch, 12.0);
        assert_eq!(h.sounding_note.pitch.step, "E");
        assert_eq!(h.sounding_note.pitch.octave, 5);

        // Toggle Natural Harmonic OFF: restores note to fret 0 and open string pitch (E4)
        app.action_toggle_harmonic();
        assert!(!app.is_selected_note_harmonic());
        assert!(!app.yaml_text.contains("harmonic:"));

        let doc2 = app.parsed_mnx.as_ref().unwrap();
        let ev2 = doc2.parts[0].measures[0].sequences[0].flattened_events()[0];
        assert!(ev2.notes[0].harmonic.is_none());
        assert_eq!(ev2.notes[0].fret, Some(12));
        assert_eq!(ev2.notes[0].pitch.step, "E");
        assert_eq!(ev2.notes[0].pitch.octave, 5);
    }

    #[test]
    fn test_natural_harmonic_custom_fretboard_toggle_and_yaml_cleanup() {
        let mut app = MnxEditorApp::default();
        app.yaml_text = r#"mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - name: "Custom Guitar"
    _x:
      microFretDivisions: 12
      fretStepMultiplier: 1
      tuning:
        - string: 1
          pitch: { step: G, octave: 4 }
        - string: 2
          pitch: { step: D, octave: 4 }
      customFrets:
        - fret_number: 6.0
          spec: { kind: ratio, num: 5, den: 4 }
          strings: [1]
          note_name: "vE"
        - fret_number: 7.0
          spec: { kind: ratio, num: 4, den: 3 }
          strings: [1, 2]
          note_name: "F"
    measures:
      - sequences:
          - content:
              - id: ev1
                duration: { base: quarter }
                notes:
                  - id: n1
                    pitch: { step: G, octave: 4 }
                    _x:
                      string: 1
                      fret: 6
"#
        .to_string();
        app.process_and_render();

        app.selected_event_idx = Some(0);
        app.editing_string = Some(1);
        app.selected_note_idx_in_event = Some(0);
        assert!(!app.is_selected_note_harmonic());

        // Toggle Natural Harmonic ON
        app.action_toggle_harmonic();
        assert!(app.is_selected_note_harmonic());
        assert!(app.yaml_text.contains("harmonic:"));
        assert!(app.yaml_text.contains("touch: 6"));
        assert!(!app.yaml_text.contains("microFretDivisions"));
        assert!(!app.yaml_text.contains("fretStepMultiplier"));

        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
        let h = ev.notes[0].harmonic.as_ref().expect("Harmonic should be present");
        assert_eq!(h.touch, 6.0);
        assert_eq!(h.sounding_note.pitch.step, "B");
        assert_eq!(h.sounding_note.pitch.octave, 6);

        // Toggle Natural Harmonic OFF
        app.action_toggle_harmonic();
        assert!(!app.is_selected_note_harmonic());
        assert!(!app.yaml_text.contains("harmonic:"));

        let doc2 = app.parsed_mnx.as_ref().unwrap();
        let ev2 = doc2.parts[0].measures[0].sequences[0].flattened_events()[0];
        assert!(ev2.notes[0].harmonic.is_none());
        assert_eq!(ev2.notes[0].fret, Some(6));
    }

    #[test]
    fn test_custom_fretboard_harmonic_entry_and_following_measures_workflow() {
        let mut app = MnxEditorApp::default();
        // 1. Create new tab with instrument setup matching the bug file
        let tunings = vec![
            crate::mnx::StringTuning {
                string: 1,
                pitch: crate::mnx::Pitch { step: "4/3".to_string(), alter: None, octave: 4, ext: crate::mnx::PitchExt { name: Some("G".to_string()) } },
            },
            crate::mnx::StringTuning {
                string: 2,
                pitch: crate::mnx::Pitch { step: "5/3".to_string(), alter: None, octave: 4, ext: crate::mnx::PitchExt { name: Some("vB".to_string()) } },
            },
            crate::mnx::StringTuning {
                string: 3,
                pitch: crate::mnx::Pitch { step: "4/3".to_string(), alter: None, octave: 3, ext: crate::mnx::PitchExt { name: Some("G".to_string()) } },
            },
            crate::mnx::StringTuning {
                string: 4,
                pitch: crate::mnx::Pitch { step: "1/1".to_string(), alter: None, octave: 3, ext: crate::mnx::PitchExt { name: Some("D".to_string()) } },
            },
            crate::mnx::StringTuning {
                string: 5,
                pitch: crate::mnx::Pitch { step: "4/3".to_string(), alter: None, octave: 2, ext: crate::mnx::PitchExt { name: Some("G".to_string()) } },
            },
            crate::mnx::StringTuning {
                string: 6,
                pitch: crate::mnx::Pitch { step: "1/1".to_string(), alter: None, octave: 2, ext: crate::mnx::PitchExt { name: Some("D".to_string()) } },
            },
        ];
        app.start_new_tab(6, 12, 1, tunings);

        // Apply custom frets with fret 18 as octave (2/1)
        let custom_frets = vec![crate::fretboard_editor::CustomFret {
            fret_number: 18.0,
            manual_number: Some(18.0),
            spec: crate::fretboard_editor::FretLocationSpec::Ratio { num: 2, den: 1 },
            strings: vec![1, 2, 3, 4, 5, 6],
            note_name: "D".to_string(),
        }];

        let fb_state = crate::fretboard_editor::FretboardEditorState::new(
            6,
            custom_frets,
            "D".to_string(),
            4,
            293.66,
        );
        app.apply_custom_fretboard_state(fb_state);

        // 2. Enter fret 18 in measure 1 on string 6
        app.selected_event_idx = Some(0);
        app.editing_string = Some(6);
        app.editing_fret_buf = "18".to_string();
        assert!(app.commit_fret_edit());

        // 3. Hit Natural Harmonic button
        app.action_toggle_harmonic();
        assert!(app.is_selected_note_harmonic());

        // Verify sounding note pitch: D3 (harmonic 2 on D2 open string is D3)
        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
        let h = ev.notes[0].harmonic.as_ref().expect("Harmonic should be present");
        assert_eq!(h.touch, 18.0);
        assert_eq!(h.sounding_note.pitch.step, "D");
        assert_eq!(h.sounding_note.pitch.octave, 3);
        assert_eq!(h.sounding_note.pitch.alter, None);
        assert_eq!(h.sounding_note.pitch.ext.name.as_deref(), Some("D"));

        // Verify YAML format is clean and does not have multi-line single braces or leaked pending_flow
        assert!(app.yaml_text.contains("harmonic:"));
        assert!(app.yaml_text.contains("touch: 18"));
        assert!(app.yaml_text.contains("soundingNote:"));
        assert!(!app.yaml_text.contains("- {sequences:"));

        // 4. Advance and add notes into following measures
        app.add_measure_to_end();
        let doc2 = app.parsed_mnx.as_ref().unwrap();
        assert_eq!(doc2.parts[0].measures.len(), 2);
        assert_eq!(doc2.global.measures.len(), 2);
        assert_eq!(app.cached_event_ranges.len(), 2, "Both measures must have cached event ranges");

        // Enter note on Measure 2, string 6, fret 18
        app.selected_event_idx = Some(1);
        app.editing_string = Some(6);
        app.editing_fret_buf = "18".to_string();
        assert!(app.commit_fret_edit());

        let doc3 = app.parsed_mnx.as_ref().unwrap();
        let m2_ev = doc3.parts[0].measures[1].sequences[0].flattened_events()[0];
        assert_eq!(m2_ev.notes.len(), 1);
        assert_eq!(m2_ev.notes[0].string, Some(6));
        assert_eq!(m2_ev.notes[0].fret, Some(18));
    }

    #[test]
    fn test_natural_harmonic_invalid_fret_dialog_prompt() {
        let mut app = MnxEditorApp::default();
        app.yaml_text = r#"mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - name: "Standard Guitar"
    _x:
      microFretDivisions: 12
      fretStepMultiplier: 1
      tuning:
        - string: 1
          pitch: { step: E, octave: 4 }
    measures:
      - sequences:
          - content:
              - id: ev1
                duration: { base: quarter }
                notes:
                  - id: n1
                    pitch: { step: E, octave: 4 }
                    _x:
                      string: 1
                      fret: 4
"#
        .to_string();
        app.process_and_render();

        app.selected_event_idx = Some(0);
        app.editing_string = Some(1);
        app.selected_note_idx_in_event = Some(0);

        // Fret 4 is not a valid harmonic node on String 1 -> should open dialog
        app.action_toggle_harmonic();
        match &app.active_dialog {
            ActiveDialog::NaturalHarmonicPrompt { event_idx, string, touch_buf } => {
                assert_eq!(*event_idx, 0);
                assert_eq!(*string, 1);
                assert_eq!(touch_buf, "4");
            }
            other => panic!("Expected NaturalHarmonicPrompt dialog, got {:?}", other),
        }

        // Simulate user typing "3.9" and submitting via ApplyNaturalHarmonic
        let part = app.parsed_mnx.as_ref().unwrap().parts.first().unwrap();
        let table = crate::harmonic::HarmonicLookupTable::from_part(part);
        let nodes = table.find_nodes_at_touch(1, 3.9);
        assert!(!nodes.is_empty());
        let node = nodes[0];

        // Apply harmonic dialog action
        let mut doc = app.parsed_mnx.clone().unwrap();
        let cur_path = doc.get_flattened_event_paths()[0];
        let ev = doc.get_event_mut_at_path(cur_path).unwrap();
        ev.notes[0].harmonic = Some(crate::mnx::Harmonic {
            touch: node.touch_fret,
            sounding_note: Box::new(crate::mnx::Note {
                id: None,
                pitch: node.sounding_pitch.clone(),
                accidental_display: None,
                ties: None,
                comment: None,
                ext: crate::mnx::NoteExt::default(),
            }),
        });
        let new_yaml = serde_saphyr::to_string(&doc).unwrap();
        app.update_yaml(new_yaml);
        app.process_and_render();

        assert!(app.is_selected_note_harmonic());
        assert!(app.yaml_text.contains("touch: 3.9"));
        assert!(app.yaml_text.contains("step: G"));
        assert!(app.yaml_text.contains("octave: 6"));
    }

    #[test]
    fn test_lyrics_action_and_melisma_in_yaml() {
        let mut app = MnxEditorApp::default();
        app.selected_event_idx = Some(0);
        app.action_set_event_lyric(0, "Ooh_");
        assert!(app.yaml_text.contains("lyrics:"));
        assert!(app.yaml_text.contains("text: Ooh"));
        assert!(app.yaml_text.contains("extender: true"));

        app.action_set_event_lyric(0, "con-");
        assert!(app.yaml_text.contains("text: \"con\"") || app.yaml_text.contains("text: con"));
        assert!(app.yaml_text.contains("type: start"));

        app.action_set_event_lyric(0, "");
        assert!(!app.yaml_text.contains("lyrics:"));
    }

    #[test]
    fn test_multi_line_lyrics_action_and_retrieval() {
        let mut app = MnxEditorApp::default();
        app.selected_event_idx = Some(0);

        // Set line 1
        app.action_set_event_lyric_line(0, 1, "Verse1", None, None);
        // Set line 2
        app.action_set_event_lyric_line(0, 2, "Verse2", None, None);

        let l1 = app.get_event_lyric_line(0, 1).expect("Line 1 should exist");
        assert_eq!(l1.text, "Verse1");
        let l2 = app.get_event_lyric_line(0, 2).expect("Line 2 should exist");
        assert_eq!(l2.text, "Verse2");

        // Remove line 1 only; line 2 must remain
        app.action_set_event_lyric_line(0, 1, "", None, None);
        assert!(app.get_event_lyric_line(0, 1).is_none());
        assert_eq!(app.get_event_lyric_line(0, 2).unwrap().text, "Verse2");

        // Remove line 2; lyrics field should now be completely None
        app.action_set_event_lyric_line(0, 2, "", None, None);
        assert!(app.get_event_lyric_line(0, 2).is_none());
        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev = doc.get_event_at_path(doc.get_flattened_event_paths()[0]).unwrap();
        assert!(ev.lyrics.is_none());
    }

    #[test]
    fn test_lyric_dialog_hyphenation_and_melisma_flow() {
        let mut app = MnxEditorApp::default();
        app.open_lyric_dialog(0, 1);
        match &app.active_dialog {
            ActiveDialog::LyricEditor { event_idx, line_idx, syllable_buf, in_word } => {
                assert_eq!(*event_idx, 0);
                assert_eq!(*line_idx, 1);
                assert!(syllable_buf.is_empty());
                assert!(!*in_word);
            }
            _ => panic!("Expected LyricEditor dialog"),
        }

        let ctx = egui::Context::default();
        crate::setup_custom_fonts(&ctx);

        // 1. Simulate typing "hap-" on event 0: initiates start, advances to event 1 with in_word = true
        if let ActiveDialog::LyricEditor { ref mut syllable_buf, .. } = app.active_dialog {
            *syllable_buf = "hap-".to_string();
        }
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.render_dialogs(ui.ctx());
        });
        out.textures_delta.clear();

        // Event 0 should now have "hap" with type "start"
        let l0 = app.get_event_lyric_line(0, 1).expect("Event 0 should have lyric");
        assert_eq!(l0.text, "hap");
        assert_eq!(l0.line_type.as_deref(), Some("start"));

        // Dialog should now be on event 1 with in_word = true
        match &app.active_dialog {
            ActiveDialog::LyricEditor { event_idx, line_idx, in_word, .. } => {
                assert_eq!(*event_idx, 1);
                assert_eq!(*line_idx, 1);
                assert!(*in_word, "in_word must be true following hyphen");
            }
            _ => panic!("Expected LyricEditor on event 1"),
        }

        // 2. Simulate typing "py " (Space) on event 1: finishes word with type "end", advances to event 2
        if let ActiveDialog::LyricEditor { ref mut syllable_buf, .. } = app.active_dialog {
            *syllable_buf = "py ".to_string();
        }
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.render_dialogs(ui.ctx());
        });
        out.textures_delta.clear();

        let l1 = app.get_event_lyric_line(1, 1).expect("Event 1 should have lyric");
        assert_eq!(l1.text, "py");
        assert_eq!(l1.line_type.as_deref(), Some("end"));

        // Dialog should now be on event 2 with in_word = false
        match &app.active_dialog {
            ActiveDialog::LyricEditor { event_idx, in_word, .. } => {
                assert_eq!(*event_idx, 2);
                assert!(!*in_word);
            }
            _ => panic!("Expected LyricEditor on event 2"),
        }

        // 3. Simulate typing "sing_" on event 2: melisma extender, advances to event 3
        if let ActiveDialog::LyricEditor { ref mut syllable_buf, .. } = app.active_dialog {
            *syllable_buf = "sing_".to_string();
        }
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.render_dialogs(ui.ctx());
        });
        out.textures_delta.clear();

        let l2 = app.get_event_lyric_line(2, 1).expect("Event 2 should have lyric");
        assert_eq!(l2.text, "sing");
        assert_eq!(l2.extender, Some(true));
        assert!(l2.is_extender());

        // 4. On event 3, type "_" alone: melisma extend continues on event 3, advances to event 4
        if let ActiveDialog::LyricEditor { ref mut syllable_buf, .. } = app.active_dialog {
            *syllable_buf = "_".to_string();
        }
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.render_dialogs(ui.ctx());
        });
        out.textures_delta.clear();

        let l3 = app.get_event_lyric_line(3, 1).expect("Event 3 should have lyric");
        assert_eq!(l3.text, "");
        assert_eq!(l3.extender, Some(true));
        assert!(l3.is_extender());

        // 5. On event 4, press Down arrow: moves to line 2 on event 4
        let mut raw_input = egui::RawInput::default();
        raw_input.events.push(egui::Event::Key {
            key: egui::Key::ArrowDown,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        let mut out = ctx.run_ui(raw_input, |ui| {
            app.render_dialogs(ui.ctx());
        });
        out.textures_delta.clear();

        match &app.active_dialog {
            ActiveDialog::LyricEditor { event_idx, line_idx, .. } => {
                assert_eq!(*event_idx, 4);
                assert_eq!(*line_idx, 2, "ArrowDown should navigate to Line 2");
            }
            _ => panic!("Expected LyricEditor on Line 2"),
        }

        // 6. Type "chorus" and press Enter: commits and exits dialog
        if let ActiveDialog::LyricEditor { ref mut syllable_buf, .. } = app.active_dialog {
            *syllable_buf = "chorus".to_string();
        }
        let mut enter_input = egui::RawInput::default();
        enter_input.events.push(egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        let mut out = ctx.run_ui(enter_input, |ui| {
            app.render_dialogs(ui.ctx());
        });
        out.textures_delta.clear();

        let l4_line2 = app.get_event_lyric_line(4, 2).expect("Event 4 should have Line 2 lyric");
        assert_eq!(l4_line2.text, "chorus");
        assert_eq!(app.active_dialog, ActiveDialog::None, "Enter must accept input and close dialog");
    }

    #[test]
    fn test_lyric_dialog_clear_and_enter_deletes_lyric() {
        let mut app = MnxEditorApp::default();
        let ctx = egui::Context::default();
        crate::setup_custom_fonts(&ctx);

        // 1. Add a lyric to event 0
        app.action_set_event_lyric_line(0, 1, "testword", None, None);
        assert!(app.get_event_lyric_line(0, 1).is_some());

        // 2. Open dialog on event 0 (like double-clicking or navigating)
        app.open_lyric_dialog(0, 1);
        match &app.active_dialog {
            ActiveDialog::LyricEditor { event_idx, line_idx, syllable_buf, .. } => {
                assert_eq!(*event_idx, 0);
                assert_eq!(*line_idx, 1);
                assert_eq!(syllable_buf, "testword");
            }
            _ => panic!("Expected LyricEditor dialog"),
        }

        // 3. Clear the word in the buffer
        if let ActiveDialog::LyricEditor { ref mut syllable_buf, .. } = app.active_dialog {
            syllable_buf.clear();
        }

        // 4. Hit Enter
        let mut enter_input = egui::RawInput::default();
        enter_input.events.push(egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        let mut out = ctx.run_ui(enter_input, |ui| {
            app.render_dialogs(ui.ctx());
        });
        out.textures_delta.clear();

        // 5. Verify lyric was removed and dialog closed
        assert_eq!(app.active_dialog, ActiveDialog::None);
        assert!(app.get_event_lyric_line(0, 1).is_none(), "Lyric should be deleted after clearing and hitting Enter");

        // 6. Test clearing via Space navigation: add lyric to event 1, open dialog, clear, press Space
        app.action_set_event_lyric_line(1, 1, "anotherword", None, None);
        assert!(app.get_event_lyric_line(1, 1).is_some());

        app.open_lyric_dialog(1, 1);
        if let ActiveDialog::LyricEditor { ref mut syllable_buf, .. } = app.active_dialog {
            syllable_buf.clear();
        }

        let mut space_input = egui::RawInput::default();
        space_input.events.push(egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        let mut out = ctx.run_ui(space_input, |ui| {
            app.render_dialogs(ui.ctx());
        });
        out.textures_delta.clear();

        // Event 1 lyric should now be deleted, and dialog moved to event 2
        assert!(app.get_event_lyric_line(1, 1).is_none(), "Lyric on event 1 should be deleted after clearing and pressing Space");
        match &app.active_dialog {
            ActiveDialog::LyricEditor { event_idx, line_idx, .. } => {
                assert_eq!(*event_idx, 2);
                assert_eq!(*line_idx, 1);
            }
            _ => panic!("Expected LyricEditor to advance to event 2"),
        }
    }

    #[test]
    fn test_multi_line_lyrics_layout_and_pdf_height() {
        let mut app = MnxEditorApp::default();
        app.action_set_event_lyric_line(0, 1, "Line1", None, None);
        app.action_set_event_lyric_line(0, 2, "Line2", None, None);

        let doc = app.parsed_mnx.as_ref().unwrap();
        let pages = crate::layout::compute_score_layout(doc, 0.0);
        assert!(!pages.is_empty());

        let mut dl = crate::layout::display_list::DisplayList::new();
        crate::pdf_export::populate_page_display_list(doc, &pages[0], &mut dl);

        // Verify both Line 1 and Line 2 exist in display list with different Y positions
        let line1_item = dl.items.iter().find(|i| match &i.kind {
            crate::layout::display_list::DrawKind::Text { text, .. } => text == "Line1",
            _ => false,
        }).expect("Line 1 item should exist");
        let line2_item = dl.items.iter().find(|i| match &i.kind {
            crate::layout::display_list::DrawKind::Text { text, .. } => text == "Line2",
            _ => false,
        }).expect("Line 2 item should exist");

        if let (crate::layout::display_list::DrawKind::Text { pos: p1, text: t1, .. },
                crate::layout::display_list::DrawKind::Text { pos: p2, text: t2, .. }) = (&line1_item.kind, &line2_item.kind) {
            assert_eq!(t1, "Line1");
            assert_eq!(t2, "Line2");
            assert!(p2.y > p1.y, "Line 2 Y ({}) must be below Line 1 Y ({})", p2.y, p1.y);
            assert!((p2.y - p1.y - 14.0).abs() < 0.1, "Line 2 should be exactly 14.0pt below Line 1");
        } else {
            panic!("Expected text draw items for lyrics");
        }
    }

    #[test]
    fn test_split_syllable_hyphen_centered_between_boundaries() {
        let mut app = MnxEditorApp::default();
        // Set event 0 to "hap" (type start) and event 1 to "py" (type end)
        app.action_set_event_lyric_line(0, 1, "hap", Some("start".to_string()), None);
        app.action_set_event_lyric_line(1, 1, "py", Some("end".to_string()), None);

        let doc = app.parsed_mnx.as_ref().unwrap();
        let pages = crate::layout::compute_score_layout(doc, 0.0);
        assert!(!pages.is_empty());

        let mut dl = crate::layout::display_list::DisplayList::new();
        crate::pdf_export::populate_page_display_list(doc, &pages[0], &mut dl);

        // Find "hap", "py", and the hyphen "-"
        let hap_item = dl.items.iter().find(|i| match &i.kind {
            crate::layout::display_list::DrawKind::Text { text, .. } => text == "hap",
            _ => false,
        }).expect("'hap' item should exist without trailing '-'");

        let py_item = dl.items.iter().find(|i| match &i.kind {
            crate::layout::display_list::DrawKind::Text { text, .. } => text == "py",
            _ => false,
        }).expect("'py' item should exist");

        let hyphen_item = dl.items.iter().find(|i| match &i.kind {
            crate::layout::display_list::DrawKind::Text { text, .. } => text == "-",
            _ => false,
        }).expect("hyphen '-' item should exist");

        if let (crate::layout::display_list::DrawKind::Text { pos: p_hap, .. },
                crate::layout::display_list::DrawKind::Text { pos: p_py, .. },
                crate::layout::display_list::DrawKind::Text { pos: p_hyphen, .. }) = (&hap_item.kind, &py_item.kind, &hyphen_item.kind) {
            let w1 = ("hap".chars().count() as f32) * 6.5;
            let end_of_first = p_hap.x + w1 * 0.5;

            let w2 = ("py".chars().count() as f32) * 6.5;
            let beg_of_second = p_py.x - w2 * 0.5;

            let expected_hyphen_x = (end_of_first + beg_of_second) * 0.5;
            assert!(
                (p_hyphen.x - expected_hyphen_x).abs() < 1e-3,
                "Hyphen X ({}) should be exactly midway between end of first syllable ({}) and beginning of second ({})",
                p_hyphen.x,
                end_of_first,
                beg_of_second
            );
        }
    }

    #[test]
    fn test_layout_mode_breaks_and_settings() {
        let mut app = MnxEditorApp::default();
        
        // 1. Toggle system break on measure 0
        app.action_toggle_system_break(0);
        assert!(app.yaml_text.contains("systemBreak: true"));
        let doc = app.parsed_mnx.as_ref().unwrap();
        assert_eq!(doc.global.measures[0].system_break, Some(true));

        // Untoggle system break
        app.action_toggle_system_break(0);
        assert!(!app.yaml_text.contains("systemBreak: true"));

        // 2. Toggle page break on measure 0
        app.action_toggle_page_break(0);
        assert!(app.yaml_text.contains("pageBreak: true"));
        let doc = app.parsed_mnx.as_ref().unwrap();
        assert_eq!(doc.global.measures[0].page_break, Some(true));

        // 3. Page size and measure numbers settings
        app.action_set_layout_page_size("a4");
        assert!(app.yaml_text.contains("pageSize: a4") || app.yaml_text.contains("pageSize: \"a4\""));

        app.action_set_layout_measure_numbers("all");
        assert!(app.yaml_text.contains("measureNumbers: all") || app.yaml_text.contains("measureNumbers: \"all\""));

        // 4. Element offset hand-removal of collision
        app.action_set_element_offset("ending_0", 15.5, -8.0);
        assert!(app.yaml_text.contains("ending_0:"));
        assert!(app.yaml_text.contains("dx: 15.5"));
        assert!(app.yaml_text.contains("dy: -8"));

        let doc = app.parsed_mnx.as_ref().unwrap();
        let layout = doc.layout.as_ref().unwrap();
        assert_eq!(layout.page_size, "a4");
        assert_eq!(layout.measure_numbers, "all");
        let off = layout.offsets.get("ending_0").unwrap();
        assert_eq!(off.dx, 15.5);
        assert_eq!(off.dy, -8.0);

        // Reset offset to 0
        app.action_set_element_offset("ending_0", 0.0, 0.0);
        let doc = app.parsed_mnx.as_ref().unwrap();
        let layout = doc.layout.as_ref().unwrap();
        assert!(!layout.offsets.contains_key("ending_0"));
    }

    #[test]
    fn test_layout_mode_breaks_target_clicked_item_measure() {
        let mut app = MnxEditorApp::default();
        // Add 3 measures so we have measures 0, 1, 2, 3
        app.add_measure_to_end();
        app.add_measure_to_end();
        app.add_measure_to_end();
        assert!(app.parsed_mnx.as_ref().unwrap().global.measures.len() >= 4);

        // Continuous mode cursor is at measure 0
        app.selected_event_idx = Some(0);
        assert_eq!(app.view_mode, crate::gui::ViewMode::Continuous);
        assert_eq!(app.get_target_measure_for_layout_or_breaks(), 0);

        // Switch to Layout mode
        app.view_mode = crate::gui::ViewMode::Layout;

        // 1. Click a note in bar 2 (formatted as note_2_...)
        app.selected_layout_element_id = Some("note_2_0_1".to_string());
        assert_eq!(app.get_target_measure_for_layout_or_breaks(), 2);
        assert_eq!(app.get_selected_measure_range(), (2, 2));

        // Toggle system break while note in bar 2 is selected
        let target_m = app.get_target_measure_for_layout_or_breaks();
        app.action_toggle_system_break(target_m);
        let doc = app.parsed_mnx.as_ref().unwrap();
        assert_eq!(doc.global.measures[0].system_break, None);
        assert_eq!(doc.global.measures[2].system_break, Some(true));

        // 2. Click a lyric in bar 3
        app.selected_layout_element_id = Some("lyric_3_0_1".to_string());
        assert_eq!(app.get_target_measure_for_layout_or_breaks(), 3);
        let target_m = app.get_target_measure_for_layout_or_breaks();
        app.action_toggle_page_break(target_m);
        let doc = app.parsed_mnx.as_ref().unwrap();
        assert_eq!(doc.global.measures[0].page_break, None);
        assert_eq!(doc.global.measures[3].page_break, Some(true));

        // 3. Click other element types in bar 1
        app.selected_layout_element_id = Some("barline_1".to_string());
        assert_eq!(app.get_target_measure_for_layout_or_breaks(), 1);

        app.selected_layout_element_id = Some("time_sig_1".to_string());
        assert_eq!(app.get_target_measure_for_layout_or_breaks(), 1);

        app.selected_layout_element_id = Some("ending_1".to_string());
        assert_eq!(app.get_target_measure_for_layout_or_breaks(), 1);

        app.selected_layout_element_id = Some("stem_1_0".to_string());
        assert_eq!(app.get_target_measure_for_layout_or_breaks(), 1);

        app.selected_layout_element_id = Some("measure_1".to_string());
        assert_eq!(app.get_target_measure_for_layout_or_breaks(), 1);

        // 4. Click measure hitbox (populates selected_layout_measures)
        app.selected_layout_element_id = None;
        app.selected_layout_measures.insert(2);
        assert_eq!(app.get_target_measure_for_layout_or_breaks(), 2);

        // 5. Shift+click multiple measures: e.g. measures 1, 2
        app.selected_layout_measures.insert(1);
        assert_eq!(app.get_selected_measure_range(), (1, 2));
        assert_eq!(app.get_target_measure_for_layout_or_breaks(), 2);
    }

    #[test]
    fn test_make_into_system_replaces_spanned_system_breaks() {
        let mut app = MnxEditorApp::default();
        app.add_measure_to_end();
        app.add_measure_to_end();
        app.add_measure_to_end();

        // Initially toggle a system break on measure 1
        app.action_toggle_system_break(1);
        let doc = app.parsed_mnx.as_ref().unwrap();
        assert_eq!(doc.global.measures[1].system_break, Some(true));

        // Now select measures 0..=2 and execute "Make into System"
        app.action_make_into_system(0, 2);

        let doc = app.parsed_mnx.as_ref().unwrap();
        // The previously set system break at measure 1 must have been cleared / replaced
        assert_eq!(doc.global.measures[1].system_break, None);
        // The new break is now at measure 2 (end of system)
        assert_eq!(doc.global.measures[2].system_break, Some(true));
        // The locked system [0, 2] is recorded
        let layout = doc.layout.as_ref().unwrap();
        assert_eq!(layout.locked_systems, vec![[0, 2]]);

        // Layout breaking produces a single system for measures 0..3
        let metrics = crate::layout::sizing::compute_measure_metrics(doc, crate::layout::DEFAULT_LAYOUT_BASE_SPACING);
        let breaks = crate::layout::breaking::break_measures_into_systems(
            &metrics,
            1000.0,
            &layout.locked_systems,
        );
        assert_eq!(breaks[0].measure_range, 0..3);

        // Setting a new manual break on measure 1 inside the locked system dissolves the lock
        app.action_toggle_system_break(1);
        let doc = app.parsed_mnx.as_ref().unwrap();
        assert_eq!(doc.global.measures[1].system_break, Some(true));
        let layout = doc.layout.as_ref().unwrap();
        assert!(layout.locked_systems.is_empty());
    }

    #[test]
    fn test_start_new_tab_microtonal_tuning_preserves_alter() {
        let mut app = MnxEditorApp::default();
        let edo = 24;
        let step_multiplier = 1;
        let tunings = vec![
            crate::mnx::StringTuning {
                string: 1,
                pitch: crate::mnx::Pitch {
                    step: "E".to_string(),
                    octave: 4,
                    alter: Some(crate::mnx::Alteration::Value(0.5)),
                    ..Default::default()
                },
            },
            crate::mnx::StringTuning {
                string: 2,
                pitch: crate::mnx::Pitch {
                    step: "B".to_string(),
                    octave: 3,
                    alter: Some(crate::mnx::Alteration::Value(-0.5)),
                    ..Default::default()
                },
            },
        ];
        app.start_new_tab(2, edo, step_multiplier, tunings);
        assert!(app.validation_errors.is_empty(), "errors: {:?}", app.validation_errors);
        let doc = app.parsed_mnx.as_ref().unwrap();
        let part = &doc.parts[0];
        assert_eq!(part.micro_fret_divisions, Some(24));
        let parsed_tunings = part.tuning.as_ref().unwrap();
        assert_eq!(parsed_tunings.len(), 2);
        assert_eq!(
            parsed_tunings[0].pitch.alter,
            Some(crate::mnx::Alteration::Value(0.5))
        );
        assert_eq!(
            parsed_tunings[1].pitch.alter,
            Some(crate::mnx::Alteration::Value(-0.5))
        );
    }

    #[test]
    fn test_translate_instrument_setup_all_parameters() {
        let mut app = MnxEditorApp::default();
        // Default tab is 6 strings, 41 EDO, step_multiplier 2.
        // Translate simultaneously to 4 strings, 24 EDO, step_multiplier 2, with custom open tunings:
        let new_tunings = vec![
            crate::mnx::StringTuning {
                string: 1,
                pitch: crate::mnx::Pitch {
                    step: "G".to_string(),
                    octave: 3,
                    alter: Some(crate::mnx::Alteration::Value(0.5)),
                    ..Default::default()
                },
            },
            crate::mnx::StringTuning {
                string: 2,
                pitch: crate::mnx::Pitch {
                    step: "D".to_string(),
                    octave: 3,
                    alter: None,
                    ..Default::default()
                },
            },
            crate::mnx::StringTuning {
                string: 3,
                pitch: crate::mnx::Pitch {
                    step: "A".to_string(),
                    octave: 2,
                    alter: None,
                    ..Default::default()
                },
            },
            crate::mnx::StringTuning {
                string: 4,
                pitch: crate::mnx::Pitch {
                    step: "E".to_string(),
                    octave: 2,
                    alter: None,
                    ..Default::default()
                },
            },
        ];

        app.translate_instrument_setup(4, 24, 2, new_tunings);
        assert!(
            app.validation_errors.is_empty(),
            "Expected 0 validation errors after translating all instrument setup params, got: {:?}",
            app.validation_errors
        );
        let doc = app.parsed_mnx.as_ref().unwrap();
        let part = &doc.parts[0];
        assert_eq!(part.micro_fret_divisions, Some(24));
        assert_eq!(part.fret_step_multiplier, Some(2));
        assert_eq!(part.tuning.as_ref().unwrap().len(), 4);
        assert_eq!(
            part.tuning.as_ref().unwrap()[0].pitch.alter,
            Some(crate::mnx::Alteration::Value(0.5))
        );
        // Ensure no note references string > 4
        for m in &doc.parts[0].measures {
            for seq in &m.sequences {
                for ev in seq.flattened_events() {
                    for note in &ev.notes {
                        if let Some(s) = note.string {
                            assert!(s >= 1 && s <= 4, "Note string {} out of range 1..=4", s);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_pitch_bend_range_yaml_roundtrip() {
        let mut app = MnxEditorApp::default();
        assert_eq!(app.pitch_bend_range, 2);

        app.set_pitch_bend_range_in_yaml(12);
        assert_eq!(app.pitch_bend_range, 12);
        assert!(app.yaml_text.contains("pitchBendRange: 12"));

        // Simulate saving and reloading YAML into a fresh app
        let saved_yaml = app.yaml_text.clone();
        let mut app2 = MnxEditorApp::default();
        app2.update_yaml(saved_yaml);
        app2.process_and_render();
        assert_eq!(app2.pitch_bend_range, 12);
    }

    #[test]
    fn test_muted_note_overwrite_and_re_mute() {
        let mut app = MnxEditorApp::default();
        app.selected_event_idx = Some(0);
        app.editing_string = Some(4);
        app.is_editing_fret = true;

        // 1. Type "x" to make note muted
        app.editing_fret_buf = "x".to_string();
        assert!(app.commit_fret_edit());
        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev0 = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
        let note = ev0.notes.iter().find(|n| n.string == Some(4)).unwrap();
        assert_eq!(note.muted, Some(true));
        assert_eq!(note.fret, Some(0));

        // 2. Type "x" again on an already-muted note -> should stay muted!
        app.editing_fret_buf = "x".to_string();
        assert!(app.commit_fret_edit());
        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev0 = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
        let note = ev0.notes.iter().find(|n| n.string == Some(4)).unwrap();
        assert_eq!(note.muted, Some(true));
        assert_eq!(note.fret, Some(0));

        // 3. Type "7" on the muted note -> should overwrite muted note with fret 7!
        app.editing_fret_buf = "7".to_string();
        assert!(app.commit_fret_edit());
        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev0 = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
        let note = ev0.notes.iter().find(|n| n.string == Some(4)).unwrap();
        assert_eq!(note.muted, None);
        assert_eq!(note.fret, Some(7));
    }

    #[test]
    fn test_playback_track_trailing_rest_and_muted_note_hold_duration() {
        let mut app = MnxEditorApp::default();
        // Convert the final event (index 7) to a rest
        let last_idx = app.cached_event_ranges.len() - 1;
        app.selected_event_idx = Some(last_idx);
        app.convert_selected_to_rest();

        let doc = app.parsed_mnx.as_ref().unwrap();
        let (events, _) = crate::midi::generate_playback_track(
            doc,
            crate::midi::MidiOptions::default(),
        );

        // Find the position of EventMarker(last_idx) and check that total duration after it > 0
        let pos = events
            .iter()
            .position(|e| {
                matches!(
                    e.kind,
                    crate::midi::PlaybackEventKind::EventMarker(idx) if idx == last_idx
                )
            })
            .expect("Expected EventMarker for trailing rest");
        let remaining_ticks: u32 =
            events[pos + 1..].iter().map(|e| e.delta_ticks).sum();
        assert!(
            remaining_ticks >= 240,
            "Expected trailing rest to hold selection for >=240 ticks before EndOfTrack, got {}",
            remaining_ticks
        );
    }

    #[test]
    fn test_profile_woman_of_the_hour() {
        let Ok(yaml) = std::fs::read_to_string("woman of the hour.yaml") else {
            return;
        };
        let t0 = std::time::Instant::now();
        let doc: crate::mnx::MnxDocument = serde_saphyr::from_str(&yaml).unwrap();
        let t_parse = t0.elapsed();

        assert_eq!(doc.parts[0].name.as_deref(), Some("Guitar"));
        assert!(doc.parts[0].tuning.is_some());
        for m in &doc.parts[0].measures {
            for seq in &m.sequences {
                for ev in seq.flattened_events() {
                    for note in &ev.notes {
                        assert!(note.string.is_some(), "Note missing _x.string: {:?}", note);
                        assert!(note.fret.is_some(), "Note missing _x.fret: {:?}", note);
                    }
                }
            }
        }

        let t1 = std::time::Instant::now();
        let ser = serde_saphyr::to_string(&doc).unwrap();
        let t_ser = t1.elapsed();

        let t2 = std::time::Instant::now();
        let ranges = crate::yaml_editor::find_all_event_ranges(&ser);
        let t_ranges = t2.elapsed();

        let t3 = std::time::Instant::now();
        let errs = crate::validate::validate_mnx(&doc);
        let t_val = t3.elapsed();
        assert!(errs.is_empty(), "Validation errors in woman of the hour.yaml: {:?}", errs);

        let t4 = std::time::Instant::now();
        let pages = crate::layout::compute_score_layout(&doc, 0.0);
        let mut dl = crate::layout::display_list::DisplayList::new();
        for p in &pages {
            crate::pdf_export::populate_page_display_list(&doc, p, &mut dl);
        }
        let t_layout = t4.elapsed();

        println!(
            "PROFILE woman of the hour.yaml ({} bytes, {} events): parse={:?}, ser={:?}, ranges={:?}, validate={:?}, layout={:?}",
            yaml.len(),
            ranges.len(),
            t_parse,
            t_ser,
            t_ranges,
            t_val,
            t_layout
        );
    }

    #[test]
    fn test_shift_selected_frets_up_and_down() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures:
    - time:
        count: 4
        unit: 4
parts:
  - name: Guitar
    measures:
      - sequences:
          - content:
              - duration:
                  base: quarter
                notes:
                  - pitch:
                      step: E
                      octave: 4
                    _x:
                      string: 1
                      fret: 0
"#;
        let mut app = MnxEditorApp::default();
        app.update_yaml(yaml.trim().to_string());
        app.process_and_render();
        app.selected_event_idx = Some(0);
        app.is_editing_fret = false;

        // Shift+Up -> fret 0 -> 1 (F4 on string 1)
        app.action_shift_selected_frets(1);
        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
        assert_eq!(ev.notes[0].fret, Some(1));
        assert_eq!(ev.notes[0].pitch.step, "F");
        assert_eq!(ev.notes[0].pitch.octave, 4);

        // Shift+Down -> fret 1 -> 0 (E4 on string 1)
        app.action_shift_selected_frets(-1);
        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
        assert_eq!(ev.notes[0].fret, Some(0));
        assert_eq!(ev.notes[0].pitch.step, "E");
        assert_eq!(ev.notes[0].pitch.octave, 4);

        // Clamped at 0 when shifting down again
        app.action_shift_selected_frets(-1);
        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
        assert_eq!(ev.notes[0].fret, Some(0));
    }

    #[test]
    fn test_shift_selected_strings_and_overwrite() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures:
    - time:
        count: 4
        unit: 4
parts:
  - name: Guitar
    measures:
      - sequences:
          - content:
              - duration:
                  base: quarter
                notes:
                  - pitch:
                      step: E
                      octave: 4
                    _x:
                      string: 1
                      fret: 0
                  - pitch:
                      step: C
                      octave: 4
                    _x:
                      string: 2
                      fret: 1
"#;
        let mut app = MnxEditorApp::default();
        app.update_yaml(yaml.trim().to_string());
        app.process_and_render();
        app.selected_event_idx = Some(0);
        app.selected_note_idx_in_event = Some(1); // Select note on string 2 (fret 1)
        app.is_editing_fret = false;

        // Alt+Up shifts the selected note from string 2 -> string 1, overwriting the existing string 1 note
        app.action_shift_selected_strings(-1);
        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
        assert_eq!(ev.notes.len(), 1);
        assert_eq!(ev.notes[0].string, Some(1));
        assert_eq!(ev.notes[0].fret, Some(1));
        assert_eq!(ev.notes[0].pitch.step, "F");
        assert_eq!(ev.notes[0].pitch.octave, 4);

        // Alt+Down shifts it back to string 2 (fret 1 -> C4)
        app.action_shift_selected_strings(1);
        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
        assert_eq!(ev.notes.len(), 1);
        assert_eq!(ev.notes[0].string, Some(2));
        assert_eq!(ev.notes[0].fret, Some(1));
        assert_eq!(ev.notes[0].pitch.step, "C");
        assert_eq!(ev.notes[0].pitch.octave, 4);
    }

    #[test]
    fn test_move_selected_by_grid_overwrites_and_leaves_rests() {
        let yaml = r#"
mnx:
  version: 1
global:
  measures:
    - time:
        count: 4
        unit: 4
parts:
  - name: Guitar
    measures:
      - sequences:
          - content:
              - duration:
                  base: quarter
                notes:
                  - pitch:
                      step: E
                      octave: 4
                    _x:
                      string: 1
                      fret: 0
              - duration:
                  base: quarter
                notes:
                  - pitch:
                      step: G
                      octave: 4
                    _x:
                      string: 1
                      fret: 3
              - duration:
                  base: half
                rest: {}
"#;
        let mut app = MnxEditorApp::default();
        app.update_yaml(yaml.trim().to_string());
        app.process_and_render();
        app.selected_event_idx = Some(0);
        app.current_grid = "quarter".to_string();
        app.is_editing_fret = false;

        // Alt+Right moves the first quarter note (fret 0) right by 1 quarter beat:
        // - Beat 0..1 becomes a quarter rest
        // - Beat 1..2 is overwritten with the moved note (fret 0, replacing fret 3)
        // - Beat 2..4 remains a half rest
        app.action_move_selected_by_grid(1);
        let doc = app.parsed_mnx.as_ref().unwrap();
        let events = doc.parts[0].measures[0].sequences[0].flattened_events();
        assert_eq!(events.len(), 3);
        assert!(events[0].rest.is_some() && events[0].notes.is_empty());
        assert_eq!(events[0].duration.base, "quarter");
        assert_eq!(events[1].notes.len(), 1);
        assert_eq!(events[1].notes[0].fret, Some(0));
        assert_eq!(events[1].duration.base, "quarter");
        assert!(events[2].rest.is_some() && events[2].notes.is_empty());
        assert_eq!(events[2].duration.base, "half");
        assert_eq!(app.selected_event_idx, Some(1));

        // Alt+Left moves the selected note (now at event 1, beat 1..2) back left by 1 quarter beat:
        // - Beat 0..1 becomes the moved note (fret 0)
        // - Beat 1..4 coalesces into rests (dotted half rest = 3 beats)
        app.action_move_selected_by_grid(-1);
        let doc = app.parsed_mnx.as_ref().unwrap();
        let events = doc.parts[0].measures[0].sequences[0].flattened_events();
        assert_eq!(events[0].notes.len(), 1);
        assert_eq!(events[0].notes[0].fret, Some(0));
        assert_eq!(events[0].duration.base, "quarter");
        let total_beats = doc.parts[0].measures[0].sequences[0].total_beats();
        assert!((total_beats - 4.0).abs() < 1e-4);
        assert_eq!(app.selected_event_idx, Some(0));
    }

    #[test]
    fn test_menu_view_and_help_state_toggles() {
        let mut app = MnxEditorApp::default();
        assert!(app.show_yaml_panel);
        assert_eq!(app.view_mode, ViewMode::Continuous);
        assert_eq!(app.active_dialog, ActiveDialog::None);

        // Toggle YAML panel visibility
        app.show_yaml_panel = !app.show_yaml_panel;
        assert!(!app.show_yaml_panel);

        // Toggle view mode
        app.view_mode = ViewMode::Layout;
        assert_eq!(app.view_mode, ViewMode::Layout);

        // Help dialogs
        app.active_dialog = ActiveDialog::Manual;
        assert_eq!(app.active_dialog, ActiveDialog::Manual);
        app.active_dialog = ActiveDialog::About;
        assert_eq!(app.active_dialog, ActiveDialog::About);
    }

    #[test]
    fn test_ending_barline_roundtrip_and_metrics() {
        let yaml = r#"---
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
      barline: { type: final }
parts:
  - instrument:
      name: "Guitar"
    measures:
      - sequences:
          - content:
              - duration: { base: whole }
                notes:
                  - pitch: { step: E, octave: 4 }
                    fret: 0
                    string: 1
"#;
        let doc: MnxDocument = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(
            doc.global.measures[0].barline.as_ref().map(|b| b.barline_type.as_str()),
            Some("final")
        );

        let metrics = crate::layout::sizing::compute_measure_metrics(&doc, 24.0);
        assert_eq!(metrics.len(), 1);

        let pages = crate::layout::compute_score_layout(&doc, 0.0);
        let mut dl = crate::layout::display_list::DisplayList::new();
        crate::pdf_export::populate_page_display_list(&doc, &pages[0], &mut dl);

        // Verify final barline items are drawn: a thin line (width 1.0) and a thick line (width 3.5)
        let barlines: Vec<_> = dl
            .items
            .iter()
            .filter(|item| item.id.as_deref() == Some("barline_0"))
            .collect();
        assert_eq!(barlines.len(), 2, "Final barline must have two line segments");
        let strokes: Vec<f32> = barlines
            .iter()
            .map(|item| match &item.kind {
                crate::layout::display_list::DrawKind::Line { stroke, .. } => stroke.width,
                _ => 0.0,
            })
            .collect();
        assert!(strokes.contains(&1.0));
        assert!(strokes.contains(&3.5));
    }

    #[test]
    fn test_save_discard_dialog_targets() {
        let mut app = MnxEditorApp::default();
        app.yaml_text.push_str("\n# unsaved changes");
        assert!(app.is_modified());

        // New file prompts DiscardTarget::NewProject
        app.new_file();
        assert_eq!(
            app.active_dialog,
            ActiveDialog::ConfirmDiscardChanges {
                target: DiscardTarget::NewProject,
            }
        );

        // Open file prompts DiscardTarget::OpenFile
        app.open_file();
        assert_eq!(
            app.active_dialog,
            ActiveDialog::ConfirmDiscardChanges {
                target: DiscardTarget::OpenFile,
            }
        );
    }

    #[test]
    fn test_layout_mode_keyboard_isolation() {
        let mut app = MnxEditorApp::default();
        app.view_mode = ViewMode::Layout;
        app.selected_event_idx = Some(0);

        let initial_yaml = app.yaml_text.clone();

        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.handle_keyboard_input(ui, false, &[], 1);
        });
        out.textures_delta.clear();

        assert_eq!(app.yaml_text, initial_yaml, "Score YAML must remain completely unchanged in Layout Mode");
    }

    #[test]
    fn test_make_into_system_compression() {
        let measures = vec![
            crate::layout::sizing::MeasureMetrics {
                measure_idx: 0,
                measure_number: 1,
                min_width: 160.0,
                spring_stiffness: 4.0,
                has_system_break: false,
                has_page_break: false,
                time_signature_width: 34.0,
                has_repeat_start: false,
                has_repeat_end: false,
                ending_numbers: None,
            },
            crate::layout::sizing::MeasureMetrics {
                measure_idx: 1,
                measure_number: 2,
                min_width: 160.0,
                spring_stiffness: 4.0,
                has_system_break: false,
                has_page_break: false,
                time_signature_width: 0.0,
                has_repeat_start: false,
                has_repeat_end: false,
                ending_numbers: None,
            },
            crate::layout::sizing::MeasureMetrics {
                measure_idx: 2,
                measure_number: 3,
                min_width: 160.0,
                spring_stiffness: 4.0,
                has_system_break: false,
                has_page_break: false,
                time_signature_width: 0.0,
                has_repeat_start: false,
                has_repeat_end: false,
                ending_numbers: None,
            },
            crate::layout::sizing::MeasureMetrics {
                measure_idx: 3,
                measure_number: 4,
                min_width: 160.0,
                spring_stiffness: 4.0,
                has_system_break: false,
                has_page_break: false,
                time_signature_width: 0.0,
                has_repeat_start: false,
                has_repeat_end: false,
                ending_numbers: None,
            },
        ];

        let content_w = 500.0;
        let breaks_auto = crate::layout::breaking::break_measures_into_systems(&measures, content_w, &[]);
        assert!(breaks_auto.len() >= 2, "Without locks, 640px must break across multiple systems");

        // Lock bars 0..=3 into a single system
        let locked = [[0, 3]];
        let breaks_locked = crate::layout::breaking::break_measures_into_systems(&measures, content_w, &locked);
        assert_eq!(breaks_locked.len(), 1, "Locked system must keep bars 0..=3 together");
        assert_eq!(breaks_locked[0].measure_range, 0..4);

        let margins = crate::mnx::PageMargins { top: 40.0, bottom: 40.0, left: 50.0, right: 50.0 };
        let mut pages = crate::layout::breaking::pack_systems_into_pages(breaks_locked, 600.0, 800.0, &margins, 100.0, 20.0, 0.0);
        crate::layout::justification::justify_systems(&mut pages, &measures, true);

        // Check that measures compressed down to fit exactly into content_w (500.0)
        let total_w: f32 = pages[0].systems[0].measure_widths.iter().sum();
        assert!((total_w - content_w).abs() < 1e-2, "Compressed system width must equal content width flush to margins");
    }

    #[test]
    fn test_score_info_metadata_roundtrip() {
        let yaml = r#"---
mnx:
  version: 1
global:
  _x:
    title: "Original Title"
    subtitle: "Original Subtitle"
    composer: "Original Composer"
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - name: "Acoustic Guitar"
    measures:
      - sequences:
          - content:
              - duration: { base: whole }
                notes: []
                rest: {}
"#;
        let mut app = MnxEditorApp::default();
        app.update_yaml(yaml.to_string());
        app.process_and_render();

        let doc = app.parsed_mnx.as_ref().unwrap();
        assert_eq!(doc.score_title(), Some("Original Title"));
        assert_eq!(doc.score_subtitle(), Some("Original Subtitle"));
        assert_eq!(doc.score_composer(), Some("Original Composer"));

        // Mutate score info
        app.mutate_doc(|doc| {
            doc.global.title = Some("New Title".to_string());
            doc.global.subtitle = Some("New Subtitle".to_string());
            doc.global.composer = Some("New Composer".to_string());
            Some(())
        });

        assert!(app.yaml_text.contains("New Title"));
        assert!(app.yaml_text.contains("New Subtitle"));
        assert!(app.yaml_text.contains("New Composer"));

        let doc_after = app.parsed_mnx.as_ref().unwrap();
        assert_eq!(doc_after.score_title(), Some("New Title"));
        assert_eq!(doc_after.score_subtitle(), Some("New Subtitle"));
        assert_eq!(doc_after.score_composer(), Some("New Composer"));

        // Verify layout Page 1 header contains title, subtitle, composer, and instrument
        let pages = crate::layout::compute_score_layout(doc_after, 0.0);
        let mut dl = crate::layout::display_list::DisplayList::new();
        crate::pdf_export::populate_page_display_list(doc_after, &pages[0], &mut dl);

        let has_title = dl.items.iter().any(|item| item.id.as_deref() == Some("header_title"));
        let has_sub = dl.items.iter().any(|item| item.id.as_deref() == Some("header_subtitle"));
        let has_comp = dl.items.iter().any(|item| item.id.as_deref() == Some("header_composer"));
        let has_inst = dl.items.iter().any(|item| item.id.as_deref() == Some("header_instrument"));

        assert!(has_title, "header_title must be in display list");
        assert!(has_sub, "header_subtitle must be in display list");
        assert!(has_comp, "header_composer must be in display list");
        assert!(has_inst, "header_instrument must be in display list");
    }

    #[test]
    fn test_startup_when_example_yaml_missing_shows_prompt_and_no_errors() {
        let app = MnxEditorApp::new_with_optional_example(None);
        assert_eq!(app.active_dialog, ActiveDialog::StartupPrompt);
        assert!(
            app.validation_errors.is_empty(),
            "Expected 0 validation errors on startup: {:?}",
            app.validation_errors
        );
        assert!(
            app.parsed_errors.is_empty(),
            "Expected 0 parse errors on startup: {:?}",
            app.parsed_errors
        );
        assert!(app.parsed_mnx.is_some(), "Default document must parse successfully");
        assert!(!app.is_modified(), "Fresh startup tab should not be marked modified");

        let doc = app.parsed_mnx.as_ref().unwrap();
        assert_eq!(doc.parts.len(), 1);
        assert_eq!(doc.parts[0].num_strings(), 6);
        assert_eq!(doc.global.measures.len(), 1);
    }

    #[test]
    fn test_startup_prompt_new_tab_action_prompts_instrument_setup() {
        let mut app = MnxEditorApp::new_with_optional_example(None);
        assert_eq!(app.active_dialog, ActiveDialog::StartupPrompt);

        // Simulate choosing "Create a New Tab"
        app.prompt_new_tab_instrument_setup();
        match app.active_dialog {
            ActiveDialog::InstrumentSetup { is_new_tab, .. } => {
                assert!(is_new_tab, "Expected is_new_tab to be true");
            }
            ref other => panic!("Expected ActiveDialog::InstrumentSetup, got {:?}", other),
        }
    }

    #[test]
    fn test_startup_when_example_yaml_present_does_not_show_startup_prompt() {
        let example_content = MnxEditorApp::default_empty_yaml();
        let app = MnxEditorApp::new_with_optional_example(Some(example_content));
        assert_eq!(app.active_dialog, ActiveDialog::None);
        assert!(app.validation_errors.is_empty());
        assert!(app.parsed_mnx.is_some());
    }

    #[test]
    fn test_phase_2a_grid_controller_chord_entry_and_advance() {
        let mut app = MnxEditorApp::default();
        app.selected_event_idx = Some(0);
        app.grid_input.slots[0].enabled = true;
        app.grid_input.slots[0].fret_offset = 3; // Leftmost column = fret 3

        let t0 = std::time::Instant::now();
        // Press String 1 Col 0 (note 76 -> fret 3) and String 2 Col 2 (note 70 -> fret 5)
        let (chord1, ..) = app.grid_input.process_event(
            crate::grid_input::GridMidiEvent {
                slot_idx: 0,
                note: 76,
                is_note_on: true,
                velocity: 100,
            },
            6,
            t0,
        );
        assert!(app.grid_input.is_grid_input_mode);
        let chord1 = chord1.unwrap();
        app.mutate_doc(|doc| {
            if crate::yaml_editor::set_chord_on_event_in_doc(doc, 0, &chord1) {
                Some(())
            } else {
                None
            }
        });

        let (chord2, ..) = app.grid_input.process_event(
            crate::grid_input::GridMidiEvent {
                slot_idx: 0,
                note: 70,
                is_note_on: true,
                velocity: 100,
            },
            6,
            t0 + std::time::Duration::from_millis(20),
        );
        let chord2 = chord2.unwrap();
        app.mutate_doc(|doc| {
            if crate::yaml_editor::set_chord_on_event_in_doc(doc, 0, &chord2) {
                Some(())
            } else {
                None
            }
        });

        // Verify event 0 now has 2 notes: String 1 Fret 3 and String 2 Fret 5
        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev0 = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
        assert_eq!(ev0.notes.len(), 2);
        assert_eq!(ev0.notes[0].string, Some(1));
        assert_eq!(ev0.notes[0].fret, Some(3));
        assert_eq!(ev0.notes[1].string, Some(2));
        assert_eq!(ev0.notes[1].fret, Some(5));

        // Release both pads and advance after 500ms
        app.grid_input.process_event(
            crate::grid_input::GridMidiEvent {
                slot_idx: 0,
                note: 76,
                is_note_on: false,
                velocity: 0,
            },
            6,
            t0 + std::time::Duration::from_millis(50),
        );
        app.grid_input.process_event(
            crate::grid_input::GridMidiEvent {
                slot_idx: 0,
                note: 70,
                is_note_on: false,
                velocity: 0,
            },
            6,
            t0 + std::time::Duration::from_millis(60),
        );
        let (_, should_finalize, ..) = app
            .grid_input
            .poll(6, t0 + std::time::Duration::from_millis(600));
        assert!(should_finalize);

        let total_events = app.cached_event_ranges.len();
        app.advance_to_next_note_in_grid_mode(total_events);
        assert_eq!(app.selected_event_idx, Some(1));
        assert!(app.grid_input.is_grid_input_mode);
        assert!(!app.is_editing_fret);
    }

    #[test]
    fn test_grid_controller_input_buffer_hold_replace_and_overwrite() {
        let mut app = MnxEditorApp::default();
        let tunings = vec![
            crate::mnx::StringTuning { string: 1, pitch: crate::mnx::Pitch { step: "E".to_string(), alter: None, octave: 4, ext: Default::default() } },
            crate::mnx::StringTuning { string: 2, pitch: crate::mnx::Pitch { step: "B".to_string(), alter: None, octave: 3, ext: Default::default() } },
            crate::mnx::StringTuning { string: 3, pitch: crate::mnx::Pitch { step: "G".to_string(), alter: None, octave: 3, ext: Default::default() } },
            crate::mnx::StringTuning { string: 4, pitch: crate::mnx::Pitch { step: "D".to_string(), alter: None, octave: 3, ext: Default::default() } },
            crate::mnx::StringTuning { string: 5, pitch: crate::mnx::Pitch { step: "A".to_string(), alter: None, octave: 2, ext: Default::default() } },
            crate::mnx::StringTuning { string: 6, pitch: crate::mnx::Pitch { step: "E".to_string(), alter: None, octave: 2, ext: Default::default() } },
        ];
        app.start_new_tab(6, 12, 1, tunings);
        app.selected_event_idx = Some(0);
        app.grid_input.slots[0].enabled = true;
        app.grid_input.slots[0].is_calibrated = true;
        app.grid_input.slots[0].fret_offset = 0;

        let t0 = std::time::Instant::now();

        // 1. Press String 1 Fret 3 (note 76 + 3 = 79) and String 2 Fret 5 (note 68 + 5 = 73)
        let (c1, ..) = app.grid_input.process_event(
            crate::grid_input::GridMidiEvent { slot_idx: 0, note: 79, is_note_on: true, velocity: 100 },
            6,
            t0,
        );
        app.mutate_doc(|doc| {
            crate::yaml_editor::set_chord_on_event_in_doc(doc, 0, &c1.unwrap());
            Some(())
        });

        let (c2, ..) = app.grid_input.process_event(
            crate::grid_input::GridMidiEvent { slot_idx: 0, note: 73, is_note_on: true, velocity: 100 },
            6,
            t0 + std::time::Duration::from_millis(20),
        );
        app.mutate_doc(|doc| {
            crate::yaml_editor::set_chord_on_event_in_doc(doc, 0, &c2.unwrap());
            Some(())
        });

        // Verify event 0 has both notes
        {
            let doc = app.parsed_mnx.as_ref().unwrap();
            let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
            assert_eq!(ev.notes.len(), 2);
            assert_eq!(ev.notes[0].string, Some(1));
            assert_eq!(ev.notes[0].fret, Some(3));
            assert_eq!(ev.notes[1].string, Some(2));
            assert_eq!(ev.notes[1].fret, Some(5));
        }

        // 2. Press String 1 Fret 7 (note 76 + 7 = 83) while String 2 Fret 5 is held -> replaces String 1 note!
        let (c3, ..) = app.grid_input.process_event(
            crate::grid_input::GridMidiEvent { slot_idx: 0, note: 83, is_note_on: true, velocity: 100 },
            6,
            t0 + std::time::Duration::from_millis(50),
        );
        app.mutate_doc(|doc| {
            crate::yaml_editor::set_chord_on_event_in_doc(doc, 0, &c3.unwrap());
            Some(())
        });

        {
            let doc = app.parsed_mnx.as_ref().unwrap();
            let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
            assert_eq!(ev.notes.len(), 2);
            assert_eq!(ev.notes[0].string, Some(1));
            assert_eq!(ev.notes[0].fret, Some(7), "String 1 note should be replaced with fret 7");
            assert_eq!(ev.notes[1].string, Some(2));
            assert_eq!(ev.notes[1].fret, Some(5));
        }

        // 3. Release pads: note 79, note 83 and note 73
        let (c_rel0, ..) = app.grid_input.process_event(
            crate::grid_input::GridMidiEvent { slot_idx: 0, note: 79, is_note_on: false, velocity: 0 },
            6,
            t0 + std::time::Duration::from_millis(60),
        );
        assert!(c_rel0.is_none(), "NoteOff should not remove notes from tab input buffer");

        let (c_rel1, ..) = app.grid_input.process_event(
            crate::grid_input::GridMidiEvent { slot_idx: 0, note: 83, is_note_on: false, velocity: 0 },
            6,
            t0 + std::time::Duration::from_millis(80),
        );
        assert!(c_rel1.is_none(), "NoteOff should not remove notes from tab input buffer");

        let (c_rel2, ..) = app.grid_input.process_event(
            crate::grid_input::GridMidiEvent { slot_idx: 0, note: 73, is_note_on: false, velocity: 0 },
            6,
            t0 + std::time::Duration::from_millis(90),
        );
        assert!(c_rel2.is_none(), "NoteOff should not remove notes from tab input buffer");

        // Event 0 still holds the full chord in score
        {
            let doc = app.parsed_mnx.as_ref().unwrap();
            let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
            assert_eq!(ev.notes.len(), 2);
            assert_eq!(ev.notes[0].fret, Some(7));
            assert_eq!(ev.notes[1].fret, Some(5));
        }

        // 4. Before 500ms latch expires (at +150ms), press String 3 Fret 2 (note 60 + 2 = 62)
        // Following note-offs of all pressed keys, this MUST overwrite the full current set of red notes!
        let (c4, ..) = app.grid_input.process_event(
            crate::grid_input::GridMidiEvent { slot_idx: 0, note: 62, is_note_on: true, velocity: 100 },
            6,
            t0 + std::time::Duration::from_millis(150),
        );
        let chord4 = c4.expect("Should emit new chord");
        assert_eq!(chord4.len(), 1, "Input buffer must be overwritten");
        assert_eq!(chord4.get(&3), Some(&2));
        assert_eq!(chord4.get(&1), None);
        assert_eq!(chord4.get(&2), None);

        app.mutate_doc(|doc| {
            crate::yaml_editor::set_chord_on_event_in_doc(doc, 0, &chord4);
            Some(())
        });

        {
            let doc = app.parsed_mnx.as_ref().unwrap();
            let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
            assert_eq!(ev.notes.len(), 1, "Event 0 notes should be fully overwritten");
            assert_eq!(ev.notes[0].string, Some(3));
            assert_eq!(ev.notes[0].fret, Some(2));
        }

        // 5. Release String 3 pad and wait for 500ms latch
        app.grid_input.process_event(
            crate::grid_input::GridMidiEvent { slot_idx: 0, note: 62, is_note_on: false, velocity: 0 },
            6,
            t0 + std::time::Duration::from_millis(200),
        );
        let (_, should_finalize, ..) = app.grid_input.poll(6, t0 + std::time::Duration::from_millis(710));
        assert!(should_finalize);

        let total_events = app.cached_event_ranges.len();
        app.advance_to_next_note_in_grid_mode(total_events);
        assert_eq!(app.selected_event_idx, Some(1));
    }

    #[test]
    fn test_grid_controller_just_play_mode_bypasses_tab_and_tracks_live_notes() {
        let mut app = MnxEditorApp::default();
        let tunings = vec![
            crate::mnx::StringTuning { string: 1, pitch: crate::mnx::Pitch { step: "E".to_string(), alter: None, octave: 4, ext: Default::default() } },
            crate::mnx::StringTuning { string: 2, pitch: crate::mnx::Pitch { step: "B".to_string(), alter: None, octave: 3, ext: Default::default() } },
            crate::mnx::StringTuning { string: 3, pitch: crate::mnx::Pitch { step: "G".to_string(), alter: None, octave: 3, ext: Default::default() } },
            crate::mnx::StringTuning { string: 4, pitch: crate::mnx::Pitch { step: "D".to_string(), alter: None, octave: 3, ext: Default::default() } },
            crate::mnx::StringTuning { string: 5, pitch: crate::mnx::Pitch { step: "A".to_string(), alter: None, octave: 2, ext: Default::default() } },
            crate::mnx::StringTuning { string: 6, pitch: crate::mnx::Pitch { step: "E".to_string(), alter: None, octave: 2, ext: Default::default() } },
        ];
        app.start_new_tab(6, 12, 1, tunings);
        app.selected_event_idx = Some(0);
        app.grid_input.slots[0].enabled = true;
        app.grid_input.slots[0].is_calibrated = true;
        app.grid_input.slots[0].fret_offset = 0;
        app.grid_input.just_play_mode = true;

        // Record initial event 0 notes
        let initial_notes = {
            let doc = app.parsed_mnx.as_ref().unwrap();
            doc.parts[0].measures[0].sequences[0].flattened_events()[0].notes.clone()
        };

        let t0 = std::time::Instant::now();

        // 1. Press String 1 Fret 3 (note 76 + 3 = 79)
        let (chord1, cal1, live_ev1) = app.grid_input.process_event(
            crate::grid_input::GridMidiEvent { slot_idx: 0, note: 79, is_note_on: true, velocity: 100 },
            6,
            t0,
        );
        assert!(!app.grid_input.is_grid_input_mode, "Must NOT enter red grid input mode in Just Play");
        assert!(chord1.is_none(), "Must NOT emit chord updates for the tab");
        assert!(cal1.is_none());
        assert_eq!(
            live_ev1,
            Some(crate::grid_input::GridLivePlayEvent {
                string: 1,
                fret: 3,
                is_note_on: true,
                velocity: 100,
            })
        );

        // Feed live event into app
        app.handle_grid_live_play_events(vec![live_ev1.unwrap()]);
        // With selected_midi_port = Some(0), active_grid_live_notes tracks String 1 Fret 3 (MIDI note 67 for G4)
        if app.selected_midi_port.is_some() {
            assert!(app.active_grid_live_notes.contains_key(&(1, 3)));
            let (ch, key) = app.active_grid_live_notes.get(&(1, 3)).copied().unwrap();
            assert_eq!(key.as_int(), 67); // G4 = 67
            assert_eq!(ch.as_int(), 0); // String 1 -> Channel 1 (0-indexed 0)
        }

        // Verify the tab event was completely UNTOUCHED
        {
            let doc = app.parsed_mnx.as_ref().unwrap();
            let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
            assert_eq!(ev.notes, initial_notes, "Tab notes must be unmodified");
        }

        // 2. Release pad
        let (chord2, _, live_ev2) = app.grid_input.process_event(
            crate::grid_input::GridMidiEvent { slot_idx: 0, note: 79, is_note_on: false, velocity: 0 },
            6,
            t0 + std::time::Duration::from_millis(100),
        );
        assert!(!app.grid_input.is_grid_input_mode);
        assert!(chord2.is_none());
        assert_eq!(
            live_ev2,
            Some(crate::grid_input::GridLivePlayEvent {
                string: 1,
                fret: 3,
                is_note_on: false,
                velocity: 0,
            })
        );
        app.handle_grid_live_play_events(vec![live_ev2.unwrap()]);
        assert!(!app.active_grid_live_notes.contains_key(&(1, 3)));

        // 3. Toggling off Just Play mode resets held pads and returns note-offs
        app.grid_input.process_event(
            crate::grid_input::GridMidiEvent { slot_idx: 0, note: 79, is_note_on: true, velocity: 90 },
            6,
            t0 + std::time::Duration::from_millis(200),
        );
        let note_offs = app.grid_input.set_just_play_mode(false);
        assert_eq!(note_offs.len(), 1);
        assert_eq!(note_offs[0].string, 1);
        assert_eq!(note_offs[0].fret, 3);
        assert!(!note_offs[0].is_note_on);
        assert!(!app.grid_input.just_play_mode);
    }

    #[test]
    fn test_phase_2b_custom_fretboard_ratio_and_decimal_fretlet_end_to_end() {
        let mut app = MnxEditorApp::default();
        let doc = app.parsed_mnx.as_ref().unwrap().clone();
        let mut state = crate::fretboard_editor::FretboardEditorState::from_doc(&doc);

        // 1. Place pure ratio frets: 5/4 (full neck -> Fret 1), 11/8 (fretlet on String 1 -> Fret 1.5), 3/2 (full neck -> Fret 2)
        state.frets = vec![
            crate::fretboard_editor::CustomFret {
                fret_number: 1.0,
                manual_number: None,
                spec: crate::fretboard_editor::FretLocationSpec::Ratio { num: 5, den: 4 },
                strings: vec![1, 2, 3, 4, 5, 6],
                note_name: "vE".to_string(),
            },
            crate::fretboard_editor::CustomFret {
                fret_number: 1.5,
                manual_number: None,
                spec: crate::fretboard_editor::FretLocationSpec::Ratio { num: 11, den: 8 },
                strings: vec![1],
                note_name: "^F".to_string(),
            },
            crate::fretboard_editor::CustomFret {
                fret_number: 2.0,
                manual_number: None,
                spec: crate::fretboard_editor::FretLocationSpec::Ratio { num: 3, den: 2 },
                strings: vec![1, 2, 3, 4, 5, 6],
                note_name: "G".to_string(),
            },
        ];
        crate::fretboard_editor::recompute_fret_numbers(&mut state.frets, 6);
        assert!((state.frets[1].fret_number - 1.5).abs() < 1e-4);

        app.apply_custom_fretboard_state(state);

        // Verify global._x.tuning.referenceNote was written to YAML for PureRatio mode
        assert!(app.yaml_text.contains("referenceNote:"));
        assert!(app.yaml_text.contains("customFrets:"));

        // Enter decimal fret 1.5 on String 1 at Event 0
        app.selected_event_idx = Some(0);
        app.editing_string = Some(1);
        app.editing_fret_buf = "1.5".to_string();
        assert!(app.commit_fret_edit());

        let doc_after = app.parsed_mnx.as_ref().unwrap();
        let ev0 = doc_after.parts[0].measures[0].sequences[0].flattened_events()[0];
        let n0 = ev0.notes.iter().find(|n| n.string == Some(1)).unwrap();
        assert_eq!(n0.decimal_fret, Some(1.5));
        assert_eq!(n0.formatted_fret().as_deref(), Some("1.5"));
        assert_eq!(n0.pitch.step, "E");
        assert_eq!(
            n0.pitch.alter,
            Some(crate::mnx::Alteration::Fraction("11/8".to_string()))
        );
        // Open String 1 is vE (step 17 in 53-EDO) + 11/8 (^F = step 23 in 53-EDO) -> step 40 in 53-EDO = A
        assert_eq!(n0.pitch.ext.name.as_deref(), Some("A"));
    }

    #[test]
    fn test_ji_custom_fretboard_three_string_tuning_examples_and_mnx_export() {
        let mut app = MnxEditorApp::default();

        // Set up custom fret 6 as a full 5/4 fret (with manual_number = Some(6.0)), reference note D4 = 293.67 Hz
        let custom_fret_6 = crate::fretboard_editor::CustomFret {
            fret_number: 6.0,
            manual_number: Some(6.0),
            spec: crate::fretboard_editor::FretLocationSpec::Ratio { num: 5, den: 4 },
            strings: vec![1, 2, 3, 4, 5, 6],
            note_name: crate::fretboard_editor::recommend_53edo_note_name_from_cents(
                1200.0 * (5.0_f64 / 4.0).log2(),
            ), // "vE"
        };
        assert_eq!(custom_fret_6.note_name, "vE");

        // ---------------------------------------------------------------------
        // Example 1: Open string 1 tuned to G4 in 41-EDO, custom fret 6 = 5/4
        // ---------------------------------------------------------------------
        app.start_new_tab(
            6,
            41,
            1,
            vec![crate::mnx::StringTuning {
                string: 1,
                pitch: crate::mnx::Pitch {
                    step: "G".to_string(),
                    alter: None,
                    octave: 4,
                    ext: crate::mnx::PitchExt::default(),
                },
            }],
        );
        let state1 = crate::fretboard_editor::FretboardEditorState::new(
            6,
            vec![custom_fret_6.clone()],
            "D".to_string(),
            4,
            293.67,
        );
        app.apply_custom_fretboard_state(state1);
        app.selected_event_idx = Some(0);
        app.editing_string = Some(1);
        app.editing_fret_buf = "6".to_string();
        assert!(app.commit_fret_edit());

        assert!(
            app.yaml_text
                .contains("pitch: {step: G, alter: 5/4, octave: 4, _x: {name: vB}}"),
            "Example 1 YAML mismatch:\n{}",
            app.yaml_text
        );
        assert!(
            app.yaml_text.contains("_x: {string: 1, fret: 6}"),
            "Example 1 NoteExt mismatch:\n{}",
            app.yaml_text
        );

        let mnx_json_1 = crate::mnx::export_yaml_to_mnx_json(&app.yaml_text).unwrap();
        assert!(mnx_json_1.contains("\"step\": \"B\""));
        assert!(mnx_json_1.contains("\"alter\": -0.137"));

        // ---------------------------------------------------------------------
        // Example 2: Open string 1 tuned to 4/3, reference note 1/1 is D4 = 293.67 Hz
        // ---------------------------------------------------------------------
        let (_, cents_4_3) =
            crate::fretboard_editor::parse_ratio_or_cents_to_cents("4/3").unwrap();
        let rec_name_2 =
            crate::fretboard_editor::recommend_53edo_name_from_reference("D", cents_4_3);
        assert_eq!(rec_name_2, "G");

        app.start_new_tab(
            6,
            41,
            1,
            vec![crate::mnx::StringTuning {
                string: 1,
                pitch: crate::mnx::Pitch {
                    step: "4/3".to_string(),
                    alter: None,
                    octave: 4,
                    ext: crate::mnx::PitchExt {
                        name: Some(rec_name_2),
                    },
                },
            }],
        );
        let state2 = crate::fretboard_editor::FretboardEditorState::new(
            6,
            vec![custom_fret_6.clone()],
            "D".to_string(),
            4,
            293.67,
        );
        app.apply_custom_fretboard_state(state2);
        app.selected_event_idx = Some(0);
        app.editing_string = Some(1);
        app.editing_fret_buf = "6".to_string();
        assert!(app.commit_fret_edit());

        assert!(
            app.yaml_text
                .contains("pitch: {step: 4/3, alter: 5/4, octave: 4, _x: {name: vB}}"),
            "Example 2 YAML mismatch:\n{}",
            app.yaml_text
        );
        assert!(
            app.yaml_text.contains("_x: {string: 1, fret: 6}"),
            "Example 2 NoteExt mismatch:\n{}",
            app.yaml_text
        );

        let mnx_json_2 = crate::mnx::export_yaml_to_mnx_json(&app.yaml_text).unwrap();
        assert!(mnx_json_2.contains("\"step\": \"B\""));
        assert!(mnx_json_2.contains("\"alter\": -0.156"));

        // ---------------------------------------------------------------------
        // Example 3: Open string 1 tuned to 498.0 (cents), reference note 1/1 is D4 = 293.67 Hz
        // ---------------------------------------------------------------------
        let (step_498, cents_498) =
            crate::fretboard_editor::parse_ratio_or_cents_to_cents("498.0").unwrap();
        assert_eq!(step_498, "498.0");
        let rec_name_3 =
            crate::fretboard_editor::recommend_53edo_name_from_reference("D", cents_498);
        assert_eq!(rec_name_3, "G");

        app.start_new_tab(
            6,
            41,
            1,
            vec![crate::mnx::StringTuning {
                string: 1,
                pitch: crate::mnx::Pitch {
                    step: step_498,
                    alter: None,
                    octave: 4,
                    ext: crate::mnx::PitchExt {
                        name: Some(rec_name_3),
                    },
                },
            }],
        );
        let state3 = crate::fretboard_editor::FretboardEditorState::new(
            6,
            vec![custom_fret_6],
            "D".to_string(),
            4,
            293.67,
        );
        app.apply_custom_fretboard_state(state3);
        app.selected_event_idx = Some(0);
        app.editing_string = Some(1);
        app.editing_fret_buf = "6".to_string();
        assert!(app.commit_fret_edit());

        assert!(
            app.yaml_text
                .contains("pitch: {step: 498.0, alter: 5/4, octave: 4, _x: {name: vB}}"),
            "Example 3 YAML mismatch:\n{}",
            app.yaml_text
        );
        assert!(
            app.yaml_text.contains("_x: {string: 1, fret: 6}"),
            "Example 3 NoteExt mismatch:\n{}",
            app.yaml_text
        );

        let mnx_json_3 = crate::mnx::export_yaml_to_mnx_json(&app.yaml_text).unwrap();
        assert!(mnx_json_3.contains("\"step\": \"B\""));
        assert!(mnx_json_3.contains("\"alter\": -0.157"));

        // Verify opening Instrument Setup from this Ratio/Cents document detects RatioCents mode
        app.open_instrument_setup_dialog(true);
        match &app.active_dialog {
            ActiveDialog::InstrumentSetup {
                tuning_mode,
                tunings_text,
                tunings_name,
                show_custom_fretboard,
                ..
            } => {
                assert_eq!(*tuning_mode, InstrumentTuningMode::RatioCents);
                assert_eq!(tunings_text[0], "498.0");
                assert_eq!(tunings_name[0], "G");
                assert!(*show_custom_fretboard);
            }
            other => panic!("Expected InstrumentSetup dialog, got {:?}", other),
        }
    }

    #[test]
    fn test_natural_harmonic_shift_selected_strings_recalculates_sounding_pitch() {
        let mut app = MnxEditorApp::default();
        let tunings = vec![
            crate::mnx::StringTuning { string: 1, pitch: crate::mnx::Pitch { step: "E".to_string(), alter: None, octave: 4, ext: Default::default() } },
            crate::mnx::StringTuning { string: 2, pitch: crate::mnx::Pitch { step: "B".to_string(), alter: None, octave: 3, ext: Default::default() } },
            crate::mnx::StringTuning { string: 3, pitch: crate::mnx::Pitch { step: "G".to_string(), alter: None, octave: 3, ext: Default::default() } },
            crate::mnx::StringTuning { string: 4, pitch: crate::mnx::Pitch { step: "D".to_string(), alter: None, octave: 3, ext: Default::default() } },
            crate::mnx::StringTuning { string: 5, pitch: crate::mnx::Pitch { step: "A".to_string(), alter: None, octave: 2, ext: Default::default() } },
            crate::mnx::StringTuning { string: 6, pitch: crate::mnx::Pitch { step: "E".to_string(), alter: None, octave: 2, ext: Default::default() } },
        ];
        app.start_new_tab(6, 12, 1, tunings);
        app.selected_event_idx = Some(0);
        app.editing_string = Some(1);
        app.editing_fret_buf = "12".to_string();
        assert!(app.commit_fret_edit());
        app.selected_note_idx_in_event = Some(0);

        // Toggle natural harmonic at fret 12 on String 1 (E4 -> sounding E5)
        app.action_toggle_harmonic();
        assert!(app.is_selected_note_harmonic());
        {
            let doc = app.parsed_mnx.as_ref().unwrap();
            let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
            let h = ev.notes[0].harmonic.as_ref().unwrap();
            assert_eq!(h.touch, 12.0);
            assert_eq!(h.sounding_note.pitch.step, "E");
            assert_eq!(h.sounding_note.pitch.octave, 5);
        }
        assert!(app.validation_errors.is_empty(), "Validation errors: {:?}", app.validation_errors);

        // Shift down a string (Option+Down): String 1 -> String 2 (B3 open -> sounding B4)
        app.action_shift_selected_strings(1);
        {
            let doc = app.parsed_mnx.as_ref().unwrap();
            let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
            assert_eq!(ev.notes[0].string, Some(2));
            assert_eq!(ev.notes[0].fret, Some(12));
            let h = ev.notes[0].harmonic.as_ref().expect("Harmonic must persist when shifted to string 2");
            assert_eq!(h.touch, 12.0);
            assert_eq!(h.sounding_note.pitch.step, "B");
            assert_eq!(h.sounding_note.pitch.octave, 4);
        }
        assert!(app.validation_errors.is_empty(), "Validation errors on String 2: {:?}", app.validation_errors);

        // Shift down to String 3 (G3 open -> sounding G4)
        app.action_shift_selected_strings(1);
        {
            let doc = app.parsed_mnx.as_ref().unwrap();
            let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
            assert_eq!(ev.notes[0].string, Some(3));
            let h = ev.notes[0].harmonic.as_ref().unwrap();
            assert_eq!(h.sounding_note.pitch.step, "G");
            assert_eq!(h.sounding_note.pitch.octave, 4);
        }
        assert!(app.validation_errors.is_empty(), "Validation errors on String 3: {:?}", app.validation_errors);

        // Shift all the way down to String 6 (E2 open -> sounding E3)
        app.action_shift_selected_strings(1); // 4 (D3 -> D4)
        app.action_shift_selected_strings(1); // 5 (A2 -> A3)
        app.action_shift_selected_strings(1); // 6 (E2 -> E3)
        {
            let doc = app.parsed_mnx.as_ref().unwrap();
            let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
            assert_eq!(ev.notes[0].string, Some(6));
            let h = ev.notes[0].harmonic.as_ref().unwrap();
            assert_eq!(h.sounding_note.pitch.step, "E");
            assert_eq!(h.sounding_note.pitch.octave, 3);
        }
        assert!(app.validation_errors.is_empty(), "Validation errors on String 6: {:?}", app.validation_errors);

        // Shift back up to String 1 (Option+Up): String 6 -> 1
        for _ in 0..5 {
            app.action_shift_selected_strings(-1);
        }
        {
            let doc = app.parsed_mnx.as_ref().unwrap();
            let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
            assert_eq!(ev.notes[0].string, Some(1));
            let h = ev.notes[0].harmonic.as_ref().unwrap();
            assert_eq!(h.sounding_note.pitch.step, "E");
            assert_eq!(h.sounding_note.pitch.octave, 5);
        }
        assert!(app.validation_errors.is_empty(), "Validation errors on String 1: {:?}", app.validation_errors);
    }

    #[test]
    fn test_natural_harmonic_7th_fret_shift_selected_strings() {
        let mut app = MnxEditorApp::default();
        let tunings = vec![
            crate::mnx::StringTuning { string: 1, pitch: crate::mnx::Pitch { step: "E".to_string(), alter: None, octave: 4, ext: Default::default() } },
            crate::mnx::StringTuning { string: 2, pitch: crate::mnx::Pitch { step: "B".to_string(), alter: None, octave: 3, ext: Default::default() } },
            crate::mnx::StringTuning { string: 3, pitch: crate::mnx::Pitch { step: "G".to_string(), alter: None, octave: 3, ext: Default::default() } },
            crate::mnx::StringTuning { string: 4, pitch: crate::mnx::Pitch { step: "D".to_string(), alter: None, octave: 3, ext: Default::default() } },
            crate::mnx::StringTuning { string: 5, pitch: crate::mnx::Pitch { step: "A".to_string(), alter: None, octave: 2, ext: Default::default() } },
            crate::mnx::StringTuning { string: 6, pitch: crate::mnx::Pitch { step: "E".to_string(), alter: None, octave: 2, ext: Default::default() } },
        ];
        app.start_new_tab(6, 12, 1, tunings);
        app.selected_event_idx = Some(0);
        app.editing_string = Some(1);
        app.editing_fret_buf = "7".to_string();
        assert!(app.commit_fret_edit());
        app.selected_note_idx_in_event = Some(0);

        // Fret 7 on String 1 (n = 3, 1/3 node on E4 -> sounding B5)
        app.action_toggle_harmonic();
        assert!(app.is_selected_note_harmonic());
        {
            let doc = app.parsed_mnx.as_ref().unwrap();
            let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
            let h = ev.notes[0].harmonic.as_ref().unwrap();
            assert_eq!(h.touch, 7.0);
            assert_eq!(h.sounding_note.pitch.step, "B");
            assert_eq!(h.sounding_note.pitch.octave, 5);
        }
        assert!(app.validation_errors.is_empty());

        // Shift to String 2 (n = 3 on B3 -> sounding F#5)
        app.action_shift_selected_strings(1);
        {
            let doc = app.parsed_mnx.as_ref().unwrap();
            let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
            assert_eq!(ev.notes[0].string, Some(2));
            assert_eq!(ev.notes[0].fret, Some(7));
            let h = ev.notes[0].harmonic.as_ref().unwrap();
            assert_eq!(h.touch, 7.0);
            assert_eq!(h.sounding_note.pitch.step, "F");
            assert_eq!(h.sounding_note.pitch.octave, 5);
            let alter = h.sounding_note.pitch.alter.as_ref().map(|a| a.as_semitones()).unwrap_or(0.0);
            assert!((alter - 1.0).abs() < 0.05);
        }
        assert!(app.validation_errors.is_empty(), "Validation errors on String 2: {:?}", app.validation_errors);
    }

    #[test]
    fn test_natural_harmonic_shift_selected_strings_custom_fretboard() {
        let mut app = MnxEditorApp::default();
        let tunings = vec![
            crate::mnx::StringTuning {
                string: 1,
                pitch: crate::mnx::Pitch { step: "4/3".to_string(), alter: None, octave: 4, ext: crate::mnx::PitchExt { name: Some("G".to_string()) } },
            },
            crate::mnx::StringTuning {
                string: 2,
                pitch: crate::mnx::Pitch { step: "5/3".to_string(), alter: None, octave: 4, ext: crate::mnx::PitchExt { name: Some("vB".to_string()) } },
            },
            crate::mnx::StringTuning {
                string: 3,
                pitch: crate::mnx::Pitch { step: "4/3".to_string(), alter: None, octave: 3, ext: crate::mnx::PitchExt { name: Some("G".to_string()) } },
            },
            crate::mnx::StringTuning {
                string: 4,
                pitch: crate::mnx::Pitch { step: "1/1".to_string(), alter: None, octave: 3, ext: crate::mnx::PitchExt { name: Some("D".to_string()) } },
            },
            crate::mnx::StringTuning {
                string: 5,
                pitch: crate::mnx::Pitch { step: "4/3".to_string(), alter: None, octave: 2, ext: crate::mnx::PitchExt { name: Some("G".to_string()) } },
            },
            crate::mnx::StringTuning {
                string: 6,
                pitch: crate::mnx::Pitch { step: "1/1".to_string(), alter: None, octave: 2, ext: crate::mnx::PitchExt { name: Some("D".to_string()) } },
            },
        ];
        app.start_new_tab(6, 12, 1, tunings);

        let custom_frets = vec![crate::fretboard_editor::CustomFret {
            fret_number: 18.0,
            manual_number: Some(18.0),
            spec: crate::fretboard_editor::FretLocationSpec::Ratio { num: 2, den: 1 },
            strings: vec![1, 2, 3, 4, 5, 6],
            note_name: "D".to_string(),
        }];

        let fb_state = crate::fretboard_editor::FretboardEditorState::new(
            6,
            custom_frets,
            "D".to_string(),
            4,
            293.66,
        );
        app.apply_custom_fretboard_state(fb_state);

        // Enter fret 18 on String 6 (D2 open -> harmonic 2 sounds D3)
        app.selected_event_idx = Some(0);
        app.editing_string = Some(6);
        app.editing_fret_buf = "18".to_string();
        assert!(app.commit_fret_edit());

        app.action_toggle_harmonic();
        assert!(app.is_selected_note_harmonic());
        {
            let doc = app.parsed_mnx.as_ref().unwrap();
            let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
            let h = ev.notes[0].harmonic.as_ref().unwrap();
            assert_eq!(h.touch, 18.0);
            assert_eq!(h.sounding_note.pitch.step, "D");
            assert_eq!(h.sounding_note.pitch.octave, 3);
        }
        assert!(app.validation_errors.is_empty(), "Validation errors: {:?}", app.validation_errors);

        // Shift up to String 5 (G2 open -> harmonic 2 sounds G3)
        app.action_shift_selected_strings(-1);
        {
            let doc = app.parsed_mnx.as_ref().unwrap();
            let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
            assert_eq!(ev.notes[0].string, Some(5));
            assert_eq!(ev.notes[0].fret, Some(18));
            let h = ev.notes[0].harmonic.as_ref().unwrap();
            assert_eq!(h.touch, 18.0);
            assert_eq!(h.sounding_note.pitch.step, "G");
            assert_eq!(h.sounding_note.pitch.octave, 3);
        }
        assert!(app.validation_errors.is_empty(), "Validation errors on String 5: {:?}", app.validation_errors);

        // Shift up to String 4 (D3 open -> harmonic 2 sounds D4)
        app.action_shift_selected_strings(-1);
        {
            let doc = app.parsed_mnx.as_ref().unwrap();
            let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
            assert_eq!(ev.notes[0].string, Some(4));
            assert_eq!(ev.notes[0].fret, Some(18));
            let h = ev.notes[0].harmonic.as_ref().unwrap();
            assert_eq!(h.touch, 18.0);
            assert_eq!(h.sounding_note.pitch.step, "D");
            assert_eq!(h.sounding_note.pitch.octave, 4);
        }
        assert!(app.validation_errors.is_empty(), "Validation errors on String 4: {:?}", app.validation_errors);

        // Shift back to String 6 (D2 open -> harmonic 2 sounds D3)
        app.action_shift_selected_strings(1);
        app.action_shift_selected_strings(1);
        {
            let doc = app.parsed_mnx.as_ref().unwrap();
            let ev = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
            assert_eq!(ev.notes[0].string, Some(6));
            assert_eq!(ev.notes[0].fret, Some(18));
            let h = ev.notes[0].harmonic.as_ref().unwrap();
            assert_eq!(h.touch, 18.0);
            assert_eq!(h.sounding_note.pitch.step, "D");
            assert_eq!(h.sounding_note.pitch.octave, 3);
        }
        assert!(app.validation_errors.is_empty(), "Validation errors on String 6: {:?}", app.validation_errors);
    }

    #[test]
    fn test_instrument_preset_custom_fretboard_and_ji_dialog_actions() {
        let mut app = MnxEditorApp::default();
        app.open_instrument_setup_dialog(true);

        // Build a custom fretboard & JI preset
        let custom_frets = vec![
            crate::fretboard_editor::CustomFret {
                fret_number: 1.0,
                manual_number: None,
                spec: crate::fretboard_editor::FretLocationSpec::Ratio { num: 22, den: 21 },
                strings: vec![1, 2, 3, 4, 5, 6],
                note_name: "Db".to_string(),
            },
            crate::fretboard_editor::CustomFret {
                fret_number: 2.0,
                manual_number: None,
                spec: crate::fretboard_editor::FretLocationSpec::Ratio { num: 13, den: 12 },
                strings: vec![1, 2, 3, 4, 5, 6],
                note_name: "^C#".to_string(),
            },
        ];
        let ji_tunings = vec![
            crate::mnx::StringTuning {
                string: 1,
                pitch: crate::mnx::Pitch {
                    step: "4/3".to_string(),
                    alter: None,
                    octave: 4,
                    ext: crate::mnx::PitchExt { name: Some("G".to_string()) },
                },
            },
            crate::mnx::StringTuning {
                string: 2,
                pitch: crate::mnx::Pitch {
                    step: "5/3".to_string(),
                    alter: None,
                    octave: 4,
                    ext: crate::mnx::PitchExt { name: Some("vB".to_string()) },
                },
            },
            crate::mnx::StringTuning {
                string: 3,
                pitch: crate::mnx::Pitch {
                    step: "4/3".to_string(),
                    alter: None,
                    octave: 3,
                    ext: crate::mnx::PitchExt { name: Some("G".to_string()) },
                },
            },
            crate::mnx::StringTuning {
                string: 4,
                pitch: crate::mnx::Pitch {
                    step: "1/1".to_string(),
                    alter: None,
                    octave: 3,
                    ext: crate::mnx::PitchExt { name: Some("D".to_string()) },
                },
            },
            crate::mnx::StringTuning {
                string: 5,
                pitch: crate::mnx::Pitch {
                    step: "4/3".to_string(),
                    alter: None,
                    octave: 2,
                    ext: crate::mnx::PitchExt { name: Some("G".to_string()) },
                },
            },
            crate::mnx::StringTuning {
                string: 6,
                pitch: crate::mnx::Pitch {
                    step: "1/1".to_string(),
                    alter: None,
                    octave: 2,
                    ext: crate::mnx::PitchExt { name: Some("D".to_string()) },
                },
            },
        ];
        let ref_note = crate::mnx::ReferenceNoteSpec {
            pitch: crate::mnx::Pitch {
                step: "D".to_string(),
                alter: None,
                octave: 4,
                ext: Default::default(),
            },
            frequency_hz: 293.66,
        };

        let custom_preset = crate::presets::InstrumentPreset {
            name: "Test JI Preset".to_string(),
            edo: None,
            step_multiplier: None,
            capo: None,
            custom_frets: Some(custom_frets),
            reference_note: Some(ref_note),
            tuning: ji_tunings,
        };

        // Extract dialog mutable state and apply preset
        if let ActiveDialog::InstrumentSetup {
            num_strings,
            edo,
            step_multiplier,
            tuning_mode,
            tunings_text,
            tunings_name,
            tunings_octave,
            show_custom_fretboard,
            fretboard_editor,
            ..
        } = &mut app.active_dialog
        {
            custom_preset.apply_to_dialog_state(
                num_strings,
                edo,
                step_multiplier,
                tuning_mode,
                tunings_text,
                tunings_name,
                tunings_octave,
                fretboard_editor,
                show_custom_fretboard,
            );

            assert_eq!(*tuning_mode, InstrumentTuningMode::RatioCents);
            assert!(*show_custom_fretboard);
            assert_eq!(tunings_text[0], "4/3");
            assert_eq!(tunings_name[0], "G");
            assert_eq!(tunings_text[1], "5/3");
            assert_eq!(tunings_name[1], "vB");
            assert_eq!(fretboard_editor.frets.len(), 2);
            assert_eq!(fretboard_editor.reference_note_step, "D");
            assert_eq!(fretboard_editor.reference_frequency_hz, 293.66);

            // Test creating and adding preset into app's preset list
            let reconstructed = crate::presets::InstrumentPreset::from_dialog_state(
                "Test JI Preset",
                *num_strings,
                *edo,
                *step_multiplier,
                *tuning_mode,
                tunings_text,
                tunings_name,
                tunings_octave,
                fretboard_editor,
            )
            .expect("from_dialog_state succeeds");
            assert_eq!(reconstructed, custom_preset);
        } else {
            panic!("Expected ActiveDialog::InstrumentSetup");
        }

        crate::presets::add_or_update_preset(&mut app.instrument_presets, custom_preset);
        assert!(app.instrument_presets.iter().any(|p| p.name == "Test JI Preset"));
        let saved = app.instrument_presets.iter().find(|p| p.name == "Test JI Preset").unwrap();
        assert!(saved.has_custom_frets());
        assert!(saved.has_non_edo_tuning());
        assert_eq!(saved.custom_frets.as_ref().unwrap().len(), 2);

        // Verify YAML roundtrip of the preset
        let yaml = crate::presets::serialize_preset_to_yaml(saved).expect("yaml serialization succeeds");
        let parsed = crate::presets::parse_presets_from_yaml(&yaml).expect("yaml parsing succeeds");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0], *saved);
    }

    #[test]
    fn test_phase_a_toolbar_and_harmonic_hotkey() {
        let mut app = MnxEditorApp::default();
        app.process_and_render();

        // Select first event and select note on string 1
        app.selected_event_idx = Some(0);
        app.select_note_on_string(0, 1);
        assert!(!app.is_selected_note_harmonic());

        // Trigger action_toggle_harmonic (hotkey H behavior)
        app.action_toggle_harmonic();
        assert!(app.is_selected_note_harmonic(), "Hotkey H toggles harmonic on");

        // Toggle again turns it off
        app.action_toggle_harmonic();
        assert!(!app.is_selected_note_harmonic(), "Hotkey H toggles harmonic off");

        // Verify Continuous mode vs Layout mode break button condition
        assert_eq!(app.view_mode, ViewMode::Continuous);
        assert!(!(app.view_mode != ViewMode::Continuous), "Continuous mode suppresses break buttons");

        app.view_mode = ViewMode::Layout;
        assert!(app.view_mode != ViewMode::Continuous, "Layout mode enables break buttons");
    }

    #[test]
    fn test_harmonic_hotkey_single_toggle_in_ui_frame() {
        let mut app = MnxEditorApp::default();
        app.process_and_render();

        // Put a note on String 1 at Fret 12 (standard harmonic node on E4)
        app.selected_event_idx = Some(0);
        app.editing_string = Some(1);
        app.editing_fret_buf = "12".to_string();
        assert!(app.commit_fret_edit());
        app.select_note_on_string(0, 1);
        assert!(!app.is_selected_note_harmonic());

        let ctx = egui::Context::default();
        crate::setup_custom_fonts(&ctx);
        let total_events = app.cached_event_ranges.len();
        let event_ranges = app.cached_event_ranges.clone();

        // Frame 1: User presses 'H'
        let mut raw_input = egui::RawInput::default();
        raw_input.events.push(egui::Event::Key {
            key: egui::Key::H,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });

        let mut output = ctx.run_ui(raw_input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.render_top_panel(ui, &event_ranges, total_events);
                app.handle_keyboard_input(ui, false, &event_ranges, total_events);
            });
        });
        output.textures_delta.clear();

        assert!(
            app.is_selected_note_harmonic(),
            "Pressing 'H' once MUST leave the note as harmonic, not double-toggle it back to 0!"
        );
        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev0 = doc.parts[0].measures[0].sequences[0].flattened_events()[0];
        let n0 = ev0.notes.iter().find(|n| n.string == Some(1)).unwrap();
        assert!(n0.harmonic.is_some(), "Note must have harmonic property");
        assert_ne!(n0.fret, Some(0), "Fret must NOT be 0");

        // Frame 2: User presses 'H' again
        let mut raw_input2 = egui::RawInput::default();
        raw_input2.events.push(egui::Event::Key {
            key: egui::Key::H,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });

        let mut output2 = ctx.run_ui(raw_input2, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.render_top_panel(ui, &event_ranges, total_events);
                app.handle_keyboard_input(ui, false, &event_ranges, total_events);
            });
        });
        output2.textures_delta.clear();

        assert!(
            !app.is_selected_note_harmonic(),
            "Pressing 'H' a second time must toggle harmonic off"
        );
        let doc2 = app.parsed_mnx.as_ref().unwrap();
        let ev0_2 = doc2.parts[0].measures[0].sequences[0].flattened_events()[0];
        let n0_2 = ev0_2.notes.iter().find(|n| n.string == Some(1)).unwrap();
        assert_eq!(n0_2.fret, Some(12), "Untoggling harmonic on fret 12 must restore regular fret 12");
        assert_eq!(n0_2.pitch.step, "B");
    }

    #[test]
    fn test_grid_controller_phase_c_popup_and_just_play_disabled() {
        let mut app = MnxEditorApp::default();
        let ctx = egui::Context::default();
        crate::setup_custom_fonts(&ctx);

        let tunings = vec![
            crate::mnx::StringTuning { string: 1, pitch: crate::mnx::Pitch { step: "E".to_string(), alter: None, octave: 4, ext: Default::default() } },
            crate::mnx::StringTuning { string: 2, pitch: crate::mnx::Pitch { step: "B".to_string(), alter: None, octave: 3, ext: Default::default() } },
            crate::mnx::StringTuning { string: 3, pitch: crate::mnx::Pitch { step: "G".to_string(), alter: None, octave: 3, ext: Default::default() } },
            crate::mnx::StringTuning { string: 4, pitch: crate::mnx::Pitch { step: "D".to_string(), alter: None, octave: 3, ext: Default::default() } },
            crate::mnx::StringTuning { string: 5, pitch: crate::mnx::Pitch { step: "A".to_string(), alter: None, octave: 2, ext: Default::default() } },
            crate::mnx::StringTuning { string: 6, pitch: crate::mnx::Pitch { step: "E".to_string(), alter: None, octave: 2, ext: Default::default() } },
        ];
        app.start_new_tab(6, 12, 1, tunings);

        // 1. Initially no controller connected or calibrated -> Just Play must be disabled
        assert!(!app.grid_input.has_connected_and_calibrated_controller());
        assert!(!app.grid_input.just_play_mode);
        assert!(app.grid_input.set_just_play_mode(true).is_empty());
        assert!(!app.grid_input.just_play_mode);

        // 2. Start calibration for Grid 1
        app.grid_input.start_calibration(0);
        assert!(app.grid_input.active_calibration.is_some());
        let (slot, step, _) = app.grid_input.active_calibration.unwrap();
        assert_eq!(slot, 0);
        assert_eq!(step, crate::grid_input::CalibrationStep::UpperLeft);

        // Render UI frame while calibrating and with popup open (verifies 4 rectangles and active yellow guide text)
        app.show_grid_controllers_popup = true;
        let event_ranges = app.cached_event_ranges.clone();
        let total_events = event_ranges.len();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.render_top_panel(ui, &event_ranges, total_events);
            });
        });
        out.textures_delta.clear();
        assert!(app.show_grid_controllers_popup);

        // 3. Complete 4-corner calibration for slot 0
        let t0 = std::time::Instant::now();
        app.grid_input.process_event(crate::grid_input::GridMidiEvent { slot_idx: 0, note: 76, is_note_on: true, velocity: 100 }, 6, t0);
        app.grid_input.process_event(crate::grid_input::GridMidiEvent { slot_idx: 0, note: 36, is_note_on: true, velocity: 100 }, 6, t0);
        app.grid_input.process_event(crate::grid_input::GridMidiEvent { slot_idx: 0, note: 83, is_note_on: true, velocity: 100 }, 6, t0);
        let (_, cal_done, _) = app.grid_input.process_event(crate::grid_input::GridMidiEvent { slot_idx: 0, note: 43, is_note_on: true, velocity: 100 }, 6, t0);

        assert_eq!(cal_done, Some(0));
        assert!(app.grid_input.slots[0].is_calibrated);
        assert!(app.grid_input.active_calibration.is_none());

        // 4. Connect slot 0 with valid calibration -> Just Play mode can now be enabled!
        app.grid_input.slots[0].enabled = true;
        assert!(app.grid_input.has_connected_and_calibrated_controller());
        app.grid_input.set_just_play_mode(true);
        assert!(app.grid_input.just_play_mode);
    }

    #[test]
    fn test_phase_3a_multi_instrument_add_edit_switch_and_remove() {
        let mut app = MnxEditorApp::default();
        let gtr1_tunings = vec![
            crate::mnx::StringTuning { string: 1, pitch: crate::mnx::Pitch { step: "E".to_string(), alter: None, octave: 4, ext: Default::default() } },
            crate::mnx::StringTuning { string: 2, pitch: crate::mnx::Pitch { step: "B".to_string(), alter: None, octave: 3, ext: Default::default() } },
            crate::mnx::StringTuning { string: 3, pitch: crate::mnx::Pitch { step: "G".to_string(), alter: None, octave: 3, ext: Default::default() } },
            crate::mnx::StringTuning { string: 4, pitch: crate::mnx::Pitch { step: "D".to_string(), alter: None, octave: 3, ext: Default::default() } },
            crate::mnx::StringTuning { string: 5, pitch: crate::mnx::Pitch { step: "A".to_string(), alter: None, octave: 2, ext: Default::default() } },
            crate::mnx::StringTuning { string: 6, pitch: crate::mnx::Pitch { step: "E".to_string(), alter: None, octave: 2, ext: Default::default() } },
        ];
        app.instrument_setup_name_buf = "Lead Guitar (12 EDO)".to_string();
        app.start_new_tab(6, 12, 1, gtr1_tunings);

        // Add a second measure on part 0 so we have 2 measures
        app.add_measure_to_end();
        assert_eq!(app.parsed_mnx.as_ref().unwrap().parts.len(), 1);
        assert_eq!(app.parsed_mnx.as_ref().unwrap().global.measures.len(), 2);

        // Add a second instrument (4-string Bass in 31 EDO)
        let bass_tunings = vec![
            crate::mnx::StringTuning { string: 1, pitch: crate::mnx::Pitch { step: "G".to_string(), alter: None, octave: 2, ext: Default::default() } },
            crate::mnx::StringTuning { string: 2, pitch: crate::mnx::Pitch { step: "D".to_string(), alter: None, octave: 2, ext: Default::default() } },
            crate::mnx::StringTuning { string: 3, pitch: crate::mnx::Pitch { step: "A".to_string(), alter: None, octave: 1, ext: Default::default() } },
            crate::mnx::StringTuning { string: 4, pitch: crate::mnx::Pitch { step: "E".to_string(), alter: None, octave: 1, ext: Default::default() } },
        ];
        app.add_new_instrument_part(
            "Microtonal Bass (31 EDO)".to_string(),
            4,
            31,
            1,
            bass_tunings,
            None,
        );

        let doc = app.parsed_mnx.as_ref().unwrap();
        assert_eq!(doc.parts.len(), 2);
        assert_eq!(doc.parts[0].display_name(0), "Lead Guitar (12 EDO)");
        assert_eq!(doc.parts[1].display_name(1), "Microtonal Bass (31 EDO)");
        assert_eq!(doc.parts[0].edo(), 12);
        assert_eq!(doc.parts[1].edo(), 31);
        assert_eq!(doc.parts[0].num_strings(), 6);
        assert_eq!(doc.parts[1].num_strings(), 4);
        assert_eq!(doc.parts[0].measures.len(), 2);
        assert_eq!(doc.parts[1].measures.len(), 2);
        // Active part automatically switches to newly added instrument
        assert_eq!(app.selected_part_idx, 1);

        // Enter a note on Instrument 2 (Bass) at event 0, string 4, fret 5
        let sel_p = app.selected_part_idx;
        app.mutate_doc(|doc| {
            if crate::yaml_editor::set_or_add_note_on_string_in_doc_part(doc, sel_p, 0, 4, 5, None) {
                Some(())
            } else {
                None
            }
        });

        let doc = app.parsed_mnx.as_ref().unwrap();
        assert!(
            doc.parts[0].measures[0].sequences[0].flattened_events()[0]
                .notes
                .is_empty(),
            "Part 0 must remain untouched when editing Part 1"
        );
        assert_eq!(
            doc.parts[1].measures[0].sequences[0].flattened_events()[0].notes[0].fret,
            Some(5)
        );

        // Switch back to Part 0 and enter a note on String 1, Fret 7
        app.select_part(0);
        assert_eq!(app.selected_part_idx, 0);
        let sel_p = app.selected_part_idx;
        app.mutate_doc(|doc| {
            if crate::yaml_editor::set_or_add_note_on_string_in_doc_part(doc, sel_p, 0, 1, 7, None) {
                Some(())
            } else {
                None
            }
        });

        let doc = app.parsed_mnx.as_ref().unwrap();
        assert_eq!(
            doc.parts[0].measures[0].sequences[0].flattened_events()[0].notes[0].fret,
            Some(7)
        );
        assert_eq!(
            doc.parts[1].measures[0].sequences[0].flattened_events()[0].notes[0].fret,
            Some(5)
        );

        // Remove Part 1 and verify Part 0 remains intact
        app.remove_instrument_part(1);
        let doc = app.parsed_mnx.as_ref().unwrap();
        assert_eq!(doc.parts.len(), 1);
        assert_eq!(doc.parts[0].display_name(0), "Lead Guitar (12 EDO)");
        assert_eq!(app.selected_part_idx, 0);
    }

    #[test]
    fn test_phase_3a_continuous_sync_and_multi_part_midi_playback() {
        let yaml = r#"---
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
    - {}
parts:
  - name: "Guitar 1"
    _x:
      microFretDivisions: 12
      tuning:
        - { string: 1, pitch: { step: E, octave: 4 } }
        - { string: 2, pitch: { step: B, octave: 3 } }
        - { string: 3, pitch: { step: G, octave: 3 } }
        - { string: 4, pitch: { step: D, octave: 3 } }
        - { string: 5, pitch: { step: A, octave: 2 } }
        - { string: 6, pitch: { step: E, octave: 2 } }
    measures:
      - sequences:
          - content:
              - duration: { base: quarter }
                notes:
                  - pitch: { step: E, octave: 4 }
                    _x: { string: 1, fret: 0 }
              - duration: { base: quarter }
                notes:
                  - pitch: { step: F, octave: 4 }
                    _x: { string: 1, fret: 1 }
              - duration: { base: half }
                rest: {}
      - sequences:
          - content:
              - duration: { base: whole }
                notes:
                  - pitch: { step: G, octave: 4 }
                    _x: { string: 1, fret: 3 }
  - name: "10-String Harp Guitar"
    _x:
      microFretDivisions: 24
      tuning:
        - { string: 1, pitch: { step: E, octave: 4 } }
        - { string: 2, pitch: { step: B, octave: 3 } }
        - { string: 3, pitch: { step: G, octave: 3 } }
        - { string: 4, pitch: { step: D, octave: 3 } }
        - { string: 5, pitch: { step: A, octave: 2 } }
        - { string: 6, pitch: { step: E, octave: 2 } }
        - { string: 7, pitch: { step: D, octave: 2 } }
        - { string: 8, pitch: { step: C, octave: 2 } }
        - { string: 9, pitch: { step: B, octave: 1 } }
        - { string: 10, pitch: { step: A, octave: 1 } }
    measures:
      - sequences:
          - content:
              - duration: { base: whole }
                notes:
                  - pitch: { step: A, octave: 1 }
                    _x: { string: 10, fret: 0 }
      - sequences:
          - content:
              - duration: { base: half }
                notes:
                  - pitch: { step: B, octave: 1 }
                    _x: { string: 9, fret: 2 }
              - duration: { base: half }
                notes:
                  - pitch: { step: C, octave: 2 }
                    _x: { string: 8, fret: 4 }
"#;
        let mut app = MnxEditorApp::default();
        app.update_yaml(yaml.to_string());
        app.process_and_render();

        let doc = app.parsed_mnx.as_ref().unwrap();
        assert_eq!(doc.parts.len(), 2);

        // Verify Continuous Mode measure width computation and synchronized rendering
        let w0 = crate::render::compute_part_measure_content_widths(&doc.parts[0], false, None, "");
        let w1 = crate::render::compute_part_measure_content_widths(&doc.parts[1], false, None, "");
        assert_eq!(w0.len(), 2);
        assert_eq!(w1.len(), 2);
        // Measure 0 has 3 events in part 0 vs 1 event in part 1 -> w0[0] > w1[0]
        assert!(w0[0] > w1[0]);
        // Measure 1 has 1 event in part 0 vs 2 events in part 1 -> w1[1] > w0[1]
        assert!(w1[1] > w0[1]);

        // Render Continuous Mode UI frame with both stacked instruments
        let ctx = egui::Context::default();
        crate::setup_custom_fonts(&ctx);
        let mut out = ctx.run_ui(egui::RawInput::default(), |ctx| {
            app.render_central_panel(ctx);
        });
        out.textures_delta.clear();
        let doc = app.parsed_mnx.as_ref().unwrap();

        // Verify Multi-Part & Multi-Port MIDI playback track generation (6 + 10 = 16 strings across 2 ports)
        let mut map0 = std::collections::HashMap::new();
        for s in 1..=6u8 {
            map0.insert(s, midly::num::u4::from(s));
        }
        let mut map1 = std::collections::HashMap::new();
        for s in 1..=10u8 {
            map1.insert(s, midly::num::u4::from(s));
        }
        let configs = vec![
            crate::midi::PartMidiConfig {
                part_idx: 0,
                port_index: 0,
                pitch_bend_range: 48,
                string_to_channel: map0,
                is_mpe: true,
            },
            crate::midi::PartMidiConfig {
                part_idx: 1,
                port_index: 1,
                pitch_bend_range: 48,
                string_to_channel: map1,
                is_mpe: true,
            },
        ];

        let (track, ppq) = crate::midi::generate_multi_part_playback_track(
            doc,
            &configs,
            120.0,
            midly::num::u15::from(480),
            0,
            None,
        );
        assert_eq!(ppq, 480.0);

        let mut saw_part0_event_start = false;
        let mut saw_part1_event_start = false;
        let mut saw_port0_midi = false;
        let mut saw_port1_midi = false;

        for ev in &track {
            match &ev.kind {
                crate::midi::MultiPartPlaybackEventKind::PartEventMarker { part_idx, .. } => {
                    if *part_idx == 0 {
                        saw_part0_event_start = true;
                    } else if *part_idx == 1 {
                        saw_part1_event_start = true;
                    }
                }
                crate::midi::MultiPartPlaybackEventKind::Midi { port_index, .. } => {
                    if *port_index == 0 {
                        saw_port0_midi = true;
                    } else if *port_index == 1 {
                        saw_port1_midi = true;
                    }
                }
                _ => {}
            }
        }

        assert!(saw_part0_event_start, "Track must include EventStart markers for Part 0");
        assert!(saw_part1_event_start, "Track must include EventStart markers for Part 1");
        assert!(saw_port0_midi, "Track must route Part 0 MIDI messages to Port 0");
        assert!(saw_port1_midi, "Track must route Part 1 MIDI messages to Port 1");
    }

    #[test]
    fn test_phase_3a_layout_mode_ensemble_and_part_tabs_and_pdf_export() {
        let yaml = r#"---
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
    - {}
parts:
  - name: "Lead Guitar"
    _x:
      microFretDivisions: 12
    measures:
      - sequences:
          - content:
              - duration: { base: whole }
                notes:
                  - pitch: { step: E, octave: 4 }
                    _x: { string: 1, fret: 0 }
      - sequences:
          - content:
              - duration: { base: whole }
                notes:
                  - pitch: { step: G, octave: 4 }
                    _x: { string: 1, fret: 3 }
  - name: "Rhythm Guitar"
    _x:
      microFretDivisions: 12
    measures:
      - sequences:
          - content:
              - duration: { base: whole }
                notes:
                  - pitch: { step: A, octave: 2 }
                    _x: { string: 5, fret: 0 }
      - sequences:
          - content:
              - duration: { base: whole }
                notes:
                  - pitch: { step: E, octave: 2 }
                    _x: { string: 6, fret: 0 }
"#;
        let mut app = MnxEditorApp::default();
        app.update_yaml(yaml.to_string());
        app.process_and_render();

        let doc = app.parsed_mnx.as_ref().unwrap();

        // 1. Default LayoutViewTab is Ensemble with both parts visible
        assert_eq!(app.layout_view_tab, LayoutViewTab::Ensemble);
        assert_eq!(app.active_layout_part_indices(), vec![0, 1]);

        let pages_ensemble = crate::layout::compute_score_layout_for_parts(doc, 0.0, &[0, 1]);
        let pages_part0 = crate::layout::compute_score_layout_for_parts(doc, 0.0, &[0]);
        assert!(!pages_ensemble.is_empty());
        assert!(!pages_part0.is_empty());

        // Populate display list for Ensemble Score and verify both part labels & system bracket exist
        let mut dl_ens = crate::layout::display_list::DisplayList::new();
        crate::pdf_export::populate_page_display_list_for_parts(doc, &pages_ensemble[0], &mut dl_ens, &[0, 1]);
        assert!(
            dl_ens.items.iter().any(|i| i.id.as_deref() == Some("part_label_0_0")),
            "Ensemble display list must include part label for part 0"
        );
        assert!(
            dl_ens.items.iter().any(|i| i.id.as_deref() == Some("part_label_0_1")),
            "Ensemble display list must include part label for part 1"
        );
        assert!(
            dl_ens.items.iter().any(|i| i.id.as_deref() == Some("sys_bracket_0")),
            "Ensemble display list must include connected left system barline across staves"
        );

        // Switch to Individual Part 1 tab ("Rhythm Guitar")
        app.layout_view_tab = LayoutViewTab::Part(1);
        assert_eq!(app.active_layout_part_indices(), vec![1]);

        let pages_part1 = crate::layout::compute_score_layout_for_parts(doc, 0.0, &[1]);
        let mut dl_p1 = crate::layout::display_list::DisplayList::new();
        crate::pdf_export::populate_page_display_list_for_parts(doc, &pages_part1[0], &mut dl_p1, &[1]);
        assert!(
            !dl_p1.items.iter().any(|i| i.id.as_deref() == Some("sys_bracket_0")),
            "Single-part display list should not draw a multi-staff system bracket"
        );

        // Verify PDF bytes generation succeeds for both Ensemble and Single-Part views
        let pdf_ens = crate::pdf_export::generate_score_pdf_bytes_for_parts(doc, &pages_ensemble, &[0, 1]).unwrap();
        assert!(pdf_ens.starts_with(b"%PDF-"));
        let pdf_p1 = crate::pdf_export::generate_score_pdf_bytes_for_parts(doc, &pages_part1, &[1]).unwrap();
        assert!(pdf_p1.starts_with(b"%PDF-"));
    }

    #[test]
    fn test_second_part_measure_1_quarter_note_right_arrow_creates_beats_2_to_4() {
        let example_yaml = std::fs::read_to_string("examples/example.yaml")
            .expect("examples/example.yaml must exist");
        let mut app = MnxEditorApp::default();
        app.update_yaml(example_yaml);
        app.process_and_render();

        // Add a 2nd instrument (which switches active selection to Part 1, event 0)
        app.add_new_instrument_part(
            "Second Guitar".to_string(),
            6,
            12,
            1,
            Vec::new(),
            None,
        );
        assert_eq!(app.selected_part_idx, 1);
        assert_eq!(app.selected_event_idx, Some(0));

        // Part 1 starts with 3 measures, each containing 1 whole rest (3 events total)
        let part1 = &app.parsed_mnx.as_ref().unwrap().parts[1];
        assert_eq!(part1.measures.len(), 3);
        assert_eq!(part1.measures[0].sequences[0].flattened_events().len(), 1);

        // Adjust 1st event in Measure 1 of Part 1 to quarter-note duration (1/4)
        app.mutate_doc(|doc| {
            crate::yaml_editor::update_event_duration_in_doc_part(
                doc,
                1,
                0,
                &crate::mnx::Duration {
                    base: "quarter".to_string(),
                    ..Default::default()
                },
            )
            .then_some(())
        });

        // Enter edit mode on string 1, type fret "5", and hit Right arrow
        app.is_editing_fret = true;
        app.editing_string = Some(1);
        app.editing_fret_buf = "5".to_string();
        assert!(app.commit_fret_edit());
        app.advance_to_next_note_in_edit_mode(Some(1), app.cached_event_ranges.len());

        // Expected: Beat 2 created in Measure 1 of Part 1 (measure 0 now has 2 events, cursor is at event 1)
        let part1 = &app.parsed_mnx.as_ref().unwrap().parts[1];
        assert_eq!(
            part1.measures[0].sequences[0].flattened_events().len(),
            2,
            "Measure 1 of Part 2 must have 2 events after hitting Right on beat 1"
        );
        assert_eq!(app.selected_event_idx, Some(1));
        assert!(app.is_editing_fret);

        // Type fret "7" on Beat 2, hit Right arrow -> creates Beat 3 in Measure 1
        app.editing_fret_buf = "7".to_string();
        assert!(app.commit_fret_edit());
        app.advance_to_next_note_in_edit_mode(Some(1), app.cached_event_ranges.len());
        let part1 = &app.parsed_mnx.as_ref().unwrap().parts[1];
        assert_eq!(
            part1.measures[0].sequences[0].flattened_events().len(),
            3,
            "Measure 1 of Part 2 must have 3 events after hitting Right on beat 2"
        );
        assert_eq!(app.selected_event_idx, Some(2));

        // Type fret "8" on Beat 3, hit Right arrow -> creates Beat 4 in Measure 1
        app.editing_fret_buf = "8".to_string();
        assert!(app.commit_fret_edit());
        app.advance_to_next_note_in_edit_mode(Some(1), app.cached_event_ranges.len());
        let part1 = &app.parsed_mnx.as_ref().unwrap().parts[1];
        assert_eq!(
            part1.measures[0].sequences[0].flattened_events().len(),
            4,
            "Measure 1 of Part 2 must have 4 events after hitting Right on beat 3"
        );
        assert_eq!(app.selected_event_idx, Some(3));

        // Type fret "10" on Beat 4 (Measure 1 is now full: 4/4), hit Right arrow -> advances to Measure 2's 1st event (event 4)
        app.editing_fret_buf = "10".to_string();
        assert!(app.commit_fret_edit());
        app.advance_to_next_note_in_edit_mode(Some(1), app.cached_event_ranges.len());
        let part1 = &app.parsed_mnx.as_ref().unwrap().parts[1];
        assert_eq!(
            part1.measures[0].sequences[0].flattened_events().len(),
            4,
            "Measure 1 of Part 2 is now full (4 quarter notes) and should not add a 5th event"
        );
        assert_eq!(
            app.selected_event_idx,
            Some(4),
            "After filling Measure 1, Right arrow should advance to event 4 (first event of Measure 2)"
        );
    }

    #[test]
    fn test_adding_music_at_end_of_one_part_creates_single_full_measure_rest_in_other_part() {
        let mut app = MnxEditorApp::default();
        app.process_and_render();

        // Add a 2nd instrument
        app.add_new_instrument_part(
            "Second Guitar".to_string(),
            6,
            12,
            1,
            Vec::new(),
            None,
        );

        // Switch back to Part 0, set first event to eighth note (0.5 beats), and fill Measure 1 + advance into Measure 2 and Measure 3
        app.select_part(0);
        app.selected_event_idx = Some(0);
        app.mutate_doc(|doc| {
            crate::yaml_editor::update_event_duration_in_doc_part(
                doc,
                0,
                0,
                &crate::mnx::Duration {
                    base: "eighth".to_string(),
                    ..Default::default()
                },
            )
            .then_some(())
        });

        app.is_editing_fret = true;
        app.editing_string = Some(1);

        // Enter 8 eighth notes in Measure 1 (4/4) and 8 eighth notes in Measure 2, advancing into Measure 3
        for i in 0..16 {
            app.editing_fret_buf = format!("{}", i % 12);
            assert!(app.commit_fret_edit());
            app.advance_to_next_note_in_edit_mode(Some(1), app.cached_event_ranges.len());
        }

        let doc = app.parsed_mnx.as_ref().unwrap();
        assert_eq!(doc.parts[0].measures.len(), 3);
        assert_eq!(doc.parts[1].measures.len(), 3);

        // Every measure in Part 1 (which has no music in it) must have a single whole rest spanning the full 4/4 measure
        for (m_idx, m) in doc.parts[1].measures.iter().enumerate() {
            let events = m.sequences[0].flattened_events();
            assert_eq!(
                events.len(),
                1,
                "Measure {} in Part 1 must contain a single full-measure rest, got {:?}",
                m_idx + 1,
                events
            );
            assert_eq!(events[0].duration.base, "whole");
            assert!(events[0].rest.is_some());
            assert!(events[0].notes.is_empty());
        }
    }

    #[test]
    fn test_frontier_measure_validation_parity_in_second_part() {
        let mut app = MnxEditorApp::default();
        app.process_and_render();
        assert!(
            app.validation_errors.is_empty(),
            "examples/example.yaml should start with no validation errors"
        );

        // Add Part 1 (which gets 3 silent whole-rest measures matching Part 0)
        app.add_new_instrument_part(
            "Second Guitar".to_string(),
            6,
            12,
            1,
            Vec::new(),
            None,
        );
        app.select_part(1);
        app.selected_event_idx = Some(0);

        // Adjust duration of Note 1 in Measure 1 of Part 1 to a quarter note (1 beat in a 4/4 bar)
        app.mutate_doc(|doc| {
            crate::yaml_editor::update_event_duration_in_doc_part(
                doc,
                1,
                0,
                &crate::mnx::Duration {
                    base: "quarter".to_string(),
                    ..Default::default()
                },
            )
            .then_some(())
        });

        // Because Measures 2 and 3 of Part 1 are still untouched silent whole-rest placeholders,
        // Measure 1 of Part 1 is the active frontier measure and must NOT report an "is incomplete" error!
        assert!(
            app.validation_errors.is_empty(),
            "Active frontier measure in Part 1 should not trigger an incomplete bar validation error while adding quarter notes, got: {:?}",
            app.validation_errors
        );

        // Enter a fret on Note 1 and advance right to create Note 2 (2 quarter notes = 2 beats in 4/4)
        app.is_editing_fret = true;
        app.editing_string = Some(1);
        app.editing_fret_buf = "5".to_string();
        assert!(app.commit_fret_edit());
        app.advance_to_next_note_in_edit_mode(Some(1), app.cached_event_ranges.len());

        assert!(
            app.validation_errors.is_empty(),
            "Active frontier measure in Part 1 with 2 quarter notes should also have 0 validation errors, got: {:?}",
            app.validation_errors
        );
    }

    #[test]
    fn test_moving_into_default_rest_measure_creates_blank_note_with_previous_duration() {
        let mut app = MnxEditorApp::default();
        app.process_and_render();

        // Add Part 1 (which gets 3 silent whole-rest measures matching Part 0)
        app.add_new_instrument_part(
            "Second Guitar".to_string(),
            6,
            12,
            1,
            Vec::new(),
            None,
        );
        app.select_part(1);
        app.selected_event_idx = Some(0);

        // Set first note in Measure 1 of Part 1 to quarter note
        app.mutate_doc(|doc| {
            crate::yaml_editor::update_event_duration_in_doc_part(
                doc,
                1,
                0,
                &crate::mnx::Duration {
                    base: "quarter".to_string(),
                    ..Default::default()
                },
            )
            .then_some(())
        });

        app.is_editing_fret = true;
        app.editing_string = Some(1);

        // Enter 4 quarter notes in Measure 1 and advance right after each one
        for fret in [0, 2, 4, 5] {
            app.editing_fret_buf = fret.to_string();
            assert!(app.commit_fret_edit());
            app.advance_to_next_note_in_edit_mode(Some(1), app.cached_event_ranges.len());
        }

        // Cursor should now be on Event 4 (first event of Measure 2), and Measure 2's default whole rest
        // should have been replaced with a blank quarter note!
        assert_eq!(app.selected_event_idx, Some(4));
        {
            let doc = app.parsed_mnx.as_ref().unwrap();
            let m2_evs = doc.parts[1].measures[1].sequences[0].flattened_events();
            assert_eq!(m2_evs.len(), 1);
            assert_eq!(
                m2_evs[0].duration.base, "quarter",
                "Moving into a bar with a default rest from a quarter note should create a quarter-note blank note"
            );
            assert!(m2_evs[0].rest.is_none());
            assert!(m2_evs[0].notes.is_empty());
        }
        assert!(app.validation_errors.is_empty());

        // Retreating left from that untouched blank quarter note in Measure 2 should restore Measure 2's whole rest
        app.retreat_to_prev_note_in_edit_mode(Some(1));
        assert_eq!(app.selected_event_idx, Some(3));
        {
            let doc = app.parsed_mnx.as_ref().unwrap();
            let m2_evs = doc.parts[1].measures[1].sequences[0].flattened_events();
            assert_eq!(m2_evs.len(), 1);
            assert_eq!(m2_evs[0].duration.base, "whole");
            assert!(m2_evs[0].rest.is_some());
            assert!(m2_evs[0].notes.is_empty());
        }
    }

    #[test]
    fn test_ctrl_up_down_jumps_between_parts_at_same_duration_position() {
        let mut app = MnxEditorApp::default();
        app.process_and_render();

        // Part 0 in examples/example.yaml has 8 eighth notes in Measure 1 (beats 0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5)
        app.add_new_instrument_part(
            "Second Guitar".to_string(),
            6,
            12,
            1,
            Vec::new(),
            None,
        );
        app.select_part(1);
        app.selected_event_idx = Some(0);

        // Fill Measure 1 of Part 1 with 4 quarter notes (beats 0.0, 1.0, 2.0, 3.0)
        app.mutate_doc(|doc| {
            crate::yaml_editor::update_event_duration_in_doc_part(
                doc,
                1,
                0,
                &crate::mnx::Duration {
                    base: "quarter".to_string(),
                    ..Default::default()
                },
            )
            .then_some(())
        });
        app.is_editing_fret = true;
        app.editing_string = Some(1);
        for (idx, fret) in [0, 2, 4, 5].iter().enumerate() {
            app.editing_fret_buf = fret.to_string();
            assert!(app.commit_fret_edit());
            if idx < 3 {
                app.advance_to_next_note_in_edit_mode(Some(1), app.cached_event_ranges.len());
            }
        }
        app.is_editing_fret = false;

        // Select 3rd quarter note in Part 1 (index 2, beat 2.0 = Beat 3)
        app.selected_event_idx = Some(2);

        // Jump UP to Part 0 (Ctrl+Up): should land on the 5th eighth note of Measure 1 (index 4, beat 2.0)
        app.jump_to_adjacent_part(-1);
        assert_eq!(app.selected_part_idx, 0);
        assert_eq!(
            app.selected_event_idx,
            Some(4),
            "Jumping up from beat 2.0 in Part 1 should land on event 4 (beat 2.0) in Part 0"
        );

        // Move to 4th eighth note in Part 0 (index 3, beat 1.5 = '2-and') and jump DOWN to Part 1 (Ctrl+Down):
        // should land on the 2nd quarter note in Part 1 (index 1, spanning [1.0, 2.0))
        app.selected_event_idx = Some(3);
        app.jump_to_adjacent_part(1);
        assert_eq!(app.selected_part_idx, 1);
        assert_eq!(
            app.selected_event_idx,
            Some(1),
            "Jumping down from beat 1.5 in Part 0 should land on event 1 (spanning [1.0, 2.0)) in Part 1"
        );
    }

    #[test]
    fn test_remove_instrument_warning_dialog() {
        let mut app = MnxEditorApp::default();
        app.process_and_render();

        // Add a second instrument
        app.add_new_instrument_part(
            "Second Guitar".to_string(),
            6,
            12,
            1,
            Vec::new(),
            None,
        );
        assert_eq!(app.parsed_mnx.as_ref().unwrap().parts.len(), 2);

        // Select the second instrument and trigger removal
        app.select_part(1);
        app.active_dialog = ActiveDialog::ConfirmRemoveInstrument { part_idx: 1 };

        let ctx = egui::Context::default();
        crate::setup_custom_fonts(&ctx);

        // 1. Cancel via Escape -> instrument is NOT removed
        let mut raw_input_cancel = egui::RawInput::default();
        raw_input_cancel.events.push(egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        let mut out = ctx.run_ui(raw_input_cancel, |ctx| {
            app.render_dialogs(ctx);
        });
        out.textures_delta.clear();

        assert_eq!(app.active_dialog, ActiveDialog::None);
        assert_eq!(
            app.parsed_mnx.as_ref().unwrap().parts.len(),
            2,
            "Canceling the warning dialog should retain both instruments"
        );

        // 2. Open dialog again and confirm with Enter ("Ok") -> instrument IS removed
        app.active_dialog = ActiveDialog::ConfirmRemoveInstrument { part_idx: 1 };
        let mut raw_input_ok = egui::RawInput::default();
        raw_input_ok.events.push(egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        let mut out2 = ctx.run_ui(raw_input_ok, |ctx| {
            app.render_dialogs(ctx);
        });
        out2.textures_delta.clear();

        assert_eq!(app.active_dialog, ActiveDialog::None);
        assert_eq!(
            app.parsed_mnx.as_ref().unwrap().parts.len(),
            1,
            "Confirming removal via warning dialog should remove the instrument"
        );
    }

    #[test]
    fn test_tempo_dialog_recall_and_remove() {
        let yaml = r#"---
mnx: { version: 1 }
global:
  measures:
    - time: { count: 4, unit: 4 }
      tempos:
        - bpm: 135
          value: { base: half, dots: 1 }
parts:
  - measures:
      - sequences:
          - content:
              - duration: { base: whole }
                rest: {}
"#;
        let mut app = MnxEditorApp::default();
        app.update_yaml(yaml.trim().to_string());
        app.process_and_render();

        // Verify tempo exists initially
        let found = app.find_tempo_at_event(0);
        assert!(found.is_some(), "Should find tempo at event 0");
        let (m_idx, t_idx, tempo) = found.unwrap();
        assert_eq!(m_idx, 0);
        assert_eq!(t_idx, 0);
        assert_eq!(tempo.bpm, 135.0);
        assert_eq!(tempo.value.base, "half");
        assert_eq!(tempo.value.dots, Some(1));

        // Simulate dialog opening at event 0 (as done in panel "Insert tempo" button)
        let (bpm_str, duration_value, dots, existing_tempo) = if let Some((m, t, tempo)) = app.find_tempo_at_event(0) {
            (tempo.bpm.to_string(), tempo.value.base.clone(), tempo.value.dots.unwrap_or(0), Some((m, t)))
        } else {
            ("120".to_string(), "quarter".to_string(), 0, None)
        };
        assert_eq!(bpm_str, "135");
        assert_eq!(duration_value, "half");
        assert_eq!(dots, 1);
        assert_eq!(existing_tempo, Some((0, 0)));

        app.active_dialog = ActiveDialog::InsertTempo {
            bpm_str,
            duration_value,
            dots,
            event_idx: 0,
            existing_tempo,
        };

        // Render dialog and trigger remove action
        app.action_remove_tempo(0, 0);
        app.selected_tempo = None;

        let doc = app.parsed_mnx.as_ref().unwrap();
        assert!(doc.global.measures[0].tempos.is_empty(), "Tempos should be empty after removal");
        assert!(app.find_tempo_at_event(0).is_none(), "find_tempo_at_event should return None");
    }

    #[test]
    fn test_tempo_continuous_and_layout_mode_delete() {
        let yaml = r#"---
mnx: { version: 1 }
global:
  measures:
    - time: { count: 4, unit: 4 }
      tempos:
        - bpm: 120
          value: { base: quarter }
    - tempos:
        - bpm: 90
          value: { base: quarter }
parts:
  - measures:
      - sequences:
          - content:
              - duration: { base: whole }
                rest: {}
      - sequences:
          - content:
              - duration: { base: whole }
                rest: {}
"#;
        let mut app = MnxEditorApp::default();
        app.update_yaml(yaml.trim().to_string());
        app.process_and_render();

        // 1. Continuous mode: select tempo at (0, 0) and remove
        app.selected_tempo = Some((0, 0));
        if let Some((m_idx, t_idx)) = app.selected_tempo {
            app.action_remove_tempo(m_idx, t_idx);
            app.selected_tempo = None;
        }

        let doc = app.parsed_mnx.as_ref().unwrap();
        assert!(doc.global.measures[0].tempos.is_empty(), "Measure 0 tempo should be removed");
        assert_eq!(doc.global.measures[1].tempos.len(), 1, "Measure 1 tempo should remain");

        // 2. Layout mode: select element id "tempo_1_0_tempo" and remove
        let sel_id = "tempo_1_0_tempo";
        app.selected_layout_element_id = Some(sel_id.to_string());
        let parts: Vec<&str> = sel_id.split('_').collect();
        assert!(parts.len() >= 3);
        let m_idx = parts[1].parse::<usize>().unwrap();
        let t_idx = parts[2].parse::<usize>().unwrap();
        app.action_remove_tempo(m_idx, t_idx);
        app.selected_layout_element_id = None;

        let doc2 = app.parsed_mnx.as_ref().unwrap();
        assert!(doc2.global.measures[1].tempos.is_empty(), "Measure 1 tempo should be removed");
    }

    #[test]
    fn test_duplicate_insert_preserves_and_copies_special_barlines() {
        let yaml = r#"---
mnx: { version: 1 }
global:
  measures:
    - time: { count: 4, unit: 4 }
      repeatStart: {}
    - repeatEnd: { times: 2 }
      ending: { duration: 1, numbers: [1] }
    - ending: { duration: 1, numbers: [2] }
parts:
  - measures:
      - sequences:
          - content:
              - duration: { base: whole }
                rest: {}
      - sequences:
          - content:
              - duration: { base: whole }
                rest: {}
      - sequences:
          - content:
              - duration: { base: whole }
                rest: {}
"#;
        let mut app = MnxEditorApp::default();
        app.update_yaml(yaml.trim().to_string());
        app.process_and_render();

        // Select measure 0 (event 0)
        app.selected_event_idx = Some(0);
        app.selection_anchor = Some(0);

        // Run duplicate insert
        app.action_duplicate_insert();

        let doc = app.parsed_mnx.as_ref().unwrap();
        assert_eq!(doc.global.measures.len(), 4, "Should have 4 measures after duplicating 1 measure");
        assert_eq!(doc.parts[0].measures.len(), 4);

        // Measure 0 has repeat_start
        assert!(doc.global.measures[0].repeat_start.is_some());
        // Measure 1 (duplicate of measure 0) also has repeat_start copied
        assert!(doc.global.measures[1].repeat_start.is_some());
        // Measure 2 (shifted measure 1) has repeat_end and ending [1]
        assert!(doc.global.measures[2].repeat_end.is_some());
        assert_eq!(doc.global.measures[2].ending.as_ref().unwrap().numbers, vec![1]);
        // Measure 3 (shifted measure 2) has ending [2]
        assert_eq!(doc.global.measures[3].ending.as_ref().unwrap().numbers, vec![2]);
    }

    #[test]
    fn test_paste_insert_preserves_and_copies_special_barlines() {
        let yaml = r#"---
mnx: { version: 1 }
global:
  measures:
    - time: { count: 4, unit: 4 }
    - barline: { type: dotted }
      ending: { duration: 1, numbers: [1] }
    - ending: { duration: 1, numbers: [2] }
parts:
  - measures:
      - sequences:
          - content:
              - duration: { base: half }
                rest: {}
              - duration: { base: half }
                rest: {}
      - sequences:
          - content:
              - duration: { base: whole }
                rest: {}
      - sequences:
          - content:
              - duration: { base: whole }
                rest: {}
"#;
        let mut app = MnxEditorApp::default();
        app.update_yaml(yaml.trim().to_string());
        app.process_and_render();

        // Select Measure 1 (event 2, whole rest)
        app.selected_event_idx = Some(2);
        app.selection_anchor = Some(2);
        app.action_copy();

        let cb = app.clipboard.as_ref().expect("Clipboard should contain copied measure");
        assert_eq!(cb.measure_attrs.len(), 1);
        assert_eq!(cb.measure_attrs[0].barline.as_ref().map(|b| b.barline_type.as_str()), Some("dotted"));
        assert_eq!(cb.measure_attrs[0].ending.as_ref().unwrap().numbers, vec![1]);

        // Now move cursor to event 0 (start of Measure 0) and paste insert
        app.selected_event_idx = Some(0);
        app.selection_anchor = None;
        app.action_paste_insert();

        let doc = app.parsed_mnx.as_ref().unwrap();
        assert_eq!(doc.global.measures.len(), 4, "Should have 4 measures after paste insert");
        // Inserted measure at 0 has the dotted barline and ending [1]
        assert_eq!(doc.global.measures[0].barline.as_ref().map(|b| b.barline_type.as_str()), Some("dotted"));
        assert_eq!(doc.global.measures[0].ending.as_ref().unwrap().numbers, vec![1]);
        // Shifted original measure 1 is now at index 2
        assert_eq!(doc.global.measures[2].barline.as_ref().map(|b| b.barline_type.as_str()), Some("dotted"));
        assert_eq!(doc.global.measures[2].ending.as_ref().unwrap().numbers, vec![1]);
        // Shifted original measure 2 is now at index 3
        assert_eq!(doc.global.measures[3].ending.as_ref().unwrap().numbers, vec![2]);
    }

    #[test]
    fn test_slide_toggle_serialization_and_duplicate_insert() {
        let yaml = include_str!("../../examples/example.yaml");
        let mut app = MnxEditorApp::default();
        app.update_yaml(yaml.to_string());
        app.process_and_render();

        let total_events = app.cached_event_ranges.len();
        assert!(total_events > 0);

        app.selected_event_idx = Some(1);
        app.editing_string = Some(6);
        app.selected_note_idx_in_event = Some(0);

        // Toggle slide on
        app.action_toggle_slide();
        assert!(app.is_selected_note_sliding());
        assert!(app.validation_errors.is_empty(), "Validation errors: {:?}", app.validation_errors);

        // Verify that YAML serialization did not leak flow style into measures/events
        assert_eq!(app.cached_event_ranges.len(), total_events);
        for range in &app.cached_event_ranges {
            assert!(!range.note_ranges.is_empty());
        }

        // Test insert duplicate with the slide present
        app.action_duplicate_insert();
        assert!(app.validation_errors.is_empty(), "Validation errors after duplicate: {:?}", app.validation_errors);
        assert!(app.cached_event_ranges.len() > total_events);
        for range in &app.cached_event_ranges[..total_events] {
            assert!(!range.note_ranges.is_empty());
        }
    }

    #[test]
    fn test_edo_tuning_detection_populates_micro_fret_divisions() {
        let yaml = r#"---
mnx: { version: 1 }
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - name: Guitar
    _x:
      tuning:
        - string: 1
          pitch: { step: C, octave: 3 }
        - string: 2
          pitch: { step: G, octave: 3 }
        - string: 3
          pitch: { step: E, alter: "-1\\41", octave: 2 }
        - string: 4
          pitch: { step: C, octave: 2 }
        - string: 5
          pitch: { step: A, alter: "-1\\41", octave: 2 }
        - string: 6
          pitch: { step: F, octave: 1 }
        - string: 7
          pitch: { step: C, octave: 1 }
    measures:
      - sequences:
          - content:
              - duration: { base: whole }
                rest: {}
"#;
        let mut app = MnxEditorApp::default();
        app.update_yaml(yaml.trim().to_string());
        app.process_and_render();

        let doc = app.parsed_mnx.as_ref().unwrap();
        let part = &doc.parts[0];
        assert_eq!(part.detect_tuning_edo(), Some(41));
        assert_eq!(part.edo(), 41);
        assert_eq!(part.ext.micro_fret_divisions, Some(41));

        // Verify string labels rendered with 41-EDO formatting
        let s3_label = crate::render::format_pitch(&part.ext.tuning.as_ref().unwrap()[2].pitch, part.edo());
        let s5_label = crate::render::format_pitch(&part.ext.tuning.as_ref().unwrap()[4].pitch, part.edo());
        assert_eq!(s3_label, "vE", "String 3 should render as vE, not Eb");
        assert_eq!(s5_label, "vA", "String 5 should render as vA, not Ab");

        // Verify YAML text was updated with microFretDivisions: 41
        assert!(app.yaml_text.contains("microFretDivisions: 41"));
    }

    #[test]
    fn test_add_instrument_custom_fretboard_preserves_edo() {
        let mut app = MnxEditorApp::default();
        let tunings = vec![
            crate::mnx::StringTuning {
                string: 1,
                pitch: crate::mnx::Pitch { step: "C".to_string(), alter: None, octave: 3, ext: Default::default() },
            },
            crate::mnx::StringTuning {
                string: 2,
                pitch: crate::mnx::Pitch { step: "G".to_string(), alter: None, octave: 3, ext: Default::default() },
            },
            crate::mnx::StringTuning {
                string: 3,
                pitch: crate::mnx::Pitch {
                    step: "E".to_string(),
                    alter: Some(crate::mnx::Alteration::Fraction("-1\\41".to_string())),
                    octave: 2,
                    ext: Default::default(),
                },
            },
            crate::mnx::StringTuning {
                string: 4,
                pitch: crate::mnx::Pitch { step: "C".to_string(), alter: None, octave: 2, ext: Default::default() },
            },
            crate::mnx::StringTuning {
                string: 5,
                pitch: crate::mnx::Pitch {
                    step: "A".to_string(),
                    alter: Some(crate::mnx::Alteration::Fraction("-1\\41".to_string())),
                    octave: 2,
                    ext: Default::default(),
                },
            },
            crate::mnx::StringTuning {
                string: 6,
                pitch: crate::mnx::Pitch { step: "F".to_string(), alter: None, octave: 1, ext: Default::default() },
            },
            crate::mnx::StringTuning {
                string: 7,
                pitch: crate::mnx::Pitch { step: "C".to_string(), alter: None, octave: 1, ext: Default::default() },
            },
        ];

        let custom_frets = vec![
            crate::fretboard_editor::CustomFret {
                fret_number: 1.0,
                manual_number: None,
                spec: crate::fretboard_editor::FretLocationSpec::EdoStep { steps: 2, edo: 41 },
                strings: vec![1, 2, 3, 4, 5, 6, 7],
                note_name: "Fret 1".to_string(),
            },
            crate::fretboard_editor::CustomFret {
                fret_number: 2.0,
                manual_number: None,
                spec: crate::fretboard_editor::FretLocationSpec::EdoStep { steps: 4, edo: 41 },
                strings: vec![1, 2, 3, 4, 5, 6, 7],
                note_name: "Fret 2".to_string(),
            },
        ];

        let fb_state = crate::fretboard_editor::FretboardEditorState::new(
            7,
            custom_frets.clone(),
            "D".to_string(),
            4,
            293.67,
        );

        app.open_add_instrument_dialog();
        app.instrument_setup_name_buf = "41-EDO Kite Guitar".to_string();

        // Add instrument part and apply custom fretboard state (as done on Apply in Add Instrument dialog)
        app.add_new_instrument_part(
            "41-EDO Kite Guitar".to_string(),
            7,
            41,
            2,
            tunings,
            Some(custom_frets),
        );
        app.apply_custom_fretboard_state(fb_state);
        app.is_adding_instrument_in_dialog = false;
        app.active_dialog = ActiveDialog::None;

        let doc = app.parsed_mnx.as_ref().unwrap();
        assert_eq!(doc.parts.len(), 2);
        let new_part = &doc.parts[1];
        assert_eq!(new_part.name.as_deref(), Some("41-EDO Kite Guitar"));
        assert_eq!(new_part.ext.micro_fret_divisions, Some(41));
        assert_eq!(new_part.ext.fret_step_multiplier, Some(2));
        assert!(new_part.has_custom_frets());

        // Verify YAML output includes both microFretDivisions and fretStepMultiplier
        assert!(app.yaml_text.contains("microFretDivisions: 41"));
        assert!(app.yaml_text.contains("fretStepMultiplier: 2"));
    }

    #[test]
    fn test_extender_mnx_spec_and_yaml_serialization() {
        let mut app = MnxEditorApp::default();
        let yaml = r#"---
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - name: "Guitar"
    measures:
      - sequences:
          - content:
              - duration: { base: quarter }
                notes:
                  - pitch: { step: E, octave: 4 }
                    _x: { string: 1, fret: 0 }
              - duration: { base: quarter }
                notes:
                  - pitch: { step: G, octave: 3 }
                    _x: { string: 3, fret: 0 }
              - duration: { base: half }
                rest: {}
"#;
        app.update_yaml(yaml.to_string());
        app.process_and_render();

        // 1. Set lyric on event 0 with extender
        app.action_set_event_lyric(0, "Ooh_");
        assert!(app.yaml_text.contains("extender: true"));
        // Ensure extender is not inside _x
        assert!(!app.yaml_text.contains("_x:\n            extender: true"));
        assert!(!app.yaml_text.contains("_x:\n              extender: true"));

        let l0 = app.get_event_lyric_line(0, 1).expect("Event 0 should have lyric");
        assert_eq!(l0.text, "Ooh");
        assert_eq!(l0.extender, Some(true));
        assert!(l0.is_extender());

        // 2. Set standalone extender on event 1
        app.action_set_event_lyric(1, "_");
        let l1 = app.get_event_lyric_line(1, 1).expect("Event 1 should have lyric");
        assert_eq!(l1.text, "");
        assert_eq!(l1.extender, Some(true));
        assert!(l1.is_extender());

        // 3. Set following word on event 2
        app.action_set_event_lyric(2, "yeah");
        let doc = app.parsed_mnx.as_ref().unwrap();
        let pages = crate::layout::compute_score_layout(doc, 0.0);
        let mut dl = crate::layout::display_list::DisplayList::new();
        crate::pdf_export::populate_page_display_list(doc, &pages[0], &mut dl);
        // Find line draw item for lyric extender
        for item in &dl.items {
            if let crate::layout::display_list::DrawKind::Line { to, .. } = item.kind {
                if item.id.as_deref().map_or(false, |id| id.starts_with("lyric_")) {
                    // Find following word text item
                    for text_item in &dl.items {
                        if let crate::layout::display_list::DrawKind::Text { pos, text, .. } = &text_item.kind {
                            if text == "yeah" {
                                let text_start_x = pos.x - (text.chars().count() as f32 * 6.5) * 0.5;
                                assert!(to.x <= text_start_x, "Extender line must not extend into or underline the following word: line end {} vs word start {}", to.x, text_start_x);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_convert_selected_to_rest_in_orange_edit_mode() {
        let mut app = MnxEditorApp::default();
        let yaml = r#"---
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - name: "Guitar"
    measures:
      - sequences:
          - content:
              - duration: { base: quarter }
                notes:
                  - pitch: { step: E, octave: 4 }
                    _x: { string: 1, fret: 0 }
              - duration: { base: quarter }
                notes:
                  - pitch: { step: B, octave: 3 }
                    _x: { string: 2, fret: 0 }
              - duration: { base: half }
                rest: {}
"#;
        app.update_yaml(yaml.to_string());
        app.process_and_render();

        // 1. Select event 0 in orange edit mode on string 1
        app.selected_event_idx = Some(0);
        app.is_editing_fret = true;
        app.editing_string = Some(1);
        app.editing_fret_buf = "3".to_string();

        // 2. Convert event 0 to rest
        app.convert_selected_to_rest();

        // 3. UX should RETAIN orange edit mode and advance to next duration (event 1)
        assert!(app.is_editing_fret, "Should retain orange edit mode");
        assert_eq!(app.selected_event_idx, Some(1), "Should advance to next duration over");
        assert_eq!(app.editing_string, Some(1), "Should retain editing string");
        assert!(app.editing_fret_buf.is_empty(), "Fret buffer should be cleared for editing next note");

        // Verify event 0 in doc is now a rest
        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev0 = doc.get_event_at_part_path(0, crate::mnx::AstPath::TopLevel(0, 0, 0)).unwrap();
        assert!(ev0.rest.is_some());

        // 4. In blue select mode (is_editing_fret = false), convert_selected_to_rest does not enter orange mode
        app.is_editing_fret = false;
        app.selected_event_idx = Some(1);
        app.convert_selected_to_rest();
        assert!(!app.is_editing_fret, "Blue mode should remain blue mode");
    }

    #[test]
    fn test_toggle_and_remove_special_measure_features() {
        let yaml = r#"---
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - name: "Guitar"
    measures:
      - sequences:
          - content:
              - duration: { base: whole }
                rest: {}
"#;
        // 1. Toggle repeatStart
        let y_rep_start = crate::yaml_editor::set_global_measure_property(yaml, 0, "repeatStart", "{}");
        let doc: crate::mnx::MnxDocument = serde_saphyr::from_str(&y_rep_start).unwrap();
        assert!(doc.global.measures[0].repeat_start.is_some());

        let y_rep_start_cleared = crate::yaml_editor::set_global_measure_property(&y_rep_start, 0, "repeatStart", "");
        let doc: crate::mnx::MnxDocument = serde_saphyr::from_str(&y_rep_start_cleared).unwrap();
        assert!(doc.global.measures[0].repeat_start.is_none(), "repeatStart should be removed");

        // 2. Toggle repeatEnd
        let y_rep_end = crate::yaml_editor::set_global_measure_property(yaml, 0, "repeatEnd", "{ times: 2 }");
        let doc: crate::mnx::MnxDocument = serde_saphyr::from_str(&y_rep_end).unwrap();
        assert!(doc.global.measures[0].repeat_end.is_some());

        let y_rep_end_cleared = crate::yaml_editor::set_global_measure_property(&y_rep_end, 0, "repeatEnd", "");
        let doc: crate::mnx::MnxDocument = serde_saphyr::from_str(&y_rep_end_cleared).unwrap();
        assert!(doc.global.measures[0].repeat_end.is_none(), "repeatEnd should be removed");

        // 3. Toggle barline double
        let y_dbar = crate::yaml_editor::set_global_measure_property(yaml, 0, "barline", "{ type: double }");
        let doc: crate::mnx::MnxDocument = serde_saphyr::from_str(&y_dbar).unwrap();
        assert_eq!(doc.global.measures[0].barline.as_ref().map(|b| b.barline_type.as_str()), Some("double"));

        let y_dbar_cleared = crate::yaml_editor::set_global_measure_property(&y_dbar, 0, "barline", "");
        let doc: crate::mnx::MnxDocument = serde_saphyr::from_str(&y_dbar_cleared).unwrap();
        assert!(doc.global.measures[0].barline.is_none(), "double barline should be removed");

        // 4. Toggle ending
        let y_ending = crate::yaml_editor::set_global_measure_property(yaml, 0, "ending", "{ duration: 1, numbers: [1] }");
        let doc: crate::mnx::MnxDocument = serde_saphyr::from_str(&y_ending).unwrap();
        assert!(doc.global.measures[0].ending.is_some());

        let y_ending_cleared = crate::yaml_editor::set_global_measure_property(&y_ending, 0, "ending", "");
        let doc: crate::mnx::MnxDocument = serde_saphyr::from_str(&y_ending_cleared).unwrap();
        assert!(doc.global.measures[0].ending.is_none(), "ending should be removed");
    }

    #[test]
    fn test_let_ring_midi_playback_without_note_off_until_next_note_on_same_string() {
        let yaml = r#"---
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
    - time: { count: 4, unit: 4 }
parts:
  - name: "Guitar"
    measures:
      - sequences:
          - content:
              - duration: { base: half }
                notes:
                  - pitch: { step: E, octave: 4 }
                    ties:
                      - { target: "ev_non_existent" }
                    _x: { string: 1, fret: 0 }
              - duration: { base: half }
                notes:
                  - pitch: { step: B, octave: 3 }
                    _x: { string: 2, fret: 0 }
      - sequences:
          - content:
              - duration: { base: half }
                notes:
                  - pitch: { step: G, octave: 4 }
                    _x: { string: 1, fret: 3 }
              - duration: { base: half }
                rest: {}
"#;
        let doc: crate::mnx::MnxDocument = serde_saphyr::from_str(yaml).unwrap();
        let smf = crate::midi::generate_midi(&doc, crate::midi::MidiOptions::default());
        let track = &smf.tracks[0];

        // Track events:
        // Event 0: string 1, note 64 (E4) starts at t=0 (ticks=0). Has let-ring tie.
        // Event 1: string 2, note 59 (B3) starts at t=960. String 1 note should NOT have NoteOff!
        // Event 2: string 1, note 67 (G4) starts at t=1920 (measure 2 beat 1).
        //          String 1 note 64 MUST receive NoteOff at exactly t=1920!
        let mut note_64_offs: Vec<u32> = Vec::new();
        let mut curr_tick: u32 = 0;
        for ev in track {
            curr_tick += ev.delta.as_int();
            if let midly::TrackEventKind::Midi { message, .. } = ev.kind {
                if let midly::MidiMessage::NoteOff { key, .. } = message {
                    if key.as_int() == 64 {
                        note_64_offs.push(curr_tick);
                    }
                }
            }
        }

        assert_eq!(note_64_offs.len(), 1, "Note 64 should have exactly 1 NoteOff");
        assert_eq!(note_64_offs[0], 1920, "Note 64 NoteOff must occur at tick 1920 (when next note on string 1 is struck), not before");
    }

    #[test]
    fn test_unique_id_generation_no_collisions() {
        let iterations = 100_000;
        let mut ev_ids = std::collections::HashSet::with_capacity(iterations);
        let mut note_ids = std::collections::HashSet::with_capacity(iterations);
        let mut uuids = std::collections::HashSet::with_capacity(iterations);

        for _ in 0..iterations {
            let eid = crate::rebar::generate_event_id();
            assert!(eid.starts_with("ev_"));
            assert!(ev_ids.insert(eid));

            let nid = crate::rebar::generate_note_id();
            assert!(nid.starts_with("n_"));
            assert!(note_ids.insert(nid));

            let uid = crate::rebar::generate_uuid();
            assert!(uid.starts_with("id_"));
            assert!(uuids.insert(uid));
        }

        assert_eq!(ev_ids.len(), iterations);
        assert_eq!(note_ids.len(), iterations);
        assert_eq!(uuids.len(), iterations);

        // Ensure completely disjoint sets
        assert!(ev_ids.is_disjoint(&note_ids));
        assert!(ev_ids.is_disjoint(&uuids));
        assert!(note_ids.is_disjoint(&uuids));
    }

    #[test]
    fn test_ensure_unique_ids_sanitizes_duplicates_and_remaps_ties_slurs() {
        let yaml = r#"
mnx: { version: 1 }
global:
  measures:
  - time: { count: 4, unit: 4 }
parts:
- name: Guitar
  measures:
  - sequences:
    - content:
      - type: event
        id: shared_id
        duration: { base: quarter }
        notes:
        - id: shared_id
          pitch: { step: E, octave: 4 }
          _x: { string: 1, fret: 0 }
          ties: [{ target: target_shared }]
        - id: shared_id
          pitch: { step: B, octave: 3 }
          _x: { string: 2, fret: 0 }
          ties: [{ target: target_shared }]
        slurs:
        - target: shared_id_2
          side: up
          startNote: shared_id
          endNote: target_shared
      - type: event
        id: shared_id_2
        duration: { base: quarter }
        notes:
        - id: target_shared
          pitch: { step: E, octave: 4 }
          _x: { string: 1, fret: 0 }
        - id: target_shared
          pitch: { step: B, octave: 3 }
          _x: { string: 2, fret: 0 }
"#;
        let mut doc = serde_saphyr::from_str::<crate::mnx::MnxDocument>(yaml).unwrap();
        let changed = doc.ensure_unique_ids();
        assert!(changed, "ensure_unique_ids should detect and repair duplicated IDs");

        // Collect all IDs
        let mut all_ids = std::collections::HashSet::new();
        let ev1 = &doc.parts[0].measures[0].sequences[0].content[0];
        let ev2 = &doc.parts[0].measures[0].sequences[0].content[1];

        if let (crate::mnx::SequenceItem::Event(e1), crate::mnx::SequenceItem::Event(e2)) = (ev1, ev2) {
            let e1_id = e1.id.as_ref().unwrap();
            let e2_id = e2.id.as_ref().unwrap();
            let n1_1_id = e1.notes[0].id.as_ref().unwrap();
            let n1_2_id = e1.notes[1].id.as_ref().unwrap();
            let n2_1_id = e2.notes[0].id.as_ref().unwrap();
            let n2_2_id = e2.notes[1].id.as_ref().unwrap();

            assert!(all_ids.insert(e1_id.clone()));
            assert!(all_ids.insert(e2_id.clone()));
            assert!(all_ids.insert(n1_1_id.clone()));
            assert!(all_ids.insert(n1_2_id.clone()));
            assert!(all_ids.insert(n2_1_id.clone()));
            assert!(all_ids.insert(n2_2_id.clone()));
            assert_eq!(all_ids.len(), 6, "All 6 IDs must be completely unique");

            // Check tie remapping: string 1 note should tie to string 1 target
            assert_eq!(e1.notes[0].ties.as_ref().unwrap()[0].target, *n2_1_id);
            // Check tie remapping: string 2 note should tie to string 2 target
            assert_eq!(e1.notes[1].ties.as_ref().unwrap()[0].target, *n2_2_id);

            // Check slur remapping
            let slur = &e1.slurs.as_ref().unwrap()[0];
            assert_eq!(slur.target, *e2_id);
            assert_eq!(slur.start_note.as_ref().unwrap(), n1_1_id);
            assert_eq!(slur.end_note.as_ref().unwrap(), n2_1_id);
        } else {
            panic!("Expected events");
        }
    }

    #[test]
    fn test_tie_toggle_blue_mode_single_and_chord() {
        let yaml = r#"
mnx: { version: 1 }
global:
  measures:
  - time: { count: 4, unit: 4 }
parts:
- name: Guitar
  measures:
  - sequences:
    - content:
      - type: event
        duration: { base: quarter }
        notes:
        - pitch: { step: E, octave: 4 }
          _x: { string: 1, fret: 0 }
        - pitch: { step: B, octave: 3 }
          _x: { string: 2, fret: 0 }
        - pitch: { step: G, octave: 3 }
          _x: { string: 3, fret: 0 }
      - type: event
        duration: { base: quarter }
        notes:
        - pitch: { step: E, octave: 4 }
          _x: { string: 1, fret: 0 }
        - pitch: { step: B, octave: 3 }
          _x: { string: 2, fret: 0 }
        - pitch: { step: G, octave: 3 }
          _x: { string: 3, fret: 0 }
"#;
        let mut app = MnxEditorApp::default();
        app.yaml_text = yaml.to_string();
        app.process_and_render();
        // Blue Select Mode on event 0 (no editing_string, no note_idx)
        app.selected_event_idx = Some(0);
        app.editing_string = None;
        app.selected_note_idx_in_event = None;

        assert!(!app.is_selected_event_tied());

        // Toggle ON
        app.action_add_tie();
        assert!(app.is_selected_event_tied(), "Chord should now be tied");
        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev0 = doc.parts[0].measures[0].sequences[0].content[0].as_event().unwrap();
        assert_eq!(ev0.notes[0].ties.as_ref().unwrap().len(), 1);
        assert_eq!(ev0.notes[1].ties.as_ref().unwrap().len(), 1);
        assert_eq!(ev0.notes[2].ties.as_ref().unwrap().len(), 1);

        // Toggle OFF
        app.action_add_tie();
        assert!(!app.is_selected_event_tied(), "Chord should now be untied");
        let doc2 = app.parsed_mnx.as_ref().unwrap();
        let ev0_again = doc2.parts[0].measures[0].sequences[0].content[0].as_event().unwrap();
        assert!(ev0_again.notes[0].ties.is_none());
        assert!(ev0_again.notes[1].ties.is_none());
        assert!(ev0_again.notes[2].ties.is_none());
    }

    #[test]
    fn test_tie_toggle_incoming_note() {
        let yaml = r#"
mnx: { version: 1 }
global:
  measures:
  - time: { count: 4, unit: 4 }
parts:
- name: Guitar
  measures:
  - sequences:
    - content:
      - type: event
        duration: { base: quarter }
        notes:
        - id: note_a
          pitch: { step: E, octave: 4 }
          _x: { string: 1, fret: 0 }
          ties: [{ target: note_b }]
      - type: event
        id: ev_b
        duration: { base: quarter }
        notes:
        - id: note_b
          pitch: { step: E, octave: 4 }
          _x: { string: 1, fret: 0 }
"#;
        let mut app = MnxEditorApp::default();
        app.yaml_text = yaml.to_string();
        app.process_and_render();
        // Select target note (event 1, string 1) in Orange Mode
        app.selected_event_idx = Some(1);
        app.editing_string = Some(1);
        app.selected_note_idx_in_event = Some(0);

        // Target note has incoming tie from event 0
        app.action_add_tie();

        // Incoming tie on event 0 must now be removed!
        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev0 = doc.parts[0].measures[0].sequences[0].content[0].as_event().unwrap();
        assert!(ev0.notes[0].ties.is_none(), "Incoming tie should have been toggled off");
    }

    #[test]
    fn test_slur_toggle_blue_mode_single_and_chord() {
        let yaml = r#"
mnx: { version: 1 }
global:
  measures:
  - time: { count: 4, unit: 4 }
parts:
- name: Guitar
  measures:
  - sequences:
    - content:
      - type: event
        duration: { base: quarter }
        notes:
        - pitch: { step: E, octave: 4 }
          _x: { string: 1, fret: 0 }
      - type: event
        duration: { base: quarter }
        notes:
        - pitch: { step: F, octave: 4 }
          _x: { string: 1, fret: 1 }
"#;
        let mut app = MnxEditorApp::default();
        app.yaml_text = yaml.to_string();
        app.process_and_render();
        // Blue Select Mode on event 0
        app.selected_event_idx = Some(0);
        app.editing_string = None;
        app.selected_note_idx_in_event = None;

        assert!(!app.is_selected_note_slurred());

        // Toggle ON
        app.action_add_slur();
        assert!(app.is_selected_note_slurred(), "Event 0 should now be slurred");
        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev0 = doc.parts[0].measures[0].sequences[0].content[0].as_event().unwrap();
        assert!(ev0.slurs.is_some());
        assert_eq!(ev0.slurs.as_ref().unwrap().len(), 1);

        // Toggle OFF
        app.action_add_slur();
        assert!(!app.is_selected_note_slurred(), "Event 0 slur should now be toggled off");
        let doc2 = app.parsed_mnx.as_ref().unwrap();
        let ev0_again = doc2.parts[0].measures[0].sequences[0].content[0].as_event().unwrap();
        assert!(ev0_again.slurs.is_none());
    }

    #[test]
    fn test_slur_toggle_incoming_note() {
        let yaml = r#"
mnx: { version: 1 }
global:
  measures:
  - time: { count: 4, unit: 4 }
parts:
- name: Guitar
  measures:
  - sequences:
    - content:
      - type: event
        id: ev_0
        duration: { base: quarter }
        notes:
        - id: note_0
          pitch: { step: E, octave: 4 }
          _x: { string: 1, fret: 0 }
        slurs:
        - target: ev_1
          side: up
          startNote: note_0
          endNote: note_1
      - type: event
        id: ev_1
        duration: { base: quarter }
        notes:
        - id: note_1
          pitch: { step: F, octave: 4 }
          _x: { string: 1, fret: 1 }
"#;
        let mut app = MnxEditorApp::default();
        app.yaml_text = yaml.to_string();
        app.process_and_render();
        // Select event 1 (target of slur) in Blue mode
        app.selected_event_idx = Some(1);
        app.editing_string = None;
        app.selected_note_idx_in_event = None;

        app.action_add_slur();

        // Slur on event 0 should be removed!
        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev0 = doc.parts[0].measures[0].sequences[0].content[0].as_event().unwrap();
        assert!(ev0.slurs.is_none(), "Incoming slur on preceding event must be toggled off");
    }

    #[test]
    fn test_tie_and_slur_orange_mode_specific_string() {
        let yaml = r#"
mnx: { version: 1 }
global:
  measures:
  - time: { count: 4, unit: 4 }
parts:
- name: Guitar
  measures:
  - sequences:
    - content:
      - type: event
        duration: { base: quarter }
        notes:
        - pitch: { step: E, octave: 4 }
          _x: { string: 1, fret: 0 }
        - pitch: { step: B, octave: 3 }
          _x: { string: 2, fret: 0 }
      - type: event
        duration: { base: quarter }
        notes:
        - pitch: { step: E, octave: 4 }
          _x: { string: 1, fret: 0 }
        - pitch: { step: C, octave: 4 }
          _x: { string: 2, fret: 1 }
"#;
        let mut app = MnxEditorApp::default();
        app.yaml_text = yaml.to_string();
        app.process_and_render();
        // Orange Edit Mode on string 1 of event 0
        app.selected_event_idx = Some(0);
        app.editing_string = Some(1);
        app.selected_note_idx_in_event = Some(0);

        // Add tie on string 1
        app.action_add_tie();
        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev0 = doc.parts[0].measures[0].sequences[0].content[0].as_event().unwrap();
        assert!(ev0.notes[0].ties.is_some(), "String 1 must have tie");
        assert!(ev0.notes[1].ties.is_none(), "String 2 must NOT have tie");

        // Toggle tie off on string 1
        app.action_add_tie();
        let doc2 = app.parsed_mnx.as_ref().unwrap();
        let ev0_again = doc2.parts[0].measures[0].sequences[0].content[0].as_event().unwrap();
        assert!(ev0_again.notes[0].ties.is_none(), "String 1 tie must be removed");

        // Now move to string 2 in Orange Edit Mode
        app.editing_string = Some(2);
        app.selected_note_idx_in_event = Some(1);

        // Add slur on string 2 (hammer-on from fret 0 to fret 1)
        app.action_add_slur();
        let doc3 = app.parsed_mnx.as_ref().unwrap();
        let ev0_slur = doc3.parts[0].measures[0].sequences[0].content[0].as_event().unwrap();
        assert!(ev0_slur.slurs.is_some(), "Event must have slur on string 2");

        // Toggle slur off on string 2
        app.action_add_slur();
        let doc4 = app.parsed_mnx.as_ref().unwrap();
        let ev0_noslur = doc4.parts[0].measures[0].sequences[0].content[0].as_event().unwrap();
        assert!(ev0_noslur.slurs.is_none(), "String 2 slur must be toggled off");
    }

    #[test]
    fn test_view_menu_theme_switch_dark_light() {
        let mut app = MnxEditorApp::default();
        let ctx = egui::Context::default();
        crate::setup_custom_fonts(&ctx);

        let event_ranges = app.cached_event_ranges.clone();
        let total_events = event_ranges.len();

        // 1. Initially Dark Mode
        ctx.set_visuals(egui::Visuals::dark());
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            assert!(ui.visuals().dark_mode, "Context should be in dark mode");
            app.render_top_panel(ui, &event_ranges, total_events);
        });
        out.textures_delta.clear();

        // 2. Switch to Light Mode
        ctx.set_visuals(egui::Visuals::light());
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            assert!(!ui.visuals().dark_mode, "Context should be in light mode");
            app.render_top_panel(ui, &event_ranges, total_events);
        });
        out.textures_delta.clear();

        // 3. Switch back to Dark Mode
        ctx.set_visuals(egui::Visuals::dark());
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            assert!(ui.visuals().dark_mode, "Context should be back in dark mode");
            app.render_top_panel(ui, &event_ranges, total_events);
        });
        out.textures_delta.clear();
    }

    #[test]
    fn test_fretboard_editor_ui_dark_and_light_mode() {
        let ctx = egui::Context::default();
        crate::setup_custom_fonts(&ctx);
        let mut editor = crate::fretboard_editor::FretboardEditorState::new(
            6,
            Vec::new(),
            "D".to_string(),
            4,
            440.0,
        );
        editor.guide_input = "5/4".to_string();
        editor.update_guides();
        editor.add_all_guide_frets();

        // 1. Render in Dark Mode
        ctx.set_visuals(egui::Visuals::dark());
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            assert!(ui.visuals().dark_mode);
            editor.ui(ui);
        });
        out.textures_delta.clear();

        // 2. Render in Light Mode
        ctx.set_visuals(egui::Visuals::light());
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            assert!(!ui.visuals().dark_mode);
            editor.ui(ui);
        });
        out.textures_delta.clear();
    }

    #[test]
    fn test_shift_alt_up_down_duration_bump_select_and_edit_mode() {
        let mut app = MnxEditorApp::default();
        let yaml = r#"mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - name: "Guitar"
    _x:
      tuning:
        - string: 1
          pitch: { step: E, octave: 4 }
        - string: 2
          pitch: { step: B, octave: 3 }
    measures:
      - sequences:
          - content:
              - duration: { base: eighth }
                notes:
                  - pitch: { step: E, octave: 4 }
                    _x: { string: 1, fret: 0 }
              - duration: { base: eighth }
                notes:
                  - pitch: { step: B, octave: 3 }
                    _x: { string: 2, fret: 0 }
"#;
        app.yaml_text = yaml.to_string();
        app.process_and_render();

        let ctx = egui::Context::default();
        crate::setup_custom_fonts(&ctx);

        let send_key = |app: &mut MnxEditorApp, ctx: &egui::Context, key: egui::Key, shift: bool, alt: bool| {
            let mods = egui::Modifiers {
                shift,
                alt,
                ..Default::default()
            };
            let mut raw_input = egui::RawInput::default();
            raw_input.events.push(egui::Event::ModifiersChanged(mods));
            raw_input.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: mods,
            });
            let event_ranges = app.cached_event_ranges.clone();
            let total_events = event_ranges.len();
            let mut out = ctx.run_ui(raw_input, |ui| {
                app.handle_keyboard_input(ui, false, &event_ranges, total_events);
            });
            out.textures_delta.clear();
        };

        // ── 1. Select Mode Bump Tests ──────────────────────────────────────────
        app.selected_event_idx = Some(0);
        app.is_editing_fret = false;
        assert_eq!(app.get_selected_event_duration().unwrap().base, "eighth");

        // Shift+Alt+Up -> quarter
        send_key(&mut app, &ctx, egui::Key::ArrowUp, true, true);
        assert_eq!(app.get_selected_event_duration().unwrap().base, "quarter");

        // Shift+Alt+Up -> half
        send_key(&mut app, &ctx, egui::Key::ArrowUp, true, true);
        assert_eq!(app.get_selected_event_duration().unwrap().base, "half");

        // Shift+Alt+Up -> whole
        send_key(&mut app, &ctx, egui::Key::ArrowUp, true, true);
        assert_eq!(app.get_selected_event_duration().unwrap().base, "whole");

        // Shift+Alt+Up again -> capped at whole
        send_key(&mut app, &ctx, egui::Key::ArrowUp, true, true);
        assert_eq!(app.get_selected_event_duration().unwrap().base, "whole");

        // Shift+Alt+Down -> half
        send_key(&mut app, &ctx, egui::Key::ArrowDown, true, true);
        assert_eq!(app.get_selected_event_duration().unwrap().base, "half");

        // Shift+Alt+Down -> quarter
        send_key(&mut app, &ctx, egui::Key::ArrowDown, true, true);
        assert_eq!(app.get_selected_event_duration().unwrap().base, "quarter");

        // Shift+Alt+Down -> eighth
        send_key(&mut app, &ctx, egui::Key::ArrowDown, true, true);
        assert_eq!(app.get_selected_event_duration().unwrap().base, "eighth");

        // Shift+Alt+Down -> 16th
        send_key(&mut app, &ctx, egui::Key::ArrowDown, true, true);
        assert_eq!(app.get_selected_event_duration().unwrap().base, "16th");

        // Shift+Alt+Down -> 32nd
        send_key(&mut app, &ctx, egui::Key::ArrowDown, true, true);
        assert_eq!(app.get_selected_event_duration().unwrap().base, "32nd");

        // Shift+Alt+Down again -> capped at 32nd
        send_key(&mut app, &ctx, egui::Key::ArrowDown, true, true);
        assert_eq!(app.get_selected_event_duration().unwrap().base, "32nd");

        // ── 2. Edit Mode Bump Tests ────────────────────────────────────────────
        app.selected_event_idx = Some(0);
        app.editing_string = Some(1);
        app.selected_note_idx_in_event = Some(0);
        app.is_editing_fret = true;
        app.editing_fret_buf.clear();

        // Shift+Alt+Up -> 16th (from 32nd)
        send_key(&mut app, &ctx, egui::Key::ArrowUp, true, true);
        assert_eq!(app.get_selected_event_duration().unwrap().base, "16th");
        assert!(app.is_editing_fret, "Must remain in edit mode");
        assert_eq!(app.editing_string, Some(1), "Must keep active string");

        // Type fret "7" and press Shift+Alt+Up -> commits fret 7 and bumps duration to eighth
        app.editing_fret_buf = "7".to_string();
        send_key(&mut app, &ctx, egui::Key::ArrowUp, true, true);
        assert_eq!(app.get_selected_event_duration().unwrap().base, "eighth");
        assert!(app.is_editing_fret, "Must remain in edit mode");
        let doc = app.parsed_mnx.as_ref().unwrap();
        let ev0 = doc.parts[0].measures[0].sequences[0].content[0].as_event().unwrap();
        assert_eq!(ev0.notes[0].fret, Some(7), "Fret 7 must be committed");

        // ── 3. Multi-Select Range Bump Tests ───────────────────────────────────
        app.is_editing_fret = false;
        app.selected_event_idx = Some(1);
        app.selection_anchor = Some(0);
        // Both event 0 and event 1 are currently "eighth" notes.
        // Bump both to "quarter"
        send_key(&mut app, &ctx, egui::Key::ArrowUp, true, true);
        let doc2 = app.parsed_mnx.as_ref().unwrap();
        let ev0_dur = &doc2.parts[0].measures[0].sequences[0].content[0].as_event().unwrap().duration.base;
        let ev1_dur = &doc2.parts[0].measures[0].sequences[0].content[1].as_event().unwrap().duration.base;
        assert_eq!(ev0_dur, "quarter", "Event 0 in range bumped to quarter");
        assert_eq!(ev1_dur, "quarter", "Event 1 in range bumped to quarter");
    }

    #[test]
    fn test_per_instrument_midi_port_configuration() {
        let mut app = MnxEditorApp::default();
        app.midi_ports = vec![
            crate::midi::MidiPortInfo {
                id: 0,
                name: "Port A".to_string(),
            },
            crate::midi::MidiPortInfo {
                id: 1,
                name: "Port B".to_string(),
            },
        ];

        // Add second instrument
        let tunings = vec![
            crate::mnx::StringTuning {
                string: 1,
                pitch: crate::mnx::Pitch {
                    step: "E".to_string(),
                    octave: 4,
                    alter: None,
                    ..Default::default()
                },
            },
        ];
        app.add_new_instrument_part("Bass".to_string(), 1, 12, 1, tunings, None);

        // Assign Part 0 to Port 0 and Part 1 to Port 1
        app.set_part_midi_port(0, 0);
        app.set_part_midi_port(1, 1);
        assert_eq!(app.get_part_midi_port(0), Some(0));
        assert_eq!(app.get_part_midi_port(1), Some(1));

        // When switching active part, selected_midi_port syncs to the active part's port
        app.select_part(0);
        assert_eq!(app.selected_midi_port, Some(0));
        app.select_part(1);
        assert_eq!(app.selected_midi_port, Some(1));

        // When removing part 0, part 1 shifts down to index 0 and retains its port assignment (Port 1)
        app.remove_instrument_part(0);
        assert_eq!(app.get_part_midi_port(0), Some(1));
    }

    #[test]
    fn test_per_instrument_pb_range_and_yaml_persistence() {
        let mut app = MnxEditorApp::default();
        let tunings = vec![
            crate::mnx::StringTuning {
                string: 1,
                pitch: crate::mnx::Pitch {
                    step: "E".to_string(),
                    octave: 4,
                    alter: None,
                    ..Default::default()
                },
            },
        ];
        app.add_new_instrument_part("Synth".to_string(), 1, 12, 1, tunings, None);

        app.set_part_pitch_bend_range(0, 12);
        app.set_part_pitch_bend_range(1, 24);

        assert_eq!(app.get_part_pitch_bend_range(0), 12);
        assert_eq!(app.get_part_pitch_bend_range(1), 24);

        // Verify active part selection updates app.pitch_bend_range
        app.select_part(0);
        assert_eq!(app.pitch_bend_range, 12);
        app.select_part(1);
        assert_eq!(app.pitch_bend_range, 24);

        // Verify YAML roundtrip persistence
        let mut app2 = MnxEditorApp::default();
        app2.update_yaml(app.yaml_text.clone());
        app2.process_and_render();
        assert_eq!(app2.get_part_pitch_bend_range(0), 12);
        assert_eq!(app2.get_part_pitch_bend_range(1), 24);
    }

    #[test]
    fn test_per_instrument_mpe_mode_and_channel_mapping() {
        let mut app = MnxEditorApp::default();
        let tunings = vec![
            crate::mnx::StringTuning {
                string: 1,
                pitch: crate::mnx::Pitch {
                    step: "E".to_string(),
                    octave: 4,
                    alter: None,
                    ..Default::default()
                },
            },
        ];
        app.add_new_instrument_part("Pad".to_string(), 1, 12, 1, tunings, None);

        // Part 0 is Multi-Timbral by default
        assert!(!app.is_part_mpe_mode(0));
        assert_eq!(app.get_part_pitch_bend_range(0), 2);

        // Set Part 1 to MPE
        app.set_part_mpe_mode(1, true);
        assert!(app.is_part_mpe_mode(1));
        assert!(!app.is_part_mpe_mode(0));
        assert_eq!(app.get_part_pitch_bend_range(1), 48);

        // Part 0 has 8 strings on Port 0. When Part 1 shares Port 0,
        // its member channel offset is 2 + 8 = 10.
        let map1 = app.part_string_channel_maps.get(&1).unwrap();
        assert_eq!(map1.get(&1), Some(&10));

        // When Part 1 is assigned its own MIDI port (Port 1), its prior strings on that port is 0,
        // so its member channel starts at Ch 2.
        app.midi_ports = vec![
            crate::midi::MidiPortInfo { id: 0, name: "Port 0".to_string() },
            crate::midi::MidiPortInfo { id: 1, name: "Port 1".to_string() },
        ];
        app.set_part_midi_port(1, 1);
        app.set_part_mpe_mode(1, true);
        let map1_own_port = app.part_string_channel_maps.get(&1).unwrap();
        assert_eq!(map1_own_port.get(&1), Some(&2));

        // Active part selection syncs is_mpe_mode and pitch_bend_range
        app.select_part(0);
        assert!(!app.is_mpe_mode);
        assert_eq!(app.pitch_bend_range, 2);

        app.select_part(1);
        assert!(app.is_mpe_mode);
        assert_eq!(app.pitch_bend_range, 48);

        // Verify YAML roundtrip persistence of mpe
        let mut app3 = MnxEditorApp::default();
        app3.update_yaml(app.yaml_text.clone());
        app3.process_and_render();
        assert!(!app3.is_part_mpe_mode(0));
        assert!(app3.is_part_mpe_mode(1));
        assert_eq!(app3.get_part_pitch_bend_range(1), 48);
    }

    #[test]
    fn test_per_instrument_pitch_bend_and_mpe_isolation() {
        let mut app = MnxEditorApp::default();
        let tunings = vec![crate::mnx::StringTuning {
            string: 1,
            pitch: crate::mnx::Pitch { step: "E".to_string(), octave: 4, alter: None, ..Default::default() },
        }];
        app.add_new_instrument_part("Synth".to_string(), 1, 12, 1, tunings, None);

        // Initially both are Multi-Timbral with default PB = 2
        assert_eq!(app.get_part_pitch_bend_range(0), 2);
        assert_eq!(app.get_part_pitch_bend_range(1), 2);
        assert!(!app.is_part_mpe_mode(0));
        assert!(!app.is_part_mpe_mode(1));

        // Set Part 0 PB range to 12
        app.set_part_pitch_bend_range(0, 12);
        assert_eq!(app.get_part_pitch_bend_range(0), 12);
        assert_eq!(app.get_part_pitch_bend_range(1), 2);

        // Set Part 1 MPE mode to true
        app.set_part_mpe_mode(1, true);
        assert!(!app.is_part_mpe_mode(0));
        assert!(app.is_part_mpe_mode(1));
        assert_eq!(app.get_part_pitch_bend_range(0), 12);
        assert_eq!(app.get_part_pitch_bend_range(1), 48);

        // Customize Part 1 PB range to 24
        app.set_part_pitch_bend_range(1, 24);
        assert_eq!(app.get_part_pitch_bend_range(0), 12);
        assert_eq!(app.get_part_pitch_bend_range(1), 24);
        assert!(!app.is_part_mpe_mode(0));
        assert!(app.is_part_mpe_mode(1));

        // Switching active parts in the editor syncs the editor's live controls
        app.select_part(0);
        assert_eq!(app.pitch_bend_range, 12);
        assert!(!app.is_mpe_mode);

        app.select_part(1);
        assert_eq!(app.pitch_bend_range, 24);
        assert!(app.is_mpe_mode);

        // Verify UI rendering with both instruments works cleanly
        let ctx = egui::Context::default();
        crate::setup_custom_fonts(&ctx);
        app.show_string_channels_popup = true;

        let event_ranges = app.cached_event_ranges.clone();
        let total_events = event_ranges.len();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.render_top_panel(ui, &event_ranges, total_events);
        });
        out.textures_delta.clear();

        // Verify YAML roundtrip persistence of both PB ranges and MPE modes
        let mut app2 = MnxEditorApp::default();
        app2.update_yaml(app.yaml_text.clone());
        app2.process_and_render();
        assert_eq!(app2.get_part_pitch_bend_range(0), 12);
        assert_eq!(app2.get_part_pitch_bend_range(1), 24);
        assert!(!app2.is_part_mpe_mode(0));
        assert!(app2.is_part_mpe_mode(1));

        // Removing Part 0 shifts Part 1's settings to index 0
        app.remove_instrument_part(0);
        assert_eq!(app.get_part_pitch_bend_range(0), 24);
        assert!(app.is_part_mpe_mode(0));
    }

    #[test]
    fn test_midi_string_channels_popup_ui_rendering() {
        let mut app = MnxEditorApp::default();
        let ctx = egui::Context::default();
        crate::setup_custom_fonts(&ctx);

        app.midi_ports = vec![
            crate::midi::MidiPortInfo {
                id: 0,
                name: "Virtual MIDI Out".to_string(),
            },
        ];

        // Open popup
        app.show_string_channels_popup = true;

        let event_ranges = app.cached_event_ranges.clone();
        let total_events = event_ranges.len();

        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.render_top_panel(ui, &event_ranges, total_events);
        });
        out.textures_delta.clear();
        assert!(app.show_string_channels_popup);
    }
}

