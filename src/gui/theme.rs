//! Visual styling for the injector window.
//!
//! A small dark palette plus the shared frames and text styles, kept in one
//! place so every tab looks like part of the same window.

use egui::{Color32, Context, FontId, Frame, Margin, RichText, Rounding, Stroke, TextStyle};

/// The window's color palette.
pub struct Palette;

impl Palette {
    /// Window and panel background — near-black with a slight blue cast.
    pub const BACKGROUND: Color32 = Color32::from_rgb(18, 18, 23);
    /// Raised surfaces: cards, headers, footers, tab bar.
    pub const CARD: Color32 = Color32::from_rgb(29, 30, 37);
    /// A surface one step further raised (rows, inputs).
    pub const CARD_RAISED: Color32 = Color32::from_rgb(37, 38, 47);
    /// Hairline separators and card outlines.
    pub const BORDER: Color32 = Color32::from_rgb(52, 54, 65);
    /// Primary accent, used for selection and the main action.
    pub const ACCENT: Color32 = Color32::from_rgb(92, 141, 235);
    /// A quieter accent for badges and highlights on text.
    pub const ACCENT_DIM: Color32 = Color32::from_rgb(58, 92, 165);
    pub const TEXT: Color32 = Color32::from_rgb(233, 234, 240);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(145, 147, 160);
    pub const SUCCESS: Color32 = Color32::from_rgb(110, 202, 134);
    pub const WARNING: Color32 = Color32::from_rgb(226, 176, 96);
    pub const ERROR: Color32 = Color32::from_rgb(233, 108, 116);
}

/// Corner radius shared by every raised surface.
pub const RADIUS: f32 = 8.0;

/// The rounding shared by every raised surface.
pub fn rounding() -> Rounding {
    Rounding::same(RADIUS)
}

/// Applies the dark theme and text styles to an egui context.
pub fn apply(ctx: &Context) {
    let mut style = (*ctx.style()).clone();

    style.visuals.window_fill = Palette::BACKGROUND;
    style.visuals.panel_fill = Palette::BACKGROUND;
    style.visuals.extreme_bg_color = Palette::CARD;
    style.visuals.faint_bg_color = Palette::CARD;
    style.visuals.window_stroke = Stroke::new(1.0_f32, Palette::BORDER);
    style.visuals.window_rounding = rounding();
    style.visuals.window_shadow = egui::epaint::Shadow {
        offset: egui::vec2(0.0, 2.0),
        blur: 8.0,
        spread: 0.0,
        color: Color32::from_black_alpha(96),
    };

    // Selection highlight (text selection, active tab underline).
    style.visuals.selection.bg_fill = Palette::ACCENT_DIM;
    style.visuals.selection.stroke = Stroke::new(1.0_f32, Palette::ACCENT);

    let widgets = &mut style.visuals.widgets;
    widgets.noninteractive.bg_fill = Palette::CARD_RAISED;
    widgets.noninteractive.weak_bg_fill = Palette::CARD_RAISED;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Palette::BORDER);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, Palette::TEXT);
    widgets.noninteractive.rounding = Rounding::same(6.0);

    widgets.inactive.bg_fill = Palette::CARD_RAISED;
    widgets.inactive.weak_bg_fill = Palette::CARD_RAISED;
    widgets.inactive.bg_stroke = Stroke::new(1.0_f32, Palette::BORDER);

    widgets.hovered.bg_fill = Palette::BORDER;
    widgets.hovered.weak_bg_fill = Palette::BORDER;
    widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Palette::ACCENT_DIM);

    widgets.active.bg_fill = Palette::ACCENT_DIM;
    widgets.active.weak_bg_fill = Palette::ACCENT_DIM;
    widgets.active.bg_stroke = Stroke::new(1.0_f32, Palette::ACCENT);
    widgets.active.fg_stroke = Stroke::new(1.0_f32, Palette::TEXT);

    // A little more breathing room than egui's defaults.
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.window_margin = Margin::same(14.0);
    style.spacing.scroll.bar_width = 8.0;

    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(17.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(13.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(13.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(11.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(12.0, egui::FontFamily::Monospace),
    );

    ctx.set_style(style);
}

/// The frame behind the top header bar.
pub fn header_frame() -> Frame {
    Frame {
        fill: Palette::CARD,
        inner_margin: Margin {
            left: 16.0,
            right: 16.0,
            top: 12.0,
            bottom: 10.0,
        },
        stroke: Stroke::new(1.0_f32, Palette::BORDER),
        ..Default::default()
    }
}

/// The frame behind the bottom status/action bar.
pub fn footer_frame() -> Frame {
    Frame {
        fill: Palette::CARD,
        inner_margin: Margin::same(12.0),
        stroke: Stroke::new(1.0_f32, Palette::BORDER),
        ..Default::default()
    }
}

/// A raised card, used for process and dll rows.
pub fn card(fill: Color32, accent: Option<Color32>) -> Frame {
    Frame {
        fill,
        rounding: rounding(),
        stroke: Stroke::new(1.0_f32, accent.unwrap_or(Palette::BORDER)),
        inner_margin: Margin::same(10.0),
        ..Default::default()
    }
}

/// A pill-shaped badge (pid, size, count).
pub fn badge(text: &str, color: Color32) -> RichText {
    RichText::new(format!("  {text}  "))
        .color(color)
        .monospace()
}

/// The title shown in the header bar: wordmark plus a one-line subtitle.
pub fn header(ui: &mut egui::Ui) {
    ui.label(
        RichText::new("VoidLatch")
            .heading()
            .strong()
            .color(Palette::TEXT),
    );
    ui.label(
        RichText::new("Minecraft dll injector")
            .color(Palette::TEXT_DIM)
            .size(12.5),
    );
}

/// A selectable tab button styled as a pill. Returns whether it was clicked.
///
/// The selected label is bright white on the accent fill — the accent blue is
/// too dark to read against it.
pub fn tab(ui: &mut egui::Ui, label: &str, selected: bool) -> bool {
    let color = if selected {
        Palette::TEXT
    } else {
        Palette::TEXT_DIM
    };
    let button = egui::Button::new(RichText::new(label).color(color).strong())
        .fill(if selected {
            Palette::ACCENT_DIM
        } else {
            Color32::TRANSPARENT
        })
        .stroke(Stroke::new(
            1.0_f32,
            if selected {
                Palette::ACCENT
            } else {
                Color32::TRANSPARENT
            },
        ))
        .min_size(egui::vec2(112.0, 30.0))
        .sense(egui::Sense::click());
    ui.add(button).clicked()
}

/// A section heading with a thin divider filling the remaining width.
pub fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(text.to_uppercase())
                .color(Palette::TEXT_DIM)
                .small()
                .strong(),
        );
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 0.0, Palette::BORDER);
    });
}

/// Centered hint shown when a list has nothing in it.
pub fn empty_state(ui: &mut egui::Ui, title: &str, hint: &str) {
    ui.add_space(28.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(title).color(Palette::TEXT).strong());
        ui.add_space(4.0);
        ui.label(RichText::new(hint).color(Palette::TEXT_DIM).small());
    });
}
