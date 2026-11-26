//! Every row of the reviewed routing table (`tests/context_menus/mapping.json` and
//! `supplemental.json`), checked from the table (ARC-054).
//!
//! - Whenever a test opens a menu, each of its rows runs a command that is in the command
//!   table, takes the arguments the row gives and is the one the table names
//!   ([`verify_commands`]).
//! - Each target in [`TARGETS`] is opened from the sample song: its rows, their labels in their
//!   order, are as `tests/context_menus/menus.txt` has them, and a press on each row is at most
//!   one step of the history that one undo takes back (a press that makes no step leaves the
//!   song as it was). A row that runs a command of the history itself makes exactly one step,
//!   unless [`SETS_WHAT_STANDS`] says why it changes nothing there.
//!
//! A target the table has that is not opened here is in [`NOT_OPENED`] with the reason, so a
//! target added to the table fails here until it is opened or listed; a row added to an opened
//! target changes `menus.txt` and is pressed.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;

use xirius_core::command::Registry;

use super::super::tests::steps;
use super::*;
use crate::studio::{Geometry, Hit, Studio};

/// One way to open a target's menu from the sample song: the studio with the menu open.
type Open = fn() -> (Studio, Geometry);

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<std::sync::Arc<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(xirius_session::commands::shared_registry)
}

fn route(id: &str) -> &'static Route {
    routes().iter().find(|r| r.id == id).expect("a known rule")
}

/// How a failure names a row: its rule and where the table puts it.
fn named(id: &str) -> String {
    format!("{id} ({})", route(id).after.join(" > "))
}

/// The arguments a row runs its command with; a row that asks for a name or a number gives
/// the one its box starts with.
fn args_of(item: &Item) -> Option<serde_json::Map<String, Value>> {
    let mut args = match &item.run {
        Run::Cmd(_, a) | Run::Ask(_, a, ..) | Run::AskNumber(_, a, ..) => a.as_object()?.clone(),
        _ => return None,
    };
    match &item.run {
        Run::Ask(_, _, arg, start) => {
            args.insert((*arg).into(), Value::from(start.as_str()));
        }
        Run::AskNumber(_, _, arg, start) => {
            args.insert(
                (*arg).into(),
                Value::from(start.parse::<f64>().unwrap_or(1.0)),
            );
        }
        _ => {}
    }
    Some(args)
}

/// What the command table says of one row, or nothing when it agrees.
fn table_says(item: &Item) -> Option<String> {
    let id = item.ledger_id.as_deref()?;
    // A row whose action the placement cannot name (a `Do` of the screen's own) is not compared.
    let named_in_table = &route(id).command;
    let runs = command_of(item);
    if !named_in_table.is_empty() && !runs.is_empty() && runs != named_in_table {
        return Some(format!("runs {runs}, the table names {named_in_table}"));
    }
    // The command the row runs itself; a name of the screen's own ("ui.") is not a command.
    let name = match &item.run {
        Run::Cmd(c, _) | Run::Key(c) | Run::Ask(c, ..) | Run::AskNumber(c, ..) => *c,
        Run::Field(kind, ..) => kind.command(),
        _ => return None,
    };
    if name.starts_with("ui.") {
        return None;
    }
    let Some(def) = registry().get(name) else {
        return Some(format!("runs {name}, which is not in the command table"));
    };
    // A dim row runs nothing: its arguments are not compared.
    if item.enabled && matches!(item.run, Run::Cmd(..) | Run::Ask(..) | Run::AskNumber(..)) {
        let Some(args) = args_of(item) else {
            return Some(format!("runs {name} with arguments that are not an object"));
        };
        if let Err(e) = registry().normalize(def, &args) {
            return Some(format!("{name} refuses its arguments: {e}"));
        }
    }
    None
}

/// Each row of a menu as it opens: its command is in the command table, takes the row's
/// arguments and is the one the table names.
pub(super) fn verify_commands(target: &str, items: &[Item]) {
    fn walk(items: &[Item], failed: &mut Vec<String>) {
        for item in items {
            if let Run::Sub(children) = &item.run {
                walk(children, failed);
                continue;
            }
            let Some(id) = &item.ledger_id else { continue };
            if let Some(e) = table_says(item) {
                failed.push(format!("{}: {e}", named(id)));
            }
        }
    }
    let mut failed = Vec::new();
    walk(items, &mut failed);
    assert!(
        failed.is_empty(),
        "{target}: {} rows disagree with the command table:\n{}",
        failed.len(),
        failed.join("\n")
    );
}

/// The menu as it stands, a row on each line: its label, its rule and whether it is dim,
/// a submenu's rows indented under it. The keys are left out: they differ by system.
fn snapshot(items: &[Item], depth: usize, out: &mut String) {
    for item in items {
        if matches!(item.run, Run::Gap | Run::Back) {
            continue;
        }
        out.push_str(&"  ".repeat(depth + 1));
        out.push_str(&item.label);
        if let Some(id) = &item.ledger_id {
            out.push_str(&format!("  [{id}]"));
        }
        if !item.enabled {
            out.push_str("  (dim)");
        }
        out.push('\n');
        if let Run::Sub(children) = &item.run {
            snapshot(children, depth + 1, out);
        }
    }
}

/// The indexes from the root to each row that can be pressed, with its rule.
fn pressable(items: &[Item], path: &mut Vec<usize>, out: &mut Vec<(Vec<usize>, String)>) {
    for (i, item) in items.iter().enumerate() {
        path.push(i);
        match (&item.run, &item.ledger_id) {
            (Run::Sub(children), _) => pressable(children, path, out),
            (Run::Gap | Run::Back | Run::Title, _) | (_, None) => {}
            (_, Some(id)) if item.enabled => out.push((path.clone(), id.clone())),
            _ => {}
        }
        path.pop();
    }
}

fn item_at<'a>(items: &'a [Item], path: &[usize]) -> &'a Item {
    let item = &items[path[0]];
    match &item.run {
        Run::Sub(children) if path.len() > 1 => item_at(children, &path[1..]),
        _ => item,
    }
}

