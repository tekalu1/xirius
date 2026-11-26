//! A folder owns its channel as its first child and displays it in the folder row.

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
fn a_folder_comes_with_a_bus_of_its_name_in_its_row_and_connected() {
    let mut s = Session::new("created");
    let (f, bus) = folder(&mut s, "Drums");
    let p = s.project();
    assert_eq!(p.folders[&f].bus, Some(bus));
    assert_eq!(p.tracks[&bus].kind, TrackKind::Bus);
    assert_eq!(p.tracks[&bus].name, "Drums");
    assert_eq!(p.root, [Node::Folder(f)]);
    assert_eq!(p.folders[&f].children, [Node::Track(bus)]);
    assert_eq!(p.effective_output(bus), Some(Output::Master));
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
fn packing_makes_the_bus_inside_the_folder_in_one_step_and_undo_restores_the_rows() {
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
    assert_eq!(s.project().root, [Node::Folder(f), Node::Track(b)]);
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
    assert_eq!(p.row_bus(f), Some(bus), "the folder's row shows it");
    assert_eq!(
        p.root,
        [Node::Folder(f), Node::Track(t)],
        "directly under the folder, not at the bottom"
    );
    // it moves with the folder, and goes with it
    run(
        &mut s,
        "track.move",
        json!({ "ids": [f.to_string()], "by": 1 }),
    );
    assert_eq!(s.project().root, [Node::Track(t), Node::Folder(f)]);
    run(&mut s, "folder.delete", json!({ "ids": [f.to_string()] }));
    assert!(!s.project().tracks.contains_key(&bus));
    // Naming the folder itself as where the bus goes is the same as leaving it out.
    let g = old_folder(&mut s, "Keys");
    let r = run(
        &mut s,
        "folder.bus.add",
        json!({ "id": g.to_string(), "name": "Keys bus", "into": g.to_string() }),
    );
    let keys: TrackId = id(&r, "id");
    assert_eq!(s.project().tracks[&keys].name, "Keys bus");
    assert_eq!(s.project().row_bus(g), Some(keys));
    s.project().validate_deep().unwrap();
    // A folder that is not there gets nothing.
    let before = s.project().content_hash();
    assert!(!try_run(
        &mut s,
        "folder.bus.add",
        json!({ "id": "fld_999" })
    ));
    assert_eq!(s.project().content_hash(), before);
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
fn a_folder_in_a_folder_has_its_own_bus_inside_it_and_goes_with_it() {
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
        [Node::Track(outer_bus), Node::Folder(inner)]
    );
    assert_eq!(s.project().root, [Node::Folder(outer)]);
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
    assert_eq!(s.project().row_bus(inner), Some(inner_bus));
    let r = run(&mut s, "folder.pack", json!({ "ids": [inner.to_string()] }));
    let (outer, outer_bus): (FolderId, TrackId) = (id(&r, "id"), id(&r, "bus"));
    assert_eq!(s.project().root, [Node::Folder(outer)]);
    assert_eq!(
        s.project().folders[&outer].children,
        [Node::Track(outer_bus), Node::Folder(inner)]
    );
    s.project().validate_deep().unwrap();
    run(&mut s, "folder.unpack", json!({ "id": outer.to_string() }));
    assert!(!s.project().tracks.contains_key(&outer_bus));
    assert_eq!(s.project().root, [Node::Folder(inner)]);
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
    assert_eq!(p.root, [Node::Folder(f), Node::Folder(copy),]);
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
fn moving_a_folder_row_moves_its_bus_along_as_its_child() {
    let mut s = Session::new("move");
    let (f, bus) = folder(&mut s, "Drums");
    assert_eq!(s.project().row_bus(f), Some(bus));
    let a = track(&mut s, "A");
    let b = track(&mut s, "B");
    run(
        &mut s,
        "track.move",
        json!({ "ids": [f.to_string()], "by": 1 }),
    );
    assert_eq!(
        s.project().root,
        [Node::Track(a), Node::Folder(f), Node::Track(b)]
    );
    run(
        &mut s,
        "track.move",
        json!({ "ids": [f.to_string()], "by": 5 }),
    );
    assert_eq!(
        s.project().root,
        [Node::Track(a), Node::Track(b), Node::Folder(f),]
    );
    run(
        &mut s,
        "track.move",
        json!({ "ids": [f.to_string()], "by": -2 }),
    );
    assert_eq!(
        s.project().root,
        [Node::Folder(f), Node::Track(a), Node::Track(b)]
    );
    // moved into another folder, the bus comes too
    let (g, g_bus) = folder(&mut s, "Other");
    into(&mut s, f, g);
    assert_eq!(
        s.project().folders[&g].children,
        [Node::Track(g_bus), Node::Folder(f)]
    );
    assert_eq!(s.project().row_bus(g), Some(g_bus));
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

#[test]
fn nested_defaults_follow_the_parent_but_explicit_outputs_survive_moves() {
    let mut s = Session::new("routing");
    let (outer, a) = folder(&mut s, "Outer");
    let (inner, b) = folder(&mut s, "Inner");
    let t = track(&mut s, "Child");
    into(&mut s, t, inner);
    into(&mut s, inner, outer);
    assert_eq!(s.project().effective_output(t), Some(Output::Bus(b)));
    assert_eq!(s.project().effective_output(b), Some(Output::Bus(a)));
    assert_eq!(s.project().effective_output(a), Some(Output::Master));
    let before = s.project().content_hash();
    assert!(!try_run(
        &mut s,
        "track.output.set",
        json!({"ids":[a.to_string()], "to":b.to_string()})
    ));
    assert_eq!(s.project().content_hash(), before);
    run(
        &mut s,
        "track.output.set",
        json!({"ids":[b.to_string()], "to":"master"}),
    );
    run(
        &mut s,
        "track.output.set",
        json!({"ids":[a.to_string()], "to":b.to_string()}),
    );
    let before = s.project().content_hash();
    assert!(!try_run(
        &mut s,
        "track.output.set",
        json!({"ids":[b.to_string()], "to":"default"})
    ));
    assert_eq!(s.project().content_hash(), before);
    run(
        &mut s,
        "track.move",
        json!({"ids":[inner.to_string()], "out":true}),
    );
    assert_eq!(s.project().effective_output(b), Some(Output::Master));
    assert_eq!(s.project().effective_output(a), Some(Output::Bus(b)));
}

#[test]
fn owned_channel_cannot_move_alone_and_insert_at_zero_stays_below_it() {
    let mut s = Session::new("move");
    let (f, b) = folder(&mut s, "Folder");
    let t = track(&mut s, "Child");
    let before = s.project().content_hash();
    assert!(!try_run(
        &mut s,
        "track.move",
        json!({"ids":[b.to_string()], "out":true})
    ));
    assert_eq!(s.project().content_hash(), before);
    run(
        &mut s,
        "track.move",
        json!({"ids":[t.to_string()], "into":f.to_string(), "index":0}),
    );
    assert_eq!(
        s.project().folders[&f].children,
        [Node::Track(b), Node::Track(t)]
    );
    run(
        &mut s,
        "track.move",
        json!({"ids":[t.to_string()], "by":-1}),
    );
    assert_eq!(
        s.project().folders[&f].children,
        [Node::Track(b), Node::Track(t)]
    );
}

#[test]
fn a_cyclic_folder_move_is_atomic_including_sends() {
    let mut s = Session::new("cycle");
    let (outer, a) = folder(&mut s, "Outer");
    let (inner, b) = folder(&mut s, "Inner");
    run(
        &mut s,
        "send.add",
        json!({"ids":[a.to_string()], "to":b.to_string()}),
    );
    let before = s.project().content_hash();
    assert!(!try_run(
        &mut s,
        "track.move",
        json!({"ids":[inner.to_string()], "into":outer.to_string()})
    ));
    assert_eq!(s.project().content_hash(), before);
    s.project().validate_deep().unwrap();
}

#[test]
fn duplicated_nested_channels_retarget_internal_outputs_and_sends() {
    let mut s = Session::new("duplicate routes");
    let (outer, a) = folder(&mut s, "Outer");
    let (inner, b) = folder(&mut s, "Inner");
    into(&mut s, inner, outer);
    let t = track(&mut s, "Child");
    into(&mut s, t, inner);
    run(
        &mut s,
        "track.output.set",
        json!({"ids":[t.to_string()], "to":b.to_string()}),
    );
    run(
        &mut s,
        "send.add",
        json!({"ids":[t.to_string()], "to":a.to_string()}),
    );
    let r = run(&mut s, "folder.duplicate", json!({"id":outer.to_string()}));
    let copy: FolderId = id(&r, "id");
    let ca = s.project().row_bus(copy).unwrap();
    let nested = s.project().folders[&copy]
        .children
        .iter()
        .find_map(|n| match n {
            Node::Folder(f) => Some(*f),
            _ => None,
        })
        .unwrap();
    let cb = s.project().row_bus(nested).unwrap();
    let ct = s
        .project()
        .tracks_in(nested)
        .into_iter()
        .find(|t| s.project().tracks[t].kind == TrackKind::Audio)
        .unwrap();
    assert_ne!(ca, a);
    assert_ne!(cb, b);
    assert_eq!(s.project().tracks[&ct].output, Some(Output::Bus(cb)));
    assert_eq!(s.project().tracks[&ct].sends[0].to, ca);
    assert_eq!(s.project().effective_output(cb), Some(Output::Bus(ca)));
    s.project().validate_deep().unwrap();
}

fn legacy_sibling(p: &mut Project, f: FolderId, b: TrackId) {
    p.detach(Node::Track(b));
    let (parent, i) = p.find_node(Node::Folder(f)).unwrap();
    p.children_mut(parent).insert(i + 1, Node::Track(b));
}

#[test]
fn legacy_migration_keeps_ids_and_routes_without_writing_the_opened_file() {
    let mut s = Session::new("migration");
    let (f, b) = folder(&mut s, "Drums");
    let (outer, outer_bus) = folder(&mut s, "Outer");
    into(&mut s, f, outer);
    let t = track(&mut s, "Child");
    into(&mut s, t, f);
    let outside = track(&mut s, "Key source");
    let aux = bus_named(&mut s, "Aux");
    run(
        &mut s,
        "send.add",
        json!({"ids":[b.to_string()], "to":aux.to_string(), "level":-9.0}),
    );
    let plugin = run(
        &mut s,
        "plugin.add",
        json!({"track":b.to_string(), "name":"Compressor"}),
    );
    run(
        &mut s,
        "plugin.sidechain.add",
        json!({"id":plugin["id"], "from":outside.to_string(), "pre":true, "level":-6.0}),
    );
    let mut old = s.project().clone();
    let mut lane = xirius_core::auto::AutoLane::new(xirius_core::auto::AutoTarget::Volume);
    lane.points = std::sync::Arc::new(vec![
        xirius_core::auto::AutoPoint::new(0, -3.0),
        xirius_core::auto::AutoPoint::new(3840, -9.0),
    ]);
    old.track_mut(b).unwrap().automation.push(lane);
    legacy_sibling(&mut old, f, b);
    legacy_sibling(&mut old, outer, outer_bus);
    // The old bus need not be adjacent, or even under the same parent.
    old.detach(Node::Track(b));
    old.root.push(Node::Track(b));
    let outputs: Vec<_> = old
        .tracks
        .keys()
        .map(|t| (*t, old.effective_output(*t)))
        .collect();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("project.json");
    let bytes = serde_json::to_vec(&json!({"format_version":1,"project":old})).unwrap();
    // Compare migration with the saved values, after the same JSON decoding as Store.
    #[derive(serde::Deserialize)]
    struct Saved {
        project: Project,
    }
    let saved: Saved = serde_json::from_slice(&bytes).unwrap();
    std::fs::write(&file, &bytes).unwrap();
    let store = xirius_core::store::Store::new(dir.path());
    let migrated = store.read_project().unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    assert_eq!(migrated.row_bus(f), Some(b));
    assert_eq!(migrated.row_bus(outer), Some(outer_bus));
    for (t, output) in outputs {
        assert_eq!(migrated.effective_output(t), output);
    }
    assert_eq!(migrated.tracks[&b].output, Some(Output::Master));
    assert_eq!(migrated.tracks[&b].name, old.tracks[&b].name);
    for (id, before) in &saved.project.tracks {
        let mut after = migrated.tracks[id].as_ref().clone();
        after.output = before.output;
        assert_eq!(
            &after,
            before.as_ref(),
            "all channel settings survive for {id}"
        );
    }
    store.write_project(&migrated).unwrap();
    assert_eq!(store.read_project().unwrap(), migrated);
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(file).unwrap()).unwrap()["format_version"],
        3
    );
}

#[test]
fn copying_a_folders_channels_keeps_the_nested_signal_chain_and_one_undo() {
    let mut s = Session::new("copy folder channels");
    let (outer, a) = folder(&mut s, "Outer");
    let (inner, b) = folder(&mut s, "Inner");
    into(&mut s, inner, outer);
    let child = track(&mut s, "Child");
    into(&mut s, child, inner);
    let source = s.project().tracks_in(outer);
    run(
        &mut s,
        "edit.copy",
        json!({"ids":source.iter().map(ToString::to_string).collect::<Vec<_>>()}),
    );
    let before = s.project().content_hash();
    let pasted = run(&mut s, "edit.paste", json!({}));
    let made: Vec<TrackId> = pasted["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().parse().unwrap())
        .collect();
    let copy = |id| made[source.iter().position(|t| *t == id).unwrap()];
    assert_eq!(
        s.project().effective_output(copy(child)),
        Some(Output::Bus(copy(b)))
    );
    assert_eq!(
        s.project().effective_output(copy(b)),
        Some(Output::Bus(copy(a)))
    );
    assert_eq!(s.project().effective_output(copy(a)), Some(Output::Master));
    s.project().validate_deep().unwrap();
    undo(&mut s);
    assert_eq!(s.project().content_hash(), before);
    redo(&mut s);
    s.project().validate_deep().unwrap();
}

#[test]
fn legacy_organization_shared_and_renamed_buses_keep_the_saved_shape() {
    for case in ["organization", "shared", "renamed"] {
        let mut s = Session::new(case);
        let (f, b) = folder(&mut s, "Folder");
        let other = old_folder(&mut s, "Other");
        let mut p = s.project().clone();
        legacy_sibling(&mut p, f, b);
        match case {
            "organization" => {
                p.folders.get_mut(&f).unwrap().bus = None;
            }
            "shared" => {
                p.folders.get_mut(&other).unwrap().bus = Some(b);
            }
            _ => {
                std::sync::Arc::make_mut(p.tracks.get_mut(&b).unwrap()).name = "Manual name".into();
            }
        }
        let before = p.clone();
        p.migrate_folder_rows();
        assert_eq!(p, before, "{case}");
    }
}

#[test]
fn deleting_nested_folders_keeps_externally_used_channels_and_their_chain() {
    let mut s = Session::new("retained channels");
    let (outer, a) = folder(&mut s, "Outer");
    let (inner, b) = folder(&mut s, "Inner");
    into(&mut s, inner, outer);
    let child = track(&mut s, "Child");
    into(&mut s, child, inner);
    let outside = track(&mut s, "Outside");
    run(
        &mut s,
        "send.add",
        json!({"ids":[outside.to_string()], "to":b.to_string()}),
    );
    let before = s.project().content_hash();
    run(
        &mut s,
        "folder.delete",
        json!({"id":outer.to_string(), "with-tracks":true}),
    );
    assert!(!s.project().folders.contains_key(&outer));
    assert!(!s.project().tracks.contains_key(&child));
    assert!(s.project().root.contains(&Node::Track(a)));
    assert!(s.project().root.contains(&Node::Track(b)));
    assert_eq!(s.project().effective_output(b), Some(Output::Bus(a)));
    assert_eq!(s.project().effective_output(a), Some(Output::Master));
    s.project().validate_deep().unwrap();
    undo(&mut s);
    assert_eq!(s.project().content_hash(), before);
    redo(&mut s);
    s.project().validate_deep().unwrap();
}
