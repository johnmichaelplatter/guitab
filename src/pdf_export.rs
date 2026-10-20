use printpdf::path::{PaintMode, WindingOrder};
use printpdf::*;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::layout::display_list::{DisplayList, DrawItem, DrawKind};
use crate::layout::PageLayout;
use crate::mnx::MnxDocument;
use crate::render::{LayoutItem, Rhythm, RhythmNote};

pub fn export_score_to_pdf<P: AsRef<Path>>(
    doc: &MnxDocument,
    pages: &[PageLayout],
    output_path: P,
) -> Result<(), String> {
    let pdf_bytes = generate_score_pdf_bytes(doc, pages)?;
    std::fs::write(output_path, pdf_bytes).map_err(|e| format!("Failed to write PDF: {}", e))?;
    Ok(())
}

pub fn generate_score_pdf_bytes(
    doc: &MnxDocument,
    pages: &[PageLayout],
) -> Result<Vec<u8>, String> {
    if pages.is_empty() {
        return Err("No pages to export".to_string());
    }

    let first_page = &pages[0];
    let (page_w_mm, page_h_mm) = if first_page.width > 800.0 && first_page.height > 1100.0 {
        // A4: 210 x 297 mm
        (Mm(210.0), Mm(297.0))
    } else {
        // US Letter: 8.5 x 11 in = 215.9 x 279.4 mm
        (Mm(215.9), Mm(279.4))
    };

    let title = doc
        .parts
        .first()
        .and_then(|p| p.name.as_deref())
        .unwrap_or("Score");

    let (pdf_doc, page_idx, layer_idx) =
        PdfDocument::new(title, page_w_mm, page_h_mm, "Score Layer");

    // Load fonts
    let bravura_bytes = include_bytes!("../assets/Bravura.otf");
    let bravura_font = pdf_doc
        .add_external_font(std::io::Cursor::new(bravura_bytes))
        .map_err(|e| format!("Failed to load Bravura font: {:?}", e))?;

    let text_font = pdf_doc
        .add_external_font(std::io::Cursor::new(epaint_default_fonts::UBUNTU_LIGHT))
        .or_else(|_| pdf_doc.add_builtin_font(BuiltinFont::Helvetica))
        .map_err(|e| format!("Failed to load text font: {:?}", e))?;

    // Scale factors: from layout points (screen pixels ~96DPI) to PDF Mm and Pt
    // 1 px = 72.0 / 96.0 pt = 0.75 pt
    let pt_per_px = 72.0_f32 / 96.0_f32;
    // 1 inch = 96 px = 25.4 mm => mm_per_px = 25.4 / 96.0 ≈ 0.264583
    let mm_per_px = 25.4_f32 / 96.0_f32;
    let page_h_val = page_h_mm.0;

    for (p_i, page_layout) in pages.iter().enumerate() {
        let current_layer = if p_i == 0 {
            pdf_doc.get_page(page_idx).get_layer(layer_idx)
        } else {
            let (new_page, new_layer) =
                pdf_doc.add_page(page_w_mm, page_h_mm, format!("Page {}", p_i + 1));
            pdf_doc.get_page(new_page).get_layer(new_layer)
        };

        // Render page systems and content to a DisplayList
        let mut display_list = DisplayList::new();
        populate_page_display_list(doc, page_layout, &mut display_list);

        if let Some(layout_cfg) = &doc.layout {
            display_list.apply_offsets(&layout_cfg.offsets);
        }

        // Draw items
        for item in &display_list.items {
            match &item.kind {
                DrawKind::Line { from, to, stroke } => {
                    let pt1 = Point::new(
                        Mm(from.x * mm_per_px),
                        Mm(page_h_val - from.y * mm_per_px),
                    );
                    let pt2 = Point::new(
                        Mm(to.x * mm_per_px),
                        Mm(page_h_val - to.y * mm_per_px),
                    );
                    let line = Line {
                        points: vec![(pt1, false), (pt2, false)],
                        is_closed: false,
                    };
                    current_layer.set_outline_thickness(stroke.width * pt_per_px);
                    let r = stroke.color.r() as f32 / 255.0;
                    let g = stroke.color.g() as f32 / 255.0;
                    let b = stroke.color.b() as f32 / 255.0;
                    current_layer.set_outline_color(Color::Rgb(Rgb::new(r, g, b, None)));
                    current_layer.add_line(line);
                }
                DrawKind::Rect { rect, fill, stroke, .. } => {
                    let p0 = Point::new(Mm(rect.min.x * mm_per_px), Mm(page_h_val - rect.max.y * mm_per_px));
                    let p1 = Point::new(Mm(rect.max.x * mm_per_px), Mm(page_h_val - rect.max.y * mm_per_px));
                    let p2 = Point::new(Mm(rect.max.x * mm_per_px), Mm(page_h_val - rect.min.y * mm_per_px));
                    let p3 = Point::new(Mm(rect.min.x * mm_per_px), Mm(page_h_val - rect.min.y * mm_per_px));
                    let has_fill = fill.a() > 0;
                    let has_stroke = stroke.width > 0.0;
                    let mode = match (has_fill, has_stroke) {
                        (true, true) => PaintMode::FillStroke,
                        (true, false) => PaintMode::Fill,
                        (false, true) => PaintMode::Stroke,
                        (false, false) => PaintMode::Clip,
                    };

                    if has_stroke {
                        current_layer.set_outline_thickness(stroke.width * pt_per_px);
                        current_layer.set_outline_color(Color::Rgb(Rgb::new(0.2, 0.2, 0.2, None)));
                    }
                    if has_fill {
                        let r = fill.r() as f32 / 255.0;
                        let g = fill.g() as f32 / 255.0;
                        let b = fill.b() as f32 / 255.0;
                        current_layer.set_fill_color(Color::Rgb(Rgb::new(r, g, b, None)));
                    }
                    let rect_shape = Polygon {
                        rings: vec![vec![(p0, false), (p1, false), (p2, false), (p3, false)]],
                        mode,
                        winding_order: WindingOrder::NonZero,
                    };
                    current_layer.add_polygon(rect_shape);
                }
                DrawKind::Circle { center, radius, fill } => {
                    let num_pts = 12;
                    let mut poly_pts = Vec::new();
                    for i in 0..num_pts {
                        let angle = (i as f32) * std::f32::consts::TAU / (num_pts as f32);
                        let px = center.x + radius * angle.cos();
                        let py = center.y + radius * angle.sin();
                        poly_pts.push((Point::new(Mm(px * mm_per_px), Mm(page_h_val - py * mm_per_px)), false));
                    }
                    let r = fill.r() as f32 / 255.0;
                    let g = fill.g() as f32 / 255.0;
                    let b = fill.b() as f32 / 255.0;
                    current_layer.set_fill_color(Color::Rgb(Rgb::new(r, g, b, None)));
                    let circle_poly = Polygon {
                        rings: vec![poly_pts],
                        mode: PaintMode::Fill,
                        winding_order: WindingOrder::NonZero,
                    };
                    current_layer.add_polygon(circle_poly);
                }
                DrawKind::Text { pos, align, text, font_id, color } => {
                    let is_bravura = match &font_id.family {
                        eframe::egui::FontFamily::Name(name) => name.as_ref() == "Bravura" || name.as_ref() == "BravuraText",
                        _ => text.chars().any(|c| (c as u32) >= 0xE000 && (c as u32) <= 0xF8FF),
                    };
                    let approx_w = (text.chars().count() as f32) * font_id.size * (if is_bravura { 0.48 } else { 0.52 });
                    let approx_h = font_id.size * 0.75;
                    let adj_x = match align.x() {
                        eframe::emath::Align::Min => pos.x,
                        eframe::emath::Align::Center => pos.x - approx_w * 0.5,
                        eframe::emath::Align::Max => pos.x - approx_w,
                    };
                    let adj_y = match align.y() {
                        eframe::emath::Align::Min => pos.y + approx_h * (if is_bravura { 0.5 } else { 1.0 }),
                        eframe::emath::Align::Center => if is_bravura { pos.y } else { pos.y + approx_h * 0.35 },
                        eframe::emath::Align::Max => pos.y,
                    };

                    let x = Mm(adj_x * mm_per_px);
                    let y = Mm(page_h_val - adj_y * mm_per_px);
                    let font_ref = if is_bravura {
                        &bravura_font
                    } else {
                        &text_font
                    };
                    let r = color.r() as f32 / 255.0;
                    let g = color.g() as f32 / 255.0;
                    let b = color.b() as f32 / 255.0;
                    current_layer.set_fill_color(Color::Rgb(Rgb::new(r, g, b, None)));
                    current_layer.use_text(text, font_id.size * pt_per_px, x, y, font_ref);
                }
                DrawKind::Glyph { pos, align, codepoint, size, color, .. } => {
                    let approx_w = *size * 0.48;
                    let approx_h = *size * 0.75;
                    let adj_x = match align.x() {
                        eframe::emath::Align::Min => pos.x,
                        eframe::emath::Align::Center => pos.x - approx_w * 0.5,
                        eframe::emath::Align::Max => pos.x - approx_w,
                    };
                    let adj_y = match align.y() {
                        eframe::emath::Align::Min => pos.y + approx_h * 0.5,
                        eframe::emath::Align::Center => pos.y,
                        eframe::emath::Align::Max => pos.y,
                    };
                    let x = Mm(adj_x * mm_per_px);
                    let y = Mm(page_h_val - adj_y * mm_per_px);
                    let r = color.r() as f32 / 255.0;
                    let g = color.g() as f32 / 255.0;
                    let b = color.b() as f32 / 255.0;
                    current_layer.set_fill_color(Color::Rgb(Rgb::new(r, g, b, None)));
                    current_layer.use_text(codepoint.to_string(), *size * pt_per_px, x, y, &bravura_font);
                }
                DrawKind::Curve { points, stroke } => {
                    let mut curve_pts = Vec::new();
                    let num_steps = 12;
                    for step in 0..=num_steps {
                        let t = step as f32 / num_steps as f32;
                        let t_inv = 1.0 - t;
                        let x = t_inv * t_inv * t_inv * points[0].x
                            + 3.0 * t_inv * t_inv * t * points[1].x
                            + 3.0 * t_inv * t * t * points[2].x
                            + t * t * t * points[3].x;
                        let y = t_inv * t_inv * t_inv * points[0].y
                            + 3.0 * t_inv * t_inv * t * points[1].y
                            + 3.0 * t_inv * t * t * points[2].y
                            + t * t * t * points[3].y;
                        let pt = Point::new(Mm(x * mm_per_px), Mm(page_h_val - y * mm_per_px));
                        curve_pts.push((pt, false));
                    }
                    let line = Line {
                        points: curve_pts,
                        is_closed: false,
                    };
                    current_layer.set_outline_thickness(stroke.width * pt_per_px);
                    let r = stroke.color.r() as f32 / 255.0;
                    let g = stroke.color.g() as f32 / 255.0;
                    let b = stroke.color.b() as f32 / 255.0;
                    current_layer.set_outline_color(Color::Rgb(Rgb::new(r, g, b, None)));
                    current_layer.add_line(line);
                }
            }
        }
    }

    pdf_doc.save_to_bytes().map_err(|e| format!("Failed to save PDF: {:?}", e))
}