/// Presses the row at `path` where the pop-over draws it, as the tests' `choose` does by
/// label.
fn press_at(s: &mut Studio, g: &Geometry, path: &[usize], label: &str) -> Result<(), String> {
    for (depth, &row) in path[..path.len() - 1].iter().enumerate() {
        let row = row + if depth == 0 { 0 } else { 2 };
        if !s.context_pick(g, row) {
            return Err("its submenu did not open".into());
        }
    }
    // A list longer than the window scrolls, a row at a time, until the row shows.
    let cell = loop {
        let popup = s.popup.as_mut().ok_or("no menu is open")?;
        let layout = popup.layout(g.size);
        let found = layout
            .cells
            .iter()
            .rev()
            .find(|c| c.text == label && !matches!(c.look, crate::popup::Look::Input))
            .cloned();
        match found {
            Some(cell) => break cell,
            None if popup.offset < popup.max_offset(g.size) => popup.offset += 1,
            None => return Err("no cell draws it".into()),
        }
    };
    let hit = s.hit(
        g,
        cell.rect.x + cell.rect.w / 2.0,
        cell.rect.y + cell.rect.h / 2.0,
    );
    if !matches!(hit, Hit::Item(_)) {
        return Err(format!("a press on its cell hits {hit:?}"));
    }
    s.press(g, hit);
    s.settle_browser_import();
    s.browser.settle_prefs_io();
    Ok(())
}

/// The song without its arms: a row that selects a track arms it when it is an instrument
/// track (`midi.follow-selection`), outside the history.
fn unarmed(p: &xirius_core::model::Project) -> xirius_core::model::Project {
    let mut p = p.clone();
    for t in p.tracks.values_mut() {
        let t = std::sync::Arc::make_mut(t);
        t.armed = false;
        t.monitor = false;
    }
    p
}

/// Rows that run a command of the history but, in the states they are opened in here, set what
/// stands already, so they leave no step. A row listed that leaves a step fails.
const SETS_WHAT_STANDS: &[(&str, &str)] = &[
    (
        "audio:010",
        "the clips of the sample song have the colour of their track",
    ),
    (
        "midi:007",
        "the clips of the sample song have the colour of their track",
    ),
    (
        "multi-audio:009",
        "the clips of the sample song have the colour of their track",
    ),
    (
        "multi-midi:005",
        "the clips of the sample song have the colour of their track",
    ),
    (
        "multi-mixed:005",
        "the clips of the sample song have the colour of their track",
    ),
    (
        "other-19:004",
        "the folders of the sample song have the default colour",
    ),
    ("other-23:003", "a marker starts with the default colour"),
    (
        "other-24:001",
        "the list of names has the section's own name (Verse)",
    ),
    (
        "other-54:000",
        "the list of buses has the one the send goes to",
    ),
];

/// Whether a press on the row has to leave a step: it runs a command of the history itself.
fn must_step(id: &str, item: &Item) -> bool {
    matches!(&item.run, Run::Cmd(c, _) if registry().get(c).is_some_and(|d| d.history))
        && !SETS_WHAT_STANDS.iter().any(|(row, _)| *row == id)
}

/// A press on the row: one step (none when the row does not have to leave one), and one undo
/// takes the song back. True when it made a step.
fn one_step(
    s: &mut Studio,
    g: &Geometry,
    path: &[usize],
    label: &str,
    must: bool,
) -> Result<bool, String> {
    let before = s.project();
    let n = steps(s);
    press_at(s, g, path, label)?;
    // A row that asks for a name or a number leaves its box open: a press outside closes it.
    if s.context_open() {
        s.press(g, Hit::None);
    }
    // Steps made as a group are taken back together: they count as one.
    let grouped = |s: &Studio| {
        let made = &s.song.session().steps()[n..];
        made[0].group != 0 && made.iter().all(|step| step.group == made[0].group)
    };
    match steps(s).checked_sub(n) {
        Some(0) if unarmed(&s.project()) != unarmed(&before) => {
            Err("it changed the song without a step".into())
        }
        Some(0) if must => Err("it runs a command of the history and made no step".into()),
        Some(0) => Ok(false),
        Some(d) if d == 1 || grouped(s) => {
            s.song.undo();
            if *s.project() == *before {
                Ok(true)
            } else {
                Err("one undo did not take the song back".into())
            }
        }
        Some(d) => Err(format!(
            "one press made {d} steps that one undo does not take back together"
        )),
        None => Err("a press took steps away".into()),
    }
}

fn panic_text(panic: Box<dyn std::any::Any + Send>) -> String {
    panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "?".into())
}

/// The rows of the menu `open` opens, or why it did not open.
fn opened(open: Open) -> Result<(Studio, Geometry, Vec<Item>), String> {
    let (s, g) = catch_unwind(open).map_err(|p| format!("panicked: {}", panic_text(p)))?;
    if !s.context_open() {
        return Err("the menu did not open".into());
    }
    let items = s.ctx.as_ref().ok_or("the menu has no rows")?.items.clone();
    Ok((s, g, items))
}

/// Presses each row of the menus of `target`, each on a song of its own.
fn press_every_row(target: &str) {
    let (_, states) = TARGETS.iter().find(|(t, _)| *t == target).unwrap();
    let mut failed = Vec::new();
    let mut run = 0;
    // The rows whose press left no step.
    let mut stood = Vec::new();
    for (state, open) in *states {
        let items = match opened(*open) {
            Ok((_, _, items)) => items,
            Err(e) => {
                failed.push(format!("{target} ({state}): {e}"));
                continue;
            }
        };
        let mut rows = Vec::new();
        pressable(&items, &mut Vec::new(), &mut rows);
        for (path, id) in rows {
            run += 1;
            let label = item_at(&items, &path).label.clone();
            let must = must_step(&id, item_at(&items, &path));
            let result = catch_unwind(AssertUnwindSafe(|| {
                let (mut s, g) = open();
                if !s.context_open() {
                    return Err("the menu did not open again".to_string());
                }
                one_step(&mut s, &g, &path, &label, must)
            }));
            let e = match result {
                Ok(Ok(stepped)) => {
                    if !stepped {
                        stood.push(id);
                    }
                    continue;
                }
                Ok(Err(e)) => e,
                Err(panic) => format!("panicked: {}", panic_text(panic)),
            };
            failed.push(format!(
                "{} 「{label}」 in {target} ({state}): {e}",
                named(&id)
            ));
        }
    }
    let prefix = format!("{target}:");
    for (row, why) in SETS_WHAT_STANDS {
        if row.starts_with(&prefix) && !stood.iter().any(|id| id == row) {
            failed.push(format!(
                "{}: in SETS_WHAT_STANDS (\"{why}\"), but no press of it left the song as it was",
                named(row)
            ));
        }
    }
    assert!(
        failed.is_empty(),
        "{} of {run} rows of {target} are not one step and one undo:\n{}",
        failed.len(),
        failed.join("\n")
    );
}

