//! Widgets dibujados a mano: íconos, botones y formato de texto.

use eframe::egui;

use crate::style;

/// Botón con el color de acento: la acción principal de un modal.
pub(crate) fn accent_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.scope(|ui| {
        let widgets = &mut ui.visuals_mut().widgets;
        let fg = egui::Color32::from_rgb(0xf7, 0xfb, 0xfb);
        widgets.inactive.weak_bg_fill = style::ACCENT;
        widgets.hovered.weak_bg_fill = style::ACCENT.gamma_multiply(1.15);
        widgets.active.weak_bg_fill = style::ACCENT.gamma_multiply(0.9);
        for widget in [
            &mut widgets.inactive,
            &mut widgets.hovered,
            &mut widgets.active,
        ] {
            widget.fg_stroke = egui::Stroke::new(1.0, fg);
            widget.bg_stroke = egui::Stroke::NONE;
        }
        ui.button(text)
    })
    .inner
}

/// Texto con el punto de «sin guardar» al final, truncable como un solo bloque.
pub(crate) fn title_job(
    text: &str,
    color: egui::Color32,
    size: f32,
    dot: Option<egui::Color32>,
) -> egui::text::LayoutJob {
    let format = egui::TextFormat {
        font_id: egui::FontId::proportional(size),
        color,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    job.append(text, 0.0, format.clone());
    if let Some(dot) = dot {
        job.append(
            "  •",
            0.0,
            egui::TextFormat {
                color: dot,
                ..format
            },
        );
    }
    job
}

/// Primeras dos líneas del cuerpo, con espacios colapsados, recortadas para la fila.
pub(crate) fn snippet(body: &str) -> String {
    let text = body
        .lines()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if text.chars().count() > 90 {
        let mut short: String = text.chars().take(90).collect();
        short.push('…');
        short
    } else {
        text
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Icon {
    Menu,
    Plus,
    More,
    Close,
    Back,
    Trash,
    Minimize,
    Maximize,
    Restore,
}

/// Botón cuadrado con ícono dibujado (la fuente por defecto no tiene estos glifos).
pub(crate) fn icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    size: f32,
    tooltip: &str,
    danger: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let hovered = response.hovered();
    let color = if danger && hovered {
        egui::Color32::WHITE
    } else if hovered {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    if hovered {
        let fill = if danger {
            egui::Color32::from_rgb(190, 60, 55)
        } else {
            ui.visuals().widgets.hovered.weak_bg_fill
        };
        ui.painter().rect_filled(rect, 4.0, fill);
    }
    paint_icon(ui.painter(), icon, rect, color);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tooltip));
    response.on_hover_text(tooltip)
}

/// Botón de ancho completo con ícono a la izquierda y texto alineado.
/// `label` es el nombre accesible cuando el texto visible no describe la acción.
pub(crate) fn nav_button(ui: &mut egui::Ui, icon: Icon, text: &str, label: &str) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 6.0, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 14.0, rect.center().y),
        egui::vec2(20.0, 20.0),
    );
    paint_icon(
        ui.painter(),
        icon,
        icon_rect,
        ui.visuals().weak_text_color(),
    );
    ui.painter().text(
        egui::pos2(rect.left() + 28.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        egui::TextStyle::Button.resolve(ui.style()),
        ui.visuals().text_color(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    response.on_hover_text(label)
}

pub(crate) fn paint_icon(
    painter: &egui::Painter,
    icon: Icon,
    rect: egui::Rect,
    color: egui::Color32,
) {
    let stroke = egui::Stroke::new(1.5, color);
    let center = rect.center();
    let s = rect.width();

    match icon {
        Icon::Menu => {
            for i in -1..=1 {
                let y = center.y + i as f32 * s * 0.16;
                painter.line_segment(
                    [
                        egui::pos2(center.x - s * 0.22, y),
                        egui::pos2(center.x + s * 0.22, y),
                    ],
                    stroke,
                );
            }
        }
        Icon::Close => {
            let r = s * 0.22;
            painter.line_segment(
                [center + egui::vec2(-r, -r), center + egui::vec2(r, r)],
                stroke,
            );
            painter.line_segment(
                [center + egui::vec2(r, -r), center + egui::vec2(-r, r)],
                stroke,
            );
        }
        Icon::Back => {
            let r = s * 0.18;
            painter.line_segment(
                [center + egui::vec2(r, -r), center + egui::vec2(-r, 0.0)],
                stroke,
            );
            painter.line_segment(
                [center + egui::vec2(-r, 0.0), center + egui::vec2(r, r)],
                stroke,
            );
        }
        Icon::Trash => {
            let r = s * 0.18;
            painter.line_segment(
                [
                    center + egui::vec2(-r * 1.3, -s * 0.16),
                    center + egui::vec2(r * 1.3, -s * 0.16),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    center + egui::vec2(-r * 0.45, -s * 0.22),
                    center + egui::vec2(r * 0.45, -s * 0.22),
                ],
                stroke,
            );
            let body = egui::Rect::from_min_max(
                egui::pos2(center.x - r, center.y - s * 0.09),
                egui::pos2(center.x + r, center.y + s * 0.22),
            );
            painter.rect_stroke(body, 1, stroke, egui::StrokeKind::Inside);
            for x in [-0.5, 0.5] {
                painter.line_segment(
                    [
                        egui::pos2(center.x + x * r, center.y - s * 0.02),
                        egui::pos2(center.x + x * r, center.y + s * 0.16),
                    ],
                    stroke,
                );
            }
        }
        Icon::Minimize => {
            painter.line_segment(
                [
                    center + egui::vec2(-s * 0.22, s * 0.06),
                    center + egui::vec2(s * 0.22, s * 0.06),
                ],
                stroke,
            );
        }
        Icon::Maximize => {
            painter.rect_stroke(
                egui::Rect::from_center_size(center, egui::vec2(s * 0.42, s * 0.42)),
                1,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        Icon::Restore => {
            let size = egui::vec2(s * 0.34, s * 0.34);
            painter.rect_stroke(
                egui::Rect::from_center_size(center + egui::vec2(s * 0.09, s * 0.09), size),
                1,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.rect_stroke(
                egui::Rect::from_center_size(center - egui::vec2(s * 0.09, s * 0.09), size),
                1,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        Icon::Plus => {
            painter.line_segment(
                [
                    center + egui::vec2(-s * 0.18, 0.0),
                    center + egui::vec2(s * 0.18, 0.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    center + egui::vec2(0.0, -s * 0.18),
                    center + egui::vec2(0.0, s * 0.18),
                ],
                stroke,
            );
        }
        Icon::More => {
            for i in -1..=1 {
                painter.circle_filled(
                    center + egui::vec2(0.0, i as f32 * s * 0.19),
                    s * 0.075,
                    color,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_junta_dos_lineas_y_colapsa_espacios() {
        assert_eq!(snippet("uno\n  dos   tres\ncuatro"), "uno dos tres");
    }

    #[test]
    fn snippet_vacio_no_falla() {
        assert_eq!(snippet(""), "");
    }

    #[test]
    fn snippet_recorta_a_90_con_puntos_suspensivos() {
        let largo = "a".repeat(200);
        let corto = snippet(&largo);
        assert_eq!(corto.chars().count(), 91);
        assert!(corto.ends_with('…'));
    }

    #[test]
    fn title_job_agrega_el_punto_solo_si_hay_color() {
        let color = egui::Color32::WHITE;
        assert_eq!(title_job("hola", color, 12.0, None).text, "hola");
        assert_eq!(title_job("hola", color, 12.0, Some(color)).text, "hola  •");
    }
}
