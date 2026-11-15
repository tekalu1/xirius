//! A folder comes with a bus of its name: made right under it, connected, and gone with it
//! unless something else plays to it. Songs made before keep their folders as they were.

use serde_json::{Value, json};
use xirius_core::id::{FolderId, Node, TrackId};
use xirius_core::model::{Output, Project, TrackKind};
use xirius_core::session::{RunOptions, Session};

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args.clone(), &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name} {args}: {e:?}"))
        .data
}

fn try_run(s: &mut Session, name: &str, args: Value) -> bool {
    s.run_value(name, args, &RunOptions::human()).is_ok()
}

fn id<T: std::str::FromStr>(v: &Value, key: &str) -> T
where
    T::Err: std::fmt::Debug,
{
    v[key]
        .as_str()
        .unwrap_or_else(|| panic!("{v}"))
        .parse()
        .unwrap()
}

fn track(s: &mut Session, name: &str) -> TrackId {
    id(&run(s, "track.add.audio", json!({ "name": name })), "id")
}

fn bus_named(s: &mut Session, name: &str) -> TrackId {
    id(&run(s, "track.add.bus", json!({ "name": name })), "id")
}

fn folder(s: &mut Session, name: &str) -> (FolderId, TrackId) {
    let r = run(s, "folder.add", json!({ "name": name }));
    (id(&r, "id"), id(&r, "bus"))
}

fn undo(s: &mut Session) {
    run(s, "history.undo", json!({}));
}

fn redo(s: &mut Session) {
    run(s, "history.redo", json!({}));
}

fn buses(p: &Project) -> Vec<TrackId> {
    p.tracks
        .values()
        .filter(|t| t.kind == TrackKind::Bus)
        .map(|t| t.id)
        .collect()
}

/// A folder as a song made before a folder came with a bus has it: no bus.
fn old_folder(s: &mut Session, name: &str) -> FolderId {
    let (f, bus) = folder(s, name);
    run(s, "track.delete", json!({ "ids": [bus.to_string()] }));
    f
}

fn into(s: &mut Session, t: impl ToString, f: FolderId) {
    run(
        s,
        "track.move",
        json!({ "ids": [t.to_string()], "into": f.to_string() }),
    );
}

fn reopened(s: &Session) -> Session {
    let bytes = serde_json::to_vec(s.project()).unwrap();
    Session::from_project(serde_json::from_slice(&bytes).unwrap())
}

#[test]
fn a_folder_comes_with_a_bus_of_its_name_right_under_it_and_connected() {
    let mut s = Session::new("created");
    let (f, bus) = folder(&mut s, "Drums");
    let p = s.project();
    assert_eq!(p.folders[&f].bus, Some(bus));
    assert_eq!(p.tracks[&bus].kind, TrackKind::Bus);
    assert_eq!(p.tracks[&bus].name, "Drums");
    assert_eq!(p.root, [Node::Folder(f), Node::Track(bus)]);
    p.validate_deep().unwrap();
}

#[test]
fn creating_a_folder_is_one_step_and_undo_redo_take_both_away_and_bring_both_back() {
    let mut s = Session::new("undo");
    let before = s.project().content_hash();
    let n = s.history().len();
    let (f, bus) = folder(&mut s, "Drums");
    assert_eq!(s.history().len(), n + 1);
    let made = s.project().content_hash();
    undo(&mut s);
    assert_eq!(s.project().content_hash(), before);
    assert!(s.project().folders.is_empty() && buses(s.project()).is_empty());
    redo(&mut s);
    assert_eq!(s.project().content_hash(), made);
    assert_eq!(s.project().folders[&f].bus, Some(bus));
}

#[test]
fn packing_makes_the_bus_under_the_folder_in_one_step_and_undo_restores_the_rows() {
    let mut s = Session::new("pack");
    let a = track(&mut s, "A");
    let b = track(&mut s, "B");
    let c = track(&mut s, "C");
    let before = s.project().content_hash();
    let n = s.history().len();
    let r = run(
        &mut s,
        "folder.pack",
        json!({ "ids": [a.to_string(), c.to_string()] }),
    );
    assert_eq!(s.history().len(), n + 1);
    let (f, bus): (FolderId, TrackId) = (id(&r, "id"), id(&r, "bus"));
    assert_eq!(
        s.project().root,
        [Node::Folder(f), Node::Track(bus), Node::Track(b)]
    );
    assert_eq!(s.project().folders[&f].bus, Some(bus));
    s.project().validate_deep().unwrap();
    undo(&mut s);
    assert_eq!(s.project().content_hash(), before);
    assert_eq!(
        s.project().root,
        [Node::Track(a), Node::Track(b), Node::Track(c)]
    );
}

