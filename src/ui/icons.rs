//! Icons drawn in code — crisp at every DPI, no icon font to ship.

use egui::epaint::PathStroke;
use egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Play,
    Pause,
    Prev,
    Next,
    VolumeMute,
    VolumeLow,
    VolumeHigh,
    Playlist,
    Artwork,
    Pin,
    Add,
    Remove,
    Trash,
    Note,
    Warning,
    Stop,
    Minimize,
    Close,
    /// WaveFlow's three bars.
    Logo,
}

/// Paint `icon` centred in `rect`, scaled to its shorter side.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height());
    // Unit helpers: `u` maps a fraction of the icon size to points.
    let u = |f: f32| f * s;
    let p = |x: f32, y: f32| pos2(c.x + u(x), c.y + u(y));
    let stroke = Stroke::new((s * 0.09).max(1.2), color);

    match icon {
        Icon::Play => {
            painter.add(Shape::convex_polygon(
                vec![p(-0.26, -0.34), p(0.36, 0.0), p(-0.26, 0.34)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Pause => {
            let w = u(0.18);
            let h = u(0.34);
            painter.rect_filled(
                Rect::from_center_size(p(-0.17, 0.0), vec2(w, 2.0 * h)),
                1.5,
                color,
            );
            painter.rect_filled(
                Rect::from_center_size(p(0.17, 0.0), vec2(w, 2.0 * h)),
                1.5,
                color,
            );
        }
        Icon::Prev => {
            painter.rect_filled(
                Rect::from_center_size(p(-0.30, 0.0), vec2(u(0.12), u(0.60))),
                1.0,
                color,
            );
            painter.add(Shape::convex_polygon(
                vec![p(0.34, -0.30), p(0.34, 0.30), p(-0.16, 0.0)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Next => {
            painter.rect_filled(
                Rect::from_center_size(p(0.30, 0.0), vec2(u(0.12), u(0.60))),
                1.0,
                color,
            );
            painter.add(Shape::convex_polygon(
                vec![p(-0.34, -0.30), p(-0.34, 0.30), p(0.16, 0.0)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::VolumeMute | Icon::VolumeLow | Icon::VolumeHigh => {
            // Speaker body.
            painter.add(Shape::convex_polygon(
                vec![
                    p(-0.42, -0.14),
                    p(-0.24, -0.14),
                    p(-0.04, -0.32),
                    p(-0.04, 0.32),
                    p(-0.24, 0.14),
                    p(-0.42, 0.14),
                ],
                color,
                Stroke::NONE,
            ));
            match icon {
                Icon::VolumeMute => {
                    painter.line_segment([p(0.10, -0.16), p(0.36, 0.16)], stroke);
                    painter.line_segment([p(0.36, -0.16), p(0.10, 0.16)], stroke);
                }
                Icon::VolumeLow => arc(painter, p(-0.04, 0.0), u(0.22), stroke),
                _ => {
                    arc(painter, p(-0.04, 0.0), u(0.22), stroke);
                    arc(painter, p(-0.04, 0.0), u(0.38), stroke);
                }
            }
        }
        Icon::Playlist => {
            for (i, y) in [-0.26_f32, 0.0, 0.26].iter().enumerate() {
                let x0 = -0.34;
                let x1 = if i == 2 { 0.10 } else { 0.34 };
                painter.line_segment([p(x0, *y), p(x1, *y)], stroke);
            }
            // Play glyph on the last row.
            painter.add(Shape::convex_polygon(
                vec![p(0.20, 0.14), p(0.36, 0.26), p(0.20, 0.38)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Artwork => {
            let frame = Rect::from_center_size(c, vec2(u(0.70), u(0.62)));
            painter.rect_stroke(frame, 2.0, stroke, egui::StrokeKind::Inside);
            painter.add(Shape::convex_polygon(
                vec![
                    p(-0.28, 0.22),
                    p(-0.08, -0.04),
                    p(0.08, 0.12),
                    p(0.16, 0.04),
                    p(0.28, 0.22),
                ],
                color,
                Stroke::NONE,
            ));
            painter.circle_filled(p(0.14, -0.14), u(0.06), color);
        }
        Icon::Pin => {
            // A thumbtack: head, shaft, point.
            painter.add(Shape::convex_polygon(
                vec![
                    p(-0.20, -0.30),
                    p(0.20, -0.30),
                    p(0.14, -0.02),
                    p(-0.14, -0.02),
                ],
                color,
                Stroke::NONE,
            ));
            painter.rect_filled(
                Rect::from_center_size(p(0.0, 0.04), vec2(u(0.44), u(0.08))),
                1.0,
                color,
            );
            painter.line_segment([p(0.0, 0.08), p(0.0, 0.36)], stroke);
        }
        Icon::Add => {
            painter.line_segment([p(-0.32, 0.0), p(0.32, 0.0)], stroke);
            painter.line_segment([p(0.0, -0.32), p(0.0, 0.32)], stroke);
        }
        Icon::Remove => {
            painter.line_segment([p(-0.32, 0.0), p(0.32, 0.0)], stroke);
        }
        Icon::Trash => {
            painter.line_segment([p(-0.34, -0.24), p(0.34, -0.24)], stroke);
            painter.line_segment([p(-0.10, -0.34), p(0.10, -0.34)], stroke);
            let body = Rect::from_min_max(p(-0.24, -0.20), p(0.24, 0.34));
            painter.rect_stroke(body, 2.0, stroke, egui::StrokeKind::Inside);
            painter.line_segment([p(-0.08, -0.08), p(-0.08, 0.22)], stroke);
            painter.line_segment([p(0.08, -0.08), p(0.08, 0.22)], stroke);
        }
        Icon::Note => {
            painter.circle_filled(p(-0.16, 0.22), u(0.14), color);
            painter.circle_filled(p(0.22, 0.12), u(0.14), color);
            painter.line_segment([p(-0.03, 0.20), p(-0.03, -0.30)], stroke);
            painter.line_segment([p(0.35, 0.10), p(0.35, -0.40)], stroke);
            painter.line_segment([p(-0.03, -0.30), p(0.35, -0.40)], stroke);
        }
        Icon::Stop => {
            painter.rect_filled(
                Rect::from_center_size(c, vec2(u(0.56), u(0.56))),
                1.5,
                color,
            );
        }
        Icon::Minimize => {
            painter.line_segment([p(-0.30, 0.0), p(0.30, 0.0)], stroke);
        }
        Icon::Close => {
            painter.line_segment([p(-0.28, -0.28), p(0.28, 0.28)], stroke);
            painter.line_segment([p(0.28, -0.28), p(-0.28, 0.28)], stroke);
        }
        Icon::Logo => {
            for (x, h) in [(-0.30_f32, 0.30_f32), (0.0, 0.46), (0.30, 0.22)] {
                painter.rect_filled(
                    Rect::from_center_size(p(x, 0.0), vec2(u(0.16), u(2.0 * h))),
                    1.0,
                    color,
                );
            }
        }
        Icon::Warning => {
            painter.add(Shape::convex_polygon(
                vec![p(0.0, -0.36), p(0.40, 0.32), p(-0.40, 0.32)],
                color,
                Stroke::NONE,
            ));
            let dark = Color32::from_rgb(0x12, 0x12, 0x12);
            painter.line_segment(
                [p(0.0, -0.12), p(0.0, 0.12)],
                Stroke::new((s * 0.09).max(1.2), dark),
            );
            painter.circle_filled(p(0.0, 0.22), (s * 0.05).max(1.0), dark);
        }
    }
}

/// A right-facing arc (volume waves).
fn arc(painter: &Painter, center: Pos2, radius: f32, stroke: Stroke) {
    let steps = 10;
    let a0 = -0.9_f32;
    let a1 = 0.9_f32;
    let points: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let t = a0 + (a1 - a0) * (i as f32 / steps as f32);
            pos2(center.x + radius * t.cos(), center.y + radius * t.sin())
        })
        .collect();
    painter.add(Shape::line(points, PathStroke::from(stroke)));
}
