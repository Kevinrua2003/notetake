//! Identidad visual: Inter, neutros tipo Notion y un único acento teal.

use std::collections::BTreeMap;
use std::sync::Arc;

use eframe::egui;
use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke,
    Style, TextStyle, Visuals,
};

/// Acento único de la UI.
pub const ACCENT: Color32 = Color32::from_rgb(0x2d, 0x8b, 0x8b);

/// Columna de lectura del editor.
pub const READING_WIDTH: f32 = 700.0;

/// Alto de la barra de título propia.
pub const TITLE_HEIGHT: f32 = 38.0;

/// Ancho fijo del panel de notas.
pub const SIDEBAR_WIDTH: f32 = 280.0;

/// Duración del destello al guardar.
pub const SAVE_FLASH_MS: u64 = 180;

/// Instala fuentes y estilo para los dos temas.
pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_style_of(egui::Theme::Light, style(Visuals::light()));
    ctx.set_style_of(egui::Theme::Dark, style(Visuals::dark()));
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "inter".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/InterVariable.ttf"
        ))),
    );
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .expect("egui trae familia proporcional")
        .insert(0, "inter".to_owned());
    ctx.set_fonts(fonts);
}

fn style(mut visuals: Visuals) -> Style {
    let dark = visuals.dark_mode;
    let (canvas, chrome, field, hairline, text, weak) = if dark {
        (
            Color32::from_rgb(0x19, 0x19, 0x19),
            Color32::from_rgb(0x23, 0x23, 0x23),
            Color32::from_rgb(0x2a, 0x2a, 0x2a),
            Color32::from_rgb(0x2f, 0x2f, 0x2f),
            Color32::from_rgb(0xed, 0xed, 0xed),
            Color32::from_rgb(0x9b, 0x9b, 0x9b),
        )
    } else {
        (
            Color32::WHITE,
            Color32::from_rgb(0xf7, 0xf7, 0xf5),
            Color32::from_rgb(0xf0, 0xf0, 0xee),
            Color32::from_rgb(0xe6, 0xe6, 0xe4),
            Color32::from_rgb(0x1f, 0x1f, 0x1f),
            Color32::from_rgb(0x6b, 0x6b, 0x6b),
        )
    };

    visuals.panel_fill = canvas;
    visuals.window_fill = if dark { chrome } else { Color32::WHITE };
    visuals.faint_bg_color = chrome;
    visuals.extreme_bg_color = field;
    visuals.text_edit_bg_color = Some(field);
    visuals.code_bg_color = field;
    visuals.weak_text_color = Some(weak);
    visuals.window_stroke = Stroke::new(1.0, hairline);
    visuals.window_corner_radius = CornerRadius::same(8);
    visuals.menu_corner_radius = CornerRadius::same(8);
    visuals.window_shadow = Shadow::NONE;
    visuals.popup_shadow = Shadow::NONE;
    visuals.hyperlink_color = ACCENT;
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.22);
    // `selection.stroke` pinta el texto seleccionado y el borde del editor con
    // foco: un teal claro en oscuro y oscuro en claro mantiene ambos legibles.
    visuals.selection.stroke = Stroke::new(
        1.0,
        if dark {
            Color32::from_rgb(0x8f, 0xd6, 0xd6)
        } else {
            Color32::from_rgb(0x17, 0x6b, 0x6b)
        },
    );
    visuals.text_cursor.stroke = Stroke::new(2.0, ACCENT);

    let widgets = &mut visuals.widgets;
    for widget in [
        &mut widgets.noninteractive,
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(6);
    }
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, text);
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, hairline);
    widgets.inactive.fg_stroke = Stroke::new(1.0, text);
    widgets.inactive.bg_stroke = Stroke::new(1.0, hairline);
    widgets.hovered.bg_stroke = Stroke::new(1.0, hairline);
    widgets.active.bg_stroke = Stroke::new(1.0, hairline);
    let (rest, hover, active) = if dark {
        (
            Color32::from_rgb(0x26, 0x26, 0x26),
            Color32::from_rgb(0x30, 0x30, 0x30),
            Color32::from_rgb(0x38, 0x38, 0x38),
        )
    } else {
        (
            Color32::from_rgb(0xf2, 0xf2, 0xf0),
            Color32::from_rgb(0xeb, 0xeb, 0xe8),
            Color32::from_rgb(0xe4, 0xe4, 0xe1),
        )
    };
    widgets.inactive.weak_bg_fill = rest;
    widgets.hovered.weak_bg_fill = hover;
    widgets.active.weak_bg_fill = active;

    let mut style = Style {
        visuals,
        ..Default::default()
    };
    style.text_styles = BTreeMap::from([
        (
            TextStyle::Heading,
            FontId::new(19.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(13.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Small,
            FontId::new(11.5, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(12.5, FontFamily::Monospace),
        ),
        (
            TextStyle::Name("editor".into()),
            FontId::new(15.0, FontFamily::Proportional),
        ),
    ]);
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(8.0, 3.0);
    style.spacing.menu_margin = Margin::same(6);
    style.spacing.window_margin = Margin::same(10);
    style.spacing.interact_size.y = 24.0;
    style.spacing.extra_text_line_spacing = 4.0;
    style.spacing.scroll = scroll();
    style.animation_time = 0.12;
    style
}

fn scroll() -> egui::style::ScrollStyle {
    let mut scroll = egui::style::ScrollStyle::solid();
    scroll.bar_width = 8.0;
    scroll.floating_width = 4.0;
    scroll.bar_inner_margin = 4.0;
    scroll
}
