//! Editing existing annotations: bounding boxes, hit-testing, handles,
//! move/resize geometry and the undo history. Pure and unit-tested; all
//! coordinates are image pixels like the annotations themselves.

use crate::annotate::{self, Annotation, Pt};

/// A resize handle: the eight box handles, or an arrow's two endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    NW,
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    Start,
    End,
}

/// What a drag on a selected annotation does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grab {
    Move,
    Resize(Handle),
}

/// Axis-aligned box `(min, max)`.
pub type Bounds = (Pt, Pt);

/// Geometric bounding box (without stroke width).
pub fn bounds(shape: &Annotation) -> Bounds {
    match shape {
        Annotation::Arrow { from, to, .. } => annotate::norm_rect(*from, *to),
        Annotation::Rect { a, b, .. }
        | Annotation::Ellipse { a, b, .. }
        | Annotation::Pixelate { a, b, .. } => annotate::norm_rect(*a, *b),
        Annotation::Pen { points, .. } | Annotation::Highlight { points, .. } => {
            points_bounds(points)
        }
        Annotation::Text {
            pos, text, size, ..
        } => {
            let (w, h) = annotate::text_extent(text, *size);
            (*pos, (pos.0 + w, pos.1 + h))
        }
    }
}

fn points_bounds(points: &[Pt]) -> Bounds {
    let mut it = points.iter();
    let Some(first) = it.next() else {
        return ((0.0, 0.0), (0.0, 0.0));
    };
    it.fold((*first, *first), |(min, max), p| {
        (
            (min.0.min(p.0), min.1.min(p.1)),
            (max.0.max(p.0), max.1.max(p.1)),
        )
    })
}

/// Half the stroke width: how far ink reaches beyond the geometry.
fn half_width(shape: &Annotation) -> f32 {
    match shape {
        Annotation::Arrow { width, .. }
        | Annotation::Rect { width, .. }
        | Annotation::Ellipse { width, .. }
        | Annotation::Pen { width, .. }
        | Annotation::Highlight { width, .. } => width / 2.0,
        Annotation::Text { .. } | Annotation::Pixelate { .. } => 0.0,
    }
}

/// Handles shown on a selected annotation, with their positions.
pub fn handles(shape: &Annotation) -> Vec<(Handle, Pt)> {
    if let Annotation::Arrow { from, to, .. } = shape {
        return vec![(Handle::Start, *from), (Handle::End, *to)];
    }
    let ((x0, y0), (x1, y1)) = bounds(shape);
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let corners = [
        (Handle::NW, (x0, y0)),
        (Handle::NE, (x1, y0)),
        (Handle::SE, (x1, y1)),
        (Handle::SW, (x0, y1)),
    ];
    if matches!(shape, Annotation::Text { .. }) {
        // Text scales uniformly, from its corners only.
        return corners.to_vec();
    }
    let mut all = corners.to_vec();
    all.extend([
        (Handle::N, (cx, y0)),
        (Handle::E, (x1, cy)),
        (Handle::S, (cx, y1)),
        (Handle::W, (x0, cy)),
    ]);
    all
}

/// The handle of `shape` within `tol` of `p`, if any.
pub fn handle_at(shape: &Annotation, p: Pt, tol: f32) -> Option<Handle> {
    handles(shape)
        .into_iter()
        .filter(|(_, hp)| (hp.0 - p.0).abs() <= tol && (hp.1 - p.1).abs() <= tol)
        .min_by(|a, b| {
            annotate::dist(a.1, p)
                .partial_cmp(&annotate::dist(b.1, p))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(h, _)| h)
}

fn dist_to_segment(p: Pt, a: Pt, b: Pt) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    if len2 <= f32::EPSILON {
        return annotate::dist(p, a);
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0);
    annotate::dist(p, (a.0 + t * dx, a.1 + t * dy))
}

