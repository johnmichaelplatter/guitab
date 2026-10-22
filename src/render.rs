use crate::mnx::{Global, Part, Pitch, TimeSignature};
use crate::validate::ValidationError;
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Stroke, Vec2};

// ── Pitch helpers ────────────────────────────────────────────────────────────

pub fn format_pitch(pitch: &Pitch, edo: u32) -> String {
    let mut prefix = String::new();
    let mut suffix = String::new();

    if let Some(alter) = &pitch.alter {
        let mut alter_val = 0;
        let alter_str = alter.to_string();
        if let Some(pos) = alter_str.find('\\') {
            if let Ok(v) = alter_str[..pos].parse::<i32>() {
                alter_val = v;
            }
        } else if let Ok(v) = alter_str.parse::<f64>() {
            alter_val = v as i32;
        }

        let tuning = crate::edo::EdoTuning::new(edo);
        let sharp_steps = tuning.sharp_steps;

        // Find best combination of sharps/flats and up/down
        // Similar to steps_to_pitch logic
        let mut best_score = i32::MAX;
        let mut best_sharps = 0;
        let mut best_ups = 0;

        for sharps in -2..=2 {
            let ups = alter_val - sharps * sharp_steps;
            let score = sharps.abs() * 2 + ups.abs() * 3;
            if score < best_score {
                best_score = score;
                best_sharps = sharps;
                best_ups = ups;
            }
        }

        if best_ups > 0 {
            prefix.push_str(&"^".repeat(best_ups as usize));
        } else if best_ups < 0 {
            prefix.push_str(&"v".repeat((-best_ups) as usize));
        }

        if best_sharps > 0 {
            suffix.push_str(&"#".repeat(best_sharps as usize));
        } else if best_sharps < 0 {
            suffix.push_str(&"b".repeat((-best_sharps) as usize));
        }
    }

    format!("{}{}{}", prefix, pitch.step, suffix)
}

// ── SMuFL Time Signature helpers ─────────────────────────────────────────────

