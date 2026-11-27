//! Toolbar icons drawn as vector shapes with the egui painter, so they stay
//! crisp at any scale and need no icon font.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2};

use crate::annotate::Tool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Tool(Tool),
    Undo,
    Redo,
    Copy,
    Save,
}

/// Paints `icon` centred in `rect` (designed on an 18×18 grid).
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let s = rect.width().min(rect.height()) / 18.0;
    let o = rect.center() - vec2(9.0 * s, 9.0 * s);
    // Grid coordinates to screen.
    let p = |x: f32, y: f32| pos2(o.x + x * s, o.y + y * s);
    let line = Stroke::new(1.6 * s, color);
    match icon {
        Icon::Tool(Tool::Select) => {
            let pts = [
                p(4.0, 2.0),
                p(4.0, 15.0),
                p(7.4, 11.8),
                p(9.8, 16.6),
                p(11.8, 15.7),
                p(9.5, 11.0),
                p(14.0, 11.0),
            ];
            painter.add(Shape::convex_polygon(
                pts[..3].to_vec(),
                color,
                Stroke::NONE,
            ));
            painter.add(Shape::closed_line(
                pts.to_vec(),
                Stroke::new(1.4 * s, color),
            ));
            painter.add(Shape::convex_polygon(
                vec![pts[0], pts[2], pts[6]],
                color,
                Stroke::NONE,
            ));
            painter.add(Shape::convex_polygon(
                vec![pts[2], pts[3], pts[4], pts[5]],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Tool(Tool::Arrow) => {
            painter.line_segment([p(3.0, 15.0), p(12.0, 6.0)], line);
            painter.add(Shape::convex_polygon(
                vec![p(15.0, 3.0), p(14.2, 10.2), p(7.8, 3.8)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Tool(Tool::Rect) => {
            painter.rect_stroke(
                Rect::from_min_max(p(2.5, 4.0), p(15.5, 14.0)),
                1.5 * s,
                line,
                StrokeKind::Middle,
            );
        }
        Icon::Tool(Tool::Ellipse) => {
            painter.circle_stroke(p(9.0, 9.0), 6.5 * s, line);
        }
        Icon::Tool(Tool::Pen) => {
            let pts: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let t = i as f32 / 24.0;
                    let x = 2.5 + 13.0 * t;
                    let y = 9.0 - 4.0 * (t * std::f32::consts::TAU * 1.25).sin() * (1.0 - 0.3 * t);
                    p(x, y)
                })
                .collect();
            painter.add(Shape::line(pts, line));
        }
        Icon::Tool(Tool::Highlighter) => {
            // A translucent band under a slanted chisel-tip marker.
            painter.rect_filled(
                Rect::from_min_max(p(2.0, 12.5), p(16.0, 16.0)),
                0.8 * s,
                color.gamma_multiply(0.45),
            );
            painter.add(Shape::closed_line(
                vec![p(7.5, 8.5), p(13.0, 3.0), p(16.0, 6.0), p(10.5, 11.5)],
                Stroke::new(1.4 * s, color),
            ));
            painter.add(Shape::convex_polygon(
                vec![p(7.5, 8.5), p(10.5, 11.5), p(6.5, 13.0), p(6.0, 12.5)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Tool(Tool::Text) => {
            let thick = Stroke::new(2.0 * s, color);
            painter.line_segment([p(3.5, 3.5), p(14.5, 3.5)], thick);
            painter.line_segment([p(9.0, 3.5), p(9.0, 15.0)], thick);
            painter.line_segment([p(6.8, 15.0), p(11.2, 15.0)], line);
        }
        Icon::Tool(Tool::Pixelate) => {
            let cell = 4.0;
            for row in 0..3 {
                for col in 0..3 {
                    let alpha = [1.0, 0.35, 0.7, 0.5, 1.0, 0.3, 0.8, 0.45, 1.0][row * 3 + col];
                    let min = p(3.0 + col as f32 * cell, 3.0 + row as f32 * cell);
                    painter.rect_filled(
                        Rect::from_min_size(min, vec2(cell * s - 0.6 * s, cell * s - 0.6 * s)),
                        0.0,
                        color.gamma_multiply(alpha),
                    );
                }
            }
        }
        Icon::Undo | Icon::Redo => {
            let flip = icon == Icon::Redo;
            let fx = |x: f32| if flip { 18.0 - x } else { x };
            // A hook: left-pointing head, then an arc curling down to the right.
            let mut pts = vec![p(fx(4.0), 7.0), p(fx(10.5), 7.0)];
            for i in 0..=12 {
                let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 12.0;
                pts.push(p(fx(10.5 + 4.0 * a.cos()), 11.0 + 4.0 * a.sin()));
            }
            pts.push(p(fx(7.0), 15.0));
            painter.add(Shape::line(pts, line));
            painter.add(Shape::convex_polygon(
                if flip {
                    vec![p(fx(2.0), 7.0), p(fx(6.2), 3.6), p(fx(6.2), 10.4)]
                } else {
                    vec![p(2.0, 7.0), p(6.2, 10.4), p(6.2, 3.6)]
                },
                color,
                Stroke::NONE,
            ));
        }
        Icon::Copy => {
            let r = 1.6 * s;
            painter.rect_stroke(
                Rect::from_min_max(p(6.5, 6.0), p(15.5, 16.0)),
                r,
                line,
                StrokeKind::Middle,
            );
            // The back sheet shows only where the front one does not cover it.
            painter.add(Shape::line(
                vec![
                    p(4.5, 12.5),
                    p(2.8, 12.5),
                    p(2.8, 2.5),
                    p(11.5, 2.5),
                    p(11.5, 4.2),
                ],
                line,
            ));
        }
        Icon::Save => {
            painter.add(Shape::closed_line(
                vec![
                    p(2.5, 2.5),
                    p(12.8, 2.5),
                    p(15.5, 5.2),
                    p(15.5, 15.5),
                    p(2.5, 15.5),
                ],
                line,
            ));
            painter.rect_stroke(
                Rect::from_min_max(p(5.5, 2.5), p(11.5, 6.5)),
                0.0,
                line,
                StrokeKind::Middle,
            );
            painter.rect_stroke(
                Rect::from_min_max(p(5.0, 10.0), p(13.0, 15.5)),
                0.8 * s,
                line,
                StrokeKind::Middle,
            );
        }
    }
}
