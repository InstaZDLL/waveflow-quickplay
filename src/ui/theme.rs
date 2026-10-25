//! Dark theme in WaveFlow's identity: near-black surfaces and the desktop's
//! default emerald accent (Tailwind `emerald-500`, the `--accent-*` palette
//! in `src/app.css`). Tokens only — no theming system.

use std::sync::Arc;

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke, Visuals,
};

pub const BG: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);
pub const TITLE_BG: Color32 = Color32::from_rgb(0x10, 0x10, 0x10);
pub const PANEL: Color32 = Color32::from_rgb(0x14, 0x14, 0x14);
pub const ELEVATED: Color32 = Color32::from_rgb(0x26, 0x26, 0x26);
pub const HOVER: Color32 = Color32::from_rgb(0x33, 0x33, 0x33);
pub const BORDER: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x3a);
pub const RAIL: Color32 = Color32::from_rgb(0x44, 0x44, 0x44);
pub const TEXT: Color32 = Color32::from_rgb(0xf2, 0xf2, 0xf2);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0xa8, 0xa8, 0xa8);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x70, 0x70, 0x70);
/// emerald-500
pub const ACCENT: Color32 = Color32::from_rgb(0x10, 0xb9, 0x81);
/// emerald-400
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x34, 0xd3, 0x99);
/// emerald-600
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x05, 0x96, 0x69);
pub const DANGER: Color32 = Color32::from_rgb(0xf8, 0x71, 0x71);

/// Name of the bold family registered by [`install_fonts`]. Always
/// registered, so `bold()` resolves even when no bold file was found.
const BOLD_FAMILY: &str = "quickplay-bold";

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(BOLD_FAMILY.into()))
}

/// Inter, the desktop app's own UI face (`font-family: "Inter", …` in
/// `src/app.css`), embedded in the binary so the rendering is the same on
/// every platform: Regular for the body, SemiBold as its own family for
/// titles and the clock. OFL-1.1, see `assets/fonts/OFL-Inter.txt`.
///
/// A system face with wider coverage (Segoe UI, Noto, DejaVu) is appended
/// behind it when one is found, so a glyph Inter lacks — CJK, symbols —
/// still has somewhere to fall back to before egui's bundled fonts.
pub fn install_fonts(ctx: &egui::Context) {
    const INTER_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");
    const INTER_SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-SemiBold.ttf");
    const SYSTEM_FALLBACK: &[&str] = &[
        r"C:\Windows\Fonts\segoeui.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
    ];

    // Noto Sans CJK, one regional file: it carries the full ideograph set,
    // the kana and the hangul (the region only picks default glyph shapes),
    // so Japanese, Korean and Chinese titles render everywhere. 16 MB in the
    // binary, parsed lazily — the cost is deliberate.
    const NOTO_CJK: &[u8] = include_bytes!("../../assets/fonts/NotoSansCJKsc-Regular.otf");

    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "inter".into(),
        Arc::new(FontData::from_static(INTER_REGULAR)),
    );
    fonts
        .font_data
        .insert("noto-cjk".into(), Arc::new(FontData::from_static(NOTO_CJK)));
    if let Some(list) = fonts.families.get_mut(&FontFamily::Proportional) {
        list.insert(0, "noto-cjk".into());
    }
    fonts.font_data.insert(
        BOLD_FAMILY.into(),
        Arc::new(FontData::from_static(INTER_SEMIBOLD)),
    );
    // The first candidate that reads *and* parses: egui panics at start-up
    // on font data it cannot parse, so a damaged or empty file is passed
    // over for the next one, and the bundled fonts carry on without any.
    if let Some((path, bytes)) = SYSTEM_FALLBACK.iter().find_map(|p| {
        let bytes = std::fs::read(p).ok()?;
        match skrifa::FontRef::from_index(&bytes, 0) {
            Ok(_) => Some((p.to_string(), bytes)),
            Err(err) => {
                tracing::warn!(path = %p, %err, "system fallback font unreadable, skipped");
                None
            }
        }
    }) {
        tracing::debug!(%path, "system fallback font");
        fonts.font_data.insert(
            "system-fallback".into(),
            Arc::new(FontData::from_owned(bytes)),
        );
        if let Some(list) = fonts.families.get_mut(&FontFamily::Proportional) {
            list.insert(0, "system-fallback".into());
        }
    }
    if let Some(list) = fonts.families.get_mut(&FontFamily::Proportional) {
        list.insert(0, "inter".into());
    }
    let mut bold_list: Vec<String> = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    bold_list.insert(0, BOLD_FAMILY.into());
    fonts
        .families
        .insert(FontFamily::Name(BOLD_FAMILY.into()), bold_list);
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);

    let mut visuals = Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = PANEL;
    visuals.window_stroke = Stroke::new(1.0, BORDER);
    visuals.window_corner_radius = CornerRadius::same(4);
    visuals.extreme_bg_color = Color32::from_rgb(0x0c, 0x0c, 0x0c);
    visuals.faint_bg_color = PANEL;
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.hyperlink_color = ACCENT_HOVER;
    visuals.override_text_color = Some(TEXT);
    visuals.menu_corner_radius = CornerRadius::same(4);
    visuals.popup_shadow = egui::Shadow::NONE;
    visuals.window_shadow = egui::Shadow::NONE;

    let widgets = &mut visuals.widgets;
    for w in [
        &mut widgets.noninteractive,
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(3);
        w.expansion = 0.0;
    }
    widgets.noninteractive.bg_fill = PANEL;
    widgets.noninteractive.weak_bg_fill = PANEL;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_DIM);
    widgets.inactive.bg_fill = ELEVATED;
    widgets.inactive.weak_bg_fill = ELEVATED;
    widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.hovered.bg_fill = HOVER;
    widgets.hovered.weak_bg_fill = HOVER;
    widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x4a, 0x4a, 0x4a));
    widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.active.bg_fill = ACCENT_DIM;
    widgets.active.weak_bg_fill = ACCENT_DIM;
    widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    widgets.active.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.open.bg_fill = ELEVATED;
    widgets.open.weak_bg_fill = ELEVATED;
    widgets.open.bg_stroke = Stroke::new(1.0, BORDER);
    widgets.open.fg_stroke = Stroke::new(1.0, TEXT);

    ctx.set_visuals(visuals);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        style.spacing.interact_size = egui::vec2(24.0, 22.0);
        style.spacing.menu_margin = Margin::same(4);
        style.spacing.window_margin = Margin::same(6);
        style.interaction.tooltip_delay = 0.4;
        style.interaction.selectable_labels = false;
        use egui::{FontFamily::Proportional, TextStyle};
        style
            .text_styles
            .insert(TextStyle::Small, FontId::new(11.0, Proportional));
        style
            .text_styles
            .insert(TextStyle::Body, FontId::new(13.0, Proportional));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::new(13.0, Proportional));
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::new(15.0, Proportional));
    });
}

/// The frame the controls block uses.
pub fn panel_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(BG)
        .inner_margin(Margin::symmetric(10, 6))
}

/// The artwork pane's frame: edge to edge, black letterbox.
pub fn artwork_frame() -> egui::Frame {
    egui::Frame::NONE.fill(Color32::BLACK)
}

/// The custom title bar's frame.
pub fn title_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(TITLE_BG)
        .inner_margin(Margin::symmetric(8, 0))
}
