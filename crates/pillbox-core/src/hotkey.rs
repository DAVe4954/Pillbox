//! Global shortcut text such as `"Ctrl+Alt+N"`: parsing, validation and the canonical spelling.
//!
//! The settings file stores shortcuts as text so they can be edited by hand. Parsing is strict
//! (an unknown key name is an error, not a guess) and the result knows the Win32 modifier flags
//! and virtual-key code `RegisterHotKey` wants, without this crate depending on Windows.

use std::fmt;

/// A key combination: modifiers plus exactly one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    /// Windows virtual-key code.
    pub vk: u16,
}

/// Key names accepted after the modifiers, with their virtual-key codes. The first spelling of
/// each code is the canonical one used when writing a shortcut back out.
const NAMED: &[(&str, u16)] = &[
    ("Space", 0x20),
    ("Enter", 0x0D),
    ("Return", 0x0D),
    ("Tab", 0x09),
    ("Esc", 0x1B),
    ("Escape", 0x1B),
    ("Backspace", 0x08),
    ("Delete", 0x2E),
    ("Del", 0x2E),
    ("Insert", 0x2D),
    ("Ins", 0x2D),
    ("Home", 0x24),
    ("End", 0x23),
    ("PageUp", 0x21),
    ("PgUp", 0x21),
    ("PageDown", 0x22),
    ("PgDn", 0x22),
    ("Left", 0x25),
    ("Up", 0x26),
    ("Right", 0x27),
    ("Down", 0x28),
    ("PrintScreen", 0x2C),
    ("PrtSc", 0x2C),
    ("Pause", 0x13),
    ("NumMultiply", 0x6A),
    ("NumAdd", 0x6B),
    ("NumSubtract", 0x6D),
    ("NumDecimal", 0x6E),
    ("NumDivide", 0x6F),
    // Punctuation, as on a US layout (the codes are layout-independent "OEM" keys).
    (";", 0xBA),
    ("Plus", 0xBB),
    ("=", 0xBB),
    (",", 0xBC),
    ("-", 0xBD),
    ("Minus", 0xBD),
    (".", 0xBE),
    ("/", 0xBF),
    ("`", 0xC0),
    ("[", 0xDB),
    ("\\", 0xDC),
    ("]", 0xDD),
    ("'", 0xDE),
    // Media and volume keys.
    ("MediaPlayPause", 0xB3),
    ("MediaNext", 0xB0),
    ("MediaPrevious", 0xB1),
    ("MediaStop", 0xB2),
    ("VolumeMute", 0xAD),
    ("VolumeDown", 0xAE),
    ("VolumeUp", 0xAF),
];

/// Win32 `MOD_*` flags.
pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;

fn key_code(name: &str) -> Option<u16> {
    let upper = name.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    // Single letter or digit.
    if bytes.len() == 1 && (bytes[0].is_ascii_uppercase() || bytes[0].is_ascii_digit()) {
        return Some(u16::from(bytes[0]));
    }
    // F1..F24.
    if let Some(n) = upper.strip_prefix('F').and_then(|n| n.parse::<u16>().ok())
        && (1..=24).contains(&n)
    {
        return Some(0x70 + n - 1);
    }
    // Num0..Num9 (number pad).
    if let Some(n) = upper
        .strip_prefix("NUM")
        .and_then(|n| n.parse::<u16>().ok())
        && n <= 9
    {
        return Some(0x60 + n);
    }
    NAMED
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, vk)| *vk)
}

fn key_name(vk: u16) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from(vk as u8).to_string(),
        0x60..=0x69 => format!("Num{}", vk - 0x60),
        0x70..=0x87 => format!("F{}", vk - 0x70 + 1),
        _ => NAMED
            .iter()
            .find(|(_, code)| *code == vk)
            .map(|(n, _)| (*n).to_string())
            .unwrap_or_else(|| format!("0x{vk:02X}")),
    }
}

/// Keys that make sense as a global shortcut on their own, without a modifier: they type
/// nothing, so taking them over does not break normal typing.
fn standalone_ok(vk: u16) -> bool {
    matches!(vk, 0x13 | 0x2C | 0x7C..=0x87 | 0xAD..=0xB3)
}

impl Hotkey {
    /// Parse text such as `"Ctrl+Alt+N"`, `"win + shift + f5"` or `"Ctrl++"`. Case and spaces
    /// around the `+` signs do not matter.
    pub fn parse(text: &str) -> Result<Hotkey, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("empty shortcut".into());
        }
        // A trailing "++" means the last key is the plus key itself.
        let (body, plus_key) = match text.strip_suffix("++") {
            Some(rest) => (rest, true),
            None => (text, false),
        };
        let mut hk = Hotkey {
            ctrl: false,
            alt: false,
            shift: false,
            win: false,
            vk: 0,
        };
        let mut key: Option<u16> = plus_key.then_some(0xBB);
        let parts: Vec<&str> = body.split('+').map(str::trim).collect();
        if parts.iter().any(|p| p.is_empty()) && !(plus_key && body.is_empty()) {
            return Err(format!("`{text}` has an empty part"));
        }
        for part in parts.into_iter().filter(|p| !p.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => hk.ctrl = true,
                "alt" => hk.alt = true,
                "shift" => hk.shift = true,
                "win" | "windows" | "super" | "meta" => hk.win = true,
                _ => {
                    let code = key_code(part).ok_or_else(|| format!("unknown key `{part}`"))?;
                    if key.replace(code).is_some() {
                        return Err(format!("`{text}` names more than one key"));
                    }
                }
            }
        }
        hk.vk = key.ok_or_else(|| format!("`{text}` has modifiers but no key"))?;
        if !hk.has_modifier() && !standalone_ok(hk.vk) {
            return Err(format!(
                "`{text}` needs Ctrl, Alt, Shift or Win (it would stop that key from typing)"
            ));
        }
        if !hk.ctrl && !hk.alt && !hk.win && hk.shift && !standalone_ok(hk.vk) {
            return Err(format!(
                "`{text}`: Shift alone would stop capital letters from typing"
            ));
        }
        Ok(hk)
    }

    pub fn has_modifier(&self) -> bool {
        self.ctrl || self.alt || self.shift || self.win
    }

    /// The `fsModifiers` argument for `RegisterHotKey` (without `MOD_NOREPEAT`).
    pub fn modifiers(&self) -> u32 {
        let mut m = 0;
        if self.alt {
            m |= MOD_ALT;
        }
        if self.ctrl {
            m |= MOD_CONTROL;
        }
        if self.shift {
            m |= MOD_SHIFT;
        }
        if self.win {
            m |= MOD_WIN;
        }
        m
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            f.write_str("Ctrl+")?;
        }
        if self.alt {
            f.write_str("Alt+")?;
        }
        if self.shift {
            f.write_str("Shift+")?;
        }
        if self.win {
            f.write_str("Win+")?;
        }
        f.write_str(&key_name(self.vk))
    }
}

