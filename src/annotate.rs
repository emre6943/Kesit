//! Annotation model, the geometry shared by the on-screen preview and the
//! export, and the full-resolution rasteriser.
//!
//! All coordinates and sizes are in physical image pixels, so the preview
//! (which scales them to the canvas) and the export (which draws them 1:1)
//! use exactly the same geometry.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use anyhow::{Context, Result};
use image::RgbaImage;
use tiny_skia::{
    FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PremultipliedColorU8, Rect, Stroke,
    Transform,
};

/// The proportional font egui renders with, so the export matches the preview.
pub const TEXT_FONT: &[u8] = epaint_default_fonts::UBUNTU_LIGHT;

pub type Pt = (f32, f32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Arrow,
    Rect,
    Ellipse,
    Pen,
    Highlighter,
    Text,
    Pixelate,
}

impl Tool {
    pub const ALL: [Tool; 8] = [
        Tool::Select,
        Tool::Arrow,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Pen,
        Tool::Highlighter,
        Tool::Text,
        Tool::Pixelate,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Arrow => "Arrow",
            Tool::Rect => "Rectangle",
            Tool::Ellipse => "Ellipse",
            Tool::Pen => "Pen",
            Tool::Highlighter => "Highlighter",
            Tool::Text => "Text",
            Tool::Pixelate => "Pixelate",
        }
    }

    /// Stable name used in the config file.
    pub fn id(self) -> &'static str {
        match self {
            Tool::Select => "select",
            Tool::Arrow => "arrow",
            Tool::Rect => "rect",
            Tool::Ellipse => "ellipse",
            Tool::Pen => "pen",
            Tool::Highlighter => "highlighter",
            Tool::Text => "text",
            Tool::Pixelate => "pixelate",
        }
    }

    pub fn from_id(id: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| t.id() == id)
    }
}

/// Opacity of highlighter strokes.
pub const HIGHLIGHT_ALPHA: u8 = 102;

#[derive(Debug, Clone, PartialEq)]
pub enum Annotation {
    Arrow {
        from: Pt,
        to: Pt,
        color: [u8; 3],
        width: f32,
    },
    Rect {
        a: Pt,
        b: Pt,
        color: [u8; 3],
        width: f32,
    },
    Ellipse {
        a: Pt,
        b: Pt,
        color: [u8; 3],
        width: f32,
    },
    Pen {
        points: Vec<Pt>,
        color: [u8; 3],
        width: f32,
    },
    /// A wide translucent stroke with flat ends.
    Highlight {
        points: Vec<Pt>,
        color: [u8; 3],
        width: f32,
    },
    /// `pos` is the top-left of the first line; `size` is the font em size.
    /// Lines are separated by `\n`.
    Text {
        pos: Pt,
        text: String,
        color: [u8; 3],
        size: f32,
    },
    /// Replaces the original pixels inside `a`–`b` with blocks of `block`
    /// pixels (aligned to the image grid) averaged over the original image.
    Pixelate { a: Pt, b: Pt, block: u32 },
}

impl Annotation {
    /// True when the shape is too small to be worth keeping (a click without
    /// a drag, an empty text).
    pub fn is_degenerate(&self) -> bool {
        match self {
            Annotation::Arrow { from, to, .. } => dist(*from, *to) < 2.0,
            Annotation::Rect { a, b, .. } | Annotation::Ellipse { a, b, .. } => {
                (a.0 - b.0).abs() < 2.0 && (a.1 - b.1).abs() < 2.0
            }
            Annotation::Pixelate { a, b, .. } => (a.0 - b.0).abs() < 2.0 || (a.1 - b.1).abs() < 2.0,
            Annotation::Pen { points, .. } => points.is_empty(),
            Annotation::Highlight { points, .. } => match points.as_slice() {
                [] => true,
                [first, rest @ ..] => rest.iter().all(|p| dist(*first, *p) < 2.0),
            },
            Annotation::Text { text, .. } => text.trim().is_empty(),
        }
    }
}

