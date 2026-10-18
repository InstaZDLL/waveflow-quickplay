//! The custom controls the player needs: flat and bordered icon buttons,
//! the seek / volume slider, the custom title bar, and the resize edges of
//! an undecorated window.

use egui::{
    pos2, vec2, Align, Color32, CursorIcon, Label, Layout, Rect, Response, RichText, Sense, Ui,
    Vec2, ViewportCommand,
};

use super::icons::{self, Icon};
use super::theme;

/// A flat icon button. `active` tints it with the accent (a toggled state).
pub fn icon_button(ui: &mut Ui, icon: Icon, size: f32, tooltip: &str, active: bool) -> Response {
    icon_button_ex(ui, icon, size, tooltip, active, true)
}

pub fn icon_button_ex(
    ui: &mut Ui,
    icon: Icon,
    size: f32,
    tooltip: &str,
    active: bool,
    enabled: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let response = if enabled {
        response
    } else {
        ui.interact(rect, response.id.with("off"), Sense::hover())
    };
    if ui.is_rect_visible(rect) {
        let hovered = enabled && response.hovered();
        let pressed = enabled && response.is_pointer_button_down_on();
        if hovered || pressed {
            let bg = if pressed {
                theme::ELEVATED
            } else {
                theme::HOVER
            };
            ui.painter().rect_filled(rect, 3.0, bg);
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let color = if !enabled {
            theme::TEXT_FAINT
        } else if active {
            if hovered {
                theme::ACCENT_HOVER
            } else {
                theme::ACCENT
            }
        } else if hovered {
            theme::TEXT
        } else {
            theme::TEXT_DIM
        };
        icons::paint(ui.painter(), rect.shrink(size * 0.24), icon, color);
    }
    if tooltip.is_empty() {
        response
    } else {
        response.on_hover_text(tooltip)
    }
}

/// A bordered button for the bottom bar — PotPlayer's transport row.
pub fn bar_button(
    ui: &mut Ui,
    icon: Icon,
    size: Vec2,
    tooltip: &str,
    enabled: bool,
    accent: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let response = if enabled {
        response
    } else {
        ui.interact(rect, response.id.with("off"), Sense::hover())
    };
    if ui.is_rect_visible(rect) {
        let hovered = enabled && response.hovered();
        let pressed = enabled && response.is_pointer_button_down_on();
        let (fill, border) = if !enabled {
            (theme::PANEL, theme::BORDER)
        } else if pressed {
            (theme::PANEL, theme::ACCENT)
        } else if hovered {
            (theme::HOVER, Color32::from_rgb(0x55, 0x55, 0x55))
        } else {
            (theme::ELEVATED, theme::BORDER)
        };
        let painter = ui.painter();
        painter.rect_filled(rect, 3.0, fill);
        painter.rect_stroke(
            rect,
            3.0,
            egui::Stroke::new(1.0, border),
            egui::StrokeKind::Inside,
        );
        if hovered {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let color = if !enabled {
            theme::TEXT_FAINT
        } else if accent {
            theme::ACCENT_HOVER
        } else {
            theme::TEXT
        };
        let side = rect.height().min(rect.width());
        let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(side * 0.46));
        icons::paint(painter, icon_rect, icon, color);
    }
    if tooltip.is_empty() {
        response
    } else {
        response.on_hover_text(tooltip)
    }
}

pub struct SliderOutput {
    /// Fraction under the pointer while dragging.
    pub preview: Option<f32>,
    /// Fraction to apply: on click, and on drag release.
    pub commit: Option<f32>,
    /// Fraction to apply continuously while dragging (volume).
    pub live: Option<f32>,
}

/// A thin rail with an accent fill and a round handle. `width` of `None`
/// takes the available width.
pub fn slider(
    ui: &mut Ui,
    id: &str,
    fraction: f32,
    width: Option<f32>,
    height: f32,
    enabled: bool,
) -> SliderOutput {
    let width = width.unwrap_or_else(|| ui.available_width().max(10.0));
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click_and_drag());
    let response = if enabled {
        response
    } else {
        ui.interact(rect, response.id.with(id), Sense::hover())
    };
    let hovered = enabled && (response.hovered() || response.dragged());
    // The handle sits inside the rail so it never spills past the ends.
    let pad = 6.0;
    let x0 = rect.left() + pad;
    let x1 = rect.right() - pad;
    let frac_at = |x: f32| ((x - x0) / (x1 - x0).max(1.0)).clamp(0.0, 1.0);

    let mut out = SliderOutput {
        preview: None,
        commit: None,
        live: None,
    };
    if enabled {
        if response.dragged() {
            if let Some(pos) = response.interact_pointer_pos() {
                let f = frac_at(pos.x);
                out.preview = Some(f);
                out.live = Some(f);
            }
        }
        if response.drag_stopped() || response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                out.commit = Some(frac_at(pos.x));
            }
        }
        if hovered {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
    }

    if ui.is_rect_visible(rect) {
        let shown = out.preview.unwrap_or(fraction).clamp(0.0, 1.0);
        let rail_h = 3.0;
        let cy = rect.center().y;
        let rail = Rect::from_min_max(pos2(x0, cy - rail_h / 2.0), pos2(x1, cy + rail_h / 2.0));
        let painter = ui.painter();
        painter.rect_filled(rail, rail_h / 2.0, theme::RAIL);
        if enabled {
            let fill_x = x0 + (x1 - x0) * shown;
            let fill = Rect::from_min_max(rail.min, pos2(fill_x, rail.max.y));
            let color = if hovered {
                theme::ACCENT_HOVER
            } else {
                theme::ACCENT
            };
            painter.rect_filled(fill, rail_h / 2.0, color);
            let r = if hovered { 6.0 } else { 5.0 };
            painter.circle_filled(pos2(fill_x, cy), r, theme::TEXT);
            painter.circle_stroke(
                pos2(fill_x, cy),
                r,
                egui::Stroke::new(1.0, Color32::from_rgb(0x10, 0x10, 0x10)),
            );
        }
    }
    out
}

