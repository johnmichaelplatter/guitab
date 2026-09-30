use eframe::egui::{Color32, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

#[derive(Clone, Debug, PartialEq)]
pub struct CurveNode {
    pub anchor: Pos2,
    pub right_handle: Pos2,
    pub left_handle: Pos2,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MonotonicCurveEditor {
    pub nodes: Vec<CurveNode>,
    pub last_rect: Option<Rect>,
}

impl MonotonicCurveEditor {
    pub fn new(rect: Rect) -> Self {
        let bottom_left = rect.left_bottom();
        let top_right = rect.right_top();

        let delta = top_right - bottom_left;
        
        Self {
            nodes: vec![
                CurveNode {
                    anchor: bottom_left,
                    left_handle: bottom_left,
                    right_handle: bottom_left + delta * 0.33,
                },
                CurveNode {
                    anchor: top_right,
                    left_handle: bottom_left + delta * 0.66,
                    right_handle: top_right,
                },
            ],
            last_rect: Some(rect),
        }
    }
    
    pub fn from_svg_path_normalized(path_str: &str, rect: Rect) -> Self {
        if path_str.trim().is_empty() {
            return Self::new(rect);
        }
        
        let mut nodes = Vec::new();
        let tokens: Vec<&str> = path_str.split_whitespace().collect();
        let mut i = 0;
        
        let map_x = |nx: f32| -> f32 { rect.min.x + (nx / 100.0) * rect.width() };
        let map_y = |ny: f32| -> f32 { rect.max.y - (ny / 100.0) * rect.height() };
        
        while i < tokens.len() {
            if tokens[i] == "M" {
                if i + 2 < tokens.len() {
                    let nx = tokens[i+1].parse::<f32>().unwrap_or(0.0);
                    let ny = tokens[i+2].parse::<f32>().unwrap_or(0.0);
                    let pos = Pos2::new(map_x(nx), map_y(ny));
                    nodes.push(CurveNode {
                        anchor: pos,
                        left_handle: pos,
                        right_handle: pos, // Will be updated by next C if it exists
                    });
                    i += 3;
                } else { break; }
            } else if tokens[i] == "C" {
                if i + 6 < tokens.len() {
                    let lx = tokens[i+1].trim_matches(',').parse::<f32>().unwrap_or(0.0);
                    let ly = tokens[i+2].trim_matches(',').parse::<f32>().unwrap_or(0.0);
                    let rx = tokens[i+3].trim_matches(',').parse::<f32>().unwrap_or(0.0);
                    let ry = tokens[i+4].trim_matches(',').parse::<f32>().unwrap_or(0.0);
                    let nx = tokens[i+5].trim_matches(',').parse::<f32>().unwrap_or(0.0);
                    let ny = tokens[i+6].trim_matches(',').parse::<f32>().unwrap_or(0.0);
                    
                    if let Some(last) = nodes.last_mut() {
                        last.right_handle = Pos2::new(map_x(lx), map_y(ly));
                    }
                    
                    let pos = Pos2::new(map_x(nx), map_y(ny));
                    nodes.push(CurveNode {
                        anchor: pos,
                        left_handle: Pos2::new(map_x(rx), map_y(ry)),
                        right_handle: pos, // default right handle
                    });
                    i += 7;
                } else { break; }
            } else {
                i += 1;
            }
        }
        
        if nodes.len() >= 2 {
            Self { nodes, last_rect: Some(rect) }
        } else {
            Self::new(rect)
        }
    }
    
    pub fn to_svg_path_normalized(&self) -> String {
        let rect = self.last_rect.unwrap();
        // Output normalized 0.0 to 100.0 coords
        let mut path = String::new();
        for (i, node) in self.nodes.iter().enumerate() {
            let nx = ((node.anchor.x - rect.min.x) / rect.width() * 100.0).clamp(0.0, 100.0);
            let ny = ((rect.max.y - node.anchor.y) / rect.height() * 100.0).clamp(0.0, 100.0);
            
            if i == 0 {
                path.push_str(&format!("M {:.1} {:.1} ", nx, ny));
            } else {
                let prev = &self.nodes[i - 1];
                let lx = ((prev.right_handle.x - rect.min.x) / rect.width() * 100.0).clamp(0.0, 100.0);
                let ly = ((rect.max.y - prev.right_handle.y) / rect.height() * 100.0).clamp(0.0, 100.0);
                
                let rx = ((node.left_handle.x - rect.min.x) / rect.width() * 100.0).clamp(0.0, 100.0);
                let ry = ((rect.max.y - node.left_handle.y) / rect.height() * 100.0).clamp(0.0, 100.0);
                
                path.push_str(&format!("C {:.1} {:.1}, {:.1} {:.1}, {:.1} {:.1} ", lx, ly, rx, ry, nx, ny));
            }
        }
        path.trim().to_string()
    }

    fn anchor_x_bounds(&self, index: usize, rect_bounds: Rect) -> (f32, f32) {
        let min_x = if index == 0 {
            rect_bounds.min.x
        } else {
            self.nodes[index - 1].anchor.x
        };

        let max_x = if index == self.nodes.len() - 1 {
            rect_bounds.max.x
        } else {
            self.nodes[index + 1].anchor.x
        };

        (min_x, max_x)
    }

    fn handle_x_bounds(&self, index: usize, is_right_handle: bool) -> (f32, f32) {
        let anchor_x = self.nodes[index].anchor.x;
        
        if is_right_handle {
            let next_x = if index < self.nodes.len() - 1 {
                self.nodes[index + 1].anchor.x
            } else {
                anchor_x
            };
            (anchor_x, next_x)
        } else {
            let prev_x = if index > 0 {
                self.nodes[index - 1].anchor.x
            } else {
                anchor_x
            };
            (prev_x, anchor_x)
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, rect: Rect) -> Response {
        if let Some(last) = self.last_rect {
            if last != rect {
                // Remap all nodes to new rect
                for node in &mut self.nodes {
                    let map_pos = |p: Pos2| -> Pos2 {
                        let nx = (p.x - last.min.x) / last.width();
                        let ny = (p.y - last.min.y) / last.height();
                        Pos2::new(rect.min.x + nx * rect.width(), rect.min.y + ny * rect.height())
                    };
                    node.anchor = map_pos(node.anchor);
                    node.left_handle = map_pos(node.left_handle);
                    node.right_handle = map_pos(node.right_handle);
                }
                self.last_rect = Some(rect);
            }
        } else {
            self.last_rect = Some(rect);
        }

        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter().clone();
        
        // Enforce bounds globally every frame to handle anchor dragging invalidating adjacent handles
        for i in 0..self.nodes.len() {
            let (l_min, l_max) = self.handle_x_bounds(i, false);
            self.nodes[i].left_handle.x = self.nodes[i].left_handle.x.clamp(l_min, l_max);
            self.nodes[i].left_handle.y = self.nodes[i].left_handle.y.clamp(rect.min.y, rect.max.y);
            
            let (r_min, r_max) = self.handle_x_bounds(i, true);
            self.nodes[i].right_handle.x = self.nodes[i].right_handle.x.clamp(r_min, r_max);
            self.nodes[i].right_handle.y = self.nodes[i].right_handle.y.clamp(rect.min.y, rect.max.y);
            
            // Also clamp anchors to rect Y bounds
            self.nodes[i].anchor.y = self.nodes[i].anchor.y.clamp(rect.min.y, rect.max.y);
        }
        
        // Ensure handles between adjacent nodes never cross (which breaks monotonicity)
        for i in 0..self.nodes.len().saturating_sub(1) {
            if self.nodes[i].right_handle.x > self.nodes[i+1].left_handle.x {
                let mid = (self.nodes[i].right_handle.x + self.nodes[i+1].left_handle.x) / 2.0;
                self.nodes[i].right_handle.x = mid;
                self.nodes[i+1].left_handle.x = mid;
            }
        }
        
        // Draw grid
        painter.rect_filled(rect, 0.0, Color32::from_rgb(30, 30, 30));
        let num_lines = 4;
        for i in 0..=num_lines {
            let y = rect.min.y + (rect.height() * i as f32 / num_lines as f32);
            painter.line_segment([Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)], Stroke::new(1.0, Color32::from_rgb(60, 60, 60)));
        }
        
        let curve_stroke = Stroke::new(3.0, Color32::LIGHT_BLUE);
        let handle_line_stroke = Stroke::new(1.5, Color32::GRAY);
        let point_radius = 6.0;

        if response.double_clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                self.insert_node_at_x(pos);
            }
        }

        for i in 0..self.nodes.len().saturating_sub(1) {
            let p0 = self.nodes[i].anchor;
            let p1 = self.nodes[i].right_handle;
            let p2 = self.nodes[i + 1].left_handle;
            let p3 = self.nodes[i + 1].anchor;

            let bezier = eframe::epaint::CubicBezierShape::from_points_stroke(
                [p0, p1, p2, p3],
                false,
                Color32::TRANSPARENT,
                curve_stroke,
            );
            painter.add(bezier);
        }

        for i in 0..self.nodes.len() {
            let anchor_rect = Rect::from_center_size(self.nodes[i].anchor, Vec2::splat(point_radius * 3.0));
            let anchor_resp = ui.interact(anchor_rect, ui.id().with(("anchor", i)), Sense::drag());
            
            if anchor_resp.dragged() {
                let delta = anchor_resp.drag_delta();
                let bounds = self.anchor_x_bounds(i, rect);
                
                self.nodes[i].anchor.x = (self.nodes[i].anchor.x + delta.x).clamp(bounds.0, bounds.1);
                // Clamp Y bounds strictly! (Guitar strings only bend up, so y is clamped to rect bounds)
                self.nodes[i].anchor.y = (self.nodes[i].anchor.y + delta.y).clamp(rect.min.y, rect.max.y);
                
                self.nodes[i].left_handle += delta;
                self.nodes[i].right_handle += delta;
            }
            painter.circle_filled(self.nodes[i].anchor, point_radius, Color32::WHITE);
            painter.circle_stroke(self.nodes[i].anchor, point_radius, Stroke::new(1.0, Color32::BLACK));

            if i < self.nodes.len() - 1 {
                self.draw_and_interact_handle(ui, &painter, i, true, handle_line_stroke, point_radius);
            }
            if i > 0 {
                self.draw_and_interact_handle(ui, &painter, i, false, handle_line_stroke, point_radius);
            }
        }

        response
    }

    fn draw_and_interact_handle(&mut self, ui: &mut Ui, painter: &eframe::egui::Painter, index: usize, is_right: bool, stroke: Stroke, radius: f32) {
        let (handle_pos, anchor_pos) = if is_right {
            (self.nodes[index].right_handle, self.nodes[index].anchor)
        } else {
            (self.nodes[index].left_handle, self.nodes[index].anchor)
        };

        painter.line_segment([anchor_pos, handle_pos], stroke);
        
        let rect = Rect::from_center_size(handle_pos, Vec2::splat(radius * 3.0));
        let id = ui.id().with(("handle", index, is_right));
        let resp = ui.interact(rect, id, Sense::drag());

        if resp.dragged() {
            let bounds = self.handle_x_bounds(index, is_right);
            let target = if is_right { &mut self.nodes[index].right_handle } else { &mut self.nodes[index].left_handle };
            
            target.x = (target.x + resp.drag_delta().x).clamp(bounds.0, bounds.1);
            target.y += resp.drag_delta().y;
        }

        painter.circle_filled(handle_pos, radius * 0.8, Color32::YELLOW);
        painter.circle_stroke(handle_pos, radius * 0.8, Stroke::new(1.0, Color32::BLACK));
    }

    fn insert_node_at_x(&mut self, pos: Pos2) {
        let insert_idx = self.nodes.partition_point(|n| n.anchor.x < pos.x);
        
        let new_node = CurveNode {
            anchor: pos,
            left_handle: pos + Vec2::new(-20.0, 0.0),
            right_handle: pos + Vec2::new(20.0, 0.0),
        };
        
        self.nodes.insert(insert_idx, new_node);
    }
}