/// Text em size in image pixels for a stroke preset given in screen points.
pub fn text_size_for_stroke(stroke_points: f32, image_scale: f32) -> f32 {
    (12.0 + 3.0 * stroke_points) * image_scale
}

/// Highlighter width in image pixels for a stroke preset given in screen points.
pub fn highlight_width_for_stroke(stroke_points: f32, image_scale: f32) -> f32 {
    (8.0 + 3.0 * stroke_points) * image_scale
}

/// Pixelation block size in image pixels: about 10 screen points.
pub fn pixel_block(image_scale: f32) -> u32 {
    (10.0 * image_scale).round().clamp(4.0, 64.0) as u32
}

/// Integer pixel bounds `(x0, y0, x1, y1)` of the rectangle `a`–`b`, rounded
/// outwards and clamped to a `w`×`h` image. Shared by preview and export.
pub fn pixel_bounds(a: Pt, b: Pt, w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
    let ((x0, y0), (x1, y1)) = norm_rect(a, b);
    if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
        return None;
    }
    let cx = |v: f32| v.clamp(0.0, w as f32);
    let cy = |v: f32| v.clamp(0.0, h as f32);
    let (px0, py0) = (cx(x0.floor()) as u32, cy(y0.floor()) as u32);
    let (px1, py1) = (cx(x1.ceil()) as u32, cy(y1.ceil()) as u32);
    (px1 > px0 && py1 > py0).then_some((px0, py0, px1, py1))
}

/// A copy of `img` where every `block`×`block` cell (aligned to the image
/// origin) is filled with the cell's average colour.
pub fn pixelate(img: &RgbaImage, block: u32) -> RgbaImage {
    let block = block.max(1);
    let (w, h) = img.dimensions();
    let mut out = RgbaImage::new(w, h);
    for by in (0..h).step_by(block as usize) {
        for bx in (0..w).step_by(block as usize) {
            let (x1, y1) = ((bx + block).min(w), (by + block).min(h));
            let mut sum = [0u64; 4];
            for y in by..y1 {
                for x in bx..x1 {
                    let p = img.get_pixel(x, y).0;
                    for (s, c) in sum.iter_mut().zip(p) {
                        *s += u64::from(c);
                    }
                }
            }
            let n = u64::from((x1 - bx) * (y1 - by)).max(1);
            let avg = image::Rgba(sum.map(|s| (s / n) as u8));
            for y in by..y1 {
                for x in bx..x1 {
                    out.put_pixel(x, y, avg);
                }
            }
        }
    }
    out
}

/// Size `(width, height)` in image pixels of `text` laid out at em size `em`,
/// matching [`render`]. An empty text still has the height of one line.
pub fn text_extent(text: &str, em: f32) -> Pt {
    let Ok(font) = FontRef::try_from_slice(TEXT_FONT) else {
        return (0.0, em);
    };
    let scaled = font.as_scaled(px_scale_for_em(&font, em));
    let line_height = scaled.ascent() - scaled.descent() + scaled.line_gap();
    let mut width: f32 = 0.0;
    let mut lines = 0;
    for line in text.split('\n') {
        lines += 1;
        let mut caret = 0.0;
        let mut prev = None;
        for ch in line.chars() {
            let glyph = scaled.glyph_id(ch);
            if let Some(p) = prev {
                caret += scaled.kern(p, glyph);
            }
            caret += scaled.h_advance(glyph);
            prev = Some(glyph);
        }
        width = width.max(caret);
    }
    let height = (lines.max(1) - 1) as f32 * line_height + scaled.ascent() - scaled.descent();
    (width, height)
}

pub fn dist(a: Pt, b: Pt) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// Arrow geometry: the shaft runs from `from` to `shaft_end` (which lies inside
/// the head), the head is a filled triangle `[tip, left, right]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrowGeom {
    pub shaft_end: Pt,
    pub head: [Pt; 3],
}

