//! User configuration, stored as TOML in the platform config directory.
//!
//! A missing or corrupt file never stops the app: every load falls back to the
//! defaults, field by field where possible.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::annotate::Tool;
use crate::hotkey;

/// Stroke widths offered in the editor, in screen points.
pub const STROKE_PRESETS: [f32; 3] = [2.0, 4.0, 7.0];
pub const STROKE_NAMES: [&str; 3] = ["Thin", "Medium", "Thick"];

/// Colour swatches offered in the editor (RGB). The first one is the default.
pub const SWATCHES: [[u8; 3]; 8] = [
    [0xFF, 0x3B, 0x30], // red
    [0xFF, 0x95, 0x00], // orange
    [0xFF, 0xCC, 0x00], // yellow
    [0x34, 0xC7, 0x59], // green
    [0x00, 0x7A, 0xFF], // blue
    [0xAF, 0x52, 0xDE], // purple
    [0x00, 0x00, 0x00], // black
    [0xFF, 0xFF, 0xFF], // white
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Global hotkey, e.g. `Cmd+Shift+S`.
    pub hotkey: String,
    /// Folder that `Save` writes PNGs into.
    pub save_dir: PathBuf,
    /// Start the daemon at login.
    pub autostart: bool,
    /// Annotation colour as `#RRGGBB`; the editor remembers the last one used.
    pub color: String,
    /// Stroke width in screen points; the editor remembers the last one used.
    pub stroke_width: f32,
    /// Editor tool (`arrow`, `rect`, …); the editor remembers the last one used.
    pub tool: String,
    /// Close the editor after copying.
    pub close_after_copy: bool,
    /// Close the editor after saving.
    pub close_after_save: bool,
    /// Reveal a saved file in Finder / the file manager.
    pub reveal_after_save: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotkey: default_hotkey().to_owned(),
            save_dir: default_save_dir(),
            autostart: true,
            color: rgb_to_hex(SWATCHES[0]),
            stroke_width: STROKE_PRESETS[1],
            tool: Tool::Arrow.id().to_owned(),
            close_after_copy: true,
            close_after_save: true,
            reveal_after_save: true,
        }
    }
}

pub fn default_hotkey() -> &'static str {
    if cfg!(target_os = "macos") {
        "Cmd+Shift+S"
    } else {
        "Ctrl+Shift+S"
    }
}

fn default_save_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// `<config dir>/kesit/config.toml`, or `None` when the platform has no config dir.
pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("kesit").join("config.toml"))
}

/// A config file is a few hundred bytes; anything past this is not read.
const MAX_CONFIG_BYTES: u64 = 64 * 1024;

fn read_capped(path: &Path) -> io::Result<String> {
    let mut text = String::new();
    fs::File::open(path)?
        .take(MAX_CONFIG_BYTES)
        .read_to_string(&mut text)?;
    Ok(text)
}

impl Config {
    /// Loads the config from the default location, falling back to defaults.
    pub fn load() -> Self {
        match config_path() {
            Some(path) => Self::load_from(&path),
            None => Self::default(),
        }
    }

    /// Loads the config from `path`. Never fails: a missing file yields the
    /// defaults silently, an unreadable or corrupt one yields the defaults with
    /// a warning on stderr.
    pub fn load_from(path: &Path) -> Self {
        let text = match read_capped(path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Self::default(),
            Err(err) => {
                eprintln!(
                    "kesit: cannot read {}: {err}; using defaults",
                    path.display()
                );
                return Self::default();
            }
        };
        match toml::from_str::<Config>(&text) {
            Ok(cfg) => cfg.sanitized(),
            Err(err) => {
                eprintln!(
                    "kesit: corrupt config {}: {err}; using defaults",
                    path.display()
                );
                Self::default()
            }
        }
    }

    /// Replaces invalid individual values with their defaults.
    pub fn sanitized(mut self) -> Self {
        let defaults = Self::default();
        if hotkey::parse(&self.hotkey).is_err() {
            eprintln!("kesit: invalid hotkey {:?}; using default", self.hotkey);
            self.hotkey = defaults.hotkey;
        }
        if parse_hex_color(&self.color).is_none() {
            eprintln!("kesit: invalid colour {:?}; using default", self.color);
            self.color = defaults.color;
        }
        if !self.stroke_width.is_finite() || !(0.5..=50.0).contains(&self.stroke_width) {
            self.stroke_width = defaults.stroke_width;
        }
        if Tool::from_id(&self.tool).is_none() {
            self.tool = defaults.tool;
        }
        if !self.save_dir.is_absolute() {
            eprintln!(
                "kesit: save_dir {:?} is not absolute; using default",
                self.save_dir
            );
            self.save_dir = defaults.save_dir;
        }
        self
    }

    /// Writes the config to the default location atomically.
    pub fn save(&self) -> Result<PathBuf> {
        let path = config_path().context("no config directory on this platform")?;
        self.save_to(&path)?;
        Ok(path)
    }

