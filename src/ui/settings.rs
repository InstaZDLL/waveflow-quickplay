//! The settings page, shown in place of the controls while it is open: the
//! interface language, an "about" block with the version, the licence and
//! a link to the repository, then the update check. The language and the
//! about block are laid out to fit the compact bar without scrolling in
//! English; the update check sits below them, where a scroll bar leads.

use egui::{vec2, Align, Layout, RichText, Sense, Ui};

use super::icons::{self, Icon};
use super::widgets::icon_button;
use super::{theme, Action};
use crate::i18n::{fill, Lang, Strings};

const REPOSITORY: &str = "https://github.com/InstaZDLL/waveflow-quickplay";

/// What the page shows of the settings and of the update check.
pub struct Page<'a> {
    /// The stored preference: a code, or empty for the system's.
    pub language: &'a str,
    pub check_updates: bool,
    /// A newer version this copy cannot install by itself.
    pub update_available: Option<&'a str>,
}

pub fn show(ui: &mut Ui, t: &'static Strings, page: &Page<'_>, actions: &mut Vec<Action>) {
    let language = page.language;
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
            // What is left between the title and the close button. The
            // "Language" label goes first when the bar is narrow (320
            // points fit no more than the picker), then the picker narrows
            // and truncates its text.
            let label = RichText::new(t.language).size(12.0).color(theme::TEXT_DIM);
            let label_width = ui
                .painter()
                .layout_no_wrap(
                    t.language.to_string(),
                    egui::FontId::proportional(12.0),
                    theme::TEXT_DIM,
                )
                .size()
                .x;
            let room = ui.available_width() - ui.spacing().item_spacing.x;
            let with_label = room >= PICKER_WIDTH + ui.spacing().item_spacing.x + label_width;
            let width = room.clamp(PICKER_MIN_WIDTH, PICKER_WIDTH);
            language_picker(ui, t, language, width, actions);
            if with_label {
                ui.label(label);
            }
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
            if let Some(version) = page.update_available {
                let page = format!("{}{version}", crate::update::RELEASE_PAGE);
                ui.hyperlink_to(
                    RichText::new(fill(t.status_update_available, &[("v", version)]))
                        .size(11.0)
                        .color(theme::ACCENT_HOVER),
                    &page,
                )
                .on_hover_text(&page);
            }

            // ── Updates, under the about block so that block still fits
            //    the compact bar; scrolled to there, or seen in the
            //    artwork mode.
            ui.add_space(6.0);
            let mut check = page.check_updates;
            let label = RichText::new(t.settings_check_updates)
                .size(12.0)
                .color(theme::TEXT_DIM);
            if ui.checkbox(&mut check, label).changed() {
                actions.push(Action::SetCheckUpdates(check));
            }
        });
}

/// The language picker's width when there is room, and the least it
/// narrows to.
const PICKER_WIDTH: f32 = 190.0;
const PICKER_MIN_WIDTH: f32 = 110.0;

/// One combo, `width` points wide: the system entry first, then every
/// language under its own name. Picking one pushes [`Action::SetLanguage`].
fn language_picker(
    ui: &mut Ui,
    t: &Strings,
    language: &str,
    width: f32,
    actions: &mut Vec<Action>,
) {
    let selected = Lang::from_code(language);
    let system_label = fill(t.language_system, &[("l", Lang::system().native_name())]);
    let current = selected.map_or_else(|| system_label.clone(), |l| l.native_name().to_string());
    // In a box of its own `width`, against which the selected text
    // truncates: a long system entry does not widen the combo.
    let height = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(
        vec2(width, height),
        Layout::right_to_left(Align::Center),
        |ui| {
            egui::ComboBox::from_id_salt("language")
                .selected_text(RichText::new(current).size(12.0))
                .width(width)
                .wrap_mode(egui::TextWrapMode::Truncate)
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
        },
    );
}
