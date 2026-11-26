//! The second round of the editing feel (impl-feel2), driven through the
//! screen's own press and release entry points.
use super::*;
use crate::studio::{Geometry, Studio};
use crate::tools::{Mods, Tool};

fn setup() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 820.0);
    let ctx = s.roll_ctx(&g).unwrap();
    s.song
        .run(
            "note.delete",
            json!({"ids":ids_json(ctx.notes().iter().map(|n| &n.id))}),
        )
        .unwrap();
    s.song
        .run(
            "note.put",
            json!({"clip":ctx.clip.id.to_string(),"notes":[
                {"at":960,"pitch":60,"length":960,"velocity":80},
                {"at":2880,"pitch":62,"length":1440,"velocity":90},
                {"at":4800,"pitch":64,"length":480,"velocity":100}
            ]}),
        )
        .unwrap();
    s.resized(&g);
    (s, g)
}
fn context(s: &Studio, g: &Geometry) -> Ctx {
    s.roll_ctx(g).unwrap()
}
fn upper(s: &Studio, g: &Geometry, i: usize) -> (f32, f32) {
    let c = context(s, g);
    let v = s.roll.view(&c);
    let r = super::notes::note_rect(&v, &NoteView::from(&c.notes()[i]));
    (r.x + r.w / 2.0, r.y + 1.0)
}
/// One click as the screen delivers it: `Screen::pressed` then `Screen::released`.
fn click(s: &mut Studio, g: &Geometry, p: (f32, f32), m: Mods) {
    assert!(s.roll_press(g, p.0, p.1, m, 1));
    let c = context(s, g);
    let out = s.roll.up(&c, p.0, p.1, m);
    s.apply_roll(g, out);
    assert!(s.error.is_none(), "{:?}", s.error);
}

#[test]
fn edt_051_alt_click_on_the_upper_part_mutes_the_note_and_greys_it() {
    let (mut s, g) = setup();
    s.roll.extended = true;
    s.tool = Tool::Select;
    let before = context(&s, &g).notes().to_vec();
    let steps = s.song.steps();
    let p = upper(&s, &g, 1);
    click(&mut s, &g, p, Mods::alt());
    let c = context(&s, &g);
    let n = &c.notes()[1];
    assert!(n.muted);
    // Only the mute changes: the strength and the place stay.
    assert_eq!(n.velocity, before[1].velocity);
    assert_eq!(
        (n.start, n.length, n.pitch),
        (before[1].start, before[1].length, before[1].pitch)
    );
    assert!(!c.notes()[0].muted && !c.notes()[2].muted);
    assert_eq!(s.song.steps(), steps + 1);
    // What is painted is the muted (grey) face.
    assert!(s.roll.shown(n).muted);
    // A second Alt+click brings it back, again as one step.
    click(&mut s, &g, p, Mods::alt());
    assert!(!context(&s, &g).notes()[1].muted);
    assert_eq!(s.song.steps(), steps + 2);
    // Ctrl+Z goes back one click at a time.
    assert!(s.edit_key(&g, "z", Mods::ctrl()));
    assert!(context(&s, &g).notes()[1].muted);
    // Without Alt the same place changes the strength instead and does not mute.
    assert!(s.edit_key(&g, "z", Mods::ctrl()));
    click(&mut s, &g, p, Mods::NONE);
    assert!(!context(&s, &g).notes()[1].muted);
}

