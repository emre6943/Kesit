//! The annotation editor shown after a selection (or by `kesit edit`).

mod canvas;
mod toolbar;

use anyhow::Result;
use egui::{Color32, Key, Modifiers, pos2, vec2};
use image::RgbaImage;

use crate::annotate::{self, Annotation, Pt, Tool};
use crate::config::{Config, STROKE_PRESETS};
use crate::export;
use crate::geometry;
use crate::gui::{self, DebugShot};
use crate::platform::{self, ScreenRect};
use crate::shapes::{self, Grab, History};

const TOOLBAR_HEIGHT: f32 = 52.0;
/// Backdrop around the image, in points.
const PAD: f32 = 24.0;
/// The toolbar's width plus a little air; the window never gets narrower.
/// `toolbar::show` measures the real width and grows the window if needed.
const MIN_WINDOW: (f32, f32) = (toolbar::MIN_WIDTH, 360.0);
/// How long a status message stays up, in seconds.
const TOAST_SECS: f64 = 2.5;

pub fn run(image: RgbaImage, image_scale: f32, screen: ScreenRect, config: Config) -> Result<()> {
    let scale = if image_scale.is_finite() && image_scale > 0.0 {
        image_scale
    } else {
        1.0
    };
    let image_points = (image.width() as f32 / scale, image.height() as f32 / scale);
    let (w, h) = geometry::editor_window_size(
        image_points,
        TOOLBAR_HEIGHT,
        PAD,
        (screen.width, screen.height),
        MIN_WINDOW,
    );
    let viewport = egui::ViewportBuilder::default()
        .with_title("Kesit")
        .with_inner_size(vec2(w, h))
        .with_min_inner_size(vec2(MIN_WINDOW.0, MIN_WINDOW.1))
        .with_position(pos2(
            screen.x + (screen.width - w) / 2.0,
            screen.y + (screen.height - h) / 2.0,
        ))
        .with_active(!DebugShot::from_env().active())
        .with_resizable(true);
    gui::run("Kesit", viewport, move |cc| {
        let mut editor = Editor::new(&cc.egui_ctx, image, scale, config);
        editor.screen = Some(screen);
        #[cfg(debug_assertions)]
        editor.debug_demo();
        Box::new(editor)
    })
}

/// Text being typed, in image pixels.
struct TextDraft {
    pos: Pt,
    text: String,
    color: [u8; 3],
    size: f32,
    /// Index of the text annotation being re-edited, hidden while editing.
    editing: Option<usize>,
}

/// A pointer drag in progress.
enum Drag {
    /// A new annotation being drawn.
    Create(Annotation),
    /// Moving or resizing `shapes[index]`, which was `original` at the start.
    Edit {
        index: usize,
        original: Annotation,
        grab: Grab,
        start: Pt,
    },
}

/// GPU copies of the image and of its pixelated version, which is shown
/// inside pixelate annotations and only made once the first one appears.
/// Created from a frame: only then does egui know the renderer's maximum
/// texture size.
struct Textures {
    image: egui::TextureHandle,
    pixelated: Option<egui::TextureHandle>,
}

struct Toast {
    text: String,
    error: bool,
    until: f64,
}

pub(crate) struct Editor {
    image: RgbaImage,
    textures: Option<Textures>,
    highlight_tiles: canvas::HighlightTiles,
    pixel_block: u32,
    /// Physical pixels per logical point at capture time.
    image_scale: f32,
    /// Monitor to centre on; `None` in tests.
    screen: Option<ScreenRect>,
    config: Config,
    tool: Tool,
    color: [u8; 3],
    stroke_index: usize,
    shapes: Vec<Annotation>,
    history: History,
    selected: Option<usize>,
    drag: Option<Drag>,
    draft: Option<TextDraft>,
    toast: Option<Toast>,
    /// Last window width the toolbar asked for; see `toolbar::show`.
    requested_width: Option<f32>,
    frames: u32,
    shot: DebugShot,
}

/// Like `InputState::consume_key`, but the modifiers must match exactly
/// (egui's own matching ignores an extra Shift).
fn consume_exact(i: &mut egui::InputState, modifiers: Modifiers, key: Key) -> bool {
    let before = i.events.len();
    i.events.retain(|e| {
        !matches!(e, egui::Event::Key { key: k, pressed: true, modifiers: m, .. }
            if *k == key && *m == modifiers)
    });
    i.events.len() != before
}

