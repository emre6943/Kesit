//! The image area: painting annotations and handling the pointer.

use egui::emath::GuiRounding as _;
use egui::{
    Align2, Color32, CursorIcon, FontId, Id, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, pos2,
    vec2,
};

use super::{Drag, Editor, PAD, color32};
use crate::annotate::{self, Annotation, Pt, Tool};
use crate::geometry;
use crate::gui;
use crate::shapes::{self, Grab, Handle};

/// Maps between image pixels and screen points for the current frame.
#[derive(Clone, Copy)]
pub(super) struct Mapping {
    origin: Pos2,
    /// Screen points per image pixel.
    scale: f32,
}

impl Mapping {
    fn to_screen(self, p: Pt) -> Pos2 {
        pos2(
            self.origin.x + p.0 * self.scale,
            self.origin.y + p.1 * self.scale,
        )
    }
    fn to_image(self, p: Pos2) -> Pt {
        (
            (p.x - self.origin.x) / self.scale,
            (p.y - self.origin.y) / self.scale,
        )
    }
    fn rect(self, (min, max): shapes::Bounds) -> Rect {
        Rect::from_min_max(self.to_screen(min), self.to_screen(max))
    }
}

/// Pointer tolerance for grabbing shapes and handles, in screen points.
const GRAB_TOL: f32 = 6.0;
/// Side of a square resize handle, in screen points.
const HANDLE: f32 = 8.0;
const TEXT_EDIT_ID: &str = "kesit-text-draft";

pub(super) fn show(ed: &mut Editor, ui: &mut egui::Ui) {
    let avail = ui.max_rect();
    let resp = ui.allocate_rect(avail, Sense::click_and_drag());
    let image_points = (
        ed.image.width() as f32 / ed.image_scale,
        ed.image.height() as f32 / ed.image_scale,
    );
    let inner = avail.shrink(PAD);
    let fit = geometry::fit_scale(image_points, (inner.width(), inner.height()));
    let shown = vec2(image_points.0 * fit, image_points.1 * fit);
    let img_rect =
        Rect::from_center_size(avail.center(), shown).round_to_pixels(ui.pixels_per_point());
    let map = Mapping {
        origin: img_rect.min,
        scale: img_rect.width() / ed.image.width().max(1) as f32,
    };

    let painter = ui.painter_at(avail);
    let dark = ui.visuals().dark_mode;
    painter.add(
        egui::epaint::Shadow {
            offset: [0, 4],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(if dark { 110 } else { 50 }),
        }
        .as_shape(img_rect, 0.0),
    );

    handle_pointer(ed, ui, &resp, map);
    ed.prepare_textures(ui.ctx());
    if let Some(textures) = &ed.textures {
        painter.image(
            textures.image.id(),
            img_rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }

    let hidden = ed.draft.as_ref().and_then(|d| d.editing);
    for (i, shape) in ed.shapes.iter().enumerate() {
        if Some(i) != hidden {
            paint(ed, &painter, shape, map);
        }
    }
    if let Some(Drag::Create(shape)) = &ed.drag {
        paint(ed, &painter, shape, map);
    }
    if let Some(shape) = ed.selected.and_then(|i| ed.shapes.get(i)) {
        paint_selection(&painter, shape, map, gui::accent(dark));
    }
    if ed.draft.is_some() {
        text_editor(ed, ui, map);
    }
    toast(ed, ui, avail);
}

/// Shape and handle under the pointer, for hover feedback and grabbing.
fn grab_at(ed: &Editor, p: Pt, map: Mapping) -> Option<(usize, Grab)> {
    let tol = GRAB_TOL / map.scale;
    if let Some(i) = ed.selected
        && let Some(h) = ed.shapes.get(i).and_then(|s| shapes::handle_at(s, p, tol))
    {
        return Some((i, Grab::Resize(h)));
    }
    shapes::hit_test(&ed.shapes, p, tol, ed.tool == Tool::Select).map(|i| (i, Grab::Move))
}

fn cursor_for(grab: Grab) -> CursorIcon {
    match grab {
        Grab::Move => CursorIcon::Move,
        Grab::Resize(Handle::N | Handle::S) => CursorIcon::ResizeVertical,
        Grab::Resize(Handle::E | Handle::W) => CursorIcon::ResizeHorizontal,
        Grab::Resize(Handle::NW | Handle::SE) => CursorIcon::ResizeNwSe,
        Grab::Resize(Handle::NE | Handle::SW) => CursorIcon::ResizeNeSw,
        Grab::Resize(Handle::Start | Handle::End) => CursorIcon::Crosshair,
    }
}

fn handle_pointer(ed: &mut Editor, ui: &egui::Ui, resp: &egui::Response, map: Mapping) {
    let (pressed, down, double, shift, pos) = ui.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.pointer
                .button_double_clicked(egui::PointerButton::Primary),
            i.modifiers.shift,
            i.pointer.latest_pos(),
        )
    });
    let Some(pos) = pos else {
        return;
    };
    let p = map.to_image(pos);
    let over_canvas = resp.contains_pointer();

    if ed.drag.is_none() && over_canvas {
        ui.ctx().set_cursor_icon(hover_cursor(ed, p, map));
    }
    if double && over_canvas && ed.draft.is_none() {
        let hit = shapes::hit_test(&ed.shapes, p, GRAB_TOL / map.scale, true);
        if let Some(i) = hit.filter(|i| matches!(ed.shapes[*i], Annotation::Text { .. })) {
            ed.drag = None;
            ed.edit_text(i);
            return;
        }
    }
    if pressed && over_canvas {
        press(ed, p, map);
    } else if ed.drag.is_some() {
        continue_drag(ed, p, shift, down);
    }
}