macro_rules! targets {
    ($($test:ident: $target:literal => [$(($state:literal, $open:path)),+ $(,)?]),* $(,)?) => {
        /// The targets opened here, each in the states its menu is opened in.
        const TARGETS: &[(&str, &[(&str, Open)])] = &[$(($target, &[$(($state, $open as Open)),+])),*];
        $(
            #[test]
            fn $test() {
                press_every_row($target);
            }
        )*
    };
}

targets! {
    empty_lane: "empty" => [("the sample song", open::empty)],
    audio_clip: "audio" => [("the sample song", open::audio)],
    midi_clip: "midi" => [
        ("the sample song", open::midi),
        ("a range over it", open::midi_range),
    ],
    audio_clips: "multi-audio" => [("two audio clips", open::multi_audio)],
    midi_clips: "multi-midi" => [("two MIDI clips", open::multi_midi)],
    mixed_clips: "multi-mixed" => [("a MIDI and an audio clip", open::multi_mixed)],
    track_head: "track" => [
        ("the sample song", open::track_row),
        ("an instrument track", open::instrument_row),
        ("a saved preset", open::track_row_preset),
    ],
    roll_note: "note" => [("three notes", open::note)],
    takes: "other-14" => [("two takes", open::takes)],
    clip_range: "other-16" => [
        ("a range over a clip", open::clip_range),
        ("the clip selected", open::clip_range_picked),
    ],
    clip_and_point: "other-17" => [("a clip and a point", open::clip_and_point)],
    two_tracks: "other-18" => [
        ("two tracks", open::two_tracks),
        ("two tracks and a bus", open::two_tracks_and_a_bus),
    ],
    folder_head: "other-19" => [("the sample song", open::folder_row)],
    under_rows: "other-20" => [
        ("the sample song", open::under_rows),
        ("a saved preset", open::under_rows_preset),
    ],
    marker_row: "other-21" => [("the sample song", open::marker_row)],
    loop_band: "other-22" => [("a loop", open::loop_band)],
    marker: "other-23" => [("a marker", open::marker)],
    section: "other-24" => [("the sample song", open::section)],
    section_gap: "other-25" => [("a gap between sections", open::section_gap)],
    tempo_point: "other-26" => [("two tempo points", open::tempo_point)],
    tempo_row: "other-27" => [("the sample song", open::tempo_row)],
    tempo_range: "other-28" => [("a range over a tempo change", open::tempo_range)],
    auto_point: "other-29" => [("two lanes", open::auto_point)],
    auto_lane: "other-30" => [
        ("two lanes", open::auto_lane),
        ("on a line", open::auto_line),
    ],
    auto_head: "other-31" => [("two lanes", open::auto_head)],
    auto_range: "other-32" => [("a range over two lanes", open::auto_range)],
    roll_grid: "other-33" => [("three notes", open::roll_grid)],
    roll_key: "other-34" => [("the keyboard", open::roll_key)],
    drum_name: "other-35" => [("the drum view", open::drum_name)],
    drum_map: "other-36" => [("the drum view", open::drum_map)],
    cc_point: "other-38" => [("a CC1 lane", open::cc_point)],
    lane_tab: "other-40" => [
        ("the velocity tab", open::lane_tab),
        ("the CC1 tab", open::cc1_tab),
    ],
    notes_and_point: "other-41" => [("notes and a point", open::notes_and_point)],
    strip_head: "other-48" => [
        ("the mixer", open::strip_head),
        ("an instrument track", open::instrument_strip_head),
        ("a saved preset", open::strip_head_preset),
    ],
    bus_head: "other-49" => [("a bus", open::bus_head)],
    master_head: "other-50" => [("the mixer", open::master_head)],
    pan_knob: "other-52" => [("the mixer", open::pan_knob)],
    send_level: "other-53" => [("a send", open::send_level)],
    send_name: "other-54" => [("a send", open::send_name)],
    add_send: "other-55" => [("a bus", open::add_send_button)],
    add_insert: "other-56" => [
        ("the mixer", open::add_insert),
        ("an effect added before", open::add_insert_recent),
    ],
    insert_heading: "other-57" => [("the mixer", open::insert_heading)],
    instrument_heading: "other-58" => [("the mixer", open::instrument_heading)],
    split_fold: "other-62" => [
        ("a splitter", open::split_fold),
        ("a splitter on an instrument track", open::instrument_split_fold),
    ],
    beside_strips: "other-69" => [
        ("the mixer", open::beside_strips),
        ("a saved preset", open::beside_strips_preset),
    ],
    insert_name: "insert" => [("a built-in effect", open::insert)],
    flow_card: "flow-plugin" => [("a rack with a splitter", open::flow_card)],
}

/// The rows of an opened target that no state opened here shows, and why.
const ABSENT: &[(&str, &str)] = &[(
    "other-48:032",
    "フリーズを解除 is on a frozen track; freezing renders through the engine",
)];

