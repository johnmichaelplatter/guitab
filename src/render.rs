use crate::mnx::{Part, Pitch};
use crate::validate::ValidationError;
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Stroke, Vec2};

// ── Pitch helpers ────────────────────────────────────────────────────────────

fn format_pitch(pitch: &Pitch) -> String {
    let mut prefix = String::new();
    let mut suffix = String::new();
    if let Some(alter) = &pitch.alter {
        let alter_str = alter.to_string();
        if alter_str.starts_with("-1\\") || alter_str.starts_with("-1/") {
            prefix.push('v');
        } else if alter_str.starts_with("1\\") || alter_str.starts_with("1/") {
            prefix.push('^');
        } else {
            suffix = format!("({})", alter_str);
        }
    }
    format!("{}{}{}", prefix, pitch.step, suffix)
}

// ── Rhythm types ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
enum Rhythm {
    Whole,
    Half,
    Quarter,
    Eighth,
    Sixteenth,
    ThirtySecond,
}

impl Rhythm {
    /// Duration in quarter-note beats (used for beam-grouping decisions).
    fn beats(self) -> f64 {
        match self {
            Rhythm::Whole        => 4.0,
            Rhythm::Half         => 2.0,
            Rhythm::Quarter      => 1.0,
            Rhythm::Eighth       => 0.5,
            Rhythm::Sixteenth    => 0.25,
            Rhythm::ThirtySecond => 0.125,
        }
    }

    /// Number of beam/flag lines needed (0 for whole/half/quarter).
    fn flag_count(self) -> usize {
        match self {
            Rhythm::Whole | Rhythm::Half | Rhythm::Quarter => 0,
            Rhythm::Eighth       => 1,
            Rhythm::Sixteenth    => 2,
            Rhythm::ThirtySecond => 3,
        }
    }
}

fn parse_rhythm(base: &str) -> (Rhythm, bool) {
    let s = base.to_lowercase();
    // dotted: the spec might say "dotted-quarter" or similar
    let is_dotted = s.contains("dotted") || s.contains('.');
    let r = if s.contains("whole")         { Rhythm::Whole }
       else if s.contains("half")          { Rhythm::Half }
       else if s.contains("32nd") || s.contains("thirtysecond") { Rhythm::ThirtySecond }
       else if s.contains("16th") || s.contains("sixteenth")    { Rhythm::Sixteenth }
       else if s.contains("eighth")        { Rhythm::Eighth }
       else if s.contains("quarter")       { Rhythm::Quarter }
       else                                { Rhythm::Quarter };
    (r, is_dotted)
}

// ── Collected note position for rhythm drawing ────────────────────────────────

struct RhythmNote {
    x: f32,
    rhythm: Rhythm,
    is_dotted: bool,
    measure_idx: usize,
}

// ── Main draw function ────────────────────────────────────────────────────────

