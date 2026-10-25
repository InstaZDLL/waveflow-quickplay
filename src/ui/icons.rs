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
    /// A gear.
    Settings,
    /// WaveFlow's mark: the W of its name drawn as a wave.
    Logo,
    /// Two arrows chasing each other round a circle.
    Repeat,
    /// The same with a "1" inside.
    RepeatOne,
    /// Two crossing arrows.
    Shuffle,
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
        Icon::Settings => {
            // Eight teeth around a ring, a hole in the middle.
            let teeth = Stroke::new((s * 0.13).max(1.6), color);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let (sin, cos) = a.sin_cos();
                painter.line_segment(
                    [
                        pos2(c.x + u(0.24) * cos, c.y + u(0.24) * sin),
                        pos2(c.x + u(0.40) * cos, c.y + u(0.40) * sin),
                    ],
                    teeth,
                );
            }
            painter.circle_stroke(c, u(0.24), Stroke::new((s * 0.10).max(1.4), color));
            painter.circle_stroke(c, u(0.10), stroke);
        }
        Icon::Logo => {
            // WaveFlow's small-size symbol, in its 256 box: one and a half
            // periods of a sine from x 35 to 221 about y 124 (4 above the
            // middle, the optical centre), amplitude 60, stroked at 46 with
            // round ends. Its ends sit on the inflection points, so it
            // reads W and not M.
            let at = |x: f32, y: f32| p((x - 128.0) / 256.0, (y - 128.0) / 256.0);
            let points: Vec<Pos2> = (0..=48)
                .map(|i| {
                    let x = 35.0 + 186.0 * i as f32 / 48.0;
                    let y = 124.0 + 60.0 * (std::f32::consts::PI * (x - 35.0) / 62.0).sin();
                    at(x, y)
                })
                .collect();
            let width = u(46.0 / 256.0);
            painter.circle_filled(points[0], width / 2.0, color);
            painter.circle_filled(points[points.len() - 1], width / 2.0, color);
            painter.add(Shape::line(points, Stroke::new(width, color)));
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
        Icon::Repeat | Icon::RepeatOne => {
            // Two arcs, each ending in an arrowhead: over the top from left
            // to right, under the bottom from right to left.
            // Drawn large in its box: at the toolbar's 11 px the heads must
            // still read as arrows.
            let r = u(0.42);
            let deg = std::f32::consts::PI / 180.0;
            arrow_arc(painter, c, r, -150.0 * deg, -35.0 * deg, u(0.24), stroke);
            arrow_arc(painter, c, r, 30.0 * deg, 145.0 * deg, u(0.24), stroke);
            if icon == Icon::RepeatOne {
                painter.text(
                    c,
                    egui::Align2::CENTER_CENTER,
                    "1",
                    egui::FontId::proportional(s * 0.62),
                    color,
                );
            }
        }
        Icon::Shuffle => {
            // Two paths crossing in the middle, each ending in an arrowhead.
            for sign in [-1.0_f32, 1.0] {
                let y = 0.30 * sign;
                let points = vec![p(-0.48, y), p(-0.20, y), p(0.12, -y), p(0.26, -y)];
                painter.add(Shape::line(points, PathStroke::from(stroke)));
                arrow_head(painter, p(0.26, -y), vec2(1.0, 0.0), u(0.24), color);
            }
        }
    }
}

/// An arc of `radius` round `center` from angle `a0` to `a1` (screen
/// angles, y down), with an arrowhead at its end pointing along it.
fn arrow_arc(
    painter: &Painter,
    center: Pos2,
    radius: f32,
    a0: f32,
    a1: f32,
    head: f32,
    stroke: Stroke,
) {
    let steps = 12;
    let points: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let t = a0 + (a1 - a0) * (i as f32 / steps as f32);
            pos2(center.x + radius * t.cos(), center.y + radius * t.sin())
        })
        .collect();
    let end = points[steps];
    painter.add(Shape::line(points, PathStroke::from(stroke)));
    let tangent = vec2(-a1.sin(), a1.cos());
    arrow_head(painter, end, tangent, head, stroke.color);
}

/// A filled arrowhead whose base is at `at`, pointing along `dir`.
fn arrow_head(painter: &Painter, at: Pos2, dir: egui::Vec2, size: f32, color: Color32) {
    let dir = dir.normalized();
    let normal = vec2(-dir.y, dir.x);
    painter.add(Shape::convex_polygon(
        vec![
            at + dir * size,
            at + normal * size * 0.7,
            at - normal * size * 0.7,
        ],
        color,
        Stroke::NONE,
    ));
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
