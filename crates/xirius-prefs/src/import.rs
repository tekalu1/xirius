//! Reading another program's keys (KEY-013): a Studio Pro key scheme
//! (`<Command category="Transport" name="Start"><Key name="Space"/></Command>`).
//!
//! The commands it names that this app has are bound to its keys; the rest are skipped.
//!
//! Only what an import needs is kept here (LIC-034): the name Studio Pro's key scheme writes for
//! a command, and the context and command of this app that the keys go to. The manual's keys,
//! its sections and the commands this app does not have are not copied; the keys of the built-in
//! preset are this app's own, in `xirius_core::commands::keys`.

use crate::chord::{Chord, Os};
use crate::keymap::{Keymap, Others};

/// The Studio Pro commands this app has: the name its key scheme writes, the context here, and
/// the command here. A command of this app in several contexts (the same edit in the arrange
/// window and in the piano roll) is one line each. Sorted by name, which is this app's order,
/// not the manual's.
#[rustfmt::skip]
pub const STUDIO_PRO: &[(&str, &str, &str)] = &[
    ("Add Tracks", "global", "view.add-track"),
    ("All Notes Off", "global", "transport.panic"),
    ("Arm", "edit", "track.arm"),
    ("Autopunch", "global", "record.punch"),
    ("Autoscroll", "edit", "view.autoscroll"),
    ("Bounce Selection", "arrange", "clip.bounce"),
    ("Bounce To New Track", "arrange", "clip.bounce --new-track"),
    ("Browser", "global", "view.browser"),
    ("Click", "global", "transport.metronome"),
    ("Close", "global", "app.quit"),
    ("Console", "global", "view.mixer"),
    ("Copy", "edit", "edit.copy"),
    ("Create Autofades", "arrange", "clip.fade.set --auto"),
    ("Create Crossfades", "arrange", "clip.fade.cross"),
    ("Create Ranges", "arrange", "range.from-clips"),
    ("Create Ranges on Tracks", "arrange", "range.from-clips"),
    ("Cut", "edit", "edit.cut"),
    ("Decrease Volume", "arrange", "clip.gain.set --by -1"),
    ("Decrease Volume Fine", "arrange", "clip.gain.set --by -0.1"),
    ("Delete", "arrange", "clip.delete"),
    ("Delete", "roll", "note.delete"),
    ("Delete", "automation", "clip.delete"),
    ("Delete", "lane", "note.delete"),
    ("Delete Time", "arrange", "range.time.delete"),
    ("Deselect All", "edit", "select.clear"),
    ("Deselect All on Tracks", "arrange", "select.off-tracks"),
    ("Document Setup", "global", "project.settings"),
    ("Down", "global", "select.step --dir down"),
    ("Down", "roll", "note.transpose --by -1"),
    ("Down Extend", "global", "select.step --dir down --extend"),
    ("Down Extend", "roll", "note.transpose --by -12"),
    ("Down Extend Add", "global", "select.step --dir down --extend"),
    ("Duplicate", "arrange", "clip.duplicate"),
    ("Duplicate", "roll", "note.duplicate"),
    ("Duplicate", "automation", "clip.duplicate"),
    ("Duplicate", "lane", "note.duplicate"),
    ("Duplicate and Insert", "arrange", "section.duplicate"),
    ("Editor", "global", "clip.open"),
    ("End", "global", "select.step --dir end"),
    ("End Extend", "global", "select.step --dir end --extend"),
    ("End Extend Add", "global", "select.step --dir end --extend"),
    ("Export Mixdown", "global", "export.open"),
    ("Export Stems", "global", "view.export-stems"),
    ("Find Channel", "global", "track.reveal --in mixer"),
    ("Find Command", "global", "view.palette"),
    ("Find Track", "global", "track.reveal"),
    ("Focus Next", "global", "view.focus --way next"),
    ("Focus Previous", "global", "view.focus --way prev"),
    ("Forward", "global", "transport.goto --to next-beat"),
    ("Forward Bar", "global", "transport.goto --to next-bar"),
    ("Fullscreen", "global", "view.fullscreen"),
    ("Global Mute", "global", "track.mute.clear"),
    ("Global Solo", "global", "track.solo.clear"),
    ("Goto Loop End", "global", "transport.goto --to loop-end"),
    ("Goto Loop Start", "global", "transport.goto --to loop-start"),
    ("Goto Next Event", "global", "transport.goto --to next-event"),
    ("Goto Next Marker", "global", "transport.goto --to next-marker"),
    ("Goto Previous Event", "global", "transport.goto --to prev-event"),
    ("Goto Previous Marker", "global", "transport.goto --to prev-marker"),
    ("Goto Time", "global", "transport.locate"),
    ("Import File", "global", "clip.import"),
    ("Increase Volume", "arrange", "clip.gain.set --by 1"),
    ("Increase Volume Fine", "arrange", "clip.gain.set --by 0.1"),
    ("Insert", "arrange", "marker.add"),
    ("Insert Instrument Part", "arrange", "clip.add.midi"),
    ("Insert Named", "arrange", "marker.add.named"),
    ("Insert Silence", "arrange", "range.time.insert"),
    ("Inspector", "global", "browser.tab --tab inspector"),
    ("Invert Selection", "edit", "select.invert"),
    ("Left", "global", "select.step --dir left"),
    ("Left", "roll", "edit.nudge.left --grid 1/16"),
    ("Left Extend", "global", "select.step --dir left --extend"),
    ("Left Extend Add", "global", "select.step --dir left --extend"),
    ("Locate Mouse Cursor", "global", "transport.locate-mouse"),
    ("Locate Selection", "global", "transport.locate-selection"),
    ("Loop Selection", "global", "loop.selection --no-snap"),
    ("Loop Selection Snapped", "global", "loop.selection"),
    ("Merge Events", "arrange", "clip.join"),
    ("Merge Events", "roll", "note.join"),
    ("Monitor", "edit", "track.monitor"),
    ("Move to Cursor", "arrange", "clip.move.cursor"),
    ("Mute", "edit", "track.mute"),
    ("New", "global", "project.new"),
    ("Next", "global", "plugin.preset.next"),
    ("Next Bank", "mixer", "mixer.bank --way next"),
    ("Next Channel", "mixer", "mixer.channel.next"),
    ("Next Parameter", "automation", "auto.lane.next"),
    ("Next Parameter", "arrange", "auto.lane.next"),
    ("Normalize Audio", "arrange", "clip.normalize"),
    ("Nudge", "automation", "edit.nudge.right"),
    ("Nudge", "roll", "edit.nudge.right"),
    ("Nudge", "arrange", "clip.nudge --dir right"),
    ("Nudge Back", "automation", "edit.nudge.left"),
    ("Nudge Back", "roll", "edit.nudge.left"),
    ("Nudge Back", "arrange", "clip.nudge --dir left"),
    ("Nudge Bar", "arrange", "clip.nudge --dir right --unit bar"),
    ("Nudge Bar Back", "arrange", "clip.nudge --dir left --unit bar"),
    ("Nudge Down", "roll", "note.transpose --by -1"),
    ("Nudge Down Fine", "roll", "note.transpose --by -12"),
    ("Nudge Up", "roll", "note.transpose --by 1"),
    ("Nudge Up Fine", "roll", "note.transpose --by 12"),
    ("Open", "global", "project.open"),
    ("Open Qwerty Keyboard", "global", "view.pc-keyboard"),
    ("Options", "global", "view.settings"),
    ("Page Down", "edit", "view.page --way down"),
    ("Page Up", "edit", "view.page --way up"),
    ("Paste", "edit", "edit.paste"),
    ("Paste at Original Position", "edit", "edit.paste --at original"),
    ("Perform", "export", "view.export-run"),
    ("Precount", "global", "record.count-in"),
    ("Preroll", "global", "record.preroll"),
    ("Previous", "global", "plugin.preset.prev"),
    ("Previous Bank", "mixer", "mixer.bank --way prev"),
    ("Previous Channel", "mixer", "mixer.channel.prev"),
    ("Previous Parameter", "automation", "auto.lane.next --back"),
    ("Previous Parameter", "arrange", "auto.lane.next --back"),
    ("Quantize", "roll", "note.quantize"),
    ("Quantize 50%", "roll", "note.quantize --strength 50"),
    ("Quit", "global", "app.quit"),
    ("Recall Marker", "global", "transport.goto"),
    ("Recall Marker 1", "global", "transport.goto --to marker-1"),
    ("Recall Marker 2", "global", "transport.goto --to marker-2"),
    ("Recall Marker 3", "global", "transport.goto --to marker-3"),
    ("Recall Marker 4", "global", "transport.goto --to marker-4"),
    ("Recall Marker 5", "global", "transport.goto --to marker-5"),
    ("Recall Marker 6", "global", "transport.goto --to marker-6"),
    ("Recall Marker 7", "global", "transport.goto --to marker-7"),
    ("Record", "global", "record.start"),
    ("Record Panel", "global", "view.record-panel"),
    ("Redo", "global", "history.redo"),
    ("Redo Zoom", "edit", "view.zoom.redo"),
    ("Remove Track", "edit", "track.delete"),
    ("Replace Event With Next Layer", "arrange", "take.next"),
    ("Replace Event With Previous Layer", "arrange", "take.prev"),
    ("Restore Timing", "roll", "note.quantize --restore"),
    ("Return to Zero", "global", "transport.seek --at 0"),
    ("Reverse Audio", "arrange", "clip.reverse"),
    ("Rewind", "global", "transport.goto --to prev-beat"),
    ("Rewind Bar", "global", "transport.goto --to prev-bar"),
    ("Right", "global", "select.step --dir right"),
    ("Right", "roll", "edit.nudge.right --grid 1/16"),
    ("Right Extend", "global", "select.step --dir right --extend"),
    ("Right Extend Add", "global", "select.step --dir right --extend"),
    ("Save", "global", "project.save"),
    ("Save New Version", "global", "version.save"),
    ("Search", "global", "browser.search"),
    ("Select All", "edit", "select.all"),
    ("Select All in Loop", "edit", "select.in-loop"),
    ("Select All on Tracks", "edit", "select.on-tracks"),
    ("Select from Event to End", "arrange", "select.span --way end"),
    ("Select from Start to Event", "arrange", "select.span --way start"),
    ("Set Loop End", "global", "loop.end.set"),
    ("Set Loop Start", "global", "loop.start.set"),
    ("Set Program A", "global", "quantize.program --program A"),
    ("Set Program B", "global", "quantize.program --program B"),
    ("Set Program C", "global", "quantize.program --program C"),
    ("Set Program D", "global", "quantize.program --program D"),
    ("Set Program E", "global", "quantize.program --program E"),
    ("Set Program F", "global", "quantize.program --program F"),
    ("Set Program G", "global", "quantize.program --program G"),
    ("Show Channel Editor", "global", "plugin.editor.open"),
    ("Show Effects", "global", "browser.tab --tab effects"),
    ("Show Envelopes", "edit", "view.lanes"),
    ("Show Files", "global", "browser.tab --tab files"),
    ("Show Instrument Editor", "global", "view.instrument"),
    ("Show Instruments", "global", "browser.tab --tab instruments"),
    ("Snap To Grid", "edit", "view.snap"),
    ("Solo", "edit", "track.solo"),
    ("Split", "arrange", "clip.split"),
    ("Split Loop", "arrange", "clip.split.range --range loop"),
    ("Split Range", "arrange", "clip.split.range"),
    ("Split at Grid", "arrange", "clip.split.grid"),
    ("Start", "global", "select.step --dir start"),
    ("Start Extend", "global", "select.step --dir start --extend"),
    ("Start Extend Add", "global", "select.step --dir start --extend"),
    ("Stop", "global", "transport.stop"),
    ("Store Zoom State", "edit", "view.zoom.store"),
    ("Toggle Loop", "global", "transport.loop"),
    ("Toggle Mute", "arrange", "clip.mute"),
    ("Toggle Mute", "roll", "note.mute"),
    ("Toggle Read", "automation", "auto.mode --mode read"),
    ("Toggle Read", "arrange", "auto.mode --mode read"),
    ("Toggle Ripple Edit", "global", "edit.ripple"),
    ("Toggle Snap", "edit", "view.snap"),
    ("Toggle Start", "global", "transport.play"),
    ("Toggle Zoom", "edit", "view.zoom.toggle"),
    ("Tool 1", "edit", "tool.select"),
    ("Tool 2", "edit", "tool.range"),
    ("Tool 3", "edit", "tool.cut"),
    ("Tool 4", "edit", "tool.erase"),
    ("Tool 5", "edit", "tool.draw"),
    ("Tool 6", "edit", "tool.mute"),
    ("Tool 8", "edit", "tool.listen"),
    ("Touch", "automation", "auto.mode --mode touch"),
    ("Touch", "arrange", "auto.mode --mode touch"),
    ("Transform Range", "arrange", "range.transform"),
    ("Undo", "global", "history.undo"),
    ("Undo Zoom", "edit", "view.zoom.undo"),
    ("Up", "global", "select.step --dir up"),
    ("Up", "roll", "note.transpose --by 1"),
    ("Up Extend", "global", "select.step --dir up --extend"),
    ("Up Extend", "roll", "note.transpose --by 12"),
    ("Up Extend Add", "global", "select.step --dir up --extend"),
    ("Zoom Full", "edit", "view.zoom.full"),
    ("Zoom In", "edit", "view.zoom.time --by 1"),
    ("Zoom In Vertical", "edit", "view.zoom.data --by 1"),
    ("Zoom Out", "edit", "view.zoom.time --by -1"),
    ("Zoom Out Vertical", "edit", "view.zoom.data --by -1"),
    ("Zoom to Loop", "edit", "view.zoom.loop"),
    ("Zoom to Selection", "edit", "view.zoom.selection"),
    ("Zoom to Selection Horizontally", "edit", "view.zoom.selection --horizontal"),
];

