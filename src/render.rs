use crate::mnx::{Global, Part, Pitch, TimeSignature};
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

// ── SMuFL Time Signature helpers ─────────────────────────────────────────────

fn smufl_time_sig_digits(num: u32) -> String {
    num.to_string()
        .chars()
        .map(|c| match c {
            '0' => '\u{E080}',
            '1' => '\u{E081}',
            '2' => '\u{E082}',
            '3' => '\u{E083}',
            '4' => '\u{E084}',
            '5' => '\u{E085}',
            '6' => '\u{E086}',
            '7' => '\u{E087}',
            '8' => '\u{E088}',
            '9' => '\u{E089}',
            _ => c,
        })
        .collect()
}

fn draw_time_signature(
    painter: &egui::Painter,
    time_sig: &TimeSignature,
    left_x: f32,
    top_string_y: f32,
    bottom_string_y: f32,
    color: Color32,
) -> f32 {
    let staff_center_y = (top_string_y + bottom_string_y) / 2.0;

    // In SMuFL Bravura, time signature numerals are 2 staff-spaces tall.
    let font_size = 46.0_f32;
    let bravura_font = FontId::new(font_size, egui::FontFamily::Name("Bravura".into()));

    let num_text = smufl_time_sig_digits(time_sig.count);
    let den_text = smufl_time_sig_digits(time_sig.unit);

    let num_galley = painter.layout_no_wrap(num_text.clone(), bravura_font.clone(), color);
    let den_galley = painter.layout_no_wrap(den_text.clone(), bravura_font.clone(), color);

    let width = num_galley.size().x.max(den_galley.size().x);
    let center_x = left_x + width / 2.0;
    // offset from the true center by a small pad
    let center_pad = font_size / 4.0 + 0.25;

    // Numerator: bottom sits at center minus small pad
    painter.text(
        Pos2::new(center_x, staff_center_y - center_pad),
        egui::Align2::CENTER_CENTER,
        &num_text,
        bravura_font.clone(),
        color,
    );

    // Denominator: top sits at center plus small pad
    painter.text(
        Pos2::new(center_x, staff_center_y + center_pad),
        egui::Align2::CENTER_CENTER,
        &den_text,
        bravura_font,
        color,
    );

    width
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
    #[allow(dead_code)]
    fn beats(self) -> f64 {
        match self {
            Rhythm::Whole => 4.0,
            Rhythm::Half => 2.0,
            Rhythm::Quarter => 1.0,
            Rhythm::Eighth => 0.5,
            Rhythm::Sixteenth => 0.25,
            Rhythm::ThirtySecond => 0.125,
        }
    }

    /// Number of beam/flag lines needed (0 for whole/half/quarter).
    fn flag_count(self) -> usize {
        match self {
            Rhythm::Whole | Rhythm::Half | Rhythm::Quarter => 0,
            Rhythm::Eighth => 1,
            Rhythm::Sixteenth => 2,
            Rhythm::ThirtySecond => 3,
        }
    }
}

fn parse_rhythm(base: &str) -> (Rhythm, bool) {
    let s = base.to_lowercase();
    // dotted: the spec might say "dotted-quarter" or similar
    let is_dotted = s.contains("dotted") || s.contains('.');
    let r = if s.contains("whole") {
        Rhythm::Whole
    } else if s.contains("half") {
        Rhythm::Half
    } else if s.contains("32nd") || s.contains("thirtysecond") {
        Rhythm::ThirtySecond
    } else if s.contains("16th") || s.contains("sixteenth") {
        Rhythm::Sixteenth
    } else if s.contains("eighth") {
        Rhythm::Eighth
    } else {
        Rhythm::Quarter
    };
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

pub struct TablatureOptions<'a> {
    pub part: &'a Part,
    pub global: Option<&'a Global>,
    pub errors: &'a [ValidationError],
    pub selected_event: Option<usize>,
    pub selected_note_in_event: Option<usize>,
    pub is_editing_fret: bool,
    pub editing_fret_text: &'a str,
    pub editing_string: Option<u8>,
}