fn hover_cursor(ed: &Editor, p: Pt, map: Mapping) -> CursorIcon {
    match grab_at(ed, p, map) {
        Some((_, grab)) if ed.draft.is_none() => cursor_for(grab),
        _ => match ed.tool {
            Tool::Select => CursorIcon::Default,
            Tool::Text => CursorIcon::Text,
            _ => CursorIcon::Crosshair,
        },
    }
}

/// A primary press on the canvas: ends a text draft (unless the press is on
/// the field itself), grabs an annotation, or starts a new one.
fn press(ed: &mut Editor, p: Pt, map: Mapping) {
    if let Some(d) = &ed.draft {
        let (w, h) = annotate::text_extent(&d.text, d.size);
        let tol = GRAB_TOL / map.scale;
        let on_field = p.0 >= d.pos.0 - tol
            && p.0 <= d.pos.0 + w + d.size + tol
            && p.1 >= d.pos.1 - tol
            && p.1 <= d.pos.1 + h + tol;
        if !on_field {
            ed.commit_draft();
        }
        return;
    }
    if let Some((index, grab)) = grab_at(ed, p, map) {
        ed.selected = Some(index);
        ed.drag = Some(Drag::Edit {
            index,
            original: ed.shapes[index].clone(),
            grab,
            start: p,
        });
        return;
    }
    ed.selected = None;
    if ed.tool == Tool::Text {
        ed.start_text(p);
    } else {
        ed.drag = new_shape(ed, p).map(Drag::Create);
    }
}

/// A zero-size annotation of the current tool at `p`, grown by the drag.
fn new_shape(ed: &Editor, p: Pt) -> Option<Annotation> {
    let color = ed.color;
    let width = ed.stroke_px();
    match ed.tool {
        Tool::Select | Tool::Text => None,
        Tool::Arrow => Some(Annotation::Arrow {
            from: p,
            to: p,
            color,
            width,
        }),
        Tool::Rect => Some(Annotation::Rect {
            a: p,
            b: p,
            color,
            width,
        }),
        Tool::Ellipse => Some(Annotation::Ellipse {
            a: p,
            b: p,
            color,
            width,
        }),
        Tool::Pen => Some(Annotation::Pen {
            points: vec![p],
            color,
            width,
        }),
        Tool::Highlighter => Some(Annotation::Highlight {
            points: vec![p],
            color,
            width: ed.highlight_px(),
        }),
        Tool::Pixelate => Some(Annotation::Pixelate {
            a: p,
            b: p,
            block: ed.pixel_block,
        }),
    }
}

