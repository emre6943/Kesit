//! Kesit: a lightweight screenshot and annotation tool.
//!
//! `kesit` runs the tray daemon, `kesit capture` takes a screenshot and opens
//! the editor, `kesit edit <image>` opens the editor on a file, `kesit
//! settings` opens the settings window.

mod annotate;
mod autostart;
mod capture;
mod config;
mod daemon;
mod editor;
mod export;
mod geometry;
mod gui;
mod hotkey;
mod icons;
mod lock;
mod platform;
mod settings;
mod shapes;

use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str = "\
Usage: kesit [COMMAND]

Commands:
  (none)     Run the tray daemon (global hotkey, tray menu)
  capture    Capture the monitor under the cursor and annotate a selection
  settings   Open the settings window
  edit FILE  Open the editor on an existing image

Options:
  -h, --help     Show this help
  -V, --version  Show the version";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None => daemon::run(),
        Some("capture") => {
            let delay = args
                .iter()
                .skip(1)
                .find_map(|a| a.strip_prefix("--delay-ms="))
                .and_then(|ms| ms.parse::<u64>().ok());
            if let Some(ms) = delay {
                std::thread::sleep(Duration::from_millis(ms.min(10_000)));
            }
            capture::run()
        }
        Some("settings") => settings::run(),
        Some("edit") => match args.get(1) {
            Some(path) => capture::edit_file(std::path::Path::new(path)),
            None => Err(anyhow::anyhow!("edit needs an image path")),
        },
        #[cfg(target_os = "linux")]
        Some("__clipboard-hold") => export::linux_hold::run(),
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some("-V" | "--version") => {
            println!("kesit {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(other) => {
            eprintln!("kesit: unknown command {other:?}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("kesit: {err:#}");
            ExitCode::FAILURE
        }
    }
}