pub(crate) fn smufl_time_sig_digits(num: u32) -> String {
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
pub enum Rhythm {
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
    pub fn beats(self) -> f64 {
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
    pub fn flag_count(self) -> usize {
        match self {
            Rhythm::Whole | Rhythm::Half | Rhythm::Quarter => 0,
            Rhythm::Eighth => 1,
            Rhythm::Sixteenth => 2,
            Rhythm::ThirtySecond => 3,
        }
    }
}

pub(crate) fn parse_rhythm(base: &str) -> (Rhythm, bool) {
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

/// Calculates duration spacing weight multiplier.
/// - Quarter note (1.0 beats) = 1.0 (baseline)
/// - Half note (2.0 beats) = 1.5 (+50% vs quarter note)
/// - Whole note (4.0 beats) = 1.7 (+70% vs quarter note)
/// - Eighth note (0.5 beats) = 0.85
/// - 16th note (0.25 beats) = 0.75
pub fn duration_spacing_weight(beats: f64) -> f32 {
    if beats <= 0.0 {
        return 1.0;
    }
    if beats >= 1.0 {
        if beats <= 2.0 {
            1.0 + 0.50 * (beats as f32 - 1.0)
        } else if beats <= 4.0 {
            1.50 + 0.10 * (beats as f32 - 2.0)
        } else {
            (1.70 + 0.0375 * (beats as f32 - 4.0)).min(2.0)
        }
    } else if beats >= 0.5 {
        (1.0 + 0.30 * (beats as f32 - 1.0)).max(0.85)
    } else if beats >= 0.25 {
        (0.85 + 0.40 * (beats as f32 - 0.5)).max(0.75)
    } else {
        (0.75 + 0.533 * (beats as f32 - 0.25)).max(0.60)
    }
}

/// Computes horizontal spacing for an event based on its duration.
pub fn duration_spacing(duration: &crate::mnx::Duration, base_spacing: f32) -> f32 {
    let beats = crate::validate::duration_to_quarter_beats(duration);
    base_spacing * duration_spacing_weight(beats)
}

/// Formats a harmonic touch location for tablature display, enclosed in angle brackets.
/// Integers (or values very close to an integer) are formatted without decimals (e.g. "<12>"),
/// while fractional touches are formatted with one decimal place (e.g. "<3.9>").
pub fn format_harmonic_touch(touch: f64) -> String {
    if (touch - touch.round()).abs() < 1e-4 {
        format!("<{:.0}>", touch.round())
    } else {
        format!("<{:.1}>", touch)
    }
}

/// Formats an ending's numbers for tablature display (e.g. `1.` or `1, 2.`).
pub fn format_ending_numbers(numbers: &[u32]) -> String {
    if numbers.is_empty() {
        return String::new();
    }
    let nums_str = numbers
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!("{}.", nums_str)
}

/// Returns the maximum displayed text width of an event's note labels in tablature.
pub fn event_text_width(
    event: &crate::mnx::Event,
    in_grace: bool,
    editing_len: Option<usize>,
) -> f32 {
    let char_w = if in_grace { 6.05_f32 } else { 8.45_f32 };
    let mut max_chars = editing_len.unwrap_or(0);
    for note in &event.notes {
        let len = if let Some(harmonic) = &note.harmonic {
            format_harmonic_touch(harmonic.touch).len()
        } else if note.muted.unwrap_or(false) {
            1
        } else {
            note.fret.map(|f| f.to_string().len()).unwrap_or(1)
        };
        max_chars = max_chars.max(len);
    }
    if max_chars == 0 {
        if event.rest.is_some() {
            14.0
        } else {
            char_w
        }
    } else {
        (max_chars as f32) * char_w
    }
}

/// Returns the estimated pixel width of the longest lyric syllable on this event.
pub fn event_lyric_width(event: &crate::mnx::Event) -> f32 {
    if let Some(lyrics) = &event.lyrics {
        let max_chars = lyrics
            .lines
            .values()
            .map(|l| l.text.chars().count())
            .max()
            .unwrap_or(0);
        if max_chars > 0 {
            // Proportional font ~13pt average character width ~7.5px plus margin
            (max_chars as f32) * 7.5 + 4.0
        } else {
            0.0
        }
    } else {
        0.0
    }
}

/// Calculates the horizontal advancement spacing following an event in tablature.
/// Accounts for:
/// - Duration spacing weight
/// - Grace note scaling
/// - Pitch bends
/// - Collision prevention between wide text entries (e.g. natural harmonics like "<20.5>", double digits)
/// - Collision prevention between lyric syllables (rod stretching for wide syllables)
/// - Barline clearance if this is the last event in a measure
pub fn calculate_event_spacing(
    event: &crate::mnx::Event,
    in_grace: bool,
    base_event_spacing: f32,
    next_event_info: Option<(&crate::mnx::Event, bool)>,
    editing_len: Option<usize>,
    next_editing_len: Option<usize>,
) -> f32 {
    let scale = (base_event_spacing / 30.0).clamp(0.4, 1.0);
    let mut spacing = if in_grace {
        (duration_spacing(&event.duration, base_event_spacing) * 0.75).max(19.0 * scale)
    } else {
        duration_spacing(&event.duration, base_event_spacing)
    };

    if event.notes.iter().any(|n| n.bend.is_some()) {
        spacing = spacing.max(55.0 * scale);
    }

    let w1 = event_text_width(event, in_grace, editing_len);

    let min_collision_spacing = match next_event_info {
        Some((next_ev, next_in_grace)) => {
            let w2 = event_text_width(next_ev, next_in_grace, next_editing_len);
            let gap = if w1 > 8.5 || w2 > 8.5 {
                if in_grace && next_in_grace {
                    8.0 * scale
                } else {
                    10.0 * scale
                }
            } else {
                5.0 * scale
            };
            (w1 + w2) * 0.5 + 4.0 * scale + gap
        }
        None => {
            // End of measure before barline: ensure right edge and mask clear the barline
            w1 * 0.5 + 2.0 * scale + 10.0 * scale
        }
    };

    let lyr_w1 = event_lyric_width(event);
    let min_lyric_spacing = match next_event_info {
        Some((next_ev, _)) => {
            let lyr_w2 = event_lyric_width(next_ev);
            if lyr_w1 > 0.0 || lyr_w2 > 0.0 {
                let lyr_scale = (base_event_spacing / 30.0).clamp(0.65, 1.0);
                (lyr_w1 + lyr_w2) * 0.5 + 8.0 * lyr_scale
            } else {
                0.0
            }
        }
        None => {
            if lyr_w1 > 0.0 {
                let lyr_scale = (base_event_spacing / 30.0).clamp(0.65, 1.0);
                lyr_w1 * 0.5 + 6.0 * lyr_scale
            } else {
                0.0
            }
        }
    };

    spacing.max(min_collision_spacing).max(min_lyric_spacing)
}

// ── Collected note position for rhythm drawing ────────────────────────────────

#[derive(Clone, Debug)]
pub struct RhythmNote {
    pub x: f32,
    pub rhythm: Rhythm,
    pub is_dotted: bool,
    pub measure_idx: usize,
    pub is_rest: bool,
    pub tuplet_id: Option<usize>,
    pub start_beat: f64,
    #[allow(dead_code)]
    pub end_beat: f64,
}

#[derive(Clone, Debug)]
pub struct RenderedLyric {
    pub event_idx: usize,
    pub x: f32,
    pub text: String,
    pub syllabic: Option<String>,
    pub extender: Option<bool>,
    pub line: usize,
    pub is_editing: bool,
}

// ── Main draw function ────────────────────────────────────────────────────────

pub struct TablatureOptions<'a> {
    pub part: &'a Part,
    pub global: Option<&'a Global>,
    pub errors: &'a [ValidationError],
    pub selected_event: Option<usize>,
    pub selection_anchor: Option<usize>,
    pub selected_note_in_event: Option<usize>,
    pub is_editing_fret: bool,
    pub editing_fret_text: &'a str,
    pub editing_string: Option<u8>,
    pub is_editing_lyric: bool,
    pub editing_lyric_text: &'a str,
    pub scroll_to_selected: bool,
    pub pitch_bend_range: u8,
}

pub fn draw_tablature(
    ui: &mut egui::Ui,
    opts: TablatureOptions<'_>,
) -> (egui::Response, Option<(usize, Option<usize>)>, bool, Option<(usize, usize)>) {
    let part = opts.part;
    let global = opts.global;
    let errors = opts.errors;
    let selected_event = opts.selected_event;
    let selected_note_in_event = opts.selected_note_in_event;
    let is_editing_fret = opts.is_editing_fret;
    let editing_fret_text = opts.editing_fret_text;
    let editing_string = opts.editing_string;
    let pitch_bend_range = opts.pitch_bend_range;

    let num_strings = part.num_strings();
    if num_strings == 0 {
        return (ui.label("No strings found."), None, false, None);
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
    if let Some(tuning) = &part.tuning {
        for t in tuning {
            if t.string >= 1 && (t.string as usize) <= num_strings {
                prefixes[(t.string - 1) as usize] = format_pitch(&t.pitch, part.edo());
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

    // ── Total events width ────────────────────────────────────────────────────

    let total_events_width: f32 = {
        let mut total = 0.0_f32;
        for (m_idx, m) in part.measures.iter().enumerate() {
            if let Some(seq) = m.sequences.first() {
                let mut layout_items = Vec::new();
                for item in &seq.content {
                    item.to_layout_items(&mut layout_items);
                }

                let mut first_ev_info = None;
                let mut temp_grace = false;
                for item in &layout_items {
                    match item {
                        LayoutItem::GraceStart => temp_grace = true,
                        LayoutItem::GraceEnd => temp_grace = false,
                        LayoutItem::Event(e) => {
                            first_ev_info = Some((*e, temp_grace));
                            break;
                        }
                        _ => {}
                    }
                }
                if let Some((first_ev, first_grace)) = first_ev_info {
                    let w_first = event_text_width(first_ev, first_grace, None);
                    let def_pad = if m_idx == 0 { first_bar_pad } else { measure_spacing };
                    let extra_start = (w_first * 0.5 + 10.0 - def_pad).max(0.0);
                    total += extra_start;
                }

                let mut in_grace = false;
                for (item_idx, l_item) in layout_items.iter().enumerate() {
                    match l_item {
                        LayoutItem::GraceStart => in_grace = true,
                        LayoutItem::GraceEnd => in_grace = false,
                        LayoutItem::Event(ev) => {
                            let mut next_ev_info = None;
                            let mut look_grace = in_grace;
                            for next_item in &layout_items[item_idx + 1..] {
                                match next_item {
                                    LayoutItem::GraceStart => look_grace = true,
                                    LayoutItem::GraceEnd => look_grace = false,
                                    LayoutItem::Event(ne) => {
                                        next_ev_info = Some((*ne, look_grace));
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                            let sp = calculate_event_spacing(
                                ev,
                                in_grace,
                                event_spacing,
                                next_ev_info,
                                None,
                                None,
                            );
                            total += sp;
                        }
                        _ => {}
                    }
                }
            }
        }
        total
    };

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
    let has_endings = global
        .map(|g| g.measures.iter().any(|m| m.ending.is_some()))
        .unwrap_or(false);
    let has_tempos = global
        .map(|g| g.measures.iter().any(|m| !m.tempos.is_empty() || !m.tempo_changes.is_empty()))
        .unwrap_or(false);
    let max_lyric_lines = part.measures.iter().map(|m| {
        m.sequences.iter().map(|s| {
            s.flattened_events().iter().map(|e| {
                e.lyrics.as_ref().map(|l| {
                    l.lines.keys().filter_map(|k| k.parse::<usize>().ok()).max().unwrap_or(l.lines.len())
                }).unwrap_or(0)
            }).max().unwrap_or(0)
        }).max().unwrap_or(0)
    }).max().unwrap_or(0);
    let effective_lines = if opts.is_editing_lyric { max_lyric_lines.max(1) } else { max_lyric_lines };
    let lyrics_row_h = if effective_lines > 0 {
        (effective_lines as f32) * 18.0 + 10.0
    } else {
        0.0
    };

    let top_pad = if has_endings && has_tempos {
        130.0_f32
    } else if has_endings {
        108.0_f32
    } else {
        96.0_f32
    }; // room for name + EDO label + voltas + tempos + slurs on top string
    let total_height = top_pad + staff_height + rhythm_row_h + lyrics_row_h + 10.0; // top pad + staff + rhythm + lyrics + bottom pad
    let total_width = start_offset_x
        + total_events_width
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

    if part.name.is_some() || part.micro_fret_divisions.is_some() {
        let header_x = rect.min.x + margin_left;
        let name_y = rect.min.y + 14.0;
        let edo_y = rect.min.y + 32.0;

        if let Some(name) = &part.name {
            // Bold instrument name
            painter.text(
                Pos2::new(header_x, name_y),
                egui::Align2::LEFT_CENTER,
                name,
                FontId::new(20.0, egui::FontFamily::Proportional),
                ui.visuals().strong_text_color(),
            );
        }

        // Plain "N EDO" subtitle
        if let Some(divisions) = part.micro_fret_divisions {
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

    let bg_color = ui.visuals().panel_fill;
    let note_color = ui.visuals().text_color();
    let highlight_color = Color32::from_rgb(100, 180, 255); // Light blue
    let edit_color = Color32::from_rgb(255, 175, 75); // Light orange

    let mut current_x = first_barline_x;

    let m0_repeat_start = global.and_then(|g| g.measures.first()).and_then(|m| m.repeat_start.as_ref()).is_some();
    if m0_repeat_start {
        let staff_mid = string_y_coords[0] + (last_string_y - string_y_coords[0]) / 2.0;
        let string_spacing = if string_y_coords.len() > 1 { string_y_coords[1] - string_y_coords[0] } else { 10.0 };
        let mid_y1 = staff_mid - string_spacing * 0.5;
        let mid_y2 = staff_mid + string_spacing * 0.5;

        painter.line_segment(
            [
                Pos2::new(current_x, string_y_coords[0]),
                Pos2::new(current_x, last_string_y),
            ],
            Stroke::new(3.0, Color32::from_gray(150)),
        );
        painter.line_segment(
            [
                Pos2::new(current_x + 4.0, string_y_coords[0]),
                Pos2::new(current_x + 4.0, last_string_y),
            ],
            Stroke::new(1.0, Color32::from_gray(150)),
        );
        painter.circle_filled(Pos2::new(current_x + 8.0, mid_y1), 2.0, Color32::from_gray(150));
        painter.circle_filled(Pos2::new(current_x + 8.0, mid_y2), 2.0, Color32::from_gray(150));
        current_x += 12.0;
    } else {
        painter.line_segment(
            [
                Pos2::new(current_x, string_y_coords[0]),
                Pos2::new(current_x, last_string_y),
            ],
            Stroke::new(2.0, Color32::from_gray(150)),
        );
    }
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

    let mut note_id_map: std::collections::HashMap<String, (Pos2, u8, i32)> =
        std::collections::HashMap::new();
    let mut event_id_map: std::collections::HashMap<String, (f32, bool)> =
        std::collections::HashMap::new();
    let mut event_notes_map: std::collections::HashMap<
        String,
        Vec<(Option<String>, u8, i32, Pos2)>,
    > = std::collections::HashMap::new();
    let mut event_id_to_idx: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    let mut collected_ties: Vec<(Pos2, u8, i32, String)> = Vec::new();
    
    struct ActiveTempoChange {
        start_x: f32,
        y: f32,
        target_id: String,
    }
    let mut active_tempo_changes: Vec<ActiveTempoChange> = Vec::new();

    let mut collected_slurs: Vec<(crate::mnx::Slur, Vec<(Option<String>, u8, i32, Pos2)>)> =
        Vec::new();
    let mut string_timeline: std::collections::HashMap<u8, Vec<(usize, i32, Pos2)>> =
        std::collections::HashMap::new();
    let mut collected_slides: Vec<(usize, u8, i32, Pos2)> = Vec::new();

    let mut flat_event_idx = 0;
    let mut last_event_spacing = event_spacing;
    let mut grace_event_indices: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut measure_barlines: Vec<f32> = vec![first_barline_x];
    let mut rendered_lyrics: Vec<RenderedLyric> = Vec::new();
    let mut event_x_coords: Vec<(usize, f32)> = Vec::new();

    // ── Main event loop ───────────────────────────────────────────────────────

    let mut active_tuplets: Vec<(f32, u32)> = Vec::new();
    let mut tuplet_counter: usize = 0;
    let mut tuplet_stack: Vec<(usize, f64)> = Vec::new(); // (tuplet_id, scale)

    for (m_idx, measure) in part.measures.iter().enumerate() {
        let mut current_measure_beat: f64 = 0.0;
        tuplet_stack.clear();

        if let Some(seq) = measure.sequences.first() {
            let mut layout_items = Vec::new();
            for item in &seq.content {
                item.to_layout_items(&mut layout_items);
            }

            let mut first_ev_info = None;
            let mut temp_grace = false;
            for item in &layout_items {
                match item {
                    LayoutItem::GraceStart => temp_grace = true,
                    LayoutItem::GraceEnd => temp_grace = false,
                    LayoutItem::Event(e) => {
                        first_ev_info = Some((*e, temp_grace));
                        break;
                    }
                    _ => {}
                }
            }
            if let Some((first_ev, first_grace)) = first_ev_info {
                let is_editing_first = opts.is_editing_fret
                    && opts.selected_event == Some(flat_event_idx)
                    && !opts.editing_fret_text.is_empty();
                let edit_len = if is_editing_first {
                    Some(opts.editing_fret_text.len())
                } else {
                    None
                };
                let w_first = event_text_width(first_ev, first_grace, edit_len);
                let def_pad = if m_idx == 0 { first_bar_pad } else { measure_spacing };
                let extra_start = (w_first * 0.5 + 10.0 - def_pad).max(0.0);
                current_x += extra_start;
            }

            let num_events = seq.flattened_events().len();
            let mut e_idx = 0;
            let mut in_grace = false;
            for (item_idx, l_item) in layout_items.iter().enumerate() {
                let event = match l_item {
                    LayoutItem::TupletStart(t) => {
                        tuplet_counter += 1;
                        let current_scale = tuplet_stack.last().map(|&(_, s)| s).unwrap_or(1.0);
                        let inner_beats =
                            crate::validate::duration_to_quarter_beats(&t.inner.duration)
                                * (t.inner.multiple as f64);
                        let outer_beats =
                            crate::validate::duration_to_quarter_beats(&t.outer.duration)
                                * (t.outer.multiple as f64);
                        let new_scale = if inner_beats > 0.0 {
                            current_scale * (outer_beats / inner_beats)
                        } else {
                            current_scale
                        };
                        tuplet_stack.push((tuplet_counter, new_scale));
                        active_tuplets.push((current_x, t.inner.multiple));
                        continue;
                    }
                    LayoutItem::TupletEnd => {
                        tuplet_stack.pop();
                        if let Some((start_x, mult)) = active_tuplets.pop() {
                            let end_x = current_x - last_event_spacing * 0.5;
                            let y = string_y_coords[0] - 25.0; // Above the staff
                            painter.line_segment(
                                [
                                    egui::Pos2::new(start_x, y + 6.0),
                                    egui::Pos2::new(start_x, y),
                                ],
                                Stroke::new(1.0, Color32::GRAY),
                            );
                            painter.line_segment(
                                [egui::Pos2::new(start_x, y), egui::Pos2::new(end_x, y)],
                                Stroke::new(1.0, Color32::GRAY),
                            );
                            painter.line_segment(
                                [egui::Pos2::new(end_x, y), egui::Pos2::new(end_x, y + 6.0)],
                                Stroke::new(1.0, Color32::GRAY),
                            );
                            painter.text(
                                egui::Pos2::new((start_x + end_x) / 2.0, y - 8.0),
                                egui::Align2::CENTER_CENTER,
                                mult.to_string(),
                                FontId::monospace(11.0),
                                Color32::GRAY,
                            );
                        }
                        continue;
                    }
                    LayoutItem::GraceStart => {
                        in_grace = true;
                        continue;
                    }
                    LayoutItem::GraceEnd => {
                        in_grace = false;
                        continue;
                    }
                    LayoutItem::Event(e) => *e,
                };

                let current_tuplet_id = tuplet_stack.last().map(|&(id, _)| id);
                let current_scale = tuplet_stack.last().map(|&(_, s)| s).unwrap_or(1.0);
                let event_beats = if in_grace {
                    0.0
                } else {
                    crate::validate::duration_to_quarter_beats(&event.duration) * current_scale
                };
                let start_beat = current_measure_beat;
                let end_beat = current_measure_beat + event_beats;
                current_measure_beat = end_beat;

                if let Some(g) = global {
                    if let Some(gm) = g.measures.get(m_idx) {

                        // Check for tempo_changes
                        for tc in &gm.tempo_changes {
                            let tc_beat = if let Some(s) = &tc.location {
                                if let Some((n, d)) = s.split_once('/') {
                                    if let (Ok(num), Ok(den)) = (n.parse::<f64>(), d.parse::<f64>()) {
                                        if den > 0.0 { (num / den) * 4.0 } else { 0.0 }
                                    } else { 0.0 }
                                } else { 0.0 }
                            } else { 0.0 };
                            
                            let is_last = e_idx == num_events - 1;
                            let in_range = if is_last {
                                tc_beat >= start_beat - 0.001 && tc_beat <= end_beat + 0.001
                            } else {
                                tc_beat >= start_beat - 0.001 && tc_beat < end_beat - 0.001
                            };
                            
                            if in_range {
                                // Find end bpm to know if rit or accel
                                let mut end_bpm = 120.0;
                                if let Some(tid) = &tc.target {
                                    for gm2 in &g.measures {
                                        for t in &gm2.tempos {
                                            if t.id.as_ref() == Some(tid) {
                                                end_bpm = t.bpm;
                                            }
                                        }
                                    }
                                }
                                
                                // get current bpm (just find the most recent tempo BEFORE this tc, or default to 120)
                                let mut current_bpm = 120.0;
                                for m_i in 0..=m_idx {
                                    if let Some(gm_prev) = g.measures.get(m_i) {
                                        for t in &gm_prev.tempos {
                                            let t_beat = t
                                                .location
                                                .as_ref()
                                                .map(|p| p.to_quarter_beats())
                                                .unwrap_or(0.0);
                                            if m_i < m_idx || t_beat <= tc_beat + 0.001 {
                                                if Some(t.id.clone().unwrap_or_default()) != tc.target {
                                                    current_bpm = t.bpm;
                                                }
                                            }
                                        }
                                    }
                                }
                                
                                let label = if end_bpm > current_bpm { "accel." } else { "rit." };
                                
                                let y = string_y_coords[0] - 45.0;
                                
                                let text_rect = painter.text(
                                    Pos2::new(current_x, y),
                                    egui::Align2::LEFT_BOTTOM,
                                    label,
                                    egui::FontId::new(16.0, egui::FontFamily::Proportional).clone(),
                                    note_color,
                                );
                                
                                active_tempo_changes.push(ActiveTempoChange {
                                    start_x: text_rect.max.x + 5.0,
                                    y,
                                    target_id: tc.target.clone().unwrap_or_default(),
                                });
                            }
                        }

                        for tempo in &gm.tempos {
                            let tempo_beat = tempo
                                .location
                                .as_ref()
                                .map(|p| p.to_quarter_beats())
                                .unwrap_or(0.0);
                            let is_last = e_idx == num_events - 1;
                            let in_range = if is_last {
                                tempo_beat >= start_beat - 0.001 && tempo_beat <= end_beat + 0.001
                            } else {
                                tempo_beat >= start_beat - 0.001 && tempo_beat < end_beat - 0.001
                            };
                            if in_range {
                                let glyph = match tempo.value.base.as_str() {
                                    "16th" | "sixteenth" => "\u{ECA9}",
                                    "eighth" => "\u{ECA7}",
                                    "quarter" => "\u{ECA5}",
                                    "half" => "\u{ECA3}",
                                    "whole" => "\u{ECA2}",
                                    _ => "\u{ECA5}",
                                };
                                
                                let dots_str = (0..tempo.value.dots.unwrap_or(0)).map(|_| ".").collect::<String>();
                                let text = format!("{} = {}", dots_str, tempo.bpm);
                                
                                let glyph_rect = painter.text(
                                    Pos2::new(current_x, string_y_coords[0] - 45.0),
                                    egui::Align2::LEFT_BOTTOM,
                                    glyph,
                                    egui::FontId::new(20.0, egui::FontFamily::Name("BravuraText".into())),
                                    note_color,
                                );
                                

                                painter.text(
                                    Pos2::new(glyph_rect.max.x + 2.0, string_y_coords[0] - 45.0),
                                    egui::Align2::LEFT_BOTTOM,
                                    text,
                                    egui::FontId::proportional(16.0),
                                    note_color,
                                );
                                
                                // Check if this tempo is a target for any active tempo change
                                if let Some(tid) = &tempo.id {
                                    active_tempo_changes.retain(|atc| {
                                        if &atc.target_id == tid {
                                            // Draw dashed line
                                            let p1 = Pos2::new(atc.start_x, atc.y - 6.0);
                                            let p2 = Pos2::new(current_x - 5.0, atc.y - 6.0);
                                            let mut x = p1.x;
                                            while x < p2.x {
                                                painter.line_segment([Pos2::new(x, p1.y), Pos2::new((x + 5.0).min(p2.x), p1.y)], egui::Stroke::new(1.0, note_color));
                                                x += 10.0;
                                            }
                                            false // remove
                                        } else {
                                            true // keep
                                        }
                                    });
                                }

                            }
                        }
                    }
                }

                if let Some(ev_id) = &event.id {
                    event_id_map.insert(ev_id.clone(), (current_x, event.rest.is_some()));
                    event_id_to_idx.insert(ev_id.clone(), flat_event_idx);
                }

                let mut next_ev_info = None;
                let mut look_grace = in_grace;
                for next_item in &layout_items[item_idx + 1..] {
                    match next_item {
                        LayoutItem::GraceStart => look_grace = true,
                        LayoutItem::GraceEnd => look_grace = false,
                        LayoutItem::Event(ne) => {
                            next_ev_info = Some((*ne, look_grace));
                            break;
                        }
                        _ => {}
                    }
                }

                let is_editing_this = opts.is_editing_fret
                    && opts.selected_event == Some(flat_event_idx)
                    && !opts.editing_fret_text.is_empty();
                let edit_len = if is_editing_this {
                    Some(opts.editing_fret_text.len())
                } else {
                    None
                };

                let next_editing_len = if opts.is_editing_fret
                    && opts.selected_event == Some(flat_event_idx + 1)
                    && !opts.editing_fret_text.is_empty()
                {
                    Some(opts.editing_fret_text.len())
                } else {
                    None
                };

                let spacing = calculate_event_spacing(
                    event,
                    in_grace,
                    event_spacing,
                    next_ev_info,
                    edit_len,
                    next_editing_len,
                );

                let is_selected = if let Some(cursor) = selected_event {
                    let anchor = opts.selection_anchor.unwrap_or(cursor);
                    flat_event_idx >= cursor.min(anchor) && flat_event_idx <= cursor.max(anchor)
                } else {
                    false
                };

                let event_w = event_text_width(event, in_grace, edit_len);
                let half_col_w = (event_w * 0.5 + 4.0).max(if in_grace { 8.5 } else { 12.0 });
                let col_rect = if in_grace {
                    Rect::from_min_max(
                        Pos2::new(current_x - half_col_w, string_y_coords[0] - 4.0),
                        Pos2::new(current_x + half_col_w, stem_bot_y + 4.0),
                    )
                } else {
                    Rect::from_min_max(
                        Pos2::new(current_x - half_col_w, string_y_coords[0] - 6.0),
                        Pos2::new(current_x + half_col_w, stem_bot_y + 6.0),
                    )
                };

                if is_selected {
                    let col_color = if is_editing_fret {
                        Color32::from_rgba_unmultiplied(255, 175, 75, 35)
                    } else {
                        Color32::from_rgba_unmultiplied(100, 180, 255, 35)
                    };
                    painter.rect_filled(col_rect, 4.0, col_color);
                    if opts.scroll_to_selected {
                        ui.scroll_to_rect(col_rect, None);
                    }
                }

                if event.rest.is_some() {
                    let rest_char = match event.duration.base.as_str() {
                        "whole" => '\u{E4E3}',
                        "half" => '\u{E4E4}',
                        "quarter" => '\u{E4E5}',
                        "eighth" => '\u{E4E6}',
                        "16th" => '\u{E4E7}',
                        "32nd" => '\u{E4E8}',
                        "64th" => '\u{E4E9}',
                        _ => '\u{E4E5}',
                    };

                    let center_y =
                        (string_y_coords.first().unwrap() + string_y_coords.last().unwrap()) / 2.0;
                    let text = rest_char.to_string();
                    let font_id = FontId::new(32.0, egui::FontFamily::Name("Bravura".into()));
                    let galley = painter.layout_no_wrap(text, font_id, note_color);
                    let text_rect =
                        Rect::from_center_size(Pos2::new(current_x, center_y), galley.size());
                    painter.galley(text_rect.min, galley, note_color);

                    if let Some(dots) = event.duration.dots {
                        if dots > 0 {
                            let dot_char = '\u{E1E7}';
                            let dot_text = dot_char.to_string();
                            let dot_font_id =
                                FontId::new(32.0, egui::FontFamily::Name("Bravura".into()));
                            let dot_galley =
                                painter.layout_no_wrap(dot_text, dot_font_id, note_color);
                            let dot_rect = Rect::from_center_size(
                                Pos2::new(current_x + text_rect.width() / 2.0 + 4.0, center_y),
                                dot_galley.size(),
                            );
                            painter.galley(dot_rect.min, dot_galley, note_color);
                        }
                    }
                }

                // Fret numbers on strings: store (fret, note_idx)
                let current_font_id = if in_grace { FontId::monospace(10.0) } else { font_id.clone() };
                let mut step_frets: Vec<Option<(i32, usize)>> = vec![None; num_strings];
                let mut this_event_notes: Vec<(Option<String>, u8, i32, Pos2)> = Vec::new();
                let mut min_string_y = f32::MAX;
                let mut max_string_y = f32::MIN;
                for (n_idx, note) in event.notes.iter().enumerate() {
                    if let Some(s) = note.string {
                        if s >= 1 && (s as usize) <= num_strings {
                            let y = string_y_coords[(s - 1) as usize];
                            min_string_y = min_string_y.min(y);
                            max_string_y = max_string_y.max(y);
                        }
                    }
                    let effective_fret = note
                        .fret
                        .or_else(|| note.harmonic.as_ref().map(|h| h.touch.round() as i32));
                    if let (Some(s), Some(f)) = (note.string, effective_fret) {
                        if s >= 1 && (s as usize) <= num_strings {
                            step_frets[(s - 1) as usize] = Some((f, n_idx));
                            let note_pos = Pos2::new(current_x, string_y_coords[(s - 1) as usize]);
                            this_event_notes.push((note.id.clone(), s, f, note_pos));
                            if let Some(nid) = &note.id {
                                note_id_map.insert(nid.clone(), (note_pos, s, f));
                            }
                            string_timeline.entry(s).or_default().push((
                                flat_event_idx,
                                f,
                                note_pos,
                            ));

                            if let Some(ties) = &note.ties {
                                for tie in ties {
                                    collected_ties.push((note_pos, s, f, tie.target.clone()));
                                }
                            }
                            if note.slide.is_some() {
                                collected_slides.push((flat_event_idx, s, f, note_pos));
                            }
                        }
                    }
                }
                if let Some(slurs) = &event.slurs {
                    for slur in slurs {
                        collected_slurs.push((slur.clone(), this_event_notes.clone()));
                    }
                }
                if let Some(ev_id) = &event.id {
                    event_notes_map.insert(ev_id.clone(), this_event_notes);
                }

                // Draw Event Markings (Accent, Strum)
                // Draw Strum (Arpeggio Wiggly Line)
                let mut arp_dir = None;
                if let Some(arpeggios) = &measure.arpeggios {
                    let first_id = event
                        .notes
                        .first()
                        .and_then(|n| n.id.as_deref())
                        .unwrap_or("");
                    let last_id = event
                        .notes
                        .last()
                        .and_then(|n| n.id.as_deref())
                        .unwrap_or("");
                    for arp in arpeggios {
                        if arp.span.start == first_id && arp.span.end == last_id {
                            arp_dir = arp.direction.clone();
                            break;
                        }
                    }
                }

                if let Some(dir) = arp_dir {
                    if min_string_y < max_string_y {
                        let font_size = 28.0;
                        let arp_font_id =
                            FontId::new(font_size, egui::FontFamily::Name("Bravura".into()));
                        let arp_x = current_x - (event_w * 0.5 + 6.0).max(12.0);

                        let step_y = font_size * 0.25;
                        let num_steps = ((max_string_y - min_string_y) / step_y).ceil() as i32;
                        let num_steps = num_steps.max(1);

                        let total_h = (num_steps as f32) * step_y;
                        let center_y = (min_string_y + max_string_y) / 2.0 + font_size * 0.2;
                        let start_y = center_y - total_h / 2.0;

                        for i in 0..=num_steps {
                            let y = start_y + (i as f32) * step_y;
                            let char_to_draw = if dir == "up" {
                                if i == 0 { "\u{EAAD}" } else { "\u{EAA9}" }
                            } else {
                                if i == num_steps {
                                    "\u{EAAE}"
                                } else {
                                    "\u{EAAA}"
                                }
                            };

                            let galley = ui.painter().layout_no_wrap(
                                char_to_draw.to_string(),
                                arp_font_id.clone(),
                                note_color,
                            );

                            let size = galley.size();
                            let pos = Pos2::new(arp_x - size.y / 2.0, y);

                            let mut text_shape =
                                egui::epaint::TextShape::new(pos, galley, note_color);
                            text_shape.angle = -std::f32::consts::FRAC_PI_2;
                            painter.add(text_shape);
                        }
                    }
                }

                if event.markings.as_ref().and_then(|m| m.accent.as_ref()).is_some()
                    || event.bow_direction.is_some()
                {
                    let font_size = 28.0;
                    let markings_font_id =
                        FontId::new(font_size, egui::FontFamily::Name("Bravura".into()));
                    let mut marking_y = min_string_y - 20.0; // Place above the chord

                    if event.markings.as_ref().and_then(|m| m.accent.as_ref()).is_some() {
                        let galley = painter.layout_no_wrap(
                            "\u{E4A0}".to_string(), // articAccentAbove
                            markings_font_id.clone(),
                            note_color,
                        );
                        let rect =
                            Rect::from_center_size(Pos2::new(current_x, marking_y), galley.size());
                        painter.galley(rect.min, galley, note_color);
                        marking_y -= 15.0;
                    }

                    if let Some(bow) = &event.bow_direction {
                        let glyph = if bow.direction == "up" {
                            "\u{EB70}"
                        } else {
                            "\u{EB74}"
                        };
                        let galley = painter.layout_no_wrap(
                            glyph.to_string(),
                            markings_font_id.clone(),
                            note_color,
                        );
                        let rect =
                            Rect::from_center_size(Pos2::new(current_x, marking_y), galley.size());
                        painter.galley(rect.min, galley, note_color);
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

                        let is_muted = event.notes[n_idx].muted.unwrap_or(false);
                        let display_fret = if let Some(harmonic) = &event.notes[n_idx].harmonic {
                            format_harmonic_touch(harmonic.touch)
                        } else {
                            fret.to_string()
                        };

                        let display_text = if is_this_string_editing {
                            if editing_fret_text.is_empty() {
                                if is_muted {
                                    "X".to_string()
                                } else {
                                    display_fret
                                }
                            } else {
                                editing_fret_text.to_string()
                            }
                        } else {
                            if is_muted {
                                "X".to_string()
                            } else {
                                display_fret
                            }
                        };

                        let galley =
                            painter.layout_no_wrap(display_text, current_font_id.clone(), text_color);
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
                        if event
                            .notes
                            .get(n_idx)
                            .and_then(|n| n.tap.as_ref())
                            .is_some()
                        {
                            painter.text(
                                Pos2::new(current_x, text_rect.min.y - 3.0),
                                egui::Align2::CENTER_BOTTOM,
                                "T",
                                FontId::proportional(11.0),
                                note_color,
                            );
                        }
                        current_fret_rects.push((text_rect, n_idx));

                        // Draw bend shape
                        if let Some(note) = event.notes.get(n_idx) {
                            if let Some(bend_str) = &note.bend {
                                let mut max_y_text = 0.0_f32;
                                let mut path_nodes = Vec::new();
                                let tokens: Vec<&str> = bend_str.split_whitespace().collect();
                                let mut t_idx = 0;
                                while t_idx < tokens.len() {
                                    if tokens[t_idx] == "M" && t_idx + 2 < tokens.len() {
                                        let x = tokens[t_idx + 1].parse::<f32>().unwrap_or(0.0);
                                        let y = tokens[t_idx + 2].parse::<f32>().unwrap_or(0.0);
                                        if y > max_y_text {
                                            max_y_text = y;
                                        }
                                        path_nodes.push((x, y));
                                        t_idx += 3;
                                    } else if tokens[t_idx] == "C" && t_idx + 6 < tokens.len() {
                                        let lx = tokens[t_idx + 1]
                                            .trim_matches(',')
                                            .parse::<f32>()
                                            .unwrap_or(0.0);
                                        let ly = tokens[t_idx + 2]
                                            .trim_matches(',')
                                            .parse::<f32>()
                                            .unwrap_or(0.0);
                                        let rx = tokens[t_idx + 3]
                                            .trim_matches(',')
                                            .parse::<f32>()
                                            .unwrap_or(0.0);
                                        let ry = tokens[t_idx + 4]
                                            .trim_matches(',')
                                            .parse::<f32>()
                                            .unwrap_or(0.0);
                                        let nx = tokens[t_idx + 5]
                                            .trim_matches(',')
                                            .parse::<f32>()
                                            .unwrap_or(0.0);
                                        let ny = tokens[t_idx + 6]
                                            .trim_matches(',')
                                            .parse::<f32>()
                                            .unwrap_or(0.0);
                                        if ny > max_y_text {
                                            max_y_text = ny;
                                        }
                                        path_nodes.push((lx, ly));
                                        path_nodes.push((rx, ry));
                                        path_nodes.push((nx, ny));
                                        t_idx += 7;
                                    } else {
                                        t_idx += 1;
                                    }
                                }

                                let cents = (max_y_text * pitch_bend_range as f32).round() as i32;

                                if cents > 0 && !path_nodes.is_empty() {

                                    let start_pos =
                                        Pos2::new(text_rect.max.x + 2.0, text_rect.center().y);

                                    let available_w = (current_x + spacing - 14.0) - start_pos.x;
                                    let max_w = (spacing * 0.5)
                                        .clamp(10.0, 30.0)
                                        .min(available_w.max(10.0));
                                    let max_h = 24.0; // More vertical space (about 1 string spacing)

                                    let map_pt = |nx: f32, ny: f32| -> Pos2 {
                                        Pos2::new(
                                            start_pos.x + (nx / 100.0) * max_w,
                                            start_pos.y - (ny / 100.0) * max_h,
                                        )
                                    };

                                    let mut p0 = map_pt(path_nodes[0].0, path_nodes[0].1);
                                    let mut last_p2 = p0;
                                    let mut last_p3 = p0;

                                    let mut i = 1;
                                    while i + 2 < path_nodes.len() {
                                        let p1 = map_pt(path_nodes[i].0, path_nodes[i].1);
                                        let p2 = map_pt(path_nodes[i + 1].0, path_nodes[i + 1].1);
                                        let p3 = map_pt(path_nodes[i + 2].0, path_nodes[i + 2].1);

                                        painter.add(
                                            egui::epaint::CubicBezierShape::from_points_stroke(
                                                [p0, p1, p2, p3],
                                                false,
                                                Color32::TRANSPARENT,
                                                Stroke::new(1.0, note_color),
                                            ),
                                        );

                                        p0 = p3;
                                        last_p2 = p2;
                                        last_p3 = p3;
                                        i += 3;
                                    }

                                    // Draw arrowhead at the end
                                    let dir = (last_p3 - last_p2).normalized();
                                    let dir = if dir.x.is_nan() || dir.y.is_nan() {
                                        egui::vec2(1.0, -1.0).normalized()
                                    } else {
                                        dir
                                    };

                                    // Arrowhead wings
                                    let arrow_len = 4.0;
                                    let right_wing = egui::vec2(-dir.x + dir.y, -dir.x - dir.y)
                                        .normalized()
                                        * arrow_len;
                                    let left_wing = egui::vec2(-dir.x - dir.y, dir.x - dir.y)
                                        .normalized()
                                        * arrow_len;

                                    painter.line_segment(
                                        [last_p3, last_p3 + right_wing],
                                        Stroke::new(1.0, note_color),
                                    );
                                    painter.line_segment(
                                        [last_p3, last_p3 + left_wing],
                                        Stroke::new(1.0, note_color),
                                    );

                                    // Draw cents text above the max point
                                    let peak_y = start_pos.y - (max_y_text / 100.0) * max_h;
                                    let mut peak_x = last_p3.x; // approximate near end

                                    // Prevent text from overlapping the next note
                                    let text_w = 26.0;
                                    let next_note_x = current_x + spacing - 8.0;
                                    if peak_x + text_w / 2.0 > next_note_x {
                                        peak_x = next_note_x - text_w / 2.0;
                                    }

                                    painter.text(
                                        Pos2::new(peak_x, peak_y - 3.0),
                                        egui::Align2::CENTER_BOTTOM,
                                        format!("+{} ¢", cents),
                                        FontId::proportional(10.0),
                                        note_color,
                                    );
                                }
                            }
                        }
                    } else if is_selected && is_editing_fret && editing_string == Some(string_num) {
                        // Empty string currently focused for placement in edit mode
                        let display_text = if editing_fret_text.is_empty() {
                            "_".to_string()
                        } else {
                            editing_fret_text.to_string()
                        };
                        let galley =
                            painter.layout_no_wrap(display_text, current_font_id.clone(), Color32::BLACK);
                        let text_rect =
                            Rect::from_center_size(Pos2::new(current_x, y), galley.size());
                        painter.rect_filled(text_rect.expand(3.5), 3.0, edit_color);
                        painter.galley(text_rect.min, galley, Color32::BLACK);
                    }
                }

                let stem_rect = if in_grace {
                    Rect::from_min_max(
                        Pos2::new(current_x - 7.0, stem_top_y - 10.0),
                        Pos2::new(current_x + 7.0, stem_bot_y + 4.0),
                    )
                } else {
                    Rect::from_min_max(
                        Pos2::new(current_x - 10.0, stem_top_y - 2.0),
                        Pos2::new(current_x + 10.0, stem_bot_y + 6.0),
                    )
                };
                event_hit_boxes.push((current_fret_rects, stem_rect, col_rect));

                // Collect rhythm info for drawing after loop
                let (rhythm, mut is_dotted) = parse_rhythm(&event.duration.base);
                if event.duration.dots.unwrap_or(0) > 0 {
                    is_dotted = true;
                }
                if in_grace {
                    grace_event_indices.insert(rhythm_notes.len());
                }
                rhythm_notes.push(RhythmNote {
                    x: current_x,
                    rhythm,
                    is_dotted,
                    measure_idx: m_idx,
                    is_rest: event.rest.is_some(),
                    tuplet_id: current_tuplet_id,
                    start_beat,
                    end_beat,
                });

                event_x_coords.push((flat_event_idx, current_x));

                let is_editing_this_lyric = opts.is_editing_lyric && opts.selected_event == Some(flat_event_idx);
                if is_editing_this_lyric {
                    rendered_lyrics.push(RenderedLyric {
                        event_idx: flat_event_idx,
                        x: current_x,
                        text: opts.editing_lyric_text.to_string(),
                        syllabic: None,
                        extender: None,
                        line: 0,
                        is_editing: true,
                    });
                } else if let Some(lyrics) = &event.lyrics {
                    for (line_idx, (line_key, lyric)) in lyrics.lines.iter().enumerate() {
                        let line = line_key
                            .parse::<usize>()
                            .ok()
                            .map(|l| l.saturating_sub(1))
                            .unwrap_or(line_idx);
                        rendered_lyrics.push(RenderedLyric {
                            event_idx: flat_event_idx,
                            x: current_x,
                            text: lyric.text.clone(),
                            syllabic: lyric.line_type.clone(),
                            extender: lyric.ext.extender,
                            line,
                            is_editing: false,
                        });
                    }
                }

                flat_event_idx += 1;
                e_idx += 1;
                current_x += spacing;
                last_event_spacing = spacing;
            }
        }

        // Barline at end of measure (spans staff only — rhythms are outside)
        let mut drawn_end_barline = false;
        if let Some(g) = global {
            if let Some(gm) = g.measures.get(m_idx) {
                if let Some(b) = &gm.barline {
                    if b.barline_type == "double" {
                        painter.line_segment(
                            [
                                Pos2::new(current_x - 4.0, string_y_coords[0]),
                                Pos2::new(current_x - 4.0, last_string_y),
                            ],
                            Stroke::new(1.0, Color32::from_gray(150)),
                        );
                        painter.line_segment(
                            [
                                Pos2::new(current_x, string_y_coords[0]),
                                Pos2::new(current_x, last_string_y),
                            ],
                            Stroke::new(1.0, Color32::from_gray(150)),
                        );
                        drawn_end_barline = true;
                    }
                }
                
                if gm.repeat_end.is_some() {
                    let staff_mid = string_y_coords[0] + (last_string_y - string_y_coords[0]) / 2.0;
                    let string_spacing = if string_y_coords.len() > 1 { string_y_coords[1] - string_y_coords[0] } else { 10.0 };
                    let mid_y1 = staff_mid - string_spacing * 0.5;
                    let mid_y2 = staff_mid + string_spacing * 0.5;

                    painter.circle_filled(Pos2::new(current_x - 10.0, mid_y1), 2.0, Color32::from_gray(150));
                    painter.circle_filled(Pos2::new(current_x - 10.0, mid_y2), 2.0, Color32::from_gray(150));
                    painter.line_segment(
                        [
                            Pos2::new(current_x - 6.0, string_y_coords[0]),
                            Pos2::new(current_x - 6.0, last_string_y),
                        ],
                        Stroke::new(1.0, Color32::from_gray(150)),
                    );
                    painter.line_segment(
                        [
                            Pos2::new(current_x - 2.0, string_y_coords[0]),
                            Pos2::new(current_x - 2.0, last_string_y),
                        ],
                        Stroke::new(3.0, Color32::from_gray(150)),
                    );
                    drawn_end_barline = true;
                }
            }
        }
        
        let mut next_has_repeat_start = false;
        if let Some(g) = global {
            if let Some(gm) = g.measures.get(m_idx + 1) {
                if gm.repeat_start.is_some() {
                    next_has_repeat_start = true;
                }
            }
        }

        if next_has_repeat_start {
            let staff_mid = string_y_coords[0] + (last_string_y - string_y_coords[0]) / 2.0;
            let string_spacing = if string_y_coords.len() > 1 { string_y_coords[1] - string_y_coords[0] } else { 10.0 };
            let mid_y1 = staff_mid - string_spacing * 0.5;
            let mid_y2 = staff_mid + string_spacing * 0.5;
            
            if drawn_end_barline {
                // E.g., it was an end repeat. The thick line is at current_x - 2.0.
                // Draw start repeat starting from current_x + 2.0
                painter.line_segment(
                    [
                        Pos2::new(current_x + 2.0, string_y_coords[0]),
                        Pos2::new(current_x + 2.0, last_string_y),
                    ],
                    Stroke::new(3.0, Color32::from_gray(150)),
                );
                painter.line_segment(
                    [
                        Pos2::new(current_x + 6.0, string_y_coords[0]),
                        Pos2::new(current_x + 6.0, last_string_y),
                    ],
                    Stroke::new(1.0, Color32::from_gray(150)),
                );
                painter.circle_filled(Pos2::new(current_x + 10.0, mid_y1), 2.0, Color32::from_gray(150));
                painter.circle_filled(Pos2::new(current_x + 10.0, mid_y2), 2.0, Color32::from_gray(150));
            } else {
                // It was NOT an end repeat. Replace the thin line at current_x with the start repeat.
                painter.line_segment(
                    [
                        Pos2::new(current_x, string_y_coords[0]),
                        Pos2::new(current_x, last_string_y),
                    ],
                    Stroke::new(3.0, Color32::from_gray(150)),
                );
                painter.line_segment(
                    [
                        Pos2::new(current_x + 4.0, string_y_coords[0]),
                        Pos2::new(current_x + 4.0, last_string_y),
                    ],
                    Stroke::new(1.0, Color32::from_gray(150)),
                );
                painter.circle_filled(Pos2::new(current_x + 8.0, mid_y1), 2.0, Color32::from_gray(150));
                painter.circle_filled(Pos2::new(current_x + 8.0, mid_y2), 2.0, Color32::from_gray(150));
                drawn_end_barline = true; // prevents the normal thin line from being drawn
            }
        }

        if !drawn_end_barline {
            painter.line_segment(
                [
                    Pos2::new(current_x, string_y_coords[0]),
                    Pos2::new(current_x, last_string_y),
                ],
                Stroke::new(1.0, Color32::from_gray(150)),
            );
        }

        measure_barlines.push(current_x);

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

    // ── Draw Alternate Endings (Voltas) ────────────────────────────────────────
    if let Some(g) = global {
        let bracket_y = if has_tempos {
            string_y_coords[0] - 56.0
        } else {
            string_y_coords[0] - 32.0
        };
        let hook_len = 14.0_f32;
        let bracket_stroke = Stroke::new(1.2, note_color);

        for (m_idx, gm) in g.measures.iter().enumerate() {
            if let Some(ending) = &gm.ending {
                let start_m = m_idx;
                let duration = ending.duration.max(1) as usize;
                let end_m = (start_m + duration - 1).min(part.measures.len().saturating_sub(1));

                if start_m < measure_barlines.len() && end_m + 1 < measure_barlines.len() {
                    let start_x = measure_barlines[start_m] + 1.5;
                    let is_open = ending.open.unwrap_or(false);
                    let end_x = if is_open {
                        measure_barlines[end_m + 1]
                    } else {
                        measure_barlines[end_m + 1] - 1.5
                    };

                    if end_x > start_x {
                        // Horizontal bracket line
                        painter.line_segment(
                            [Pos2::new(start_x, bracket_y), Pos2::new(end_x, bracket_y)],
                            bracket_stroke,
                        );
                        // Left vertical hook
                        painter.line_segment(
                            [
                                Pos2::new(start_x, bracket_y),
                                Pos2::new(start_x, bracket_y + hook_len),
                            ],
                            bracket_stroke,
                        );
                        // Right vertical hook (only if not open)
                        if !is_open {
                            painter.line_segment(
                                [
                                    Pos2::new(end_x, bracket_y),
                                    Pos2::new(end_x, bracket_y + hook_len),
                                ],
                                bracket_stroke,
                            );
                        }
                        // Label (e.g. "1, 2." or "3.")
                        let label_text = format_ending_numbers(&ending.numbers);
                        if !label_text.is_empty() {
                            painter.text(
                                Pos2::new(start_x + 5.0, bracket_y + 2.0),
                                egui::Align2::LEFT_TOP,
                                label_text,
                                FontId::proportional(13.0),
                                note_color,
                            );
                        }
                    }
                }
            }
        }
    }

    // ── Draw Ties ──────────────────────────────────────────────────────────────
    for (start_pos, start_string, _start_fret, target_id) in &collected_ties {
        let string_y = start_pos.y;
        if let Some(&(target_pos, target_string, _target_fret)) = note_id_map.get(target_id) {
            if *start_string == target_string && target_pos.x > start_pos.x {
                let x1 = start_pos.x + 9.0;
                let x2 = target_pos.x - 9.0;
                if x2 > x1 {
                    let p0 = Pos2::new(x1, string_y + 3.0);
                    let p3 = Pos2::new(x2, string_y + 3.0);
                    let dx = p3.x - p0.x;
                    let dip = (dx * 0.12).clamp(5.0, 11.0);
                    let p1 = Pos2::new(p0.x + dx * 0.25, string_y + 3.0 + dip);
                    let p2 = Pos2::new(p3.x - dx * 0.25, string_y + 3.0 + dip);
                    let shape = egui::epaint::CubicBezierShape::from_points_stroke(
                        [p0, p1, p2, p3],
                        false,
                        Color32::TRANSPARENT,
                        Stroke::new(1.5, note_color),
                    );
                    painter.add(shape);
                }
            }
        } else if let Some(&(rest_x, _is_rest)) = event_id_map.get(target_id) {
            // Note tied to a rest -> "let ring"
            if rest_x > start_pos.x {
                let x1 = start_pos.x + 9.0;
                let x2 = rest_x - 6.0;
                if x2 > x1 {
                    let p0 = Pos2::new(x1, string_y + 3.0);
                    let p3 = Pos2::new(x2, string_y + 3.0);
                    let dx = p3.x - p0.x;
                    let dip = (dx * 0.12).clamp(5.0, 11.0);
                    let p1 = Pos2::new(p0.x + dx * 0.25, string_y + 3.0 + dip);
                    let p2 = Pos2::new(p3.x - dx * 0.25, string_y + 3.0 + dip);
                    let shape = egui::epaint::CubicBezierShape::from_points_stroke(
                        [p0, p1, p2, p3],
                        false,
                        Color32::TRANSPARENT,
                        Stroke::new(1.5, note_color),
                    );
                    painter.add(shape);

                    painter.text(
                        Pos2::new((x1 + x2) * 0.5, string_y + 4.0 + dip),
                        egui::Align2::CENTER_TOP,
                        "l.r.",
                        FontId::proportional(10.0),
                        Color32::from_gray(160),
                    );
                }
            }
        }
    }

    // ── Draw Slides ─────────────────────────────────────────────────────────────
    for (start_idx, string_num, start_fret, start_pos) in &collected_slides {
        if let Some(timeline) = string_timeline.get(string_num) {
            // Find the next note on this string
            if let Some(pos_in_timeline) =
                timeline.iter().position(|(idx, _, _)| *idx == *start_idx)
            {
                if pos_in_timeline + 1 < timeline.len() {
                    let (_, next_fret, next_pos) = timeline[pos_in_timeline + 1];
                    let up = next_fret > *start_fret;

                    let start_x = start_pos.x + 8.0;
                    let mut end_x = start_x + 12.0; // small diagonal segment
                    let start_y = start_pos.y;

                    // In screen space, smaller Y is "up" visually.
                    // "up" (higher pitch/fret) should mean smaller Y
                    let end_y = if up {
                        start_pos.y - 6.0
                    } else {
                        start_pos.y + 6.0
                    };

                    // Don't draw past the next note if it's very close!
                    end_x = end_x.min(next_pos.x - 4.0);

                    painter.line_segment(
                        [
                            eframe::egui::Pos2::new(start_x, start_y),
                            eframe::egui::Pos2::new(end_x, end_y),
                        ],
                        eframe::egui::Stroke::new(2.0, note_color),
                    );
                }
            }
        }
    }

    // ── Draw Slurs (Hammer-ons / Pull-offs) ──────────────────────────────────────
    for (slur, start_notes) in &collected_slurs {
        let target_notes = match event_notes_map.get(&slur.target) {
            Some(notes) => notes,
            None => continue,
        };

        let mut matched_pair: Option<((u8, i32, Pos2), (u8, i32, Pos2))> = None;

        if let (Some(start_nid), Some(end_nid)) = (&slur.start_note, &slur.end_note) {
            let s_opt = start_notes
                .iter()
                .find(|(nid, _, _, _)| nid.as_ref() == Some(start_nid));
            let e_opt = target_notes
                .iter()
                .find(|(nid, _, _, _)| nid.as_ref() == Some(end_nid));
            if let (Some((_, s_str, s_fret, s_pos)), Some((_, e_str, e_fret, e_pos))) =
                (s_opt, e_opt)
            {
                matched_pair = Some(((*s_str, *s_fret, *s_pos), (*e_str, *e_fret, *e_pos)));
            }
        } else {
            for (_, s_str, s_fret, s_pos) in start_notes {
                if let Some((_, e_str, e_fret, e_pos)) = target_notes
                    .iter()
                    .find(|(_, str_num, _, _)| str_num == s_str)
                {
                    matched_pair = Some(((*s_str, *s_fret, *s_pos), (*e_str, *e_fret, *e_pos)));
                    break;
                }
            }
        }

        if let Some(((s_str, s_fret, s_pos), (e_str, e_fret, e_pos))) = matched_pair {
            // Enforce guitar constraint: "These can only occur on a single string!"
            if s_str != e_str || e_pos.x <= s_pos.x {
                continue;
            }

            let string_y = s_pos.y;
            let y_base = string_y - 8.0;
            let x1 = s_pos.x + 4.0;
            let x2 = e_pos.x - 4.0;
            let dx = x2 - x1;
            let arc_height = (dx * 0.15).clamp(6.0, 12.0);
            let peak_y = y_base - arc_height;

            let p0 = Pos2::new(x1, y_base);
            let p3 = Pos2::new(x2, y_base);
            let p1 = Pos2::new(p0.x + dx * 0.25, peak_y);
            let p2 = Pos2::new(p3.x - dx * 0.25, peak_y);

            let shape = egui::epaint::CubicBezierShape::from_points_stroke(
                [p0, p1, p2, p3],
                false,
                Color32::TRANSPARENT,
                Stroke::new(1.5, note_color),
            );
            painter.add(shape);

            let mid_x = (p0.x + p3.x) * 0.5;
            if e_fret > s_fret {
                painter.text(
                    Pos2::new(mid_x, peak_y - 2.0),
                    egui::Align2::CENTER_BOTTOM,
                    "h",
                    FontId::proportional(11.0),
                    note_color,
                );
            } else if e_fret < s_fret {
                painter.text(
                    Pos2::new(mid_x, peak_y - 2.0),
                    egui::Align2::CENTER_BOTTOM,
                    "p",
                    FontId::proportional(11.0),
                    note_color,
                );
            }
        }
    }

    // ── Rhythm notation ───────────────────────────────────────────────────────
    let n = rhythm_notes.len();
    let mut beam_to: Vec<Option<usize>> = vec![None; n];
    let mut beam_from: Vec<Option<usize>> = vec![None; n];

    for (m_idx, measure) in part.measures.iter().enumerate() {
        let all_explicit_beams: Vec<&crate::mnx::BeamGroup> = measure
            .beams
            .as_ref()
            .map(|b| b.iter().collect())
            .unwrap_or_default();

        if !all_explicit_beams.is_empty() {
            for bg in all_explicit_beams {
                apply_beam_group_indices(bg, &event_id_to_idx, &rhythm_notes, &mut beam_to, &mut beam_from);
            }
            // Grace notes in this measure: beam adjacent grace notes
            let grace_indices: Vec<usize> = (0..n)
                .filter(|&i| rhythm_notes[i].measure_idx == m_idx && grace_event_indices.contains(&i))
                .collect();
            for &ia in &grace_indices {
                let ib = ia + 1;
                if ib < n && grace_event_indices.contains(&ib) && rhythm_notes[ib].measure_idx == m_idx {
                    let note_a = &rhythm_notes[ia];
                    let note_b = &rhythm_notes[ib];
                    if !note_a.is_rest
                        && !note_b.is_rest
                        && note_a.rhythm.flag_count() >= 1
                        && note_b.rhythm.flag_count() >= 1
                    {
                        beam_to[ia] = Some(ib);
                        beam_from[ib] = Some(ia);
                    }
                }
            }
        } else {
            let time_sig = get_time_signature_for_measure(global, m_idx);
            compute_default_beaming_indices(
                m_idx,
                &time_sig,
                &rhythm_notes,
                &grace_event_indices,
                &mut beam_to,
                &mut beam_from,
            );
        }
    }

    for (i, note) in rhythm_notes.iter().enumerate() {
        if note.is_rest {
            continue;
        }
        let cx = note.x;
        let is_grace = grace_event_indices.contains(&i);
        let mut local_stem_top_y = stem_top_y;
        let mut local_stem_bot_y = stem_bot_y;
        if is_grace {
            local_stem_top_y -= 8.0;
            local_stem_bot_y -= 14.0;
        }

        let flags = note.rhythm.flag_count();
        let is_selected = if let Some(cursor) = selected_event {
            let anchor = opts.selection_anchor.unwrap_or(cursor);
            i >= cursor.min(anchor) && i <= cursor.max(anchor)
        } else {
            false
        };
        let this_stem_color = if is_selected {
            highlight_color
        } else {
            note_color
        };
        let stem_thickness = if is_selected { 2.5 } else if is_grace { 1.2 } else { 1.5 };

        // ── Stem ──────────────────────────────────────────────────────────────
        if note.rhythm != Rhythm::Whole {
            painter.line_segment(
                [Pos2::new(cx, local_stem_top_y), Pos2::new(cx, local_stem_bot_y)],
                Stroke::new(stem_thickness, this_stem_color),
            );
        }

        // ── Dot (augmentation) ────────────────────────────────────────────────
        if note.is_dotted {
            painter.circle_filled(Pos2::new(cx + 5.0, local_stem_bot_y - 2.0), 1.5, this_stem_color);
        }

        // ── Flags or beams (at bottom of stems) ────────────────────────────────
        if flags == 0 {
            // Whole / Half / Quarter — no beams or flags needed
        } else if beam_to[i].is_some() || beam_from[i].is_some() {
            // Part of a beam group — draw beams as thick horizontal lines at stem_bot_y
            if let Some(right_idx) = beam_to[i] {
                let nx = rhythm_notes[right_idx].x; // right note stem x
                let sx = cx; // this note stem x
                let right_flags = rhythm_notes[right_idx].rhythm.flag_count();

                // How many beam lines to draw between this note and the right note
                let shared_flags = flags.min(right_flags);
                let beam_thickness = if is_grace { 2.0 } else { 3.0 };
                for f in 0..shared_flags {
                    let by = local_stem_bot_y - (f as f32) * flag_spacing;
                    painter.line_segment(
                        [Pos2::new(sx, by), Pos2::new(nx, by)],
                        Stroke::new(beam_thickness, note_color),
                    );
                }

                // If this note has more flags than its right neighbour (e.g. 16th to 8th):
                let extra = flags.saturating_sub(right_flags);
                let stub_w = if is_grace { event_spacing * 0.25 } else { event_spacing * 0.35 };
                for f in shared_flags..shared_flags + extra {
                    let by = local_stem_bot_y - (f as f32) * flag_spacing;
                    painter.line_segment(
                        [Pos2::new(sx, by), Pos2::new(sx + stub_w, by)],
                        Stroke::new(beam_thickness, this_stem_color),
                    );
                }
            }

            // If this note is the end of a beam group and has extra flags compared to its left neighbour:
            if beam_to[i].is_none() {
                if let Some(left_idx) = beam_from[i] {
                    let prev_flags = rhythm_notes[left_idx].rhythm.flag_count();
                    if flags > prev_flags {
                        let extra = flags - prev_flags;
                        let sx = cx;
                        let beam_thickness = if is_grace { 2.0 } else { 3.0 };
                        let stub_w = if is_grace { event_spacing * 0.25 } else { event_spacing * 0.35 };
                        for f in prev_flags..prev_flags + extra {
                            let by = local_stem_bot_y - (f as f32) * flag_spacing;
                            painter.line_segment(
                                [Pos2::new(sx - stub_w, by), Pos2::new(sx, by)],
                                Stroke::new(beam_thickness, this_stem_color),
                            );
                        }
                    }
                }
            }
        } else {
            // Isolated note — draw flag(s) attached at stem_bot_y angling up towards staff
            let sx = cx;
            let flag_w = if is_grace { event_spacing * 0.25 } else { event_spacing * 0.45 };
            for f in 0..flags {
                let flag_y_bot = local_stem_bot_y - (f as f32) * flag_spacing;
                let flag_y_top = flag_y_bot - flag_spacing - 2.0;
                painter.line_segment(
                    [
                        Pos2::new(sx, flag_y_bot),
                        Pos2::new(sx + flag_w, flag_y_bot - flag_spacing * 0.5),
                    ],
                    Stroke::new(if is_selected { 2.0 } else if is_grace { 1.2 } else { 1.5 }, this_stem_color),
                );
                painter.line_segment(
                    [
                        Pos2::new(sx + flag_w, flag_y_bot - flag_spacing * 0.5),
                        Pos2::new(sx + flag_w * 0.4, flag_y_top),
                    ],
                    Stroke::new(if is_selected { 2.0 } else if is_grace { 1.2 } else { 1.5 }, this_stem_color),
                );
            }
        }
    }

    // ── Draw Lyrics ────────────────────────────────────────────────────────────
    let mut double_clicked_lyric: Option<(usize, usize)> = None;
    let lyrics_y = stem_bot_y + 16.0;
    for (i, lyr) in rendered_lyrics.iter().enumerate() {
        let y = lyrics_y + (lyr.line as f32) * 18.0;
        let font_id = FontId::proportional(13.0);

        let clean_text = lyr.text.trim_end_matches('-').trim_end_matches('_');
        let char_count = clean_text.chars().count();
        let text_w = (char_count as f32) * 7.5;
        let hit_w = text_w.max(24.0);
        let hit_rect = Rect::from_center_size(Pos2::new(lyr.x, y), egui::vec2(hit_w, 18.0));
        let lyr_resp = ui.interact(
            hit_rect,
            egui::Id::new(("rendered_lyric_hit", lyr.event_idx, lyr.line)),
            egui::Sense::click(),
        );
        if lyr_resp.double_clicked() {
            double_clicked_lyric = Some((lyr.event_idx, lyr.line + 1));
        }

        if lyr.is_editing {
            let display_text = if lyr.text.is_empty() { "|" } else { &lyr.text };
            let text_galley = painter.layout_no_wrap(display_text.to_string(), font_id.clone(), Color32::WHITE);
            let text_rect = Rect::from_center_size(Pos2::new(lyr.x, y), text_galley.size());
            painter.rect_filled(text_rect.expand(3.0), 3.0, Color32::from_rgb(50, 110, 200));
            painter.galley(text_rect.min, text_galley, Color32::WHITE);
        } else if !clean_text.is_empty() {
            painter.text(
                Pos2::new(lyr.x, y),
                egui::Align2::CENTER_CENTER,
                clean_text,
                font_id.clone(),
                note_color,
            );
        }

        // Draw hyphens between split syllables (type == start or middle or ends with '-')
        let is_split = matches!(
            lyr.syllabic.as_deref(),
            Some("start") | Some("begin") | Some("middle")
        ) || lyr.text.ends_with('-');
        if is_split {
            if let Some(next_lyr) = rendered_lyrics[i + 1..].iter().find(|l| l.line == lyr.line) {
                if next_lyr.x > lyr.x {
                    let first_clean = lyr.text.trim_end_matches('-').trim_end_matches('_');
                    let first_galley = painter.layout_no_wrap(first_clean.to_string(), font_id.clone(), note_color);
                    let end_of_first = lyr.x + first_galley.size().x * 0.5;

                    let next_clean = next_lyr.text.trim_end_matches('-').trim_end_matches('_');
                    let next_galley = painter.layout_no_wrap(next_clean.to_string(), font_id.clone(), note_color);
                    let beg_of_second = next_lyr.x - next_galley.size().x * 0.5;

                    if beg_of_second > end_of_first + 4.0 {
                        let hyphen_x = (end_of_first + beg_of_second) * 0.5;
                        painter.text(
                            Pos2::new(hyphen_x, y),
                            egui::Align2::CENTER_CENTER,
                            "-",
                            font_id.clone(),
                            note_color,
                        );
                    }
                }
            }
        }

        // Draw melisma extender line "_" if extender == Some(true) or text ends with '_'
        let has_extender = lyr.extender == Some(true) || lyr.text.ends_with('_');
        if has_extender {
            let next_target_x = if let Some(next_lyr) = rendered_lyrics[i + 1..].iter().find(|l| l.line == lyr.line) {
                let next_clean = next_lyr.text.trim_end_matches('-').trim_end_matches('_');
                let next_galley = painter.layout_no_wrap(next_clean.to_string(), font_id.clone(), note_color);
                Some(next_lyr.x - next_galley.size().x * 0.5 - 4.0)
            } else {
                None
            };

            let end_x = next_target_x.unwrap_or_else(|| {
                event_x_coords
                    .iter()
                    .find(|(idx, _)| *idx > lyr.event_idx)
                    .map(|(_, x)| *x + 6.0)
                    .unwrap_or(lyr.x + text_w * 0.5 + 20.0)
            });

            let start_x = if clean_text.is_empty() {
                lyr.x - 10.0
            } else {
                lyr.x + text_w * 0.5 + 3.0
            };
            if end_x > start_x + 3.0 {
                let baseline_y = y + 6.0;
                painter.line_segment(
                    [Pos2::new(start_x, baseline_y), Pos2::new(end_x, baseline_y)],
                    Stroke::new(1.2, note_color),
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

    (response, clicked_selection, add_bar_clicked, double_clicked_lyric)
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

    #[test]
    fn test_format_harmonic_touch() {
        assert_eq!(format_harmonic_touch(12.0), "<12>");
        assert_eq!(format_harmonic_touch(7.0), "<7>");
        assert_eq!(format_harmonic_touch(5.0), "<5>");
        assert_eq!(format_harmonic_touch(3.9), "<3.9>");
        assert_eq!(format_harmonic_touch(2.7), "<2.7>");
        assert_eq!(format_harmonic_touch(9.7), "<9.7>");
    }

    #[test]
    fn test_format_ending_numbers() {
        assert_eq!(format_ending_numbers(&[1]), "1.");
        assert_eq!(format_ending_numbers(&[2]), "2.");
        assert_eq!(format_ending_numbers(&[3]), "3.");
        assert_eq!(format_ending_numbers(&[1, 2]), "1, 2.");
        assert_eq!(format_ending_numbers(&[1, 2, 3]), "1, 2, 3.");
        assert_eq!(format_ending_numbers(&[]), "");
    }

    #[test]
    fn test_render_example_endings_tablature() {
        let yaml_str = include_str!("../example_endings.yaml");
        let doc: crate::mnx::MnxDocument = serde_saphyr::from_str(yaml_str).unwrap();
        let ctx = egui::Context::default();
        crate::setup_custom_fonts(&ctx);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let part = &doc.parts[0];
            let errors = Vec::new();
            let opts = TablatureOptions {
                part,
                global: Some(&doc.global),
                errors: &errors,
                selected_event: None,
                selection_anchor: None,
                selected_note_in_event: None,
                is_editing_fret: false,
                editing_fret_text: "",
                editing_string: None,
                is_editing_lyric: false,
                editing_lyric_text: "",
                scroll_to_selected: false,
                pitch_bend_range: 2,
            };
            let (_resp, _clicked, _add_bar, _lyric_dc) = draw_tablature(ui, opts);
        });
        output.textures_delta.clear();
    }



    #[test]
    fn test_apply_beam_group() {
        let notes = vec![
            RhythmNote {
                x: 0.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 0.0,
                end_beat: 0.5,
            },
            RhythmNote {
                x: 30.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 0.5,
                end_beat: 1.0,
            },
            RhythmNote {
                x: 60.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 1.0,
                end_beat: 1.5,
            },
            RhythmNote {
                x: 90.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 1.5,
                end_beat: 2.0,
            },
        ];
        let mut id_map = std::collections::HashMap::new();
        id_map.insert("ev1".to_string(), 0);
        id_map.insert("ev2".to_string(), 1);
        id_map.insert("ev3".to_string(), 2);
        id_map.insert("ev4".to_string(), 3);

        // Group 1: ev1 and ev2 beamed
        let bg1 = crate::mnx::BeamGroup {
            events: vec!["ev1".to_string(), "ev2".to_string()],
            beams: None,
            direction: None,
        };
        // Group 2: ev3 and ev4 beamed
        let bg2 = crate::mnx::BeamGroup {
            events: vec!["ev3".to_string(), "ev4".to_string()],
            beams: None,
            direction: None,
        };

        let mut beam_right = vec![false; 4];
        apply_beam_group(&bg1, &id_map, &notes, &mut beam_right);
        apply_beam_group(&bg2, &id_map, &notes, &mut beam_right);

        assert_eq!(beam_right, vec![true, false, true, false]);
    }

    #[test]
    fn test_apply_beam_group_does_not_beam_rest() {
        let notes = vec![
            RhythmNote {
                x: 0.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 0.0,
                end_beat: 0.5,
            },
            RhythmNote {
                x: 30.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: true, // Rest
                tuplet_id: None,
                start_beat: 0.5,
                end_beat: 1.0,
            },
        ];
        let mut id_map = std::collections::HashMap::new();
        id_map.insert("ev1".to_string(), 0);
        id_map.insert("ev2".to_string(), 1);

        let bg = crate::mnx::BeamGroup {
            events: vec!["ev1".to_string(), "ev2".to_string()],
            beams: None,
            direction: None,
        };
        let mut beam_right = vec![false; 2];
        apply_beam_group(&bg, &id_map, &notes, &mut beam_right);
        assert_eq!(beam_right, vec![false, false]);
    }

    #[test]
    fn test_slur_classification_and_guitar_string_constraint() {
        // Hammer-on: same string (6), upward fret (9 -> 12)
        let s_str = 6;
        let s_fret = 9;
        let e_str = 6;
        let e_fret = 12;
        assert_eq!(s_str, e_str);
        assert!(e_fret > s_fret);

        // Pull-off: same string (1), downward fret (14 -> 12)
        let s_str2 = 1;
        let s_fret2 = 14;
        let e_str2 = 1;
        let e_fret2 = 12;
        assert_eq!(s_str2, e_str2);
        assert!(e_fret2 < s_fret2);

        // Cross-string slur (invalid on guitar)
        let s_str3 = 6;
        let e_str3 = 5;
        assert_ne!(s_str3, e_str3);
    }

    #[test]
    fn test_duration_spacing_weights() {
        let base = 30.0;
        let quarter_d = crate::mnx::Duration {
            base: "quarter".into(),
            dots: None,
            multiple: None,
        };
        let half_d = crate::mnx::Duration {
            base: "half".into(),
            dots: None,
            multiple: None,
        };
        let whole_d = crate::mnx::Duration {
            base: "whole".into(),
            dots: None,
            multiple: None,
        };
        let eighth_d = crate::mnx::Duration {
            base: "eighth".into(),
            dots: None,
            multiple: None,
        };
        let sixteenth_d = crate::mnx::Duration {
            base: "16th".into(),
            dots: None,
            multiple: None,
        };

        let q_spacing = duration_spacing(&quarter_d, base);
        let h_spacing = duration_spacing(&half_d, base);
        let w_spacing = duration_spacing(&whole_d, base);
        let e_spacing = duration_spacing(&eighth_d, base);
        let s_spacing = duration_spacing(&sixteenth_d, base);

        // Quarter is baseline (30.0)
        assert_eq!(q_spacing, 30.0);
        // Half note is ~50% more than quarter (45.0)
        assert_eq!(h_spacing, 45.0);
        assert!((h_spacing / q_spacing - 1.5).abs() < 1e-4);
        // Whole note is ~70% more than quarter (51.0)
        assert_eq!(w_spacing, 51.0);
        assert!((w_spacing / q_spacing - 1.7).abs() < 1e-4);
        // Eighth is less than quarter (25.5)
        assert!(e_spacing < q_spacing);
        assert_eq!(e_spacing, 25.5);
        // 16th is less than eighth (22.5)
        assert!(s_spacing < e_spacing);
        assert_eq!(s_spacing, 22.5);
    }

    #[test]
    fn test_get_beam_breaks() {
        let ts_4_4 = TimeSignature { count: 4, unit: 4 };
        assert_eq!(get_beam_breaks(&ts_4_4, false), vec![2.0, 4.0]);
        assert_eq!(get_beam_breaks(&ts_4_4, true), vec![1.0, 2.0, 3.0, 4.0]);

        let ts_3_4 = TimeSignature { count: 3, unit: 4 };
        assert_eq!(get_beam_breaks(&ts_3_4, false), vec![1.0, 2.0, 3.0]);
        assert_eq!(get_beam_breaks(&ts_3_4, true), vec![1.0, 2.0, 3.0]);

        let ts_2_4 = TimeSignature { count: 2, unit: 4 };
        assert_eq!(get_beam_breaks(&ts_2_4, false), vec![1.0, 2.0]);

        let ts_5_4 = TimeSignature { count: 5, unit: 4 };
        assert_eq!(
            get_beam_breaks(&ts_5_4, false),
            vec![1.0, 2.0, 3.0, 4.0, 5.0]
        );

        let ts_5_8 = TimeSignature { count: 5, unit: 8 };
        assert_eq!(get_beam_breaks(&ts_5_8, false), vec![1.5, 2.5]);

        let ts_6_8 = TimeSignature { count: 6, unit: 8 };
        assert_eq!(get_beam_breaks(&ts_6_8, false), vec![1.5, 3.0]);

        let ts_9_8 = TimeSignature { count: 9, unit: 8 };
        assert_eq!(get_beam_breaks(&ts_9_8, false), vec![1.5, 3.0, 4.5]);

        let ts_12_8 = TimeSignature { count: 12, unit: 8 };
        assert_eq!(get_beam_breaks(&ts_12_8, false), vec![1.5, 3.0, 4.5, 6.0]);

        let ts_3_8 = TimeSignature { count: 3, unit: 8 };
        assert_eq!(get_beam_breaks(&ts_3_8, false), vec![1.5]);

        let ts_9_16 = TimeSignature { count: 9, unit: 16 };
        assert_eq!(get_beam_breaks(&ts_9_16, false), vec![0.75, 1.5, 2.25]);

        let ts_7_8 = TimeSignature { count: 7, unit: 8 };
        assert_eq!(get_beam_breaks(&ts_7_8, false), vec![1.5, 3.0, 4.0]);
    }

    #[test]
    fn test_default_beaming_4_4_eighth_and_sixteenth() {
        let ts_4_4 = TimeSignature { count: 4, unit: 4 };

        // 8 eighth notes in 4/4 -> beamed 4 at a time (beats 1-2 and beats 3-4)
        let mut notes_8th = Vec::new();
        for i in 0..8 {
            notes_8th.push(RhythmNote {
                x: (i as f32) * 30.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: (i as f64) * 0.5,
                end_beat: ((i + 1) as f64) * 0.5,
            });
        }
        let mut beam_right_8th = vec![false; 8];
        compute_default_beaming_for_measure(0, &ts_4_4, &notes_8th, &mut beam_right_8th);
        assert_eq!(
            beam_right_8th,
            vec![true, true, true, false, true, true, true, false]
        );

        // 16 sixteenth notes in 4/4 -> break at each beat boundary (groups of 4)
        let mut notes_16th = Vec::new();
        for i in 0..16 {
            notes_16th.push(RhythmNote {
                x: (i as f32) * 20.0,
                rhythm: Rhythm::Sixteenth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: (i as f64) * 0.25,
                end_beat: ((i + 1) as f64) * 0.25,
            });
        }
        let mut beam_right_16th = vec![false; 16];
        compute_default_beaming_for_measure(0, &ts_4_4, &notes_16th, &mut beam_right_16th);
        for i in 0..16 {
            let should_beam = (i % 4) != 3;
            assert_eq!(
                beam_right_16th[i], should_beam,
                "16th note at index {} beaming mismatch",
                i
            );
        }
    }

    #[test]
    fn test_default_beaming_3_4_two_eighths_at_a_time() {
        let ts_3_4 = TimeSignature { count: 3, unit: 4 };

        // 6 eighth notes in 3/4 -> beamed two at a time (breaks at 1.0, 2.0, 3.0)
        let mut notes = Vec::new();
        for i in 0..6 {
            notes.push(RhythmNote {
                x: (i as f32) * 30.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: (i as f64) * 0.5,
                end_beat: ((i + 1) as f64) * 0.5,
            });
        }
        let mut beam_right = vec![false; 6];
        compute_default_beaming_for_measure(0, &ts_3_4, &notes, &mut beam_right);
        assert_eq!(beam_right, vec![true, false, true, false, true, false]);
    }

    #[test]
    fn test_default_beaming_5_8_odd_meter() {
        let ts_5_8 = TimeSignature { count: 5, unit: 8 };

        // 5 eighth notes in 5/8 -> beamed 3 + 2 (break at 1.5)
        let mut notes_8th = Vec::new();
        for i in 0..5 {
            notes_8th.push(RhythmNote {
                x: (i as f32) * 30.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: (i as f64) * 0.5,
                end_beat: ((i + 1) as f64) * 0.5,
            });
        }
        let mut beam_right_8th = vec![false; 5];
        compute_default_beaming_for_measure(0, &ts_5_8, &notes_8th, &mut beam_right_8th);
        assert_eq!(beam_right_8th, vec![true, true, false, true, false]);

        // 10 sixteenth notes in 5/8 -> beamed 6 + 4 up to break at 1.5
        let mut notes_16th = Vec::new();
        for i in 0..10 {
            notes_16th.push(RhythmNote {
                x: (i as f32) * 20.0,
                rhythm: Rhythm::Sixteenth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: (i as f64) * 0.25,
                end_beat: ((i + 1) as f64) * 0.25,
            });
        }
        let mut beam_right_16th = vec![false; 10];
        compute_default_beaming_for_measure(0, &ts_5_8, &notes_16th, &mut beam_right_16th);
        assert_eq!(
            beam_right_16th,
            vec![true, true, true, true, true, false, true, true, true, false]
        );
    }

    #[test]
    fn test_default_beaming_compound_meters() {
        let ts_6_8 = TimeSignature { count: 6, unit: 8 };

        // 6 eighth notes in 6/8 -> 3 + 3
        let mut notes_8th = Vec::new();
        for i in 0..6 {
            notes_8th.push(RhythmNote {
                x: (i as f32) * 30.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: (i as f64) * 0.5,
                end_beat: ((i + 1) as f64) * 0.5,
            });
        }
        let mut beam_right_8th = vec![false; 6];
        compute_default_beaming_for_measure(0, &ts_6_8, &notes_8th, &mut beam_right_8th);
        assert_eq!(beam_right_8th, vec![true, true, false, true, true, false]);

        // 12 sixteenth notes in 6/8 -> 6 + 6
        let mut notes_16th = Vec::new();
        for i in 0..12 {
            notes_16th.push(RhythmNote {
                x: (i as f32) * 20.0,
                rhythm: Rhythm::Sixteenth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: (i as f64) * 0.25,
                end_beat: ((i + 1) as f64) * 0.25,
            });
        }
        let mut beam_right_16th = vec![false; 12];
        compute_default_beaming_for_measure(0, &ts_6_8, &notes_16th, &mut beam_right_16th);
        assert_eq!(
            beam_right_16th,
            vec![
                true, true, true, true, true, false, // First 6 (span 0.0 - 1.5)
                true, true, true, true, true, false // Second 6 (span 1.5 - 3.0)
            ]
        );

        // 9/16 with 9 sixteenth notes -> 3 + 3 + 3
        let ts_9_16 = TimeSignature { count: 9, unit: 16 };
        let mut notes_9_16 = Vec::new();
        for i in 0..9 {
            notes_9_16.push(RhythmNote {
                x: (i as f32) * 20.0,
                rhythm: Rhythm::Sixteenth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: (i as f64) * 0.25,
                end_beat: ((i + 1) as f64) * 0.25,
            });
        }
        let mut beam_right_9_16 = vec![false; 9];
        compute_default_beaming_for_measure(0, &ts_9_16, &notes_9_16, &mut beam_right_9_16);
        assert_eq!(
            beam_right_9_16,
            vec![true, true, false, true, true, false, true, true, false]
        );
    }

    #[test]
    fn test_default_beaming_7_8() {
        let ts_7_8 = TimeSignature { count: 7, unit: 8 };

        // 7 eighth notes in 7/8 with breaks at 1.5 and 3.0 (3 + 3 + 2 grouping)
        let mut notes_8th = Vec::new();
        for i in 0..7 {
            notes_8th.push(RhythmNote {
                x: (i as f32) * 30.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: (i as f64) * 0.5,
                end_beat: ((i + 1) as f64) * 0.5,
            });
        }
        let mut beam_right_8th = vec![false; 7];
        compute_default_beaming_for_measure(0, &ts_7_8, &notes_8th, &mut beam_right_8th);
        assert_eq!(
            beam_right_8th,
            vec![true, true, false, true, true, false, false]
        );

        // 14 sixteenth notes in 7/8 -> 6 + 6 + 2
        let mut notes_16th = Vec::new();
        for i in 0..14 {
            notes_16th.push(RhythmNote {
                x: (i as f32) * 20.0,
                rhythm: Rhythm::Sixteenth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: (i as f64) * 0.25,
                end_beat: ((i + 1) as f64) * 0.25,
            });
        }
        let mut beam_right_16th = vec![false; 14];
        compute_default_beaming_for_measure(0, &ts_7_8, &notes_16th, &mut beam_right_16th);
        assert_eq!(
            beam_right_16th,
            vec![
                true, true, true, true, true, false, // 6 sixteenths
                true, true, true, true, true, false, // 6 sixteenths
                true, false // 2 sixteenths
            ]
        );
    }

    #[test]
    fn test_default_beaming_tuplet_isolation() {
        let ts_4_4 = TimeSignature { count: 4, unit: 4 };

        // Notes:
        // 0, 1, 2: in Tuplet 1 (triplet eighth notes)
        // 3, 4: outside tuplet (regular eighth notes)
        // 5, 6, 7: in Tuplet 2 (another triplet)
        let notes = vec![
            // Tuplet 1
            RhythmNote {
                x: 0.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: Some(1),
                start_beat: 0.0,
                end_beat: 0.333,
            },
            RhythmNote {
                x: 20.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: Some(1),
                start_beat: 0.333,
                end_beat: 0.666,
            },
            RhythmNote {
                x: 40.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: Some(1),
                start_beat: 0.666,
                end_beat: 1.0,
            },
            // Non-tuplet notes on beat 2
            RhythmNote {
                x: 70.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 1.0,
                end_beat: 1.5,
            },
            RhythmNote {
                x: 100.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 1.5,
                end_beat: 2.0,
            },
            // Tuplet 2 on beat 3
            RhythmNote {
                x: 130.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: Some(2),
                start_beat: 2.0,
                end_beat: 2.333,
            },
            RhythmNote {
                x: 150.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: Some(2),
                start_beat: 2.333,
                end_beat: 2.666,
            },
            RhythmNote {
                x: 170.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: Some(2),
                start_beat: 2.666,
                end_beat: 3.0,
            },
        ];

        let mut beam_right = vec![false; 8];
        compute_default_beaming_for_measure(0, &ts_4_4, &notes, &mut beam_right);

        assert_eq!(
            beam_right,
            vec![
                true, true,
                false, // Tuplet 1 notes beam together; note 2 does not beam to note 3
                true,
                false, // Non-tuplet notes 3 & 4 beam together; note 4 does not beam into Tuplet 2
                true, true, false // Tuplet 2 notes beam together
            ]
        );
    }

    #[test]
    fn test_default_beaming_rests_and_quarter_notes() {
        let ts_4_4 = TimeSignature { count: 4, unit: 4 };

        let notes = vec![
            RhythmNote {
                x: 0.0,
                rhythm: Rhythm::Quarter,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 0.0,
                end_beat: 1.0,
            },
            RhythmNote {
                x: 30.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 1.0,
                end_beat: 1.5,
            },
            RhythmNote {
                x: 60.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: true, // Rest
                tuplet_id: None,
                start_beat: 1.5,
                end_beat: 2.0,
            },
            RhythmNote {
                x: 90.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 2.0,
                end_beat: 2.5,
            },
            RhythmNote {
                x: 120.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 2.5,
                end_beat: 3.0,
            },
        ];

        let mut beam_right = vec![false; 5];
        compute_default_beaming_for_measure(0, &ts_4_4, &notes, &mut beam_right);

        // Quarter note: no beam
        // Eighth note before rest: no beam to rest
        // Rest: no beam
        // Eighth notes on beat 3 (2.0 and 2.5): beam together
        assert_eq!(beam_right, vec![false, false, false, true, false]);
    }

    #[test]
    fn test_grace_notes_do_not_alter_regular_rhythm_beaming() {
        let ts_4_4 = TimeSignature { count: 4, unit: 4 };

        // Test 1: Baseline 4 regular eighth notes in 4/4 (two pairs: 0-1 and 2-3)
        let baseline_notes = vec![
            RhythmNote {
                x: 0.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 0.0,
                end_beat: 0.5,
            },
            RhythmNote {
                x: 30.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 0.5,
                end_beat: 1.0,
            },
            RhythmNote {
                x: 60.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 1.0,
                end_beat: 1.5,
            },
            RhythmNote {
                x: 90.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 1.5,
                end_beat: 2.0,
            },
        ];
        let mut baseline_to = vec![None; 4];
        let mut baseline_from = vec![None; 4];
        compute_default_beaming_indices(
            0,
            &ts_4_4,
            &baseline_notes,
            &std::collections::HashSet::new(),
            &mut baseline_to,
            &mut baseline_from,
        );
        // In 4/4, four 8th notes beam up to beat 2.0
        assert_eq!(baseline_to[0], Some(1));
        assert_eq!(baseline_to[1], Some(2));
        assert_eq!(baseline_to[2], Some(3));
        assert_eq!(baseline_to[3], None);

        // Test 2: Insert one 16th grace note before note 1
        // Indices:
        // 0: Regular 8th (beat 0.0)
        // 1: Grace 16th (beat 0.5, grace)
        // 2: Regular 8th (beat 0.5)
        // 3: Regular 8th (beat 1.0)
        // 4: Regular 8th (beat 1.5)
        let mut grace_set = std::collections::HashSet::new();
        grace_set.insert(1);
        let notes_with_one_grace = vec![
            RhythmNote {
                x: 0.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 0.0,
                end_beat: 0.5,
            },
            RhythmNote {
                x: 19.0,
                rhythm: Rhythm::Sixteenth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 0.5,
                end_beat: 0.5,
            },
            RhythmNote {
                x: 38.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 0.5,
                end_beat: 1.0,
            },
            RhythmNote {
                x: 68.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 1.0,
                end_beat: 1.5,
            },
            RhythmNote {
                x: 98.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 1.5,
                end_beat: 2.0,
            },
        ];
        let mut to = vec![None; 5];
        let mut from = vec![None; 5];
        compute_default_beaming_indices(
            0,
            &ts_4_4,
            &notes_with_one_grace,
            &grace_set,
            &mut to,
            &mut from,
        );

        // Note 0 (regular) MUST beam directly to Note 2 (regular), across Grace note 1!
        assert_eq!(to[0], Some(2), "Regular note 0 must beam to regular note 2");
        assert_eq!(from[2], Some(0), "Regular note 2 receives beam from regular note 0");
        // Grace note 1 must NOT beam to regular note 2
        assert_eq!(to[1], None, "Grace note 1 must not beam to regular note 2");
        assert_eq!(from[1], None, "Grace note 1 must not receive beam from regular note 0");
        // Regular notes continue their normal beam
        assert_eq!(to[2], Some(3), "Regular note 2 beams to regular note 3");
        assert_eq!(to[3], Some(4), "Regular note 3 beams to regular note 4");
        assert_eq!(to[4], None);

        // Test 3: Insert two 16th grace notes before note 1
        // Indices:
        // 0: Regular 8th (beat 0.0)
        // 1: Grace 16th
        // 2: Grace 16th
        // 3: Regular 8th (beat 0.5)
        let mut multi_grace_set = std::collections::HashSet::new();
        multi_grace_set.insert(1);
        multi_grace_set.insert(2);
        let notes_with_two_grace = vec![
            RhythmNote {
                x: 0.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 0.0,
                end_beat: 0.5,
            },
            RhythmNote {
                x: 19.0,
                rhythm: Rhythm::Sixteenth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 0.5,
                end_beat: 0.5,
            },
            RhythmNote {
                x: 38.0,
                rhythm: Rhythm::Sixteenth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 0.5,
                end_beat: 0.5,
            },
            RhythmNote {
                x: 57.0,
                rhythm: Rhythm::Eighth,
                is_dotted: false,
                measure_idx: 0,
                is_rest: false,
                tuplet_id: None,
                start_beat: 0.5,
                end_beat: 1.0,
            },
        ];
        let mut multi_to = vec![None; 4];
        let mut multi_from = vec![None; 4];
        compute_default_beaming_indices(
            0,
            &ts_4_4,
            &notes_with_two_grace,
            &multi_grace_set,
            &mut multi_to,
            &mut multi_from,
        );

        // Regular note 0 beams to Regular note 3!
        assert_eq!(multi_to[0], Some(3), "Regular note 0 beams across both grace notes to regular note 3");
        assert_eq!(multi_from[3], Some(0));
        // Grace note 1 beams to Grace note 2!
        assert_eq!(multi_to[1], Some(2), "Grace note 1 beams to Grace note 2");
        assert_eq!(multi_from[2], Some(1));
        // Grace note 2 does NOT beam to regular note 3!
        assert_eq!(multi_to[2], None, "Grace note 2 must not beam to regular note 3");
    }

    #[test]
    fn test_event_text_width_calculations() {
        use crate::mnx::{Duration, Event, Harmonic, Note, NoteExt, Pitch};

        let dummy = |fret: i32, harmonic: Option<f64>| Note {
            pitch: Pitch { step: "C".into(), alter: None, octave: 4 },
            ext: NoteExt {
                fret: Some(fret),
                string: Some(1),
                harmonic: harmonic.map(|t| Harmonic {
                    touch: t,
                    sounding_note: Box::new(Note {
                        pitch: Pitch { step: "C".into(), alter: None, octave: 5 },
                        ..Default::default()
                    }),
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        // Single digit note (fret 7)
        let single_digit_ev = Event {
            duration: Duration { base: "quarter".into(), dots: None, multiple: None },
            notes: vec![dummy(7, None)],
            ..Default::default()
        };
        assert!((event_text_width(&single_digit_ev, false, None) - 8.45).abs() < 1e-4);
        assert!((event_text_width(&single_digit_ev, true, None) - 6.05).abs() < 1e-4);

        // Double digit note (fret 12)
        let double_digit_ev = Event {
            duration: Duration { base: "quarter".into(), dots: None, multiple: None },
            notes: vec![dummy(12, None)],
            ..Default::default()
        };
        assert!((event_text_width(&double_digit_ev, false, None) - 16.90).abs() < 1e-4);

        // Natural harmonic <12> (4 chars)
        let harmonic_12_ev = Event {
            duration: Duration { base: "quarter".into(), dots: None, multiple: None },
            notes: vec![dummy(12, Some(12.0))],
            ..Default::default()
        };
        assert!((event_text_width(&harmonic_12_ev, false, None) - 33.80).abs() < 1e-4);

        // Natural harmonic <3.9> (5 chars)
        let harmonic_3_9_ev = Event {
            duration: Duration { base: "quarter".into(), dots: None, multiple: None },
            notes: vec![dummy(4, Some(3.9))],
            ..Default::default()
        };
        assert!((event_text_width(&harmonic_3_9_ev, false, None) - 42.25).abs() < 1e-4);

        // Natural harmonic <20.5> (6 chars)
        let harmonic_20_5_ev = Event {
            duration: Duration { base: "quarter".into(), dots: None, multiple: None },
            notes: vec![dummy(21, Some(20.5))],
            ..Default::default()
        };
        assert!((event_text_width(&harmonic_20_5_ev, false, None) - 50.70).abs() < 1e-4);
        assert!((event_text_width(&harmonic_20_5_ev, true, None) - 36.30).abs() < 1e-4);
    }

    #[test]
    fn test_calculate_event_spacing_wide_entries() {
        use crate::mnx::{Duration, Event, Harmonic, Note, NoteExt, Pitch};

        let quarter_d = Duration { base: "quarter".into(), dots: None, multiple: None };
        let sixteenth_d = Duration { base: "16th".into(), dots: None, multiple: None };

        let make_note_ev = |dur: Duration, fret: i32, harmonic: Option<f64>| Event {
            duration: dur,
            notes: vec![Note {
                pitch: Pitch { step: "C".into(), alter: None, octave: 4 },
                ext: NoteExt {
                    string: Some(1),
                    fret: Some(fret),
                    harmonic: harmonic.map(|t| Harmonic {
                        touch: t,
                        sounding_note: Box::new(Note {
                            pitch: Pitch { step: "C".into(), alter: None, octave: 5 },
                            ..Default::default()
                        }),
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }],
            ..Default::default()
        };

        let ev_single = make_note_ev(quarter_d.clone(), 7, None);
        let ev_16th_single = make_note_ev(sixteenth_d.clone(), 7, None);
        let ev_double = make_note_ev(sixteenth_d.clone(), 12, None);
        let ev_h12 = make_note_ev(sixteenth_d.clone(), 12, Some(12.0));
        let ev_h3_9 = make_note_ev(sixteenth_d.clone(), 4, Some(3.9));
        let ev_h20_5 = make_note_ev(sixteenth_d.clone(), 21, Some(20.5));

        let base_spacing = 30.0;

        // Baseline single-digit quarter note keeps 30.0
        let sp_single_q = calculate_event_spacing(&ev_single, false, base_spacing, Some((&ev_single, false)), None, None);
        assert_eq!(sp_single_q, 30.0);

        // Baseline single-digit 16th notes keep 22.5 (0.75 * 30.0)
        let sp_single_16 = calculate_event_spacing(&ev_16th_single, false, base_spacing, Some((&ev_16th_single, false)), None, None);
        assert_eq!(sp_single_16, 22.5);

        // Double-digit notes expand beyond 18.0 (to at least 28.0)
        let sp_double = calculate_event_spacing(&ev_double, false, base_spacing, Some((&ev_double, false)), None, None);
        assert!(sp_double >= 28.0, "Double-digit notes must expand beyond 18.0, got {}", sp_double);

        // Harmonic <12> (4 chars) expands to >= 47.0
        let sp_h12 = calculate_event_spacing(&ev_h12, false, base_spacing, Some((&ev_h12, false)), None, None);
        assert!(sp_h12 >= 47.0, "Harmonic <12> must expand to >= 47.0, got {}", sp_h12);

        // Harmonic <3.9> (5 chars) expands to >= 56.0
        let sp_h3_9 = calculate_event_spacing(&ev_h3_9, false, base_spacing, Some((&ev_h3_9, false)), None, None);
        assert!(sp_h3_9 >= 56.0, "Harmonic <3.9> must expand to >= 56.0, got {}", sp_h3_9);

        // Harmonic <20.5> (6 chars) followed by <20.5> expands to >= 64.0
        let sp_h20_5 = calculate_event_spacing(&ev_h20_5, false, base_spacing, Some((&ev_h20_5, false)), None, None);
        assert!(sp_h20_5 >= 64.0, "Harmonic <20.5> must expand to >= 64.0, got {}", sp_h20_5);

        // Harmonic <20.5> followed by single-digit expands to >= 43.0
        let sp_h20_5_single = calculate_event_spacing(&ev_h20_5, false, base_spacing, Some((&ev_single, false)), None, None);
        assert!(sp_h20_5_single >= 43.0, "Harmonic <20.5> before single note must expand to >= 43.0, got {}", sp_h20_5_single);

        // Single digit followed by <20.5> also expands to >= 43.0 to prevent right-hand note from colliding
        let sp_single_h20_5 = calculate_event_spacing(&ev_16th_single, false, base_spacing, Some((&ev_h20_5, false)), None, None);
        assert!(sp_single_h20_5 >= 43.0, "Single note before <20.5> must expand to >= 43.0, got {}", sp_single_h20_5);

        // Harmonic <20.5> as last note before barline expands to >= 37.0
        let sp_h20_5_barline = calculate_event_spacing(&ev_h20_5, false, base_spacing, None, None, None);
        assert!(sp_h20_5_barline >= 37.0, "Harmonic <20.5> before barline must expand to >= 37.0, got {}", sp_h20_5_barline);

        // Grace note with <20.5> expands to >= 46.0
        let sp_grace_h20_5 = calculate_event_spacing(&ev_h20_5, true, base_spacing, Some((&ev_h20_5, true)), None, None);
        assert!(sp_grace_h20_5 >= 46.0, "Grace harmonic <20.5> must expand to >= 46.0, got {}", sp_grace_h20_5);
    }

    #[test]
    fn test_lyric_rod_stretching() {
        use crate::mnx::{Duration, Event, EventLyrics, LyricLine};
        use std::collections::BTreeMap;

        let sixteenth_d = Duration { base: "16th".into(), dots: None, multiple: None };
        let mut lines1 = BTreeMap::new();
        lines1.insert(
            "1".into(),
            LyricLine {
                text: "extraordinary".into(),
                ..Default::default()
            },
        );
        let ev_lyric = Event {
            duration: sixteenth_d.clone(),
            lyrics: Some(EventLyrics { lines: lines1 }),
            ..Default::default()
        };
        let mut lines2 = BTreeMap::new();
        lines2.insert(
            "1".into(),
            LyricLine {
                text: "voice".into(),
                ..Default::default()
            },
        );
        let ev_next = Event {
            duration: sixteenth_d.clone(),
            lyrics: Some(EventLyrics { lines: lines2 }),
            ..Default::default()
        };

        // Natural 16th base spacing is 22.5.
        // Long syllable "extraordinary" (13 chars) + "voice" (5 chars) must push spacing far out to prevent collision.
        let sp = calculate_event_spacing(&ev_lyric, false, 30.0, Some((&ev_next, false)), None, None);
        assert!(sp > 70.0, "Lyric syllables must push spacing beyond natural 16th, got {}", sp);
    }
}

pub enum LayoutItem<'a> {
    Event(&'a crate::mnx::Event),
    TupletStart(&'a crate::mnx::Tuplet),
    TupletEnd,
    GraceStart,
    GraceEnd,
}

impl crate::mnx::SequenceItem {
    pub fn to_layout_items<'a>(&'a self, out: &mut Vec<LayoutItem<'a>>) {
        match self {
            crate::mnx::SequenceItem::Event(e) => out.push(LayoutItem::Event(e)),
            crate::mnx::SequenceItem::Tuplet(t) => {
                out.push(LayoutItem::TupletStart(t));
                for inner in &t.content {
                    inner.to_layout_items(out);
                }
                out.push(LayoutItem::TupletEnd);
            }
            crate::mnx::SequenceItem::Grace(g) => {
                out.push(LayoutItem::GraceStart);
                for inner in &g.content {
                    inner.to_layout_items(out);
                }
                out.push(LayoutItem::GraceEnd);
            }
        }
    }
}

pub fn apply_beam_group_indices(
    group: &crate::mnx::BeamGroup,
    event_id_to_idx: &std::collections::HashMap<String, usize>,
    rhythm_notes: &[RhythmNote],
    beam_to: &mut [Option<usize>],
    beam_from: &mut [Option<usize>],
) {
    for window in group.events.windows(2) {
        if let (Some(&ia), Some(&ib)) = (
            event_id_to_idx.get(&window[0]),
            event_id_to_idx.get(&window[1]),
        ) {
            if ib > ia
                && !rhythm_notes[ia].is_rest
                && !rhythm_notes[ib].is_rest
                && rhythm_notes[ia].rhythm.flag_count() >= 1
                && rhythm_notes[ib].rhythm.flag_count() >= 1
            {
                beam_to[ia] = Some(ib);
                beam_from[ib] = Some(ia);
            }
        }
    }
    if let Some(inner_beams) = &group.beams {
        for inner in inner_beams {
            apply_beam_group_indices(inner, event_id_to_idx, rhythm_notes, beam_to, beam_from);
        }
    }
}

#[allow(dead_code)]
pub fn apply_beam_group(
    group: &crate::mnx::BeamGroup,
    event_id_to_idx: &std::collections::HashMap<String, usize>,
    rhythm_notes: &[RhythmNote],
    beam_right: &mut [bool],
) {
    let mut beam_to = vec![None; rhythm_notes.len()];
    let mut beam_from = vec![None; rhythm_notes.len()];
    apply_beam_group_indices(group, event_id_to_idx, rhythm_notes, &mut beam_to, &mut beam_from);
    for i in 0..rhythm_notes.len() {
        if beam_to[i].is_some() {
            beam_right[i] = true;
        }
    }
}

/// Finds the applicable TimeSignature for a measure index by inspecting global.measures up to measure_idx,
/// defaulting to 4/4 if not specified.
pub fn get_time_signature_for_measure(
    global: Option<&crate::mnx::Global>,
    measure_idx: usize,
) -> TimeSignature {
    if let Some(g) = global {
        let max_check = measure_idx.min(g.measures.len().saturating_sub(1));
        if !g.measures.is_empty() {
            for i in (0..=max_check).rev() {
                if let Some(time) = g.measures[i].time {
                    return time;
                }
            }
        }
    }
    TimeSignature { count: 4, unit: 4 }
}

/// Computes beam break points (in quarter-note beats from start of measure)
/// based on the measure's time signature and whether sixteenth notes (or shorter) are present.
pub fn get_beam_breaks(time_sig: &TimeSignature, has_sub_eighth: bool) -> Vec<f64> {
    let count = time_sig.count;
    let unit = time_sig.unit;
    let unit_beat = 4.0 / (unit as f64);
    let total_beats = (count as f64) * unit_beat;

    let mut breaks = Vec::new();

    if unit >= 8 {
        // Compound / odd meters with eighth-note or smaller unit
        let group_sizes: Vec<usize> = if count == 5 {
            vec![3, 2]
        } else if count % 3 == 0 {
            vec![3; (count / 3) as usize]
        } else if count == 7 {
            vec![3, 3, 2]
        } else if count % 2 == 0 {
            vec![2; (count / 2) as usize]
        } else {
            vec![1; count as usize]
        };

        let mut acc = 0.0;
        for sz in group_sizes {
            acc += (sz as f64) * unit_beat;
            breaks.push(acc);
        }
    } else if unit == 4 {
        if count == 4 {
            // 4/4: Eighth notes beam up to 4 at a time (half-measure breaks at 2.0, 4.0).
            // Sixteenth notes (or shorter) break at each quarter-note beat (1.0, 2.0, 3.0, 4.0).
            if has_sub_eighth {
                breaks = vec![1.0, 2.0, 3.0, 4.0];
            } else {
                breaks = vec![2.0, 4.0];
            }
        } else {
            // Other time signatures with quarter-note beats (e.g. 2/4, 3/4, 5/4, 6/4):
            // "in time signatures other than 4/4, eighth notes should generally be beamed two at a time"
            // Break at every 1.0 beat (every quarter note).
            let mut acc = 0.0;
            while acc + 1.0 <= total_beats + 1e-4 {
                acc += 1.0;
                breaks.push(acc);
            }
        }
    } else {
        // Other units (e.g. unit == 2, half notes): break every unit_beat or 1.0 beat
        let step = unit_beat.min(1.0);
        let mut acc = 0.0;
        while acc + step <= total_beats + 1e-4 {
            acc += step;
            breaks.push(acc);
        }
    }

    breaks
}

/// Applies default beaming rules for a given measure when explicit beams are absent,
/// connecting notes by their target indices and keeping regular notes and grace notes separate.
pub fn compute_default_beaming_indices(
    measure_idx: usize,
    time_sig: &TimeSignature,
    rhythm_notes: &[RhythmNote],
    grace_event_indices: &std::collections::HashSet<usize>,
    beam_to: &mut [Option<usize>],
    beam_from: &mut [Option<usize>],
) {
    let n = rhythm_notes.len();

    // 1. Regular notes: beam consecutive regular notes in measure_idx
    let reg_indices: Vec<usize> = (0..n)
        .filter(|&i| rhythm_notes[i].measure_idx == measure_idx && !grace_event_indices.contains(&i))
        .collect();

    for k in 0..reg_indices.len().saturating_sub(1) {
        let ia = reg_indices[k];
        let ib = reg_indices[k + 1];
        let note_a = &rhythm_notes[ia];
        let note_b = &rhythm_notes[ib];

        // Neither note can be a rest, and both must require beams (flag_count >= 1)
        if note_a.is_rest || note_b.is_rest {
            continue;
        }
        if note_a.rhythm.flag_count() == 0 || note_b.rhythm.flag_count() == 0 {
            continue;
        }

        // Tuplet constraint:
        // - notes of a tuplet should beam to notes inside the tuplet and NOT to notes outside the tuplet
        if note_a.tuplet_id.is_some() || note_b.tuplet_id.is_some() {
            if note_a.tuplet_id == note_b.tuplet_id {
                beam_to[ia] = Some(ib);
                beam_from[ib] = Some(ia);
            }
            continue;
        }

        // Non-tuplet notes: apply time signature beam breaks
        let has_sub_eighth = note_a.rhythm.flag_count() >= 2 || note_b.rhythm.flag_count() >= 2;
        let breaks = get_beam_breaks(time_sig, has_sub_eighth);
        let eps = 1e-4;
        let has_break = breaks
            .iter()
            .any(|&b| note_a.start_beat + eps < b && b <= note_b.start_beat + eps);

        if !has_break {
            beam_to[ia] = Some(ib);
            beam_from[ib] = Some(ia);
        }
    }

    // 2. Grace notes: beam adjacent grace notes within same measure
    let grace_indices: Vec<usize> = (0..n)
        .filter(|&i| rhythm_notes[i].measure_idx == measure_idx && grace_event_indices.contains(&i))
        .collect();

    for &ia in &grace_indices {
        let ib = ia + 1;
        if ib < n && grace_event_indices.contains(&ib) && rhythm_notes[ib].measure_idx == measure_idx {
            let note_a = &rhythm_notes[ia];
            let note_b = &rhythm_notes[ib];
            if !note_a.is_rest
                && !note_b.is_rest
                && note_a.rhythm.flag_count() >= 1
                && note_b.rhythm.flag_count() >= 1
            {
                beam_to[ia] = Some(ib);
                beam_from[ib] = Some(ia);
            }
        }
    }
}

/// Applies default beaming rules for a given measure when explicit beams are absent.
#[allow(dead_code)]
pub fn compute_default_beaming_for_measure(
    measure_idx: usize,
    time_sig: &TimeSignature,
    rhythm_notes: &[RhythmNote],
    beam_right: &mut [bool],
) {
    let empty_grace = std::collections::HashSet::new();
    let mut beam_to = vec![None; rhythm_notes.len()];
    let mut beam_from = vec![None; rhythm_notes.len()];
    compute_default_beaming_indices(
        measure_idx,
        time_sig,
        rhythm_notes,
        &empty_grace,
        &mut beam_to,
        &mut beam_from,
    );
    for i in 0..rhythm_notes.len() {
        if beam_to[i].is_some() {
            beam_right[i] = true;
        }
    }
}
