//! Геройский стиль: ночная синева, красно-золотые акценты, контуры с жёсткими тенями,
//! полутон, реплики в облачках, вспышки и кольцевые шкалы.
//! Цвета приглушены, чтобы окно не слепило. Только общие мотивы: чужих имён, логотипов
//! и персонажей здесь быть не должно.

use std::f32::consts::{FRAC_PI_2, TAU};
use std::sync::Arc;

use eframe::egui::epaint::{Mesh, TextShape, Vertex};
use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, ColorImage, CornerRadius, CursorIcon, FontData,
    FontDefinitions, FontFamily, FontId, Frame, Layout, Margin, Painter, Pos2, Rect, Response,
    Sense, Shadow, Shape, Stroke, StrokeKind, TextStyle, TextureHandle, TextureOptions, Ui, Vec2,
};

// Палитра
pub const PAPER: Color32 = Color32::from_rgb(15, 23, 41);
pub const HEADER: Color32 = Color32::from_rgb(74, 15, 22);
pub const CARD: Color32 = Color32::from_rgb(24, 35, 59);
pub const BUBBLE: Color32 = Color32::from_rgb(36, 50, 79);
const HOVER: Color32 = Color32::from_rgb(49, 67, 106);
pub const INK: Color32 = Color32::from_rgb(7, 11, 20);
pub const TEXT: Color32 = Color32::from_rgb(237, 231, 218);
pub const TEXT_DIM: Color32 = Color32::from_rgb(174, 184, 203);
pub const GOLD: Color32 = Color32::from_rgb(212, 166, 58);
const LINE: Color32 = Color32::from_rgb(142, 111, 42);
pub const RED: Color32 = Color32::from_rgb(179, 38, 46);
const SHADOW: Color32 = Color32::from_rgb(122, 27, 34);
pub const BLUE: Color32 = Color32::from_rgb(92, 158, 224);
pub const CYAN: Color32 = Color32::from_rgb(98, 199, 220);

const OUTLINE: f32 = 2.5;
const DROP: f32 = 5.0;
const HALFTONE_TILE: usize = 24;
const HISTORY_HEIGHT: f32 = 112.0;

const TITLE_FONT: &str = "comic-title";
const BOLD_FONT: &str = "comic-bold";

/// Заголовки: Impact, у него есть кириллица.
pub fn title(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(TITLE_FONT.into()))
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(BOLD_FONT.into()))
}

fn load_font(fonts: &mut FontDefinitions, name: &str, file: &str) -> bool {
    match std::fs::read(format!(r"C:\Windows\Fonts\{file}")) {
        Ok(bytes) => {
            fonts.font_data.insert(name.to_owned(), Arc::new(FontData::from_owned(bytes)));
            true
        }
        Err(_) => false,
    }
}

/// Шрифты берём из Windows. Если файла нет, остаётся шрифт egui и окно всё равно откроется.
pub fn apply(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let fallback = fonts.families[&FontFamily::Proportional].clone();
    for (name, file) in [(TITLE_FONT, "impact.ttf"), (BOLD_FONT, "comicbd.ttf")] {
        let mut family = fallback.clone();
        if load_font(&mut fonts, name, file) {
            family.insert(0, name.to_owned());
        }
        fonts.families.insert(FontFamily::Name(name.into()), family);
    }
    if load_font(&mut fonts, "comic-body", "comic.ttf") {
        if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
            family.insert(0, "comic-body".to_owned());
        }
    }
    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();
    let mut v = egui::Visuals::dark();
    let line = Stroke::new(2.0, LINE);
    v.panel_fill = PAPER;
    v.window_fill = CARD;
    v.extreme_bg_color = INK; // поля ввода
    v.override_text_color = Some(TEXT);
    v.hyperlink_color = GOLD;
    v.selection.bg_fill = GOLD;
    v.selection.stroke = Stroke::new(2.0, INK);
    v.slider_trailing_fill = true;
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.bg_stroke = line;
        w.fg_stroke = Stroke::new(2.0, TEXT);
        w.corner_radius = CornerRadius::ZERO;
        w.expansion = 0.0;
    }
    v.widgets.inactive.bg_fill = BUBBLE;
    v.widgets.inactive.weak_bg_fill = BUBBLE;
    v.widgets.hovered.bg_fill = HOVER;
    v.widgets.hovered.weak_bg_fill = HOVER;
    v.widgets.active.bg_fill = HOVER;
    v.widgets.active.weak_bg_fill = HOVER;
    style.visuals = v;
    style.text_styles.insert(TextStyle::Body, FontId::proportional(15.0));
    style.text_styles.insert(TextStyle::Button, FontId::proportional(15.0));
    style.spacing.item_spacing = vec2(10.0, 10.0);
    style.spacing.button_padding = vec2(10.0, 6.0);
    style.spacing.slider_width = 260.0;
    style.spacing.slider_rail_height = 12.0;
    style.spacing.interact_size.y = 30.0;
    ctx.set_style(style);
}