#[test]
fn packing_and_unpacking_three_times_leaves_no_bus_behind() {
    let mut s = Session::new("again");
    let a = track(&mut s, "A");
    let _b = track(&mut s, "B");
    let c = track(&mut s, "C");
    let root = s.project().root.clone();
    let tracks = s.project().tracks.len();
    for _ in 0..3 {
        let r = run(
            &mut s,
            "folder.pack",
            json!({ "ids": [a.to_string(), c.to_string()] }),
        );
        assert_eq!(s.project().tracks.len(), tracks + 1);
        run(&mut s, "folder.unpack", json!({ "id": r["id"] }));
        assert_eq!(s.project().root, root);
        assert_eq!(s.project().tracks.len(), tracks);
        assert!(buses(s.project()).is_empty());
        assert!(s.project().folders.is_empty() && s.project().packed.is_empty());
    }
    s.project().validate_deep().unwrap();
}

#[test]
fn deleting_a_folder_takes_the_bus_made_with_it_and_one_undo_brings_everything_back() {
    let mut s = Session::new("delete");
    let (f, bus) = folder(&mut s, "Drums");
    let t = track(&mut s, "Kick");
    into(&mut s, t, f);
    let before = s.project().content_hash();
    let n = s.history().len();
    run(&mut s, "folder.delete", json!({ "ids": [f.to_string()] }));
    assert_eq!(s.history().len(), n + 1);
    assert!(!s.project().tracks.contains_key(&bus));
    assert!(s.project().folders.is_empty());
    assert!(
        s.project().tracks.contains_key(&t),
        "the track stays: only the folder went"
    );
    s.project().validate_deep().unwrap();
    undo(&mut s);
    assert_eq!(s.project().content_hash(), before);
    assert!(s.project().tracks.contains_key(&bus));
}

#[test]
fn a_bus_something_else_plays_to_survives_the_folder_and_the_link_goes() {
    // each way a bus is used outside the folder
    for how in ["output", "send", "other folder"] {
        let mut s = Session::new("kept");
        let (f, bus) = folder(&mut s, "Drums");
        let other = track(&mut s, "Other");
        let other_folder = old_folder(&mut s, "Other folder");
        match how {
            "output" => {
                run(
                    &mut s,
                    "track.output.set",
                    json!({ "ids": [other.to_string()], "to": bus.to_string() }),
                );
            }
            "send" => {
                run(
                    &mut s,
                    "send.add",
                    json!({ "ids": [other.to_string()], "to": bus.to_string() }),
                );
            }
            _ => {
                run(
                    &mut s,
                    "folder.bus.set",
                    json!({ "id": other_folder.to_string(), "bus": bus.to_string() }),
                );
            }
        }
        let before = s.project().content_hash();
        run(&mut s, "folder.delete", json!({ "ids": [f.to_string()] }));
        let p = s.project();
        assert!(p.tracks.contains_key(&bus), "{how}: the bus stays");
        assert!(!p.folders.contains_key(&f), "{how}");
        match how {
            "output" => assert_eq!(p.tracks[&other].output, Some(Output::Bus(bus))),
            "send" => assert!(p.tracks[&other].sends.iter().any(|x| x.to == bus)),
            _ => assert_eq!(p.folders[&other_folder].bus, Some(bus)),
        }
        p.validate_deep().unwrap();
        undo(&mut s);
        assert_eq!(s.project().content_hash(), before, "{how}");
    }
}

