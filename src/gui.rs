//! Shared eframe setup and look for the capture, editor and settings windows.

use anyhow::{Result, anyhow};
use egui::{Color32, CornerRadius, Sense, Stroke, Theme, Vec2, vec2};

/// Native options common to every Kesit window: glow renderer, no
/// persistence and (on macOS) the accessory activation policy so no Dock
/// icon appears. egui's clipboard shortcuts stay on: Cmd/Ctrl+C arrives as
/// `Event::Copy` (the editor treats it as "copy image" unless text is being
/// edited) and Cmd/Ctrl+V pastes into text fields.
pub fn native_options(viewport: egui::ViewportBuilder) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        persist_window: false,
        run_and_return: true,
        event_loop_builder: Some(Box::new(|_builder| {
            #[cfg(target_os = "macos")]
            {
                use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
                _builder.with_activation_policy(ActivationPolicy::Accessory);
            }
        })),
        ..Default::default()
    }
}

/// Runs one eframe window to completion. Several can run one after another
/// in the same process (eframe keeps the event loop between runs).
pub fn run<'a>(
    title: &str,
    viewport: egui::ViewportBuilder,
    app: impl FnOnce(&eframe::CreationContext<'_>) -> Box<dyn eframe::App + 'a> + 'a,
) -> Result<()> {
    eframe::run_native(
        title,
        native_options(viewport),
        Box::new(move |cc| {
            apply_style(&cc.egui_ctx);
            Ok(app(cc))
        }),
    )
    .map_err(|e| anyhow!("window error: {e}"))
}

/// Uploads `img`, scaled down for display if a side exceeds the renderer's
/// maximum texture size. Call from a frame, not from the app creator: only
/// then does egui know that maximum.
pub fn load_texture(
    ctx: &egui::Context,
    name: &str,
    img: &image::RgbaImage,
    options: egui::TextureOptions,
) -> egui::TextureHandle {
    let max = ctx.input(|i| i.max_texture_side) as u32;
    let (w, h) = img.dimensions();
    let shown = if w > max || h > max {
        let s = max as f32 / w.max(h) as f32;
        let (sw, sh) = (
            ((w as f32 * s) as u32).clamp(1, max),
            ((h as f32 * s) as u32).clamp(1, max),
        );
        let filter = if options.magnification == egui::TextureFilter::Nearest {
            image::imageops::FilterType::Nearest
        } else {
            image::imageops::FilterType::Triangle
        };
        std::borrow::Cow::Owned(image::imageops::resize(img, sw, sh, filter))
    } else {
        std::borrow::Cow::Borrowed(img)
    };
    let size = [shown.width() as usize, shown.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, shown.as_raw());
    ctx.load_texture(name, color, options)
}

/// macOS system blue, used for selected tools and primary buttons.
pub fn accent(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x0A, 0x84, 0xFF)
    } else {
        Color32::from_rgb(0x00, 0x7A, 0xFF)
    }
}

/// Error text colour.
pub fn error_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0xFF, 0x6B, 0x60)
    } else {
        Color32::from_rgb(0xD7, 0x00, 0x15)
    }
}

