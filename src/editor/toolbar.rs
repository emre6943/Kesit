//! The editor toolbar: tools, colours, stroke widths, undo/redo, copy/save.

use egui::{Align, Color32, Layout, Rect, Sense, Stroke, StrokeKind, Vec2, vec2};

use super::{Editor, MIN_WINDOW, command_key_label, tool_key};
use crate::annotate::Tool;
use crate::config::{self, STROKE_NAMES, STROKE_PRESETS, SWATCHES};
use crate::gui;
use crate::icons::{self, Icon};

/// Square icon button side, in points.
const BUTTON_SIZE: f32 = 32.0;
/// Gap between buttons of one group.
const GAP: f32 = 2.0;
/// Space on each side of a group separator.
const SEPARATOR_SPACE: f32 = 10.0;
/// Horizontal margin of the toolbar frame (see `Editor::frame_ui`).
pub(super) const MARGIN: i8 = 12;
/// Narrowest window that shows the whole toolbar, with some air; checked
/// against the measured width at runtime (see `show`).
pub(super) const MIN_WIDTH: f32 = 930.0;

pub(super) fn show(ed: &mut Editor, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let dark = ui.visuals().dark_mode;
    let mut left_w = 0.0;
    let mut right_w = 0.0;
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing = vec2(GAP, 0.0);
        let start = ui.cursor().min.x;
        for tool in Tool::ALL {
            let tip = format!("{} ({})", tool.label(), tool_key(tool).name());
            if icon_button(ui, Icon::Tool(tool), ed.tool == tool, true, dark)
                .on_hover_text(tip)
                .clicked()
            {
                ed.select_tool(tool);
            }
        }
        separator(ui);
        for rgb in SWATCHES {
            if gui::swatch(ui, rgb, ed.color == rgb)
                .on_hover_text(format!("Colour {}", config::rgb_to_hex(rgb)))
                .clicked()
            {
                ed.set_color(rgb);
            }
        }
        separator(ui);
        for (index, (width, name)) in STROKE_PRESETS.into_iter().zip(STROKE_NAMES).enumerate() {
            if width_button(ui, width, ed.stroke_index == index)
                .on_hover_text(format!("{name} stroke ({})", index + 1))
                .clicked()
            {
                ed.set_stroke(index);
            }
        }
        separator(ui);
        let (can_undo, can_redo) = (ed.history.can_undo(), ed.history.can_redo());
        if icon_button(ui, Icon::Undo, false, can_undo, dark)
            .on_hover_text(format!("Undo ({}Z)", command_key_label()))
            .clicked()
            && can_undo
        {
            ed.undo();
        }
        let redo_tip = if cfg!(target_os = "macos") {
            "Redo (⇧⌘Z)".to_owned()
        } else {
            "Redo (Ctrl+Shift+Z)".to_owned()
        };
        if icon_button(ui, Icon::Redo, false, can_redo, dark)
            .on_hover_text(redo_tip)
            .clicked()
            && can_redo
        {
            ed.redo();
        }
        left_w = ui.cursor().min.x - start;
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 0.0);
            let right_start = ui.cursor().max.x;
            let save_tip = format!(
                "Save PNG to {} ({}S)",
                ed.config.save_dir.display(),
                command_key_label()
            );
            if action_button(ui, Icon::Save, "Save", false, dark)
                .on_hover_text(save_tip)
                .clicked()
            {
                ed.save(&ctx);
            }
            if action_button(ui, Icon::Copy, "Copy", true, dark)
                .on_hover_text(format!(
                    "Copy to the clipboard ({}C or Enter)",
                    command_key_label()
                ))
                .clicked()
            {
                ed.copy(&ctx);
            }
            right_w = right_start - ui.cursor().max.x;
        });
    });
    // Never crop the toolbar: grow the window if its content does not fit.
    // Each width is requested once; a window manager that refuses it must
    // not turn every frame into a new request (and a repaint).
    let needed = (left_w + right_w + 24.0 + 2.0 * f32::from(MARGIN)).ceil();
    let have = ctx.content_rect().width();
    if needed > have + 0.5 && needed.is_finite() && ed.requested_width != Some(needed) {
        ed.requested_width = Some(needed);
        let height = ctx.content_rect().height();
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(vec2(
            needed,
            MIN_WINDOW.1,
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(needed, height)));
    }
}