/// A name Studio Pro's key scheme gives to more than one command: the category that tells them
/// apart (the one place a section name is kept), the name, the context and the command.
const SHARED: &[(&str, &str, &str, &str)] = &[("Transport", "Start", "global", "transport.start")];

pub fn default_studio_pro_path() -> Option<std::path::PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(std::path::PathBuf::from)
    } else {
        std::env::var_os("HOME")
            .map(|h| std::path::PathBuf::from(h).join("Library/Application Support"))
    }?;
    [
        "Fender/Studio Pro 8/user.keyscheme",
        "PreSonus/Studio One 6/user.keyscheme",
    ]
    .iter()
    .map(|p| base.join(p))
    .find(|p| p.is_file())
}

/// The commands of this app that the scheme's command (`category`, `name`) runs.
pub fn targets(category: &str, name: &str) -> Vec<(String, String)> {
    if let Some((_, _, ctx, cmd)) = SHARED
        .iter()
        .find(|(c, n, _, _)| *c == category && *n == name)
    {
        return vec![((*ctx).into(), (*cmd).into())];
    }
    STUDIO_PRO
        .iter()
        .filter(|(n, _, _)| *n == name)
        .map(|(_, ctx, cmd)| ((*ctx).into(), (*cmd).into()))
        .collect()
}