/// The targets not opened here, and why.
const NOT_OPENED: &[(&str, &str)] = &[
    (
        "audio-states",
        "rows an audio clip adds with takes or with ARA, which needs a hosted plug-in",
    ),
    (
        "midi-states",
        "rows a MIDI clip adds with takes or with ARA, which needs a hosted plug-in",
    ),
    (
        "track-states",
        "rows a frozen track adds; freezing renders through the engine",
    ),
    (
        "insert-states",
        "rows an external plug-in adds, which needs a host",
    ),
    (
        "other-15",
        "a take inside a clip's lane, after a recording of takes",
    ),
    ("other-37", "a velocity bar of the roll's lane"),
    ("other-39", "the empty part of a CC lane"),
    ("other-42", "a pad of the Drum Kit's panel"),
    ("other-43", "a sample of the Sampler's panel"),
    ("other-44", "a zone of the Sampler's panel"),
    ("other-45", "a knob of a device's panel"),
    ("other-51", "a volume fader"),
    (
        "other-59",
        "the instrument row of a strip with an instrument",
    ),
    (
        "other-60",
        "an external plug-in that crashed, which needs a host",
    ),
    (
        "other-61",
        "the crash badge of an ARA plug-in, which needs a host",
    ),
    ("other-63", "a branch's gain in the channel editor"),
    ("other-64", "a branch's pan in the channel editor"),
    ("other-65", "a splitter's card in the channel editor"),
    ("other-66", "the add-a-branch part of the channel editor"),
    ("other-67", "a macro knob"),
    (
        "other-68",
        "a plug-in of the CPU list, which needs the engine's load",
    ),
    (
        "other-70",
        "an instrument of the browser, which lists the plug-ins found on the PC",
    ),
    (
        "other-71",
        "an effect of the browser, which lists the plug-ins found on the PC",
    ),
    ("other-72", "a built-in preset of the browser"),
    ("other-73", "the empty part of the browser"),
    ("other-74", "a file of the browser, on the PC's disk"),
    ("other-75", "a folder of the browser, on the PC's disk"),
    ("other-76", "a place tab of the browser"),
    ("other-77", "a file of the song's files, on disk"),
    ("other-78", "a file of the song's files that is not found"),
    ("other-79", "the song's files tab"),
    (
        "other-80",
        "the card of a finished export, which writes a file",
    ),
    ("other-81", "the card of a failed export"),
    ("other-82", "a stem file of an export, which writes files"),
    ("graph-81", "the EQ's graph"),
    ("graph-82", "the EQ's graph"),
    ("graph-83", "the EQ's graph"),
    (
        "graph-84",
        "the filter graph of the Sampler and the Drum Kit",
    ),
    ("graph-85", "the Delay's graph, synced"),
    ("graph-86", "the points of the other devices' graphs"),
    ("graph-limiter", "the Limiter's graph"),
    ("graph-reverb", "the Reverb's graph"),
    (
        "track-browser-device-preset",
        "a device preset of the browser",
    ),
    (
        "track-browser-track-preset",
        "a track preset of the browser",
    ),
];

/// Each target's menu opened from the sample song, as the tests of each menu open it.
mod open {
    use std::collections::BTreeSet;

    use serde_json::json;
    use xirius_core::auto::AutoTarget;
    use xirius_core::id::{PluginId, SendId, TrackId};
    use xirius_core::model::CtrlKind;
    use xirius_core::rack::RackNode;

    use crate::arrange::probe::{clip_mid, folder, point_xy, row_mid, track, two_lanes};
    use crate::context::roll_tests::{note_at, notes, roll};
    use crate::context::tests::{BAR, choose, lane_at, studio};
    use crate::mixer::MixerHit;
    use crate::mixer::flow::{Area, Pick};
    use crate::roll::{LaneKind, y_of};
    use crate::studio::{DrawerTab, Geometry, Hit, Studio};
    use crate::tree::RowKind;

    type Opened = (Studio, Geometry);

    fn at(mut s: Studio, g: Geometry, (x, y): (f32, f32)) -> Opened {
        s.popup = None;
        s.scene_context(&g, x, y);
        (s, g)
    }

    pub(super) fn empty() -> Opened {
        let (s, g) = studio();
        let p = lane_at(&s, &g, "Keys", 13);
        at(s, g, p)
    }

    pub(super) fn audio() -> Opened {
        let (s, g) = studio();
        let p = clip_mid(&s, &g, "Vocal", 0);
        at(s, g, p)
    }

    pub(super) fn midi() -> Opened {
        let (s, g) = studio();
        let p = clip_mid(&s, &g, "Keys", 0);
        at(s, g, p)
    }

    pub(super) fn multi_audio() -> Opened {
        let (mut s, g) = studio();
        let vocal = track(&s, "Vocal");
        let first = s.project().tracks[&vocal].clips[0].clone();
        s.arrange_run(
            "clip.add.audio",
            json!({ "track": vocal.to_string(), "at": first.end(), "length": first.length,
                    "file": "vocal_take2.wav" }),
        )
        .expect("a second clip");
        s.rebuild(&g);
        s.arrange.clips = s.project().tracks[&vocal]
            .clips
            .iter()
            .map(|c| (vocal, c.id))
            .collect();
        let p = clip_mid(&s, &g, "Vocal", 0);
        at(s, g, p)
    }

    pub(super) fn multi_midi() -> Opened {
        let (mut s, g) = studio();
        let t = track(&s, "Keys");
        let mut clips = Vec::new();
        for at in [12 * BAR, 13 * BAR] {
            let made = s
                .song
                .run(
                    "clip.add.midi",
                    json!({ "track": t.to_string(), "at": at, "bars": 1 }),
                )
                .unwrap();
            clips.push(made["id"].as_str().unwrap().parse().unwrap());
        }
        s.rebuild(&g);
        s.arrange.clips = clips.iter().map(|id| (t, *id)).collect();
        let i = s.project().tracks[&t]
            .clips
            .iter()
            .position(|c| c.id == clips[0])
            .unwrap();
        let p = clip_mid(&s, &g, "Keys", i);
        at(s, g, p)
    }

    pub(super) fn multi_mixed() -> Opened {
        let (mut s, g) = midi();
        let vt = track(&s, "Vocal");
        let vc = s.project().tracks[&vt].clips[0].id;
        s.arrange.clips.insert((vt, vc));
        let p = clip_mid(&s, &g, "Keys", 0);
        at(s, g, p)
    }

    pub(super) fn track_row() -> Opened {
        let (s, g) = studio();
        let p = row_mid(&s, &g, RowKind::Track(track(&s, "Vocal")));
        at(s, g, p)
    }

    /// Saves Vocal as the track preset "Chain", in the test song's own folder of presets.
    fn saved(s: &mut Studio) {
        let vocal = track(s, "Vocal");
        s.song
            .run(
                "track.preset.save",
                json!({ "track": vocal.to_string(), "name": "Chain" }),
            )
            .unwrap();
    }

    pub(super) fn track_row_preset() -> Opened {
        let (mut s, g) = studio();
        saved(&mut s);
        let p = row_mid(&s, &g, RowKind::Track(track(&s, "Vocal")));
        at(s, g, p)
    }