/// Head length grows with the stroke width, and is capped for short arrows.
pub fn arrow_geometry(from: Pt, to: Pt, width: f32) -> Option<ArrowGeom> {
    let len = dist(from, to);
    if len.is_nan() || len < 1.0 {
        return None;
    }
    let (ux, uy) = ((to.0 - from.0) / len, (to.1 - from.1) / len);
    let head_len = (width * 4.0 + 6.0).min(len * 0.6);
    let half_w = head_len * 0.5;
    let base = (to.0 - ux * head_len, to.1 - uy * head_len);
    let (nx, ny) = (-uy, ux);
    let left = (base.0 + nx * half_w, base.1 + ny * half_w);
    let right = (base.0 - nx * half_w, base.1 - ny * half_w);
    let inset = head_len * 0.7;
    let shaft_end = (to.0 - ux * inset, to.1 - uy * inset);
    Some(ArrowGeom {
        shaft_end,
        head: [to, left, right],
    })
}

/// Normalised rectangle `(min, max)` from two corners.
pub fn norm_rect(a: Pt, b: Pt) -> (Pt, Pt) {
    ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)))
}

/// Polygon approximation of the ellipse inscribed in the rectangle `a`–`b`,
/// used by the preview.
pub fn ellipse_points(a: Pt, b: Pt, segments: usize) -> Vec<Pt> {
    let ((x0, y0), (x1, y1)) = norm_rect(a, b);
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (rx, ry) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
    (0..segments)
        .map(|i| {
            let t = i as f32 / segments as f32 * std::f32::consts::TAU;
            (cx + rx * t.cos(), cy + ry * t.sin())
        })
        .collect()
}

/// Draws `annotations` onto a copy of `base` at full resolution.
pub fn render(base: &RgbaImage, annotations: &[Annotation]) -> Result<RgbaImage> {
    let (w, h) = base.dimensions();
    let mut pixmap = to_pixmap(base).context("creating the export canvas")?;
    let font = FontRef::try_from_slice(TEXT_FONT).context("loading the text font")?;
    // One pixelated copy per block size; normally there is only one.
    let mut pixelated: Vec<(u32, RgbaImage)> = Vec::new();
    for annotation in annotations {
        if let Annotation::Pixelate { a, b, block } = annotation {
            let source = match pixelated.iter().position(|(size, _)| size == block) {
                Some(index) => &pixelated[index].1,
                None => {
                    pixelated.push((*block, pixelate(base, *block)));
                    &pixelated[pixelated.len() - 1].1
                }
            };
            copy_region(&mut pixmap, source, *a, *b);
        } else {
            draw(&mut pixmap, &font, annotation);
        }
    }
    RgbaImage::from_raw(w, h, pixmap.take_demultiplied())
        .context("converting the export canvas back to an image")
}

fn to_pixmap(img: &RgbaImage) -> Option<Pixmap> {
    let (w, h) = img.dimensions();
    let mut data = img.as_raw().clone();
    for px in data.as_chunks_mut::<4>().0 {
        let a = px[3] as u16;
        if a != 255 {
            for c in &mut px[..3] {
                *c = ((*c as u16 * a + 127) / 255) as u8;
            }
        }
    }
    Pixmap::from_vec(data, tiny_skia::IntSize::from_wh(w, h)?)
}

/// Copies the pixels of `src` inside `a`–`b` onto `pixmap` (same size).
fn copy_region(pixmap: &mut Pixmap, src: &RgbaImage, a: Pt, b: Pt) {
    let (w, h) = src.dimensions();
    let Some((x0, y0, x1, y1)) = pixel_bounds(a, b, w, h) else {
        return;
    };
    let pw = pixmap.width();
    let pixels = pixmap.pixels_mut();
    for y in y0..y1 {
        for x in x0..x1 {
            let [r, g, b, a] = src.get_pixel(x, y).0;
            let pm = |c: u8| ((u16::from(c) * u16::from(a) + 127) / 255) as u8;
            if let (Some(dst), Some(px)) = (
                pixels.get_mut((y * pw + x) as usize),
                PremultipliedColorU8::from_rgba(pm(r), pm(g), pm(b), a),
            ) {
                *dst = px;
            }
        }
    }
}