#[test]
fn unpacking_a_folder_takes_its_bus_unless_something_else_plays_to_it() {
    let mut s = Session::new("unpack");
    let (f, bus) = folder(&mut s, "Drums");
    run(&mut s, "folder.unpack", json!({ "id": f.to_string() }));
    assert!(!s.project().tracks.contains_key(&bus));

    let (f, bus) = folder(&mut s, "Drums");
    let other = track(&mut s, "Other");
    run(
        &mut s,
        "track.output.set",
        json!({ "ids": [other.to_string()], "to": bus.to_string() }),
    );
    run(&mut s, "folder.unpack", json!({ "id": f.to_string() }));
    assert!(s.project().tracks.contains_key(&bus));
    assert_eq!(s.project().tracks[&other].output, Some(Output::Bus(bus)));
}

#[test]
fn only_the_bus_made_with_the_folder_goes_not_one_it_was_linked_to_later() {
    let mut s = Session::new("relinked");
    let (f, made) = folder(&mut s, "Drums");
    let chosen = bus_named(&mut s, "Chosen");
    run(
        &mut s,
        "folder.bus.set",
        json!({ "id": f.to_string(), "bus": chosen.to_string() }),
    );
    run(&mut s, "folder.delete", json!({ "ids": [f.to_string()] }));
    assert!(s.project().tracks.contains_key(&chosen));
    assert!(
        s.project().tracks.contains_key(&made),
        "the folder no longer played to the bus made with it: it is an ordinary bus now"
    );
}

#[test]
fn not_connecting_only_takes_the_link_off_and_keeps_the_bus() {
    let mut s = Session::new("unlink");
    let (f, bus) = folder(&mut s, "Drums");
    run(
        &mut s,
        "folder.bus.set",
        json!({ "id": f.to_string(), "bus": "none" }),
    );
    assert_eq!(s.project().folders[&f].bus, None);
    assert!(s.project().tracks.contains_key(&bus));
    run(&mut s, "folder.delete", json!({ "ids": [f.to_string()] }));
    assert!(s.project().tracks.contains_key(&bus));
}

#[test]
fn renaming_the_folder_renames_the_bus_while_the_names_are_the_same() {
    let mut s = Session::new("rename");
    let (f, bus) = folder(&mut s, "Drums");
    run(
        &mut s,
        "folder.rename",
        json!({ "id": f.to_string(), "name": "Kit" }),
    );
    assert_eq!(s.project().tracks[&bus].name, "Kit");
    run(
        &mut s,
        "folder.rename",
        json!({ "id": f.to_string(), "name": "Kit 2" }),
    );
    assert_eq!(s.project().tracks[&bus].name, "Kit 2");
    undo(&mut s);
    assert_eq!(s.project().folders[&f].name, "Kit");
    assert_eq!(s.project().tracks[&bus].name, "Kit");
}

#[test]
fn once_the_bus_is_renamed_by_hand_the_folders_name_no_longer_reaches_it() {
    let mut s = Session::new("no-sync");
    let (f, bus) = folder(&mut s, "Drums");
    run(
        &mut s,
        "track.rename",
        json!({ "id": bus.to_string(), "name": "Drum bus" }),
    );
    run(
        &mut s,
        "folder.rename",
        json!({ "id": f.to_string(), "name": "Kit" }),
    );
    assert_eq!(s.project().tracks[&bus].name, "Drum bus");
    assert_eq!(s.project().folders[&f].name, "Kit");
}

#[test]
fn a_folder_of_an_old_song_gets_no_bus_and_its_bus_menu_makes_one_named_as_it_under_it() {
    let mut s = Session::new("old");
    let f = old_folder(&mut s, "Drums");
    let t = track(&mut s, "Kick");
    assert_eq!(s.project().folders[&f].bus, None);
    let r = run(&mut s, "folder.bus.add", json!({ "id": f.to_string() }));
    let bus: TrackId = id(&r, "id");
    let p = s.project();
    assert_eq!(p.tracks[&bus].name, "Drums");
    assert_eq!(p.folders[&f].bus, Some(bus));
    assert_eq!(
        p.root,
        [Node::Folder(f), Node::Track(bus), Node::Track(t)],
        "directly under the folder, not at the bottom"
    );
    // it moves with the folder, and goes with it
    run(
        &mut s,
        "track.move",
        json!({ "ids": [f.to_string()], "by": 1 }),
    );
    assert_eq!(
        s.project().root,
        [Node::Track(t), Node::Folder(f), Node::Track(bus)]
    );
    run(&mut s, "folder.delete", json!({ "ids": [f.to_string()] }));
    assert!(!s.project().tracks.contains_key(&bus));
}