    pub(super) fn note() -> Opened {
        let (s, g) = roll();
        let p = note_at(&s, &g, 1);
        at(s, g, p)
    }

    pub(super) fn takes() -> Opened {
        let (mut s, g) = studio();
        let t = track(&s, "Vocal");
        s.song
            .run(
                "record.take.add",
                json!({"takes":[
                    {"track":t.to_string(),"start":2*BAR,"length":2*BAR,"file":"a.wav","name":"Take 1"},
                    {"track":t.to_string(),"start":2*BAR,"length":2*BAR,"file":"b.wav","name":"Take 2"}
                ]}),
            )
            .unwrap();
        s.rebuild(&g);
        let id = s.project().tracks[&t]
            .clips
            .iter()
            .find(|c| c.name == "Take 1")
            .unwrap()
            .id;
        s.take_context_over(id, Some((2 * BAR, 3 * BAR)), 400.0, 400.0);
        (s, g)
    }

    pub(super) fn clip_range() -> Opened {
        let (mut s, g) = studio();
        let keys = track(&s, "Keys");
        let (_, y) = clip_mid(&s, &g, "Keys", 0);
        let c = s.project().tracks[&keys].clips[0].clone();
        s.arrange.range = Some((c.start + c.length / 4, c.start + c.length + 7680));
        let x = s.tick_x(&g, c.start + c.length + 480);
        at(s, g, (x, y))
    }

    /// The range over Keys' clip with the clip selected: the range's bounces.
    pub(super) fn clip_range_picked() -> Opened {
        let (mut s, g) = studio();
        let keys = track(&s, "Keys");
        let (_, y) = clip_mid(&s, &g, "Keys", 0);
        let c = s.project().tracks[&keys].clips[0].clone();
        s.arrange.clips = BTreeSet::from([(keys, c.id)]);
        s.arrange.range = Some((c.start + c.length / 4, c.start + c.length + 7680));
        let x = s.tick_x(&g, c.start + c.length + 480);
        at(s, g, (x, y))
    }

    /// Keys' clip, selected, with a range over its first half.
    pub(super) fn midi_range() -> Opened {
        let (mut s, g) = studio();
        let keys = track(&s, "Keys");
        let c = s.project().tracks[&keys].clips[0].clone();
        s.arrange.clips = BTreeSet::from([(keys, c.id)]);
        s.selected_clip = Some((keys, c.id));
        s.arrange.range = Some((c.start, c.start + c.length / 2));
        let x = s.tick_x(&g, c.start + c.length / 4);
        let (_, y) = clip_mid(&s, &g, "Keys", 0);
        at(s, g, (x, y))
    }

    pub(super) fn clip_and_point() -> Opened {
        let (mut s, g) = studio();
        let lane = two_lanes(&mut s, &g, "Bass");
        let clip_track = track(&s, "Keys");
        let clip = s.project().tracks[&clip_track].clips[0].id;
        let vol = AutoTarget::Volume;
        let tick = s.project().tracks[&lane]
            .automation
            .iter()
            .find(|l| l.target == vol)
            .expect("the volume lane")
            .points[1]
            .tick;
        s.arrange.clips = BTreeSet::from([(clip_track, clip)]);
        s.selected_clip = Some((clip_track, clip));
        s.arrange.points.select(lane, &vol, tick, false);
        let p = clip_mid(&s, &g, "Keys", 0);
        at(s, g, p)
    }

    pub(super) fn two_tracks() -> Opened {
        let (mut s, g) = studio();
        let (vocal, guitar) = (track(&s, "Vocal"), track(&s, "Guitar"));
        s.arrange.tracks = vec![RowKind::Track(vocal), RowKind::Track(guitar)];
        let p = row_mid(&s, &g, RowKind::Track(guitar));
        at(s, g, p)
    }

    pub(super) fn two_tracks_and_a_bus() -> Opened {
        let (mut s, g) = studio();
        add_bus(&mut s, &g);
        let (vocal, guitar) = (track(&s, "Vocal"), track(&s, "Guitar"));
        s.arrange.tracks = vec![RowKind::Track(vocal), RowKind::Track(guitar)];
        let p = row_mid(&s, &g, RowKind::Track(guitar));
        at(s, g, p)
    }

    pub(super) fn instrument_row() -> Opened {
        let (s, g) = studio();
        let p = row_mid(&s, &g, RowKind::Track(track(&s, "Keys")));
        at(s, g, p)
    }

    pub(super) fn folder_row() -> Opened {
        let (s, g) = studio();
        let p = row_mid(&s, &g, RowKind::Folder(folder(&s, "Rhythm")));
        at(s, g, p)
    }

    pub(super) fn under_rows() -> Opened {
        let (s, g) = studio();
        under(s, g)
    }

    pub(super) fn under_rows_preset() -> Opened {
        let (mut s, g) = studio();
        saved(&mut s);
        under(s, g)
    }

    fn under(s: Studio, g: Geometry) -> Opened {
        let row = *s.tree().rows.last().expect("rows");
        let y = s.row_y(&g, &row) + row.h + 12.0;
        let x = g.column.x + 40.0;
        at(s, g, (x, y))
    }

    pub(super) fn marker_row() -> Opened {
        let (s, g) = studio();
        let p = (s.tick_x(&g, 2 * BAR) + 2.0, g.ruler.bottom() - 3.0);
        at(s, g, p)
    }

    pub(super) fn loop_band() -> Opened {
        let (mut s, g) = studio();
        s.song
            .run("loop.set", json!({ "start": 4 * BAR, "end": 8 * BAR }))
            .unwrap();
        let p = (s.tick_x(&g, 5 * BAR) + 2.0, g.ruler.y + 4.0);
        at(s, g, p)
    }

    pub(super) fn marker() -> Opened {
        let (mut s, g) = studio();
        s.song.run("marker.add", json!({ "at": 4 * BAR })).unwrap();
        let id = s.project().markers[0].id;
        let r = s.marker_rect(&g, id).unwrap();
        at(s, g, (r.x + 4.0, r.y + 4.0))
    }

