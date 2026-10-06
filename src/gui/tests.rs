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
  - instrument:
      name: "Guitar"
    measures:
      - sequences:
          - content:
              - duration: { base: whole }
                notes:
                  - pitch: { step: C, octave: 3 }
                    string: 5
                    fret: 3
                  - pitch: { step: E, octave: 3 }
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
            .flat_map(|m| m.directions.iter())
            .find_map(|d| d.tempo.as_ref().map(|t| t.bpm));
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
            .flat_map(|m| m.directions.iter())
            .find_map(|d| d.tempo.as_ref().map(|t| t.bpm));
        assert_eq!(bpm, Some(200.0), "BPM should be updated to 200");
        let tempo_count = doc
            .global
            .measures
            .iter()
            .flat_map(|m| m.directions.iter())
            .filter(|d| d.tempo.is_some())
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
            ActiveDialog::ConfirmDiscardChanges,
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
}