fn dist_to_polyline(p: Pt, points: &[Pt]) -> f32 {
    match points {
        [] => f32::INFINITY,
        [only] => annotate::dist(p, *only),
        _ => points
            .windows(2)
            .map(|w| dist_to_segment(p, w[0], w[1]))
            .fold(f32::INFINITY, f32::min),
    }
}

fn inside(p: Pt, (min, max): Bounds, pad: f32) -> bool {
    p.0 >= min.0 - pad && p.0 <= max.0 + pad && p.1 >= min.1 - pad && p.1 <= max.1 + pad
}

/// True when `p` is on the annotation's ink (within `tol`). With
/// `interior`, the inside of rectangles and ellipses counts too; filled
/// shapes (text, pixelate) always count their inside.
pub fn hit(shape: &Annotation, p: Pt, tol: f32, interior: bool) -> bool {
    let reach = tol + half_width(shape);
    match shape {
        Annotation::Arrow { from, to, .. } => dist_to_segment(p, *from, *to) <= reach,
        Annotation::Rect { a, b, .. } => {
            let ((x0, y0), (x1, y1)) = annotate::norm_rect(*a, *b);
            let outline = dist_to_polyline(p, &[(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)]);
            outline <= reach || (interior && inside(p, ((x0, y0), (x1, y1)), 0.0))
        }
        Annotation::Ellipse { a, b, .. } => {
            let mut ring = annotate::ellipse_points(*a, *b, 64);
            if let Some(first) = ring.first().copied() {
                ring.push(first);
            }
            if dist_to_polyline(p, &ring) <= reach {
                return true;
            }
            let ((x0, y0), (x1, y1)) = annotate::norm_rect(*a, *b);
            let (rx, ry) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
            if !interior || rx <= 0.0 || ry <= 0.0 {
                return false;
            }
            let (nx, ny) = ((p.0 - x0 - rx) / rx, (p.1 - y0 - ry) / ry);
            nx * nx + ny * ny <= 1.0
        }
        Annotation::Pen { points, .. } | Annotation::Highlight { points, .. } => {
            dist_to_polyline(p, points) <= reach
        }
        Annotation::Text { .. } | Annotation::Pixelate { .. } => inside(p, bounds(shape), tol),
    }
}

/// Index of the topmost annotation under `p`.
pub fn hit_test(shapes: &[Annotation], p: Pt, tol: f32, interior: bool) -> Option<usize> {
    shapes.iter().rposition(|s| hit(s, p, tol, interior))
}

/// `shape` moved by `d`.
pub fn translate(shape: &Annotation, d: Pt) -> Annotation {
    let mv = |p: &Pt| (p.0 + d.0, p.1 + d.1);
    let mut out = shape.clone();
    match &mut out {
        Annotation::Arrow { from, to, .. } => {
            *from = mv(from);
            *to = mv(to);
        }
        Annotation::Rect { a, b, .. }
        | Annotation::Ellipse { a, b, .. }
        | Annotation::Pixelate { a, b, .. } => {
            *a = mv(a);
            *b = mv(b);
        }
        Annotation::Pen { points, .. } | Annotation::Highlight { points, .. } => {
            for p in points.iter_mut() {
                *p = mv(p);
            }
        }
        Annotation::Text { pos, .. } => *pos = mv(pos),
    }
    out
}

/// Text never shrinks below this em size (image pixels) when resized.
const MIN_TEXT_EM: f32 = 6.0;

