//! A key with its modifiers (`Mod+Alt+Shift+Q`), as the keymap writes it (06 §3.2).
//!
//! `Mod` is Ctrl on Windows and Linux and ⌘ on a Mac; `Ctrl` is written only for a Mac's
//! Control. The modifiers always come in the order Mod, Ctrl, Alt, Shift, and a letter is
//! written in capitals, so one chord has one spelling and the files diff line by line.

use std::fmt;

pub const MOD: u8 = 1;
pub const CTRL: u8 = 2;
pub const ALT: u8 = 4;
pub const SHIFT: u8 = 8;

/// The order the modifiers are written in.
const ORDER: [(u8, &str); 4] = [(MOD, "Mod"), (CTRL, "Ctrl"), (ALT, "Alt"), (SHIFT, "Shift")];

/// Which system the chord is shown for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Win,
    Mac,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else {
            Os::Win
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Chord {
    pub mods: u8,
    /// The key's name: `Q`, `1`, `F5`, `Up`, `Delete`, `Space`, `,` ...
    pub key: String,
}

/// Names a key may be written with, and the name it is kept as.
const ALIASES: &[(&str, &str)] = &[
    ("esc", "Escape"),
    ("escape", "Escape"),
    ("del", "Delete"),
    ("delete", "Delete"),
    ("backspace", "Backspace"),
    ("bs", "Backspace"),
    ("enter", "Enter"),
    ("return", "Enter"),
    ("space", "Space"),
    ("tab", "Tab"),
    ("up", "Up"),
    ("down", "Down"),
    ("left", "Left"),
    ("right", "Right"),
    ("↑", "Up"),
    ("↓", "Down"),
    ("←", "Left"),
    ("→", "Right"),
    ("home", "Home"),
    ("end", "End"),
    ("pageup", "PageUp"),
    ("pgup", "PageUp"),
    ("pagedown", "PageDown"),
    ("pgdn", "PageDown"),
    ("insert", "Insert"),
    ("menu", "Menu"),
    ("apps", "Menu"),
    ("contextmenu", "Menu"),
    ("ins", "Insert"),
    ("plus", "+"),
    ("left arrow", "Left"),
    ("right arrow", "Right"),
    ("up arrow", "Up"),
    ("down arrow", "Down"),
    ("page up", "PageUp"),
    ("page down", "PageDown"),
    ("caps lock", "CapsLock"),
    ("capslock", "CapsLock"),
    ("play pause", "MediaPlayPause"),
    ("stop", "MediaStop"),
    ("record", "MediaRecord"),
    ("rewind", "MediaRewind"),
    ("forward", "MediaForward"),
];

/// Single characters a key may be (the printed character of an unshifted key).
const PUNCT: &str = ",./;'[]\\-=`+{}#!:\"@~<>?_|()*&%^$";

/// How a key is shown, per system (the rest are shown as kept).
fn shown(key: &str, os: Os) -> &str {
    match (os, key) {
        (_, "Up") => "↑",
        (_, "Down") => "↓",
        (_, "Left") => "←",
        (_, "Right") => "→",
        (Os::Win, "Delete") => "Del",
        (Os::Win, "Escape") => "Esc",
        (Os::Mac, "Delete") => "⌫",
        (Os::Mac, "Backspace") => "⌫",
        (Os::Mac, "Escape") => "esc",
        (Os::Mac, "Enter") => "↩",
        (Os::Mac, "Tab") => "⇥",
        _ => key,
    }
}

/// The key's kept name, or `None` for a name no key has.
pub fn key_name(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let lower = raw.to_lowercase();
    if let Some((_, k)) = ALIASES.iter().find(|(a, _)| *a == lower) {
        return Some((*k).to_string());
    }
    if let Some(tail) = lower.strip_prefix("numpad")
        && (tail.len() == 1 && "0123456789+-*/.,".contains(tail))
    {
        return Some(format!("NumPad{tail}"));
    }
    if [
        "mediaplaypause",
        "mediastop",
        "mediarecord",
        "mediarewind",
        "mediaforward",
    ]
    .contains(&lower.as_str())
    {
        return Some(
            match lower.as_str() {
                "mediaplaypause" => "MediaPlayPause",
                "mediastop" => "MediaStop",
                "mediarecord" => "MediaRecord",
                "mediarewind" => "MediaRewind",
                _ => "MediaForward",
            }
            .into(),
        );
    }
    if let Some(code) = raw.strip_prefix("code:") {
        return (!code.is_empty() && code.chars().all(|c| c.is_ascii_alphanumeric()))
            .then(|| format!("code:{code}"));
    }
    let mut chars = raw.chars();
    let (c, rest) = (chars.next()?, chars.next());
    if rest.is_none() {
        if c.is_ascii_alphanumeric() {
            return Some(c.to_ascii_uppercase().to_string());
        }
        if PUNCT.contains(c) {
            return Some(c.to_string());
        }
        return None;
    }
    if let Some(n) = lower.strip_prefix('f')
        && let Ok(n) = n.parse::<u8>()
        && (1..=24).contains(&n)
    {
        return Some(format!("F{n}"));
    }
    None
}

impl Chord {
    pub fn new(mods: u8, key: &str) -> Chord {
        Chord {
            mods,
            key: key.to_string(),
        }
    }

    /// Reads `Mod+Alt+Q`, `Ctrl+Shift+Z`, `⌘⇧Z` or `Alt+↑`. On Windows and Linux `Ctrl` is the
    /// same as `Mod`; on a Mac it is the Control key.
    pub fn parse(text: &str, os: Os) -> Result<Chord, String> {
        let text = text.trim();
        let mut mods = 0u8;
        // The Mac symbols, written without `+`.
        let mut rest = text;
        loop {
            let (bit, len) = match rest.chars().next() {
                Some('⌘') => (MOD, '⌘'.len_utf8()),
                Some('⌃') => (CTRL, '⌃'.len_utf8()),
                Some('⌥') => (ALT, '⌥'.len_utf8()),
                Some('⇧') => (SHIFT, '⇧'.len_utf8()),
                _ => break,
            };
            if rest.len() == len {
                break;
            }
            mods |= bit;
            rest = &rest[len..];
        }
        // A trailing `+` is the key itself (`Mod++`).
        let (head, key) = if let Some(at) = rest.to_lowercase().find("numpad") {
            (rest[..at].trim_end_matches('+'), &rest[at..])
        } else {
            match rest.strip_suffix("++") {
                Some(h) => (h, "+"),
                None if rest == "+" => ("", "+"),
                None => match rest.rfind('+') {
                    Some(i) => (&rest[..i], &rest[i + 1..]),
                    None => ("", rest),
                },
            }
        };
        for part in head.split('+').filter(|p| !p.is_empty()) {
            mods |= match part.trim().to_lowercase().as_str() {
                "mod" | "cmd" | "command" | "⌘" => MOD,
                "ctrl" | "control" | "ctl" | "⌃" => {
                    if os == Os::Mac {
                        CTRL
                    } else {
                        MOD
                    }
                }
                "alt" | "option" | "opt" | "⌥" => ALT,
                "shift" | "⇧" => SHIFT,
                other => return Err(format!("「{other}」は修飾キーではありません（{text}）")),
            };
        }
        let key = key_name(key)
            .ok_or_else(|| format!("「{key}」はキーの名前ではありません（{text}）"))?;
        Ok(Chord { mods, key })
    }

    /// The chord a key press makes. `key` is the name the window gives (`a`, `up`, `f5`, `,`).
    pub fn from_event(key: &str, mods: u8) -> Option<Chord> {
        Some(Chord {
            mods,
            key: key_name(key)?,
        })
    }

    pub fn has(&self, bit: u8) -> bool {
        self.mods & bit != 0
    }

    /// As the screen shows it: `Ctrl+Shift+Z` on Windows, `⇧⌘Z` on a Mac (the Mac's order is
    /// ⌃⌥⇧⌘).
    pub fn display(&self, os: Os) -> String {
        let key = shown(&self.key, os);
        let key = key.strip_prefix("code:").unwrap_or(key);
        match os {
            Os::Win => {
                let mut s = String::new();
                for (bit, name) in [(MOD | CTRL, "Ctrl"), (ALT, "Alt"), (SHIFT, "Shift")] {
                    if self.mods & bit != 0 {
                        s.push_str(name);
                        s.push('+');
                    }
                }
                s.push_str(key);
                s
            }
            Os::Mac => {
                let mut s = String::new();
                for (bit, sym) in [(CTRL, '⌃'), (ALT, '⌥'), (SHIFT, '⇧'), (MOD, '⌘')] {
                    if self.mods & bit != 0 {
                        s.push(sym);
                    }
                }
                s.push_str(key);
                s
            }
        }
    }

    /// The modifiers alone, in the written form (`Mod+Alt`), `none` for none.
    pub fn mods_text(mods: u8) -> String {
        let parts: Vec<&str> = ORDER
            .iter()
            .filter(|(b, _)| mods & b != 0)
            .map(|(_, n)| *n)
            .collect();
        if parts.is_empty() {
            "none".into()
        } else {
            parts.join("+")
        }
    }

    /// Reads modifiers alone (`Mod+Shift`, `none`).
    pub fn parse_mods(text: &str, os: Os) -> Result<u8, String> {
        let t = text.trim();
        if t.is_empty() || t.eq_ignore_ascii_case("none") {
            return Ok(0);
        }
        Chord::parse(&format!("{t}+A"), os).map(|c| c.mods)
    }
}

/// The written form: `Mod+Alt+Shift+Q`.
impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (bit, name) in ORDER {
            if self.mods & bit != 0 {
                write!(f, "{name}+")?;
            }
        }
        f.write_str(&self.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_has_one_spelling() {
        for (text, want) in [
            ("ctrl+shift+z", "Mod+Shift+Z"),
            ("Shift+Ctrl+Z", "Mod+Shift+Z"),
            ("Alt+Up", "Alt+Up"),
            ("alt+↑", "Alt+Up"),
            ("Mod++", "Mod++"),
            ("+", "+"),
            ("Del", "Delete"),
            ("q", "Q"),
            ("f5", "F5"),
            ("Mod+,", "Mod+,"),
            ("code:Slash", "code:Slash"),
            ("⌘⇧Z", "Mod+Shift+Z"),
        ] {
            assert_eq!(
                Chord::parse(text, Os::Win).unwrap().to_string(),
                want,
                "{text}"
            );
        }
        assert!(Chord::parse("Hyper+Q", Os::Win).is_err());
        assert!(Chord::parse("Mod+Nothing", Os::Win).is_err());
    }

    #[test]
    fn ctrl_is_the_control_key_only_on_a_mac() {
        assert_eq!(
            Chord::parse("Ctrl+Z", Os::Mac).unwrap().to_string(),
            "Ctrl+Z"
        );
        assert_eq!(
            Chord::parse("Ctrl+Z", Os::Win).unwrap().to_string(),
            "Mod+Z"
        );
    }

    #[test]
    fn it_is_shown_per_system() {
        let c = Chord::parse("Mod+Shift+Z", Os::Win).unwrap();
        assert_eq!(c.display(Os::Win), "Ctrl+Shift+Z");
        assert_eq!(c.display(Os::Mac), "⇧⌘Z");
        let c = Chord::parse("Alt+Up", Os::Win).unwrap();
        assert_eq!(c.display(Os::Win), "Alt+↑");
        assert_eq!(c.display(Os::Mac), "⌥↑");
        assert_eq!(Chord::new(0, "Delete").display(Os::Win), "Del");
    }

    #[test]
    fn a_window_key_becomes_a_chord() {
        assert_eq!(Chord::from_event("q", ALT).unwrap().to_string(), "Alt+Q");
        assert_eq!(
            Chord::from_event("up", MOD | SHIFT).unwrap().to_string(),
            "Mod+Shift+Up"
        );
        assert_eq!(Chord::from_event(",", MOD).unwrap().to_string(), "Mod+,");
        assert_eq!(Chord::from_event("shift", SHIFT), None);
    }

    #[test]
    fn modifiers_alone() {
        assert_eq!(
            Chord::parse_mods("Mod+Shift", Os::Win).unwrap(),
            MOD | SHIFT
        );
        assert_eq!(Chord::parse_mods("none", Os::Win).unwrap(), 0);
        assert_eq!(Chord::mods_text(MOD | ALT), "Mod+Alt");
        assert_eq!(Chord::mods_text(0), "none");
    }
}
