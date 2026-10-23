//! Identidad visual: Inter, neutros tipo Notion y un único acento teal.

use std::collections::BTreeMap;
use std::sync::Arc;

use eframe::egui;
use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke,
    Style, TextStyle, Visuals,
};

/// Teal base, para rellenos y bordes.
pub const ACCENT: Color32 = Color32::from_rgb(0x2d, 0x8b, 0x8b);

/// Columna de lectura del editor.
pub const READING_WIDTH: f32 = 700.0;

/// Alto de la barra de título propia.
pub const TITLE_HEIGHT: f32 = 38.0;

/// Ancho fijo del panel de notas.
pub const SIDEBAR_WIDTH: f32 = 280.0;

/// Duración del destello al guardar.
pub const SAVE_FLASH_MS: u64 = 180;

/// Teal legible sobre el fondo del tema actual (punto, regla y destello).
pub fn accent(visuals: &Visuals) -> Color32 {
    palette(visuals.dark_mode).accent_ink
}

/// Relleno de la fila y el tab activos: más suave que la selección de texto,
/// que necesita más contraste para leerse.
pub fn active_fill(visuals: &Visuals) -> Color32 {
    ACCENT.gamma_multiply(palette(visuals.dark_mode).active_alpha)
}

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

/// Los ocho tonos que definen cada tema. Un solo hue (teal) como acento.
struct Palette {
    canvas: Color32,
    chrome: Color32,
    window: Color32,
    field: Color32,
    hover: Color32,
    pressed: Color32,
    hairline: Color32,
    text: Color32,
    weak: Color32,
    /// Riel y manija de las barras de scroll.
    rail: Color32,
    accent_ink: Color32,
    /// Opacidad del teal en la selección de texto.
    selection_alpha: f32,
    /// Opacidad del teal en fila y tab activos.
    active_alpha: f32,
}

fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            canvas: Color32::from_rgb(0x19, 0x19, 0x19),
            chrome: Color32::from_rgb(0x20, 0x20, 0x20),
            window: Color32::from_rgb(0x24, 0x24, 0x24),
            field: Color32::from_rgb(0x25, 0x25, 0x25),
            hover: Color32::from_rgb(0x2b, 0x2b, 0x2b),
            pressed: Color32::from_rgb(0x33, 0x33, 0x33),
            hairline: Color32::from_rgb(0x2e, 0x2e, 0x2e),
            text: Color32::from_rgb(0xec, 0xec, 0xec),
            weak: Color32::from_rgb(0x9c, 0x9c, 0x9c),
            rail: Color32::from_rgb(0x3a, 0x3a, 0x3a),
            accent_ink: Color32::from_rgb(0x5f, 0xc3, 0xc3),
            selection_alpha: 0.3,
            active_alpha: 0.2,
        }
    } else {
        Palette {
            canvas: Color32::WHITE,
            chrome: Color32::from_rgb(0xf7, 0xf7, 0xf5),
            window: Color32::WHITE,
            field: Color32::from_rgb(0xf1, 0xf1, 0xee),
            hover: Color32::from_rgb(0xed, 0xed, 0xea),
            pressed: Color32::from_rgb(0xe5, 0xe5, 0xe1),
            hairline: Color32::from_rgb(0xe4, 0xe4, 0xe0),
            text: Color32::from_rgb(0x1d, 0x1d, 0x1b),
            weak: Color32::from_rgb(0x6b, 0x6b, 0x67),
            rail: Color32::from_rgb(0xd7, 0xd7, 0xd2),
            accent_ink: Color32::from_rgb(0x14, 0x70, 0x70),
            selection_alpha: 0.22,
            active_alpha: 0.16,
        }
    }
}

fn style(mut visuals: Visuals) -> Style {
    let p = palette(visuals.dark_mode);

    visuals.panel_fill = p.canvas;
    visuals.window_fill = p.window;
    visuals.faint_bg_color = p.chrome;
    visuals.extreme_bg_color = p.field;
    visuals.text_edit_bg_color = Some(p.field);
    visuals.code_bg_color = p.field;
    visuals.weak_text_color = Some(p.weak);
    visuals.window_stroke = Stroke::new(1.0, p.hairline);
    visuals.window_corner_radius = CornerRadius::same(8);
    visuals.menu_corner_radius = CornerRadius::same(8);
    visuals.window_shadow = Shadow::NONE;
    visuals.popup_shadow = Shadow::NONE;
    visuals.hyperlink_color = p.accent_ink;
    visuals.selection.bg_fill = ACCENT.gamma_multiply(p.selection_alpha);
    // `selection.stroke` pinta el texto seleccionado y el borde del editor con
    // foco: un teal claro en oscuro y oscuro en claro mantiene ambos legibles.
    visuals.selection.stroke = Stroke::new(1.0, p.accent_ink);
    visuals.text_cursor.stroke = Stroke::new(2.0, p.accent_ink);

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
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.hairline);
    widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
    widgets.inactive.bg_stroke = Stroke::new(1.0, p.hairline);
    widgets.hovered.bg_stroke = Stroke::new(1.0, p.hairline);
    widgets.active.bg_stroke = Stroke::new(1.0, p.hairline);
    widgets.open.bg_stroke = Stroke::new(1.0, p.hairline);

    widgets.inactive.weak_bg_fill = p.field;
    widgets.hovered.weak_bg_fill = p.hover;
    widgets.active.weak_bg_fill = p.pressed;
    widgets.open.weak_bg_fill = p.field;

    // Ese `bg_fill` lo usan las manijas de scroll y los radios: un tono callado
    // pero visible, no el gris por defecto de egui.
    widgets.noninteractive.bg_fill = p.rail;
    widgets.inactive.bg_fill = p.rail;
    widgets.hovered.bg_fill = p.weak;
    widgets.active.bg_fill = p.weak;
    widgets.open.bg_fill = p.rail;

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

#[cfg(test)]
mod tests {
    use super::*;

    /// Contraste WCAG de dos colores opacos.
    fn contrast(a: Color32, b: Color32) -> f32 {
        fn luminance(color: Color32) -> f32 {
            let channel = |value: u8| {
                let value = value as f32 / 255.0;
                if value <= 0.03928 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            };
            let [r, g, b, _] = color.to_array();
            0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
        }
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn ambos_temas_son_legibles() {
        for dark in [false, true] {
            let p = palette(dark);
            assert!(contrast(p.text, p.canvas) >= 12.0, "texto sobre lienzo");
            assert!(contrast(p.text, p.chrome) >= 12.0, "texto sobre chrome");
            assert!(contrast(p.weak, p.chrome) >= 4.5, "texto tenue");
            assert!(contrast(p.weak, p.canvas) >= 4.5, "texto tenue en lienzo");
            assert!(contrast(p.accent_ink, p.canvas) >= 4.5, "acento");
            assert!(contrast(p.accent_ink, p.field) >= 4.5, "acento en campo");
        }
    }
}