fn paint_for(color: [u8; 3]) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], 255);
    paint.anti_alias = true;
    paint
}

fn stroke_for(width: f32, cap: LineCap, join: LineJoin) -> Stroke {
    Stroke {
        width: width.max(0.5),
        line_cap: cap,
        line_join: join,
        ..Stroke::default()
    }
}

fn draw(pixmap: &mut Pixmap, font: &FontRef<'_>, annotation: &Annotation) {
    let identity = Transform::identity();
    match annotation {
        Annotation::Arrow {
            from,
            to,
            color,
            width,
        } => {
            let Some(geom) = arrow_geometry(*from, *to, *width) else {
                return;
            };
            let paint = paint_for(*color);
            let mut pb = PathBuilder::new();
            pb.move_to(from.0, from.1);
            pb.line_to(geom.shaft_end.0, geom.shaft_end.1);
            if let Some(path) = pb.finish() {
                let stroke = stroke_for(*width, LineCap::Round, LineJoin::Round);
                pixmap.stroke_path(&path, &paint, &stroke, identity, None);
            }
            let [tip, l, r] = geom.head;
            let mut pb = PathBuilder::new();
            pb.move_to(tip.0, tip.1);
            pb.line_to(l.0, l.1);
            pb.line_to(r.0, r.1);
            pb.close();
            if let Some(path) = pb.finish() {
                pixmap.fill_path(&path, &paint, FillRule::Winding, identity, None);
            }
        }
        Annotation::Rect { a, b, color, width } => {
            let ((x0, y0), (x1, y1)) = norm_rect(*a, *b);
            if let Some(rect) = Rect::from_ltrb(x0, y0, x1, y1) {
                let path = PathBuilder::from_rect(rect);
                let stroke = stroke_for(*width, LineCap::Butt, LineJoin::Miter);
                pixmap.stroke_path(&path, &paint_for(*color), &stroke, identity, None);
            }
        }
        Annotation::Ellipse { a, b, color, width } => {
            let ((x0, y0), (x1, y1)) = norm_rect(*a, *b);
            if let Some(path) = Rect::from_ltrb(x0, y0, x1, y1).and_then(PathBuilder::from_oval) {
                let stroke = stroke_for(*width, LineCap::Round, LineJoin::Round);
                pixmap.stroke_path(&path, &paint_for(*color), &stroke, identity, None);
            }
        }
        Annotation::Pen {
            points,
            color,
            width,
        } => {
            let paint = paint_for(*color);
            match points.as_slice() {
                [] => {}
                [p] => {
                    if let Some(path) = PathBuilder::from_circle(p.0, p.1, (width / 2.0).max(0.5)) {
                        pixmap.fill_path(&path, &paint, FillRule::Winding, identity, None);
                    }
                }
                [first, rest @ ..] => {
                    let mut pb = PathBuilder::new();
                    pb.move_to(first.0, first.1);
                    for p in rest {
                        pb.line_to(p.0, p.1);
                    }
                    if let Some(path) = pb.finish() {
                        let stroke = stroke_for(*width, LineCap::Round, LineJoin::Round);
                        pixmap.stroke_path(&path, &paint, &stroke, identity, None);
                    }
                }
            }
        }
        Annotation::Highlight {
            points,
            color,
            width,
        } => {
            let mut paint = paint_for(*color);
            paint.set_color_rgba8(color[0], color[1], color[2], HIGHLIGHT_ALPHA);
            let mut pb = PathBuilder::new();
            if let [first, rest @ ..] = points.as_slice() {
                pb.move_to(first.0, first.1);
                for p in rest {
                    pb.line_to(p.0, p.1);
                }
            }
            if let Some(path) = pb.finish() {
                let stroke = stroke_for(*width, LineCap::Butt, LineJoin::Round);
                pixmap.stroke_path(&path, &paint, &stroke, identity, None);
            }
        }
        Annotation::Text {
            pos,
            text,
            color,
            size,
        } => draw_text(pixmap, font, *pos, text, *color, *size),
        // Handled by `render`, which owns the pixelated source image.
        Annotation::Pixelate { .. } => {}
    }
}

