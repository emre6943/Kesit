//! `kesit settings`: a small window that edits the config file. The daemon
//! reloads the file when this process exits (and when its mtime changes).

use anyhow::Result;
use egui::{Color32, Key, Sense, vec2};

use crate::config::{self, Config, STROKE_NAMES, STROKE_PRESETS, SWATCHES};
use crate::gui::{self, DebugShot};
use crate::hotkey;

pub fn run() -> Result<()> {
    let config = Config::load();
    let viewport = egui::ViewportBuilder::default()
        .with_title("Kesit Settings")
        .with_inner_size(vec2(500.0, 620.0))
        .with_min_inner_size(vec2(500.0, 620.0))
        .with_active(!DebugShot::from_env().active())
        .with_resizable(false);
    gui::run("Kesit Settings", viewport, move |_cc| {
        Box::new(Settings::new(config))
    })
}

struct Settings {
    original: Config,
    draft: Config,
    hotkey_text: String,
    recording: bool,
    status: Option<(bool, String)>,
    frames: u32,
    shot: DebugShot,
}

impl Settings {
    fn new(config: Config) -> Self {
        Self {
            hotkey_text: config.hotkey.clone(),
            original: config.clone(),
            draft: config,
            recording: false,
            status: None,
            frames: 0,
            shot: DebugShot::from_env(),
        }
    }

    fn hotkey_error(&self) -> Option<String> {
        hotkey::parse(&self.hotkey_text).err()
    }

    fn save(&mut self) -> bool {
        if let Some(err) = self.hotkey_error() {
            self.status = Some((false, err));
            return false;
        }
        self.draft.hotkey = self.hotkey_text.trim().to_owned();
        match self.draft.save() {
            Ok(path) => {
                self.original = self.draft.clone();
                self.status = Some((true, format!("Saved to {}", path.display())));
                true
            }
            Err(err) => {
                self.status = Some((false, format!("Save failed: {err:#}")));
                false
            }
        }
    }

    /// While recording, the next non-modifier key press becomes the hotkey.
    fn record(&mut self, ctx: &egui::Context) {
        let pressed = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        let Some((key, m)) = pressed else {
            return;
        };
        if key == Key::Escape && !m.any() {
            self.recording = false;
            return;
        }
        let meta = if cfg!(target_os = "macos") {
            m.mac_cmd || m.command
        } else {
            false
        };
        let ctrl = if cfg!(target_os = "macos") {
            m.ctrl
        } else {
            m.ctrl || m.command
        };
        self.hotkey_text = hotkey::format(meta, ctrl, m.alt, m.shift, key_name(key));
        self.recording = false;
    }

    fn form(&mut self, ui: &mut egui::Ui) {
        let dark = ui.visuals().dark_mode;
        section(ui, "Capture", |ui| {
            row(ui, "Hotkey", |ui| {
                let edit = egui::TextEdit::singleline(&mut self.hotkey_text)
                    .desired_width(180.0)
                    .hint_text(config::default_hotkey());
                ui.add_enabled(!self.recording, edit);
                let label = if self.recording {
                    "Press keys…"
                } else {
                    "Record"
                };
                if ui
                    .add(egui::Button::new(label).selected(self.recording))
                    .on_hover_text("Press the new key combination; Esc cancels")
                    .clicked()
                {
                    self.recording = !self.recording;
                }
            });
            if let Some(err) = self.hotkey_error() {
                row(ui, "", |ui| {
                    ui.colored_label(gui::error_color(dark), err);
                });
            }
            row(ui, "Save folder", |ui| {
                let shown = self.draft.save_dir.display().to_string();
                if ui.button("Choose…").clicked()
                    && let Some(dir) = rfd::FileDialog::new()
                        .set_directory(&self.draft.save_dir)
                        .pick_folder()
                {
                    self.draft.save_dir = dir;
                }
                ui.add(egui::Label::new(egui::RichText::new(&shown).weak()).truncate())
                    .on_hover_text(shown);
            });
        });
        section(ui, "After copy and save", |ui| {
            ui.checkbox(
                &mut self.draft.close_after_copy,
                "Close the editor after copying",
            );
            ui.checkbox(
                &mut self.draft.close_after_save,
                "Close the editor after saving",
            );
            let reveal = if cfg!(target_os = "macos") {
                "Show the saved file in Finder"
            } else {
                "Open the folder after saving"
            };
            ui.checkbox(&mut self.draft.reveal_after_save, reveal);
        });
        section(ui, "Annotations", |ui| {
            row(ui, "Colour", |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let current = self.draft.color_rgb();
                for rgb in SWATCHES {
                    if gui::swatch(ui, rgb, rgb == current)
                        .on_hover_text(config::rgb_to_hex(rgb))
                        .clicked()
                    {
                        self.draft.color = config::rgb_to_hex(rgb);
                    }
                }
            });
            row(ui, "Stroke", |ui| {
                let current = self.draft.stroke_index();
                for (index, (width, name)) in
                    STROKE_PRESETS.into_iter().zip(STROKE_NAMES).enumerate()
                {
                    if ui.selectable_label(current == index, name).clicked() {
                        self.draft.stroke_width = width;
                    }
                }
            });
            ui.label(
                egui::RichText::new(
                    "The editor remembers the last tool, colour and stroke you used.",
                )
                .weak()
                .small(),
            );
        });
        section(ui, "General", |ui| {
            ui.checkbox(&mut self.draft.autostart, "Launch Kesit at login");
        });
    }
}