    fn band_at(s: &Studio, g: &Geometry, bar: i64) -> (f32, f32) {
        let x = s.tick_x(g, (bar - 1) * BAR) + 24.0;
        (x, g.section_band.y + g.section_band.h / 2.0)
    }

    pub(super) fn section() -> Opened {
        let (s, g) = studio();
        let p = band_at(&s, &g, 7);
        at(s, g, p)
    }

    pub(super) fn section_gap() -> Opened {
        let (mut s, g) = studio();
        let chorus = s.project().sections[2].id.to_string();
        s.song
            .run("section.delete", json!({ "id": chorus }))
            .unwrap();
        s.rebuild(&g);
        let p = band_at(&s, &g, 15);
        at(s, g, p)
    }

    pub(super) fn tempo_point() -> Opened {
        let (mut s, g) = studio();
        s.song
            .run("tempo.set", json!({ "at": 8 * BAR, "bpm": 90 }))
            .unwrap();
        let t = s.project().tempo[1].clone();
        let p = s.tempo_xy(&g, t.tick, t.bpm);
        at(s, g, p)
    }

    pub(super) fn tempo_row() -> Opened {
        let (s, g) = studio();
        let p = (s.tick_x(&g, 3 * BAR) + 2.0, g.tempo.y + 3.0);
        at(s, g, p)
    }

    pub(super) fn tempo_range() -> Opened {
        let (mut s, g) = studio();
        s.song
            .run("tempo.set", json!({ "at": 2 * BAR, "bpm": 150 }))
            .unwrap();
        s.arrange.range = Some((BAR, 3 * BAR));
        let p = (s.tick_x(&g, BAR + BAR / 2), g.tempo.y + 4.0);
        at(s, g, p)
    }

    pub(super) fn auto_point() -> Opened {
        let (mut s, g) = studio();
        let t = two_lanes(&mut s, &g, "Bass");
        let p = point_xy(&s, &g, t, 0, 1);
        at(s, g, p)
    }

    pub(super) fn auto_lane() -> Opened {
        let (mut s, g) = studio();
        let t = two_lanes(&mut s, &g, "Bass");
        let (x, y) = point_xy(&s, &g, t, 1, 1);
        // Right of the last point: no line there.
        at(s, g, (x + 200.0, y))
    }

    pub(super) fn auto_line() -> Opened {
        let (mut s, g) = studio();
        let t = two_lanes(&mut s, &g, "Bass");
        let (x0, y0) = point_xy(&s, &g, t, 1, 0);
        let (x1, y1) = point_xy(&s, &g, t, 1, 1);
        at(s, g, ((x0 + x1) / 2.0, (y0 + y1) / 2.0))
    }

    pub(super) fn auto_head() -> Opened {
        let (mut s, g) = studio();
        let t = two_lanes(&mut s, &g, "Bass");
        let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
        let p = (g.column.x + 20.0, s.lane_top(&g, &row, 0) + 24.0);
        at(s, g, p)
    }

    pub(super) fn auto_range() -> Opened {
        let (mut s, g) = studio();
        let t = two_lanes(&mut s, &g, "Bass");
        s.arrange.range = Some((4 * BAR, 8 * BAR));
        s.arrange.ranges = vec![(4 * BAR, 8 * BAR)];
        let top = s
            .lane_top_of(&g, t, &AutoTarget::Volume)
            .expect("the lane on screen");
        let p = (s.tick_x(&g, 4 * BAR + BAR / 2), top + 10.0);
        at(s, g, p)
    }

    pub(super) fn roll_grid() -> Opened {
        let (s, g) = roll();
        let c = s.roll_ctx(&g).unwrap();
        let v = s.roll.view(&c);
        let p = (v.x(3000), v.y(61) + v.row_h / 2.0);
        at(s, g, p)
    }

    /// A press on the name of the row of `pitch`, scrolled into the grid.
    fn key_name(mut s: Studio, g: Geometry, pitch: u8) -> Opened {
        let c = s.roll_ctx(&g).unwrap();
        s.roll.scroll_to_pitch(&c, pitch);
        let f = s.roll.frame(c.body);
        let v = s.roll.view(&c);
        let p = (f.keys.x + f.keys.w / 2.0, v.y(pitch) + v.row_h / 2.0);
        at(s, g, p)
    }

    pub(super) fn roll_key() -> Opened {
        let (s, g) = roll();
        key_name(s, g, 60)
    }

    fn left_press(s: &mut Studio, g: &Geometry, hit: Hit) {
        let (x, y) = s.center_of(g, hit).unwrap();
        assert_eq!(s.hit(g, x, y), hit);
        assert!(s.press(g, hit));
    }

    fn drums() -> Opened {
        let (mut s, g) = roll();
        left_press(&mut s, &g, Hit::RollView(true));
        (s, g)
    }

    pub(super) fn drum_name() -> Opened {
        let (s, g) = drums();
        key_name(s, g, 36)
    }

    pub(super) fn drum_map() -> Opened {
        let (mut s, g) = drums();
        left_press(&mut s, &g, Hit::RollMap);
        (s, g)
    }

    /// The roll with a CC1 lane of three points: 0 → 10, 480 → 90, 960 → 40.
    fn with_cc1() -> (Studio, Geometry, usize) {
        let (mut s, g) = roll();
        let c = s.roll_ctx(&g).unwrap();
        s.song
            .run(
                "cc.replace",
                json!({ "clip": c.clip.id.to_string(), "lane": "cc1", "from": 0, "to": 1920,
                        "points": [{ "tick": 0, "value": 10 }, { "tick": 480, "value": 90 },
                                   { "tick": 960, "value": 40 }] }),
            )
            .unwrap();
        s.resized(&g);
        let index = s
            .roll
            .lanes
            .iter()
            .position(|l| l.kind == LaneKind::Ctrl(CtrlKind::Cc(1)))
            .unwrap();
        (s, g, index)
    }

    pub(super) fn cc_point() -> Opened {
        let (s, g, index) = with_cc1();
        let c = s.roll_ctx(&g).unwrap();
        let plot = s.roll.frame(c.body).lanes[index].plot;
        let v = s.roll.view(&c);
        let p = (v.x(480), y_of(s.roll.lanes[index].kind, &plot, 90));
        at(s, g, p)
    }

