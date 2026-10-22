//! What a key can be bound to, where, and the keys each preset gives (06 §3.1, §3.2).
//!
//! Every binding calls a registry command. Studio Pro's source keys and their supported
//! targets live in `studio_pro`; the other presets override individual rows.

/// A context: where a key works. A narrower one is looked in first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Context {
    pub name: &'static str,
    pub label: &'static str,
    pub parent: Option<&'static str>,
}

pub const CONTEXTS: &[Context] = &[
    Context {
        name: "global",
        label: "全体",
        parent: None,
    },
    Context {
        name: "edit",
        label: "編集",
        parent: Some("global"),
    },
    Context {
        name: "arrange",
        label: "アレンジ",
        parent: Some("edit"),
    },
    Context {
        name: "roll",
        label: "ピアノロール",
        parent: Some("edit"),
    },
    Context {
        name: "lane",
        label: "レーン",
        parent: Some("edit"),
    },
    Context {
        name: "automation",
        label: "オートメーション",
        parent: Some("edit"),
    },
    Context {
        name: "mixer",
        label: "ミキサー",
        parent: Some("global"),
    },
    Context {
        name: "browser",
        label: "ブラウザー",
        parent: Some("global"),
    },
    Context {
        name: "export",
        label: "書き出し",
        parent: Some("global"),
    },
    Context {
        name: "text",
        label: "文字入力",
        parent: Some("global"),
    },
];

pub fn context(name: &str) -> Option<&'static Context> {
    CONTEXTS.iter().find(|c| c.name == name)
}

/// `name` and the contexts around it, narrowest first (`roll`, `edit`, `global`).
pub fn chain(name: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    let mut at = context(name);
    while let Some(c) = at {
        out.push(c.name);
        at = c.parent.and_then(context);
    }
    out
}

pub use xirius_core::commands::keys::{Cat, Entry};

pub use crate::studio_pro::ENTRIES;

pub fn entry(context: &str, command: &str) -> Option<&'static Entry> {
    ENTRIES
        .iter()
        .find(|e| e.context == context && e.command == command)
}

