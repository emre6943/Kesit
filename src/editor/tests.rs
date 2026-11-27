//! Headless editor tests: real egui frames with synthetic input.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, pos2, vec2};
use image::RgbaImage;

use super::Editor;
use crate::annotate::{Annotation, Tool};
use crate::config::Config;
use crate::shapes;

const SCREEN: (f32, f32) = (900.0, 600.0);

struct Harness {
    ctx: egui::Context,
    ed: Editor,
}

impl Harness {
    /// A 400×200 white image at scale 1; the canvas maps it 1:1, centred.
    fn new() -> Self {
        let ctx = egui::Context::default();
        let img = RgbaImage::from_pixel(400, 200, image::Rgba([255, 255, 255, 255]));
        let ed = Editor::new(&ctx, img, 1.0, Config::default());
        let mut h = Self { ctx, ed };
        let _ = h.frame(vec![]);
        h
    }

    /// Runs one frame and returns how long egui may wait before the next.
    fn frame(&mut self, events: Vec<Event>) -> std::time::Duration {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(SCREEN.0, SCREEN.1))),
            events,
            focused: true,
            ..Default::default()
        };
        let ed = &mut self.ed;
        let mut out = self.ctx.run_ui(input, |ui| ed.frame_ui(ui));
        out.textures_delta.clear();
        out.viewport_output
            .get(&egui::ViewportId::ROOT)
            .map_or(std::time::Duration::ZERO, |v| v.repaint_delay)
    }

    /// Screen position of image pixel `p` (image is centred below the toolbar).
    fn at(&self, p: (f32, f32)) -> Pos2 {
        let top = super::TOOLBAR_HEIGHT;
        let cx = SCREEN.0 / 2.0;
        let cy = top + (SCREEN.1 - top) / 2.0;
        pos2(cx - 200.0 + p.0, cy - 100.0 + p.1)
    }

    fn button(&mut self, pos: Pos2, pressed: bool, modifiers: Modifiers) {
        let _ = self.frame(vec![Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers,
        }]);
    }

    fn click(&mut self, p: (f32, f32)) {
        let pos = self.at(p);
        let _ = self.frame(vec![Event::PointerMoved(pos)]);
        self.button(pos, true, Modifiers::NONE);
        self.button(pos, false, Modifiers::NONE);
    }

    fn drag(&mut self, from: (f32, f32), to: (f32, f32)) {
        let (a, b) = (self.at(from), self.at(to));
        let _ = self.frame(vec![Event::PointerMoved(a)]);
        self.button(a, true, Modifiers::NONE);
        for i in 1..=4 {
            let t = i as f32 / 4.0;
            let _ = self.frame(vec![Event::PointerMoved(a + (b - a) * t)]);
        }
        self.button(b, false, Modifiers::NONE);
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) {
        let _ = self.frame(vec![
            Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            },
            Event::Key {
                key,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers,
            },
        ]);
    }

    fn rect_at(&mut self, a: (f32, f32), b: (f32, f32)) {
        self.ed.tool = Tool::Rect;
        self.drag(a, b);
    }
}

fn text_of(shape: &Annotation) -> Option<&str> {
    match shape {
        Annotation::Text { text, .. } => Some(text),
        _ => None,
    }
}

#[test]
fn typed_text_is_kept() {
    let mut h = Harness::new();
    h.ed.tool = Tool::Text;
    h.click((50.0, 50.0));
    assert!(h.ed.draft.is_some(), "the click opens a text field");
    let _ = h.frame(vec![Event::Text("Hello".into())]);
    let _ = h.frame(vec![Event::Paste(" world".into())]);
    assert_eq!(
        h.ed.draft.as_ref().map(|d| d.text.as_str()),
        Some("Hello world")
    );
    h.key(Key::Enter, Modifiers::NONE);
    assert!(h.ed.draft.is_none());
    assert_eq!(h.ed.shapes.len(), 1);
    assert_eq!(text_of(&h.ed.shapes[0]), Some("Hello world"));
}

#[test]
fn shift_enter_adds_a_line_and_copy_stays_in_the_text_field() {
    let mut h = Harness::new();
    h.ed.tool = Tool::Text;
    h.click((50.0, 50.0));
    let _ = h.frame(vec![Event::Text("a".into())]);
    h.key(Key::Enter, Modifiers::SHIFT);
    let _ = h.frame(vec![Event::Text("b".into()), Event::Copy]);
    assert!(
        h.ed.draft.is_some(),
        "Cmd+C while typing does not copy the image"
    );
    h.click((300.0, 150.0));
    assert_eq!(text_of(&h.ed.shapes[0]), Some("a\nb"));
}

#[test]
fn escape_cancels_text_and_empty_text_is_dropped() {
    let mut h = Harness::new();
    h.ed.tool = Tool::Text;
    h.click((50.0, 50.0));
    let _ = h.frame(vec![Event::Text("gone".into())]);
    h.key(Key::Escape, Modifiers::NONE);
    assert!(h.ed.draft.is_none() && h.ed.shapes.is_empty());
    h.click((50.0, 50.0));
    h.click((300.0, 150.0));
    assert!(h.ed.shapes.is_empty());
}

#[test]
fn double_click_re_edits_text() {
    let mut h = Harness::new();
    h.ed.shapes.push(Annotation::Text {
        pos: (50.0, 50.0),
        text: "Old".into(),
        color: [0, 0, 0],
        size: 24.0,
    });
    h.ed.tool = Tool::Select;
    h.click((60.0, 60.0));
    h.click((60.0, 60.0));
    assert_eq!(h.ed.draft.as_ref().and_then(|d| d.editing), Some(0));
    let _ = h.frame(vec![Event::Text("!".into())]);
    h.key(Key::Enter, Modifiers::NONE);
    assert_eq!(h.ed.shapes.len(), 1);
    assert_eq!(text_of(&h.ed.shapes[0]), Some("Old!"));
    h.key(Key::Z, Modifiers::COMMAND);
    assert_eq!(text_of(&h.ed.shapes[0]), Some("Old"));
}