    pub(super) fn lane_tab() -> Opened {
        let (s, g, _) = with_cc1();
        let c = s.roll_ctx(&g).unwrap();
        let r = s.roll.frame(c.body).tabs[0];
        at(s, g, (r.x + r.w / 2.0, r.y + r.h / 2.0))
    }

    pub(super) fn cc1_tab() -> Opened {
        let (s, g, index) = with_cc1();
        let c = s.roll_ctx(&g).unwrap();
        let r = s.roll.frame(c.body).tabs[index];
        at(s, g, (r.x + r.w / 2.0, r.y + r.h / 2.0))
    }

    pub(super) fn notes_and_point() -> Opened {
        let (mut s, g, index) = with_cc1();
        let ids: Vec<_> = notes(&s, &g).iter().take(2).map(|n| n.id).collect();
        s.roll.selected = ids.into_iter().collect();
        s.roll.active_lane = index;
        s.roll.points = [480].into();
        let p = note_at(&s, &g, 1);
        at(s, g, p)
    }

    /// The sample song with the mixer open, its flow 600 high.
    fn mix() -> Opened {
        let mut s = crate::studio::Studio::sample();
        s.drawer_open = true;
        s.tab = DrawerTab::Mixer;
        s.drawer_size.mixer = Some(600.0);
        let g = s.geometry(1600.0, 1000.0);
        s.resized(&g);
        (s, g)
    }

    fn on_mixer(s: Studio, g: Geometry, hit: MixerHit) -> Opened {
        let r = s.mixer_rect(&g, hit).expect("on screen");
        // a heading is pressed left of its ＋
        let p = match hit {
            MixerHit::Heading(..) => (r.x + 12.0, r.y + r.h / 2.0),
            _ => (r.x + r.w / 2.0, r.y + r.h / 2.0),
        };
        at(s, g, p)
    }

    fn head(s: Studio, g: Geometry, t: TrackId) -> Opened {
        let r = s
            .mixer_strips(&g)
            .into_iter()
            .find(|x| x.track == t)
            .expect("a strip")
            .name;
        at(s, g, (r.x + r.w / 2.0, r.y + r.h / 2.0))
    }

    fn add_bus(s: &mut Studio, g: &Geometry) -> TrackId {
        assert!(s.run_command("track.add.bus", json!({ "name": "Fx", "into": "root" })));
        s.resized(g);
        track(s, "Fx")
    }

    fn add_send(s: &mut Studio, t: TrackId, to: TrackId) -> SendId {
        assert!(s.run_command(
            "send.add",
            json!({ "ids": [t.to_string()], "to": to.to_string() })
        ));
        s.project().tracks[&t].sends.last().expect("sent").id
    }

    fn add_effect(s: &mut Studio, t: TrackId, name: &str) -> PluginId {
        assert!(s.run_command(
            "plugin.add",
            json!({ "track": t.to_string(), "name": name, "kind": "effect", "format": "builtin",
                    "uid": name }),
        ));
        s.project().tracks[&t].inserts.last().unwrap().id
    }

    pub(super) fn strip_head() -> Opened {
        let (s, g) = mix();
        let t = track(&s, "Vocal");
        head(s, g, t)
    }

    pub(super) fn strip_head_preset() -> Opened {
        let (mut s, g) = mix();
        saved(&mut s);
        let t = track(&s, "Vocal");
        head(s, g, t)
    }

    pub(super) fn instrument_strip_head() -> Opened {
        let (s, g) = mix();
        let t = track(&s, "Keys");
        head(s, g, t)
    }

    pub(super) fn bus_head() -> Opened {
        let (mut s, g) = mix();
        let bus = add_bus(&mut s, &g);
        let g = s.geometry(1600.0, 1000.0);
        head(s, g, bus)
    }

    pub(super) fn master_head() -> Opened {
        let (s, g) = mix();
        head(s, g, TrackId::MASTER)
    }

    pub(super) fn pan_knob() -> Opened {
        let (s, g) = mix();
        let t = track(&s, "Vocal");
        on_mixer(s, g, MixerHit::Pan(t))
    }

    fn with_send() -> (Studio, Geometry, TrackId, SendId) {
        let (mut s, g) = mix();
        let vocal = track(&s, "Vocal");
        let bus = add_bus(&mut s, &g);
        let send = add_send(&mut s, vocal, bus);
        (s, g, vocal, send)
    }

    pub(super) fn send_level() -> Opened {
        let (s, g, t, send) = with_send();
        on_mixer(s, g, MixerHit::SendLevel(t, send))
    }

    pub(super) fn send_name() -> Opened {
        let (s, g, t, send) = with_send();
        on_mixer(s, g, MixerHit::SendPre(t, send))
    }

    pub(super) fn add_send_button() -> Opened {
        let (mut s, g) = mix();
        let vocal = track(&s, "Vocal");
        add_bus(&mut s, &g);
        on_mixer(s, g, MixerHit::AddSend(vocal))
    }

    pub(super) fn add_insert() -> Opened {
        let (s, g) = mix();
        let t = track(&s, "Keys");
        on_mixer(s, g, MixerHit::AddInsert(t))
    }

    pub(super) fn add_insert_recent() -> Opened {
        let (mut s, g) = mix();
        s.mixer.recent = vec![crate::instrument::Pick {
            name: "EQ".into(),
            format: "builtin".into(),
            uid: "eq".into(),
        }];
        let t = track(&s, "Keys");
        on_mixer(s, g, MixerHit::AddInsert(t))
    }

    pub(super) fn insert_heading() -> Opened {
        let (s, g) = mix();
        let t = track(&s, "Vocal");
        on_mixer(s, g, MixerHit::Heading(t, Area::Insert))
    }

    pub(super) fn instrument_heading() -> Opened {
        let (s, g) = mix();
        let t = track(&s, "Keys");
        on_mixer(s, g, MixerHit::Heading(t, Area::Instrument))
    }

    /// The fold of a splitter added after an EQ on `name`'s inserts.
    fn fold_on(name: &str) -> Opened {
        let (mut s, g) = mix();
        let t = track(&s, name);
        add_effect(&mut s, t, "EQ");
        let (mut s, g) = on_mixer(s, g, MixerHit::Heading(t, Area::Insert));
        assert!(choose(&mut s, &g, "スプリッターを追加"));
        let id = match s.project().tracks[&t].rack.as_ref().unwrap().fx.last() {
            Some(RackNode::Split(sp)) => sp.id,
            other => panic!("{other:?}"),
        };
        on_mixer(s, g, MixerHit::Fold(t, id))
    }