/// Follows the pointer while a drag is in progress and records the change
/// as one undo step when the button is released.
fn continue_drag(ed: &mut Editor, p: Pt, shift: bool, down: bool) {
    match &mut ed.drag {
        None => return,
        Some(Drag::Create(shape)) => update_new_shape(shape, p, shift),
        Some(Drag::Edit {
            index,
            original,
            grab,
            start,
        }) => {
            let delta = (p.0 - start.0, p.1 - start.1);
            if let Some(slot) = ed.shapes.get_mut(*index) {
                *slot = shapes::drag(original, *grab, delta);
            }
        }
    }
    if down {
        return;
    }
    match ed.drag.take() {
        Some(Drag::Create(shape)) if !shape.is_degenerate() => {
            ed.change(|shapes| shapes.push(shape));
        }
        Some(Drag::Edit {
            index, original, ..
        }) if ed.shapes.get(index).is_some_and(|s| *s != original) => {
            let mut before = ed.shapes.clone();
            before[index] = original;
            ed.history.record(before);
        }
        _ => {}
    }
}

fn update_new_shape(shape: &mut Annotation, p: Pt, shift: bool) {
    match shape {
        Annotation::Arrow { to, .. } => *to = p,
        Annotation::Rect { a, b, .. } | Annotation::Ellipse { a, b, .. } => {
            *b = if shift { square_from(*a, p) } else { p };
        }
        Annotation::Pixelate { b, .. } => *b = p,
        Annotation::Pen { points, width, .. } | Annotation::Highlight { points, width, .. } => {
            if shift {
                // Shift draws a straight line from the starting point.
                points.truncate(1);
                points.push(p);
                return;
            }
            let min_step = (*width * 0.25).clamp(0.5, 4.0);
            if points
                .last()
                .is_none_or(|last| annotate::dist(*last, p) >= min_step)
            {
                points.push(p);
            }
        }
        Annotation::Text { .. } => {}
    }
}

fn square_from(a: Pt, p: Pt) -> Pt {
    let side = (p.0 - a.0).abs().max((p.1 - a.1).abs());
    (
        a.0 + side * (p.0 - a.0).signum(),
        a.1 + side * (p.1 - a.1).signum(),
    )
}

fn text_editor(ed: &mut Editor, ui: &mut egui::Ui, map: Mapping) {
    let accent = gui::accent(ui.visuals().dark_mode);
    let Some(draft) = &mut ed.draft else {
        return;
    };
    let font = FontId::proportional((draft.size * map.scale).max(4.0));
    let color = color32(draft.color);
    let sample = if draft.text.is_empty() {
        "M".to_owned()
    } else {
        draft.text.clone()
    };
    let galley = ui
        .ctx()
        .fonts_mut(|f| f.layout_no_wrap(sample, font.clone(), color));
    let size = galley.size() + vec2(font.size, 0.0);
    let rect = Rect::from_min_size(map.to_screen(draft.pos), size);
    ui.painter().rect_stroke(
        rect.expand(4.0),
        3.0,
        Stroke::new(1.0, accent),
        StrokeKind::Outside,
    );
    let id = Id::unique(TEXT_EDIT_ID);
    // The field owns keyboard focus for as long as the draft exists; the
    // editor ends the draft itself (Enter, Esc, click-away, tool change).
    ui.memory_mut(|m| m.request_focus(id));
    let edit = egui::TextEdit::multiline(&mut draft.text)
        .id(id)
        .font(font)
        .text_color(color)
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::ZERO)
        .desired_rows(1)
        .desired_width(size.x)
        .return_key(egui::KeyboardShortcut::new(
            egui::Modifiers::SHIFT,
            egui::Key::Enter,
        ));
    ui.put(rect, edit);
}

