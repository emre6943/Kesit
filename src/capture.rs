//! `kesit capture`: freeze the monitor under the cursor, let the user drag a
//! selection on a full-screen overlay, then open the editor on the crop.

use anyhow::{Context, Result, anyhow};
use egui::{Color32, FontId, Pos2, Rect, Stroke, StrokeKind, TextureHandle, pos2, vec2};
use image::RgbaImage;

use std::path::Path;

use crate::config::Config;
use crate::editor;
use crate::geometry::{self, CropRect};
use crate::gui;
use crate::lock;
use crate::platform::{self, ScreenRect};

/// A frozen screenshot of one monitor.
pub struct Shot {
    /// Physical pixels.
    pub image: RgbaImage,
    /// The monitor in global logical coordinates.
    pub screen: ScreenRect,
}

/// What the user selected on the overlay.
#[derive(Debug, Clone, Copy)]
pub struct Selection {
    pub crop: CropRect,
    /// Physical image pixels per logical point (2.0 on Retina).
    pub image_scale: f32,
}

pub fn run() -> Result<()> {
    let Some(overlay_lock) = lock::try_lock(&lock::overlay_lock_path()?)? else {
        eprintln!("kesit: a selection is already on screen");
        return Ok(());
    };
    let config = Config::load();
    let shot = grab().context("capturing the screen")?;
    let selection = select(&shot)?;
    drop(overlay_lock);
    let Some(selection) = selection else {
        return Ok(());
    };
    let CropRect {
        x,
        y,
        width,
        height,
    } = selection.crop;
    let cropped = image::imageops::crop_imm(&shot.image, x, y, width, height).to_image();
    let screen = shot.screen;
    drop(shot);
    editor::run(cropped, selection.image_scale, screen, config)
}

/// `kesit edit <image>`: opens the editor on an existing image, centred on
/// the monitor under the cursor and shown at that monitor's scale.
pub fn edit_file(path: &Path) -> Result<()> {
    let config = Config::load();
    let image = read_image(path).with_context(|| format!("reading {}", path.display()))?;
    if image.width() == 0 || image.height() == 0 {
        return Err(anyhow!("{} is empty", path.display()));
    }
    let (screen, scale) = monitor_geometry(&pick_monitor()?)?;
    editor::run(image, scale, screen, config)
}

/// Largest side `kesit edit` accepts; an 8K screenshot is 7680 wide.
const MAX_IMAGE_SIDE: u32 = 16_384;
/// Decoder allocation cap. The editor holds a few copies of the image (the
/// original, the pixelated preview, the export), so this bounds the total.
const MAX_DECODE_BYTES: u64 = 512 * 1024 * 1024;

/// Decodes by content, not extension, with size limits checked before any
/// pixel buffer is allocated.
fn read_image(path: &Path) -> Result<RgbaImage> {
    let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    Ok(reader.decode()?.to_rgba8())
}

fn pick_monitor() -> Result<xcap::Monitor> {
    if let Some((x, y)) = platform::cursor_position() {
        match xcap::Monitor::from_point(x, y) {
            Ok(m) => return Ok(m),
            Err(err) => eprintln!("kesit: no monitor at cursor ({x}, {y}): {err}"),
        }
    }
    let monitors = xcap::Monitor::all().map_err(|e| anyhow!("listing monitors: {e}"))?;
    let primary = monitors
        .iter()
        .position(|m| m.is_primary().unwrap_or(false))
        .unwrap_or(0);
    monitors
        .into_iter()
        .nth(primary)
        .context("no monitors found")
}

fn xcap_err(what: &'static str) -> impl Fn(xcap::XCapError) -> anyhow::Error {
    move |e| anyhow!("{what}: {e}")
}

/// The monitor in global logical coordinates, and its pixels per point.
fn monitor_geometry(monitor: &xcap::Monitor) -> Result<(ScreenRect, f32)> {
    let x = monitor.x().map_err(xcap_err("monitor x"))? as f32;
    let y = monitor.y().map_err(xcap_err("monitor y"))? as f32;
    let w = monitor.width().map_err(xcap_err("monitor width"))? as f32;
    let h = monitor.height().map_err(xcap_err("monitor height"))? as f32;
    let scale = monitor.scale_factor().unwrap_or(1.0).max(0.1);
    // macOS reports monitor bounds in points already; X11 reports pixels.
    let to_logical = if cfg!(target_os = "macos") {
        1.0
    } else {
        scale
    };
    let screen = ScreenRect {
        x: x / to_logical,
        y: y / to_logical,
        width: w / to_logical,
        height: h / to_logical,
    };
    Ok((screen, scale))
}