pub fn draw_tablature(
    ui: &mut egui::Ui,
    opts: TablatureOptions<'_>,
) -> (egui::Response, Option<(usize, Option<usize>)>, bool) {
    let part = opts.part;
    let global = opts.global;
    let errors = opts.errors;
    let selected_event = opts.selected_event;
    let selected_note_in_event = opts.selected_note_in_event;
    let is_editing_fret = opts.is_editing_fret;
    let editing_fret_text = opts.editing_fret_text;
    let editing_string = opts.editing_string;

    let num_strings = part.num_strings();
    if num_strings == 0 {
        return (ui.label("No strings found."), None, false);
    }

    // ── Layout constants ─────────────────────────────────────────────────────

    let line_spacing = 24.0_f32; // vertical gap between string lines
    let event_spacing = 30.0_f32; // horizontal gap between events
    let measure_spacing = 20.0_f32; // extra gap after a bar line
    let margin_left = 10.0_f32;
    let label_gap = 8.0_f32;
    let first_bar_pad = 25.0_f32; // breathing room after the opening barline

    // Rhythm row sits below the staff
    let rhythm_gap = 14.0_f32; // gap between last string line and stem top
    let stem_height = 22.0_f32;
    let flag_spacing = 4.5_f32; // vertical distance between flags

    // ── Font ─────────────────────────────────────────────────────────────────

    let font_id = FontId::monospace(14.0);

    // ── String labels ────────────────────────────────────────────────────────

    let mut prefixes: Vec<String> = (1..=num_strings).map(|i| i.to_string()).collect();
    if let Some(inst) = &part.instrument
        && let Some(tuning) = &inst.tuning
    {
        for t in tuning {
            if t.string >= 1 && (t.string as usize) <= num_strings {
                prefixes[(t.string - 1) as usize] = format_pitch(&t.pitch);
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

    // ── Time signatures to render ─────────────────────────────────────────────

    let mut num_rendered_time_sigs = 0;
    let mut current_ts_check: Option<TimeSignature> = None;
    for m_idx in 0..part.measures.len() {
        let m_ts = global
            .and_then(|g| g.measures.get(m_idx))
            .and_then(|m| m.time);
        if m_idx == 0 {
            if let Some(ts) = m_ts {
                num_rendered_time_sigs += 1;
                current_ts_check = Some(ts);
            }
        } else if let Some(ts) = m_ts
            && current_ts_check != Some(ts)
        {
            num_rendered_time_sigs += 1;
            current_ts_check = Some(ts);
        }
    }

    // ── Canvas size ───────────────────────────────────────────────────────────

    let first_barline_offset = margin_left + max_prefix_width + label_gap;
    let start_offset_x = first_barline_offset + first_bar_pad;

    let staff_height = (num_strings as f32 - 1.0).max(0.0) * line_spacing;
    let rhythm_row_h = rhythm_gap + stem_height + 10.0; // stem + flags/dots + pad
    let top_pad = 48.0_f32; // room for name + EDO label
    let total_height = top_pad + staff_height + rhythm_row_h + 10.0; // top pad + staff + rhythm + bottom pad
    let total_width = start_offset_x
        + (total_events as f32) * event_spacing
        + (part.measures.len() as f32) * measure_spacing
        + (num_rendered_time_sigs as f32) * 75.0
        + 80.0;

    // ── Allocate painter ──────────────────────────────────────────────────────

    let (response, painter) =
        ui.allocate_painter(Vec2::new(total_width, total_height), egui::Sense::click());
    let rect = response.rect;

    let first_barline_x = rect.min.x + first_barline_offset;
    let label_x = first_barline_x - label_gap;

    // ── Instrument name + EDO header ──────────────────────────────────────────

    if let Some(inst) = &part.instrument {
        let header_x = rect.min.x + margin_left;
        let name_y = rect.min.y + 14.0;
        let edo_y = rect.min.y + 32.0;

        // Bold instrument name
        painter.text(
            Pos2::new(header_x, name_y),
            egui::Align2::LEFT_CENTER,
            &inst.name,
            FontId::new(20.0, egui::FontFamily::Proportional),
            ui.visuals().strong_text_color(),
        );

        // Plain "N EDO" subtitle
        if let Some(divisions) = inst.micro_fret_divisions {
            painter.text(
                Pos2::new(header_x, edo_y),
                egui::Align2::LEFT_CENTER,
                format!("{} EDO", divisions),
                FontId::new(15.0, egui::FontFamily::Proportional),
                Color32::from_gray(160),
            );
        }
    }

    // ── Draw string lines and labels ──────────────────────────────────────────

    let mut string_y_coords = Vec::with_capacity(num_strings);
    for i in 0..num_strings {
        let y = rect.min.y + top_pad + (i as f32) * line_spacing;
        string_y_coords.push(y);

        painter.line_segment(
            [
                Pos2::new(first_barline_x, y),
                Pos2::new(rect.max.x - 10.0, y),
            ],
            Stroke::new(1.0, Color32::from_gray(100)),
        );

        // String name / number label to the left of the opening barline
        let label_str = &prefixes[i];
        painter.text(
            Pos2::new(label_x, y),
            egui::Align2::RIGHT_CENTER,
            label_str,
            font_id.clone(),
            Color32::from_gray(180),
        );
    }

    // The bottom of the last string line — rhythms sit below here.
    let last_string_y = *string_y_coords.last().unwrap();
    let stem_top_y = last_string_y + rhythm_gap;
    let stem_bot_y = stem_top_y + stem_height;

    // ── Error coordinate set ──────────────────────────────────────────────────

    let error_coords: std::collections::HashSet<_> = errors
        .iter()
        .filter(|e| e.string > 0)
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

    let bg_color = ui.visuals().panel_fill;
    let note_color = ui.visuals().text_color();
    let highlight_color = Color32::from_rgb(100, 180, 255); // Light blue
    let edit_color = Color32::from_rgb(255, 175, 75); // Light orange

    let mut current_x = first_barline_x;
    let mut current_time_sig: Option<TimeSignature> = None;

    // Render time signature at start of measure 0 if specified
    let m0_ts = global.and_then(|g| g.measures.first()).and_then(|m| m.time);
    if let Some(ts) = m0_ts {
        current_time_sig = Some(ts);
        let left_pad = 12.0_f32;
        let right_pad = 16.0_f32;
        let ts_left_x = current_x + left_pad;
        let ts_w = draw_time_signature(
            &painter,
            &ts,
            ts_left_x,
            string_y_coords[0],
            last_string_y,
            note_color,
        );
        current_x = ts_left_x + ts_w + right_pad;
    } else {
        current_x += first_bar_pad;
    }

    // Collect rhythm notes so we can do beam-grouping after the main loop.
    let mut rhythm_notes: Vec<RhythmNote> = Vec::new();
    // Hit boxes for each event: (fret_rects: Vec<(Rect, usize)>, stem_rect, col_rect)
    type HitBoxInfo = (Vec<(Rect, usize)>, Rect, Rect);
    let mut event_hit_boxes: Vec<HitBoxInfo> = Vec::new();

    let mut flat_event_idx = 0;

    // ── Main event loop ───────────────────────────────────────────────────────

    for (m_idx, measure) in part.measures.iter().enumerate() {
        if let Some(seq) = measure.sequences.first() {
            for (e_idx, event) in seq.content.iter().enumerate() {
                let is_selected = selected_event == Some(flat_event_idx);

                let col_rect = Rect::from_min_max(
                    Pos2::new(current_x - 12.0, string_y_coords[0] - 6.0),
                    Pos2::new(current_x + 12.0, stem_bot_y + 6.0),
                );

                if is_selected {
                    let col_color = if is_editing_fret {
                        Color32::from_rgba_unmultiplied(255, 175, 75, 35)
                    } else {
                        Color32::from_rgba_unmultiplied(100, 180, 255, 35)
                    };
                    painter.rect_filled(col_rect, 4.0, col_color);
                    ui.scroll_to_rect(col_rect, None);
                }

                // Fret numbers on strings: store (fret, note_idx)
                let mut step_frets: Vec<Option<(u8, usize)>> = vec![None; num_strings];
                for (n_idx, note) in event.notes.iter().enumerate() {
                    if let (Some(s), Some(f)) = (note.string, note.fret) {
                        if s >= 1 && (s as usize) <= num_strings {
                            step_frets[(s - 1) as usize] = Some((f, n_idx));
                        }
                    }
                }

                let mut current_fret_rects = Vec::new();

                for i in 0..num_strings {
                    let string_num = (i + 1) as u8;
                    let y = string_y_coords[i];

                    if let Some((fret, n_idx)) = step_frets[i] {
                        let is_err = error_coords.contains(&(m_idx + 1, e_idx, string_num, fret));
                        let is_this_string_editing =
                            is_selected && is_editing_fret && editing_string == Some(string_num);
                        let is_this_note_selected = !is_editing_fret
                            && is_selected
                            && (selected_note_in_event.is_none()
                                || selected_note_in_event == Some(n_idx));

                        let text_color = if is_err {
                            Color32::RED
                        } else if is_this_string_editing || is_this_note_selected {
                            Color32::BLACK
                        } else {
                            note_color
                        };

                        let display_text = if is_this_string_editing {
                            if editing_fret_text.is_empty() {
                                fret.to_string()
                            } else {
                                editing_fret_text.to_string()
                            }
                        } else {
                            fret.to_string()
                        };

                        let galley =
                            painter.layout_no_wrap(display_text, font_id.clone(), text_color);
                        let text_rect =
                            Rect::from_center_size(Pos2::new(current_x, y), galley.size());

                        if is_this_string_editing {
                            painter.rect_filled(text_rect.expand(3.5), 3.0, edit_color);
                        } else if is_this_note_selected {
                            painter.rect_filled(text_rect.expand(3.5), 3.0, highlight_color);
                        } else if is_selected {
                            // Subtly frame other notes in the selected chord
                            painter.rect_filled(
                                text_rect.expand(2.5),
                                2.0,
                                Color32::from_rgba_unmultiplied(100, 180, 255, 60),
                            );
                        } else {
                            painter.rect_filled(text_rect.expand(2.0), 0.0, bg_color);
                        }
                        painter.galley(text_rect.min, galley, text_color);
                        current_fret_rects.push((text_rect, n_idx));
                    } else if is_selected && is_editing_fret && editing_string == Some(string_num) {
                        // Empty string currently focused for placement in edit mode
                        let display_text = if editing_fret_text.is_empty() {
                            "_".to_string()
                        } else {
                            editing_fret_text.to_string()
                        };
                        let galley =
                            painter.layout_no_wrap(display_text, font_id.clone(), Color32::BLACK);
                        let text_rect =
                            Rect::from_center_size(Pos2::new(current_x, y), galley.size());
                        painter.rect_filled(text_rect.expand(3.5), 3.0, edit_color);
                        painter.galley(text_rect.min, galley, Color32::BLACK);
                    }
                }

                let stem_rect = Rect::from_min_max(
                    Pos2::new(current_x - 10.0, stem_top_y - 2.0),
                    Pos2::new(current_x + 10.0, stem_bot_y + 6.0),
                );
                event_hit_boxes.push((current_fret_rects, stem_rect, col_rect));

                // Collect rhythm info for drawing after loop
                let (rhythm, mut is_dotted) = parse_rhythm(&event.duration.base);
                if event.duration.dots.unwrap_or(0) > 0 {
                    is_dotted = true;
                }
                rhythm_notes.push(RhythmNote {
                    x: current_x,
                    rhythm,
                    is_dotted,
                    measure_idx: m_idx,
                });

                flat_event_idx += 1;
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

        if m_idx + 1 < part.measures.len() {
            let next_ts = global
                .and_then(|g| g.measures.get(m_idx + 1))
                .and_then(|m| m.time);
            if let Some(ts) = next_ts {
                if current_time_sig != Some(ts) {
                    current_time_sig = Some(ts);
                    let left_pad = 6.0_f32;
                    let right_pad = 16.0_f32;
                    let ts_left_x = current_x + left_pad;
                    let ts_w = draw_time_signature(
                        &painter,
                        &ts,
                        ts_left_x,
                        string_y_coords[0],
                        last_string_y,
                        note_color,
                    );
                    current_x = ts_left_x + ts_w + right_pad;
                } else {
                    current_x += measure_spacing;
                }
            } else {
                current_x += measure_spacing;
            }
        }
    }

    // ── Rhythm notation ───────────────────────────────────────────────────────
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
        let is_selected = selected_event == Some(i);
        let this_stem_color = if is_selected {
            highlight_color
        } else {
            note_color
        };
        let stem_thickness = if is_selected { 2.5 } else { 1.5 };

        // ── Stem ──────────────────────────────────────────────────────────────
        if note.rhythm != Rhythm::Whole {
            painter.line_segment(
                [Pos2::new(cx, stem_top_y), Pos2::new(cx, stem_bot_y)],
                Stroke::new(stem_thickness, this_stem_color),
            );
        }

        // ── Dot (augmentation) ────────────────────────────────────────────────
        if note.is_dotted {
            painter.circle_filled(Pos2::new(cx + 5.0, stem_bot_y - 2.0), 1.5, this_stem_color);
        }

        // ── Flags or beams (at bottom of stems) ────────────────────────────────
        if flags == 0 {
            // Whole / Half / Quarter — no beams or flags needed
        } else if beam_right[i] || (i > 0 && beam_right[i - 1]) {
            // Part of a beam group — draw beams as thick horizontal lines at stem_bot_y
            if beam_right[i] {
                let nx = rhythm_notes[i + 1].x; // right note stem x
                let sx = cx; // this note stem x

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
                        Stroke::new(3.0, this_stem_color),
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
                            Stroke::new(3.0, this_stem_color),
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
                let flag_w = event_spacing * 0.45;
                painter.line_segment(
                    [
                        Pos2::new(sx, flag_y_bot),
                        Pos2::new(sx + flag_w, flag_y_bot - flag_spacing * 0.5),
                    ],
                    Stroke::new(if is_selected { 2.0 } else { 1.5 }, this_stem_color),
                );
                painter.line_segment(
                    [
                        Pos2::new(sx + flag_w, flag_y_bot - flag_spacing * 0.5),
                        Pos2::new(sx + flag_w * 0.4, flag_y_top),
                    ],
                    Stroke::new(if is_selected { 2.0 } else { 1.5 }, this_stem_color),
                );
            }
        }
    }

    // ── Floating "Add Bar" Button (| + |) after final barline ──────────────────
    let btn_x = current_x + 6.0;
    let btn_w = 32.0;
    let btn_top_y = string_y_coords[0];
    let btn_bot_y = last_string_y;
    let btn_rect = Rect::from_min_max(
        Pos2::new(btn_x, btn_top_y),
        Pos2::new(btn_x + btn_w, btn_bot_y),
    );

    let hover_pos = ui.input(|i| i.pointer.hover_pos());
    let is_btn_hovered = hover_pos.is_some_and(|pos| btn_rect.expand(5.0).contains(pos));

    if is_btn_hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        egui::Area::new(egui::Id::new("add_bar_tooltip"))
            .fixed_pos(Pos2::new(btn_x + btn_w / 2.0 - 50.0, btn_top_y - 28.0))
            .order(egui::Order::Tooltip)
            .interactable(false)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(egui::RichText::new("Add measure (Shift+B)").small());
                });
            });
    }

    let (btn_stroke, btn_bg) = if is_btn_hovered {
        (
            Stroke::new(1.8, ui.visuals().strong_text_color()),
            Color32::from_rgba_unmultiplied(100, 180, 255, 35),
        )
    } else {
        (
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(140, 140, 140, 75)),
            Color32::from_rgba_unmultiplied(150, 150, 150, 10),
        )
    };

    // Draw subtle button background
    painter.rect_filled(btn_rect.expand(2.0), 3.0, btn_bg);

    // Draw left and right vertical lines | and |
    painter.line_segment(
        [Pos2::new(btn_x, btn_top_y), Pos2::new(btn_x, btn_bot_y)],
        btn_stroke,
    );
    painter.line_segment(
        [
            Pos2::new(btn_x + btn_w, btn_top_y),
            Pos2::new(btn_x + btn_w, btn_bot_y),
        ],
        btn_stroke,
    );

    // Draw + sign in center
    let center = btn_rect.center();
    let plus_size = 5.0;
    painter.line_segment(
        [
            Pos2::new(center.x - plus_size, center.y),
            Pos2::new(center.x + plus_size, center.y),
        ],
        btn_stroke,
    );
    painter.line_segment(
        [
            Pos2::new(center.x, center.y - plus_size),
            Pos2::new(center.x, center.y + plus_size),
        ],
        btn_stroke,
    );

    // ── Click interaction ─────────────────────────────────────────────────────
    let mut clicked_selection = None;
    let mut add_bar_clicked = false;
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if btn_rect.expand(5.0).contains(pos) {
                add_bar_clicked = true;
            } else {
                for (idx, (fret_rects, stem_rect, col_rect)) in event_hit_boxes.iter().enumerate() {
                    // Check if directly clicked on a specific fret number
                    let mut clicked_note = None;
                    for (r, n_idx) in fret_rects {
                        if r.expand(4.0).contains(pos) {
                            clicked_note = Some(*n_idx);
                            break;
                        }
                    }

                    if let Some(n_idx) = clicked_note {
                        clicked_selection = Some((idx, Some(n_idx)));
                        break;
                    } else if stem_rect.contains(pos) || col_rect.contains(pos) {
                        clicked_selection = Some((idx, None));
                        break;
                    }
                }
            }
        }
    }

    (response, clicked_selection, add_bar_clicked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smufl_time_sig_digits() {
        assert_eq!(smufl_time_sig_digits(4), "\u{E084}");
        assert_eq!(smufl_time_sig_digits(3), "\u{E083}");
        assert_eq!(smufl_time_sig_digits(12), "\u{E081}\u{E082}");
        assert_eq!(smufl_time_sig_digits(15), "\u{E081}\u{E085}");
        assert_eq!(smufl_time_sig_digits(8), "\u{E088}");
        assert_eq!(smufl_time_sig_digits(16), "\u{E081}\u{E086}");
    }
}