#[test]
fn songs_made_before_load_save_and_reload_unchanged_and_behave_as_they_did() {
    // three folders as an old song has them: no bus, linked to a bus, and not linked
    let mut s = Session::new("legacy");
    let bare = old_folder(&mut s, "Bare");
    let linked = old_folder(&mut s, "Linked");
    let hand = bus_named(&mut s, "Hand");
    run(
        &mut s,
        "folder.bus.set",
        json!({ "id": linked.to_string(), "bus": hand.to_string() }),
    );
    let loose = old_folder(&mut s, "Loose");
    run(
        &mut s,
        "folder.bus.set",
        json!({ "id": loose.to_string(), "bus": "none" }),
    );

    // there is no mark in what is written, and an old file reads as "no mark"
    let json = serde_json::to_string(s.project()).unwrap();
    assert!(!json.contains("made_bus"), "{json}");
    let back: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(&back, s.project());
    assert_eq!(back.content_hash(), s.project().content_hash());
    let mut again = Session::from_project(back);
    assert_eq!(serde_json::to_string(again.project()).unwrap(), json);

    // and they behave as before: deleting takes no bus, the link only goes
    for f in [bare, linked, loose] {
        run(
            &mut again,
            "folder.delete",
            json!({ "ids": [f.to_string()] }),
        );
    }
    assert!(again.project().tracks.contains_key(&hand));
    assert!(again.project().folders.is_empty());
    // unpacking a linked old folder keeps its bus too
    let mut s2 = reopened(&s);
    run(
        &mut s2,
        "folder.unpack",
        json!({ "id": linked.to_string() }),
    );
    assert!(s2.project().tracks.contains_key(&hand));
}

#[test]
fn the_mark_survives_a_save_and_the_reopened_folder_still_takes_its_bus_with_it() {
    let mut s = Session::new("mark");
    let (f, bus) = folder(&mut s, "Drums");
    let mut back = reopened(&s);
    assert_eq!(back.project(), s.project());
    run(
        &mut back,
        "folder.delete",
        json!({ "ids": [f.to_string()] }),
    );
    assert!(!back.project().tracks.contains_key(&bus));
}

#[test]
fn a_folder_in_a_folder_has_its_own_bus_under_it_and_goes_with_it() {
    let mut s = Session::new("nested");
    let (outer, outer_bus) = folder(&mut s, "Drums");
    let r = run(
        &mut s,
        "folder.add",
        json!({ "name": "Snares", "into": outer.to_string() }),
    );
    let (inner, inner_bus): (FolderId, TrackId) = (id(&r, "id"), id(&r, "bus"));
    assert_eq!(
        s.project().folders[&outer].children,
        [Node::Folder(inner), Node::Track(inner_bus)]
    );
    assert_eq!(
        s.project().root,
        [Node::Folder(outer), Node::Track(outer_bus)]
    );
    s.project().validate_deep().unwrap();
    run(
        &mut s,
        "folder.delete",
        json!({ "ids": [inner.to_string()] }),
    );
    assert!(!s.project().tracks.contains_key(&inner_bus));
    assert!(s.project().tracks.contains_key(&outer_bus));
}

#[test]
fn packing_a_folder_into_another_takes_its_bus_in_and_makes_the_outer_one() {
    let mut s = Session::new("pack-folder");
    let (inner, inner_bus) = folder(&mut s, "Snares");
    let r = run(&mut s, "folder.pack", json!({ "ids": [inner.to_string()] }));
    let (outer, outer_bus): (FolderId, TrackId) = (id(&r, "id"), id(&r, "bus"));
    assert_eq!(
        s.project().root,
        [Node::Folder(outer), Node::Track(outer_bus)]
    );
    assert_eq!(
        s.project().folders[&outer].children,
        [Node::Folder(inner), Node::Track(inner_bus)]
    );
    s.project().validate_deep().unwrap();
    run(&mut s, "folder.unpack", json!({ "id": outer.to_string() }));
    assert!(!s.project().tracks.contains_key(&outer_bus));
    assert_eq!(
        s.project().root,
        [Node::Folder(inner), Node::Track(inner_bus)]
    );
}