/// Populates a DisplayList with the score graphics for a specific page layout.
pub fn populate_page_display_list(
    doc: &MnxDocument,
    page: &PageLayout,
    dl: &mut DisplayList,
) {
    use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke};

    let part = match doc.parts.first() {
        Some(p) => p,
        None => return,
    };
    let num_strings = part.num_strings();
    let line_spacing = 11.0_f32;
    let staff_height = (num_strings as f32 - 1.0).max(0.0) * line_spacing;

    let text_color = Color32::from_rgb(20, 20, 20);
    let staff_color = Color32::from_rgb(100, 100, 100);
    let barline_color = Color32::from_rgb(60, 60, 60);

    // Header on Page 1
    if page.page_number == 1 {
        let title_text = doc
            .title
            .as_deref()
            .or_else(|| doc.layout.as_ref().and_then(|l| l.title.as_deref()))
            .or_else(|| part.name.as_deref())
            .unwrap_or("Title");

        if !title_text.is_empty() {
            dl.push(DrawItem {
                id: Some("header_title".to_string()),
                kind: DrawKind::Text {
                    pos: Pos2::new(page.width / 2.0, page.margins.top + 16.0),
                    align: Align2::CENTER_CENTER,
                    text: title_text.to_string(),
                    font_id: FontId::proportional(22.0),
                    color: text_color,
                },
                z_index: 10,
            });
        }

        let edo_sub = part
            .micro_fret_divisions
            .map(|divs| format!("{} EDO", divs));
        let subtitle_text = doc
            .subtitle
            .as_deref()
            .or_else(|| doc.layout.as_ref().and_then(|l| l.subtitle.as_deref()))
            .or_else(|| edo_sub.as_deref());

        if let Some(sub) = subtitle_text {
            if !sub.is_empty() {
                dl.push(DrawItem {
                    id: Some("header_subtitle".to_string()),
                    kind: DrawKind::Text {
                        pos: Pos2::new(page.width / 2.0, page.margins.top + 42.0),
                        align: Align2::CENTER_CENTER,
                        text: sub.to_string(),
                        font_id: FontId::proportional(12.0),
                        color: Color32::from_rgb(100, 100, 100),
                    },
                    z_index: 10,
                });
            }
        }

        let composer_text = doc
            .composer
            .as_deref()
            .or_else(|| doc.layout.as_ref().and_then(|l| l.composer.as_deref()))
            .unwrap_or("Composer");

        if !composer_text.is_empty() {
            dl.push(DrawItem {
                id: Some("header_composer".to_string()),
                kind: DrawKind::Text {
                    pos: Pos2::new(page.width - page.margins.right, page.margins.top + 42.0),
                    align: Align2::RIGHT_CENTER,
                    text: composer_text.to_string(),
                    font_id: FontId::proportional(12.0),
                    color: text_color,
                },
                z_index: 10,
            });
        }
    }

    let measure_numbers_setting = doc
        .layout
        .as_ref()
        .map(|l| l.measure_numbers.as_str())
        .unwrap_or("systemStart");

    for system in &page.systems {
        if system.measure_widths.is_empty() {
            continue;
        }

        let system_start_x = system.measure_x_positions[0];
        let total_system_w: f32 = system.measure_widths.iter().sum();
        let system_end_x = system_start_x + total_system_w;
        let staff_top_y = system.y_offset;
        let last_string_y = staff_top_y + staff_height;
        let staff_mid_y = staff_top_y + staff_height * 0.5;
        let string_spacing = if num_strings > 1 { line_spacing } else { 10.0 };
        let mid_y1 = staff_mid_y - string_spacing * 0.5;
        let mid_y2 = staff_mid_y + string_spacing * 0.5;

        // Draw staff string lines
        for s in 0..num_strings {
            let y = staff_top_y + (s as f32) * line_spacing;
            dl.push(DrawItem {
                id: None,
                kind: DrawKind::Line {
                    from: Pos2::new(system_start_x, y),
                    to: Pos2::new(system_end_x, y),
                    stroke: Stroke::new(0.8, staff_color),
                },
                z_index: 1,
            });
        }

        // Open string tuning labels at the first bar
        if system.measure_range.start == 0 {
            let edo = part.edo();
            for s in 1..=num_strings {
                let str_y = staff_top_y + ((s - 1) as f32) * line_spacing;
                let tuning_str = if let Some(pitch) = part.get_string_tuning(s as u8) {
                    crate::render::format_pitch(&pitch, edo)
                } else {
                    let default_name = match s {
                        1 => "E",
                        2 => "B",
                        3 => "G",
                        4 => "D",
                        5 => "A",
                        6 => "E",
                        7 => "B",
                        8 => "F#",
                        _ => "",
                    };
                    default_name.to_string()
                };

                if !tuning_str.is_empty() {
                    dl.push(DrawItem {
                        id: Some(format!("tuning_{}", s)),
                        kind: DrawKind::Text {
                            pos: Pos2::new(system_start_x - 6.0, str_y),
                            align: Align2::RIGHT_CENTER,
                            text: tuning_str,
                            font_id: FontId::proportional(10.5),
                            color: Color32::from_rgb(70, 70, 70),
                        },
                        z_index: 3,
                    });
                }
            }
        }

        // Draw opening barline
        let first_m_idx = system.measure_range.start;
        let sys_first_repeat_start = doc
            .global
            .measures
            .get(first_m_idx)
            .and_then(|g| g.repeat_start.as_ref())
            .is_some();

        if sys_first_repeat_start {
            let sys_bar_id = Some(format!("sys_barline_{}", first_m_idx));
            dl.push(DrawItem {
                id: sys_bar_id.clone(),
                kind: DrawKind::Line {
                    from: Pos2::new(system_start_x, staff_top_y),
                    to: Pos2::new(system_start_x, last_string_y),
                    stroke: Stroke::new(3.0, barline_color),
                },
                z_index: 2,
            });
            dl.push(DrawItem {
                id: sys_bar_id.clone(),
                kind: DrawKind::Line {
                    from: Pos2::new(system_start_x + 4.0, staff_top_y),
                    to: Pos2::new(system_start_x + 4.0, last_string_y),
                    stroke: Stroke::new(1.0, barline_color),
                },
                z_index: 2,
            });
            dl.push(DrawItem {
                id: sys_bar_id.clone(),
                kind: DrawKind::Circle {
                    center: Pos2::new(system_start_x + 8.0, mid_y1),
                    radius: 2.0,
                    fill: barline_color,
                },
                z_index: 2,
            });
            dl.push(DrawItem {
                id: sys_bar_id,
                kind: DrawKind::Circle {
                    center: Pos2::new(system_start_x + 8.0, mid_y2),
                    radius: 2.0,
                    fill: barline_color,
                },
                z_index: 2,
            });
        } else {
            dl.push(DrawItem {
                id: Some(format!("sys_barline_{}", first_m_idx)),
                kind: DrawKind::Line {
                    from: Pos2::new(system_start_x, staff_top_y),
                    to: Pos2::new(system_start_x, last_string_y),
                    stroke: Stroke::new(1.2, barline_color),
                },
                z_index: 2,
            });
        }

        // Measure numbers for system start
        let show_first_mn = match measure_numbers_setting {
            "all" => true,
            "systemStart" => first_m_idx > 0, // omit bar 1 at start of piece
            _ => false,
        };

        if show_first_mn {
            let mn_str = format!("{}", first_m_idx + 1);
            dl.push(DrawItem {
                id: Some(format!("mn_{}", first_m_idx)),
                kind: DrawKind::Text {
                    pos: Pos2::new(system_start_x, staff_top_y - 12.0),
                    align: Align2::LEFT_BOTTOM,
                    text: mn_str,
                    font_id: FontId::proportional(10.0),
                    color: Color32::from_rgb(90, 90, 90),
                },
                z_index: 5,
            });
        }

        // System-level collections for cross-event / cross-measure rendering
        let mut system_rhythm_notes: Vec<RhythmNote> = Vec::new();
        let mut grace_event_indices: HashSet<usize> = HashSet::new();
        let mut event_id_to_idx: HashMap<String, usize> = HashMap::new();
        let mut event_id_map: HashMap<String, (f32, bool)> = HashMap::new();
        let mut note_id_map: HashMap<String, (Pos2, u8, i32)> = HashMap::new();
        let mut event_notes_map: HashMap<String, Vec<(Option<String>, u8, i32, Pos2)>> = HashMap::new();
        let mut collected_slurs: Vec<(crate::mnx::Slur, Vec<(Option<String>, u8, i32, Pos2)>)> = Vec::new();
        let mut collected_ties: Vec<(Pos2, u8, i32, String)> = Vec::new();
        let mut collected_slides: Vec<(usize, u8, i32, Pos2)> = Vec::new();
        let mut string_timeline: HashMap<u8, Vec<(usize, i32, Pos2)>> = HashMap::new();
        let mut tuplet_brackets: Vec<(usize, usize, f32, f32, u32, f32)> = Vec::new();

        // Iterate measures in system
        for (rel_i, m_idx) in system.measure_range.clone().enumerate() {
            let m_x = system.measure_x_positions[rel_i];
            let m_w = system.measure_widths[rel_i];
            let barline_x = m_x + m_w;

            // Measure number if "all"
            if measure_numbers_setting == "all" && rel_i > 0 {
                let mn_str = format!("{}", m_idx + 1);
                dl.push(DrawItem {
                    id: Some(format!("mn_{}", m_idx)),
                    kind: DrawKind::Text {
                        pos: Pos2::new(m_x + 2.0, staff_top_y - 12.0),
                        align: Align2::LEFT_BOTTOM,
                        text: mn_str,
                        font_id: FontId::proportional(10.0),
                        color: Color32::from_rgb(90, 90, 90),
                    },
                    z_index: 5,
                });
            }

            let gm = doc.global.measures.get(m_idx);
            let m_ts = gm.and_then(|g| g.time);
            let shows_ts = if m_idx == 0 {
                m_ts.is_some()
            } else if let Some(ts) = m_ts {
                let prev_ts = crate::render::get_time_signature_for_measure(Some(&doc.global), m_idx.saturating_sub(1));
                prev_ts != ts
            } else {
                false
            };

            let m_has_repeat_start = if rel_i == 0 {
                sys_first_repeat_start
            } else {
                gm.and_then(|g| g.repeat_start.as_ref()).is_some()
            };

            let mut start_pad = 12.0_f32;
            if m_has_repeat_start {
                start_pad += 12.0;
            }

            // Draw Time Signature
            if shows_ts {
                if let Some(ts) = m_ts {
                    let ts_center_x = m_x + start_pad + 12.0;
                    let num_text = crate::render::smufl_time_sig_digits(ts.count);
                    let den_text = crate::render::smufl_time_sig_digits(ts.unit);
                    let center_pad = 38.0_f32 / 4.0 + 0.25;

                    dl.push(DrawItem {
                        id: Some(format!("time_sig_{}", m_idx)),
                        kind: DrawKind::Text {
                            pos: Pos2::new(ts_center_x, staff_mid_y - center_pad),
                            align: Align2::CENTER_CENTER,
                            text: num_text,
                            font_id: FontId::new(38.0, egui::FontFamily::Name("Bravura".into())),
                            color: text_color,
                        },
                        z_index: 5,
                    });

                    dl.push(DrawItem {
                        id: Some(format!("time_sig_{}", m_idx)),
                        kind: DrawKind::Text {
                            pos: Pos2::new(ts_center_x, staff_mid_y + center_pad),
                            align: Align2::CENTER_CENTER,
                            text: den_text,
                            font_id: FontId::new(38.0, egui::FontFamily::Name("Bravura".into())),
                            color: text_color,
                        },
                        z_index: 5,
                    });

                    start_pad += 34.0;
                }
            }

            // Draw measure closing barline
            let is_double = gm.and_then(|g| g.barline.as_ref()).map(|b| b.barline_type == "double").unwrap_or(false);
            let is_repeat_end = gm.and_then(|g| g.repeat_end.as_ref()).is_some();
            let next_m_idx = m_idx + 1;
            let next_is_in_system = rel_i + 1 < system.measure_widths.len();
            let next_has_repeat_start = next_is_in_system && doc.global.measures.get(next_m_idx).and_then(|g| g.repeat_start.as_ref()).is_some();

            let barline_id = Some(format!("barline_{}", m_idx));
            if is_repeat_end && next_has_repeat_start {
                // Combined repeat: :||:
                dl.push(DrawItem { id: barline_id.clone(), kind: DrawKind::Circle { center: Pos2::new(barline_x - 10.0, mid_y1), radius: 2.0, fill: barline_color }, z_index: 2 });
                dl.push(DrawItem { id: barline_id.clone(), kind: DrawKind::Circle { center: Pos2::new(barline_x - 10.0, mid_y2), radius: 2.0, fill: barline_color }, z_index: 2 });
                dl.push(DrawItem { id: barline_id.clone(), kind: DrawKind::Line { from: Pos2::new(barline_x - 6.0, staff_top_y), to: Pos2::new(barline_x - 6.0, last_string_y), stroke: Stroke::new(1.0, barline_color) }, z_index: 2 });
                dl.push(DrawItem { id: barline_id.clone(), kind: DrawKind::Line { from: Pos2::new(barline_x - 2.0, staff_top_y), to: Pos2::new(barline_x - 2.0, last_string_y), stroke: Stroke::new(3.0, barline_color) }, z_index: 2 });
                dl.push(DrawItem { id: barline_id.clone(), kind: DrawKind::Line { from: Pos2::new(barline_x + 2.0, staff_top_y), to: Pos2::new(barline_x + 2.0, last_string_y), stroke: Stroke::new(3.0, barline_color) }, z_index: 2 });
                dl.push(DrawItem { id: barline_id.clone(), kind: DrawKind::Line { from: Pos2::new(barline_x + 6.0, staff_top_y), to: Pos2::new(barline_x + 6.0, last_string_y), stroke: Stroke::new(1.0, barline_color) }, z_index: 2 });
                dl.push(DrawItem { id: barline_id.clone(), kind: DrawKind::Circle { center: Pos2::new(barline_x + 10.0, mid_y1), radius: 2.0, fill: barline_color }, z_index: 2 });
                dl.push(DrawItem { id: barline_id.clone(), kind: DrawKind::Circle { center: Pos2::new(barline_x + 10.0, mid_y2), radius: 2.0, fill: barline_color }, z_index: 2 });
            } else if is_repeat_end {
                // End repeat: :||
                dl.push(DrawItem { id: barline_id.clone(), kind: DrawKind::Circle { center: Pos2::new(barline_x - 10.0, mid_y1), radius: 2.0, fill: barline_color }, z_index: 2 });
                dl.push(DrawItem { id: barline_id.clone(), kind: DrawKind::Circle { center: Pos2::new(barline_x - 10.0, mid_y2), radius: 2.0, fill: barline_color }, z_index: 2 });
                dl.push(DrawItem { id: barline_id.clone(), kind: DrawKind::Line { from: Pos2::new(barline_x - 6.0, staff_top_y), to: Pos2::new(barline_x - 6.0, last_string_y), stroke: Stroke::new(1.0, barline_color) }, z_index: 2 });
                dl.push(DrawItem { id: barline_id.clone(), kind: DrawKind::Line { from: Pos2::new(barline_x - 2.0, staff_top_y), to: Pos2::new(barline_x - 2.0, last_string_y), stroke: Stroke::new(3.0, barline_color) }, z_index: 2 });
            } else if next_has_repeat_start {
                // Start repeat: ||:
                dl.push(DrawItem { id: barline_id.clone(), kind: DrawKind::Line { from: Pos2::new(barline_x, staff_top_y), to: Pos2::new(barline_x, last_string_y), stroke: Stroke::new(3.0, barline_color) }, z_index: 2 });
                dl.push(DrawItem { id: barline_id.clone(), kind: DrawKind::Line { from: Pos2::new(barline_x + 4.0, staff_top_y), to: Pos2::new(barline_x + 4.0, last_string_y), stroke: Stroke::new(1.0, barline_color) }, z_index: 2 });
                dl.push(DrawItem { id: barline_id.clone(), kind: DrawKind::Circle { center: Pos2::new(barline_x + 8.0, mid_y1), radius: 2.0, fill: barline_color }, z_index: 2 });
                dl.push(DrawItem { id: barline_id.clone(), kind: DrawKind::Circle { center: Pos2::new(barline_x + 8.0, mid_y2), radius: 2.0, fill: barline_color }, z_index: 2 });
            } else if is_double {
                // Double barline: ||
                dl.push(DrawItem { id: barline_id.clone(), kind: DrawKind::Line { from: Pos2::new(barline_x - 4.0, staff_top_y), to: Pos2::new(barline_x - 4.0, last_string_y), stroke: Stroke::new(1.0, barline_color) }, z_index: 2 });
                dl.push(DrawItem { id: barline_id.clone(), kind: DrawKind::Line { from: Pos2::new(barline_x, staff_top_y), to: Pos2::new(barline_x, last_string_y), stroke: Stroke::new(1.0, barline_color) }, z_index: 2 });
            } else {
                // Single normal barline
                dl.push(DrawItem { id: barline_id, kind: DrawKind::Line { from: Pos2::new(barline_x, staff_top_y), to: Pos2::new(barline_x, last_string_y), stroke: Stroke::new(1.0, barline_color) }, z_index: 2 });
            }

            // Draw voltas / ending if present
            if let Some(gm) = gm {
                if let Some(ending) = &gm.ending {
                    let start_m = m_idx;
                    let duration = ending.duration.max(1) as usize;
                    let end_m = (start_m + duration - 1).min(system.measure_range.end.saturating_sub(1));
                    let end_rel_i = end_m - system.measure_range.start;
                    let end_barline_x = system.measure_x_positions[end_rel_i] + system.measure_widths[end_rel_i];

                    let has_tempos_in_sys = system.measure_range.clone().any(|mi| {
                        doc.global.measures.get(mi).map(|g| !g.tempos.is_empty()).unwrap_or(false)
                    });
                    let bracket_y = if has_tempos_in_sys { staff_top_y - 36.0 } else { staff_top_y - 20.0 };
                    let hook_len = 8.0_f32;
                    let is_open = ending.open.unwrap_or(false);

                    let ending_id = Some(format!("ending_{}", m_idx));

                    // Top horizontal line
                    dl.push(DrawItem {
                        id: ending_id.clone(),
                        kind: DrawKind::Line {
                            from: Pos2::new(m_x, bracket_y),
                            to: Pos2::new(end_barline_x, bracket_y),
                            stroke: Stroke::new(1.2, barline_color),
                        },
                        z_index: 4,
                    });

                    // Left vertical hook
                    dl.push(DrawItem {
                        id: ending_id.clone(),
                        kind: DrawKind::Line {
                            from: Pos2::new(m_x, bracket_y),
                            to: Pos2::new(m_x, bracket_y + hook_len),
                            stroke: Stroke::new(1.2, barline_color),
                        },
                        z_index: 4,
                    });

                    // Right vertical hook if not open
                    if !is_open {
                        dl.push(DrawItem {
                            id: ending_id.clone(),
                            kind: DrawKind::Line {
                                from: Pos2::new(end_barline_x, bracket_y),
                                to: Pos2::new(end_barline_x, bracket_y + hook_len),
                                stroke: Stroke::new(1.2, barline_color),
                            },
                            z_index: 4,
                        });
                    }

                    // Ending numbers text
                    let nums_str = ending
                        .numbers
                        .iter()
                        .map(|n| n.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    dl.push(DrawItem {
                        id: ending_id,
                        kind: DrawKind::Text {
                            pos: Pos2::new(m_x + 4.0, bracket_y + 2.0),
                            align: Align2::LEFT_TOP,
                            text: format!("{}.", nums_str),
                            font_id: FontId::proportional(12.0),
                            color: text_color,
                        },
                        z_index: 5,
                    });
                }
            }

            // Draw tempo marks
            if let Some(gm) = gm {
                let has_endings_in_sys = system.measure_range.clone().any(|mi| {
                    doc.global.measures.get(mi).and_then(|g| g.ending.as_ref()).is_some()
                });
                let tempo_y = if has_endings_in_sys { staff_top_y - 34.0 } else { staff_top_y - 20.0 };

                for tempo in &gm.tempos {
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
                    let tempo_x = m_x + start_pad;
                    let tid = tempo.id.as_deref().unwrap_or("tempo");

                    let tempo_mark_id = Some(format!("tempo_{}_{}", m_idx, tid));

                    dl.push(DrawItem {
                        id: tempo_mark_id.clone(),
                        kind: DrawKind::Text {
                            pos: Pos2::new(tempo_x, tempo_y),
                            align: Align2::LEFT_BOTTOM,
                            text: glyph.to_string(),
                            font_id: FontId::new(17.0, egui::FontFamily::Name("BravuraText".into())),
                            color: text_color,
                        },
                        z_index: 6,
                    });

                    dl.push(DrawItem {
                        id: tempo_mark_id,
                        kind: DrawKind::Text {
                            pos: Pos2::new(tempo_x + 12.0, tempo_y),
                            align: Align2::LEFT_BOTTOM,
                            text,
                            font_id: FontId::proportional(13.0),
                            color: text_color,
                        },
                        z_index: 6,
                    });
                }

                for (tc_i, _tc) in gm.tempo_changes.iter().enumerate() {
                    let label = "rit.";
                    let tc_x = m_x + start_pad;
                    dl.push(DrawItem {
                        id: Some(format!("tc_{}_{}", m_idx, tc_i)),
                        kind: DrawKind::Text {
                            pos: Pos2::new(tc_x, tempo_y),
                            align: Align2::LEFT_BOTTOM,
                            text: label.to_string(),
                            font_id: FontId::proportional(13.0),
                            color: text_color,
                        },
                        z_index: 6,
                    });
                }
            }

            // Draw measure notes, rests, and lyrics
            if let Some(measure) = part.measures.get(m_idx) {
                if let Some(seq) = measure.sequences.first() {
                    let mut layout_items = Vec::new();
                    for item in &seq.content {
                        item.to_layout_items(&mut layout_items);
                    }

                    let end_pad = if is_repeat_end { 16.0_f32 } else { 12.0_f32 };
                    let avail_w = (m_w - start_pad - end_pad).max(10.0);

                    // Precompute event spacings using calculate_event_spacing
                    let mut event_spacings = Vec::new();
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
                                let sp = crate::render::calculate_event_spacing(
                                    ev,
                                    in_grace,
                                    crate::layout::DEFAULT_LAYOUT_BASE_SPACING,
                                    next_ev_info,
                                    None,
                                    None,
                                );
                                event_spacings.push(sp);
                            }
                            _ => {}
                        }
                    }

                    let total_natural: f32 = event_spacings.iter().sum();
                    let ev_scale = if total_natural > 0.0 { avail_w / total_natural } else { 1.0 };

                    let mut ev_idx_in_m = 0;
                    let mut accum_x = 0.0_f32;
                    let mut active_tuplets: Vec<(usize, f32, u32)> = Vec::new();
                    let mut tuplet_counter = 0;
                    let mut tuplet_stack: Vec<(usize, f64)> = Vec::new(); // (tuplet_id, scale)
                    let mut current_measure_beat: f64 = 0.0;
                    in_grace = false;

                    for l_item in &layout_items {
                        match l_item {
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
                                let current_x = m_x + start_pad + accum_x * ev_scale;
                                active_tuplets.push((tuplet_counter, current_x, t.inner.multiple));
                            }
                            LayoutItem::TupletEnd => {
                                tuplet_stack.pop();
                                if let Some((t_cnt, start_x, mult)) = active_tuplets.pop() {
                                    let end_x = m_x + start_pad + accum_x * ev_scale;
                                    let y = staff_top_y - 12.0;
                                    tuplet_brackets.push((m_idx, t_cnt, start_x, end_x, mult, y));
                                }
                            }
                            LayoutItem::GraceStart => {
                                in_grace = true;
                            }
                            LayoutItem::GraceEnd => {
                                in_grace = false;
                            }
                            LayoutItem::Event(ev) => {
                                let current_tuplet_id = tuplet_stack.last().map(|&(id, _)| id);
                                let current_scale = tuplet_stack.last().map(|&(_, s)| s).unwrap_or(1.0);
                                let event_beats = if in_grace {
                                    0.0
                                } else {
                                    crate::validate::duration_to_quarter_beats(&ev.duration) * current_scale
                                };
                                let start_beat = current_measure_beat;
                                let end_beat = current_measure_beat + event_beats;
                                current_measure_beat = end_beat;
                                let ev_w = event_spacings.get(ev_idx_in_m).copied().unwrap_or(crate::layout::DEFAULT_LAYOUT_BASE_SPACING);
                                let ev_x = m_x + start_pad + (accum_x + ev_w * 0.5) * ev_scale;
                                accum_x += ev_w;
                                ev_idx_in_m += 1;

                                let is_rest = ev.rest.is_some() || ev.notes.is_empty();

                                if let Some(ev_id) = &ev.id {
                                    event_id_map.insert(ev_id.clone(), (ev_x, is_rest));
                                    event_id_to_idx.insert(ev_id.clone(), system_rhythm_notes.len());
                                }

                                if is_rest {
                                    // Draw rest glyph
                                    let rest_char = match ev.duration.base.as_str() {
                                        "whole" => '\u{E4E3}',
                                        "half" => '\u{E4E4}',
                                        "quarter" => '\u{E4E5}',
                                        "eighth" => '\u{E4E6}',
                                        "16th" => '\u{E4E7}',
                                        "32nd" => '\u{E4E8}',
                                        "64th" => '\u{E4E9}',
                                        _ => '\u{E4E5}',
                                    };
                                    let rest_id = Some(ev.id.clone().unwrap_or_else(|| format!("rest_{}_{}", m_idx, ev_idx_in_m)));
                                    dl.push(DrawItem {
                                        id: rest_id.clone(),
                                        kind: DrawKind::Text {
                                            pos: Pos2::new(ev_x, staff_mid_y),
                                            align: Align2::CENTER_CENTER,
                                            text: rest_char.to_string(),
                                            font_id: FontId::new(26.0, egui::FontFamily::Name("Bravura".into())),
                                            color: text_color,
                                        },
                                        z_index: 4,
                                    });

                                    if ev.duration.dots.unwrap_or(0) > 0 {
                                        dl.push(DrawItem {
                                            id: rest_id,
                                            kind: DrawKind::Text {
                                                pos: Pos2::new(ev_x + 6.0, staff_mid_y),
                                                align: Align2::CENTER_CENTER,
                                                text: "\u{E1E7}".to_string(),
                                                font_id: FontId::new(26.0, egui::FontFamily::Name("Bravura".into())),
                                                color: text_color,
                                            },
                                            z_index: 4,
                                        });
                                    }
                                } else {
                                    // Draw fret digits
                                    let mut this_event_notes = Vec::new();
                                    for note in &ev.notes {
                                        if let Some(s) = note.string {
                                            if s >= 1 && (s as usize) <= num_strings {
                                                let str_y = staff_top_y + ((s - 1) as f32) * line_spacing;
                                                let fret_text = if let Some(h) = &note.harmonic {
                                                    crate::render::format_harmonic_touch(h.touch)
                                                } else if note.muted.unwrap_or(false) {
                                                    "X".to_string()
                                                } else {
                                                    note.fret.map(|f| f.to_string()).unwrap_or_else(|| "-".to_string())
                                                };

                                                let note_pos = Pos2::new(ev_x, str_y);
                                                let fret_val = note.fret.unwrap_or(0);
                                                this_event_notes.push((note.id.clone(), s, fret_val, note_pos));

                                                if let Some(nid) = &note.id {
                                                    note_id_map.insert(nid.clone(), (note_pos, s, fret_val));
                                                }

                                                if let Some(ties) = &note.ties {
                                                    for tie in ties {
                                                        collected_ties.push((note_pos, s, fret_val, tie.target.clone()));
                                                    }
                                                }

                                                if note.slide.is_some() {
                                                    collected_slides.push((system_rhythm_notes.len(), s, fret_val, note_pos));
                                                }

                                                string_timeline.entry(s).or_default().push((system_rhythm_notes.len(), fret_val, note_pos));

                                                let font_sz = if in_grace { 9.5 } else { 11.0 };
                                                let text_w = (fret_text.chars().count() as f32) * (if in_grace { 5.5 } else { 7.5 });
                                                let note_item_id = Some(note.id.clone().unwrap_or_else(|| format!("note_{}_{}_{}", m_idx, ev_idx_in_m, s)));
                                                dl.push(DrawItem {
                                                    id: note_item_id.clone(),
                                                    kind: DrawKind::Rect {
                                                        rect: Rect::from_center_size(
                                                            note_pos,
                                                            eframe::egui::Vec2::new(text_w + 3.0, 11.0),
                                                        ),
                                                        fill: Color32::WHITE,
                                                        stroke: Stroke::NONE,
                                                        rounding: 0.0,
                                                    },
                                                    z_index: 3,
                                                });

                                                dl.push(DrawItem {
                                                    id: note_item_id,
                                                    kind: DrawKind::Text {
                                                        pos: note_pos,
                                                        align: Align2::CENTER_CENTER,
                                                        text: fret_text,
                                                        font_id: FontId::proportional(font_sz),
                                                        color: text_color,
                                                    },
                                                    z_index: 4,
                                                });
                                            }
                                        }
                                    }

                                    if let Some(ev_id) = &ev.id {
                                        event_notes_map.insert(ev_id.clone(), this_event_notes.clone());
                                    }

                                    if let Some(slurs) = &ev.slurs {
                                        for slur in slurs {
                                            collected_slurs.push((slur.clone(), this_event_notes.clone()));
                                        }
                                    }
                                }

                                // Collect rhythm note for beaming
                                let (rhythm, mut is_dotted) = crate::render::parse_rhythm(&ev.duration.base);
                                if ev.duration.dots.unwrap_or(0) > 0 {
                                    is_dotted = true;
                                }
                                if in_grace {
                                    grace_event_indices.insert(system_rhythm_notes.len());
                                }
                                system_rhythm_notes.push(RhythmNote {
                                    x: ev_x,
                                    rhythm,
                                    is_dotted,
                                    measure_idx: m_idx,
                                    is_rest,
                                    tuplet_id: current_tuplet_id,
                                    start_beat,
                                    end_beat,
                                });

                                // Draw lyrics if present
                                if let Some(lyrics) = &ev.lyrics {
                                    let stem_bot = last_string_y + 26.0;
                                    for (l_i, (_line_key, lyr)) in lyrics.lines.iter().enumerate() {
                                        let lyr_y = stem_bot + 10.0 + (l_i as f32) * 14.0;
                                        let lyric_id = Some(format!("lyric_{}_{}", m_idx, ev_idx_in_m));
                                        let display_text = if matches!(lyr.line_type.as_deref(), Some("start") | Some("middle")) && !lyr.text.ends_with('-') {
                                            format!("{}-", lyr.text)
                                        } else {
                                            lyr.text.clone()
                                        };
                                        dl.push(DrawItem {
                                            id: lyric_id.clone(),
                                            kind: DrawKind::Text {
                                                pos: Pos2::new(ev_x, lyr_y),
                                                align: Align2::CENTER_CENTER,
                                                text: display_text,
                                                font_id: FontId::proportional(11.0),
                                                color: text_color,
                                            },
                                            z_index: 4,
                                        });

                                        // Melisma extender line
                                        if lyr.ext.extender == Some(true) || lyr.text.ends_with('_') {
                                            let text_w = (lyr.text.chars().count() as f32) * 6.5;
                                            let start_x = ev_x + text_w * 0.5 + 2.0;
                                            let end_x = ev_x + ev_w * ev_scale * 0.9;
                                            if end_x > start_x + 4.0 {
                                                dl.push(DrawItem {
                                                    id: lyric_id,
                                                    kind: DrawKind::Line {
                                                        from: Pos2::new(start_x, lyr_y + 5.0),
                                                        to: Pos2::new(end_x, lyr_y + 5.0),
                                                        stroke: Stroke::new(1.0, text_color),
                                                    },
                                                    z_index: 4,
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Draw rhythm stems and beams
        let n_rhythm = system_rhythm_notes.len();
        let mut beam_to: Vec<Option<usize>> = vec![None; n_rhythm];
        let mut beam_from: Vec<Option<usize>> = vec![None; n_rhythm];

        for m_idx in system.measure_range.clone() {
            if let Some(measure) = part.measures.get(m_idx) {
                let all_explicit_beams: Vec<&crate::mnx::BeamGroup> = measure
                    .beams
                    .as_ref()
                    .map(|b| b.iter().collect())
                    .unwrap_or_default();

                if !all_explicit_beams.is_empty() {
                    for bg in all_explicit_beams {
                        crate::render::apply_beam_group_indices(
                            bg,
                            &event_id_to_idx,
                            &system_rhythm_notes,
                            &mut beam_to,
                            &mut beam_from,
                        );
                    }
                    // Grace notes in this measure: beam adjacent grace notes
                    let n = system_rhythm_notes.len();
                    let grace_indices: Vec<usize> = (0..n)
                        .filter(|&i| system_rhythm_notes[i].measure_idx == m_idx && grace_event_indices.contains(&i))
                        .collect();
                    for &ia in &grace_indices {
                        let ib = ia + 1;
                        if ib < n && grace_event_indices.contains(&ib) && system_rhythm_notes[ib].measure_idx == m_idx {
                            let note_a = &system_rhythm_notes[ia];
                            let note_b = &system_rhythm_notes[ib];
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
                    let time_sig = crate::render::get_time_signature_for_measure(Some(&doc.global), m_idx);
                    crate::render::compute_default_beaming_indices(
                        m_idx,
                        &time_sig,
                        &system_rhythm_notes,
                        &grace_event_indices,
                        &mut beam_to,
                        &mut beam_from,
                    );
                }
            }
        }

        let stem_top_y = last_string_y + 6.0;
        let stem_bot_y = stem_top_y + 20.0;
        let flag_spacing = 4.5_f32;

        for (i, note) in system_rhythm_notes.iter().enumerate() {
            if note.is_rest {
                continue;
            }
            let cx = note.x;
            let is_grace = grace_event_indices.contains(&i);
            let mut local_stem_top_y = stem_top_y;
            let mut local_stem_bot_y = stem_bot_y;
            if is_grace {
                local_stem_top_y -= 6.0;
                local_stem_bot_y -= 10.0;
            }

            let flags = note.rhythm.flag_count();
            let stem_thickness = if is_grace { 1.0 } else { 1.2 };
            let stem_id = Some(format!("stem_{}_{}", note.measure_idx, i));

            // Stem
            if note.rhythm != Rhythm::Whole {
                dl.push(DrawItem {
                    id: stem_id.clone(),
                    kind: DrawKind::Line {
                        from: Pos2::new(cx, local_stem_top_y),
                        to: Pos2::new(cx, local_stem_bot_y),
                        stroke: Stroke::new(stem_thickness, barline_color),
                    },
                    z_index: 2,
                });
            }

            // Augmentation dot
            if note.is_dotted {
                dl.push(DrawItem {
                    id: stem_id,
                    kind: DrawKind::Circle {
                        center: Pos2::new(cx + 5.0, local_stem_bot_y - 2.0),
                        radius: 1.5,
                        fill: barline_color,
                    },
                    z_index: 3,
                });
            }

            // Flags / Beams
            if flags > 0 {
                if beam_to[i].is_some() || beam_from[i].is_some() {
                    if let Some(right_idx) = beam_to[i] {
                        let nx = system_rhythm_notes[right_idx].x;
                        let sx = cx;
                        let right_flags = system_rhythm_notes[right_idx].rhythm.flag_count();
                        let shared_flags = flags.min(right_flags);
                        let beam_thickness = if is_grace { 2.0 } else { 3.0 };

                        for f in 0..shared_flags {
                            let by = local_stem_bot_y - (f as f32) * flag_spacing;
                            dl.push(DrawItem {
                                id: None,
                                kind: DrawKind::Line {
                                    from: Pos2::new(sx, by),
                                    to: Pos2::new(nx, by),
                                    stroke: Stroke::new(beam_thickness, barline_color),
                                },
                                z_index: 3,
                            });
                        }

                        let extra = flags.saturating_sub(right_flags);
                        let stub_w = 8.0_f32;
                        for f in shared_flags..shared_flags + extra {
                            let by = local_stem_bot_y - (f as f32) * flag_spacing;
                            dl.push(DrawItem {
                                id: None,
                                kind: DrawKind::Line {
                                    from: Pos2::new(sx, by),
                                    to: Pos2::new(sx + stub_w, by),
                                    stroke: Stroke::new(beam_thickness, barline_color),
                                },
                                z_index: 3,
                            });
                        }
                    }

                    if beam_to[i].is_none() {
                        if let Some(left_idx) = beam_from[i] {
                            let prev_flags = system_rhythm_notes[left_idx].rhythm.flag_count();
                            if flags > prev_flags {
                                let extra = flags - prev_flags;
                                let sx = cx;
                                let beam_thickness = if is_grace { 2.0 } else { 3.0 };
                                let stub_w = if is_grace { 4.5_f32 } else { 6.0_f32 };
                                for f in prev_flags..prev_flags + extra {
                                    let by = local_stem_bot_y - (f as f32) * flag_spacing;
                                    dl.push(DrawItem {
                                        id: None,
                                        kind: DrawKind::Line {
                                            from: Pos2::new(sx - stub_w, by),
                                            to: Pos2::new(sx, by),
                                            stroke: Stroke::new(beam_thickness, barline_color),
                                        },
                                        z_index: 3,
                                    });
                                }
                            }
                        }
                    }
                } else {
                    // Isolated flags
                    let sx = cx;
                    let flag_w = if is_grace { 6.0_f32 } else { 9.0_f32 };
                    for f in 0..flags {
                        let flag_y_bot = local_stem_bot_y - (f as f32) * flag_spacing;
                        let flag_y_top = flag_y_bot - flag_spacing - 2.0;
                        dl.push(DrawItem {
                            id: None,
                            kind: DrawKind::Line {
                                from: Pos2::new(sx, flag_y_bot),
                                to: Pos2::new(sx + flag_w, flag_y_bot - flag_spacing * 0.5),
                                stroke: Stroke::new(1.2, barline_color),
                            },
                            z_index: 3,
                        });
                        dl.push(DrawItem {
                            id: None,
                            kind: DrawKind::Line {
                                from: Pos2::new(sx + flag_w, flag_y_bot - flag_spacing * 0.5),
                                to: Pos2::new(sx + flag_w * 0.4, flag_y_top),
                                stroke: Stroke::new(1.2, barline_color),
                            },
                            z_index: 3,
                        });
                    }
                }
            }
        }

        // Draw slurs (Hammer-ons / Pull-offs)
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
                if let (Some((_, s_str, s_fret, s_pos)), Some((_, e_str, e_fret, e_pos))) = (s_opt, e_opt) {
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
                // Enforce guitar constraint: single string only!
                if s_str != e_str || e_pos.x <= s_pos.x {
                    continue;
                }

                let slur_id = Some(format!("slur_{}_{}", s_str, slur.target));
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

                dl.push(DrawItem {
                    id: slur_id.clone(),
                    kind: DrawKind::Curve {
                        points: [p0, p1, p2, p3],
                        stroke: Stroke::new(1.5, text_color),
                    },
                    z_index: 5,
                });

                let mid_x = (p0.x + p3.x) * 0.5;
                let label = if e_fret > s_fret {
                    "h"
                } else if e_fret < s_fret {
                    "p"
                } else {
                    ""
                };

                if !label.is_empty() {
                    dl.push(DrawItem {
                        id: slur_id,
                        kind: DrawKind::Text {
                            pos: Pos2::new(mid_x, peak_y - 2.0),
                            align: Align2::CENTER_BOTTOM,
                            text: label.to_string(),
                            font_id: FontId::proportional(11.0),
                            color: text_color,
                        },
                        z_index: 5,
                    });
                }
            }
        }

        // Draw ties & let ring
        for (start_pos, start_string, _start_fret, target_id) in &collected_ties {
            let tie_id = Some(format!("tie_{}_{}", start_string, target_id));
            let string_y = start_pos.y;
            if let Some(&(target_pos, target_string, _target_fret)) = note_id_map.get(target_id) {
                if *start_string == target_string && target_pos.x > start_pos.x {
                    let total_dx = target_pos.x - start_pos.x;
                    let pad = (total_dx * 0.2).clamp(4.0, 8.0);
                    let x1 = start_pos.x + pad;
                    let x2 = target_pos.x - pad;
                    if x2 > x1 {
                        let p0 = Pos2::new(x1, string_y + 3.0);
                        let p3 = Pos2::new(x2, string_y + 3.0);
                        let dx = p3.x - p0.x;
                        let dip = (dx * 0.12).clamp(4.0, 10.0);
                        let p1 = Pos2::new(p0.x + dx * 0.25, string_y + 3.0 + dip);
                        let p2 = Pos2::new(p3.x - dx * 0.25, string_y + 3.0 + dip);

                        dl.push(DrawItem {
                            id: tie_id,
                            kind: DrawKind::Curve {
                                points: [p0, p1, p2, p3],
                                stroke: Stroke::new(1.5, text_color),
                            },
                            z_index: 5,
                        });
                    }
                }
            } else if let Some(&(rest_x, _is_rest)) = event_id_map.get(target_id) {
                if rest_x > start_pos.x {
                    let total_dx = rest_x - start_pos.x;
                    let pad = (total_dx * 0.2).clamp(4.0, 8.0);
                    let x1 = start_pos.x + pad;
                    let x2 = rest_x - 4.0;
                    if x2 > x1 {
                        let p0 = Pos2::new(x1, string_y + 3.0);
                        let p3 = Pos2::new(x2, string_y + 3.0);
                        let dx = p3.x - p0.x;
                        let dip = (dx * 0.12).clamp(4.0, 10.0);
                        let p1 = Pos2::new(p0.x + dx * 0.25, string_y + 3.0 + dip);
                        let p2 = Pos2::new(p3.x - dx * 0.25, string_y + 3.0 + dip);

                        dl.push(DrawItem {
                            id: tie_id.clone(),
                            kind: DrawKind::Curve {
                                points: [p0, p1, p2, p3],
                                stroke: Stroke::new(1.5, text_color),
                            },
                            z_index: 5,
                        });

                        dl.push(DrawItem {
                            id: tie_id,
                            kind: DrawKind::Text {
                                pos: Pos2::new((x1 + x2) * 0.5, string_y + 4.0 + dip),
                                align: Align2::CENTER_TOP,
                                text: "l.r.".to_string(),
                                font_id: FontId::proportional(10.0),
                                color: Color32::from_gray(140),
                            },
                            z_index: 5,
                        });
                    }
                }
            }
        }

        // Draw slides
        for (start_idx, string_num, start_fret, start_pos) in &collected_slides {
            if let Some(timeline) = string_timeline.get(string_num) {
                if let Some(pos_in_tl) = timeline.iter().position(|(idx, _, _)| *idx == *start_idx) {
                    if pos_in_tl + 1 < timeline.len() {
                        let (_, next_fret, next_pos) = timeline[pos_in_tl + 1];
                        let up = next_fret > *start_fret;
                        let start_x = start_pos.x + 5.5;
                        let mut end_x = start_x + 10.0;
                        let start_y = start_pos.y;
                        let end_y = if up { start_pos.y - 6.0 } else { start_pos.y + 6.0 };
                        end_x = end_x.min(next_pos.x - 4.0).max(start_x + 2.0);

                        let slide_id = Some(format!("slide_{}_{}", string_num, start_idx));
                        dl.push(DrawItem {
                            id: slide_id,
                            kind: DrawKind::Line {
                                from: Pos2::new(start_x, start_y),
                                to: Pos2::new(end_x, end_y),
                                stroke: Stroke::new(2.0, text_color),
                            },
                            z_index: 5,
                        });
                    }
                }
            }
        }

        // Draw tuplets
        for (m_idx, t_cnt, start_x, end_x, mult, y) in &tuplet_brackets {
            let tuplet_id = Some(format!("tuplet_{}_{}", m_idx, t_cnt));
            dl.push(DrawItem {
                id: tuplet_id.clone(),
                kind: DrawKind::Line {
                    from: Pos2::new(*start_x, *y + 6.0),
                    to: Pos2::new(*start_x, *y),
                    stroke: Stroke::new(1.0, Color32::from_gray(140)),
                },
                z_index: 5,
            });
            dl.push(DrawItem {
                id: tuplet_id.clone(),
                kind: DrawKind::Line {
                    from: Pos2::new(*start_x, *y),
                    to: Pos2::new(*end_x, *y),
                    stroke: Stroke::new(1.0, Color32::from_gray(140)),
                },
                z_index: 5,
            });
            dl.push(DrawItem {
                id: tuplet_id.clone(),
                kind: DrawKind::Line {
                    from: Pos2::new(*end_x, *y),
                    to: Pos2::new(*end_x, *y + 6.0),
                    stroke: Stroke::new(1.0, Color32::from_gray(140)),
                },
                z_index: 5,
            });
            dl.push(DrawItem {
                id: tuplet_id,
                kind: DrawKind::Text {
                    pos: Pos2::new((*start_x + *end_x) * 0.5, *y - 7.0),
                    align: Align2::CENTER_CENTER,
                    text: mult.to_string(),
                    font_id: FontId::monospace(11.0),
                    color: Color32::from_gray(100),
                },
                z_index: 5,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pdf_export_generation() {
        let yaml = r#"---
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
    - {}
parts:
  - name: "Classical Guitar"
    _x:
      microFretDivisions: 12
    measures:
      - sequences:
          - content:
              - duration: { base: quarter }
                notes:
                  - pitch: { step: E, octave: 4 }
                    _x:
                      string: 1
                      fret: 0
                lyrics:
                  lines:
                    "1":
                      text: "Sing"
              - duration: { base: quarter }
                notes:
                  - pitch: { step: B, octave: 3 }
                    _x:
                      string: 2
                      fret: 0
                lyrics:
                  lines:
                    "1":
                      text: "loud_"
                      _x:
                        extender: true
      - sequences:
          - content:
              - duration: { base: half }
                notes:
                  - pitch: { step: G, octave: 3 }
                    _x:
                      string: 3
                      fret: 0
"#;
        let doc: MnxDocument = serde_saphyr::from_str(yaml).unwrap();
        let pages = crate::layout::compute_score_layout(&doc, 0.0);
        assert!(!pages.is_empty());

        let pdf_bytes = generate_score_pdf_bytes(&doc, &pages).expect("PDF export should succeed");
        assert!(!pdf_bytes.is_empty());
        assert!(pdf_bytes.starts_with(b"%PDF-1."));
    }

    #[test]
    fn test_pdf_export_multipage_with_breaks_and_offsets() {
        let yaml = r#"---
mnx:
  version: 1
_x:
  layout:
    pageSize: letter
    measureNumbers: all
    offsets:
      ending_0: { dx: 10.0, dy: -5.0 }
      lyric_0_0: { dx: 2.0, dy: 4.0 }
global:
  measures:
    - time: { count: 4, unit: 4 }
      ending: { duration: 1, numbers: [1], open: false }
    - _x:
        pageBreak: true
      ending: { duration: 1, numbers: [2], open: true }
    - {}
parts:
  - name: "Acoustic Guitar"
    _x:
      microFretDivisions: 12
    measures:
      - sequences:
          - content:
              - duration: { base: half }
                notes:
                  - pitch: { step: E, octave: 4 }
                    _x:
                      string: 1
                      fret: 0
                lyrics:
                  lines:
                    "1":
                      text: "Ooh_"
                      _x:
                        extender: true
              - duration: { base: half }
                notes:
                  - pitch: { step: B, octave: 3 }
                    _x:
                      string: 2
                      fret: 0
      - sequences:
          - content:
              - duration: { base: whole }
                notes:
                  - pitch: { step: G, octave: 3 }
                    _x:
                      string: 3
                      fret: 0
      - sequences:
          - content:
              - duration: { base: whole }
                notes:
                  - pitch: { step: D, octave: 3 }
                    _x:
                      string: 4
                      fret: 0
"#;
        let doc: MnxDocument = serde_saphyr::from_str(yaml).unwrap();
        let pages = crate::layout::compute_score_layout(&doc, 0.0);
        assert_eq!(pages.len(), 2, "Page break on measure 2 should produce 2 pages");

        let pdf_bytes = generate_score_pdf_bytes(&doc, &pages).expect("Multi-page PDF export should succeed");
        assert!(!pdf_bytes.is_empty());
        assert!(pdf_bytes.starts_with(b"%PDF-1."));
    }

    #[test]
    fn test_layout_display_list_full_notation_elements() {
        let yaml = r#"---
mnx:
  version: 1
global:
  measures:
    - time: { count: 3, unit: 4 }
      repeatStart: {}
      tempos:
        - bpm: 120
          value: { base: quarter }
    - time: { count: 4, unit: 4 }
      repeatEnd: { times: 2 }
parts:
  - name: "Electric Guitar"
    _x:
      microFretDivisions: 12
    measures:
      - sequences:
          - content:
              - duration: { base: eighth }
                notes:
                  - id: "n1"
                    pitch: { step: E, octave: 4 }
                    ties:
                      - target: "n2"
                    _x:
                      string: 1
                      fret: 0
                slurs:
                  - target: "ev2"
              - id: "ev2"
                duration: { base: eighth, dots: 1 }
                notes:
                  - id: "n2"
                    pitch: { step: F, alter: 1, octave: 4 }
                    _x:
                      string: 1
                      fret: 2
              - duration: { base: quarter }
                rest: {}
      - sequences:
          - content:
              - type: tuplet
                inner: { duration: { base: eighth }, multiple: 3 }
                outer: { duration: { base: quarter }, multiple: 1 }
                content:
                  - duration: { base: eighth }
                    notes:
                      - pitch: { step: G, octave: 3 }
                        _x:
                          string: 3
                          fret: 0
                  - duration: { base: eighth }
                    notes:
                      - pitch: { step: A, octave: 3 }
                        _x:
                          string: 3
                          fret: 2
                  - duration: { base: eighth }
                    notes:
                      - pitch: { step: B, octave: 3 }
                        _x:
                          string: 3
                          fret: 4
              - duration: { base: half }
                notes:
                  - pitch: { step: C, octave: 4 }
                    _x:
                      string: 2
                      fret: 1
"#;
        let doc: MnxDocument = serde_saphyr::from_str(yaml).unwrap();
        let pages = crate::layout::compute_score_layout(&doc, 0.0);
        assert!(!pages.is_empty());

        let mut dl = DisplayList::new();
        populate_page_display_list(&doc, &pages[0], &mut dl);

        // Verify Header elements
        let has_title = dl.items.iter().any(|item| item.id.as_deref() == Some("header_title"));
        assert!(has_title, "Header title should be present on page 1");
        let has_composer = dl.items.iter().any(|item| item.id.as_deref() == Some("header_composer"));
        assert!(has_composer, "Header composer should be present on page 1");

        // Verify String Tunings on first bar
        let has_tuning_1 = dl.items.iter().any(|item| item.id.as_deref() == Some("tuning_1"));
        let has_tuning_6 = dl.items.iter().any(|item| item.id.as_deref() == Some("tuning_6"));
        assert!(has_tuning_1, "Tuning label for string 1 should be present at bar 1");
        assert!(has_tuning_6, "Tuning label for string 6 should be present at bar 1");

        // Verify Time Signatures
        let has_ts0 = dl.items.iter().any(|item| item.id.as_deref() == Some("time_sig_0"));
        let has_ts1 = dl.items.iter().any(|item| item.id.as_deref() == Some("time_sig_1"));
        assert!(has_ts0, "Measure 0 time signature should be present");
        assert!(has_ts1, "Measure 1 meter change time signature should be present");

        // Verify Repeat Barlines (repeat start and repeat end circles)
        let circles_count = dl.items.iter().filter(|item| matches!(&item.kind, DrawKind::Circle { .. })).count();
        assert!(circles_count >= 4, "Should have repeat dots for both start and end repeats, got {}", circles_count);
        let has_barline = dl.items.iter().any(|item| item.id.as_deref() == Some("barline_1"));
        assert!(has_barline, "Measure 1 repeat closing barline should have barline_1 ID");

        // Verify Rests (Bravura quarter rest glyph \u{E4E5})
        let has_rest = dl.items.iter().any(|item| match &item.kind {
            DrawKind::Text { text, .. } => text.contains('\u{E4E5}'),
            _ => false,
        });
        assert!(has_rest, "Quarter rest SMuFL glyph should be present");
        let has_rest_id = dl.items.iter().any(|item| item.id.as_ref().map(|id| id.starts_with("rest_")).unwrap_or(false));
        assert!(has_rest_id, "Quarter rest should have an ID starting with rest_");

        // Verify Tempo Mark
        let has_tempo = dl.items.iter().any(|item| item.id.as_ref().map(|id| id.starts_with("tempo_0_")).unwrap_or(false));
        assert!(has_tempo, "Tempo mark should be present in display list");

        // Verify Slurs & Ties (Curves and 'h' hammer-on text)
        let curve_count = dl.items.iter().filter(|item| matches!(&item.kind, DrawKind::Curve { .. })).count();
        assert!(curve_count >= 2, "Should have at least 2 curves for tie and slur, got {}", curve_count);

        let has_slur_id = dl.items.iter().any(|item| item.id.as_ref().map(|id| id.starts_with("slur_")).unwrap_or(false));
        assert!(has_slur_id, "Slur should have an ID starting with slur_");

        let has_tie_id = dl.items.iter().any(|item| item.id.as_ref().map(|id| id.starts_with("tie_")).unwrap_or(false));
        assert!(has_tie_id, "Tie should have an ID starting with tie_");

        let has_hammer = dl.items.iter().any(|item| match &item.kind {
            DrawKind::Text { text, .. } => text == "h",
            _ => false,
        });
        assert!(has_hammer, "Hammer-on 'h' label should be present");

        // Verify Beams (stroke thickness 3.0)
        let has_beam = dl.items.iter().any(|item| match &item.kind {
            DrawKind::Line { stroke, .. } => stroke.width >= 2.9,
            _ => false,
        });
        assert!(has_beam, "Beam horizontal line should be present");

        // Verify Tuplet (tuplet multiple text "3")
        let has_tuplet = dl.items.iter().any(|item| match &item.kind {
            DrawKind::Text { text, font_id, .. } => text == "3" && font_id.size < 13.0,
            _ => false,
        });
        assert!(has_tuplet, "Tuplet number '3' should be present");
        let has_tuplet_id = dl.items.iter().any(|item| item.id.as_ref().map(|id| id.starts_with("tuplet_")).unwrap_or(false));
        assert!(has_tuplet_id, "Tuplet bracket/number should have an ID starting with tuplet_");

        // Also verify vector PDF generation succeeds with all these elements
        let pdf_bytes = generate_score_pdf_bytes(&doc, &pages).expect("PDF generation should succeed");
        assert!(!pdf_bytes.is_empty());
    }

    #[test]
    fn test_layout_element_adjustment_and_hit_testing() {
        let yaml = r#"---
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
      ending: { numbers: [1], duration: 1, open: false }
      tempos:
        - bpm: 100
          value: { base: quarter }
parts:
  - name: "Guitar"
    _x:
      microFretDivisions: 12
    measures:
      - sequences:
          - content:
              - type: tuplet
                inner: { duration: { base: eighth }, multiple: 3 }
                outer: { duration: { base: quarter }, multiple: 1 }
                content:
                  - id: "e1"
                    duration: { base: eighth }
                    notes:
                      - id: "n1"
                        pitch: { step: E, octave: 4 }
                        ties: [{ target: "e2" }]
                        _x:
                          string: 1
                          fret: 0
                    slurs: [{ target: "e2" }]
                  - id: "e2"
                    duration: { base: eighth }
                    notes:
                      - id: "n2"
                        pitch: { step: G, octave: 4 }
                        _x:
                          string: 1
                          fret: 3
                  - id: "e3"
                    duration: { base: eighth }
                    rest: {}
              - duration: { base: dotted-half }
                rest: {}
"#;
        let doc: MnxDocument = serde_saphyr::from_str(yaml).unwrap();
        let pages = crate::layout::compute_score_layout(&doc, 0.0);
        let mut dl = DisplayList::new();
        populate_page_display_list(&doc, &pages[0], &mut dl);

        // 1. Verify tuplet bracket items all share tuplet_0_1 and can be hit-tested
        let tuplet_items: Vec<_> = dl.items.iter().filter(|i| i.id.as_deref() == Some("tuplet_0_1")).collect();
        assert_eq!(tuplet_items.len(), 4, "Tuplet should have 3 bracket lines and 1 text number");
        let tuplet_bbox = tuplet_items[0].bounding_box();
        let hit = dl.hit_test(tuplet_bbox.center());
        assert_eq!(hit, Some("tuplet_0_1".to_string()), "Tuplet line should be hit testable");

        // 2. Verify slur items share ID and can be hit tested
        let slur_items: Vec<_> = dl.items.iter().filter(|i| i.id.as_deref() == Some("slur_1_e2")).collect();
        assert_eq!(slur_items.len(), 2, "Slur should have curve and 'h' label");
        let slur_bbox = slur_items[0].bounding_box();
        let hit_slur = dl.hit_test(slur_bbox.center());
        assert_eq!(hit_slur, Some("slur_1_e2".to_string()), "Slur curve should be hit testable");

        // 3. Verify ending items share ID and can be hit tested
        let ending_items: Vec<_> = dl.items.iter().filter(|i| i.id.as_deref() == Some("ending_0")).collect();
        assert_eq!(ending_items.len(), 4, "Ending should have top line, 2 hooks, and text");
        let ending_hook_bbox = ending_items[2].bounding_box();
        let hit_ending = dl.hit_test(ending_hook_bbox.center());
        assert_eq!(hit_ending, Some("ending_0".to_string()), "Ending hook should be hit testable");

        // 4. Test translation of all components when offset is applied
        let mut offsets = std::collections::HashMap::new();
        offsets.insert("tuplet_0_1".to_string(), crate::mnx::Offset2D { dx: 10.0, dy: -15.0 });
        offsets.insert("slur_1_e2".to_string(), crate::mnx::Offset2D { dx: 5.0, dy: 7.0 });

        let orig_tuplet_bboxes: Vec<_> = tuplet_items.iter().map(|i| i.bounding_box()).collect();
        let orig_slur_bboxes: Vec<_> = slur_items.iter().map(|i| i.bounding_box()).collect();

        dl.apply_offsets(&offsets);

        let moved_tuplet_items: Vec<_> = dl.items.iter().filter(|i| i.id.as_deref() == Some("tuplet_0_1")).collect();
        for (orig, moved) in orig_tuplet_bboxes.iter().zip(moved_tuplet_items.iter()) {
            let m_bbox = moved.bounding_box();
            assert!((m_bbox.min.x - (orig.min.x + 10.0)).abs() < 0.01, "Tuplet x should shift by 10");
            assert!((m_bbox.min.y - (orig.min.y - 15.0)).abs() < 0.01, "Tuplet y should shift by -15");
        }

        let moved_slur_items: Vec<_> = dl.items.iter().filter(|i| i.id.as_deref() == Some("slur_1_e2")).collect();
        for (orig, moved) in orig_slur_bboxes.iter().zip(moved_slur_items.iter()) {
            let m_bbox = moved.bounding_box();
            assert!((m_bbox.min.x - (orig.min.x + 5.0)).abs() < 0.01, "Slur x should shift by 5");
            assert!((m_bbox.min.y - (orig.min.y + 7.0)).abs() < 0.01, "Slur y should shift by 7");
        }
    }

    #[test]
    fn test_four_bars_fit_in_system_by_default() {
        let yaml = r#"---
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
    - {}
    - {}
    - {}
parts:
  - name: "Guitar"
    measures:
      - sequences:
          - content:
              - duration: { base: quarter }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: quarter }
                notes: [{ pitch: { step: B, octave: 3 }, _x: { string: 2, fret: 0 } }]
              - duration: { base: half }
                notes: [{ pitch: { step: G, octave: 3 }, _x: { string: 3, fret: 0 } }]
      - sequences:
          - content:
              - duration: { base: whole }
                notes: [{ pitch: { step: D, octave: 3 }, _x: { string: 4, fret: 0 } }]
      - sequences:
          - content:
              - duration: { base: whole }
                notes: [{ pitch: { step: A, octave: 2 }, _x: { string: 5, fret: 0 } }]
      - sequences:
          - content:
              - duration: { base: whole }
                notes: [{ pitch: { step: E, octave: 2 }, _x: { string: 6, fret: 0 } }]
"#;
        let doc: MnxDocument = serde_saphyr::from_str(yaml).unwrap();
        let pages = crate::layout::compute_score_layout(&doc, 0.0);
        assert!(!pages.is_empty(), "Should generate at least one page");
        let sys0 = &pages[0].systems[0];
        assert_eq!(
            sys0.measure_range,
            0..4,
            "Roughly four bars should fit onto System 0 by default, got {:?}",
            sys0.measure_range
        );
    }

    #[test]
    fn test_layout_mode_beaming_matches_continuous_mode() {
        // 1. Verify 4/4 measure with 8 eighth notes splits at beat 2.0 (4 + 4 beaming)
        let yaml_4_4 = r#"---
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
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
"#;
        let doc_4_4: MnxDocument = serde_saphyr::from_str(yaml_4_4).unwrap();
        let pages_4_4 = crate::layout::compute_score_layout(&doc_4_4, 0.0);
        let mut dl_4_4 = DisplayList::new();
        populate_page_display_list(&doc_4_4, &pages_4_4[0], &mut dl_4_4);

        // Find beam lines (horizontal lines with thickness >= 2.9 at stem bottom)
        let beam_lines_4_4: Vec<_> = dl_4_4
            .items
            .iter()
            .filter(|item| match &item.kind {
                DrawKind::Line { from, to, stroke } => stroke.width >= 2.9 && from.y == to.y,
                _ => false,
            })
            .collect();

        // 8 eighth notes split into two groups of 4 notes -> exactly 3 + 3 = 6 beam segments
        // (Without the fix, it would be 7 beam segments spanning the entire measure!)
        assert_eq!(
            beam_lines_4_4.len(),
            6,
            "4/4 with 8 eighth notes must produce exactly 6 beam lines (3 in beats 1-2, 3 in beats 3-4)"
        );

        // 2. Verify 3/4 measure with 6 eighth notes splits into 2 + 2 + 2 beaming
        let yaml_3_4 = r#"---
mnx:
  version: 1
global:
  measures:
    - time: { count: 3, unit: 4 }
parts:
  - name: "Guitar"
    measures:
      - sequences:
          - content:
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
"#;
        let doc_3_4: MnxDocument = serde_saphyr::from_str(yaml_3_4).unwrap();
        let pages_3_4 = crate::layout::compute_score_layout(&doc_3_4, 0.0);
        let mut dl_3_4 = DisplayList::new();
        populate_page_display_list(&doc_3_4, &pages_3_4[0], &mut dl_3_4);

        let beam_lines_3_4: Vec<_> = dl_3_4
            .items
            .iter()
            .filter(|item| match &item.kind {
                DrawKind::Line { from, to, stroke } => stroke.width >= 2.9 && from.y == to.y,
                _ => false,
            })
            .collect();

        // 6 eighth notes in 3/4 split into 3 groups of 2 notes -> exactly 1 + 1 + 1 = 3 beam segments
        assert_eq!(
            beam_lines_3_4.len(),
            3,
            "3/4 with 6 eighth notes must produce exactly 3 beam lines (1 per pair: beats 1, 2, 3)"
        );
    }

    #[test]
    fn test_layout_mode_horizontal_spacing_measure_of_eighth_notes() {
        let yaml = r#"---
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
    - {}
    - {}
    - {}
parts:
  - name: "Guitar"
    measures:
      - sequences:
          - content:
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
      - sequences:
          - content:
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
      - sequences:
          - content:
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
      - sequences:
          - content:
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
              - duration: { base: eighth }
                notes: [{ pitch: { step: E, octave: 4 }, _x: { string: 1, fret: 0 } }]
"#;
        let doc: MnxDocument = serde_saphyr::from_str(yaml).unwrap();
        let pages = crate::layout::compute_score_layout(&doc, 0.0);
        assert!(!pages.is_empty());
        let sys0 = &pages[0].systems[0];
        assert_eq!(
            sys0.measure_range,
            0..4,
            "All four bars of 8 eighth notes must fit into System 0 with decreased default horizontal spacing"
        );

        let mut dl = DisplayList::new();
        populate_page_display_list(&doc, &pages[0], &mut dl);

        // Find the x coordinates of the 8 notes in measure 0
        let note_x_coords: Vec<f32> = (0..8)
            .map(|ev_i| {
                let nid = format!("note_0_{}_1", ev_i + 1);
                dl.items
                    .iter()
                    .find(|item| item.id.as_deref() == Some(&nid) && matches!(item.kind, DrawKind::Text { .. }))
                    .map(|item| match &item.kind {
                        DrawKind::Text { pos, .. } => pos.x,
                        _ => 0.0,
                    })
                    .expect("Note text must exist")
            })
            .collect();

        // Calculate spacings between consecutive eighth notes
        for i in 0..7 {
            let spacing = note_x_coords[i + 1] - note_x_coords[i];
            assert!(
                spacing < 20.0,
                "Eighth note spacing must be decreased below 20px, got {}px at index {}",
                spacing,
                i
            );
            assert!(
                spacing > 10.0,
                "Eighth note spacing should remain clear (>10px), got {}px at index {}",
                spacing,
                i
            );
        }
    }

    #[test]
    fn test_pdf_export_wysiwyg_parity() {
        let yaml = r#"---
mnx:
  version: 1
global:
  measures:
    - time: { count: 4, unit: 4 }
parts:
  - instrument:
      name: "Guitar"
      microFretDivisions: 12
    measures:
      - sequences:
          - content:
              - duration: { base: eighth }
                notes:
                  - pitch: { step: E, octave: 4 }
                    fret: 0
                    string: 1
              - duration: { base: eighth }
                notes:
                  - pitch: { step: F, alter: 1, octave: 4 }
                    fret: 2
                    string: 1
              - duration: { base: eighth }
                notes:
                  - pitch: { step: G, octave: 4 }
                    fret: 3
                    string: 1
              - duration: { base: eighth }
                notes:
                  - pitch: { step: A, octave: 4 }
                    fret: 5
                    string: 1
              - duration: { base: quarter }
                notes:
                  - pitch: { step: B, octave: 4 }
                    fret: 7
                    string: 1
              - duration: { base: quarter }
                notes:
                  - pitch: { step: B, octave: 3 }
                    fret: 0
                    string: 2
"#;
        let doc: MnxDocument = serde_saphyr::from_str(yaml).unwrap();
        let pages = crate::layout::compute_score_layout(&doc, 0.0);
        assert!(!pages.is_empty());

        let pdf_bytes = generate_score_pdf_bytes(&doc, &pages).expect("PDF generation should succeed");
        assert!(!pdf_bytes.is_empty(), "PDF bytes should not be empty");

        // Verify that the PDF is valid PDF syntax
        let pdf_str = String::from_utf8_lossy(&pdf_bytes);
        assert!(pdf_str.starts_with("%PDF-"));
    }
}