/// The result of dragging `grab` of `original` by `delta`.
pub fn drag(original: &Annotation, grab: Grab, delta: Pt) -> Annotation {
    let handle = match grab {
        Grab::Move => return translate(original, delta),
        Grab::Resize(h) => h,
    };
    let mv = |p: Pt| (p.0 + delta.0, p.1 + delta.1);
    let mut out = original.clone();
    if let Annotation::Arrow { from, to, .. } = &mut out {
        match handle {
            Handle::Start => *from = mv(*from),
            Handle::End => *to = mv(*to),
            _ => {}
        }
        return out;
    }
    let ((x0, y0), (x1, y1)) = bounds(original);
    // New edges; they may cross, which mirrors the shape.
    let (mut l, mut t, mut r, mut b) = (x0, y0, x1, y1);
    match handle {
        Handle::NW => (l, t) = (x0 + delta.0, y0 + delta.1),
        Handle::N => t = y0 + delta.1,
        Handle::NE => (r, t) = (x1 + delta.0, y0 + delta.1),
        Handle::E => r = x1 + delta.0,
        Handle::SE => (r, b) = (x1 + delta.0, y1 + delta.1),
        Handle::S => b = y1 + delta.1,
        Handle::SW => (l, b) = (x0 + delta.0, y1 + delta.1),
        Handle::W => l = x0 + delta.0,
        Handle::Start | Handle::End => return out,
    }
    let map = |p: Pt| -> Pt {
        let fx = if x1 - x0 > f32::EPSILON {
            (p.0 - x0) / (x1 - x0)
        } else {
            0.0
        };
        let fy = if y1 - y0 > f32::EPSILON {
            (p.1 - y0) / (y1 - y0)
        } else {
            0.0
        };
        (l + fx * (r - l), t + fy * (b - t))
    };
    match &mut out {
        Annotation::Rect { a, b: bb, .. }
        | Annotation::Ellipse { a, b: bb, .. }
        | Annotation::Pixelate { a, b: bb, .. } => {
            *a = (l, t);
            *bb = (r, b);
        }
        Annotation::Pen { points, .. } | Annotation::Highlight { points, .. } => {
            for p in points.iter_mut() {
                *p = map(*p);
            }
        }
        Annotation::Text { pos, size, .. } => {
            let (w, h) = (x1 - x0, y1 - y0);
            let sx = if w > f32::EPSILON {
                (r - l).abs() / w
            } else {
                1.0
            };
            let sy = if h > f32::EPSILON {
                (b - t).abs() / h
            } else {
                1.0
            };
            let s = ((sx + sy) / 2.0).max(MIN_TEXT_EM / size.max(f32::EPSILON));
            *size *= s;
            let (nw, nh) = (w * s, h * s);
            // The corner opposite the handle stays put.
            *pos = match handle {
                Handle::NW => (x1 - nw, y1 - nh),
                Handle::NE => (x0, y1 - nh),
                Handle::SW => (x1 - nw, y0),
                _ => (x0, y0),
            };
        }
        Annotation::Arrow { .. } => {}
    }
    out
}

/// Snapshot-based undo/redo of the annotation list.
#[derive(Debug, Default)]
pub struct History {
    undo: Vec<Vec<Annotation>>,
    redo: Vec<Vec<Annotation>>,
}

/// Oldest snapshots are dropped beyond this many.
const HISTORY_LIMIT: usize = 200;

impl History {
    /// Records `before` (the state prior to a change) and clears redo.
    pub fn record(&mut self, before: Vec<Annotation>) {
        if self.undo.len() >= HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.undo.push(before);
        self.redo.clear();
    }