pub fn draw_tablature(
    ui: &mut egui::Ui,
    part: &Part,
    errors: &[ValidationError],
) -> egui::Response {
    let num_strings = part.num_strings();
    if num_strings == 0 {
        return ui.label("No strings found.");
    }

    // ── Layout constants ─────────────────────────────────────────────────────

    let line_spacing   = 24.0_f32;  // vertical gap between string lines
    let event_spacing  = 30.0_f32;  // horizontal gap between events
    let measure_spacing= 20.0_f32;  // extra gap after a bar line
    let margin_left    = 10.0_f32;
    let label_gap      =  8.0_f32;
    let first_bar_pad  = 25.0_f32;  // breathing room after the opening barline

    // Rhythm row sits below the staff
    let rhythm_gap     = 14.0_f32;  // gap between last string line and stem top
    let stem_height    = 22.0_f32;
    let flag_spacing   =  4.5_f32;  // vertical distance between flags

    // ── Font ─────────────────────────────────────────────────────────────────

    let font_id = FontId::monospace(14.0);

    // ── String labels ────────────────────────────────────────────────────────

    let mut prefixes: Vec<String> = (1..=num_strings).map(|i| i.to_string()).collect();
    if let Some(inst) = &part.instrument {
        if let Some(tuning) = &inst.tuning {
            for t in tuning {
                if t.string >= 1 && (t.string as usize) <= num_strings {
                    prefixes[(t.string - 1) as usize] = format_pitch(&t.pitch);
                }
            }
        }
    }

    let max_prefix_width = prefixes
        .iter()
        .map(|p| {
            ui.painter()
                .layout_no_wrap(p.clone(), font_id.clone(), Color32::WHITE)
                .size()
                .x
        })
        .fold(0.0_f32, f32::max);

    // ── Total event count ─────────────────────────────────────────────────────

    let total_events: usize = part
        .measures
        .iter()
        .filter_map(|m| m.sequences.first())
        .map(|s| s.content.len())
        .sum();

    // ── Canvas size ───────────────────────────────────────────────────────────

    let first_barline_offset = margin_left + max_prefix_width + label_gap;
    let start_offset_x       = first_barline_offset + first_bar_pad;

    let staff_height   = (num_strings as f32 - 1.0).max(0.0) * line_spacing;
    let rhythm_row_h   = rhythm_gap + stem_height + 10.0;                 // stem + flags/dots + pad
    let total_height   = 20.0 + staff_height + rhythm_row_h + 10.0;       // top pad + staff + rhythm + bottom pad
    let total_width    = start_offset_x
        + (total_events as f32) * event_spacing
        + (part.measures.len() as f32) * measure_spacing
        + 20.0;

    // ── Allocate painter ──────────────────────────────────────────────────────

    let (response, painter) = ui.allocate_painter(
        Vec2::new(total_width, total_height),
        egui::Sense::hover(),
    );
    let rect = response.rect;

    let first_barline_x = rect.min.x + first_barline_offset;
    let label_x         = first_barline_x - label_gap;

    // ── Draw string lines and labels ──────────────────────────────────────────

    let mut string_y_coords = Vec::with_capacity(num_strings);
    for i in 0..num_strings {
        let y = rect.min.y + 20.0 + (i as f32) * line_spacing;
        string_y_coords.push(y);

        painter.line_segment(
            [Pos2::new(first_barline_x, y), Pos2::new(rect.max.x - 10.0, y)],
            Stroke::new(1.0, Color32::from_gray(100)),
        );

        painter.text(
            Pos2::new(label_x, y),
            egui::Align2::RIGHT_CENTER,
            &prefixes[i],
            font_id.clone(),
            Color32::DARK_GRAY,
        );
    }

    // The bottom of the last string line — rhythms sit below here.
    let last_string_y = *string_y_coords.last().unwrap();
    let stem_top_y    = last_string_y + rhythm_gap;
    let stem_bot_y    = stem_top_y + stem_height;

    // ── Error coordinate set ──────────────────────────────────────────────────

    let error_coords: std::collections::HashSet<_> = errors
        .iter()
        .map(|e| (e.measure_idx, e.event_idx, e.string, e.fret))
        .collect();

    // ── Opening barline ───────────────────────────────────────────────────────

    painter.line_segment(
        [
            Pos2::new(first_barline_x, string_y_coords[0]),
            Pos2::new(first_barline_x, last_string_y),
        ],
        Stroke::new(2.0, Color32::from_gray(150)),
    );

    let mut current_x   = rect.min.x + start_offset_x;
    let bg_color        = ui.visuals().panel_fill;
    let note_color      = ui.visuals().text_color();

    // Collect rhythm notes so we can do beam-grouping after the main loop.
    let mut rhythm_notes: Vec<RhythmNote> = Vec::new();

    // ── Main event loop ───────────────────────────────────────────────────────

    for (m_idx, measure) in part.measures.iter().enumerate() {
        if let Some(seq) = measure.sequences.first() {
            for (e_idx, event) in seq.content.iter().enumerate() {

                // Fret numbers on strings
                let mut step_frets: Vec<Option<u8>> = vec![None; num_strings];
                for note in &event.notes {
                    if let (Some(s), Some(f)) = (note.string, note.fret) {
                        if s >= 1 && (s as usize) <= num_strings {
                            step_frets[(s - 1) as usize] = Some(f);
                        }
                    }
                }

                for i in 0..num_strings {
                    if let Some(fret) = step_frets[i] {
                        let string_num = (i + 1) as u8;
                        let is_err =
                            error_coords.contains(&(m_idx + 1, e_idx, string_num, fret));
                        let text_color = if is_err { Color32::RED } else { note_color };
                        let y = string_y_coords[i];

                        let text   = fret.to_string();
                        let galley = painter.layout_no_wrap(text, font_id.clone(), text_color);
                        let text_rect = Rect::from_center_size(
                            Pos2::new(current_x, y),
                            galley.size(),
                        );

                        painter.rect_filled(text_rect.expand(2.0), 0.0, bg_color);
                        painter.galley(text_rect.min, galley, text_color);
                    }
                }

                // Collect rhythm info for drawing after loop
                let (rhythm, is_dotted) = parse_rhythm(&event.duration.base);
                rhythm_notes.push(RhythmNote {
                    x: current_x,
                    rhythm,
                    is_dotted,
                    measure_idx: m_idx,
                });

                current_x += event_spacing;
            }
        }

        // Barline at end of measure (spans staff only — rhythms are outside)
        painter.line_segment(
            [
                Pos2::new(current_x, string_y_coords[0]),
                Pos2::new(current_x, last_string_y),
            ],
            Stroke::new(1.0, Color32::from_gray(150)),
        );
        current_x += measure_spacing;
    }

    // ── Rhythm notation ───────────────────────────────────────────────────────
    // Strategy:
    //  • Every note gets a vertical stem at event center `cx` (whole notes get none)
    //  • No circle noteheads are drawn
    //  • Beams/flags are drawn at the bottom of the stem (`stem_bot_y`), stacking upward
    //  • Beam groups connect up to 4 consecutive 8th/16th/32nd notes within the same measure

    let n = rhythm_notes.len();
    let mut beam_right = vec![false; n];
    let mut current_group_count = 0;

    for i in 0..n {
        let flags = rhythm_notes[i].rhythm.flag_count();
        if flags == 0 {
            current_group_count = 0;
            continue;
        }

        current_group_count += 1;

        if current_group_count < 4 && i + 1 < n {
            let next_note = &rhythm_notes[i + 1];
            if next_note.measure_idx == rhythm_notes[i].measure_idx
                && next_note.rhythm.flag_count() >= 1
            {
                beam_right[i] = true;
            } else {
                current_group_count = 0;
            }
        } else {
            current_group_count = 0;
        }
    }

    for (i, note) in rhythm_notes.iter().enumerate() {
        let cx = note.x;
        let flags = note.rhythm.flag_count();

        // ── Stem ──────────────────────────────────────────────────────────────
        if note.rhythm != Rhythm::Whole {
            painter.line_segment(
                [Pos2::new(cx, stem_top_y), Pos2::new(cx, stem_bot_y)],
                Stroke::new(1.5, note_color),
            );
        }

        // ── Dot (augmentation) ────────────────────────────────────────────────
        if note.is_dotted {
            painter.circle_filled(
                Pos2::new(cx + 5.0, stem_bot_y - 2.0),
                1.5,
                note_color,
            );
        }

        // ── Flags or beams (at bottom of stems) ────────────────────────────────
        if flags == 0 {
            // Whole / Half / Quarter — no beams or flags needed
        } else if beam_right[i] || (i > 0 && beam_right[i - 1]) {
            // Part of a beam group — draw beams as thick horizontal lines at stem_bot_y
            if beam_right[i] {
                let nx = rhythm_notes[i + 1].x; // right note stem x
                let sx = cx;                     // this note stem x

                // How many beam lines to draw between this note and the right note
                let shared_flags = flags.min(rhythm_notes[i + 1].rhythm.flag_count());
                for f in 0..shared_flags {
                    let by = stem_bot_y - (f as f32) * flag_spacing;
                    painter.line_segment(
                        [Pos2::new(sx, by), Pos2::new(nx, by)],
                        Stroke::new(3.0, note_color),
                    );
                }

                // If this note has more flags than its right neighbour (e.g. 16th to 8th):
                let extra = flags.saturating_sub(rhythm_notes[i + 1].rhythm.flag_count());
                for f in shared_flags..shared_flags + extra {
                    let by = stem_bot_y - (f as f32) * flag_spacing;
                    let stub_w = event_spacing * 0.35;
                    painter.line_segment(
                        [Pos2::new(sx, by), Pos2::new(sx + stub_w, by)],
                        Stroke::new(3.0, note_color),
                    );
                }
            }

            // If this note is the end of a beam group and has extra flags compared to its left neighbour:
            if i > 0 && beam_right[i - 1] && !beam_right[i] {
                let prev_flags = rhythm_notes[i - 1].rhythm.flag_count();
                if flags > prev_flags {
                    let extra = flags - prev_flags;
                    let sx = cx;
                    for f in prev_flags..prev_flags + extra {
                        let by = stem_bot_y - (f as f32) * flag_spacing;
                        let stub_w = event_spacing * 0.35;
                        painter.line_segment(
                            [Pos2::new(sx - stub_w, by), Pos2::new(sx, by)],
                            Stroke::new(3.0, note_color),
                        );
                    }
                }
            }
        } else {
            // Isolated note — draw flag(s) attached at stem_bot_y angling up towards staff
            let sx = cx;
            for f in 0..flags {
                let flag_y_bot = stem_bot_y - (f as f32) * flag_spacing;
                let flag_y_top = flag_y_bot - flag_spacing - 2.0;
                let flag_w     = event_spacing * 0.45;
                painter.line_segment(
                    [Pos2::new(sx, flag_y_bot), Pos2::new(sx + flag_w, flag_y_bot - flag_spacing * 0.5)],
                    Stroke::new(1.5, note_color),
                );
                painter.line_segment(
                    [Pos2::new(sx + flag_w, flag_y_bot - flag_spacing * 0.5), Pos2::new(sx + flag_w * 0.4, flag_y_top)],
                    Stroke::new(1.5, note_color),
                );
            }
        }
    }

    response
}
