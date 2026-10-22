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

#[test]
fn ui_088_the_arrows_alone_move_the_selected_notes_by_a_sixteenth_in_one_step() {
    let (mut s, g) = setup();
    s.tool = Tool::Select;
    // A quarter grid: the arrows still go by a sixteenth.
    s.song
        .run("view.grid", json!({"grid": "1/4", "editor": "roll"}))
        .unwrap();
    s.set_grid_mode(crate::tools::SnapMode::Quantize);
    assert_eq!(s.input_context(&g), "roll");
    // With nothing selected the arrow picks a note, as before.
    let steps = s.song.steps();
    assert!(s.edit_key(&g, "right", Mods::NONE));
    assert_eq!(s.roll.selected.len(), 1);
    assert_eq!(s.song.steps(), steps);
    let id = *s.roll.selected.iter().next().unwrap();
    let start = |s: &Studio| {
        context(s, &g)
            .notes()
            .iter()
            .find(|n| n.id == id)
            .unwrap()
            .start
    };
    let was = start(&s);
    // Then → moves it a sixteenth later as one step, ← back.
    assert!(s.edit_key(&g, "right", Mods::NONE));
    assert!(s.error.is_none(), "{:?}", s.error);
    assert_eq!(start(&s), was + 240);
    assert_eq!(s.song.steps(), steps + 1);
    assert!(s.edit_key(&g, "right", Mods::NONE));
    assert!(s.edit_key(&g, "left", Mods::NONE));
    assert_eq!(start(&s), was + 240);
    assert_eq!(s.song.steps(), steps + 3);
    assert_eq!(s.roll.selected.len(), 1, "the note stays selected");
    // Alt+→ still goes by the grid.
    assert!(s.edit_key(&g, "right", Mods::alt()));
    assert_eq!(start(&s), was + 240 + 960);
    // Each press is undone on its own.
    assert!(s.edit_key(&g, "z", Mods::ctrl()));
    assert!(s.edit_key(&g, "z", Mods::ctrl()));
    assert_eq!(start(&s), was + 480);
    // Several selected notes move together, in one step.
    assert!(s.edit_key(&g, "a", Mods::ctrl()));
    let before: Vec<_> = context(&s, &g).notes().iter().map(|n| n.start).collect();
    assert!(s.edit_key(&g, "left", Mods::NONE));
    let starts =
        |s: &Studio| -> Vec<_> { context(s, &g).notes().iter().map(|n| n.start).collect() };
    assert_eq!(
        starts(&s),
        before.iter().map(|t| t - 240).collect::<Vec<_>>()
    );
    assert!(s.edit_key(&g, "z", Mods::ctrl()));
    assert_eq!(starts(&s), before, "one Ctrl+Z takes back the whole move");
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

#[test]
fn ui_284_the_up_and_down_arrows_move_the_selected_notes_a_semitone_in_one_step() {
    let (mut s, g) = setup();
    s.tool = Tool::Select;
    s.rec_set(
        "prefs.set",
        json!({"key": "edit.audition", "value": "true"}),
    );
    assert_eq!(s.input_context(&g), "roll");
    // With nothing selected ↑ picks a note, as before, and the note sounds while the key is down.
    let steps = s.song.steps();
    assert!(s.edit_key(&g, "up", Mods::NONE));
    assert_eq!(s.roll.selected.len(), 1);
    assert_eq!(s.song.steps(), steps);
    let id = *s.roll.selected.iter().next().unwrap();
    let pitch = |s: &Studio| {
        context(s, &g)
            .notes()
            .iter()
            .find(|n| n.id == id)
            .unwrap()
            .pitch
    };
    let was = pitch(&s);
    assert_eq!(s.roll.sounding(), Some(was));
    assert!(s.key_sound_up());
    assert_eq!(s.roll.sounding(), None);
    // Then ↑ moves it a semitone up as one step, and the moved note sounds.
    assert!(s.edit_key(&g, "up", Mods::NONE));
    assert!(s.error.is_none(), "{:?}", s.error);
    assert_eq!(pitch(&s), was + 1);
    assert_eq!(s.song.steps(), steps + 1);
    assert_eq!(s.roll.sounding(), Some(was + 1));
    assert!(s.key_sound_up());
    assert!(s.edit_key(&g, "down", Mods::NONE));
    assert!(s.edit_key(&g, "down", Mods::NONE));
    assert_eq!(pitch(&s), was - 1);
    assert_eq!(s.song.steps(), steps + 3);
    assert_eq!(s.roll.selected.len(), 1, "the note stays selected");
    // Alt+↑ is still a semitone, Alt+Shift+↑ an octave.
    assert!(s.edit_key(&g, "up", Mods::alt()));
    assert_eq!(pitch(&s), was);
    assert!(s.edit_key(&g, "up", Mods::alt_shift()));
    assert_eq!(pitch(&s), was + 12);
    // Each press is undone on its own.
    assert!(s.edit_key(&g, "z", Mods::ctrl()));
    assert_eq!(pitch(&s), was);
    assert!(s.edit_key(&g, "z", Mods::ctrl()));
    assert_eq!(pitch(&s), was - 1);
    // Several selected notes move together, in one step.
    assert!(s.edit_key(&g, "a", Mods::ctrl()));
    let pitches =
        |s: &Studio| -> Vec<_> { context(s, &g).notes().iter().map(|n| n.pitch).collect() };
    let before = pitches(&s);
    assert!(s.edit_key(&g, "up", Mods::NONE));
    assert_eq!(
        pitches(&s),
        before.iter().map(|p| p + 1).collect::<Vec<_>>()
    );
    // The top one sounds.
    assert_eq!(s.roll.sounding(), before.iter().max().map(|p| p + 1));
    assert!(s.key_sound_up());
    assert!(s.edit_key(&g, "z", Mods::ctrl()));
    assert_eq!(pitches(&s), before, "one Ctrl+Z takes back the whole move");
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

#[test]
fn ui_284_the_up_and_down_arrows_step_the_selection_outside_the_roll_as_before() {
    let (mut s, g) = setup();
    s.tool = Tool::Select;
    assert!(s.edit_key(&g, "a", Mods::ctrl()));
    assert!(!s.roll.selected.is_empty());
    s.arrange.focus = true;
    assert_ne!(s.input_context(&g), "roll");
    let steps = s.song.steps();
    let picked = s.picked_clips();
    assert!(s.edit_key(&g, "down", Mods::NONE));
    assert_eq!(s.song.steps(), steps, "no note moved");
    assert_ne!(s.picked_clips(), picked, "the clip selection stepped");
}

#[test]
fn ui_295_shift_up_and_down_move_the_selected_notes_an_octave_and_grow_the_selection_with_none() {
    let (mut s, g) = setup();
    s.tool = Tool::Select;
    assert_eq!(s.input_context(&g), "roll");
    // With nothing selected Shift+↑ grows the selection as before: a note is picked, none moves.
    let steps = s.song.steps();
    assert!(s.roll.selected.is_empty());
    assert!(s.edit_key(&g, "up", Mods::shift()));
    assert!(s.error.is_none(), "{:?}", s.error);
    assert_eq!(s.roll.selected.len(), 1);
    assert_eq!(s.song.steps(), steps);
    let id = *s.roll.selected.iter().next().unwrap();
    let pitch = |s: &Studio| {
        context(s, &g)
            .notes()
            .iter()
            .find(|n| n.id == id)
            .unwrap()
            .pitch
    };
    let was = pitch(&s);
    // With a note selected Shift+↑↓ move it an octave, each press one step.
    assert!(s.edit_key(&g, "up", Mods::shift()));
    assert!(s.error.is_none(), "{:?}", s.error);
    assert_eq!(pitch(&s), was + 12);
    assert_eq!(s.song.steps(), steps + 1);
    assert!(s.edit_key(&g, "down", Mods::shift()));
    assert!(s.edit_key(&g, "down", Mods::shift()));
    assert_eq!(pitch(&s), was - 12);
    assert_eq!(s.song.steps(), steps + 3);
    assert_eq!(s.roll.selected.len(), 1, "the note stays selected");
    // Alt+Shift+↑ is still an octave.
    assert!(s.edit_key(&g, "up", Mods::alt_shift()));
    assert_eq!(pitch(&s), was);
    assert!(s.edit_key(&g, "z", Mods::ctrl()));
    assert_eq!(pitch(&s), was - 12);
    // Mod+Shift+↑ still grows the selection, and no note moves.
    let steps = s.song.steps();
    assert!(s.edit_key(&g, "up", Mods::ctrl_shift()));
    assert_eq!(s.roll.selected.len(), 2);
    assert_eq!(s.song.steps(), steps);
    assert_eq!(pitch(&s), was - 12);
}

#[test]
fn ui_295_shift_up_and_down_grow_the_clip_selection_outside_the_roll_as_before() {
    let (mut s, g) = setup();
    s.tool = Tool::Select;
    assert!(s.edit_key(&g, "a", Mods::ctrl()));
    assert!(!s.roll.selected.is_empty());
    s.arrange.focus = true;
    assert_ne!(s.input_context(&g), "roll");
    let steps = s.song.steps();
    let picked = s.picked_clips();
    assert!(s.edit_key(&g, "down", Mods::shift()));
    assert_eq!(s.song.steps(), steps, "no note moved");
    assert!(
        s.picked_clips().len() > picked.len(),
        "the clip selection grew"
    );
}
