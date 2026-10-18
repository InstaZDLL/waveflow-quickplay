//! The settings page, shown in place of the controls while it is open: the
//! interface language, and an "about" block with the version, the licence
//! and a link to the repository. Laid out to fit the compact bar without
//! scrolling in English; a scroll bar appears when a language needs more.

use egui::{vec2, Align, Layout, RichText, Sense, Ui};

use super::icons::{self, Icon};
use super::widgets::icon_button;
use super::{theme, Action};
use crate::i18n::{fill, Lang, Strings};

const REPOSITORY: &str = "https://github.com/InstaZDLL/waveflow-quickplay";

/// `language` is the stored preference: a code, or empty for the system's.
pub fn show(ui: &mut Ui, t: &'static Strings, language: &str, actions: &mut Vec<Action>) {
    ui.spacing_mut().item_spacing = vec2(8.0, 2.0);

    // ── Header: gear and title at the left; close, then the language
    //    picker, at the right.
    ui.horizontal(|ui| {
        let (gear, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
        icons::paint(ui.painter(), gear, Icon::Settings, theme::TEXT_DIM);
        ui.label(
            RichText::new(t.settings)
                .font(theme::bold(13.0))
                .color(theme::TEXT),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if icon_button(ui, Icon::Close, 22.0, t.tip_close, false).clicked() {
                actions.push(Action::ToggleSettings);
            }
            ui.add_space(4.0);
            language_picker(ui, t, language, actions);
            ui.label(RichText::new(t.language).size(12.0).color(theme::TEXT_DIM));
        });
    });

    let line = ui.available_rect_before_wrap();
    ui.painter().hline(
        line.x_range(),
        line.top() + 1.0,
        egui::Stroke::new(1.0, theme::ELEVATED),
    );
    ui.add_space(2.0);

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 2.0);

            // ── About: name and version, the tagline, then engine, licence
            //    and the link on one wrapped line.
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 0.0);
                ui.label(RichText::new(t.about).size(12.0).color(theme::TEXT_DIM));
                let (logo, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                icons::paint(ui.painter(), logo, Icon::Logo, theme::ACCENT);
                ui.label(
                    RichText::new("WaveFlow QuickPlay")
                        .font(theme::bold(13.0))
                        .color(theme::TEXT),
                );
                ui.label(
                    RichText::new(fill(t.about_version, &[("v", env!("CARGO_PKG_VERSION"))]))
                        .size(11.5)
                        .color(theme::TEXT_FAINT),
                );
            });
            ui.label(
                RichText::new(t.about_tagline)
                    .size(11.0)
                    .color(theme::TEXT_DIM),
            );
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 2.0);
                let faint = |s: &str| RichText::new(s).size(11.0).color(theme::TEXT_FAINT);
                ui.label(faint(t.about_engine));
                ui.label(faint("·"));
                ui.label(faint(t.about_license));
                ui.label(faint("·"));
                ui.hyperlink_to(
                    RichText::new(t.about_source)
                        .size(11.0)
                        .color(theme::ACCENT_HOVER),
                    REPOSITORY,
                )
                .on_hover_text(REPOSITORY);
            });
        });
}

/// One combo: the system entry first, then every language under its own
/// name. Picking one pushes [`Action::SetLanguage`].
fn language_picker(ui: &mut Ui, t: &Strings, language: &str, actions: &mut Vec<Action>) {
    let selected = Lang::from_code(language);
    let system_label = fill(t.language_system, &[("l", Lang::system().native_name())]);
    let current = selected.map_or_else(|| system_label.clone(), |l| l.native_name().to_string());
    egui::ComboBox::from_id_salt("language")
        .selected_text(RichText::new(current).size(12.0))
        .width(190.0)
        .height(96.0)
        .show_ui(ui, |ui| {
            let mut pick = |ui: &mut Ui, code: &str, label: &str, is_selected: bool| {
                if ui.selectable_label(is_selected, label).clicked() && !is_selected {
                    actions.push(Action::SetLanguage(code.to_string()));
                }
            };
            pick(ui, "", &system_label, selected.is_none());
            for lang in Lang::ALL {
                pick(ui, lang.code(), lang.native_name(), selected == Some(lang));
            }
        });
}