#[test]
fn draw_select_move_resize_undo() {
    let mut h = Harness::new();
    h.rect_at((20.0, 20.0), (120.0, 80.0));
    assert_eq!(h.ed.shapes.len(), 1);
    // Grab the outline directly with the rectangle tool still active.
    h.drag((20.0, 50.0), (40.0, 60.0));
    assert_eq!(h.ed.shapes.len(), 1, "grabbing moves instead of drawing");
    assert_eq!(h.ed.selected, Some(0));
    assert_eq!(
        shapes::bounds(&h.ed.shapes[0]),
        ((40.0, 30.0), (140.0, 90.0))
    );
    // Resize from the bottom-right handle.
    h.drag((140.0, 90.0), (160.0, 100.0));
    assert_eq!(
        shapes::bounds(&h.ed.shapes[0]),
        ((40.0, 30.0), (160.0, 100.0))
    );
    h.key(Key::Z, Modifiers::COMMAND);
    assert_eq!(
        shapes::bounds(&h.ed.shapes[0]),
        ((40.0, 30.0), (140.0, 90.0))
    );
    h.key(Key::Z, Modifiers::COMMAND);
    assert_eq!(
        shapes::bounds(&h.ed.shapes[0]),
        ((20.0, 20.0), (120.0, 80.0))
    );
    h.key(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(
        shapes::bounds(&h.ed.shapes[0]),
        ((40.0, 30.0), (140.0, 90.0))
    );
}

#[test]
fn select_tool_grabs_interior_and_clicking_empty_deselects() {
    let mut h = Harness::new();
    h.rect_at((20.0, 20.0), (120.0, 80.0));
    h.ed.tool = Tool::Select;
    h.click((70.0, 50.0));
    assert_eq!(h.ed.selected, Some(0));
    h.click((300.0, 180.0));
    assert_eq!(h.ed.selected, None);
    assert_eq!(h.ed.shapes.len(), 1, "the select tool never draws");
}

#[test]
fn nudge_and_delete() {
    let mut h = Harness::new();
    h.rect_at((20.0, 20.0), (120.0, 80.0));
    h.ed.tool = Tool::Select;
    h.click((70.0, 50.0));
    h.key(Key::ArrowRight, Modifiers::NONE);
    h.key(Key::ArrowDown, Modifiers::SHIFT);
    assert_eq!(
        shapes::bounds(&h.ed.shapes[0]),
        ((21.0, 30.0), (121.0, 90.0))
    );
    h.key(Key::Backspace, Modifiers::NONE);
    assert!(h.ed.shapes.is_empty() && h.ed.selected.is_none());
    h.key(Key::Z, Modifiers::COMMAND);
    assert_eq!(
        shapes::bounds(&h.ed.shapes[0]),
        ((21.0, 30.0), (121.0, 90.0))
    );
}

#[test]
fn colour_change_applies_to_selection_as_one_step() {
    let mut h = Harness::new();
    h.rect_at((20.0, 20.0), (120.0, 80.0));
    h.ed.tool = Tool::Select;
    h.click((70.0, 50.0));
    h.ed.set_color([0, 0, 255]);
    assert!(matches!(
        h.ed.shapes[0],
        Annotation::Rect {
            color: [0, 0, 255],
            ..
        }
    ));
    h.key(Key::Z, Modifiers::COMMAND);
    assert!(!matches!(
        h.ed.shapes[0],
        Annotation::Rect {
            color: [0, 0, 255],
            ..
        }
    ));
}

#[test]
fn tool_shortcuts() {
    let mut h = Harness::new();
    for (key, tool) in [
        (Key::V, Tool::Select),
        (Key::H, Tool::Highlighter),
        (Key::B, Tool::Pixelate),
        (Key::T, Tool::Text),
    ] {
        h.key(key, Modifiers::NONE);
        assert_eq!(h.ed.tool, tool);
    }
    // While typing, letters are text, not shortcuts.
    h.click((50.0, 50.0));
    let _ = h.frame(vec![Event::Text("v".into())]);
    assert_eq!(h.ed.tool, Tool::Text);
}

#[test]
fn highlighter_and_pixelate_draw() {
    let mut h = Harness::new();
    h.ed.tool = Tool::Highlighter;
    h.drag((20.0, 150.0), (200.0, 150.0));
    h.ed.tool = Tool::Pixelate;
    h.drag((250.0, 20.0), (350.0, 80.0));
    assert!(matches!(h.ed.shapes[0], Annotation::Highlight { .. }));
    assert!(matches!(h.ed.shapes[1], Annotation::Pixelate { .. }));
}

#[test]
fn idle_editor_does_not_repaint() {
    let mut h = Harness::new();
    h.rect_at((20.0, 20.0), (120.0, 80.0));
    for _ in 0..3 {
        let _ = h.frame(vec![]);
    }
    assert_eq!(h.frame(vec![]), std::time::Duration::MAX);
}

#[test]
fn pixelated_texture_is_made_on_first_use() {
    let mut h = Harness::new();
    let has_pixelated = |h: &Harness| {
        h.ed.textures
            .as_ref()
            .is_some_and(|t| t.pixelated.is_some())
    };
    assert!(!has_pixelated(&h));
    h.ed.tool = Tool::Pixelate;
    h.drag((250.0, 20.0), (350.0, 80.0));
    assert!(has_pixelated(&h));
}