pub fn grab() -> Result<Shot> {
    let monitor = pick_monitor()?;
    let (screen, _) = monitor_geometry(&monitor)?;
    let image = monitor.capture_image().map_err(xcap_err(
        "capturing the monitor (is Screen Recording permission granted?)",
    ))?;
    if image.width() == 0 || image.height() == 0 {
        return Err(anyhow!("the screenshot is empty"));
    }
    Ok(Shot { image, screen })
}

/// Shows the overlay and returns the selection, or `None` if cancelled.
fn select(shot: &Shot) -> Result<Option<Selection>> {
    let mut result: Option<Selection> = None;
    let s = shot.screen;
    let viewport = egui::ViewportBuilder::default()
        .with_title("Kesit")
        .with_decorations(false)
        .with_resizable(false)
        .with_transparent(false)
        .with_has_shadow(false)
        .with_taskbar(false)
        .with_always_on_top()
        .with_position(pos2(s.x, s.y))
        .with_inner_size(vec2(s.width, s.height));
    {
        let result = &mut result;
        gui::run("Kesit", viewport, move |_cc| {
            Box::new(Overlay {
                shot,
                texture: None,
                start: None,
                current: None,
                dragged: false,
                result,
                setup_frames: 0,
            })
        })?;
    }
    Ok(result)
}

struct Overlay<'a> {
    shot: &'a Shot,
    /// Uploaded on the first frame (see `gui::load_texture`).
    texture: Option<TextureHandle>,
    start: Option<Pos2>,
    current: Option<Pos2>,
    /// Set once the first drag starts; hides the hint for good.
    dragged: bool,
    result: &'a mut Option<Selection>,
    /// Frames shown so far, counted only during native window setup.
    setup_frames: u32,
}

impl Overlay<'_> {
    fn finish(&mut self, ctx: &egui::Context, view: Rect, crop: CropRect) {
        *self.result = Some(Selection {
            crop,
            image_scale: self.shot.image.width() as f32 / view.width().max(1.0),
        });
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn whole_screen(&self) -> CropRect {
        let (width, height) = self.shot.image.dimensions();
        CropRect {
            x: 0,
            y: 0,
            width,
            height,
        }
    }

    fn crop_for(&self, view: Rect, a: Pos2, b: Pos2) -> Option<CropRect> {
        geometry::selection_to_crop(
            (a.x - view.min.x, a.y - view.min.y),
            (b.x - view.min.x, b.y - view.min.y),
            (view.width(), view.height()),
            self.shot.image.dimensions(),
        )
    }
}

impl eframe::App for Overlay<'_> {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.setup_frames < 3 {
            // The native window exists from the first frame on; repeat the
            // setup for a couple of frames in case the window server moved it.
            let focus = self.setup_frames > 0;
            if !platform::setup_overlay_window(frame, self.shot.screen, focus)
                && self.setup_frames == 0
            {
                #[cfg(target_os = "linux")]
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            self.setup_frames += 1;
            ctx.request_repaint();
        }
        self.frame_ui(ui);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 1.0]
    }
}

