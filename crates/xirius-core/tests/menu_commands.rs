//! The small commands the right-click menus run (CTX): a clip's colour, taking the fades and
//! the gain back, and moving one end of the loop.

use serde_json::{Value, json};
use xirius_core::id::{ClipId, FolderId, Node, TrackId};
use xirius_core::model::{ClipBody, Output as Route};
use xirius_core::session::{RunOptions, RunResult, Session};

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    s.run_value(name, args.clone(), &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name} {args}: {e:?}"))
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

fn audio(s: &mut Session) -> String {
    let t = id_of(&run(s, "track.add.audio", json!({})));
    id_of(&run(
        s,
        "clip.add.audio",
        json!({"track": t, "at": 0, "length": 960, "file": "a.wav"}),
    ))
}

#[test]
fn a_clip_takes_a_colour_and_gives_it_back_in_one_step_each() {
    let mut s = Session::new("clip-color");
    let a = audio(&mut s);
    let n = s.history().len();
    run(
        &mut s,
        "clip.color.set",
        json!({"ids": [a], "color": "#ff8800"}),
    );
    assert_eq!(s.history().len(), n + 1);
    let id: ClipId = a.parse().unwrap();
    assert_eq!(
        s.project().clip(id).unwrap().1.color.as_deref(),
        Some("#ff8800")
    );
    run(
        &mut s,
        "clip.color.set",
        json!({"ids": [a], "color": "none"}),
    );
    assert_eq!(s.project().clip(id).unwrap().1.color, None);
    assert!(
        s.run_value(
            "clip.color.set",
            json!({"ids": [a], "color": "red"}),
            &RunOptions::human()
        )
        .is_err()
    );
}

#[test]
fn the_fades_and_the_gain_come_back() {
    let mut s = Session::new("clip-back");
    let a = audio(&mut s);
    run(&mut s, "clip.fade.set", json!({"ids": [a], "in": 240}));
    run(&mut s, "clip.gain.set", json!({"ids": [a], "db": -6}));
    assert!(s.project().clip_fx.contains_key(&a.parse().unwrap()));
    run(&mut s, "clip.fade.clear", json!({"ids": [a]}));
    assert!(s.project().clip_fx.is_empty());
    run(&mut s, "clip.gain.reset", json!({"ids": [a]}));
    let id: ClipId = a.parse().unwrap();
    let ClipBody::Audio { gain_db, .. } = &s.project().clip(id).unwrap().1.body else {
        panic!("not audio");
    };
    assert_eq!(*gain_db, 0.0);
    // both undo in one step each
    s.run_value("history.undo", json!({}), &RunOptions::human())
        .unwrap();
    let ClipBody::Audio { gain_db, .. } = &s.project().clip(id).unwrap().1.body else {
        panic!("not audio");
    };
    assert_eq!(*gain_db, -6.0);
}

#[test]
fn one_end_of_the_loop_moves_and_a_missing_loop_is_made() {
    let mut s = Session::new("loop-ends");
    run(&mut s, "loop.start.set", json!({"at": 3840}));
    let l = s.project().loop_range.unwrap();
    assert_eq!((l.start, l.end), (3840, 7680));
    run(&mut s, "loop.end.set", json!({"at": 15360}));
    let l = s.project().loop_range.unwrap();
    assert_eq!((l.start, l.end), (3840, 15360));
    // an end before the start is refused and nothing changes
    assert!(
        s.run_value("loop.end.set", json!({"at": 100}), &RunOptions::human())
            .is_err()
    );
    let l = s.project().loop_range.unwrap();
    assert_eq!((l.start, l.end), (3840, 15360));
}

fn track_named(s: &mut Session, kind: &str, name: &str) -> String {
    id_of(&run(s, kind, json!({ "name": name })))
}

fn column(s: &Session) -> Vec<Node> {
    s.project().root.clone()
}

#[test]
fn several_nodes_are_packed_into_one_new_folder_where_the_first_one_was() {
    let mut s = Session::new("pack");
    let a = track_named(&mut s, "track.add.audio", "A");
    let b = track_named(&mut s, "track.add.audio", "B");
    let c = track_named(&mut s, "track.add.audio", "C");
    let n = s.history().len();
    let r = run(&mut s, "folder.pack", json!({"ids": [c, a]}));
    assert_eq!(s.history().len(), n + 1);
    let f: FolderId = id_of(&r).parse().unwrap();
    let (a, b, c): (TrackId, TrackId, TrackId) =
        (a.parse().unwrap(), b.parse().unwrap(), c.parse().unwrap());
    // in column order, whatever the order of the ids
    assert_eq!(column(&s), [Node::Folder(f), Node::Track(b)]);
    assert_eq!(
        s.project().folders[&f].children,
        [Node::Track(a), Node::Track(c)]
    );
    s.project().validate_deep().unwrap();
    s.run_value("history.undo", json!({}), &RunOptions::human())
        .unwrap();
    assert_eq!(column(&s), [Node::Track(a), Node::Track(b), Node::Track(c)]);
}