#[test]
fn duplicating_a_folder_duplicates_the_bus_and_links_the_copies_to_each_other() {
    let mut s = Session::new("duplicate");
    let (f, bus) = folder(&mut s, "Drums");
    let r = run(&mut s, "folder.duplicate", json!({ "id": f.to_string() }));
    let (copy, copy_bus): (FolderId, TrackId) = (id(&r, "id"), id(&r, "bus"));
    assert_ne!(copy_bus, bus);
    let p = s.project();
    assert_eq!(p.folders[&f].bus, Some(bus));
    assert_eq!(p.folders[&copy].bus, Some(copy_bus));
    assert_eq!(p.tracks[&copy_bus].name, p.folders[&copy].name);
    assert_eq!(
        p.root,
        [
            Node::Folder(f),
            Node::Track(bus),
            Node::Folder(copy),
            Node::Track(copy_bus)
        ]
    );
    p.validate_deep().unwrap();
    // the copy's bus goes with the copy, the original's stays
    run(
        &mut s,
        "folder.delete",
        json!({ "ids": [copy.to_string()] }),
    );
    assert!(!s.project().tracks.contains_key(&copy_bus));
    assert!(s.project().tracks.contains_key(&bus));
}

#[test]
fn moving_a_folder_row_moves_its_bus_along_right_under_it() {
    let mut s = Session::new("move");
    let (f, bus) = folder(&mut s, "Drums");
    let a = track(&mut s, "A");
    let b = track(&mut s, "B");
    run(
        &mut s,
        "track.move",
        json!({ "ids": [f.to_string()], "by": 1 }),
    );
    assert_eq!(
        s.project().root,
        [
            Node::Track(a),
            Node::Folder(f),
            Node::Track(bus),
            Node::Track(b)
        ]
    );
    run(
        &mut s,
        "track.move",
        json!({ "ids": [f.to_string()], "by": 5 }),
    );
    assert_eq!(
        s.project().root,
        [
            Node::Track(a),
            Node::Track(b),
            Node::Folder(f),
            Node::Track(bus)
        ]
    );
    run(
        &mut s,
        "track.move",
        json!({ "ids": [f.to_string()], "by": -2 }),
    );
    assert_eq!(
        s.project().root,
        [
            Node::Folder(f),
            Node::Track(bus),
            Node::Track(a),
            Node::Track(b)
        ]
    );
    // moved into another folder, the bus comes too
    let (g, g_bus) = folder(&mut s, "Other");
    into(&mut s, f, g);
    assert_eq!(
        s.project().folders[&g].children,
        [Node::Folder(f), Node::Track(bus)]
    );
    assert!(s.project().root.contains(&Node::Track(g_bus)));
    s.project().validate_deep().unwrap();
}

#[test]
fn a_track_with_its_own_output_keeps_it_when_it_goes_into_a_folder() {
    let mut s = Session::new("own-output");
    let (f, _bus) = folder(&mut s, "Drums");
    let other = bus_named(&mut s, "Verb");
    let t = track(&mut s, "Kick");
    run(
        &mut s,
        "track.output.set",
        json!({ "ids": [t.to_string()], "to": other.to_string() }),
    );
    into(&mut s, t, f);
    assert_eq!(s.project().tracks[&t].output, Some(Output::Bus(other)));
}

#[test]
fn a_loop_through_a_folders_bus_is_still_refused() {
    let mut s = Session::new("loop");
    let (f, bus) = folder(&mut s, "Drums");
    let t = track(&mut s, "Kick");
    into(&mut s, t, f);
    let up = bus_named(&mut s, "Up");
    // the folder's bus plays to Up; Up may not play back to it
    assert!(try_run(
        &mut s,
        "track.output.set",
        json!({ "ids": [bus.to_string()], "to": up.to_string() })
    ));
    assert!(!try_run(
        &mut s,
        "track.output.set",
        json!({ "ids": [up.to_string()], "to": bus.to_string() })
    ));
    // a folder may not be connected to a bus that sits in it: that bus plays to the folder's bus
    let inside = bus_named(&mut s, "Inside");
    into(&mut s, inside, f);
    assert!(!try_run(
        &mut s,
        "folder.bus.set",
        json!({ "id": f.to_string(), "bus": inside.to_string() })
    ));
    s.project().validate_deep().unwrap();
}