impl Overlay<'_> {
    /// One overlay frame (everything except native window setup).
    fn frame_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);

        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            *self.result = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        let view = ui.max_rect();
        let (enter, double) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                i.pointer
                    .button_double_clicked(egui::PointerButton::Primary),
            )
        });
        if enter || double {
            let crop = self.whole_screen();
            self.finish(&ctx, view, crop);
            return;
        }
        let texture = self.texture.get_or_insert_with(|| {
            gui::load_texture(
                &ctx,
                "screenshot",
                &self.shot.image,
                egui::TextureOptions::LINEAR,
            )
        });
        let painter = ui.painter();
        painter.image(
            texture.id(),
            view,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );

        let (pressed, down, released, secondary, pos) = ctx.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.pointer.secondary_pressed(),
                i.pointer.latest_pos(),
            )
        });
        if secondary {
            self.start = None;
            self.current = None;
        }
        if pressed {
            self.start = pos;
            self.current = pos;
        } else if down && self.start.is_some() && pos.is_some() {
            self.current = pos;
            if let (Some(a), Some(b)) = (self.start, pos) {
                self.dragged |= a.distance(b) > geometry::MIN_SELECTION_POINTS;
            }
        }
        if released && let (Some(a), Some(b)) = (self.start, pos.or(self.current)) {
            self.start = None;
            self.current = None;
            if let Some(crop) = self.crop_for(view, a, b) {
                self.finish(&ctx, view, crop);
                return;
            }
        }

        if let (Some(a), Some(b)) = (self.start, self.current) {
            let sel = Rect::from_two_pos(a, b).intersect(view);
            dim_outside(painter, view, sel);
            painter.rect_stroke(
                sel,
                0.0,
                Stroke::new(1.0, Color32::WHITE),
                StrokeKind::Outside,
            );
            if let Some(crop) = self.crop_for(view, a, b) {
                size_label(painter, view, sel, crop);
            }
        } else if let Some(p) = pos {
            crosshair(painter, view, p);
        }
        if !self.dragged {
            hint(painter, view);
        }
    }
}

const DIM: Color32 = Color32::from_black_alpha(110);

fn dim_outside(painter: &egui::Painter, view: Rect, sel: Rect) {
    let rects = [
        Rect::from_min_max(view.min, pos2(view.max.x, sel.min.y)),
        Rect::from_min_max(pos2(view.min.x, sel.max.y), view.max),
        Rect::from_min_max(pos2(view.min.x, sel.min.y), pos2(sel.min.x, sel.max.y)),
        Rect::from_min_max(pos2(sel.max.x, sel.min.y), pos2(view.max.x, sel.max.y)),
    ];
    for r in rects {
        if r.is_positive() {
            painter.rect_filled(r, 0.0, DIM);
        }
    }
}

fn crosshair(painter: &egui::Painter, view: Rect, p: Pos2) {
    let stroke = Stroke::new(1.0, Color32::from_white_alpha(90));
    painter.line_segment([pos2(view.min.x, p.y), pos2(view.max.x, p.y)], stroke);
    painter.line_segment([pos2(p.x, view.min.y), pos2(p.x, view.max.y)], stroke);
}

/// The usage hint at the top centre, until the first drag.
fn hint(painter: &egui::Painter, view: Rect) {
    let text = "Drag to select  ·  Enter for full screen  ·  Esc to cancel";
    let galley = painter.layout_no_wrap(text.into(), FontId::proportional(14.0), Color32::WHITE);
    let pad = vec2(14.0, 8.0);
    let size = galley.size() + pad * 2.0;
    let min = pos2(view.center().x - size.x / 2.0, view.min.y + 64.0);
    painter.rect_filled(
        Rect::from_min_size(min, size),
        10.0,
        Color32::from_black_alpha(170),
    );
    painter.galley(min + pad, galley, Color32::WHITE);
}