/// Цвет текста на заливке: тёмный на светлом, светлый на тёмном.
pub fn text_on(fill: Color32) -> Color32 {
    let brightness = 0.299 * fill.r() as f32 + 0.587 * fill.g() as f32 + 0.114 * fill.b() as f32;
    if brightness > 110.0 { INK } else { TEXT }
}

fn lighten(c: Color32) -> Color32 {
    let up = |v: u8| v + (255 - v) / 6;
    Color32::from_rgb(up(c.r()), up(c.g()), up(c.b()))
}

/// Цвет нагрузки: голубой с запасом, золотой у предела, красный за пределом.
pub fn load_color(value: f32, limit: f32) -> Color32 {
    if value >= limit {
        RED
    } else if value >= limit - 15.0 {
        GOLD
    } else {
        CYAN
    }
}

/// Панель: контур с жёсткой красной тенью.
pub fn panel<R>(ui: &mut Ui, fill: Color32, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(OUTLINE, LINE))
        .inner_margin(Margin::same(14))
        .outer_margin(Margin { left: 0, top: 0, right: 7, bottom: 7 })
        .shadow(Shadow { offset: [6, 6], blur: 0, spread: 0, color: SHADOW })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            // Колонки egui растягивают текст на всю ширину строки: здесь нужен обычный левый край.
            ui.with_layout(Layout::top_down(Align::Min), add).inner
        })
        .inner
}

/// Плашка-заголовок панели: скошенный красный ярлык.
pub fn caption(ui: &mut Ui, text: &str) {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), title(22.0), TEXT);
    let size = vec2(galley.size().x + 34.0, galley.size().y + 8.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let slant = 10.0;
    let points = vec![
        rect.left_bottom(),
        pos2(rect.left() + slant, rect.top()),
        rect.right_top(),
        pos2(rect.right() - slant, rect.bottom()),
    ];
    ui.painter().add(Shape::convex_polygon(points, RED, Stroke::NONE));
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, TEXT);
}

