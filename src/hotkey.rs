//! Hotkey strings such as `Cmd+Shift+S`, parsed into `global-hotkey` values.

use global_hotkey::hotkey::{Code, HotKey};

/// Parses and validates a hotkey string.
///
/// Accepts the `global-hotkey` syntax (`Cmd`, `Command`, `Super`, `Ctrl`,
/// `Control`, `Alt`, `Option`, `Shift`, `CmdOrCtrl`, then exactly one key),
/// case-insensitively and with optional spaces around `+`. A hotkey must have
/// at least one modifier, except for the function keys F1–F24.
pub fn parse(s: &str) -> Result<HotKey, String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err("hotkey is empty".into());
    }
    let normalised: String = trimmed
        .split('+')
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("+");
    let hotkey: HotKey = normalised
        .parse()
        .map_err(|e| format!("invalid hotkey {trimmed:?}: {e}"))?;
    if hotkey.mods.is_empty() && !is_function_key(hotkey.key) {
        return Err(format!(
            "hotkey {trimmed:?} needs a modifier (Cmd, Ctrl, Alt or Shift)"
        ));
    }
    Ok(hotkey)
}

fn is_function_key(code: Code) -> bool {
    use Code::*;
    matches!(
        code,
        F1 | F2
            | F3
            | F4
            | F5
            | F6
            | F7
            | F8
            | F9
            | F10
            | F11
            | F12
            | F13
            | F14
            | F15
            | F16
            | F17
            | F18
            | F19
            | F20
            | F21
            | F22
            | F23
            | F24
    )
}

/// Builds a hotkey string from recorded modifiers and a key name, in the
/// order the parser expects (modifiers first). `key` is an egui key name such
/// as `S`, `F5` or `Space`.
pub fn format(meta: bool, ctrl: bool, alt: bool, shift: bool, key: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if meta {
        parts.push(if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Super"
        });
    }
    if ctrl {
        parts.push("Ctrl");
    }
    if alt {
        parts.push(if cfg!(target_os = "macos") {
            "Option"
        } else {
            "Alt"
        });
    }
    if shift {
        parts.push("Shift");
    }
    parts.push(key);
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;
    use global_hotkey::hotkey::Modifiers;

    #[test]
    fn parses_default_hotkeys() {
        let mac = parse("Cmd+Shift+S").expect("mac default");
        assert!(mac.mods == Modifiers::META | Modifiers::SHIFT);
        assert_eq!(mac.key, Code::KeyS);

        let linux = parse("Ctrl+Shift+S").expect("linux default");
        assert!(linux.mods == Modifiers::CONTROL | Modifiers::SHIFT);
        assert_eq!(linux.key, Code::KeyS);

        assert!(parse(crate::config::default_hotkey()).is_ok());
    }

    #[test]
    fn tolerates_case_and_spaces() {
        let h = parse("  cmd + option +  4 ").expect("parse");
        assert!(h.mods == Modifiers::META | Modifiers::ALT);
        assert_eq!(h.key, Code::Digit4);
    }

    #[test]
    fn rejects_invalid() {
        assert!(parse("").is_err());
        assert!(parse("   ").is_err());
        assert!(parse("S").is_err(), "bare letter needs a modifier");
        assert!(parse("Cmd+").is_err());
        assert!(parse("Cmd+Shift").is_err(), "no main key");
        assert!(parse("Cmd+S+Shift").is_err(), "modifier after key");
        assert!(parse("Cmd+A+B").is_err(), "two keys");
        assert!(parse("Hyper+S").is_err(), "unknown modifier");
        assert!(parse("Cmd+Banana").is_err(), "unknown key");
    }

    #[test]
    fn function_key_alone_is_allowed() {
        let h = parse("F13").expect("F13");
        assert_eq!(h.key, Code::F13);
    }

    #[test]
    fn format_round_trips() {
        let s = format(true, false, true, true, "S");
        let h = parse(&s).expect("formatted hotkey parses");
        assert!(h.mods == Modifiers::META | Modifiers::ALT | Modifiers::SHIFT);
        assert_eq!(h.key, Code::KeyS);
        let s = format(false, true, false, false, "F5");
        assert!(parse(&s).is_ok());
    }
}