fn separator(ui: &mut egui::Ui) {
    ui.add_space(SEPARATOR_SPACE - GAP);
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, 20.0), Sense::hover());
    let color = ui.visuals().widgets.noninteractive.bg_stroke.color;
    ui.painter().rect_filled(rect, 0.0, color);
    ui.add_space(SEPARATOR_SPACE - GAP);
}

/// Background for a hovered or selected toolbar button.
fn button_bg(ui: &egui::Ui, rect: Rect, resp: &egui::Response, selected: bool, dark: bool) {
    let fill = if selected {
        gui::accent(dark)
    } else if resp.is_pointer_button_down_on() {
        ui.visuals().widgets.active.weak_bg_fill
    } else if resp.hovered() {
        ui.visuals().widgets.hovered.weak_bg_fill
    } else {
        return;
    };
    ui.painter().rect_filled(rect, 7.0, fill);
}

fn icon_color(ui: &egui::Ui, selected: bool, enabled: bool) -> Color32 {
    if selected {
        Color32::WHITE
    } else if enabled {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().weak_text_color().gamma_multiply(0.6)
    }
}

fn icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    selected: bool,
    enabled: bool,
    dark: bool,
) -> egui::Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(BUTTON_SIZE), sense);
    if enabled {
        button_bg(ui, rect, &resp, selected, dark);
    }
    if resp.hovered() && enabled {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(18.0));
    icons::paint(
        ui.painter(),
        icon_rect,
        icon,
        icon_color(ui, selected, enabled),
    );
    resp
}

/// Copy / Save: icon plus a short label; `primary` is filled with the accent.
fn action_button(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    primary: bool,
    dark: bool,
) -> egui::Response {
    let font = egui::FontId::proportional(13.5);
    let text_color = if primary {
        Color32::WHITE
    } else {
        ui.visuals().strong_text_color()
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, text_color);
    let size = vec2(12.0 + 16.0 + 6.0 + galley.size().x + 14.0, 30.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let visuals = ui.visuals();
    let fill = if primary {
        let base = gui::accent(dark);
        if resp.is_pointer_button_down_on() {
            base.gamma_multiply(0.8)
        } else if resp.hovered() {
            base.gamma_multiply(0.9)
        } else {
            base
        }
    } else if resp.is_pointer_button_down_on() {
        visuals.widgets.active.weak_bg_fill
    } else if resp.hovered() {
        visuals.widgets.hovered.weak_bg_fill
    } else {
        visuals.widgets.inactive.weak_bg_fill
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 7.0, fill);
    let icon_rect = Rect::from_center_size(
        egui::pos2(rect.min.x + 12.0 + 8.0, rect.center().y),
        Vec2::splat(16.0),
    );
    icons::paint(painter, icon_rect, icon, text_color);
    let text_pos = egui::pos2(
        icon_rect.max.x + 6.0,
        rect.center().y - galley.size().y / 2.0,
    );
    painter.galley(text_pos, galley, text_color);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn width_button(ui: &mut egui::Ui, width: f32, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(BUTTON_SIZE), Sense::click());
    let visuals = ui.visuals();
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 7.0, visuals.widgets.inactive.weak_bg_fill);
        painter.rect_stroke(
            rect,
            7.0,
            Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color),
            StrokeKind::Inside,
        );
    } else if resp.hovered() {
        painter.rect_filled(rect, 7.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let y = rect.center().y;
    let color = if selected {
        visuals.strong_text_color()
    } else {
        visuals.text_color()
    };
    painter.line_segment(
        [
            egui::pos2(rect.left() + 8.0, y),
            egui::pos2(rect.right() - 8.0, y),
        ],
        Stroke::new(width.min(8.0), color),
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}