/// UI-088, UI-284, UI-295: in the roll, → ← move the selected notes a sixteenth (on a quarter
/// grid too), ↑ ↓ a semitone, Shift+↑ ↓ an octave. With nothing selected the key picks a note and
/// moves none, as before. Each press is one step undone on its own, the note stays selected,
/// the Alt form is kept (Alt+→ by the grid, Alt+↑ a semitone, Alt+Shift+↑ an octave), and
/// several selected notes move together in one step that one Ctrl+Z takes back.
#[test]
fn the_arrow_keys_move_the_selected_notes_by_their_step_each_press_one_undo() {
    type Of = fn(&Note) -> i64;
    // (row, key, back, mods, its step, Alt's mods, Alt's step, what moves)
    type Row = (
        &'static str,
        &'static str,
        &'static str,
        Mods,
        i64,
        Mods,
        i64,
        Of,
    );
    let rows: [Row; 3] = [
        (
            "→ a sixteenth",
            "right",
            "left",
            Mods::NONE,
            240,
            Mods::alt(),
            960,
            |n| n.start,
        ),
        (
            "↑ a semitone",
            "up",
            "down",
            Mods::NONE,
            1,
            Mods::alt(),
            1,
            |n| i64::from(n.pitch),
        ),
        (
            "Shift+↑ an octave",
            "up",
            "down",
            Mods::shift(),
            12,
            Mods::alt_shift(),
            12,
            |n| i64::from(n.pitch),
        ),
    ];
    let mut failures = Vec::new();
    for (row, key, back, mods, by, alt, alt_by, of) in rows {
        let (mut s, g) = setup();
        s.tool = Tool::Select;
        // A quarter grid: the arrows still go by a sixteenth.
        s.song
            .run("view.grid", json!({"grid": "1/4", "editor": "roll"}))
            .unwrap();
        s.set_grid_mode(crate::tools::SnapMode::Quantize);
        assert_eq!(s.input_context(&g), "roll", "{row}");
        let steps = s.song.steps();
        let press = |s: &mut Studio, k: &str, m: Mods| {
            assert!(s.edit_key(&g, k, m), "{row}: {k}");
            assert!(s.error.is_none(), "{row}: {:?}", s.error);
        };
        press(&mut s, key, mods);
        if (s.roll.selected.len(), s.song.steps()) != (1, steps) {
            failures.push(format!(
                "{row}: with nothing selected the key did not only pick one"
            ));
            continue;
        }
        let id = *s.roll.selected.iter().next().unwrap();
        let at = |s: &Studio| of(context(s, &g).note(id).unwrap());
        let all = |s: &Studio| context(s, &g).notes().iter().map(of).collect::<Vec<_>>();
        let was = at(&s);
        press(&mut s, key, mods);
        let one = (at(&s), s.song.steps());
        press(&mut s, back, mods);
        press(&mut s, back, mods);
        let back_twice = (at(&s), s.song.steps(), s.roll.selected.len());
        press(&mut s, key, alt);
        let by_alt = at(&s);
        press(&mut s, "z", Mods::ctrl());
        let undone = at(&s);
        press(&mut s, "a", Mods::ctrl());
        let before = all(&s);
        press(&mut s, back, mods);
        let together = all(&s);
        press(&mut s, "z", Mods::ctrl());
        let got = (one, back_twice, by_alt, undone, together, all(&s));
        let want = (
            (was + by, steps + 1),
            (was - by, steps + 3, 1),
            was - by + alt_by,
            was - by,
            before.iter().map(|v| v - by).collect::<Vec<_>>(),
            before.clone(),
        );
        if got != want {
            failures.push(format!("{row}: {got:?}, want {want:?}"));
        }
    }
    assert!(failures.is_empty(), "the arrow keys: {failures:#?}");
}

/// The id a command made.
fn made(v: serde_json::Value) -> String {
    v["id"]
        .as_str()
        .or_else(|| v["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {v}"))
        .to_string()
}

/// Every note of the MIDI parts of these tracks: (id, start in the song, pitch).
fn notes_on(s: &Studio, tracks: &[xirius_core::id::TrackId]) -> Vec<(NoteId, Tick, u8)> {
    let p = s.project();
    let mut v: Vec<_> = tracks
        .iter()
        .flat_map(|t| p.tracks[t].clips.iter())
        .flat_map(|c| c.notes().iter().map(|n| (n.id, c.start + n.start, n.pitch)))
        .collect();
    v.sort();
    v
}

