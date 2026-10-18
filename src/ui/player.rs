//! The player pane: the information block, the seek row, the bottom
//! transport bar, and the artwork area.

use egui::{pos2, vec2, Align, Color32, Label, Layout, Rect, RichText, Ui};

use super::icons::{self, Icon};
use super::widgets::{bar_button, icon_button, slider, spectrum_bars};
use super::{format_time, theme, Action, PlayerView};
use crate::audio::PlayerState;

const BAR_BUTTON: egui::Vec2 = egui::Vec2::new(34.0, 26.0);

/// The controls block — exactly [`super::CONTROLS_HEIGHT`] tall including
/// the panel frame's margins.
pub fn controls(ui: &mut Ui, view: &PlayerView<'_>, actions: &mut Vec<Action>) {
    ui.spacing_mut().item_spacing = vec2(6.0, 2.0);
    let duration = view.duration_ms.filter(|d| *d > 0);
    let shown_ms = match (view.seek_preview, duration) {
        (Some(f), Some(d)) => (f as f64 * d as f64) as u64,
        _ => view.position_ms,
    };

    // ── Information block: big clock at the left, titles at the right.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 0.0);
        let clock_w = 78.0;
        let spectrum_w = if view.spectrum.is_some() { 112.0 } else { 0.0 };
        let (clock, _) = ui.allocate_exact_size(vec2(clock_w, 50.0), egui::Sense::hover());
        let painter = ui.painter();
        painter.text(
            pos2(clock.left(), clock.top() - 2.0),
            egui::Align2::LEFT_TOP,
            format_time(shown_ms),
            theme::bold(26.0),
            if view.has_track {
                theme::TEXT
            } else {
                theme::TEXT_FAINT
            },
        );
        painter.text(
            pos2(clock.left() + 1.0, clock.bottom()),
            egui::Align2::LEFT_BOTTOM,
            duration.map(format_time).unwrap_or_else(|| "--:--".into()),
            egui::FontId::proportional(11.5),
            theme::TEXT_DIM,
        );

        // The visualizer sits at the right edge of the block; the titles get
        // what is left between the clock and it.
        if let Some(bands) = view.spectrum {
            let right = ui.max_rect().right();
            let area = Rect::from_min_max(
                pos2(right - spectrum_w, clock.top() + 2.0),
                pos2(right, clock.bottom() - 2.0),
            );
            if spectrum_bars(ui, area, bands).clicked() {
                actions.push(Action::ToggleSpectrum);
            }
        }
        let text_w = (ui.available_width() - spectrum_w - 10.0).max(60.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 1.0);
            ui.set_height(50.0);
            ui.set_max_width(text_w);
            let title = if view.has_track {
                RichText::new(view.lyric.unwrap_or(view.title))
                    .font(theme::bold(14.0))
                    .color(theme::TEXT)
            } else {
                RichText::new("Nothing playing")
                    .font(theme::bold(14.0))
                    .color(theme::TEXT_DIM)
            };
            ui.add(Label::new(title).truncate());
            let subtitle = if view.has_track {
                match (view.artist, view.album) {
                    (Some(a), Some(b)) => format!("{a} · {b}"),
                    (Some(a), None) => a.to_string(),
                    (None, Some(b)) => b.to_string(),
                    (None, None) => "Unknown artist".to_string(),
                }
            } else {
                "Drop audio files here, or press Ctrl+O".to_string()
            };
            ui.add(
                Label::new(RichText::new(subtitle).size(12.0).color(theme::TEXT_DIM)).truncate(),
            );
            ui.add_space(2.0);
            if let Some(status) = view.status {
                ui.add(
                    Label::new(RichText::new(status).size(11.0).color(theme::DANGER)).truncate(),
                );
            } else if !view.format_chips.is_empty() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = vec2(12.0, 0.0);
                    for chip in &view.format_chips {
                        ui.label(RichText::new(chip).size(11.0).color(theme::TEXT_FAINT));
                    }
                });
            }
        });
    });

    ui.add_space(2.0);

    // ── Seek row: the bar, then the volume at the right.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 0.0);
        let vol_w = 72.0;
        let bar_w = (ui.available_width() - vol_w - 30.0).max(40.0);
        let fraction = duration
            .map(|d| (view.position_ms as f64 / d as f64) as f32)
            .unwrap_or(0.0);
        let out = slider(
            ui,
            "seek",
            fraction,
            Some(bar_w),
            18.0,
            view.has_track && duration.is_some(),
        );
        if let Some(f) = out.preview {
            actions.push(Action::SeekPreview(f));
        }
        if let Some(f) = out.commit {
            actions.push(Action::SeekCommit(f));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            let out = slider(ui, "volume", view.volume, Some(vol_w), 18.0, true);
            if let Some(v) = out.live.or(out.commit) {
                actions.push(Action::SetVolume(v));
            }
            let icon = if view.volume <= 0.001 {
                Icon::VolumeMute
            } else if view.volume < 0.5 {
                Icon::VolumeLow
            } else {
                Icon::VolumeHigh
            };
            let pct = format!("Volume {}%", (view.volume * 100.0).round() as u32);
            if icon_button(ui, icon, 22.0, &pct, false).clicked() {
                let next = if view.volume <= 0.001 { 0.8 } else { 0.0 };
                actions.push(Action::SetVolume(next));
            }
        });
    });

    ui.add_space(3.0);

    // ── Bottom bar: transport at the left, time in the middle, panels at
    //    the right.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(3.0, 0.0);
        let playing = view.state == PlayerState::Playing;
        let (icon, tip) = if playing {
            (Icon::Pause, "Pause (Space)")
        } else {
            (Icon::Play, "Play (Space)")
        };
        let can_play = view.has_track || view.has_next;
        if bar_button(ui, icon, BAR_BUTTON, tip, can_play, playing).clicked() {
            actions.push(Action::TogglePlay);
        }
        let stoppable = matches!(
            view.state,
            PlayerState::Playing | PlayerState::Paused | PlayerState::Loading
        );
        if bar_button(ui, Icon::Stop, BAR_BUTTON, "Stop", stoppable, false).clicked() {
            actions.push(Action::Stop);
        }
        if bar_button(
            ui,
            Icon::Prev,
            BAR_BUTTON,
            "Previous (P)",
            view.has_prev || view.has_track,
            false,
        )
        .clicked()
        {
            actions.push(Action::Prev);
        }
        if bar_button(ui, Icon::Next, BAR_BUTTON, "Next (N)", view.has_next, false).clicked() {
            actions.push(Action::Next);
        }
        ui.add_space(3.0);
        if bar_button(
            ui,
            Icon::Add,
            BAR_BUTTON,
            "Open files (Ctrl+O)",
            true,
            false,
        )
        .clicked()
        {
            actions.push(Action::OpenFiles);
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing = vec2(3.0, 0.0);
            if bar_button(
                ui,
                Icon::Playlist,
                BAR_BUTTON,
                "Playlist (L)",
                true,
                view.playlist_open,
            )
            .clicked()
            {
                actions.push(Action::TogglePlaylist);
            }
            if bar_button(
                ui,
                Icon::Artwork,
                BAR_BUTTON,
                "Artwork (A)",
                true,
                view.expanded,
            )
            .clicked()
            {
                actions.push(Action::ToggleExpanded);
            }
            ui.add_space(6.0);
            let time = match duration {
                Some(d) => format!("{} / {}", format_time(shown_ms), format_time(d)),
                None => format_time(shown_ms),
            };
            ui.with_layout(
                Layout::centered_and_justified(egui::Direction::LeftToRight),
                |ui| {
                    ui.add(
                        Label::new(RichText::new(time).size(12.0).color(theme::TEXT_DIM))
                            .truncate(),
                    );
                },
            );
        });
    });
}