/// A preset that starts from another.
pub struct Preset {
    pub name: &'static str,
    pub label: &'static str,
    /// (context, command, keys): the rows whose keys differ from `studio-pro`.
    pub diff: &'static [(&'static str, &'static str, &'static [&'static str])],
    /// (title, url): the published lists the rows were made from (the page title as opened).
    pub sources: &'static [(&'static str, &'static str)],
}

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "studio-pro",
        label: "Studio Pro",
        diff: &[],
        sources: &[(
            "Key Commands",
            "https://fenderstudiopromanual.fender.com/en/Content/Fundamentals_Topics/Key_Commands.htm",
        )],
    },
    // CUBASE: the number keys pick the tools; Ctrl+D duplicates; Redo is Ctrl+Shift+Z only.
    Preset {
        name: "cubase",
        label: "Cubase",
        diff: &[
            // CUB: Erase 5, Mute 7, Draw 8, Play/Color 9.
            ("edit", "tool.erase", &["5"]),
            ("edit", "tool.draw", &["8"]),
            ("edit", "tool.mute", &["7"]),
            ("edit", "tool.listen", &["9"]),
            // CUB: Duplicate Ctrl+D. Studio Pro's Ctrl+D (deselect) gives it up.
            ("roll", "note.duplicate", &["Mod+D"]),
            ("arrange", "clip.duplicate", &["Mod+D"]),
            ("edit", "select.clear", &[]),
            // CUB: Redo Ctrl+Shift+Z (Studio Pro also has Ctrl+Y).
            ("global", "history.redo", &["Mod+Shift+Z"]),
        ],
        sources: &[(
            "Cubase Keyboard Shortcuts: Complete Cheat Sheet for Windows & Mac",
            "https://tutorialtactic.com/blog/cubase-shortcuts/",
        )],
    },
    // LOGIC: Return goes to the beginning, K is the click, C is the cycle, Cmd+T splits at the
    // playhead, Cmd+R repeats (the key Studio Pro gives to reverse).
    Preset {
        name: "logic",
        label: "Logic Pro",
        diff: &[
            // LGC: Return = Go to beginning, so it is no longer the start (Studio Pro's Enter).
            ("global", "transport.seek --at 0", &["Enter"]),
            ("global", "transport.start", &[]),
            // LGC: C = toggle cycle mode, K = toggle click track.
            ("global", "transport.loop", &["C"]),
            ("global", "transport.metronome", &["K"]),
            // Studio Pro's K (touch) gives it up.
            ("arrange", "auto.mode --mode touch", &[]),
            ("automation", "auto.mode --mode touch", &[]),
            // LGC: Redo Cmd+Shift+Z (Studio Pro also has Ctrl+Y).
            ("global", "history.redo", &["Mod+Shift+Z"]),
            // LGC: Cmd+T = split regions at playhead.
            ("arrange", "clip.split", &["Mod+T"]),
            // Apple's pages: Cmd+R = Repeat Regions/Events (default assignment), and Cmd+R repeats
            // notes in the Piano Roll. Reverse gives the key up.
            ("roll", "note.duplicate", &["Mod+R"]),
            ("arrange", "clip.duplicate", &["Mod+R"]),
            ("arrange", "clip.reverse", &[]),
            // Cmd+T splits, so Studio Pro's Ctrl+T (go to a time) gives it up.
            ("global", "transport.locate", &[]),
        ],
        sources: &[
            (
                "50 Keyboard Shortcuts for Faster Workflow in Logic Pro - RouteNote Create Blog",
                "https://create.routenote.com/blog/50-keyboard-shortcuts-logic-pro/",
            ),
            (
                "Mac用Logic Proでキーコマンドを割り当てる - Apple サポート (日本)",
                "https://support.apple.com/guide/logicpro/lgcp41ff6979/mac",
            ),
            (
                "Creating Multiple Copies of Regions",
                "https://help.apple.com/logicpro/mac/9.1.6/en/logicpro/usermanual/chapter_13_section_20.html",
            ),
            (
                "Repeat notes in the Piano Roll Editor in Logic Pro for iPad - Apple Support",
                "https://support.apple.com/guide/logicpro-ipad/lpipc6f10c0a/ipados",
            ),
        ],
    },
    // ABLETON: Live 12's Windows column.
    Preset {
        name: "ableton",
        label: "Ableton Live",
        diff: &[
            // ALK 42.20: Record F9. 42.16: Toggle Loop Brace Ctrl+L. 42.13: Draw Mode B.
            ("global", "transport.loop", &["Mod+L"]),
            // Studio Pro's Ctrl+L (move the clip to the cursor) gives it up.
            ("arrange", "clip.move.cursor", &[]),
            ("global", "record.start", &["F9"]),
            ("edit", "tool.draw", &["B"]),
            // ALK 42.16: Split Clip at Selection Ctrl+E. Studio Pro's Ctrl+E (export) gives it up.
            ("arrange", "clip.split", &["Mod+E"]),
            ("global", "export.open", &[]),
            // ALK 42.5: Duplicate Ctrl+D. 42.11: Quantize Ctrl+U. Ctrl+D (deselect) gives it up.
            ("roll", "note.duplicate", &["Mod+D"]),
            ("arrange", "clip.duplicate", &["Mod+D"]),
            ("edit", "select.clear", &[]),
            ("roll", "note.quantize", &["Mod+U"]),
            // ALK 42.5: Redo Ctrl+Y only.
            ("global", "history.redo", &["Mod+Y"]),
            // ALK 42.20: Toggle Metronome O. Studio Pro's O (pre-roll) gives it up.
            ("global", "transport.metronome", &["O"]),
            ("global", "record.preroll", &[]),
            // ALK 42.16: Reverse Audio Clip Selection R. Studio Pro's R (record arm) gives it up.
            ("arrange", "clip.reverse", &["R"]),
            ("edit", "track.arm", &[]),
            // F9 records, so Studio Pro's F9 (the files of the browser) gives it up.
            ("global", "browser.tab --tab files", &[]),
        ],
        sources: &[(
            "Live Keyboard Shortcuts — Ableton Reference Manual Version 12 | Ableton",
            "https://www.ableton.com/en/live-manual/12/live-keyboard-shortcuts/",
        )],
    },
    // REAPER: Ctrl+R records, S splits, the metronome has no key.
    Preset {
        name: "reaper",
        label: "REAPER",
        diff: &[
            // RPDF p60: Record Ctrl+R. Reverse gives the key up.
            ("global", "record.start", &["Mod+R"]),
            ("arrange", "clip.reverse", &[]),
            // RPDF p109: Metronome has no default shortcut.
            ("global", "transport.metronome", &[]),
            // RPDF p124: Split items S. Track solo gives the key up.
            ("arrange", "clip.split", &["S"]),
            ("edit", "track.solo", &[]),
            // RPDF p140: Alt P cycles the ripple editing modes (off, per track, all tracks).
            ("global", "edit.ripple", &["Alt+P"]),
        ],
        sources: &[(
            "REAPER User Guide v7.81 (Up and Running)",
            "https://www.reaper.fm/userguide/ReaperUserGuide781c.pdf",
        )],
    },
];

