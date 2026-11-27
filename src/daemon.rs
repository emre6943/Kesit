//! `kesit` with no arguments: the resident daemon.
//!
//! It owns no window and no GPU context: a tray icon, the global hotkey and
//! child processes for everything visual. It wakes only on events and on a
//! slow timer that checks the config file's mtime.

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result, anyhow};
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::autostart;
use crate::config::{self, Config};
use crate::hotkey;
use crate::lock;

/// How often the config file's mtime is checked.
const CONFIG_POLL: Duration = Duration::from_secs(5);
/// Lets the tray menu finish closing before the screen is frozen.
const MENU_CAPTURE_DELAY_MS: u64 = 250;

#[derive(Debug)]
enum UserEvent {
    Hotkey,
    Menu(MenuId),
    SettingsExited,
}

struct MenuIds {
    capture: MenuId,
    settings: MenuId,
    quit: MenuId,
}

struct Daemon {
    config: Config,
    manager: GlobalHotKeyManager,
    registered: Option<HotKey>,
    /// Running `kesit capture` children (overlays and editors), reaped on the timer.
    captures: Vec<Child>,
    settings_open: Arc<AtomicBool>,
    config_mtime: Option<SystemTime>,
    proxy: EventLoopProxy<UserEvent>,
    exe: PathBuf,
    tray: Option<TrayIcon>,
    menu: Option<MenuIds>,
}

fn config_mtime() -> Option<SystemTime> {
    config::config_path()
        .and_then(|p| fs::metadata(p).ok())
        .and_then(|m| m.modified().ok())
}

pub fn run() -> Result<()> {
    let Some(lock) = lock::try_lock(&lock::daemon_lock_path()?)? else {
        eprintln!("kesit: already running");
        return Ok(());
    };

    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
        event_loop.set_dock_visibility(false);
    }

    let proxy = event_loop.create_proxy();
    GlobalHotKeyEvent::set_event_handler(Some({
        let proxy = proxy.clone();
        move |e: GlobalHotKeyEvent| {
            if e.state == HotKeyState::Pressed {
                let _ = proxy.send_event(UserEvent::Hotkey);
            }
        }
    }));
    MenuEvent::set_event_handler(Some({
        let proxy = proxy.clone();
        Arc::new(move |e: MenuEvent| {
            let _ = proxy.send_event(UserEvent::Menu(e.id));
        })
    }));

    let manager = GlobalHotKeyManager::new().map_err(|e| anyhow!("hotkey manager: {e}"))?;
    let mut daemon = Daemon {
        config: Config::load(),
        manager,
        registered: None,
        captures: Vec::new(),
        settings_open: Arc::new(AtomicBool::new(false)),
        config_mtime: config_mtime(),
        proxy,
        exe: std::env::current_exe().context("locating the kesit binary")?,
        tray: None,
        menu: None,
    };
    daemon.register_hotkey();
    daemon.apply_autostart();

    event_loop.run(move |event, _, control_flow| {
        let _keep = &lock;
        *control_flow = ControlFlow::WaitUntil(Instant::now() + CONFIG_POLL);
        match event {
            Event::NewEvents(StartCause::Init) => {
                if let Err(err) = daemon.build_tray() {
                    eprintln!("kesit: tray icon unavailable: {err:#}");
                }
            }
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                daemon.reap_captures();
                if config_mtime() != daemon.config_mtime {
                    daemon.reload();
                }
            }
            Event::UserEvent(UserEvent::Hotkey) => daemon.spawn_capture(None),
            Event::UserEvent(UserEvent::SettingsExited) => {
                daemon.settings_open.store(false, Ordering::SeqCst);
                daemon.reload();
            }
            Event::UserEvent(UserEvent::Menu(id)) => {
                let Some(ids) = &daemon.menu else {
                    return;
                };
                if id == ids.capture {
                    daemon.spawn_capture(Some(MENU_CAPTURE_DELAY_MS));
                } else if id == ids.settings {
                    daemon.spawn_settings();
                } else if id == ids.quit {
                    daemon.unregister_hotkey();
                    daemon.tray = None;
                    *control_flow = ControlFlow::Exit;
                }
            }
            _ => {}
        }
    })
}

impl Daemon {
    fn register_hotkey(&mut self) {
        self.unregister_hotkey();
        let parsed = match hotkey::parse(&self.config.hotkey) {
            Ok(parsed) => parsed,
            Err(err) => {
                eprintln!("kesit: {err}; using {}", config::default_hotkey());
                match hotkey::parse(config::default_hotkey()) {
                    Ok(parsed) => parsed,
                    Err(err) => {
                        eprintln!("kesit: default hotkey invalid: {err}");
                        return;
                    }
                }
            }
        };
        match self.manager.register(parsed) {
            Ok(()) => {
                eprintln!("kesit: hotkey {} registered", self.config.hotkey);
                self.registered = Some(parsed);
            }
            Err(err) => eprintln!(
                "kesit: cannot register hotkey {}: {err} (in use by another app?)",
                self.config.hotkey
            ),
        }
    }