#[derive(Debug, Default)]
pub struct Imported {
    pub count: usize,
    pub unsupported: Vec<String>,
    pub problems: Vec<String>,
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let end = tag[at..].find('"')? + at;
    Some(
        tag[at..end]
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&"),
    )
}

/// (category, name, keys) of each `<Command>`.
pub fn read_scheme(text: &str) -> Vec<(String, String, Vec<String>)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("<Command") {
        rest = &rest[i + "<Command".len()..];
        let Some(tag_end) = rest.find('>') else { break };
        let tag = &rest[..tag_end];
        let body = if tag.ends_with('/') {
            ""
        } else {
            let end = rest.find("</Command>").unwrap_or(rest.len());
            &rest[tag_end..end]
        };
        let (Some(category), Some(name)) = (attr(tag, "category"), attr(tag, "name")) else {
            continue;
        };
        let mut keys = Vec::new();
        let mut b = body;
        while let Some(k) = b.find("<Key") {
            b = &b[k + 4..];
            let e = b.find('>').unwrap_or(b.len());
            if let Some(n) = attr(&b[..e], "name") {
                keys.push(n);
            }
        }
        out.push((category, name, keys));
    }
    out
}

/// Binds the scheme's keys; how many keys were bound.
pub fn studio_pro(text: &str, keymap: &mut Keymap) -> Result<usize, String> {
    Ok(studio_pro_report(text, keymap)?.count)
}