fn color32(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// Drops every `Event::Copy` (egui's Cmd/Ctrl+C) and reports whether there was one.
fn consume_copy(i: &mut egui::InputState) -> bool {
    let before = i.events.len();
    i.events.retain(|e| !matches!(e, egui::Event::Copy));
    i.events.len() != before
}

/// The key that picks `tool` in the editor.
fn tool_key(tool: Tool) -> Key {
    match tool {
        Tool::Select => Key::V,
        Tool::Arrow => Key::A,
        Tool::Rect => Key::R,
        Tool::Ellipse => Key::O,
        Tool::Pen => Key::P,
        Tool::Highlighter => Key::H,
        Tool::Text => Key::T,
        Tool::Pixelate => Key::B,
    }
}

/// "⌘" on macOS, "Ctrl+" elsewhere, for tooltips.
fn command_key_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl+"
    }
}

impl Editor {
    pub(crate) fn new(
        ctx: &egui::Context,
        image: RgbaImage,
        image_scale: f32,
        config: Config,
    ) -> Self {
        let pixel_block = annotate::pixel_block(image_scale);
        // Focus is managed explicitly: the text field keeps it until the
        // editor commits or cancels the text (see `canvas::text_editor`).
        ctx.options_mut(|o| o.input_options.surrender_focus_on = egui::SurrenderFocusOn::Never);
        Self {
            image,
            textures: None,
            highlight_tiles: canvas::HighlightTiles::default(),
            pixel_block,
            image_scale,
            screen: None,
            tool: config.tool(),
            color: config.color_rgb(),
            stroke_index: config.stroke_index(),
            config,
            shapes: Vec::new(),
            history: History::default(),
            selected: None,
            drag: None,
            draft: None,
            toast: None,
            requested_width: None,
            frames: 0,
            shot: DebugShot::from_env(),
        }
    }

    fn stroke_points(&self) -> f32 {
        STROKE_PRESETS[self.stroke_index.min(STROKE_PRESETS.len() - 1)]
    }

    /// Stroke width in image pixels.
    fn stroke_px(&self) -> f32 {
        self.stroke_points() * self.image_scale
    }

    fn text_size_px(&self) -> f32 {
        annotate::text_size_for_stroke(self.stroke_points(), self.image_scale)
    }

    fn highlight_px(&self) -> f32 {
        annotate::highlight_width_for_stroke(self.stroke_points(), self.image_scale)
    }

