use eframe::egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, Vec2};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct DrawItem {
    pub id: Option<String>,
    pub kind: DrawKind,
    pub z_index: i32,
}

#[derive(Debug, Clone)]
pub enum DrawKind {
    Line { from: Pos2, to: Pos2, stroke: Stroke },
    Rect { rect: Rect, fill: Color32, stroke: Stroke, rounding: f32 },
    Text { pos: Pos2, align: Align2, text: String, font_id: FontId, color: Color32 },
    Glyph { pos: Pos2, align: Align2, codepoint: char, font_family: String, size: f32, color: Color32 },
    Curve { points: [Pos2; 4], stroke: Stroke },
    Circle { center: Pos2, radius: f32, fill: Color32 },
}

impl DrawItem {
    pub fn translate(&mut self, dx: f32, dy: f32) {
        let delta = Vec2::new(dx, dy);
        match &mut self.kind {
            DrawKind::Line { from, to, .. } => {
                *from += delta;
                *to += delta;
            }
            DrawKind::Rect { rect, .. } => {
                *rect = rect.translate(delta);
            }
            DrawKind::Text { pos, .. } | DrawKind::Glyph { pos, .. } => {
                *pos += delta;
            }
            DrawKind::Circle { center, .. } => {
                *center += delta;
            }
            DrawKind::Curve { points, .. } => {
                for p in points.iter_mut() {
                    *p += delta;
                }
            }
        }
    }

    pub fn bounding_box(&self) -> Rect {
        match &self.kind {
            DrawKind::Line { from, to, .. } => Rect::from_two_pos(*from, *to).expand(6.0),
            DrawKind::Rect { rect, .. } => rect.expand(3.0),
            DrawKind::Text { pos, align, text, font_id, .. } => {
                let approx_w = (text.chars().count() as f32) * font_id.size * 0.65;
                let approx_h = font_id.size;
                let min_x = match align.x() {
                    eframe::emath::Align::Min => pos.x,
                    eframe::emath::Align::Center => pos.x - approx_w * 0.5,
                    eframe::emath::Align::Max => pos.x - approx_w,
                };
                let min_y = match align.y() {
                    eframe::emath::Align::Min => pos.y,
                    eframe::emath::Align::Center => pos.y - approx_h * 0.5,
                    eframe::emath::Align::Max => pos.y - approx_h,
                };
                Rect::from_min_size(Pos2::new(min_x, min_y), Vec2::new(approx_w, approx_h)).expand(4.0)
            }
            DrawKind::Glyph { pos, size, .. } => {
                Rect::from_center_size(*pos, Vec2::new(*size, *size)).expand(4.0)
            }
            DrawKind::Circle { center, radius, .. } => {
                Rect::from_center_size(*center, Vec2::splat((*radius).max(4.0) * 2.0)).expand(3.0)
            }
            DrawKind::Curve { points, .. } => {
                let mut r = Rect::from_two_pos(points[0], points[1]);
                r.extend_with(points[2]);
                r.extend_with(points[3]);
                r.expand(6.0)
            }
        }
    }

    pub fn render(&self, painter: &Painter) {
        match &self.kind {
            DrawKind::Line { from, to, stroke } => {
                painter.line_segment([*from, *to], *stroke);
            }
            DrawKind::Rect { rect, fill, stroke, rounding } => {
                painter.rect_filled(*rect, *rounding, *fill);
                if stroke.width > 0.0 {
                    painter.rect_stroke(*rect, *rounding, *stroke, eframe::egui::StrokeKind::Middle);
                }
            }
            DrawKind::Text { pos, align, text, font_id, color } => {
                painter.text(*pos, *align, text, font_id.clone(), *color);
            }
            DrawKind::Glyph { pos, align, codepoint, font_family, size, color } => {
                painter.text(
                    *pos,
                    *align,
                    codepoint.to_string(),
                    FontId::new(*size, eframe::egui::FontFamily::Name(font_family.clone().into())),
                    *color,
                );
            }
            DrawKind::Circle { center, radius, fill } => {
                painter.circle_filled(*center, *radius, *fill);
            }
            DrawKind::Curve { points, stroke } => {
                let shape = eframe::egui::epaint::CubicBezierShape::from_points_stroke(
                    *points,
                    false,
                    Color32::TRANSPARENT,
                    *stroke,
                );
                painter.add(shape);
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DisplayList {
    pub items: Vec<DrawItem>,
}

impl DisplayList {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn push(&mut self, item: DrawItem) {
        self.items.push(item);
    }

    pub fn apply_offsets(&mut self, offsets: &HashMap<String, crate::mnx::Offset2D>) {
        for item in &mut self.items {
            if let Some(id) = &item.id {
                if let Some(offset) = offsets.get(id) {
                    item.translate(offset.dx, offset.dy);
                }
            }
        }
    }

    pub fn render(&self, painter: &Painter) {
        let mut sorted: Vec<&DrawItem> = self.items.iter().collect();
        sorted.sort_by_key(|item| item.z_index);
        for item in sorted {
            item.render(painter);
        }
    }

    pub fn hit_test(&self, pos: Pos2) -> Option<String> {
        let mut sorted: Vec<&DrawItem> = self.items.iter().collect();
        sorted.sort_by_key(|item| -item.z_index); // topmost first
        for item in sorted {
            if let Some(id) = &item.id {
                if item.bounding_box().contains(pos) {
                    return Some(id.clone());
                }
            }
        }
        None
    }
}