/// A round colour swatch, ringed when selected or hovered.
pub fn swatch(ui: &mut egui::Ui, rgb: [u8; 3], selected: bool) -> egui::Response {
    const SIZE: f32 = 24.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(SIZE), Sense::click());
    let painter = ui.painter();
    let center = rect.center();
    let fg = ui.visuals().strong_text_color();
    let ring = SIZE / 2.0 - 1.0;
    if selected {
        painter.circle_stroke(center, ring, Stroke::new(2.0, fg));
    } else if resp.hovered() {
        painter.circle_stroke(center, ring, Stroke::new(1.0, fg.gamma_multiply(0.5)));
    }
    let dot = SIZE / 2.0 - 4.5;
    painter.circle_filled(center, dot, Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
    // Keeps white visible on light and black on dark backgrounds.
    painter.circle_stroke(center, dot, Stroke::new(1.0, Color32::from_black_alpha(40)));
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Spacing, corner radii and colours for both appearances. The window
/// follows the system appearance (egui's default theme preference).
fn apply_style(ctx: &egui::Context) {
    for theme in [Theme::Dark, Theme::Light] {
        let dark = theme == Theme::Dark;
        ctx.style_mut_of(theme, |style| {
            let s = &mut style.spacing;
            s.item_spacing = vec2(8.0, 8.0);
            s.button_padding = vec2(10.0, 5.0);
            s.interact_size.y = 24.0;
            let v = &mut style.visuals;
            let radius = CornerRadius::same(6);
            for w in [
                &mut v.widgets.noninteractive,
                &mut v.widgets.inactive,
                &mut v.widgets.hovered,
                &mut v.widgets.active,
                &mut v.widgets.open,
            ] {
                w.corner_radius = radius;
            }
            v.window_corner_radius = CornerRadius::same(10);
            v.selection.bg_fill = accent(dark);
            v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
            if dark {
                v.panel_fill = Color32::from_gray(40);
                v.window_fill = Color32::from_gray(40);
                v.extreme_bg_color = Color32::from_gray(28);
                v.widgets.inactive.weak_bg_fill = Color32::from_gray(58);
                v.widgets.inactive.bg_fill = Color32::from_gray(58);
                v.widgets.hovered.weak_bg_fill = Color32::from_gray(70);
                v.widgets.hovered.bg_fill = Color32::from_gray(70);
            } else {
                v.panel_fill = Color32::from_gray(246);
                v.window_fill = Color32::from_gray(246);
                v.extreme_bg_color = Color32::from_gray(236);
                v.widgets.inactive.weak_bg_fill = Color32::from_gray(226);
                v.widgets.inactive.bg_fill = Color32::from_gray(226);
                v.widgets.hovered.weak_bg_fill = Color32::from_gray(214);
                v.widgets.hovered.bg_fill = Color32::from_gray(214);
            }
            v.widgets.hovered.bg_stroke = Stroke::NONE;
            v.widgets.inactive.bg_stroke = Stroke::NONE;
        });
    }
    #[cfg(debug_assertions)]
    if let Ok(theme) = std::env::var("KESIT_DEBUG_THEME") {
        match theme.as_str() {
            "light" => ctx.set_theme(egui::ThemePreference::Light),
            "dark" => ctx.set_theme(egui::ThemePreference::Dark),
            _ => {}
        }
    }
}

/// Debug builds only: with `KESIT_DEBUG_SHOT=<file.png>`, a window renders a
/// few frames without taking focus, writes its own frame to that file and
/// closes. Used to inspect the UI without driving the mouse or keyboard.
/// In release builds this is an inert unit struct.
#[cfg(debug_assertions)]
pub struct DebugShot(Option<PendingShot>);

#[cfg(not(debug_assertions))]
pub struct DebugShot;

#[cfg(debug_assertions)]
struct PendingShot {
    path: std::path::PathBuf,
    frames: u32,
    written: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(debug_assertions)]
impl DebugShot {
    /// Frames rendered before the shot, so layout and textures have settled.
    const SETTLE_FRAMES: u32 = 8;

    pub fn from_env() -> Self {
        Self(
            std::env::var_os("KESIT_DEBUG_SHOT").map(|path| PendingShot {
                path: path.into(),
                frames: 0,
                written: Default::default(),
            }),
        )
    }

    /// True when the window must not activate itself or steal focus.
    pub fn active(&self) -> bool {
        self.0.is_some()
    }

    /// Call once per frame; closes the window once the shot is written.
    pub fn tick(&mut self, ctx: &egui::Context) {
        use std::sync::atomic::Ordering;
        let Some(shot) = &mut self.0 else {
            return;
        };
        shot.frames += 1;
        ctx.request_repaint();
        if shot.written.load(Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if shot.frames == Self::SETTLE_FRAMES {
            let (path, written) = (shot.path.clone(), shot.written.clone());
            ctx.request_screenshot(move |img| {
                let [w, h] = img.size;
                let saved = image::RgbaImage::from_raw(w as u32, h as u32, img.as_raw().to_vec())
                    .map(|rgba| rgba.save(&path));
                match saved {
                    Some(Ok(())) => eprintln!("kesit: debug shot {}", path.display()),
                    _ => eprintln!("kesit: debug shot failed"),
                }
                written.store(true, Ordering::SeqCst);
            });
        }
    }
}

#[cfg(not(debug_assertions))]
impl DebugShot {
    pub fn from_env() -> Self {
        Self
    }

    pub fn active(&self) -> bool {
        false
    }

    pub fn tick(&mut self, _ctx: &egui::Context) {}
}

#[cfg(test)]
mod tests {
    #[test]
    fn oversized_textures_are_scaled_to_fit() {
        let ctx = egui::Context::default();
        let max = ctx.input(|i| i.max_texture_side);
        let img = image::RgbaImage::new(max as u32 + 1000, 10);
        let tex = super::load_texture(&ctx, "big", &img, egui::TextureOptions::LINEAR);
        let [w, h] = tex.size();
        assert!(w <= max && h >= 1, "{w}x{h}");
        let small = super::load_texture(
            &ctx,
            "small",
            &image::RgbaImage::new(30, 20),
            egui::TextureOptions::LINEAR,
        );
        assert_eq!(small.size(), [30, 20]);
    }
}