/// Paints one annotation on the canvas; mirrors `annotate::render`.
fn paint(ed: &Editor, painter: &egui::Painter, shape: &Annotation, map: Mapping) {
    let k = map.scale;
    match shape {
        Annotation::Arrow {
            from,
            to,
            color,
            width,
        } => {
            let Some(g) = annotate::arrow_geometry(*from, *to, *width) else {
                return;
            };
            let c = color32(*color);
            painter.line_segment(
                [map.to_screen(*from), map.to_screen(g.shaft_end)],
                Stroke::new(width * k, c),
            );
            painter.circle_filled(map.to_screen(*from), width * k / 2.0, c);
            painter.add(Shape::convex_polygon(
                g.head.iter().map(|p| map.to_screen(*p)).collect(),
                c,
                Stroke::NONE,
            ));
        }
        Annotation::Rect { a, b, color, width } => {
            painter.rect_stroke(
                map.rect(annotate::norm_rect(*a, *b)),
                0.0,
                Stroke::new(width * k, color32(*color)),
                StrokeKind::Middle,
            );
        }
        Annotation::Ellipse { a, b, color, width } => {
            let pts = annotate::ellipse_points(*a, *b, 96)
                .into_iter()
                .map(|p| map.to_screen(p))
                .collect();
            painter.add(Shape::closed_line(
                pts,
                Stroke::new(width * k, color32(*color)),
            ));
        }
        Annotation::Pen {
            points,
            color,
            width,
        } => {
            let c = color32(*color);
            let r = width * k / 2.0;
            if let Some(first) = points.first() {
                painter.circle_filled(map.to_screen(*first), r, c);
            }
            if points.len() > 1 {
                if let Some(last) = points.last() {
                    painter.circle_filled(map.to_screen(*last), r, c);
                }
                let pts: Vec<Pos2> = points.iter().map(|p| map.to_screen(*p)).collect();
                painter.add(Shape::line(pts, Stroke::new(width * k, c)));
            }
        }
        Annotation::Highlight {
            points,
            color,
            width,
        } => {
            if points.len() > 1 {
                let c = Color32::from_rgba_unmultiplied(
                    color[0],
                    color[1],
                    color[2],
                    annotate::HIGHLIGHT_ALPHA,
                );
                let pts: Vec<Pos2> = points.iter().map(|p| map.to_screen(*p)).collect();
                painter.add(Shape::line(pts, Stroke::new(width * k, c)));
            }
        }
        Annotation::Text {
            pos,
            text,
            color,
            size,
        } => {
            painter.text(
                map.to_screen(*pos),
                Align2::LEFT_TOP,
                text,
                FontId::proportional((size * k).max(1.0)),
                color32(*color),
            );
        }
        Annotation::Pixelate { a, b, .. } => {
            let (w, h) = ed.image.dimensions();
            if let Some((x0, y0, x1, y1)) = annotate::pixel_bounds(*a, *b, w, h) {
                let (x0, y0, x1, y1) = (x0 as f32, y0 as f32, x1 as f32, y1 as f32);
                let uv = Rect::from_min_max(
                    pos2(x0 / w as f32, y0 / h as f32),
                    pos2(x1 / w as f32, y1 / h as f32),
                );
                let Some(pixelated) = ed.textures.as_ref().and_then(|t| t.pixelated.as_ref())
                else {
                    return;
                };
                painter.image(
                    pixelated.id(),
                    map.rect(((x0, y0), (x1, y1))),
                    uv,
                    Color32::WHITE,
                );
            }
        }
    }
}

/// Bounding box and handles of the selected annotation.
fn paint_selection(painter: &egui::Painter, shape: &Annotation, map: Mapping, accent: Color32) {
    let outline = Stroke::new(1.0, accent);
    if !matches!(shape, Annotation::Arrow { .. }) {
        let pad = match shape {
            Annotation::Text { .. } | Annotation::Pixelate { .. } => 2.0,
            _ => 0.0,
        };
        painter.rect_stroke(
            map.rect(shapes::bounds(shape)).expand(pad),
            0.0,
            outline,
            StrokeKind::Outside,
        );
    }
    for (_, p) in shapes::handles(shape) {
        let c = map.to_screen(p);
        let r = Rect::from_center_size(c, vec2(HANDLE, HANDLE));
        painter.rect_filled(r, 2.0, Color32::WHITE);
        painter.rect_stroke(r, 2.0, Stroke::new(1.5, accent), StrokeKind::Inside);
    }
}

/// A short status message at the bottom of the canvas.
fn toast(ed: &mut Editor, ui: &egui::Ui, area: Rect) {
    let now = ui.input(|i| i.time);
    let Some(t) = &ed.toast else {
        return;
    };
    if now > t.until {
        ed.toast = None;
        return;
    }
    let dark = ui.visuals().dark_mode;
    let color = if t.error {
        gui::error_color(dark)
    } else {
        ui.visuals().strong_text_color()
    };
    let painter = ui.painter();
    let galley = painter.layout(
        t.text.clone(),
        FontId::proportional(13.0),
        color,
        (area.width() - 64.0).max(80.0),
    );
    let pad = vec2(12.0, 7.0);
    let size = galley.size() + pad * 2.0;
    let rect = Rect::from_center_size(
        pos2(area.center().x, area.max.y - 16.0 - size.y / 2.0),
        size,
    );
    painter.rect(
        rect,
        8.0,
        ui.visuals().panel_fill,
        ui.visuals().widgets.noninteractive.bg_stroke,
        StrokeKind::Inside,
    );
    painter.galley(rect.min + pad, galley, color);
}
