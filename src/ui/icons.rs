use crate::ui::theme::EarthPalette;
use egui::{pos2, vec2, Color32, CornerRadius, Rect, Sense, Stroke, Ui};

pub fn draw_earth_emblem(ui: &mut Ui, size: f32, palette: &EarthPalette, paused: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let center = rect.center();
        let r = size * 0.36;
        let fill = if paused {
            palette.text_muted
        } else {
            palette.accent
        };
        p.circle_filled(center, r, fill);
        p.circle_stroke(center, r, Stroke::new(1.0, palette.accent_secondary));

        // Meridian arc inside globe
        p.line_segment(
            [pos2(center.x, center.y - r * 0.85), pos2(center.x, center.y + r * 0.85)],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 160)),
        );
        p.line_segment(
            [pos2(center.x - r * 0.85, center.y), pos2(center.x + r * 0.85, center.y)],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 140)),
        );

        // Tilted orbital ring
        let ring_color = if paused {
            palette.text_secondary
        } else {
            Color32::from_rgb(236, 253, 245)
        };
        let pts: Vec<egui::Pos2> = (0..=20)
            .map(|i| {
                let t = -0.2 + (i as f32 / 20.0) * std::f32::consts::PI * 1.4;
                let rx = t.cos() * size * 0.48;
                let ry = t.sin() * size * 0.17;
                let angle = -0.38f32;
                let x = center.x + rx * angle.cos() - ry * angle.sin();
                let y = center.y + rx * angle.sin() + ry * angle.cos();
                pos2(x, y)
            })
            .collect();
        p.add(egui::Shape::line(pts, Stroke::new(1.5, ring_color)));
    }
    response
}

pub fn icon_button(
    ui: &mut Ui,
    size: f32,
    active: bool,
    tooltip: &str,
    palette: &EarthPalette,
    draw_fn: impl FnOnce(&egui::Painter, Rect, Color32),
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if active {
            p.rect_filled(rect, CornerRadius::same(7), palette.accent_soft);
            p.rect_stroke(
                rect,
                CornerRadius::same(7),
                Stroke::new(1.0, palette.accent),
                egui::StrokeKind::Inside,
            );
        } else if response.hovered() {
            p.rect_filled(rect, CornerRadius::same(7), palette.row_hover_bg);
        }
        let color = if active {
            palette.accent
        } else if response.hovered() {
            palette.text_primary
        } else {
            palette.text_secondary
        };
        draw_fn(p, rect.shrink(size * 0.24), color);
    }
    response.on_hover_text(tooltip)
}

pub fn draw_split_pane_glyph(p: &egui::Painter, r: Rect, color: Color32) {
    p.rect_stroke(r, CornerRadius::same(2), Stroke::new(1.4, color), egui::StrokeKind::Inside);
    let split_x = r.left() + r.width() * 0.44;
    p.line_segment([pos2(split_x, r.top()), pos2(split_x, r.bottom())], Stroke::new(1.4, color));
}

pub fn draw_gear_glyph(p: &egui::Painter, r: Rect, color: Color32) {
    let c = r.center();
    let rad = r.width().min(r.height()) * 0.36;
    p.circle_stroke(c, rad, Stroke::new(1.5, color));
    p.circle_filled(c, rad * 0.34, color);
    for i in 0..6 {
        let a = i as f32 * std::f32::consts::FRAC_PI_3;
        let p1 = pos2(c.x + a.cos() * rad * 0.9, c.y + a.sin() * rad * 0.9);
        let p2 = pos2(c.x + a.cos() * rad * 1.45, c.y + a.sin() * rad * 1.45);
        p.line_segment([p1, p2], Stroke::new(1.6, color));
    }
}