    /// Writes the config to `path` via a temporary file and a rename, so a
    /// reader (the daemon) never sees a half-written file.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        let dir = path.parent().context("config path has no parent")?;
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let text = toml::to_string_pretty(self).context("serialising config")?;
        let tmp = path.with_extension("toml.tmp");
        {
            let mut file =
                fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
            file.write_all(text.as_bytes())
                .with_context(|| format!("writing {}", tmp.display()))?;
            file.sync_all().ok();
        }
        fs::rename(&tmp, path).with_context(|| format!("renaming to {}", path.display()))?;
        Ok(())
    }

    /// The configured colour as RGB, falling back to the default swatch.
    pub fn color_rgb(&self) -> [u8; 3] {
        parse_hex_color(&self.color).unwrap_or(SWATCHES[0])
    }

    /// The configured editor tool, falling back to the arrow.
    pub fn tool(&self) -> Tool {
        Tool::from_id(&self.tool).unwrap_or(Tool::Arrow)
    }

    /// Stores the editor's last tool, colour and stroke in the config file.
    /// Re-reads the file first so concurrent edits of other fields survive.
    pub fn remember_editor_state(tool: Tool, color: [u8; 3], stroke_width: f32) -> Result<()> {
        let path = config_path().context("no config directory on this platform")?;
        let mut cfg = Self::load_from(&path);
        let (tool, color) = (tool.id().to_owned(), rgb_to_hex(color));
        if cfg.tool == tool && cfg.color == color && cfg.stroke_width == stroke_width {
            return Ok(());
        }
        cfg.tool = tool;
        cfg.color = color;
        cfg.stroke_width = stroke_width;
        cfg.save_to(&path)
    }

    /// Index into [`STROKE_PRESETS`] closest to the configured width.
    pub fn stroke_index(&self) -> usize {
        nearest_stroke_index(self.stroke_width)
    }
}

pub fn nearest_stroke_index(width: f32) -> usize {
    STROKE_PRESETS
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            (*a - width)
                .abs()
                .partial_cmp(&(*b - width).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
        .unwrap_or(1)
}

/// Parses `#RRGGBB` (the `#` is optional).
pub fn parse_hex_color(s: &str) -> Option<[u8; 3]> {
    let hex = s.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

pub fn rgb_to_hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kesit-test-{}-{name}", std::process::id()));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir.join("config.toml")
    }

    #[test]
    fn missing_file_gives_defaults() {
        let path = temp_file("missing");
        let _ = fs::remove_file(&path);
        assert_eq!(Config::load_from(&path), Config::default());
    }

    #[test]
    fn corrupt_file_gives_defaults() {
        let path = temp_file("corrupt");
        fs::write(&path, "this is = = not [toml").expect("write");
        assert_eq!(Config::load_from(&path), Config::default());
        fs::write(&path, [0xFF, 0xFE, 0x00, 0x41]).expect("write");
        assert_eq!(Config::load_from(&path), Config::default());
    }

    #[test]
    fn partial_file_fills_defaults_and_sanitises() {
        let path = temp_file("partial");
        fs::write(
            &path,
            "autostart = false\ntool = \"laser\"\ncolor = \"not-a-colour\"\nhotkey = \"Banana+Q\"\nstroke_width = 1e9\nsave_dir = \"../elsewhere\"\n",
        )
        .expect("write");
        let cfg = Config::load_from(&path);
        let d = Config::default();
        assert!(!cfg.autostart);
        assert!(cfg.close_after_copy && cfg.close_after_save && cfg.reveal_after_save);
        assert_eq!(cfg.tool, d.tool);
        assert_eq!(cfg.color, d.color);
        assert_eq!(cfg.hotkey, d.hotkey);
        assert_eq!(cfg.stroke_width, d.stroke_width);
        assert_eq!(cfg.save_dir, d.save_dir);
    }

    #[test]
    fn round_trip() {
        let path = temp_file("roundtrip");
        let cfg = Config {
            hotkey: "Ctrl+Alt+K".into(),
            save_dir: PathBuf::from("/tmp/somewhere else"),
            autostart: false,
            color: "#007AFF".into(),
            stroke_width: 7.0,
            tool: "highlighter".into(),
            close_after_copy: false,
            close_after_save: true,
            reveal_after_save: false,
        };
        cfg.save_to(&path).expect("save");
        assert_eq!(Config::load_from(&path), cfg);
    }

    #[test]
    fn defaults_are_valid() {
        let d = Config::default();
        assert_eq!(d.clone().sanitized(), d);
        assert!(d.autostart);
        assert!(d.close_after_copy && d.close_after_save && d.reveal_after_save);
        assert_eq!(d.tool(), Tool::Arrow);
        assert_eq!(d.color_rgb(), SWATCHES[0]);
    }

    #[test]
    fn hex_colours() {
        assert_eq!(parse_hex_color("#FF3B30"), Some([0xFF, 0x3B, 0x30]));
        assert_eq!(parse_hex_color("007aff"), Some([0x00, 0x7A, 0xFF]));
        assert_eq!(parse_hex_color("#12345"), None);
        assert_eq!(parse_hex_color("#GG0000"), None);
        assert_eq!(parse_hex_color("#ÿÿÿ"), None);
        assert_eq!(rgb_to_hex([1, 2, 255]), "#0102FF");
    }

    #[test]
    fn stroke_index_snaps() {
        assert_eq!(nearest_stroke_index(0.1), 0);
        assert_eq!(nearest_stroke_index(4.2), 1);
        assert_eq!(nearest_stroke_index(100.0), 2);
    }
}