/// Normalise a shortcut setting: `Ok("")` for "no shortcut", the canonical spelling for a valid
/// one, or the parse error.
pub fn normalize(text: &str) -> Result<String, String> {
    if text.trim().is_empty() {
        return Ok(String::new());
    }
    Hotkey::parse(text).map(|h| h.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(text: &str) -> Hotkey {
        match Hotkey::parse(text) {
            Ok(h) => h,
            Err(e) => panic!("{text}: {e}"),
        }
    }

    #[test]
    fn parses_the_default_shortcuts() {
        let h = ok("Ctrl+Alt+N");
        assert!(h.ctrl && h.alt && !h.shift && !h.win);
        assert_eq!(h.vk, 0x4E);
        assert_eq!(h.modifiers(), MOD_CONTROL | MOD_ALT);
        for (text, vk) in [
            ("Ctrl+Alt+P", 0x50),
            ("Ctrl+Alt+M", 0x4D),
            ("Ctrl+Alt+H", 0x48),
            ("Ctrl+Alt+S", 0x53),
            ("Ctrl+Alt+C", 0x43),
        ] {
            assert_eq!(ok(text).vk, vk, "{text}");
        }
    }

    #[test]
    fn case_spaces_and_aliases_do_not_matter() {
        assert_eq!(ok(" control + ALT + n "), ok("Ctrl+Alt+N"));
        assert_eq!(ok("windows+shift+f5"), ok("Win+Shift+F5"));
        assert_eq!(ok("super+e"), ok("Win+E"));
        assert_eq!(ok("Ctrl+Esc"), ok("ctrl+escape"));
    }

    #[test]
    fn canonical_spelling_round_trips() {
        for text in [
            "Ctrl+Alt+N",
            "Ctrl+Shift+Win+F12",
            "Alt+Space",
            "Ctrl+Num7",
            "Win+PageDown",
            "Ctrl+Alt+;",
            "Ctrl+Alt+Plus",
            "MediaPlayPause",
            "F13",
        ] {
            let h = ok(text);
            assert_eq!(h.to_string(), text);
            assert_eq!(ok(&h.to_string()), h);
        }
        // Modifier order is normalised.
        assert_eq!(ok("Shift+Alt+Ctrl+K").to_string(), "Ctrl+Alt+Shift+K");
        assert_eq!(ok("Ctrl++").to_string(), "Ctrl+Plus");
    }

    #[test]
    fn function_number_and_named_keys() {
        assert_eq!(ok("Ctrl+F1").vk, 0x70);
        assert_eq!(ok("Ctrl+F24").vk, 0x87);
        assert_eq!(ok("Alt+0").vk, 0x30);
        assert_eq!(ok("Alt+Num0").vk, 0x60);
        assert_eq!(ok("Ctrl+Left").vk, 0x25);
        assert_eq!(ok("Ctrl+PrintScreen").vk, 0x2C);
        assert_eq!(ok("Ctrl+/").vk, 0xBF);
    }

    #[test]
    fn rejects_bad_text() {
        for text in [
            "",
            "Ctrl+Alt",
            "Ctrl+",
            "+N",
            "Ctrl+Alt+NN",
            "Ctrl+F25",
            "Ctrl+F0",
            "Ctrl+A+B",
            "Hyper+A",
            "Ctrl++Alt+A",
        ] {
            assert!(Hotkey::parse(text).is_err(), "{text:?} should be rejected");
        }
    }

    #[test]
    fn typing_keys_need_a_real_modifier() {
        assert!(Hotkey::parse("N").is_err());
        assert!(Hotkey::parse("Space").is_err());
        assert!(Hotkey::parse("Shift+N").is_err());
        // Keys that type nothing may stand alone.
        assert!(Hotkey::parse("Pause").is_ok());
        assert!(Hotkey::parse("F18").is_ok());
        assert!(Hotkey::parse("VolumeMute").is_ok());
        assert!(Hotkey::parse("Shift+F20").is_ok());
        // F1..F12 are used by apps; they need a modifier.
        assert!(Hotkey::parse("F5").is_err());
    }

    #[test]
    fn normalize_handles_empty_and_errors() {
        assert_eq!(normalize("  "), Ok(String::new()));
        assert_eq!(normalize("alt+shift+z"), Ok("Alt+Shift+Z".into()));
        assert!(normalize("Ctrl+Nope").is_err());
    }
}