/// What the custom title bar's buttons asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleAction {
    Pin,
    Minimize,
    Close,
}

/// A draggable title bar for an undecorated window: logo, title, an
/// optional dim subtitle, then pin (when `pinned` is `Some`), minimize and
/// close. Fills the `ui` it is given; the caller sizes it.
pub fn title_bar(
    ui: &mut Ui,
    title: &str,
    subtitle: Option<&str>,
    pinned: Option<bool>,
    minimize: bool,
) -> Option<TitleAction> {
    let rect = ui.max_rect();
    // The drag region goes first so the buttons drawn after it win hover.
    let drag = ui.interact(rect, ui.id().with("title-drag"), Sense::click_and_drag());
    if drag.drag_started() {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    ui.painter().rect_filled(rect, 0.0, theme::TITLE_BG);
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0, Color32::from_rgb(0x24, 0x24, 0x24)),
    );

    let mut action = None;
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
        let (logo, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
        icons::paint(ui.painter(), logo, Icon::Logo, theme::ACCENT);
        ui.add(
            Label::new(
                RichText::new(title)
                    .font(theme::bold(12.5))
                    .color(theme::TEXT),
            )
            .truncate(),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing = vec2(2.0, 0.0);
            if icon_button(ui, Icon::Close, 22.0, "Close", false).clicked() {
                action = Some(TitleAction::Close);
            }
            if minimize && icon_button(ui, Icon::Minimize, 22.0, "Minimize", false).clicked() {
                action = Some(TitleAction::Minimize);
            }
            if let Some(pinned) = pinned {
                if icon_button(ui, Icon::Pin, 22.0, "Always on top (T)", pinned).clicked() {
                    action = Some(TitleAction::Pin);
                }
            }
            ui.add_space(6.0);
            if let Some(subtitle) = subtitle {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(
                        Label::new(RichText::new(subtitle).size(11.5).color(theme::TEXT_FAINT))
                            .truncate(),
                    );
                });
            }
        });
    });
    action
}

/// Resize handles along the edges of an undecorated window: a thin band
/// inside each edge and corner that starts a native resize when dragged.
pub fn resize_edges(ui: &mut Ui, allow_vertical: bool) {
    use egui::ResizeDirection as D;
    let rect = ui.ctx().content_rect();
    let b = 5.0;
    let c = 10.0;
    let edges: [(D, Rect); 8] = [
        (
            D::West,
            Rect::from_min_max(
                rect.left_top() + vec2(0.0, c),
                pos2(rect.left() + b, rect.bottom() - c),
            ),
        ),
        (
            D::East,
            Rect::from_min_max(
                pos2(rect.right() - b, rect.top() + c),
                rect.right_bottom() - vec2(0.0, c),
            ),
        ),
        (
            D::North,
            Rect::from_min_max(
                rect.left_top() + vec2(c, 0.0),
                pos2(rect.right() - c, rect.top() + b),
            ),
        ),
        (
            D::South,
            Rect::from_min_max(
                pos2(rect.left() + c, rect.bottom() - b),
                rect.right_bottom() - vec2(c, 0.0),
            ),
        ),
        (
            D::NorthWest,
            Rect::from_min_size(rect.left_top(), Vec2::splat(c)),
        ),
        (
            D::NorthEast,
            Rect::from_min_size(rect.right_top() - vec2(c, 0.0), Vec2::splat(c)),
        ),
        (
            D::SouthWest,
            Rect::from_min_size(rect.left_bottom() - vec2(0.0, c), Vec2::splat(c)),
        ),
        (
            D::SouthEast,
            Rect::from_min_size(rect.right_bottom() - Vec2::splat(c), Vec2::splat(c)),
        ),
    ];
    for (i, (dir, edge)) in edges.into_iter().enumerate() {
        let vertical = matches!(dir, D::North | D::South);
        if vertical && !allow_vertical {
            continue;
        }
        let resp = ui.interact(
            edge,
            ui.id().with(("resize-edge", i)),
            Sense::click_and_drag(),
        );
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(match dir {
                D::West | D::East => CursorIcon::ResizeHorizontal,
                D::North | D::South => CursorIcon::ResizeVertical,
                D::NorthWest | D::SouthEast => CursorIcon::ResizeNwSe,
                D::NorthEast | D::SouthWest => CursorIcon::ResizeNeSw,
            });
        }
        if resp.drag_started() {
            ui.ctx()
                .send_viewport_cmd(ViewportCommand::BeginResize(dir));
        }
    }
}

