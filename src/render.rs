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

    /// Does this value have a filled note-head?
    fn is_filled(self) -> bool {
        !matches!(self, Rhythm::Whole | Rhythm::Half)
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
    let head_rx        =  4.5_f32;  // half-width of note head ellipse
    let head_ry        =  3.0_f32;  // half-height
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
    let rhythm_row_h   = rhythm_gap + stem_height + head_ry * 2.0 + 6.0; // stem + head + dot
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
    let head_center_y = stem_bot_y + head_ry;

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
                rhythm_notes.push(RhythmNote { x: current_x, rhythm, is_dotted });

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
    //  • Every note gets a vertical stem (whole notes get none)
    //  • Whole = open oval, no stem
    //  • Half  = open oval + stem
    //  • Quarter+ = filled oval + stem
    //  • Flags / beams: eighth gets 1, 16th gets 2, 32nd gets 3.
    //    We beam consecutive sub-quarter notes together (replace individual
    //    flags with a shared thick horizontal beam between them).

    // First identify beam groups: runs of notes with flag_count >= 1.
    // A beam group shares a single thick line per flag level.
    let n = rhythm_notes.len();

    // For each note, record whether it is beamed to its right neighbour.
    // Simple rule: beam if both notes have flags and are adjacent in the slice.
    let mut beam_right = vec![false; n];
    for i in 0..n.saturating_sub(1) {
        let a = &rhythm_notes[i];
        let b = &rhythm_notes[i + 1];
        if a.rhythm.flag_count() >= 1 && b.rhythm.flag_count() >= 1 {
            beam_right[i] = true;
        }
    }

    for (i, note) in rhythm_notes.iter().enumerate() {
        let cx = note.x;
        let flags = note.rhythm.flag_count();

        // ── Stem ──────────────────────────────────────────────────────────────
        if note.rhythm != Rhythm::Whole {
            painter.line_segment(
                [Pos2::new(cx + head_rx, stem_top_y), Pos2::new(cx + head_rx, stem_bot_y)],
                Stroke::new(1.5, note_color),
            );
        }

        // ── Note head ─────────────────────────────────────────────────────────
        if note.rhythm.is_filled() {
            // Filled ellipse
            painter.circle_filled(
                Pos2::new(cx, head_center_y),
                head_rx,  // approximate with circle; looks fine at this size
                note_color,
            );
        } else {
            // Open ellipse — draw outline circle
            painter.circle_stroke(
                Pos2::new(cx, head_center_y),
                head_rx,
                Stroke::new(1.5, note_color),
            );
        }

        // ── Dot (augmentation) ────────────────────────────────────────────────
        if note.is_dotted {
            painter.circle_filled(
                Pos2::new(cx + head_rx + 4.0, head_center_y - 2.0),
                1.5,
                note_color,
            );
        }

        // ── Flags or beams ────────────────────────────────────────────────────
        if flags == 0 {
            // nothing more
        } else if beam_right[i] || (i > 0 && beam_right[i - 1]) {
            // Part of a beam group — draw beams as thick horizontal lines
            // between this note and the next (only when beam_right[i] is true,
            // i.e., we are the left side of a beam).
            if beam_right[i] {
                let nx = rhythm_notes[i + 1].x + head_rx; // right note's stem x
                let sx = cx + head_rx;                     // this note's stem x

                // How many beam lines to draw = min of the two notes' flag counts
                let shared_flags = flags.min(rhythm_notes[i + 1].rhythm.flag_count());
                for f in 0..shared_flags {
                    let by = stem_top_y + (f as f32) * flag_spacing;
                    painter.line_segment(
                        [Pos2::new(sx, by), Pos2::new(nx, by)],
                        Stroke::new(3.0, note_color),
                    );
                }

                // If this note has *more* flags than its right neighbour it also
                // gets a partial stub flag on the right side of its own stem.
                let extra = flags.saturating_sub(rhythm_notes[i + 1].rhythm.flag_count());
                for f in shared_flags..shared_flags + extra {
                    let by = stem_top_y + (f as f32) * flag_spacing;
                    let stub_w = event_spacing * 0.35;
                    painter.line_segment(
                        [Pos2::new(sx, by), Pos2::new(sx + stub_w, by)],
                        Stroke::new(3.0, note_color),
                    );
                }
            }
            // Left-neighbour was already handled by beam_right[i-1] pass.
        } else {
            // Isolated note — draw curly flag(s) as angled lines from stem tip.
            let sx = cx + head_rx;
            for f in 0..flags {
                let flag_y_top = stem_top_y + (f as f32) * flag_spacing;
                let flag_y_bot = flag_y_top + flag_spacing + 4.0;
                let flag_w     = event_spacing * 0.45;
                // Two-segment flag curve approximation: straight then hooking down
                painter.line_segment(
                    [Pos2::new(sx, flag_y_top), Pos2::new(sx + flag_w, flag_y_top + flag_spacing * 0.6)],
                    Stroke::new(1.5, note_color),
                );
                painter.line_segment(
                    [Pos2::new(sx + flag_w, flag_y_top + flag_spacing * 0.6), Pos2::new(sx + flag_w * 0.4, flag_y_bot)],
                    Stroke::new(1.5, note_color),
                );
            }
        }
    }

    response
}
