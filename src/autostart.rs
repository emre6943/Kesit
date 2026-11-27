//! Launch at login: a LaunchAgent on macOS, an XDG autostart entry on Linux.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[cfg(target_os = "macos")]
const BUNDLE_ID: &str = "org.versesoftware.kesit";

/// Why the running binary is not suitable as a login item, if it is not.
///
/// On macOS the LaunchAgent must start the binary inside `Kesit.app` (the
/// Screen Recording grant belongs to the bundle's signature), and on every
/// platform a binary under a cargo `target/` directory is a dev build that
/// must not be registered.
pub fn unsuitable_reason(exe: &Path) -> Option<String> {
    let parts: Vec<String> = exe
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let in_target = parts
        .windows(2)
        .any(|w| w[0] == "target" && (w[1] == "debug" || w[1] == "release"));
    if cfg!(target_os = "macos") && !in_app_bundle(&parts) {
        return Some(format!("{} is not inside an .app bundle", exe.display()));
    }
    if in_target && !in_app_bundle(&parts) {
        return Some(format!("{} is a cargo build output", exe.display()));
    }
    None
}

/// `…/Something.app/Contents/MacOS/<binary>`
fn in_app_bundle(parts: &[String]) -> bool {
    let n = parts.len();
    n >= 4
        && parts[n - 2] == "MacOS"
        && parts[n - 3] == "Contents"
        && parts[n - 4].ends_with(".app")
}

#[cfg(target_os = "macos")]
fn entry_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("no home directory")?;
    Ok(home
        .join("Library/LaunchAgents")
        .join(format!("{BUNDLE_ID}.plist")))
}

#[cfg(not(target_os = "macos"))]
fn entry_path() -> Result<PathBuf> {
    let config = dirs::config_dir().context("no config directory")?;
    Ok(config.join("autostart/Kesit.desktop"))
}

#[cfg(target_os = "macos")]
fn entry_contents(exe: &Path) -> Result<String> {
    let exe = exe.to_str().context("binary path is not UTF-8")?;
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{BUNDLE_ID}</string>
  <key>AssociatedBundleIdentifiers</key>
  <array><string>{BUNDLE_ID}</string></array>
  <key>ProgramArguments</key>
  <array><string>{}</string></array>
  <key>RunAtLoad</key>
  <true/>
</dict>
</plist>
"#,
        xml_escape(exe)
    ))
}

#[cfg(not(target_os = "macos"))]
fn entry_contents(exe: &Path) -> Result<String> {
    let exe = exe.to_str().context("binary path is not UTF-8")?;
    Ok(format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName=Kesit\n\
         Comment=Screenshot and annotate\nExec={}\nStartupNotify=false\nTerminal=false\n",
        desktop_exec_quote(exe)
    ))
}

#[cfg(any(target_os = "macos", test))]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Quotes one argument of a desktop entry's `Exec` key, per the Desktop
/// Entry Specification: double quotes, a backslash before `"`, `` ` ``, `$`
/// and `\`, then the key-file escaping of backslashes, and `%%` for `%`.
#[cfg(any(not(target_os = "macos"), test))]
fn desktop_exec_quote(arg: &str) -> String {
    let mut quoted = String::from("\"");
    for ch in arg.chars() {
        match ch {
            '"' | '`' | '$' => {
                quoted.push_str("\\\\");
                quoted.push(ch);
            }
            '\\' => quoted.push_str("\\\\\\\\"),
            '%' => quoted.push_str("%%"),
            _ => quoted.push(ch),
        }
    }
    quoted.push('"');
    quoted
}

fn current_exe() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("locating the kesit binary")?;
    Ok(exe.canonicalize().unwrap_or(exe))
}

/// Enables or disables launch at login to match `enabled`.
///
/// Enabling from an unsuitable binary is skipped with a message rather than
/// failing; disabling always works.
pub fn apply(enabled: bool) -> Result<()> {
    let path = entry_path()?;
    if !enabled {
        return match fs::remove_file(&path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                Err(err).with_context(|| format!("removing {}", path.display()))
            }
            _ => Ok(()),
        };
    }
    let exe = current_exe()?;
    if let Some(reason) = unsuitable_reason(&exe) {
        eprintln!("kesit: not enabling launch at login: {reason}");
        return Ok(());
    }
    // Rewritten every time so the entry follows the app if it was moved.
    write_atomically(&path, &entry_contents(&exe)?)
}

fn write_atomically(path: &Path, contents: &str) -> Result<()> {
    let dir = path.parent().context("login item path has no parent")?;
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let tmp = path.with_extension("tmp");
    let mut file = fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
    file.write_all(contents.as_bytes())
        .with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("renaming to {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_detection() {
        let bundled = Path::new("/Applications/Kesit.app/Contents/MacOS/kesit");
        assert!(unsuitable_reason(bundled).is_none());
        let dev = Path::new("/home/u/Kesit/target/release/kesit");
        assert!(unsuitable_reason(dev).is_some());
        let dist = Path::new("/home/u/Kesit/dist/Kesit.app/Contents/MacOS/kesit");
        assert!(unsuitable_reason(dist).is_none());
        let installed = Path::new("/home/u/.local/bin/kesit");
        assert_eq!(
            unsuitable_reason(installed).is_some(),
            cfg!(target_os = "macos")
        );
    }

    #[test]
    fn entry_paths_are_escaped() {
        assert_eq!(
            xml_escape("/A&B/<x>/\"q\".app"),
            "/A&amp;B/&lt;x&gt;/&quot;q&quot;.app"
        );
        assert_eq!(
            desktop_exec_quote("/home/u/my apps/100%/a\"b$c\\d/kesit"),
            r#""/home/u/my apps/100%%/a\\"b\\$c\\\\d/kesit""#
        );
        let contents = entry_contents(Path::new("/x/A B&C/kesit")).expect("entry");
        let expected = if cfg!(target_os = "macos") {
            "<string>/x/A B&amp;C/kesit</string>"
        } else {
            "Exec=\"/x/A B&C/kesit\"\n"
        };
        assert!(contents.contains(expected), "{contents}");
    }
}