pub const PRESET_NAMES: [&str; 5] = ["studio-pro", "cubase", "logic", "ableton", "reaper"];

pub fn preset(name: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.name == name)
}

/// Keys the system keeps for itself, per system (written form).
pub const OS_RESERVED_WIN: &[&str] = &["Alt+F4", "Alt+Tab", "Mod+Escape"];
pub const OS_RESERVED_MAC: &[&str] = &["Mod+Q", "Mod+Tab", "Mod+H", "Mod+Space", "Mod+M"];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chord::{Chord, Os};

    #[test]
    fn every_key_is_written_once_and_reads_back() {
        for e in ENTRIES {
            assert!(context(e.context).is_some(), "{}", e.context);
            for k in e.keys {
                let c = Chord::parse(k, Os::Win).unwrap();
                assert_eq!(&c.to_string(), k);
            }
        }
        for p in PRESETS {
            for (ctx, cmd, keys) in p.diff {
                assert!(entry(ctx, cmd).is_some(), "{} {ctx} {cmd}", p.name);
                for k in *keys {
                    assert_eq!(&Chord::parse(k, Os::Win).unwrap().to_string(), k);
                }
            }
        }
        assert_eq!(PRESET_NAMES.len(), PRESETS.len());
    }

    #[test]
    fn key_012_every_preset_names_the_pages_it_was_made_from() {
        for p in PRESETS {
            assert!(!p.sources.is_empty(), "{}", p.name);
            for (title, url) in p.sources {
                assert!(
                    !title.is_empty() && url.starts_with("https://"),
                    "{}",
                    p.name
                );
            }
        }
        // The other four make at least five rows different from Studio Pro, and no row repeats.
        for p in PRESETS.iter().filter(|p| p.name != "studio-pro") {
            assert!(p.diff.len() >= 5, "{}", p.name);
            for (i, (c, cmd, _)) in p.diff.iter().enumerate() {
                assert!(
                    !p.diff[..i]
                        .iter()
                        .any(|(c2, cmd2, _)| c2 == c && cmd2 == cmd),
                    "{} {c} {cmd}",
                    p.name
                );
            }
        }
    }

    #[test]
    fn contexts_nest() {
        assert_eq!(chain("roll"), ["roll", "edit", "global"]);
        assert_eq!(chain("global"), ["global"]);
        assert!(chain("nowhere").is_empty());
    }
}