    fn toast(&mut self, ctx: &egui::Context, text: impl Into<String>, error: bool) {
        let now = ctx.input(|i| i.time);
        self.toast = Some(Toast {
            text: text.into(),
            error,
            until: now + TOAST_SECS,
        });
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(TOAST_SECS));
    }

    /// Applies a change to the annotations as one undo step.
    fn change(&mut self, f: impl FnOnce(&mut Vec<Annotation>)) {
        let before = self.shapes.clone();
        f(&mut self.shapes);
        if self.shapes != before {
            self.history.record(before);
        }
    }

    fn select_tool(&mut self, tool: Tool) {
        self.commit_draft();
        self.tool = tool;
    }

    fn set_color(&mut self, rgb: [u8; 3]) {
        self.color = rgb;
        if let Some(d) = &mut self.draft {
            d.color = rgb;
        } else if let Some(i) = self.selected {
            self.change(|shapes| {
                if let Some(
                    Annotation::Arrow { color, .. }
                    | Annotation::Rect { color, .. }
                    | Annotation::Ellipse { color, .. }
                    | Annotation::Pen { color, .. }
                    | Annotation::Highlight { color, .. }
                    | Annotation::Text { color, .. },
                ) = shapes.get_mut(i)
                {
                    *color = rgb;
                }
            });
        }
    }

    fn set_stroke(&mut self, index: usize) {
        self.stroke_index = index.min(STROKE_PRESETS.len() - 1);
        let (stroke, text, highlight) =
            (self.stroke_px(), self.text_size_px(), self.highlight_px());
        if let Some(d) = &mut self.draft {
            d.size = text;
        } else if let Some(i) = self.selected {
            self.change(|shapes| match shapes.get_mut(i) {
                Some(
                    Annotation::Arrow { width, .. }
                    | Annotation::Rect { width, .. }
                    | Annotation::Ellipse { width, .. }
                    | Annotation::Pen { width, .. },
                ) => *width = stroke,
                Some(Annotation::Highlight { width, .. }) => *width = highlight,
                Some(Annotation::Text { size, .. }) => *size = text,
                _ => {}
            });
        }
    }

    fn start_text(&mut self, pos: Pt) {
        self.draft = Some(TextDraft {
            pos,
            text: String::new(),
            color: self.color,
            size: self.text_size_px(),
            editing: None,
        });
    }

    /// Re-opens the text annotation at `index` for editing.
    fn edit_text(&mut self, index: usize) {
        self.commit_draft();
        if let Some(Annotation::Text {
            pos,
            text,
            color,
            size,
        }) = self.shapes.get(index)
        {
            self.draft = Some(TextDraft {
                pos: *pos,
                text: text.clone(),
                color: *color,
                size: *size,
                editing: Some(index),
            });
            self.selected = None;
        }
    }

    fn commit_draft(&mut self) {
        let Some(d) = self.draft.take() else {
            return;
        };
        let shape = Annotation::Text {
            pos: d.pos,
            text: d.text,
            color: d.color,
            size: d.size,
        };
        let keep = !shape.is_degenerate();
        self.change(|shapes| match d.editing {
            Some(i) if i < shapes.len() => {
                if keep {
                    shapes[i] = shape;
                } else {
                    shapes.remove(i);
                }
            }
            _ if keep => shapes.push(shape),
            _ => {}
        });
    }

    fn delete_selected(&mut self) {
        if let Some(i) = self.selected.take() {
            self.change(|shapes| {
                if i < shapes.len() {
                    shapes.remove(i);
                }
            });
        }
    }

    fn nudge_selected(&mut self, d: Pt) {
        if let Some(i) = self.selected {
            self.change(|shapes| {
                if let Some(s) = shapes.get_mut(i) {
                    *s = shapes::translate(s, d);
                }
            });
        }
    }

    fn undo(&mut self) {
        self.drag = None;
        if self.history.undo(&mut self.shapes) {
            self.selected = None;
        }
    }

    fn redo(&mut self) {
        self.drag = None;
        if self.history.redo(&mut self.shapes) {
            self.selected = None;
        }
    }

    fn rendered(&self) -> Result<RgbaImage> {
        annotate::render(&self.image, &self.shapes)
    }

    fn copy(&mut self, ctx: &egui::Context) {
        self.commit_draft();
        match self.rendered().and_then(|img| export::copy_image(&img)) {
            Ok(()) if self.config.close_after_copy => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Ok(()) => self.toast(ctx, "Copied to the clipboard", false),
            Err(err) => self.toast(ctx, format!("Copy failed: {err:#}"), true),
        }
    }

    fn save(&mut self, ctx: &egui::Context) {
        self.commit_draft();
        let saved = self
            .rendered()
            .and_then(|img| export::save_png(&img, &self.config.save_dir));
        match saved {
            Ok(path) => {
                eprintln!("kesit: saved {}", path.display());
                if self.config.reveal_after_save
                    && let Err(err) = export::reveal(&path)
                {
                    eprintln!("kesit: {err:#}");
                }
                if self.config.close_after_save {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                } else {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    self.toast(ctx, format!("Saved {name}"), false);
                }
            }
            Err(err) => self.toast(ctx, format!("Save failed: {err:#}"), true),
        }
    }

    /// Keyboard shortcuts, handled before the widgets see the input.
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::S)) {
            self.save(ctx);
        } else if self.draft.is_some() {
            self.text_draft_shortcuts(ctx);
        } else {
            self.canvas_shortcuts(ctx);
        }
    }

    /// While typing, every other key belongs to the text field (including
    /// Cmd+C/V/X and its own undo).
    fn text_draft_shortcuts(&mut self, ctx: &egui::Context) {
        // Exact match: Shift+Enter must reach the field as a newline.
        let (esc, enter) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Escape),
                consume_exact(i, Modifiers::NONE, Key::Enter),
            )
        });
        if esc {
            self.draft = None;
        } else if enter {
            self.commit_draft();
        }
    }

    fn canvas_shortcuts(&mut self, ctx: &egui::Context) {
        let command = Modifiers::COMMAND;
        let (copy, esc, enter, redo, undo, delete) = ctx.input_mut(|i| {
            (
                consume_copy(i),
                i.consume_key(Modifiers::NONE, Key::Escape),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(command | Modifiers::SHIFT, Key::Z) || i.consume_key(command, Key::Y),
                i.consume_key(command, Key::Z),
                i.consume_key(Modifiers::NONE, Key::Delete)
                    || i.consume_key(Modifiers::NONE, Key::Backspace),
            )
        });
        if copy || enter {
            self.copy(ctx);
            return;
        }
        if esc {
            if self.selected.take().is_none() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }
        if redo {
            self.redo();
        } else if undo {
            self.undo();
        }
        if delete {
            self.delete_selected();
        }
        if self.selected.is_some() {
            let step = self.image_scale;
            for (key, d) in [
                (Key::ArrowLeft, (-step, 0.0)),
                (Key::ArrowRight, (step, 0.0)),
                (Key::ArrowUp, (0.0, -step)),
                (Key::ArrowDown, (0.0, step)),
            ] {
                // Shift first: a plain pattern also matches Shift+key.
                if ctx.input_mut(|i| i.consume_key(Modifiers::SHIFT, key)) {
                    self.nudge_selected((d.0 * 10.0, d.1 * 10.0));
                }
                if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key)) {
                    self.nudge_selected(d);
                }
            }
        }
        for tool in Tool::ALL {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, tool_key(tool))) {
                self.select_tool(tool);
            }
        }
        for (index, key) in [Key::Num1, Key::Num2, Key::Num3].into_iter().enumerate() {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key)) {
                self.set_stroke(index);
            }
        }
    }

    /// Uploads what this frame paints: the image always, the pixelated copy
    /// once a pixelate annotation exists or is being drawn.
    fn prepare_textures(&mut self, ctx: &egui::Context) {
        let textures = self.textures.get_or_insert_with(|| Textures {
            image: gui::load_texture(ctx, "image", &self.image, egui::TextureOptions::LINEAR),
            pixelated: None,
        });
        let pixelating = matches!(self.drag, Some(Drag::Create(Annotation::Pixelate { .. })))
            || self
                .shapes
                .iter()
                .any(|s| matches!(s, Annotation::Pixelate { .. }));
        if pixelating && textures.pixelated.is_none() {
            let pixelated = annotate::pixelate(&self.image, self.pixel_block);
            textures.pixelated = Some(gui::load_texture(
                ctx,
                "pixelated",
                &pixelated,
                egui::TextureOptions::NEAREST,
            ));
        }
    }

    /// One frame of the editor UI (everything except native window tweaks).
    pub(crate) fn frame_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.shortcuts(&ctx);
        let fill = ui.visuals().panel_fill;
        let border = ui.visuals().widgets.noninteractive.bg_stroke;
        let bar = egui::Panel::top("toolbar")
            .exact_size(TOOLBAR_HEIGHT)
            .show_separator_line(false)
            .frame(
                egui::Frame::NONE
                    .fill(fill)
                    .inner_margin(egui::Margin::symmetric(toolbar::MARGIN, 0)),
            )
            .show(ui, |ui| toolbar::show(self, ui));
        let r = bar.response.rect;
        ui.painter()
            .hline(r.x_range(), r.bottom() - border.width / 2.0, border);
        let backdrop = if ui.visuals().dark_mode {
            Color32::from_gray(24)
        } else {
            Color32::from_gray(222)
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(backdrop))
            .show(ui, |ui| canvas::show(self, ui));
    }

    /// Debug builds only: `KESIT_DEBUG_DEMO=select|text` fills the editor
    /// with sample annotations, to inspect the UI with `KESIT_DEBUG_SHOT`.
    #[cfg(debug_assertions)]
    fn debug_demo(&mut self) {
        let Ok(mode) = std::env::var("KESIT_DEBUG_DEMO") else {
            return;
        };
        let (w, h) = (self.image.width() as f32, self.image.height() as f32);
        let s = self.stroke_px();
        self.shapes = vec![
            Annotation::Pixelate {
                a: (w * 0.1, h * 0.2),
                b: (w * 0.6, h * 0.5),
                block: self.pixel_block,
            },
            Annotation::Highlight {
                points: vec![(w * 0.08, h * 0.7), (w * 0.55, h * 0.7)],
                color: [0xFF, 0xCC, 0x00],
                width: self.highlight_px(),
            },
            Annotation::Arrow {
                from: (w * 0.85, h * 0.85),
                to: (w * 0.62, h * 0.55),
                color: self.color,
                width: s,
            },
            Annotation::Rect {
                a: (w * 0.65, h * 0.1),
                b: (w * 0.92, h * 0.4),
                color: [0x00, 0x7A, 0xFF],
                width: s,
            },
            Annotation::Text {
                pos: (w * 0.08, h * 0.78),
                text: "Kesit".into(),
                color: self.color,
                size: self.text_size_px(),
            },
        ];
        match mode.as_str() {
            "select" => {
                self.tool = Tool::Select;
                self.selected = Some(3);
            }
            "text" => {
                self.tool = Tool::Text;
                self.start_text((w * 0.4, h * 0.82));
                if let Some(d) = &mut self.draft {
                    d.text = "Typing".into();
                }
            }
            _ => {}
        }
    }
}

impl eframe::App for Editor {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.frames < 2 && !self.shot.active() {
            if self.frames == 0
                && let Some(screen) = self.screen
                && !platform::center_window_on(frame, screen)
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            if self.frames == 1 {
                platform::focus_window(frame);
            }
            ctx.request_repaint();
        }
        self.frames = self.frames.saturating_add(1);
        self.shot.tick(&ctx);
        self.frame_ui(ui);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.shot.active() {
            return;
        }
        if let Err(err) = Config::remember_editor_state(self.tool, self.color, self.stroke_points())
        {
            eprintln!("kesit: cannot remember the editor state: {err:#}");
        }
    }
}

#[cfg(test)]
mod tests;