    pub(super) fn split_fold() -> Opened {
        fold_on("Vocal")
    }

    pub(super) fn instrument_split_fold() -> Opened {
        fold_on("Keys")
    }

    pub(super) fn beside_strips() -> Opened {
        let (s, g) = mix();
        beside(s, g)
    }

    pub(super) fn beside_strips_preset() -> Opened {
        let (mut s, g) = mix();
        saved(&mut s);
        beside(s, g)
    }

    fn beside(s: Studio, g: Geometry) -> Opened {
        let last = s
            .mixer_strips(&g)
            .into_iter()
            .rfind(|x| !x.track.is_master())
            .expect("a strip")
            .rect;
        at(s, g, (last.right() + 15.0, last.y + 60.0))
    }

    pub(super) fn insert() -> Opened {
        let (mut s, g) = mix();
        let vocal = track(&s, "Vocal");
        assert!(s.run_command(
            "plugin.add",
            json!({ "track": vocal, "name": "Compressor" })
        ));
        let comp = s.project().tracks[&vocal].inserts.last().unwrap().id;
        on_mixer(s, g, MixerHit::Open(vocal, comp))
    }

    /// Vocal's channel editor with EQ, then a splitter of Comp and Reverb: the menu of EQ's
    /// card.
    pub(super) fn flow_card() -> Opened {
        let mut s = crate::studio::Studio::sample();
        s.instruments.open.clear();
        let g = s.geometry(1600.0, 1000.0);
        let t = track(&s, "Vocal");
        let add = |s: &mut Studio, name: &str| {
            assert!(s.run_command("plugin.add", json!({ "track": t, "name": name })));
            s.project().tracks[&t].inserts.last().unwrap().id
        };
        let eq = add(&mut s, "EQ");
        let comp = add(&mut s, "Comp");
        let verb = add(&mut s, "Reverb");
        assert!(s.run_command("rack.split.add", json!({ "track": t })));
        let b = match &s.project().tracks[&t].rack.as_ref().unwrap().fx[..] {
            [.., RackNode::Split(x)] => [x.branches[0].id, x.branches[1].id],
            other => panic!("{other:?}"),
        };
        assert!(s.run_command("rack.node.move", json!({ "id": comp, "to": b[0] })));
        assert!(s.run_command("rack.node.move", json!({ "id": verb, "to": b[1] })));
        s.open_channel(t);
        let v = s.channel_view(&g).unwrap();
        let card = v
            .flow
            .cards
            .iter()
            .find(|c| c.pick == Pick::Plugin(eq))
            .cloned()
            .unwrap();
        let r = crate::channel::card_parts(&card).more;
        assert!(s.channel_press(&g, r.x + r.w / 2.0, r.y + r.h / 2.0, 1));
        s.channel_up(&g);
        (s, g)
    }
}

#[test]
fn every_target_of_the_table_is_opened_here_or_says_why_and_its_menus_are_as_written() {
    let mut failed = Vec::new();
    let mut targets: Vec<&str> = routes().iter().map(|r| r.target.as_str()).collect();
    targets.sort();
    targets.dedup();
    for target in &targets {
        let opened = TARGETS.iter().any(|(t, _)| t == target);
        let listed = NOT_OPENED.iter().any(|(t, _)| t == target);
        if opened == listed {
            let how = if opened {
                "opened, and listed in NOT_OPENED"
            } else {
                "neither in TARGETS nor in NOT_OPENED"
            };
            failed.push(format!("{target}: {how}"));
        }
    }
    let mut menus = String::new();
    for (target, states) in TARGETS {
        let mut seen = std::collections::BTreeSet::new();
        for (state, open) in *states {
            let items = match opened(*open) {
                Ok((_, _, items)) => items,
                Err(e) => {
                    failed.push(format!("{target} ({state}): {e}"));
                    continue;
                }
            };
            menus.push_str(&format!("{target} ({state})\n"));
            snapshot(&items, 0, &mut menus);
            let mut rows = Vec::new();
            ids(&items, &mut rows);
            seen.extend(rows);
        }
        for r in routes().iter().filter(|r| r.target == *target) {
            let absent = ABSENT.iter().any(|(id, _)| *id == r.id);
            match (seen.contains(&r.id), absent) {
                (false, false) => failed.push(format!(
                    "{}: not in any menu of {target} opened here (open it in a state that has \
                     it, or list it in ABSENT with the reason)",
                    named(&r.id)
                )),
                (true, true) => failed.push(format!(
                    "{}: in a menu of {target} opened here, and listed in ABSENT",
                    named(&r.id)
                )),
                _ => {}
            }
        }
    }
    let file = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/context_menus/menus.txt");
    if std::env::var_os("XIRIUS_WRITE_MENUS").is_some() {
        std::fs::write(file, &menus).unwrap();
    }
    let kept = std::fs::read_to_string(file)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    if kept != menus {
        let line = kept
            .lines()
            .zip(menus.lines())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| kept.lines().count().min(menus.lines().count()));
        let heading = |text: &str| {
            text.lines()
                .take(line + 1)
                .filter(|l| !l.starts_with(' '))
                .last()
                .unwrap_or("")
                .to_string()
        };
        failed.push(format!(
            "the menus differ from tests/context_menus/menus.txt at line {} (in {}):\n  written: {}\n  now:     {}\n  \
             (if the change is meant, XIRIUS_WRITE_MENUS=1 writes the file again)",
            line + 1,
            heading(&menus),
            kept.lines().nth(line).unwrap_or("(the end)"),
            menus.lines().nth(line).unwrap_or("(the end)")
        ));
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

fn ids(items: &[Item], out: &mut Vec<String>) {
    for item in items {
        if let Some(id) = &item.ledger_id {
            // A dynamic rule with nothing to offer shows as an empty submenu.
            if !matches!(&item.run, Run::Sub(rows) if rows.is_empty()) {
                out.push(id.clone());
            }
        }
        if let Run::Sub(children) = &item.run {
            ids(children, out);
        }
    }
}