pub fn button(ui: &mut Ui, text: &str, fill: Color32) -> Response {
    let color = text_on(fill);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), title(19.0), color);
    let size = vec2(galley.size().x + 32.0, (galley.size().y + 16.0).max(44.0));
    let (rect, response) = ui.allocate_exact_size(size + Vec2::splat(DROP), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let face = Rect::from_min_size(rect.min, size);
        painter.rect_filled(face.translate(Vec2::splat(DROP)), CornerRadius::ZERO, SHADOW);
        let pressed = response.is_pointer_button_down_on();
        let face = if pressed { face.translate(Vec2::splat(DROP - 1.0)) } else { face };
        let shade = if response.hovered() { lighten(fill) } else { fill };
        painter.rect(face, CornerRadius::ZERO, shade, Stroke::new(OUTLINE, LINE), StrokeKind::Inside);
        if response.has_focus() {
            painter.rect_stroke(face.expand(2.0), CornerRadius::ZERO, Stroke::new(2.0, BLUE), StrokeKind::Outside);
        }
        painter.galley(face.center() - galley.size() / 2.0, galley, color);
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// Большой выключатель. Состояние написано словами, а не только цветом.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(vec2(112.0, 40.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let fill = if *on { CYAN } else { RED };
        painter.rect(rect, CornerRadius::ZERO, fill, Stroke::new(OUTLINE, LINE), StrokeKind::Inside);
        let slide = ui.ctx().animate_bool_with_time(response.id, *on, 0.18);
        let knob = Rect::from_min_size(rect.min + vec2((rect.width() - 30.0) * slide, 0.0), vec2(30.0, rect.height()));
        painter.rect(knob, CornerRadius::ZERO, TEXT, Stroke::new(OUTLINE, LINE), StrokeKind::Inside);
        let (word, anchor) = if *on {
            ("ВКЛ", rect.left_center() + vec2(38.0, 0.0))
        } else {
            ("ВЫКЛ", rect.right_center() - vec2(38.0, 0.0))
        };
        painter.text(anchor, Align2::CENTER_CENTER, word, title(19.0), text_on(fill));
        if response.has_focus() {
            painter.rect_stroke(rect.expand(2.0), CornerRadius::ZERO, Stroke::new(2.0, BLUE), StrokeKind::Outside);
        }
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}

#[derive(Clone, Copy, PartialEq)]
pub enum Chip {
    /// Можно убрать крестиком.
    Removable,
    /// Защищено навсегда: тёмная метка без крестика.
    Locked,
    /// Только показ, красная метка.
    Alert,
}

/// Метка списка. Возвращает true, если нажат крестик.
pub fn chip(ui: &mut Ui, text: &str, kind: Chip, note: &str) -> bool {
    let (fill, color) = match kind {
        Chip::Removable => (BUBBLE, TEXT),
        Chip::Locked => (INK, GOLD),
        Chip::Alert => (RED, TEXT),
    };
    let close = if kind == Chip::Removable { 26.0 } else { 0.0 };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), bold(14.0), color);
    let size = vec2(galley.size().x + 20.0 + close, 30.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let mut removed = false;
    if ui.is_rect_visible(rect) {
        let painter = ui.painter().clone();
        painter.rect(rect, CornerRadius::same(4), fill, Stroke::new(2.0, LINE), StrokeKind::Inside);
        painter.galley(pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0), galley, color);
        if kind == Chip::Removable {
            let cross = Rect::from_min_max(pos2(rect.right() - close, rect.top()), rect.right_bottom());
            let hit = ui
                .interact(cross, ui.id().with(("chip", text)), Sense::click())
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text(format!("Убрать {text}"));
            if hit.hovered() {
                painter.rect_filled(cross.shrink(2.0), CornerRadius::same(3), RED);
            }
            painter.text(cross.center(), Align2::CENTER_CENTER, "×", title(19.0), TEXT);
            removed = hit.clicked();
        }
    }
    if !note.is_empty() {
        response.on_hover_text(note);
    }
    removed
}