pub fn draw_pause_glyph(p: &egui::Painter, r: Rect, color: Color32) {
    let w = r.width() * 0.26;
    let left_bar = Rect::from_min_max(pos2(r.left() + r.width() * 0.18, r.top()), pos2(r.left() + r.width() * 0.18 + w, r.bottom()));
    let right_bar = Rect::from_min_max(pos2(r.right() - r.width() * 0.18 - w, r.top()), pos2(r.right() - r.width() * 0.18, r.bottom()));
    p.rect_filled(left_bar, CornerRadius::same(1), color);
    p.rect_filled(right_bar, CornerRadius::same(1), color);
}

pub fn draw_play_glyph(p: &egui::Painter, r: Rect, color: Color32) {
    let pts = vec![
        pos2(r.left() + r.width() * 0.2, r.top()),
        pos2(r.right() - r.width() * 0.1, r.center().y),
        pos2(r.left() + r.width() * 0.2, r.bottom()),
    ];
    p.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
}

pub fn draw_shield_glyph(p: &egui::Painter, r: Rect, color: Color32) {
    let pts = vec![
        pos2(r.center().x, r.top()),
        pos2(r.right(), r.top() + r.height() * 0.24),
        pos2(r.right() - r.width() * 0.12, r.center().y + r.height() * 0.15),
        pos2(r.center().x, r.bottom()),
        pos2(r.left() + r.width() * 0.12, r.center().y + r.height() * 0.15),
        pos2(r.left(), r.top() + r.height() * 0.24),
    ];
    p.add(egui::Shape::closed_line(pts, Stroke::new(1.4, color)));
}

pub fn draw_search_glyph(p: &egui::Painter, r: Rect, color: Color32) {
    let rad = r.width().min(r.height()) * 0.34;
    let c = pos2(r.left() + rad + 1.0, r.top() + rad + 1.0);
    p.circle_stroke(c, rad, Stroke::new(1.5, color));
    let dir = std::f32::consts::FRAC_PI_4;
    p.line_segment(
        [
            pos2(c.x + dir.cos() * rad, c.y + dir.sin() * rad),
            pos2(r.right() - 1.0, r.bottom() - 1.0),
        ],
        Stroke::new(1.7, color),
    );
}

pub fn draw_pin_badge(p: &egui::Painter, r: Rect, color: Color32) {
    let c = r.center();
    p.circle_filled(pos2(c.x, c.y - r.height() * 0.18), r.width() * 0.24, color);
    p.line_segment(
        [pos2(c.x, c.y + r.height() * 0.05), pos2(c.x, r.bottom() - 1.0)],
        Stroke::new(1.6, color),
    );
}

pub fn draw_file_badge(p: &egui::Painter, r: Rect, color: Color32) {
    let fold = r.width() * 0.3;
    let pts = vec![
        r.left_top(),
        pos2(r.right() - fold, r.top()),
        pos2(r.right(), r.top() + fold),
        r.right_bottom(),
        r.left_bottom(),
    ];
    p.add(egui::Shape::closed_line(pts, Stroke::new(1.4, color)));
    p.line_segment(
        [pos2(r.right() - fold, r.top()), pos2(r.right() - fold, r.top() + fold)],
        Stroke::new(1.2, color),
    );
    p.line_segment(
        [pos2(r.right() - fold, r.top() + fold), pos2(r.right(), r.top() + fold)],
        Stroke::new(1.2, color),
    );
}

pub fn draw_link_badge(p: &egui::Painter, r: Rect, color: Color32) {
    let left_box = Rect::from_min_max(
        pos2(r.left(), r.center().y - r.height() * 0.25),
        pos2(r.center().x + 1.5, r.center().y + r.height() * 0.25),
    );
    let right_box = Rect::from_min_max(
        pos2(r.center().x - 1.5, r.center().y - r.height() * 0.25),
        pos2(r.right(), r.center().y + r.height() * 0.25),
    );
    p.rect_stroke(left_box, CornerRadius::same(3), Stroke::new(1.3, color), egui::StrokeKind::Inside);
    p.rect_stroke(right_box, CornerRadius::same(3), Stroke::new(1.3, color), egui::StrokeKind::Inside);
}
