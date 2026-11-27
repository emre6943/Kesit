//! Small platform shims: cursor position and native window tweaks.

/// A monitor rectangle in the global logical coordinate space that `xcap`
/// and the window system share (top-left origin; points on macOS).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(target_os = "macos")]
mod macos {
    use super::ScreenRect;
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_app_kit::{
        NSApplication, NSEvent, NSScreen, NSView, NSWindow, NSWindowCollectionBehavior,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    /// `NSScreenSaverWindowLevel`: above the menu bar (24) and the Dock (20).
    const OVERLAY_WINDOW_LEVEL: isize = 1000;

    /// Height of the primary screen, which anchors Cocoa's bottom-left origin.
    fn primary_screen_height(mtm: MainThreadMarker) -> Option<f64> {
        let screens = NSScreen::screens(mtm);
        screens.firstObject().map(|s| s.frame().size.height)
    }

    /// Cursor position in CoreGraphics global coordinates (top-left origin,
    /// points), which is what `xcap::Monitor::from_point` expects.
    pub fn cursor_position() -> Option<(i32, i32)> {
        let mtm = MainThreadMarker::new()?;
        let p = NSEvent::mouseLocation();
        let h = primary_screen_height(mtm)?;
        Some((p.x.floor() as i32, (h - p.y).floor() as i32))
    }

    /// The window's NSWindow. Taking `MainThreadMarker` proves we are on
    /// the main thread, the only one allowed to touch AppKit views.
    fn ns_window(_mtm: MainThreadMarker, frame: &eframe::Frame) -> Option<Retained<NSWindow>> {
        let window = frame.winit_window()?;
        let handle = window.window_handle().ok()?;
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            return None;
        };
        // SAFETY: `ns_view` points at this window's live NSView while `handle`
        // borrows the window, and the reference does not outlive this call.
        let view: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
        view.window()
    }

    /// Converts a top-left-origin rectangle into a Cocoa frame.
    fn to_cocoa(rect: ScreenRect, primary_h: f64) -> NSRect {
        NSRect::new(
            NSPoint::new(
                rect.x as f64,
                primary_h - rect.y as f64 - rect.height as f64,
            ),
            NSSize::new(rect.width as f64, rect.height as f64),
        )
    }

    /// Makes the overlay cover `screen` exactly, above the menu bar and Dock,
    /// on every Space (including full-screen apps). With `focus`, also
    /// activates the app and makes the overlay key (done from the second
    /// frame on, once eframe has shown the painted window). Returns false
    /// when the native window could not be reached.
    pub fn setup_overlay_window(frame: &eframe::Frame, screen: ScreenRect, focus: bool) -> bool {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let (Some(window), Some(primary_h)) = (ns_window(mtm, frame), primary_screen_height(mtm))
        else {
            return false;
        };
        window.setLevel(OVERLAY_WINDOW_LEVEL);
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        window.setHasShadow(false);
        window.setFrame_display(to_cocoa(screen, primary_h), true);
        if focus {
            activate(mtm, &window);
        }
        true
    }

    /// Centres a normal window on `screen`.
    pub fn center_window_on(frame: &eframe::Frame, screen: ScreenRect) -> bool {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let (Some(window), Some(primary_h)) = (ns_window(mtm, frame), primary_screen_height(mtm))
        else {
            return false;
        };
        let size = window.frame().size;
        let target = ScreenRect {
            x: screen.x + (screen.width - size.width as f32) / 2.0,
            y: screen.y + (screen.height - size.height as f32) / 2.0,
            width: size.width as f32,
            height: size.height as f32,
        };
        window.setFrame_display(to_cocoa(target, primary_h), true);
        true
    }

    /// Brings a window of this (accessory) app to the front with key focus.
    pub fn focus_window(frame: &eframe::Frame) {
        if let Some(mtm) = MainThreadMarker::new()
            && let Some(window) = ns_window(mtm, frame)
        {
            activate(mtm, &window);
        }
    }

    fn activate(mtm: MainThreadMarker, window: &NSWindow) {
        NSApplication::sharedApplication(mtm).activate();
        window.makeKeyAndOrderFront(None);
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::ScreenRect;

    /// Cursor position in X11 root coordinates. `None` on Wayland, where
    /// clients cannot read the global pointer position.
    pub fn cursor_position() -> Option<(i32, i32)> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::ConnectionExt;
        // Wayland-only sessions have no X display to ask.
        std::env::var_os("DISPLAY")?;
        let (conn, screen_num) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots.get(screen_num)?.root;
        let reply = conn.query_pointer(root).ok()?.reply().ok()?;
        Some((i32::from(reply.root_x), i32::from(reply.root_y)))
    }

    /// The window was created at the monitor's position; full-screening it
    /// there (via a viewport command) is enough on X11/Wayland.
    pub fn setup_overlay_window(_frame: &eframe::Frame, _screen: ScreenRect, _focus: bool) -> bool {
        false
    }

    pub fn center_window_on(_frame: &eframe::Frame, _screen: ScreenRect) -> bool {
        false
    }

    pub fn focus_window(_frame: &eframe::Frame) {}
}