    pub fn undo(&mut self, current: &mut Vec<Annotation>) -> bool {
        let Some(prev) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(current, prev));
        true
    }

    pub fn redo(&mut self, current: &mut Vec<Annotation>) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(current, next));
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: [u8; 3] = [255, 0, 0];

    fn rect(a: Pt, b: Pt) -> Annotation {
        Annotation::Rect {
            a,
            b,
            color: RED,
            width: 4.0,
        }
    }

    fn close(a: Pt, b: Pt) -> bool {
        (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
    }

    #[test]
    fn rect_hits_outline_and_optionally_interior() {
        let r = rect((10.0, 10.0), (110.0, 60.0));
        assert!(hit(&r, (10.0, 30.0), 3.0, false));
        assert!(hit(&r, (60.0, 63.0), 3.0, false), "within tol + half width");
        assert!(!hit(&r, (60.0, 35.0), 3.0, false));
        assert!(hit(&r, (60.0, 35.0), 3.0, true));
        assert!(!hit(&r, (200.0, 35.0), 3.0, true));
    }

    #[test]
    fn ellipse_arrow_pen_text_pixelate_hits() {
        let e = Annotation::Ellipse {
            a: (0.0, 0.0),
            b: (100.0, 50.0),
            color: RED,
            width: 2.0,
        };
        assert!(hit(&e, (50.0, 1.0), 2.0, false));
        assert!(!hit(&e, (50.0, 25.0), 2.0, false));
        assert!(hit(&e, (50.0, 25.0), 2.0, true));
        assert!(!hit(&e, (2.0, 2.0), 2.0, true), "bbox corner is outside");

        let a = Annotation::Arrow {
            from: (0.0, 0.0),
            to: (100.0, 0.0),
            color: RED,
            width: 4.0,
        };
        assert!(hit(&a, (50.0, 3.0), 2.0, false));
        assert!(!hit(&a, (50.0, 10.0), 2.0, false));
        assert!(!hit(&a, (110.0, 0.0), 2.0, false));

        let pen = Annotation::Pen {
            points: vec![(0.0, 0.0), (50.0, 50.0), (100.0, 0.0)],
            color: RED,
            width: 2.0,
        };
        assert!(hit(&pen, (25.0, 26.0), 2.0, false));
        assert!(!hit(&pen, (50.0, 10.0), 2.0, true));

        let text = Annotation::Text {
            pos: (10.0, 10.0),
            text: "Hello".into(),
            color: RED,
            size: 20.0,
        };
        assert!(hit(&text, (20.0, 20.0), 0.0, false));
        assert!(!hit(&text, (5.0, 5.0), 0.0, false));

        let px = Annotation::Pixelate {
            a: (0.0, 0.0),
            b: (40.0, 40.0),
            block: 8,
        };
        assert!(hit(&px, (20.0, 20.0), 0.0, false));
    }

    #[test]
    fn hit_test_prefers_topmost() {
        let shapes = vec![
            rect((0.0, 0.0), (100.0, 100.0)),
            rect((0.0, 0.0), (50.0, 50.0)),
        ];
        assert_eq!(hit_test(&shapes, (0.0, 25.0), 2.0, false), Some(1));
        assert_eq!(hit_test(&shapes, (100.0, 25.0), 2.0, false), Some(0));
        assert_eq!(hit_test(&shapes, (75.0, 75.0), 2.0, false), None);
        assert_eq!(hit_test(&shapes, (75.0, 75.0), 2.0, true), Some(0));
    }

    #[test]
    fn handles_and_handle_at() {
        let r = rect((10.0, 10.0), (110.0, 60.0));
        assert_eq!(handles(&r).len(), 8);
        assert_eq!(handle_at(&r, (111.0, 61.0), 4.0), Some(Handle::SE));
        assert_eq!(handle_at(&r, (60.0, 9.0), 4.0), Some(Handle::N));
        assert_eq!(handle_at(&r, (60.0, 35.0), 4.0), None);
        let a = Annotation::Arrow {
            from: (0.0, 0.0),
            to: (50.0, 50.0),
            color: RED,
            width: 2.0,
        };
        assert_eq!(handle_at(&a, (49.0, 51.0), 4.0), Some(Handle::End));
        let t = Annotation::Text {
            pos: (0.0, 0.0),
            text: "Hi".into(),
            color: RED,
            size: 20.0,
        };
        assert_eq!(handles(&t).len(), 4);
    }

    #[test]
    fn move_translates_every_kind() {
        let d = (5.0, -3.0);
        let r = drag(&rect((10.0, 10.0), (20.0, 20.0)), Grab::Move, d);
        assert_eq!(r, rect((15.0, 7.0), (25.0, 17.0)));
        let pen = Annotation::Highlight {
            points: vec![(0.0, 0.0), (10.0, 10.0)],
            color: RED,
            width: 20.0,
        };
        let moved = drag(&pen, Grab::Move, d);
        assert_eq!(bounds(&moved), ((5.0, -3.0), (15.0, 7.0)));
    }

    #[test]
    fn resize_rect_edges_and_corners() {
        let r = rect((10.0, 10.0), (110.0, 60.0));
        let out = drag(&r, Grab::Resize(Handle::SE), (10.0, 20.0));
        assert_eq!(bounds(&out), ((10.0, 10.0), (120.0, 80.0)));
        let out = drag(&r, Grab::Resize(Handle::W), (-10.0, 99.0));
        assert_eq!(bounds(&out), ((0.0, 10.0), (110.0, 60.0)));
        // Dragging past the opposite edge mirrors instead of collapsing.
        let out = drag(&r, Grab::Resize(Handle::N), (0.0, 70.0));
        assert_eq!(bounds(&out), ((10.0, 60.0), (110.0, 80.0)));
    }

    #[test]
    fn resize_arrow_endpoints() {
        let a = Annotation::Arrow {
            from: (0.0, 0.0),
            to: (50.0, 50.0),
            color: RED,
            width: 2.0,
        };
        let Annotation::Arrow { from, to, .. } = drag(&a, Grab::Resize(Handle::Start), (5.0, 5.0))
        else {
            panic!("kind changed");
        };
        assert!(close(from, (5.0, 5.0)) && close(to, (50.0, 50.0)));
    }

    #[test]
    fn resize_pen_scales_points() {
        let pen = Annotation::Pen {
            points: vec![(0.0, 0.0), (10.0, 5.0), (20.0, 10.0)],
            color: RED,
            width: 2.0,
        };
        let Annotation::Pen { points, .. } = drag(&pen, Grab::Resize(Handle::SE), (20.0, 10.0))
        else {
            panic!("kind changed");
        };
        assert!(close(points[1], (20.0, 10.0)));
        assert!(close(points[2], (40.0, 20.0)));
        assert!(close(points[0], (0.0, 0.0)));
    }

    #[test]
    fn resize_text_scales_uniformly_from_opposite_corner() {
        let t = Annotation::Text {
            pos: (10.0, 10.0),
            text: "Hello".into(),
            color: RED,
            size: 20.0,
        };
        let ((x0, y0), (x1, y1)) = bounds(&t);
        let out = drag(&t, Grab::Resize(Handle::SE), (x1 - x0, y1 - y0));
        let Annotation::Text { pos, size, .. } = &out else {
            panic!("kind changed");
        };
        assert!((size - 40.0).abs() < 0.01, "size {size}");
        assert!(close(*pos, (10.0, 10.0)));
        // Dragging the NW corner keeps the SE corner fixed.
        let out = drag(&t, Grab::Resize(Handle::NW), (-(x1 - x0), -(y1 - y0)));
        let (_, max) = bounds(&out);
        assert!(
            (max.0 - x1).abs() < 0.1 && (max.1 - y1).abs() < 0.1,
            "{max:?}"
        );
        // It never shrinks to nothing.
        let tiny = drag(&t, Grab::Resize(Handle::SE), (-1000.0, -1000.0));
        let Annotation::Text { size, .. } = tiny else {
            panic!("kind changed");
        };
        assert!(size >= MIN_TEXT_EM - 1e-3);
    }

    #[test]
    fn history_undo_redo() {
        let mut h = History::default();
        let mut shapes = vec![];
        assert!(!h.undo(&mut shapes));
        h.record(shapes.clone());
        shapes.push(rect((0.0, 0.0), (10.0, 10.0)));
        h.record(shapes.clone());
        shapes[0] = translate(&shapes[0], (5.0, 5.0));
        assert!(h.undo(&mut shapes));
        assert_eq!(shapes, vec![rect((0.0, 0.0), (10.0, 10.0))]);
        assert!(h.undo(&mut shapes));
        assert!(shapes.is_empty());
        assert!(!h.can_undo() && h.can_redo());
        assert!(h.redo(&mut shapes));
        assert_eq!(shapes.len(), 1);
        h.record(shapes.clone());
        assert!(!h.can_redo(), "a new change clears redo");
    }
}