/// Converts an em size in pixels into ab_glyph's height-based scale.
pub fn px_scale_for_em(font: &FontRef<'_>, em: f32) -> PxScale {
    let upem = font.units_per_em().unwrap_or(1000.0);
    PxScale::from(em * font.height_unscaled() / upem)
}

/// Lays out and blends `text` with its first line's top-left at `pos`.
fn draw_text(
    pixmap: &mut Pixmap,
    font: &FontRef<'_>,
    pos: Pt,
    text: &str,
    color: [u8; 3],
    em: f32,
) {
    let scaled = font.as_scaled(px_scale_for_em(font, em));
    let line_height = scaled.ascent() - scaled.descent() + scaled.line_gap();
    let (pw, ph) = (pixmap.width() as i64, pixmap.height() as i64);
    let pixels = pixmap.pixels_mut();
    for (line_no, line) in text.split('\n').enumerate() {
        let baseline = pos.1 + scaled.ascent() + line_no as f32 * line_height;
        let mut caret = pos.0;
        let mut prev = None;
        for ch in line.chars() {
            let glyph_id = scaled.glyph_id(ch);
            if let Some(p) = prev {
                caret += scaled.kern(p, glyph_id);
            }
            let glyph = glyph_id.with_scale_and_position(scaled.scale(), point(caret, baseline));
            caret += scaled.h_advance(glyph_id);
            prev = Some(glyph_id);
            let Some(outlined) = font.outline_glyph(glyph) else {
                continue;
            };
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, coverage| {
                let x = bounds.min.x as i64 + gx as i64;
                let y = bounds.min.y as i64 + gy as i64;
                if x < 0 || y < 0 || x >= pw || y >= ph {
                    return;
                }
                let idx = (y * pw + x) as usize;
                if let Some(px) = pixels.get_mut(idx) {
                    *px = blend_over(*px, color, coverage);
                }
            });
        }
    }
}

