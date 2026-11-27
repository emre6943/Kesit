//! Pure geometry: selection to physical crop, window sizing.

/// Selections smaller than this (in logical points, either side) are treated
/// as clicks and ignored.
pub const MIN_SELECTION_POINTS: f32 = 4.0;

/// A crop rectangle in physical image pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Converts a drag between two logical points (relative to the overlay's
/// top-left) into a crop of the physical screenshot.
///
/// The overlay shows the whole screenshot stretched over `view` logical
/// points, so the logical-to-physical ratio is taken from the image and view
/// sizes rather than trusted from the reported scale factor. The selection is
/// clamped to the view; outer edges are rounded outwards so a selection never
/// loses a partially covered pixel. Returns `None` for tiny selections or a
/// degenerate view.
pub fn selection_to_crop(
    a: (f32, f32),
    b: (f32, f32),
    view: (f32, f32),
    image: (u32, u32),
) -> Option<CropRect> {
    let (view_w, view_h) = view;
    let (img_w, img_h) = image;
    if !(view_w > 0.0 && view_h > 0.0) || img_w == 0 || img_h == 0 {
        return None;
    }
    let clamp_x = |v: f32| v.clamp(0.0, view_w);
    let clamp_y = |v: f32| v.clamp(0.0, view_h);
    let (x0, x1) = min_max(clamp_x(a.0), clamp_x(b.0));
    let (y0, y1) = min_max(clamp_y(a.1), clamp_y(b.1));
    if !(x1 - x0 >= MIN_SELECTION_POINTS && y1 - y0 >= MIN_SELECTION_POINTS) {
        return None;
    }
    let sx = img_w as f32 / view_w;
    let sy = img_h as f32 / view_h;
    let px0 = ((x0 * sx).floor() as u32).min(img_w - 1);
    let py0 = ((y0 * sy).floor() as u32).min(img_h - 1);
    let px1 = ((x1 * sx).ceil() as u32).clamp(px0 + 1, img_w);
    let py1 = ((y1 * sy).ceil() as u32).clamp(py0 + 1, img_h);
    Some(CropRect {
        x: px0,
        y: py0,
        width: px1 - px0,
        height: py1 - py0,
    })
}

fn min_max(a: f32, b: f32) -> (f32, f32) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Scales `content` down (never up) so it fits inside `max`, keeping the
/// aspect ratio. Returns the scale factor applied.
pub fn fit_scale(content: (f32, f32), max: (f32, f32)) -> f32 {
    if !(content.0 > 0.0 && content.1 > 0.0) {
        return 1.0;
    }
    (max.0 / content.0).min(max.1 / content.1).clamp(0.0, 1.0)
}