/// Рисованные буквы: заливка с обводкой со всех сторон.
pub fn lettering(painter: &Painter, pos: Pos2, text: &str, font: FontId, fill: Color32, outline: Color32, width: f32, angle: f32) {
    let galley = painter.layout_no_wrap(text.to_owned(), font, fill);
    for (dx, dy) in [(-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (-1.0, 0.0), (1.0, 0.0), (-1.0, 1.0), (0.0, 1.0), (1.0, 1.0)] {
        let at = pos + vec2(dx, dy) * width;
        painter.add(TextShape::new(at, galley.clone(), outline).with_override_text_color(outline).with_angle(angle));
    }
    painter.add(TextShape::new(pos, galley, fill).with_angle(angle));
}

/// Вспышка: звезда с неровными лучами, как нарисованная от руки.
pub fn burst(painter: &Painter, center: Pos2, radius: Vec2, rays: usize, fill: Color32) {
    let count = rays * 2;
    let ring: Vec<Pos2> = (0..count)
        .map(|i| {
            let angle = TAU * i as f32 / count as f32 - FRAC_PI_2;
            let scale = if i % 2 == 0 { 1.0 + 0.14 * ((i * 37 % 7) as f32 / 7.0) } else { 0.7 };
            center + vec2(angle.cos() * radius.x, angle.sin() * radius.y) * scale
        })
        .collect();
    let star = |offset: Vec2, color: Color32| {
        let mut mesh = Mesh::default();
        mesh.colored_vertex(center + offset, color);
        for point in &ring {
            mesh.colored_vertex(*point + offset, color);
        }
        for i in 0..count as u32 {
            mesh.add_triangle(0, 1 + i, 1 + (i + 1) % count as u32);
        }
        Shape::mesh(mesh)
    };
    painter.add(star(Vec2::splat(DROP), SHADOW));
    painter.add(star(Vec2::ZERO, fill));
    painter.add(Shape::closed_line(ring, Stroke::new(OUTLINE, INK)));
}

/// Реплика в облачке с хвостиком.
pub fn bubble(ui: &mut Ui, text: &str, fill: Color32) {
    let wrap = ui.available_width() - 34.0;
    let color = text_on(fill);
    let galley = ui.fonts(|f| f.layout(text.to_owned(), bold(15.0), color, wrap));
    let size = galley.size() + vec2(28.0, 20.0);
    let tail = 14.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), size.y + tail), Sense::hover());
    let body = Rect::from_min_size(rect.min, vec2(size.x.max(120.0), size.y));
    let painter = ui.painter();
    painter.rect(body, CornerRadius::same(16), fill, Stroke::new(OUTLINE, LINE), StrokeKind::Inside);
    let a = pos2(body.left() + 26.0, body.bottom() - OUTLINE);
    let b = pos2(body.left() + 58.0, body.bottom() - OUTLINE);
    let tip = pos2(body.left() + 22.0, body.bottom() + tail - 1.0);
    painter.add(Shape::convex_polygon(vec![a, b, tip], fill, Stroke::NONE));
    painter.line_segment([a, tip], Stroke::new(OUTLINE, LINE));
    painter.line_segment([b, tip], Stroke::new(OUTLINE, LINE));
    painter.galley(body.min + vec2(14.0, 10.0), galley, color);
}

/// Белая плитка с точками: тонируется при выводе и повторяется без швов.
pub fn halftone_texture(ctx: &egui::Context) -> TextureHandle {
    let tile = HALFTONE_TILE as f32;
    let dots = [(0.0, 0.0), (tile, 0.0), (0.0, tile), (tile, tile), (tile / 2.0, tile / 2.0)];
    let mut pixels = Vec::with_capacity(HALFTONE_TILE * HALFTONE_TILE);
    for y in 0..HALFTONE_TILE {
        for x in 0..HALFTONE_TILE {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let nearest = dots
                .iter()
                .map(|(dx, dy)| ((px - dx).powi(2) + (py - dy).powi(2)).sqrt())
                .fold(f32::MAX, f32::min);
            let cover = (4.5 - nearest + 0.5).clamp(0.0, 1.0);
            pixels.push(Color32::from_white_alpha((cover * 255.0) as u8));
        }
    }
    let image = ColorImage { size: [HALFTONE_TILE, HALFTONE_TILE], pixels };
    ctx.load_texture("halftone", image, TextureOptions::LINEAR_REPEAT)
}

pub fn halftone(painter: &Painter, texture: &TextureHandle, rect: Rect, tint: Color32) {
    let tile = HALFTONE_TILE as f32;
    let uv = Rect::from_min_max((rect.min.to_vec2() / tile).to_pos2(), (rect.max.to_vec2() / tile).to_pos2());
    let mut mesh = Mesh::with_texture(texture.id());
    mesh.add_rect_with_uv(rect, uv, tint);
    painter.add(Shape::mesh(mesh));
}