#[test]
fn edt_062_parts_picked_on_two_tracks_are_edited_together_in_one_step() {
    let (mut s, g) = setup();
    s.tool = Tool::Select;
    s.song
        .run("view.grid", json!({"grid": "1/4", "editor": "roll"}))
        .unwrap();
    s.set_grid_mode(crate::tools::SnapMode::Quantize);
    let first = context(&s, &g).clip.clone();
    let keys = s.selected_clip.unwrap().0;
    // A second part on the Keys track after its others, and a part on a new track.
    let end = s.project().tracks[&keys]
        .clips
        .iter()
        .map(|c| c.start + c.length)
        .max()
        .unwrap();
    let later = made(
        s.song
            .run(
                "clip.add.midi",
                json!({"track": keys.to_string(), "at": end, "bars": 1}),
            )
            .unwrap(),
    );
    s.song
        .run(
            "note.put",
            json!({"clip": later, "notes": [{"at": 480, "pitch": 67, "length": 480, "velocity": 90}]}),
        )
        .unwrap();
    let lead = made(
        s.song
            .run("track.add.instrument", json!({"name": "Lead"}))
            .unwrap(),
    );
    let other = made(
        s.song
            .run(
                "clip.add.midi",
                json!({"track": lead, "at": first.start, "bars": 1}),
            )
            .unwrap(),
    );
    s.song
        .run(
            "note.put",
            json!({"clip": other, "notes": [
                {"at": 0, "pitch": 72, "length": 480, "velocity": 90},
                {"at": 960, "pitch": 74, "length": 480, "velocity": 90}
            ]}),
        )
        .unwrap();
    s.rebuild(&g);
    let lead: xirius_core::id::TrackId = lead.parse().unwrap();
    let other: xirius_core::id::ClipId = other.parse().unwrap();

    // The Keys part, then Shift and the Lead's part, in the arrangement.
    // Each row scrolled into view first, as the hand would.
    let middle = |s: &mut Studio, t, c| {
        let row = *s
            .tree()
            .row_of(crate::tree::RowKind::Track(t))
            .expect("row");
        s.scroll.1 = (row.y - 8.0).max(0.0);
        s.clamp_scroll(&g);
        let k = s.project().tracks[&t]
            .clips
            .iter()
            .find(|k| k.id == c)
            .unwrap()
            .clone();
        let r = s.clip_rect(&g, &row, &k);
        assert!(
            g.lanes.contains(r.x + 4.0, r.y + r.h / 2.0),
            "{r:?} in view"
        );
        (r.x + 4.0, r.y + r.h / 2.0)
    };
    let p = middle(&mut s, keys, first.id);
    assert!(s.arrange_down(&g, p.0, p.1, Mods::NONE));
    s.arrange_up(&g, p.0, p.1, Mods::NONE);
    let p = middle(&mut s, lead, other);
    assert!(s.arrange_down(&g, p.0, p.1, Mods::shift()));
    s.arrange_up(&g, p.0, p.1, Mods::shift());
    assert_eq!(s.arrange.clips.len(), 2);

    // Into the roll: its clip and the other picked part are shown together.
    let c = context(&s, &g);
    let v = s.roll.view(&c);
    let empty = (v.grid.x + 4.0, v.grid.y + 2.0);
    assert!(s.roll_press(&g, empty.0, empty.1, Mods::NONE, 1));
    let out = s.roll.up(&context(&s, &g), empty.0, empty.1, Mods::NONE);
    s.apply_roll(&g, out);
    assert_eq!(s.input_context(&g), "roll");
    let c = context(&s, &g);
    assert_eq!(c.others.len(), 1, "the other picked part");

    // Mod+A: the notes of the roll's part only.
    assert!(s.edit_key(&g, "a", Mods::ctrl()));
    let c = context(&s, &g);
    assert_eq!(s.roll.selected.len(), c.notes().len());
    assert!(s.roll.selected_elsewhere(&c).is_empty());

    // Mod+Shift+A: every note of every part on both tracks.
    assert!(s.edit_key(&g, "a", Mods::ctrl_shift()));
    let both = [keys, lead];
    let all = notes_on(&s, &both);
    assert!(all.len() >= 6);
    let c = context(&s, &g);
    // The last part clicked is in the roll; the others are shown with it.
    assert_eq!(c.clip.id, other);
    let shown: Vec<String> = c.others.iter().map(|p| p.clip.id.to_string()).collect();
    assert!(shown.contains(&later) && shown.contains(&first.id.to_string()));
    assert!(all.iter().all(|(id, ..)| s.roll.selected.contains(id)));

    // → moves all of them a sixteenth, in one step; Alt+↑ a semitone up, again in one.
    let steps = s.song.steps();
    assert!(s.edit_key(&g, "right", Mods::NONE));
    assert!(s.error.is_none(), "{:?}", s.error);
    let moved = notes_on(&s, &both);
    assert_eq!(
        moved,
        all.iter()
            .map(|&(id, at, pitch)| (id, at + 240, pitch))
            .collect::<Vec<_>>()
    );
    assert_eq!(s.song.steps(), steps + 1);
    assert!(s.edit_key(&g, "up", Mods::alt()));
    assert_eq!(
        notes_on(&s, &both),
        all.iter()
            .map(|&(id, at, pitch)| (id, at + 240, pitch + 1))
            .collect::<Vec<_>>()
    );
    assert_eq!(s.song.steps(), steps + 2);
    // One Ctrl+Z takes each back from every part.
    assert!(s.edit_key(&g, "z", Mods::ctrl()));
    assert!(s.edit_key(&g, "z", Mods::ctrl()));
    assert_eq!(notes_on(&s, &both), all);

    // Delete takes every selected note of every part, and one Ctrl+Z brings them back.
    assert!(s.edit_key(&g, "a", Mods::ctrl_shift()));
    assert!(s.edit_key(&g, "delete", Mods::NONE));
    assert!(notes_on(&s, &both).is_empty());
    assert!(s.edit_key(&g, "z", Mods::ctrl()));
    assert_eq!(notes_on(&s, &both), all);
}

