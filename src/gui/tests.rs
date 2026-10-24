use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_all_event_ranges_example_yaml() {
        let yaml = include_str!("../../example.yaml");
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
        let yaml = include_str!("../../example.yaml");
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
        let yaml = include_str!("../../example.yaml");
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
        let yaml = include_str!("../../example.yaml");
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
        let yaml = include_str!("../../example.yaml");
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
            "example.yaml",
            "example_endings.yaml",
            "example_harmonics.yaml",
            "example_layout_lyrics.yaml",
            "c scale.yaml",
            "chord example.yaml",
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
                "example.yaml" | "example_endings.yaml" | "example_harmonics.yaml" | "example_layout_lyrics.yaml"
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
        let yaml = include_str!("../../example.yaml");
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
        let yaml = include_str!("../../example.yaml");
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
        let yaml = include_str!("../../example.yaml");
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
        let yaml = include_str!("../../example.yaml");
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
        // In example.yaml, event 0 has a note on string 7, but string 1 is blank at event 0.
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
                },
            },
            crate::mnx::StringTuning {
                string: 2,
                pitch: crate::mnx::Pitch {
                    step: "B".to_string(),
                    alter: None,
                    octave: 3,
                },
            },
            crate::mnx::StringTuning {
                string: 3,
                pitch: crate::mnx::Pitch {
                    step: "G".to_string(),
                    alter: None,
                    octave: 3,
                },
            },
            crate::mnx::StringTuning {
                string: 4,
                pitch: crate::mnx::Pitch {
                    step: "D".to_string(),
                    alter: None,
                    octave: 3,
                },
            },
            crate::mnx::StringTuning {
                string: 5,
                pitch: crate::mnx::Pitch {
                    step: "A".to_string(),
                    alter: None,
                    octave: 2,
                },
            },
            crate::mnx::StringTuning {
                string: 6,
                pitch: crate::mnx::Pitch {
                    step: "E".to_string(),
                    alter: None,
                    octave: 2,
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
        assert_eq!(ev2.notes[0].fret, Some(0));
        assert_eq!(ev2.notes[0].pitch.step, "E");
        assert_eq!(ev2.notes[0].pitch.octave, 4);
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
        assert_eq!(l2.ext.extender, Some(true));

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
        assert_eq!(l3.ext.extender, Some(true));

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
                },
            },
            crate::mnx::StringTuning {
                string: 2,
                pitch: crate::mnx::Pitch {
                    step: "B".to_string(),
                    octave: 3,
                    alter: Some(crate::mnx::Alteration::Value(-0.5)),
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
                },
            },
            crate::mnx::StringTuning {
                string: 2,
                pitch: crate::mnx::Pitch {
                    step: "D".to_string(),
                    octave: 3,
                    alter: None,
                },
            },
            crate::mnx::StringTuning {
                string: 3,
                pitch: crate::mnx::Pitch {
                    step: "A".to_string(),
                    octave: 2,
                    alter: None,
                },
            },
            crate::mnx::StringTuning {
                string: 4,
                pitch: crate::mnx::Pitch {
                    step: "E".to_string(),
                    octave: 2,
                    alter: None,
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
}

