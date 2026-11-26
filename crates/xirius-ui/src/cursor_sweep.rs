//! Every place of every surface, swept on a grid: what kind of thing is under the pointer and
//! the shape it shows (UI-385). The pairs are kept in `tests/cursor_table.txt`; a new kind of
//! place, or a shape that changed, shows up as a difference in that table. With
//! `XIRIUS_BLESS_CURSOR=1` each scene writes its lines instead.

use crate::instrument::{DEFAULT, Pick, current};
use crate::pointer::{Pointer, of_studio, show};
use crate::song::Song;
use crate::studio::{DrawerTab, Studio};
use crate::tools::Mods;
use std::collections::BTreeSet;

const TABLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/cursor_table.txt");
const STEP: f32 = 8.0;
const SIZE: (f32, f32) = (1440.0, 900.0);

/// The kinds of place that keep the arrow on purpose: nothing to press there, the middle of a
/// clip or a note (UI-382), the bars that move the window, and the empty parts of panels.
const ARROW: &[&str] = &[
    // Nothing under the pointer, or the empty parts of a window of its own.
    "None",
    "Body",
    "Graph",
    "Mixer::Strip",
    // The bars of the windows of their own move the window.
    "Head",
    // The middle of a clip and of a note keep the arrow, held too (UI-382).
    "Clip@Clip::Body",
    "None@Note::Body",
    // A track's row is chosen and moved like a clip; its buttons have the hand.
    "Row::Track@Row::Track",
    "Row::Track@Space::Track",
    "Row::Folder@Row::Folder",
    "Row::Folder@Space::Folder",
    // The tempo row between its points takes a double press only (a new point).
    "None@TempoGap",
    // A heading of the mixer's strip only opens its menu with the right button.
    "Mixer::Heading::Insert",
    "Mixer::Heading::Instrument",
];

/// The kind of a thing from its `Debug`: its names, without ids, numbers or strings
/// (`Mixer(Fader(TrackId(…)))` is `Mixer::Fader`).
fn kind(debug: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut flush = |word: &mut String| {
        if word.starts_with(|c: char| c.is_ascii_uppercase())
            && !word.ends_with("Id")
            && word != "Some"
        {
            out.push(std::mem::take(word));
        }
        word.clear();
    };
    for c in debug.chars() {
        if quoted {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                quoted = false;
            }
            continue;
        }
        if c == '"' {
            quoted = true;
            flush(&mut word);
        } else if c.is_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            flush(&mut word);
        }
    }
    flush(&mut word);
    out.join("::")
}

/// The rows of one scene: (kind of place, system shape, picture).
#[derive(Default)]
struct Table {
    rows: BTreeSet<(String, String, String)>,
}

#[cfg(test)]
impl Table {
    fn add(&mut self, kind: String, p: Pointer) {
        self.rows.insert((
            kind,
            format!("{:?}", p.style),
            p.shape.map_or_else(String::new, |s| format!("{s:?}")),
        ));
    }

    /// Checks the scene's rows against the kept table, or writes them there when blessing
    /// (`XIRIUS_BLESS_CURSOR=1 cargo nextest run -p xirius-ui -j 1 cursor_sweep`: one at a
    /// time, as each rewrites its own lines of the one file).
    fn check(self, scene: &str) {
        // Nothing pressable keeps the arrow by leaving its shape out.
        let arrows: Vec<&str> = self
            .rows
            .iter()
            .filter(|(_, style, shape)| style == "Arrow" && shape.is_empty())
            .map(|(kind, _, _)| kind.as_str())
            .filter(|kind| !ARROW.contains(kind))
            .collect();
        let bless = std::env::var_os("XIRIUS_BLESS_CURSOR").is_some();
        assert!(
            bless || arrows.is_empty(),
            "{scene}: these places show the arrow but are not in ARROW: {arrows:#?}"
        );
        let now: Vec<String> = self
            .rows
            .iter()
            .map(|(kind, style, shape)| format!("{scene}\t{kind}\t{style}\t{shape}"))
            .collect();
        let kept = std::fs::read_to_string(TABLE)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        let prefix = format!("{scene}\t");
        let (mine, others): (Vec<&str>, Vec<&str>) = kept
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .partition(|l| l.starts_with(&prefix));
        if bless {
            eprintln!("{scene}: arrows {arrows:#?}");
            let mut lines: BTreeSet<String> = others.iter().map(|l| l.to_string()).collect();
            lines.extend(now);
            let mut out = String::from(HEADER);
            for l in lines {
                out.push_str(&l);
                out.push('\n');
            }
            std::fs::write(TABLE, out).unwrap();
            return;
        }
        let mine: BTreeSet<&str> = mine.into_iter().collect();
        let now: BTreeSet<&str> = now.iter().map(String::as_str).collect();
        let gone: Vec<_> = mine.difference(&now).collect();
        let new: Vec<_> = now.difference(&mine).collect();
        assert!(
            gone.is_empty() && new.is_empty(),
            "{scene}: the shapes changed (XIRIUS_BLESS_CURSOR=1 rewrites {TABLE}):\n\
             no longer: {gone:#?}\nnew: {new:#?}"
        );
    }
}