/// Editor window inner size for an image of `image_points` logical points
/// shown with `pad` points of backdrop on every side below a toolbar of
/// `toolbar_h`. The image is scaled down (never up) to fit 90% of `screen`;
/// the window is never smaller than `min` (whose width is the toolbar's).
pub fn editor_window_size(
    image_points: (f32, f32),
    toolbar_h: f32,
    pad: f32,
    screen: (f32, f32),
    min: (f32, f32),
) -> (f32, f32) {
    let max = (
        screen.0 * 0.9 - 2.0 * pad,
        screen.1 * 0.9 - toolbar_h - 2.0 * pad,
    );
    let s = fit_scale(image_points, (max.0.max(1.0), max.1.max(1.0)));
    let w = (image_points.0 * s + 2.0 * pad).max(min.0);
    let h = (image_points.1 * s + 2.0 * pad + toolbar_h).max(min.1);
    (w.round(), h.round())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retina_doubles() {
        let c = selection_to_crop((10.0, 20.0), (110.0, 70.0), (1512.0, 982.0), (3024, 1964))
            .expect("crop");
        assert_eq!(
            c,
            CropRect {
                x: 20,
                y: 40,
                width: 200,
                height: 100
            }
        );
    }

    #[test]
    fn non_retina_identity_and_reversed_drag() {
        let c = selection_to_crop(
            (300.0, 200.0),
            (100.0, 50.0),
            (1920.0, 1080.0),
            (1920, 1080),
        )
        .expect("crop");
        assert_eq!(
            c,
            CropRect {
                x: 100,
                y: 50,
                width: 200,
                height: 150
            }
        );
    }

    #[test]
    fn fractional_points_round_outwards() {
        let c = selection_to_crop((10.3, 10.3), (20.6, 20.6), (100.0, 100.0), (200, 200))
            .expect("crop");
        assert_eq!(c.x, 20);
        assert_eq!(c.y, 20);
        assert_eq!(c.width, 42 - 20);
        assert_eq!(c.height, 42 - 20);
    }

    #[test]
    fn clamps_to_screen() {
        let c = selection_to_crop(
            (-50.0, -50.0),
            (5000.0, 5000.0),
            (1512.0, 982.0),
            (3024, 1964),
        )
        .expect("crop");
        assert_eq!(
            c,
            CropRect {
                x: 0,
                y: 0,
                width: 3024,
                height: 1964
            }
        );
        let c = selection_to_crop(
            (1500.0, 970.0),
            (1600.0, 1100.0),
            (1512.0, 982.0),
            (3024, 1964),
        )
        .expect("crop at edge");
        assert_eq!(c.x + c.width, 3024);
        assert_eq!(c.y + c.height, 1964);
    }

    #[test]
    fn tiny_selections_ignored() {
        assert!(
            selection_to_crop((10.0, 10.0), (10.0, 10.0), (100.0, 100.0), (200, 200)).is_none()
        );
        assert!(
            selection_to_crop((10.0, 10.0), (12.0, 80.0), (100.0, 100.0), (200, 200)).is_none()
        );
        assert!(
            selection_to_crop((10.0, 10.0), (80.0, 13.9), (100.0, 100.0), (200, 200)).is_none()
        );
        // Entirely off-screen collapses to a sliver and is ignored.
        assert!(
            selection_to_crop((-50.0, -50.0), (-10.0, -10.0), (100.0, 100.0), (200, 200)).is_none()
        );
        assert!(
            selection_to_crop((10.0, 10.0), (14.0, 14.0), (100.0, 100.0), (200, 200)).is_some()
        );
    }

    #[test]
    fn degenerate_inputs() {
        assert!(selection_to_crop((0.0, 0.0), (50.0, 50.0), (0.0, 100.0), (200, 200)).is_none());
        assert!(selection_to_crop((0.0, 0.0), (50.0, 50.0), (100.0, 100.0), (0, 200)).is_none());
        assert!(
            selection_to_crop((f32::NAN, 0.0), (50.0, 50.0), (100.0, 100.0), (200, 200)).is_none()
        );
    }

    #[test]
    fn window_sizing() {
        // Small crop: the window is as wide as the toolbar, never narrower.
        assert_eq!(
            editor_window_size((100.0, 50.0), 52.0, 24.0, (1512.0, 982.0), (760.0, 360.0)),
            (760.0, 360.0)
        );
        // Medium crop: shown 1:1 with padding on every side.
        assert_eq!(
            editor_window_size((900.0, 500.0), 52.0, 24.0, (1512.0, 982.0), (760.0, 360.0)),
            (948.0, 600.0)
        );
        // Huge crop: scaled down to fit 90% of the screen, padding included.
        let (w, h) = editor_window_size(
            (3000.0, 2000.0),
            52.0,
            24.0,
            (1512.0, 982.0),
            (760.0, 360.0),
        );
        assert!(w <= 1512.0 * 0.9 + 0.5 && h <= 982.0 * 0.9 + 0.5, "{w}x{h}");
        // The toolbar minimum wins even on a tiny screen.
        let (w, _) = editor_window_size((10.0, 10.0), 52.0, 24.0, (600.0, 400.0), (760.0, 360.0));
        assert_eq!(w, 760.0);
        assert!(fit_scale((3000.0, 2000.0), (100.0, 100.0)) < 0.05);
        assert_eq!(fit_scale((10.0, 10.0), (100.0, 100.0)), 1.0);
    }
}