    fn unregister_hotkey(&mut self) {
        if let Some(registered) = self.registered.take()
            && let Err(err) = self.manager.unregister(registered)
        {
            eprintln!("kesit: cannot unregister hotkey: {err}");
        }
    }

    fn apply_autostart(&self) {
        if let Err(err) = autostart::apply(self.config.autostart) {
            eprintln!("kesit: {err:#}");
        }
    }

    /// Re-reads the config and applies what changed.
    fn reload(&mut self) {
        self.config_mtime = config_mtime();
        let new = Config::load();
        let hotkey_changed = new.hotkey != self.config.hotkey;
        let autostart_changed = new.autostart != self.config.autostart;
        self.config = new;
        // While the settings window is open the hotkey stays unregistered so
        // the recorder can see it; it is re-registered when the window exits.
        if !self.settings_open.load(Ordering::SeqCst)
            && (hotkey_changed || self.registered.is_none())
        {
            self.register_hotkey();
        }
        if autostart_changed {
            self.apply_autostart();
        }
    }

    /// Drops the handles of finished capture children (reaping the zombies).
    fn reap_captures(&mut self) {
        self.captures
            .retain_mut(|child| matches!(child.try_wait(), Ok(None)));
    }

    /// Starts a capture. Any number of editors may be open; the capture
    /// process itself exits at once if a selection overlay is already up
    /// (see `lock::overlay_lock_path`).
    fn spawn_capture(&mut self, delay_ms: Option<u64>) {
        self.reap_captures();
        let mut cmd = Command::new(&self.exe);
        cmd.arg("capture");
        if let Some(ms) = delay_ms {
            cmd.arg(format!("--delay-ms={ms}"));
        }
        match cmd.spawn() {
            Ok(child) => self.captures.push(child),
            Err(err) => eprintln!("kesit: cannot start capture: {err}"),
        }
    }

    fn spawn_settings(&mut self) {
        if self.settings_open.swap(true, Ordering::SeqCst) {
            return;
        }
        let mut child = match Command::new(&self.exe).arg("settings").spawn() {
            Ok(child) => child,
            Err(err) => {
                eprintln!("kesit: cannot open settings: {err}");
                self.settings_open.store(false, Ordering::SeqCst);
                return;
            }
        };
        self.unregister_hotkey();
        let proxy = self.proxy.clone();
        let spawned = std::thread::Builder::new()
            .name("settings-wait".into())
            .spawn(move || {
                if let Err(err) = child.wait() {
                    eprintln!("kesit: waiting for settings: {err}");
                }
                let _ = proxy.send_event(UserEvent::SettingsExited);
            });
        if let Err(err) = spawned {
            eprintln!("kesit: cannot watch settings: {err}");
            self.settings_open.store(false, Ordering::SeqCst);
            self.register_hotkey();
        }
    }

    fn build_tray(&mut self) -> Result<()> {
        let capture = MenuItem::new("Capture", true, None);
        let settings = MenuItem::new("Settings…", true, None);
        let quit = MenuItem::new("Quit Kesit", true, None);
        let menu = Menu::new();
        menu.append_items(&[
            &capture,
            &PredefinedMenuItem::separator(),
            &settings,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .map_err(|e| anyhow!("building the tray menu: {e}"))?;
        let (rgba, size) = tray_icon_rgba();
        let icon = Icon::from_rgba_with_scale(rgba, size, size, 2.0)
            .map_err(|e| anyhow!("tray icon image: {e}"))?;
        let builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Kesit");
        // On macOS the icon is a template so it follows the menu bar's colour.
        #[cfg(target_os = "macos")]
        let builder = builder.with_icon_templated(icon);
        #[cfg(not(target_os = "macos"))]
        let builder = builder.with_icon(icon);
        let tray = builder
            .build()
            .map_err(|e| anyhow!("creating the tray icon: {e}"))?;
        self.menu = Some(MenuIds {
            capture: capture.id().clone(),
            settings: settings.id().clone(),
            quit: quit.id().clone(),
        });
        self.tray = Some(tray);
        Ok(())
    }
}

/// A 36×36 (18 pt @2x) template icon: two interlocking crop brackets.
fn tray_icon_rgba() -> (Vec<u8>, u32) {
    const SIZE: u32 = 36;
    const STROKE: u32 = 3;
    let bars = [
        (8..8 + STROKE, 2..28),
        (2..28, 8..8 + STROKE),
        (25..25 + STROKE, 8..34),
        (8..34, 25..25 + STROKE),
    ];
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    for (xs, ys) in bars {
        for y in ys {
            for x in xs.clone() {
                let i = ((y * SIZE + x) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&[0, 0, 0, 255]);
            }
        }
    }
    (rgba, SIZE)
}