/// UI-284 with the audition setting: the picked or moved note sounds while the key is down (the
/// top one of several), and without the setting nothing sounds. The moves themselves are the
/// table above.
#[test]
fn ui_284_the_up_and_down_arrows_sound_the_moved_note_only_with_the_audition_setting() {
    let (mut s, g) = setup();
    s.tool = Tool::Select;
    s.rec_set(
        "prefs.set",
        json!({"key": "edit.audition", "value": "true"}),
    );
    assert!(s.edit_key(&g, "up", Mods::NONE));
    let id = *s.roll.selected.iter().next().unwrap();
    let was = context(&s, &g).note(id).unwrap().pitch;
    assert_eq!(s.roll.sounding(), Some(was), "the picked note sounds");
    assert!(s.key_sound_up());
    assert_eq!(s.roll.sounding(), None);
    assert!(s.edit_key(&g, "up", Mods::NONE));
    assert_eq!(s.roll.sounding(), Some(was + 1), "the moved note sounds");
    assert!(s.key_sound_up());
    assert!(s.edit_key(&g, "a", Mods::ctrl()));
    let pitches =
        |s: &Studio| -> Vec<_> { context(s, &g).notes().iter().map(|n| n.pitch).collect() };
    let before = pitches(&s);
    assert!(s.edit_key(&g, "up", Mods::NONE));
    assert_eq!(
        s.roll.sounding(),
        before.iter().max().map(|p| p + 1),
        "the top one sounds"
    );
    assert!(s.key_sound_up());
    assert!(s.edit_key(&g, "z", Mods::ctrl()));
    // Without the audition setting the keys move the notes and nothing sounds.
    s.rec_set(
        "prefs.set",
        json!({"key": "edit.audition", "value": "false"}),
    );
    assert!(s.edit_key(&g, "down", Mods::NONE));
    assert_eq!(
        pitches(&s),
        before.iter().map(|p| p - 1).collect::<Vec<_>>()
    );
    assert_eq!(s.roll.sounding(), None);
    assert!(!s.key_sound_up());
}

/// UI-284, UI-295: the keys that pick rather than move. Outside the roll ↓ steps the clip
/// selection and Shift+↓ grows it; in the roll Mod+Shift+↑ grows the note selection. None of
/// them moves a note or makes a step.
#[test]
fn the_arrow_keys_that_pick_grow_or_step_the_selection_and_move_no_note() {
    type Picked = fn(&Studio) -> Vec<String>;
    type Right = fn(&[String], &[String]) -> bool;
    fn clips(s: &Studio) -> Vec<String> {
        s.picked_clips().iter().map(|c| format!("{c:?}")).collect()
    }
    // (row, in the roll, key, mods, what is picked, is it right: before, after)
    let rows: [(&str, bool, &str, Mods, Picked, Right); 3] = [
        (
            "↓ outside the roll steps the clips",
            false,
            "down",
            Mods::NONE,
            clips,
            |before, after| after != before,
        ),
        (
            "Shift+↓ outside the roll grows the clips",
            false,
            "down",
            Mods::shift(),
            clips,
            |before, after| after.len() > before.len(),
        ),
        (
            "Mod+Shift+↑ in the roll grows the notes",
            true,
            "up",
            Mods::ctrl_shift(),
            |s| s.roll.selected.iter().map(|n| n.to_string()).collect(),
            |before, after| (before.len(), after.len()) == (1, 2),
        ),
    ];
    let mut failures = Vec::new();
    for (row, in_roll, key, mods, picked, right) in rows {
        let (mut s, g) = setup();
        s.tool = Tool::Select;
        if in_roll {
            // One note picked and taken an octave down, so there are notes above it.
            assert!(s.edit_key(&g, "up", Mods::shift()), "{row}");
            assert!(s.edit_key(&g, "down", Mods::shift()), "{row}");
        } else {
            assert!(s.edit_key(&g, "a", Mods::ctrl()), "{row}");
            assert!(!s.roll.selected.is_empty(), "{row}");
            s.arrange.focus = true;
        }
        if (s.input_context(&g) == "roll") != in_roll {
            failures.push(format!(
                "{row}: the input context is {}",
                s.input_context(&g)
            ));
            continue;
        }
        let notes = context(&s, &g).notes().to_vec();
        let (steps, before) = (s.song.steps(), picked(&s));
        assert!(s.edit_key(&g, key, mods), "{row}");
        let after = picked(&s);
        if !right(&before, &after) {
            failures.push(format!("{row}: picked {before:?} -> {after:?}"));
        }
        // Outside the roll the part shown follows the clips, so only the steps tell.
        let moved = in_roll && context(&s, &g).notes() != notes.as_slice();
        if s.song.steps() != steps || moved {
            failures.push(format!("{row}: a note moved"));
        }
    }
    assert!(failures.is_empty(), "the keys that pick: {failures:#?}");
}