/// Шкала нагрузки: значение плавно догоняет цель, по заполненной части бежит блик.
pub fn meter(ui: &mut Ui, texture: &TextureHandle, name: &str, value: f32, limit: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
    let shown = ui.ctx().animate_value_with_time(ui.id().with(("meter", name)), value, 0.6);
    let time = ui.input(|i| i.time) as f32;
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, INK);
    let filled = Rect::from_min_size(rect.min, vec2(rect.width() * (shown / 100.0).clamp(0.0, 1.0), rect.height()));
    painter.rect_filled(filled, CornerRadius::ZERO, load_color(value, limit));
    halftone(painter, texture, filled, Color32::from_black_alpha(46));
    if filled.width() > 1.0 {
        let run = filled.width() + 60.0;
        let shine = filled.left() - 30.0 + (time * 90.0) % run;
        let band = Rect::from_min_size(pos2(shine, rect.top()), vec2(26.0, rect.height()));
        painter.with_clip_rect(filled).rect_filled(band, CornerRadius::ZERO, Color32::from_white_alpha(34));
    }
    let tick = rect.left() + rect.width() * (limit / 100.0).clamp(0.0, 1.0);
    painter.line_segment([pos2(tick, rect.top()), pos2(tick, rect.bottom())], Stroke::new(3.0, TEXT));
    painter.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(OUTLINE, LINE), StrokeKind::Inside);
    let label = format!("{name} {shown:.0} %");
    lettering(painter, rect.min + vec2(12.0, 6.0), &label, title(22.0), TEXT, INK, 1.5, 0.0);
}

fn arc(center: Pos2, radius: f32, start: f32, sweep: f32) -> Vec<Pos2> {
    let steps = ((sweep.abs() / 0.09) as usize).max(2);
    (0..=steps)
        .map(|i| {
            let angle = start + sweep * i as f32 / steps as f32;
            center + radius * vec2(angle.cos(), angle.sin())
        })
        .collect()
}

/// Кольцевая шкала с вращающимися сегментами: прибор на приборной панели героя.
pub fn ring_gauge(painter: &Painter, center: Pos2, radius: f32, name: &str, value: Option<f32>, limit: f32, time: f32) {
    let shown = value.unwrap_or(0.0).clamp(0.0, 100.0);
    let color = value.map_or(BUBBLE, |v| load_color(v, limit));
    let band = radius * 0.17;
    let track = radius - band;
    painter.circle_filled(center, radius + 2.0, INK);
    painter.add(Shape::line(arc(center, track, -FRAC_PI_2, TAU), Stroke::new(band, BUBBLE)));
    if shown > 0.5 {
        painter.add(Shape::line(arc(center, track, -FRAC_PI_2, TAU * shown / 100.0), Stroke::new(band, color)));
    }
    let mark = -FRAC_PI_2 + TAU * limit / 100.0;
    let dir = vec2(mark.cos(), mark.sin());
    painter.line_segment([center + dir * (radius - band * 1.7), center + dir * radius], Stroke::new(3.0, TEXT));
    for i in 0..3 {
        let start = time * 0.7 + TAU * i as f32 / 3.0;
        painter.add(Shape::line(arc(center, radius + 1.0, start, 1.3), Stroke::new(2.0, GOLD)));
    }
    let inner = radius - band * 2.2;
    for i in 0..12 {
        let start = -time * 0.35 + TAU * i as f32 / 12.0;
        painter.add(Shape::line(arc(center, inner, start, 0.26), Stroke::new(2.0, LINE)));
    }
    painter.text(center - vec2(0.0, radius * 0.27), Align2::CENTER_CENTER, name, title(radius * 0.28), TEXT_DIM);
    let number = value.map_or("?".to_string(), |v| format!("{v:.0}%"));
    painter.text(center + vec2(0.0, radius * 0.13), Align2::CENTER_CENTER, number, title(radius * 0.5), TEXT);
}