const HEADER: &str =
    "# scene\tkind of place\tsystem shape\tpicture  (written by cursor_sweep, UI-385)\n";

fn grid(w: f32, h: f32) -> impl Iterator<Item = (f32, f32)> {
    let cols = (w / STEP) as usize;
    let rows = (h / STEP) as usize;
    (0..rows).flat_map(move |r| {
        (0..cols).map(move |c| (c as f32 * STEP + STEP / 2.0, r as f32 * STEP + STEP / 2.0))
    })
}

/// The main window's body, moved over as a person's pointer would. Where the hit says nothing
/// of its own, the arrangement's spot or the roll's note under the pointer is named after `@`.
fn sweep_body(scene: &str, mut s: Studio) {
    let mut table = Table::default();
    let g = s.geometry(SIZE.0, SIZE.1);
    s.resized(&g);
    for (x, y) in grid(SIZE.0, SIZE.1) {
        s.window_moved(&g, x, y, Mods::NONE);
        let hit = s.hit(&g, x, y);
        let mut place = kind(&format!("{hit:?}"));
        if s.arrange.hover != crate::arrange::Spot::None {
            place = format!("{place}@{}", kind(&format!("{:?}", s.arrange.hover)));
        } else if let Some((_, zone)) = s.roll.hover {
            place = format!("{place}@Note::{zone:?}");
        }
        table.add(place, of_studio(&s, &g));
    }
    table.check(scene);
}

fn track(s: &Studio, name: &str) -> xirius_core::id::TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == name)
        .map(|t| t.id)
        .unwrap()
}

fn start_screen() -> (crate::shot::TestFolder, Studio) {
    let tmp = crate::shot::TestFolder::new("xirius-ui-cursor-");
    let keep = crate::keep::Keep::at(tmp.path().join("keep"));
    let path = tmp.path().join("a");
    Song::empty().save_as(&path).unwrap();
    keep.remember(&path, "a", false, 1);
    let mut s = Studio::new(Song::empty());
    s.keep = keep;
    s.keep.sync(&s.song.project());
    s.keep.settle_recent();
    s.keep.poll_recent();
    s.home = Some(crate::home::Home::default());
    s.home_tick();
    (tmp, s)
}

#[test]
fn the_empty_song() {
    sweep_body("empty", Studio::new(Song::empty()));
}

#[test]
fn the_arrangement_and_the_roll() {
    sweep_body("roll", Studio::sample());
}

#[test]
fn the_arrangement_and_the_roll_with_the_pencil() {
    let mut s = Studio::sample();
    s.tool = crate::tools::Tool::Draw;
    sweep_body("pencil", s);
}

#[test]
fn the_mixer() {
    let mut s = Studio::sample();
    s.tab = DrawerTab::Mixer;
    sweep_body("mixer", s);
}

#[test]
fn the_start_screen() {
    let (_tmp, s) = start_screen();
    assert!(s.home_shown());
    sweep_body("start", s);
}

#[test]
fn a_devices_window() {
    // A sample in the Sampler, so that its knobs show.
    let mut s = Studio::sample();
    let t = track(&s, "Keys");
    assert!(s.set_instrument(t, &Pick::built_in(DEFAULT)));
    let id = current(&s.project().tracks[&t]).unwrap().id;
    assert!(s.run_command(
        "plugin.samples.add",
        serde_json::json!({ "id": id.to_string(), "file": "Piano C3.wav" }),
    ));
    s.show_instrument(t, id);
    let mut table = Table::default();
    crate::device_window::with_surface(&mut s, id, |s, g| {
        let p = s.auto_shown();
        for (x, y) in grid(g.size.0, g.size.1) {
            let part = s.instruments.at(&p, g, x, y).map(|(_, part)| part);
            let (want, held) = s.float_want(g, x, y);
            table.add(kind(&format!("{part:?}")), show(want, false, held));
        }
    });
    table.check("device");
}

#[test]
fn the_channel_editors_window() {
    let mut s = Studio::sample();
    s.instruments.open.clear();
    let g = s.geometry(1600.0, 1000.0);
    s.open_channel(track(&s, "Vocal"));
    s.instruments.detached_channel = true;
    let v = s.channel_view(&g).unwrap();
    let mut table = Table::default();
    for (x, y) in grid(g.size.0, g.size.1) {
        let part = crate::channel::part_at(&v, x, y);
        let (want, held) = s.float_want(&g, x, y);
        table.add(kind(&format!("{part:?}")), show(want, false, held));
    }
    table.check("channel");
}

#[test]
fn the_macro_controls_window() {
    let mut s = Studio::sample();
    s.instruments.open.clear();
    let g = s.geometry(1600.0, 1000.0);
    s.open_macros(track(&s, "Vocal"));
    s.instruments.detached_macros = true;
    let v = s.macro_view().unwrap();
    let mut table = Table::default();
    for (x, y) in grid(g.size.0, g.size.1) {
        let part = crate::macro_pad::part_at(&v, x, y);
        let (want, held) = s.float_want(&g, x, y);
        table.add(kind(&format!("{part:?}")), show(want, false, held));
    }
    table.check("macros");
}
