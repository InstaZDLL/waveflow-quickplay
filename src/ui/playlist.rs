//! The playlist: a custom title bar, a virtualised list of rows and a
//! bottom toolbar. Shown in its own native window when the backend
//! supports several viewports, embedded as an egui window otherwise.

use egui::{vec2, Align, Key, Label, Layout, Modifiers, RichText, Sense, Ui};

use super::icons::{self, Icon};
use super::widgets::{bar_button, title_bar, TitleAction};
use super::{format_time, theme, Action, TITLE_HEIGHT};
use crate::i18n::{fill, Strings};
use crate::queue::Queue;

const ROW_HEIGHT: f32 = 30.0;
const TOOL_BUTTON: egui::Vec2 = egui::Vec2::new(30.0, 24.0);

pub fn show(
    ui: &mut Ui,
    t: &'static Strings,
    queue: &Queue,
    selected: Option<usize>,
    actions: &mut Vec<Action>,
) {
    if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Delete)) {
        actions.push(Action::RemoveSelected);
    }

    egui::Panel::top("playlist-title")
        .exact_size(TITLE_HEIGHT)
        .show_separator_line(false)
        .frame(theme::title_frame())
        .show(ui, |ui| {
            let n = queue.len();
            let total = format_time(queue.total_duration_ms());
            let summary = match n {
                0 => t.playlist_empty.to_string(),
                1 => fill(t.playlist_one_track, &[("t", &total)]),
                n => fill(t.playlist_n_tracks, &[("n", &n.to_string()), ("t", &total)]),
            };
            if let Some(TitleAction::Close) =
                title_bar(ui, t, t.playlist, Some(&summary), None, false, None)
            {
                actions.push(Action::ClosePlaylist);
            }
        });

    egui::Panel::bottom("playlist-toolbar")
        .exact_size(34.0)
        .show_separator_line(false)
        .frame(
            egui::Frame::NONE
                .fill(theme::PANEL)
                .inner_margin(egui::Margin::symmetric(6, 5)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(3.0, 0.0);
                if bar_button(ui, Icon::Add, TOOL_BUTTON, t.tip_add_files, true, false).clicked() {
                    actions.push(Action::OpenFiles);
                }
                if bar_button(
                    ui,
                    Icon::Remove,
                    TOOL_BUTTON,
                    t.tip_remove_selected,
                    selected.is_some(),
                    false,
                )
                .clicked()
                {
                    actions.push(Action::RemoveSelected);
                }
                if bar_button(
                    ui,
                    Icon::Trash,
                    TOOL_BUTTON,
                    t.tip_clear_playlist,
                    !queue.is_empty(),
                    false,
                )
                .clicked()
                {
                    actions.push(Action::Clear);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(i) = selected {
                        ui.label(
                            RichText::new(format!("{} / {}", i + 1, queue.len()))
                                .size(11.0)
                                .color(theme::TEXT_FAINT),
                        );
                    }
                });
            });
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(theme::BG))
        .show(ui, |ui| {
            if queue.is_empty() {
                let rect = ui.available_rect_before_wrap();
                let icon =
                    egui::Rect::from_center_size(rect.center() - vec2(0.0, 12.0), vec2(40.0, 40.0));
                icons::paint(ui.painter(), icon, Icon::Playlist, theme::ELEVATED);
                ui.painter().text(
                    egui::pos2(rect.center().x, icon.bottom() + 8.0),
                    egui::Align2::CENTER_TOP,
                    t.playlist_drop_hint,
                    egui::FontId::proportional(12.0),
                    theme::TEXT_FAINT,
                );
                return;
            }
            let current = queue.current_index();
            egui::ScrollArea::vertical().auto_shrink(false).show_rows(
                ui,
                ROW_HEIGHT,
                queue.len(),
                |ui, range| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    for index in range {
                        let Some(entry) = queue.get(index) else {
                            continue;
                        };
                        row(
                            ui,
                            t,
                            index,
                            entry,
                            current == Some(index),
                            selected == Some(index),
                            actions,
                        );
                    }
                },
            );
        });
}

fn row(
    ui: &mut Ui,
    t: &Strings,
    index: usize,
    entry: &crate::queue::TrackEntry,
    is_current: bool,
    is_selected: bool,
    actions: &mut Vec<Action>,
) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT), Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let hovered = response.hovered();
    let painter = ui.painter();
    if is_selected {
        painter.rect_filled(rect, 0.0, theme::ELEVATED);
    } else if hovered {
        painter.rect_filled(rect, 0.0, theme::PANEL);
    }
    if is_current {
        painter.rect_filled(
            egui::Rect::from_min_size(rect.min, vec2(3.0, rect.height())),
            0.0,
            theme::ACCENT,
        );
    }

    let failed = entry.failed.is_some();
    let title_color = if failed {
        theme::TEXT_FAINT
    } else if is_current {
        theme::ACCENT_HOVER
    } else {
        theme::TEXT
    };
    let sub_color = if failed {
        theme::TEXT_FAINT
    } else {
        theme::TEXT_DIM
    };

    let inner = rect.shrink2(vec2(10.0, 0.0));
    let number_w = 28.0;
    let duration_w = 44.0;
    if failed {
        let r = egui::Rect::from_center_size(
            egui::pos2(inner.left() + 9.0, inner.center().y),
            vec2(14.0, 14.0),
        );
        icons::paint(painter, r, Icon::Warning, theme::DANGER);
    } else {
        painter.text(
            egui::pos2(inner.left() + number_w - 8.0, inner.center().y),
            egui::Align2::RIGHT_CENTER,
            format!("{}.", index + 1),
            egui::FontId::proportional(11.5),
            if is_current {
                theme::ACCENT
            } else {
                theme::TEXT_FAINT
            },
        );
    }
    if let Some(d) = entry.duration_ms() {
        painter.text(
            egui::pos2(inner.right(), inner.center().y),
            egui::Align2::RIGHT_CENTER,
            format_time(d),
            egui::FontId::proportional(11.5),
            sub_color,
        );
    }
    let text_rect = egui::Rect::from_min_max(
        egui::pos2(inner.left() + number_w, inner.top()),
        egui::pos2(inner.right() - duration_w, inner.bottom()),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(text_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    child.spacing_mut().item_spacing = vec2(0.0, 0.0);
    child.add_space(2.0);
    let title_font = if is_current {
        theme::bold(12.5)
    } else {
        egui::FontId::proportional(12.5)
    };
    child.add(
        Label::new(
            RichText::new(entry.title())
                .font(title_font)
                .color(title_color),
        )
        .truncate(),
    );
    let subtitle = match (entry.artist(), &entry.failed) {
        (_, Some(err)) => err.clone(),
        (Some(a), None) => a.to_string(),
        (None, None) => entry
            .path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string(),
    };
    child.add(Label::new(RichText::new(subtitle).size(10.5).color(sub_color)).truncate());

    if response.double_clicked() {
        actions.push(Action::PlayIndex(index));
    } else if response.clicked() {
        actions.push(Action::Select(Some(index)));
    }
    response.context_menu(|ui| {
        if ui.button(t.menu_play).clicked() {
            actions.push(Action::PlayIndex(index));
            ui.close();
        }
        if ui.button(t.menu_remove).clicked() {
            actions.push(Action::Remove(index));
            ui.close();
        }
    });
}