/// The artwork pane: the cover fitted to the available area, or a quiet
/// placeholder. Fills whatever rect the layout gives it.
pub fn artwork(ui: &mut Ui, view: &PlayerView<'_>) {
    let rect = ui.available_rect_before_wrap();
    // A window mid-resize can hand us a sliver; nothing sensible fits in it,
    // and a negative fitted rect would panic egui's placer.
    if rect.width() < 40.0 || rect.height() < 40.0 {
        return;
    }
    ui.painter().rect_filled(rect, 0.0, Color32::BLACK);

    match view.cover {
        Some(tex) => {
            let tex_size = tex.size_vec2();
            if tex_size.x <= 0.0 || tex_size.y <= 0.0 {
                return;
            }
            let scale = (rect.width() / tex_size.x).min(rect.height() / tex_size.y);
            let size = tex_size * scale;
            if size.x < 1.0 || size.y < 1.0 {
                return;
            }
            let fitted = Rect::from_center_size(rect.center(), size);
            let image = egui::Image::from_texture(egui::load::SizedTexture::from_handle(tex))
                .fit_to_exact_size(size);
            ui.put(fitted, image);
        }
        None => {
            let side = rect.width().min(rect.height()).min(220.0) * 0.5;
            let icon_rect =
                Rect::from_center_size(rect.center() - vec2(0.0, 8.0), vec2(side, side));
            icons::paint(ui.painter(), icon_rect, Icon::Note, theme::ELEVATED);
            let label = if view.has_track {
                "No artwork"
            } else {
                "Nothing playing"
            };
            ui.painter().text(
                pos2(rect.center().x, icon_rect.bottom() + 10.0),
                egui::Align2::CENTER_TOP,
                label,
                egui::FontId::proportional(12.0),
                theme::TEXT_FAINT,
            );
        }
    }
}

/// Accent frame + label painted over the whole window while files are
/// dragged over it.
pub fn drop_overlay(ui: &mut Ui) {
    let rect = ui.ctx().content_rect();
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        0.0,
        Color32::from_rgba_unmultiplied(0x10, 0x10, 0x10, 190),
    );
    let inner = rect.shrink(4.0);
    painter.rect_stroke(
        inner,
        4.0,
        egui::Stroke::new(2.0, theme::ACCENT),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "Drop to add to the playlist",
        theme::bold(14.0),
        theme::ACCENT_HOVER,
    );
}