/// Width of the label column in a settings row.
const LABEL_WIDTH: f32 = 92.0;

/// A titled group on a subtle card.
fn section(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    ui.label(egui::RichText::new(title).strong());
    ui.add_space(2.0);
    let fill = if ui.visuals().dark_mode {
        Color32::from_gray(48)
    } else {
        Color32::WHITE
    };
    egui::Frame::NONE
        .fill(fill)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 10.0;
            body(ui);
        });
    ui.add_space(14.0);
}

/// A label column followed by controls.
fn row(ui: &mut egui::Ui, label: &str, controls: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(LABEL_WIDTH, 24.0), Sense::hover());
        ui.painter().text(
            rect.left_center(),
            egui::Align2::LEFT_CENTER,
            label,
            egui::TextStyle::Body.resolve(ui.style()),
            ui.visuals().text_color(),
        );
        controls(ui);
    });
}

/// egui key names that `global-hotkey` understands.
fn key_name(key: Key) -> &'static str {
    match key {
        Key::Space => "Space",
        Key::Enter => "Enter",
        Key::Tab => "Tab",
        Key::Backspace => "Backspace",
        Key::Delete => "Delete",
        Key::Home => "Home",
        Key::End => "End",
        Key::PageUp => "PageUp",
        Key::PageDown => "PageDown",
        Key::ArrowUp => "ArrowUp",
        Key::ArrowDown => "ArrowDown",
        Key::ArrowLeft => "ArrowLeft",
        Key::ArrowRight => "ArrowRight",
        Key::Minus => "Minus",
        Key::Equals => "Equal",
        Key::Comma => "Comma",
        Key::Period => "Period",
        Key::Slash => "Slash",
        Key::Backslash => "Backslash",
        Key::Semicolon => "Semicolon",
        Key::Quote => "Quote",
        Key::Backtick => "Backquote",
        Key::OpenBracket => "BracketLeft",
        Key::CloseBracket => "BracketRight",
        other => other.name(),
    }
}

impl eframe::App for Settings {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.frames < 2 && !self.shot.active() {
            if self.frames == 1 {
                crate::platform::focus_window(frame);
            }
            ctx.request_repaint();
        }
        self.frames = self.frames.saturating_add(1);
        self.shot.tick(&ctx);
        if self.recording {
            self.record(&ctx);
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let dirty = self.draft != self.original || self.hotkey_text != self.original.hotkey;
        let dark = ui.visuals().dark_mode;
        egui::Panel::bottom("buttons")
            .show_separator_line(true)
            .frame(
                egui::Frame::NONE
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(egui::Margin::symmetric(20, 12)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some((ok, msg)) = &self.status {
                        let color = if *ok {
                            ui.visuals().weak_text_color()
                        } else {
                            gui::error_color(dark)
                        };
                        ui.add(
                            egui::Label::new(egui::RichText::new(msg).color(color).small())
                                .truncate(),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let save =
                            egui::Button::new(egui::RichText::new("Save").color(Color32::WHITE))
                                .fill(gui::accent(dark))
                                .min_size(vec2(84.0, 28.0));
                        if ui
                            .add_enabled(dirty, save)
                            .on_hover_text("Save and close")
                            .clicked()
                            && self.save()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        let cancel = egui::Button::new("Cancel").min_size(vec2(84.0, 28.0));
                        if ui
                            .add(cancel)
                            .on_hover_text("Close without saving (Esc)")
                            .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(egui::Margin::symmetric(20, 18)),
            )
            .show(ui, |ui| self.form(ui));
    }
}