/// Source-over blend of an opaque colour with `coverage` onto a premultiplied pixel.
fn blend_over(dst: PremultipliedColorU8, color: [u8; 3], coverage: f32) -> PremultipliedColorU8 {
    let c = coverage.clamp(0.0, 1.0);
    if c <= 0.0 {
        return dst;
    }
    let inv = 1.0 - c;
    let a = (255.0 * c + dst.alpha() as f32 * inv).round().min(255.0);
    let ch = |src: u8, d: u8| (src as f32 * c + d as f32 * inv).round().min(a) as u8;
    let r = ch(color[0], dst.red());
    let g = ch(color[1], dst.green());
    let b = ch(color[2], dst.blue());
    PremultipliedColorU8::from_rgba(r, g, b, a as u8).unwrap_or(dst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    const W: u32 = 200;
    const H: u32 = 200;
    const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);
    const RED: [u8; 3] = [255, 0, 0];

    fn base() -> RgbaImage {
        RgbaImage::from_pixel(W, H, WHITE)
    }

    fn changed_in(img: &RgbaImage, x0: u32, y0: u32, x1: u32, y1: u32) -> usize {
        let mut n = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                if *img.get_pixel(x, y) != WHITE {
                    n += 1;
                }
            }
        }
        n
    }

    /// The shape changes pixels inside its box and leaves the far corner alone.
    fn check(a: Annotation, region: (u32, u32, u32, u32)) {
        let out = render(&base(), std::slice::from_ref(&a)).expect("render");
        assert_eq!(out.dimensions(), (W, H));
        let (x0, y0, x1, y1) = region;
        assert!(
            changed_in(&out, x0, y0, x1, y1) > 0,
            "{a:?} drew nothing in {region:?}"
        );
        assert_eq!(
            changed_in(&out, 170, 170, W, H),
            0,
            "{a:?} touched the far corner"
        );
        assert_eq!(*out.get_pixel(199, 199), WHITE);
    }

    #[test]
    fn arrow_draws() {
        let a = Annotation::Arrow {
            from: (20.0, 20.0),
            to: (100.0, 100.0),
            color: RED,
            width: 6.0,
        };
        check(a.clone(), (20, 20, 101, 101));
        // The tip is reached and coloured red.
        let out = render(&base(), &[a]).expect("render");
        let tip = out.get_pixel(97, 97);
        assert!(tip[0] > 200 && tip[1] < 80, "tip {tip:?}");
    }

    #[test]
    fn rect_draws_outline_only() {
        let a = Annotation::Rect {
            a: (100.0, 100.0),
            b: (20.0, 30.0),
            color: RED,
            width: 4.0,
        };
        let out = render(&base(), std::slice::from_ref(&a)).expect("render");
        assert!(*out.get_pixel(20, 60) != WHITE, "left edge");
        assert_eq!(*out.get_pixel(60, 65), WHITE, "interior is not filled");
        check(a, (18, 28, 102, 102));
    }

    #[test]
    fn ellipse_draws() {
        let a = Annotation::Ellipse {
            a: (20.0, 20.0),
            b: (120.0, 80.0),
            color: RED,
            width: 4.0,
        };
        let out = render(&base(), std::slice::from_ref(&a)).expect("render");
        assert!(*out.get_pixel(70, 20) != WHITE, "top of ellipse");
        assert_eq!(*out.get_pixel(70, 50), WHITE, "centre untouched");
        assert_eq!(*out.get_pixel(21, 21), WHITE, "bbox corner untouched");
        check(a, (18, 18, 122, 82));
    }

    #[test]
    fn pen_draws_and_single_point_dot() {
        check(
            Annotation::Pen {
                points: vec![(20.0, 20.0), (60.0, 40.0), (100.0, 20.0)],
                color: RED,
                width: 5.0,
            },
            (18, 18, 102, 42),
        );
        check(
            Annotation::Pen {
                points: vec![(50.0, 50.0)],
                color: RED,
                width: 8.0,
            },
            (46, 46, 54, 54),
        );
    }

    #[test]
    fn text_draws() {
        let a = Annotation::Text {
            pos: (20.0, 20.0),
            text: "Kesit".into(),
            color: RED,
            size: 40.0,
        };
        check(a, (20, 20, 160, 70));
    }

    #[test]
    fn highlight_is_translucent() {
        let a = Annotation::Highlight {
            points: vec![(20.0, 50.0), (150.0, 50.0)],
            color: RED,
            width: 20.0,
        };
        let out = render(&base(), std::slice::from_ref(&a)).expect("render");
        let mid = out.get_pixel(80, 50);
        assert!(mid[0] == 255 && mid[1] > 100 && mid[1] < 200, "mid {mid:?}");
        check(a, (20, 40, 150, 60));
    }

    #[test]
    fn pixelate_averages_blocks() {
        let mut img = RgbaImage::from_pixel(8, 4, WHITE);
        img.put_pixel(0, 0, Rgba([0, 0, 0, 255]));
        let out = pixelate(&img, 2);
        // Block (0,0)-(2,2): one black and three white pixels.
        assert_eq!(out.get_pixel(1, 1).0, [191, 191, 191, 255]);
        assert_eq!(out.get_pixel(0, 0), out.get_pixel(1, 0));
        assert_eq!(*out.get_pixel(5, 3), WHITE);
        // Partial blocks at the edges are averaged over what exists.
        assert_eq!(
            pixelate(&RgbaImage::from_pixel(5, 5, WHITE), 4).dimensions(),
            (5, 5)
        );
    }

    #[test]
    fn pixelate_export_hides_detail_inside_only() {
        let mut img = base();
        for y in 0..H {
            for x in 0..W {
                if (x + y) % 2 == 0 {
                    img.put_pixel(x, y, Rgba([0, 0, 0, 255]));
                }
            }
        }
        let a = Annotation::Pixelate {
            a: (20.0, 20.0),
            b: (100.0, 100.0),
            block: 10,
        };
        let out = render(&img, &[a]).expect("render");
        // The checkerboard is gone inside: neighbours in one block are equal.
        assert_eq!(out.get_pixel(30, 30), out.get_pixel(31, 30));
        let p = out.get_pixel(30, 30).0;
        assert!(p[0] > 100 && p[0] < 155, "{p:?}");
        // Outside the rectangle the original pixels are untouched.
        assert_eq!(out.get_pixel(150, 150), img.get_pixel(150, 150));
        assert_eq!(out.get_pixel(151, 150), img.get_pixel(151, 150));
        assert_eq!(out.get_pixel(19, 30), img.get_pixel(19, 30));
    }

    #[test]
    fn pixel_bounds_round_out_and_clamp() {
        assert_eq!(
            pixel_bounds((10.5, 20.2), (3.0, 4.9), 100, 100),
            Some((3, 4, 11, 21))
        );
        assert_eq!(
            pixel_bounds((-5.0, -5.0), (500.0, 50.0), 100, 100),
            Some((0, 0, 100, 50))
        );
        assert_eq!(pixel_bounds((200.0, 0.0), (300.0, 10.0), 100, 100), None);
    }

    #[test]
    fn text_extent_grows_with_text_and_lines() {
        let (w1, h1) = text_extent("Kesit", 20.0);
        let (w2, _) = text_extent("Kesit Kesit", 20.0);
        let (w3, h3) = text_extent("Kesit\nKesit", 20.0);
        assert!(w1 > 20.0 && w2 > w1 * 1.8);
        assert!((w3 - w1).abs() < 0.01);
        assert!(h3 > h1 * 1.8);
        let (w4, _) = text_extent("Kesit", 40.0);
        assert!((w4 - 2.0 * w1).abs() < 0.5);
        assert_eq!(text_extent("", 20.0).0, 0.0);
    }

    #[test]
    fn tool_ids_round_trip() {
        for tool in Tool::ALL {
            assert_eq!(Tool::from_id(tool.id()), Some(tool));
        }
        assert_eq!(Tool::from_id("nope"), None);
    }

    #[test]
    fn stroke_width_scales_coverage() {
        let thin = Annotation::Rect {
            a: (20.0, 20.0),
            b: (120.0, 120.0),
            color: RED,
            width: 2.0,
        };
        let thick = Annotation::Rect {
            a: (20.0, 20.0),
            b: (120.0, 120.0),
            color: RED,
            width: 8.0,
        };
        let n_thin = changed_in(&render(&base(), &[thin]).expect("render"), 0, 0, W, H);
        let n_thick = changed_in(&render(&base(), &[thick]).expect("render"), 0, 0, W, H);
        assert!(n_thick > n_thin * 3, "thin {n_thin} thick {n_thick}");
    }

    #[test]
    fn arrow_head_proportional_and_capped() {
        let g1 = arrow_geometry((0.0, 0.0), (200.0, 0.0), 2.0).expect("g1");
        let g2 = arrow_geometry((0.0, 0.0), (200.0, 0.0), 8.0).expect("g2");
        let head_len = |g: &ArrowGeom| g.head[0].0 - g.head[1].0;
        assert!(head_len(&g2) > head_len(&g1));
        let short = arrow_geometry((0.0, 0.0), (10.0, 0.0), 8.0).expect("short");
        assert!(head_len(&short) <= 6.0 + 1e-3);
        assert!(arrow_geometry((5.0, 5.0), (5.0, 5.0), 4.0).is_none());
    }

    #[test]
    fn degenerate_detection() {
        assert!(
            Annotation::Text {
                pos: (0.0, 0.0),
                text: "  ".into(),
                color: RED,
                size: 10.0
            }
            .is_degenerate()
        );
        assert!(
            Annotation::Rect {
                a: (1.0, 1.0),
                b: (1.5, 1.5),
                color: RED,
                width: 2.0
            }
            .is_degenerate()
        );
    }
}