#[test]
fn what_is_inside_a_picked_folder_goes_along_and_is_not_moved_twice() {
    let mut s = Session::new("pack-inside");
    let f = id_of(&run(&mut s, "folder.add", json!({"name": "F"})));
    let x = id_of(&run(
        &mut s,
        "track.add.audio",
        json!({"name": "X", "into": f}),
    ));
    let r = run(&mut s, "folder.pack", json!({"ids": [x, f]}));
    let outer: FolderId = id_of(&r).parse().unwrap();
    let inner: FolderId = f.parse().unwrap();
    assert_eq!(column(&s), [Node::Folder(outer)]);
    assert_eq!(s.project().folders[&outer].children, [Node::Folder(inner)]);
    assert_eq!(
        s.project().folders[&inner].children,
        [Node::Track(x.parse().unwrap())]
    );
    s.project().validate_deep().unwrap();
}

#[test]
fn unpacking_hands_the_content_to_the_level_above() {
    let mut s = Session::new("unpack");
    let f = id_of(&run(&mut s, "folder.add", json!({"name": "F"})));
    let x = track_named(&mut s, "track.add.audio", "X");
    run(&mut s, "track.move", json!({"ids": [x], "into": f}));
    let n = s.history().len();
    run(&mut s, "folder.unpack", json!({"id": f}));
    assert_eq!(s.history().len(), n + 1);
    assert_eq!(column(&s), [Node::Track(x.parse().unwrap())]);
    assert!(s.project().folders.is_empty());
}

#[test]
fn a_folder_is_copied_with_its_tracks_clips_and_folders_under_new_ids() {
    let mut s = Session::new("folder-copy");
    let f = id_of(&run(&mut s, "folder.add", json!({"name": "Drums"})));
    let g = id_of(&run(
        &mut s,
        "folder.add",
        json!({"name": "Inner", "into": f}),
    ));
    let k = id_of(&run(
        &mut s,
        "track.add.audio",
        json!({"name": "Kick", "into": f}),
    ));
    let h = id_of(&run(
        &mut s,
        "track.add.audio",
        json!({"name": "Hat", "into": g}),
    ));
    run(
        &mut s,
        "clip.add.audio",
        json!({"track": k, "at": 0, "length": 960, "file": "k.wav"}),
    );
    let n = s.history().len();
    let r = run(&mut s, "folder.duplicate", json!({"id": f}));
    assert_eq!(s.history().len(), n + 1);
    let copy: FolderId = id_of(&r).parse().unwrap();
    let p = s.project();
    assert_eq!(p.root.len(), 2);
    assert_eq!(p.root[1], Node::Folder(copy));
    assert_eq!(p.folders[&copy].name, "Drums コピー");
    let kids = &p.folders[&copy].children;
    assert_eq!(kids.len(), 2);
    let Node::Folder(inner) = kids[0] else {
        panic!("{kids:?}")
    };
    let Node::Track(kick) = kids[1] else {
        panic!("{kids:?}")
    };
    assert_ne!(inner.to_string(), g);
    assert_ne!(kick.to_string(), k);
    assert_eq!(p.tracks[&kick].name, "Kick");
    assert_eq!(p.tracks[&kick].clips.len(), 1);
    assert_ne!(
        p.tracks[&kick].clips[0].id,
        p.tracks[&k.parse::<TrackId>().unwrap()].clips[0].id
    );
    let Node::Track(hat) = p.folders[&inner].children[0] else {
        panic!("not a track")
    };
    assert_eq!(p.tracks[&hat].name, "Hat");
    assert_ne!(hat.to_string(), h);
    p.validate_deep().unwrap();
}

#[test]
fn a_new_bus_takes_the_picked_tracks_and_one_undo_takes_it_away() {
    let mut s = Session::new("bus-sources");
    let a = track_named(&mut s, "track.add.audio", "A");
    let b = track_named(&mut s, "track.add.audio", "B");
    let n = s.history().len();
    let r = run(
        &mut s,
        "track.add.bus",
        json!({"name": "Sub", "sources": [a, b]}),
    );
    assert_eq!(s.history().len(), n + 1);
    let bus: TrackId = id_of(&r).parse().unwrap();
    for t in [&a, &b] {
        let t: TrackId = t.parse().unwrap();
        assert_eq!(s.project().tracks[&t].output, Some(Route::Bus(bus)));
    }
    assert!(
        s.run_value(
            "track.add.bus",
            json!({"sources": ["master"]}),
            &RunOptions::human()
        )
        .is_err()
    );
    s.run_value("history.undo", json!({}), &RunOptions::human())
        .unwrap();
    assert!(!s.project().tracks.contains_key(&bus));
    let a: TrackId = a.parse().unwrap();
    assert_eq!(s.project().tracks[&a].output, None);
}

#[test]
fn a_track_can_be_copied_without_its_clips() {
    let mut s = Session::new("copy-empty");
    let a = audio(&mut s);
    let t = s.project().clip(a.parse().unwrap()).unwrap().0.id;
    run(&mut s, "track.mute", json!({"ids": [t.to_string()]}));
    let r = run(
        &mut s,
        "track.duplicate",
        json!({"ids": [t.to_string()], "mode": "settings"}),
    );
    let copy: TrackId = id_of(&r).parse().unwrap();
    let c = &s.project().tracks[&copy];
    assert!(c.clips.is_empty());
    assert!(c.mute);
    let full = run(&mut s, "track.duplicate", json!({"ids": [t.to_string()]}));
    let whole: TrackId = id_of(&full).parse().unwrap();
    assert_eq!(s.project().tracks[&whole].clips.len(), 1);
}