pub fn studio_pro_report(text: &str, keymap: &mut Keymap) -> Result<Imported, String> {
    let scheme = read_scheme(text);
    if scheme.is_empty() {
        return Err("キーの割り当て（<Command> の並び）が見つかりません".into());
    }
    let mut report = Imported::default();
    let mut grouped = std::collections::BTreeMap::<(String, String), Vec<String>>::new();
    for (category, name, keys) in &scheme {
        let targets = targets(category, name);
        if targets.is_empty() && !keys.is_empty() {
            report
                .unsupported
                .push(format!("{category} / {name}: {}", keys.join(" / ")));
        }
        for (ctx, cmd) in targets {
            grouped.entry((ctx, cmd)).or_default().extend(keys.clone());
        }
    }
    for ((ctx, cmd), keys) in grouped {
        for old in keymap.keys_of(&ctx, &cmd) {
            keymap.unset(&ctx, &old, Some(&cmd));
        }
        for key in keys {
            match Chord::parse(&key, Os::Win) {
                Ok(c) => {
                    keymap.set(&ctx, &c, &cmd, None, Others::Keep);
                    report.count += 1;
                }
                Err(e) => report.problems.push(format!("{ctx} / {cmd}: {e}")),
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scheme_binds_what_this_app_has() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<KeyScheme title="Mine">
  <Command category="Transport" name="Toggle Start"><Key name="Space"/></Command>
  <Command category="Musical Functions" name="Quantize"><Key name="Ctrl+Q"/></Command>
  <Command category="Edit" name="Duplicate"><Key name="Ctrl+D"/></Command>
  <Command category="Audio" name="Normalize Audio"><Key name="Ctrl+N"/></Command>
  <Command category="Something" name="Analyser"><Key name="Ctrl+Shift+A"/></Command>
  <Command category="Edit" name="Toggle Mute"/>
</KeyScheme>"#;
        let mut k = Keymap::default();
        let report = studio_pro_report(xml, &mut k).unwrap();
        // Space, Mod+Q, Mod+D in arrange / roll / automation / lane, and Mod+N.
        assert_eq!(report.count, 7);
        // The command this app does not have is listed, not bound.
        assert_eq!(report.unsupported.len(), 1);
        assert!(report.unsupported[0].starts_with("Something / Analyser"));
        let q = Chord::parse("Mod+Q", Os::Win).unwrap();
        assert_eq!(k.lookup("roll", &q).unwrap().1, "note.quantize");
        assert_eq!(
            k.keys_of("arrange", "clip.duplicate"),
            [Chord::parse("Mod+D", Os::Win).unwrap()]
        );
        // A command the scheme names with no key loses its key.
        assert!(k.keys_of("roll", "note.mute").is_empty());
        assert!(studio_pro("<nothing/>", &mut k).is_err());
    }

    /// A line is written once (a name may repeat in another context), so a lookup gives one
    /// answer per line, and [`SHARED`] takes its command out of the name alone.
    #[test]
    fn every_line_is_written_once() {
        let mut lines: Vec<(&str, &str, &str)> = STUDIO_PRO.to_vec();
        lines.sort_unstable();
        let before = lines.len();
        lines.dedup();
        assert_eq!(before, lines.len(), "a line twice");
        for (category, name, ctx, cmd) in SHARED {
            assert!(
                STUDIO_PRO.iter().any(|(n, c, _)| n == name && c == ctx),
                "{category} / {name} is not in the table"
            );
            assert_eq!(
                targets(category, name),
                vec![((*ctx).to_string(), (*cmd).to_string())]
            );
            assert!(!targets("", name).iter().any(|(_, c)| c == cmd));
        }
    }
}