pub fn parse_bend_string(val: &str, rect: Rect) -> MonotonicCurveEditor {
    MonotonicCurveEditor::from_svg_path_normalized(val, rect)
}

impl MonotonicCurveEditor {
    pub fn sample_y_at_x(&self, target_x: f32) -> f32 {
        if self.nodes.is_empty() {
            return 0.0;
        }
        if target_x <= self.nodes[0].anchor.x {
            return self.nodes[0].anchor.y;
        }
        if target_x >= self.nodes.last().unwrap().anchor.x {
            return self.nodes.last().unwrap().anchor.y;
        }
        
        // Find segment
        for i in 0..self.nodes.len() - 1 {
            let p0 = self.nodes[i].anchor;
            let p3 = self.nodes[i+1].anchor;
            if target_x >= p0.x && target_x <= p3.x {
                let p1 = self.nodes[i].right_handle;
                let p2 = self.nodes[i+1].left_handle;
                
                // Binary search for t
                let mut t_low = 0.0;
                let mut t_high = 1.0;
                let mut t = 0.5;
                for _ in 0..20 {
                    t = (t_low + t_high) / 2.0;
                    let u = 1.0 - t;
                    let current_x = u*u*u*p0.x + 3.0*u*u*t*p1.x + 3.0*u*t*t*p2.x + t*t*t*p3.x;
                    if current_x < target_x {
                        t_low = t;
                    } else {
                        t_high = t;
                    }
                }
                let u = 1.0 - t;
                let current_y = u*u*u*p0.y + 3.0*u*u*t*p1.y + 3.0*u*t*t*p2.y + t*t*t*p3.y;
                return current_y;
            }
        }
        0.0
    }
}