/// `W × H` in physical pixels, below the selection (or inside it at the bottom
/// edge of the screen).
fn size_label(painter: &egui::Painter, view: Rect, sel: Rect, crop: CropRect) {
    let text = format!("{} × {}", crop.width, crop.height);
    let galley = painter.layout_no_wrap(text, FontId::proportional(13.0), Color32::WHITE);
    let pad = vec2(6.0, 3.0);
    let size = galley.size() + pad * 2.0;
    let mut min = pos2(sel.min.x, sel.max.y + 6.0);
    if min.y + size.y > view.max.y {
        min.y = sel.max.y - size.y - 6.0;
    }
    min.x = min
        .x
        .clamp(view.min.x, (view.max.x - size.x).max(view.min.x));
    let bg = Rect::from_min_size(min, size);
    painter.rect_filled(bg, 4.0, Color32::from_black_alpha(190));
    painter.galley(min + pad, galley, Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Key, Modifiers, PointerButton};

    const VIEW: (f32, f32) = (500.0, 300.0);

    /// Runs the frames and returns the repaint delay egui asked for last.
    fn run_frames(
        overlay: &mut Overlay<'_>,
        ctx: &egui::Context,
        frames: Vec<Vec<Event>>,
    ) -> std::time::Duration {
        let mut delay = std::time::Duration::ZERO;
        for events in frames {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(VIEW.0, VIEW.1))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| overlay.frame_ui(ui));
            out.textures_delta.clear();
            delay = out
                .viewport_output
                .get(&egui::ViewportId::ROOT)
                .map_or(std::time::Duration::ZERO, |v| v.repaint_delay);
        }
        delay
    }

    fn shot() -> Shot {
        Shot {
            image: RgbaImage::new(1000, 600),
            screen: ScreenRect {
                x: 0.0,
                y: 0.0,
                width: VIEW.0,
                height: VIEW.1,
            },
        }
    }

    fn overlay<'a>(shot: &'a Shot, result: &'a mut Option<Selection>) -> Overlay<'a> {
        Overlay {
            shot,
            texture: None,
            start: None,
            current: None,
            dragged: false,
            result,
            setup_frames: 0,
        }
    }

    fn press(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }
    }

    #[test]
    fn enter_selects_the_whole_screen() {
        let ctx = egui::Context::default();
        let shot = shot();
        let mut result = None;
        let mut o = overlay(&shot, &mut result);
        let enter = Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let _ = run_frames(&mut o, &ctx, vec![vec![], vec![enter]]);
        let sel = result.expect("selection");
        assert_eq!(
            (sel.crop.x, sel.crop.y, sel.crop.width, sel.crop.height),
            (0, 0, 1000, 600)
        );
        assert_eq!(sel.image_scale, 2.0);
    }

    #[test]
    fn double_click_selects_the_whole_screen() {
        let ctx = egui::Context::default();
        let shot = shot();
        let mut result = None;
        let mut o = overlay(&shot, &mut result);
        let p = pos2(100.0, 100.0);
        let _ = run_frames(
            &mut o,
            &ctx,
            vec![
                vec![Event::PointerMoved(p)],
                vec![press(p, true)],
                vec![press(p, false)],
                vec![press(p, true)],
                vec![press(p, false)],
            ],
        );
        assert_eq!(result.map(|s| s.crop.width), Some(1000));
    }

    #[test]
    fn drag_selects_a_region_and_hides_the_hint() {
        let ctx = egui::Context::default();
        let shot = shot();
        let mut result = None;
        let mut o = overlay(&shot, &mut result);
        let (a, b) = (pos2(10.0, 20.0), pos2(110.0, 70.0));
        let _ = run_frames(
            &mut o,
            &ctx,
            vec![
                vec![Event::PointerMoved(a)],
                vec![press(a, true)],
                vec![Event::PointerMoved(b)],
            ],
        );
        assert!(o.dragged);
        let _ = run_frames(&mut o, &ctx, vec![vec![press(b, false)]]);
        let sel = result.expect("selection");
        assert_eq!(
            (sel.crop.x, sel.crop.y, sel.crop.width, sel.crop.height),
            (20, 40, 200, 100)
        );
    }

    #[test]
    fn idle_overlay_does_not_repaint() {
        let ctx = egui::Context::default();
        let shot = shot();
        let mut result = None;
        let mut o = overlay(&shot, &mut result);
        let p = pos2(100.0, 100.0);
        let frames = vec![vec![Event::PointerMoved(p)], vec![], vec![], vec![]];
        assert_eq!(run_frames(&mut o, &ctx, frames), std::time::Duration::MAX);
    }

    #[test]
    fn edit_refuses_oversized_images() {
        let dir = std::env::temp_dir().join(format!("kesit-decode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let wide = dir.join("wide.png");
        RgbaImage::new(MAX_IMAGE_SIDE + 1, 1)
            .save(&wide)
            .expect("write");
        assert!(read_image(&wide).is_err());
        let ok = dir.join("ok.png");
        RgbaImage::new(8, 4).save(&ok).expect("write");
        assert_eq!(read_image(&ok).expect("decode").dimensions(), (8, 4));
        let not_png = dir.join("not.png");
        std::fs::write(&not_png, b"not an image").expect("write");
        assert!(read_image(&not_png).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