/// The bar visualizer: `bands` in 0..1, drawn as LED-style stacks in the
/// accent colour, dim where unlit. Click toggles it.
pub fn spectrum_bars(ui: &mut Ui, rect: Rect, bands: &[f32]) -> Response {
    let response = ui.interact(rect, ui.id().with("spectrum"), Sense::click());
    if !ui.is_rect_visible(rect) || bands.is_empty() {
        return response;
    }
    let painter = ui.painter();
    let gap = 1.5;
    let n = bands.len() as f32;
    let bar_w = ((rect.width() - gap * (n - 1.0)) / n).max(1.0);
    // Continuous bars: a dim track for the full height, the lit part on
    // top, brighter the higher it reaches. Sub-pixel heights keep the
    // motion smooth rather than stepped.
    let track = Color32::from_rgb(0x22, 0x22, 0x22);
    for (i, &v) in bands.iter().enumerate() {
        let x = rect.left() + i as f32 * (bar_w + gap);
        let full = Rect::from_min_max(pos2(x, rect.top()), pos2(x + bar_w, rect.bottom()));
        painter.rect_filled(full, 1.0, track);
        let v = v.clamp(0.0, 1.0);
        if v <= 0.0 {
            continue;
        }
        let h = (rect.height() * v).max(1.5);
        let lit = Rect::from_min_max(pos2(x, rect.bottom() - h), pos2(x + bar_w, rect.bottom()));
        painter.rect_filled(
            lit,
            1.0,
            lerp_color(theme::ACCENT_DIM, theme::ACCENT_HOVER, v),
        );
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response.on_hover_text("Spectrum (S)")
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// A single line of text that scrolls horizontally when it is wider than
/// the space it gets (PotPlayer's title / lyric line). Short text is drawn
/// as a plain label. Long text holds still for a moment, scrolls left at a
/// steady pace with a gap before it comes round again, and starts over
/// whenever the text changes, so every lyric line is read from its start.
pub fn marquee(ui: &mut Ui, text: RichText, id: &str) -> Response {
    const HOLD_SECS: f64 = 1.5;
    const SPEED: f64 = 28.0; // points per second
    const GAP: f32 = 48.0;

    let width = ui.available_width();
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Body,
    );
    let text_w = galley.size().x;
    let (rect, response) = ui.allocate_exact_size(vec2(width, galley.size().y), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter().with_clip_rect(rect);
    if text_w <= width {
        painter.galley(rect.left_top(), galley, theme::TEXT);
        return response;
    }

    // Phase of the animation, restarted when the text changes.
    let key = response.id.with(id);
    let hash = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        galley.job.text.hash(&mut h);
        h.finish()
    };
    let now = ui.input(|i| i.time);
    let start = ui.ctx().data_mut(|d| {
        let (h, t) = d.get_temp::<(u64, f64)>(key).unwrap_or((0, now));
        if h != hash {
            d.insert_temp(key, (hash, now));
            now
        } else {
            t
        }
    });
    let travel = (text_w + GAP) as f64;
    let period = HOLD_SECS + travel / SPEED;
    let phase = (now - start) % period;
    let offset = ((phase - HOLD_SECS).max(0.0) * SPEED) as f32;

    let origin = pos2(rect.left() - offset, rect.top());
    painter.galley(origin, galley.clone(), theme::TEXT);
    painter.galley(origin + vec2(text_w + GAP, 0.0), galley, theme::TEXT);
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(16));
    response
}