/// Прямоугольник с цветом, плавно меняющимся слева направо.
fn gradient(painter: &Painter, rect: Rect, left: Color32, right: Color32) {
    let mut mesh = Mesh::default();
    for (pos, color) in [(rect.left_top(), left), (rect.right_top(), right), (rect.right_bottom(), right), (rect.left_bottom(), left)] {
        mesh.colored_vertex(pos, color);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

/// Полутон, который проявляется от левого края к правому.
fn halftone_fade(painter: &Painter, texture: &TextureHandle, rect: Rect, from: Color32, to: Color32) {
    let tile = HALFTONE_TILE as f32;
    let mut mesh = Mesh::with_texture(texture.id());
    for (pos, color) in [(rect.left_top(), from), (rect.right_top(), to), (rect.right_bottom(), to), (rect.left_bottom(), from)] {
        mesh.vertices.push(Vertex { pos, uv: pos2(pos.x / tile, pos.y / tile), color });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

/// Фон шапки: тёмно-красный уходит в ночную синеву, справа проступает полутон,
/// раз в несколько секунд проходит блик, по нижней золотой кромке бежит искра.
pub fn header_backdrop(painter: &Painter, texture: &TextureHandle, rect: Rect, time: f32) {
    gradient(painter, rect, HEADER, Color32::from_rgb(20, 18, 34));
    halftone_fade(painter, texture, rect, Color32::TRANSPARENT, Color32::from_black_alpha(70));
    let slant = 46.0;
    let band = Rect::from_min_max(rect.min, pos2(rect.left() + 108.0, rect.bottom()));
    painter.add(Shape::convex_polygon(
        vec![band.left_top(), band.right_top() + vec2(slant, 0.0), band.right_bottom(), band.left_bottom()],
        RED.gamma_multiply(0.55),
        Stroke::NONE,
    ));
    let run = (time % 7.0) / 7.0;
    let x = rect.left() - 160.0 + run * (rect.width() + 320.0);
    painter.add(Shape::convex_polygon(
        vec![pos2(x, rect.top()), pos2(x + 70.0, rect.top()), pos2(x + 30.0, rect.bottom()), pos2(x - 40.0, rect.bottom())],
        Color32::from_white_alpha(7),
        Stroke::NONE,
    ));
    let edge = rect.bottom() - 1.5;
    painter.line_segment([pos2(rect.left(), edge), pos2(rect.right(), edge)], Stroke::new(3.0, LINE));
    let spark = rect.left() + (time * 160.0) % rect.width();
    painter.line_segment([pos2(spark, edge), pos2((spark + 90.0).min(rect.right()), edge)], Stroke::new(3.0, GOLD));
}

/// Эмблема приложения, та же, что на иконке: золотое кольцо, красные дуги и буква K.
pub fn emblem(painter: &Painter, center: Pos2, radius: f32, time: f32) {
    painter.circle_filled(center + Vec2::splat(DROP), radius, SHADOW);
    painter.circle_filled(center, radius, INK);
    painter.circle_stroke(center, radius - 3.0, Stroke::new(5.0, GOLD));
    for i in 0..3 {
        let start = time * 0.4 + TAU * i as f32 / 3.0;
        painter.add(Shape::line(arc(center, radius - 11.0, start, 1.5), Stroke::new(4.0, RED)));
    }
    let font = title(radius * 1.15);
    let size = painter.layout_no_wrap("K".to_owned(), font.clone(), GOLD).size();
    lettering(painter, center - size / 2.0, "K", font, GOLD, INK, 2.5, 0.0);
}

/// Расшифровка названия: первые буквы слов, из которых складывается имя, золотые.
/// Возвращает правый край надписи.
pub fn acronym(painter: &Painter, pos: Pos2, words: &[(&str, bool)]) -> f32 {
    let mut job = LayoutJob::default();
    let plain = TextFormat { font_id: bold(14.0), color: TEXT_DIM, ..Default::default() };
    let marked = TextFormat { font_id: title(17.0), color: GOLD, ..Default::default() };
    for (i, (word, lead)) in words.iter().enumerate() {
        let space = if i == 0 { 0.0 } else { 5.0 };
        let mut chars = word.chars();
        match (*lead, chars.next()) {
            (true, Some(first)) => {
                job.append(&first.to_string(), space, marked.clone());
                job.append(chars.as_str(), 0.0, plain.clone());
            }
            _ => job.append(word, space, plain.clone()),
        }
    }
    let galley = painter.layout_job(job);
    let right = pos.x + galley.size().x;
    painter.galley(pos, galley, TEXT_DIM);
    right
}

/// Строка списка: подпись слева, значение справа, под ними полоса доли.
pub fn bar_row(ui: &mut Ui, label: &str, value: &str, share: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    let shown = ui.ctx().animate_value_with_time(ui.id().with(("bar", label)), share.clamp(0.0, 1.0), 0.6);
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, INK);
    let filled = Rect::from_min_size(rect.min, vec2(rect.width() * shown, rect.height()));
    painter.rect_filled(filled, CornerRadius::ZERO, color.gamma_multiply(0.45));
    painter.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, LINE), StrokeKind::Inside);
    painter.text(pos2(rect.left() + 10.0, rect.center().y), Align2::LEFT_CENTER, label, bold(14.0), TEXT);
    painter.text(pos2(rect.right() - 10.0, rect.center().y), Align2::RIGHT_CENTER, value, bold(14.0), TEXT);
}

/// Плашка состояния в шапке: цветная метка, подпись и значение.
pub fn status_pill(painter: &Painter, rect: Rect, label: &str, value: &str, color: Color32) {
    painter.rect(rect, CornerRadius::same(3), INK.gamma_multiply(0.85), Stroke::new(1.5, LINE), StrokeKind::Inside);
    let mark = Rect::from_min_size(rect.min, vec2(6.0, rect.height()));
    painter.rect_filled(mark, CornerRadius::same(2), color);
    painter.text(pos2(rect.left() + 14.0, rect.center().y), Align2::LEFT_CENTER, label, title(14.0), TEXT_DIM);
    painter.text(pos2(rect.right() - 10.0, rect.center().y), Align2::RIGHT_CENTER, value, bold(13.0), TEXT);
}

/// Живой график: линия истории, заливка под ней, бегущая развёртка и пульс на последней точке.
pub fn history(ui: &mut Ui, name: &str, data: &[f32], limit: f32, time: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HISTORY_HEIGHT), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, INK);
    let y_of = |value: f32| rect.bottom() - rect.height() * (value / 100.0).clamp(0.0, 1.0);
    for level in [25.0, 50.0, 75.0] {
        let y = y_of(level);
        painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0, BUBBLE));
    }
    let y = y_of(limit);
    painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.5, RED));
    let last = data.last().copied().unwrap_or(0.0);
    let color = load_color(last, limit);
    if data.len() >= 2 {
        let step = rect.width() / (crate::live::HISTORY - 1) as f32;
        let points: Vec<Pos2> = data
            .iter()
            .enumerate()
            .map(|(i, v)| pos2(rect.right() - step * (data.len() - 1 - i) as f32, y_of(*v)))
            .collect();
        let shade = color.gamma_multiply(0.22);
        let mut mesh = Mesh::default();
        for point in &points {
            mesh.colored_vertex(*point, shade);
            mesh.colored_vertex(pos2(point.x, rect.bottom()), shade);
        }
        for i in 0..points.len() as u32 - 1 {
            let at = i * 2;
            mesh.add_triangle(at, at + 1, at + 2);
            mesh.add_triangle(at + 1, at + 2, at + 3);
        }
        painter.add(Shape::mesh(mesh));
        let tip = points[points.len() - 1];
        painter.add(Shape::line(points, Stroke::new(2.0, color)));
        painter.circle_filled(tip, 3.5 + 1.5 * (time * 4.0).sin(), color);
    }
    let sweep = rect.left() + (time * 70.0) % rect.width();
    painter.line_segment([pos2(sweep, rect.top()), pos2(sweep, rect.bottom())], Stroke::new(2.0, CYAN.gamma_multiply(0.35)));
    painter.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(OUTLINE, LINE), StrokeKind::Inside);
    let label = format!("{name} {last:.0} %");
    lettering(painter, rect.min + vec2(10.0, 6.0), &label, title(20.0), TEXT, INK, 1.5, 0.0);
}
