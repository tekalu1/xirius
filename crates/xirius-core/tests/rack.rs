//! The rack model (PRJ-026): saving, loading with repair, the relaxed
//! instrument rule and `rack.show`.
use serde_json::{Value, json};
use std::sync::Arc;
use xirius_core::id::{NodeId, PluginId, TrackId};
use xirius_core::model::{Plugin, PluginKind, Project, Track, TrackKind};
use xirius_core::rack::{Branch, Layer, Rack, RackNode, Source, Split, SplitMode};
use xirius_core::store::Store;
use xirius_core::track_preset::AdjustKind;
use xirius_core::{Session, session::RunOptions};

fn run(s: &mut Session, n: &str, a: Value) -> Value {
    s.run_value(n, a, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{n}: {e:?}"))
        .data
}

fn fresh_id() -> impl FnMut() -> u64 {
    let mut n = 9_000u64;
    move || {
        n += 1;
        n
    }
}

fn plugin(id: u64, kind: PluginKind) -> Plugin {
    Plugin {
        id: PluginId(id),
        name: format!("p{id}"),
        kind,
        format: "builtin".into(),
        uid: format!("u{id}"),
        bypass: false,
        deactivated: false,
        params: Default::default(),
        state: None,
        samples: Vec::new(),
        isolation: Default::default(),
        sidechain: None,
        preset: None,
    }
}

fn split(id: u64, branches: Vec<Vec<RackNode>>) -> RackNode {
    RackNode::Split(Split {
        id: NodeId(id),
        mode: SplitMode::Normal,
        crossovers: Vec::new(),
        branches: branches
            .into_iter()
            .enumerate()
            .map(|(i, fx)| Branch {
                id: NodeId(id * 10 + i as u64 + 1),
                fx,
                gain_db: 0.0,
                pan: 0.0,
                mute: false,
            })
            .collect(),
    })
}

fn multi(id: u64, instruments: &[u64]) -> Source {
    Source::Multi {
        id: NodeId(id),
        layers: instruments
            .iter()
            .enumerate()
            .map(|(i, x)| Layer {
                id: NodeId(id * 10 + i as u64 + 1),
                name: format!("l{x}"),
                instrument: PluginId(*x),
                fx: Vec::new(),
                keys: (0, 127),
                velocity: (1, 127),
                transpose: 0,
                gain_db: 0.0,
                pan: 0.0,
                mute: false,
            })
            .collect(),
    }
}

fn track(id: u64, kind: TrackKind, inserts: Vec<Plugin>, rack: Option<Rack>) -> Track {
    let mut t = Track::new(TrackId(id), kind, format!("t{id}"));
    t.inserts = inserts;
    t.rack = rack;
    t
}

fn project(tracks: Vec<Track>) -> Project {
    let mut p = Project::new("r");
    // The default tempo/timesig entries hold ids 1 and 2, under the test
    // plug-ins'; move them out of the way so `validate_deep` sees a clean
    // id space.
    for (i, t) in p.tempo.iter_mut().enumerate() {
        t.id = xirius_core::id::TempoId(900 + i as u64);
    }
    for (i, t) in p.timesig.iter_mut().enumerate() {
        t.id = xirius_core::id::TimeSigId(950 + i as u64);
    }
    for t in tracks {
        p.tracks.insert(t.id, Arc::new(t.clone()));
        p.root.push(xirius_core::id::Node::Track(t.id));
    }
    p.next_id = 1000;
    p
}

fn dir(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let d = tempfile::Builder::new()
        .prefix(&format!("xirius-rack-{name}-"))
        .tempdir()
        .unwrap();
    let dir = d.path().join("song.xirius");
    std::fs::create_dir_all(&dir).unwrap();
    (d, dir)
}

#[test]
fn a_track_without_a_rack_writes_no_rack_key() {
    let (_d, dir) = dir("shape");
    let p = project(vec![track(
        1,
        TrackKind::Instrument,
        vec![
            plugin(1, PluginKind::Instrument),
            plugin(2, PluginKind::Effect),
        ],
        None,
    )]);
    Store::new(&dir).write_project(&p).unwrap();
    let text = std::fs::read_to_string(dir.join("project.json")).unwrap();
    assert!(!text.contains("\"rack\""));
}

#[test]
fn a_rack_round_trips_through_project_json() {
    let (_d, dir) = dir("roundtrip");
    let rack = Rack {
        source: Some(multi(50, &[1, 2])),
        fx: vec![
            RackNode::Plugin(PluginId(3)),
            split(60, vec![vec![RackNode::Plugin(PluginId(4))], vec![]]),
        ],
    };
    let p = project(vec![track(
        1,
        TrackKind::Instrument,
        vec![
            plugin(1, PluginKind::Instrument),
            plugin(2, PluginKind::Instrument),
            plugin(3, PluginKind::Effect),
            plugin(4, PluginKind::Effect),
        ],
        Some(rack.clone()),
    )]);
    Store::new(&dir).write_project(&p).unwrap();
    let (q, rows) = Store::new(&dir).read_project_report().unwrap();
    assert!(rows.is_empty(), "a valid rack needs no repair: {rows:?}");
    assert_eq!(q.tracks[&TrackId(1)].rack, Some(rack));
}

#[test]
fn an_old_song_without_a_rack_opens_unchanged() {
    let (_d, dir) = dir("old");
    let p = project(vec![track(
        1,
        TrackKind::Instrument,
        vec![
            plugin(1, PluginKind::Instrument),
            plugin(2, PluginKind::Effect),
        ],
        None,
    )]);
    Store::new(&dir).write_project(&p).unwrap();
    let (q, rows) = Store::new(&dir).read_project_report().unwrap();
    assert!(rows.is_empty());
    assert_eq!(q.tracks[&TrackId(1)].rack, None);
}

#[test]
fn reading_repairs_the_rack_and_reports_every_change() {
    let t = track(
        1,
        TrackKind::Instrument,
        vec![
            plugin(1, PluginKind::Instrument),
            plugin(2, PluginKind::Effect),
            plugin(3, PluginKind::Effect),
            plugin(4, PluginKind::Effect),
        ],
        Some(Rack {
            source: Some(Source::Single(PluginId(2))), // an effect: rejected
            fx: vec![
                RackNode::Plugin(PluginId(9)), // not in inserts: dropped
                RackNode::Plugin(PluginId(3)),
                RackNode::Plugin(PluginId(3)), // twice: dropped
                RackNode::Plugin(PluginId(1)), // instrument in fx: rejected
            ],
        }),
    );
    let mut t = t;
    let rows = t.fix_rack(&mut fresh_id());
    // p1 becomes the source, p4 is filled at the end, then the rack is a
    // plain line and folds away.
    assert!(t.rack.is_none());
    let kinds: Vec<AdjustKind> = rows.iter().map(|r| r.kind).collect();
    assert!(kinds.contains(&AdjustKind::Rejected));
    assert!(kinds.contains(&AdjustKind::Dropped));
    assert!(kinds.contains(&AdjustKind::Filled));
    assert_eq!(
        t.inserts.iter().map(|x| x.id.0).collect::<Vec<_>>(),
        vec![1, 3, 2, 4]
    );
}

#[test]
fn a_serial_rack_folds_back_to_the_plain_shape() {
    let mut t = track(
        1,
        TrackKind::Audio,
        vec![plugin(1, PluginKind::Effect), plugin(2, PluginKind::Effect)],
        Some(Rack {
            source: None,
            fx: vec![RackNode::Plugin(PluginId(2)), RackNode::Plugin(PluginId(1))],
        }),
    );
    assert!(t.fix_rack(&mut fresh_id()).is_empty());
    assert_eq!(t.rack, None);
    assert_eq!(
        t.inserts.iter().map(|x| x.id.0).collect::<Vec<_>>(),
        vec![2, 1]
    );
}

#[test]
fn splits_over_the_limits_are_mended() {
    // A split nested deeper than MAX_SPLIT_DEPTH is dropped; a one-branch
    // split unfolds; a split of many branches keeps them all.
    let mut t = track(
        1,
        TrackKind::Audio,
        vec![
            plugin(1, PluginKind::Effect),
            plugin(2, PluginKind::Effect),
            plugin(3, PluginKind::Effect),
            plugin(4, PluginKind::Effect),
            plugin(5, PluginKind::Effect),
            plugin(6, PluginKind::Effect),
            plugin(7, PluginKind::Effect),
            plugin(8, PluginKind::Effect),
        ],
        Some(Rack {
            source: None,
            fx: vec![
                split(10, vec![vec![RackNode::Plugin(PluginId(1))]]),
                split(
                    20,
                    vec![
                        vec![RackNode::Plugin(PluginId(2))],
                        vec![RackNode::Plugin(PluginId(3))],
                        vec![RackNode::Plugin(PluginId(4))],
                        vec![RackNode::Plugin(PluginId(5))],
                        vec![RackNode::Plugin(PluginId(6))],
                        vec![],
                    ],
                ),
                split(
                    30,
                    vec![
                        vec![RackNode::Plugin(PluginId(8))],
                        vec![split(
                            40,
                            vec![
                                vec![],
                                vec![split(
                                    41,
                                    vec![
                                        vec![],
                                        vec![split(
                                            42,
                                            vec![
                                                vec![],
                                                vec![split(
                                                    43,
                                                    vec![
                                                        vec![RackNode::Plugin(PluginId(7))],
                                                        vec![],
                                                    ],
                                                )],
                                            ],
                                        )],
                                    ],
                                )],
                            ],
                        )],
                    ],
                ),
            ],
        }),
    );
    let rows = t.fix_rack(&mut fresh_id());
    assert!(!rows.is_empty());
    let Some(r) = &t.rack else {
        panic!("a real split stays")
    };
    // The one-branch split unfolded inline; the six-branch kept all six (a
    // split has no upper limit); the five-deep split is gone and its plug-in
    // is filled at the end.
    let top: Vec<&RackNode> = r.fx.iter().collect();
    assert!(matches!(top[0], RackNode::Plugin(_)));
    if let RackNode::Split(s) = top[1] {
        assert_eq!(s.branches.len(), 6);
    } else {
        panic!("second node is the six-branch split");
    }
    assert_eq!(
        t.inserts.iter().map(|x| x.id.0).collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 6, 8, 7]
    );
}

#[test]
fn a_split_of_many_branches_is_written_and_read_whole() {
    // A split has no upper limit (2026-11-06): 64 branches, each with its plug-in, come back
    // as they were written, with nothing dropped and nothing to report.
    let (_d, dir) = dir("many");
    let n = 64;
    let p = project(vec![track(
        2,
        TrackKind::Audio,
        (1..=n).map(|i| plugin(i, PluginKind::Effect)).collect(),
        Some(Rack {
            source: None,
            fx: vec![split(
                1000,
                (1..=n)
                    .map(|i| vec![RackNode::Plugin(PluginId(i))])
                    .collect(),
            )],
        }),
    )]);
    p.validate().unwrap();
    Store::new(&dir).write_project(&p).unwrap();
    let (q, rows) = Store::new(&dir).read_project_report().unwrap();
    assert!(rows.is_empty(), "{rows:?}");
    let t = &q.tracks[&TrackId(2)];
    let r = t.rack.as_ref().expect("the split stays");
    let RackNode::Split(s) = &r.fx[0] else {
        panic!("first node is the split")
    };
    assert_eq!(s.branches.len(), n as usize);
    assert_eq!(t.inserts.len(), n as usize);
    q.validate().unwrap();
}

#[test]
fn the_default_crossovers_rise_for_any_number_of_branches() {
    use xirius_core::rack::{CROSSOVER_GAP, crossovers_fit, default_crossovers};
    // Two to five keep the values they always had.
    assert_eq!(default_crossovers(2), [1000.0]);
    assert_eq!(default_crossovers(3), [200.0, 2000.0]);
    assert_eq!(default_crossovers(4), [150.0, 800.0, 4000.0]);
    assert_eq!(default_crossovers(5), [100.0, 400.0, 1600.0, 6000.0]);
    let gap = |c: &[f64]| c.windows(2).map(|w| w[1] / w[0]).fold(f64::MAX, f64::min);
    for n in [
        6, 7, 12, 64, 109, 110, 111, 112, 141, 142, 143, 144, 512, 100_000,
    ] {
        let c = default_crossovers(n);
        assert!(crossovers_fit(&c, n), "{n}: {:?}", &c[..c.len().min(8)]);
        // Up to 142 branches the bands keep the gap a dragged crossover keeps; past that they
        // still rise, narrower than the gap.
        let g = gap(&c);
        assert!(n > 142 || g >= CROSSOVER_GAP - 1e-12, "{n}: {g}");
        if n > 110 {
            // 111 and more start at 20 Hz and leave one even step free under 20 kHz, so the
            // next added branch has room.
            let top = c[n - 2];
            assert!((c[0] - 20.0).abs() < 1e-9, "{n}: {}", c[0]);
            assert!(top < 20_000.0, "{n}: {top}");
            assert!(
                (top * (1000.0_f64).powf(1.0 / (n - 1) as f64) - 20_000.0).abs() < 1.0,
                "{n}: {top}"
            );
        }
    }
    assert!(gap(&default_crossovers(143)) < CROSSOVER_GAP);
    // Six to 110 spread over 60 Hz to 12 kHz.
    for n in [6, 64, 110] {
        let c = default_crossovers(n);
        assert!(
            (c[0] - 60.0).abs() < 1e-9 && (c[n - 2] - 12_000.0).abs() < 1e-6,
            "{n}: {c:?}"
        );
    }
}

/// A frequency split of `n` empty branches with `crossovers`, alone on a track.
fn frequency_track(n: u64, crossovers: Vec<f64>) -> Track {
    let RackNode::Split(mut s) = split(1000, (0..n).map(|_| Vec::new()).collect()) else {
        unreachable!("split makes a split")
    };
    s.mode = SplitMode::Frequency;
    s.crossovers = crossovers;
    track(
        2,
        TrackKind::Audio,
        Vec::new(),
        Some(Rack {
            source: None,
            fx: vec![RackNode::Split(s)],
        }),
    )
}

fn crossovers_of(t: &Track) -> Vec<f64> {
    match &t.rack.as_ref().expect("the rack stays").fx[0] {
        RackNode::Split(s) => s.crossovers.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn missing_crossovers_are_filled_on_top_of_the_kept_ones_for_any_number_of_branches() {
    use xirius_core::rack::{crossovers_fit, crossovers_kept, default_crossovers};
    for n in [6, 64, 512] {
        // None at all, or out of order: the defaults, as they are.
        for broken in [vec![], vec![2000.0, 200.0], vec![500.0, 500.0]] {
            let mut t = frequency_track(n, broken.clone());
            t.fix_rack(&mut fresh_id());
            assert_eq!(
                crossovers_of(&t),
                default_crossovers(n as usize),
                "{n} {broken:?}"
            );
        }
        // Kept ones that rise stay, even outside 20 Hz to 20 kHz as an older song may hold
        // them; the missing ones go on top.
        for kept in [vec![1000.0], vec![5.0], vec![1000.0, 50_000.0]] {
            let mut t = frequency_track(n, kept.clone());
            t.fix_rack(&mut fresh_id());
            let c = crossovers_of(&t);
            assert!(crossovers_kept(&c, n as usize), "{n} {kept:?}");
            assert_eq!(c[..kept.len()], kept[..], "{n}");
        }
        // Ones in the range stay in it.
        let mut t = frequency_track(n, vec![1000.0]);
        t.fix_rack(&mut fresh_id());
        let c = crossovers_of(&t);
        assert!(crossovers_fit(&c, n as usize), "{n}: {:?}", &c[..6]);
    }
    let mut t = frequency_track(4, vec![1000.0]);
    t.fix_rack(&mut fresh_id());
    assert_eq!(crossovers_of(&t), [1000.0, 2000.0, 4000.0]);
    // A zero, a negative or one that is not a number goes; the rest stay.
    let mut t = frequency_track(3, vec![0.0, 300.0, -5.0, f64::NAN, 3000.0]);
    let rows = t.fix_rack(&mut fresh_id());
    assert_eq!(crossovers_of(&t), [300.0, 3000.0]);
    assert!(!rows.is_empty());
}

/// The crossovers a version before ENG-076 took and wrote: one fewer than the branches, above
/// zero and rising, outside 20 Hz to 20 kHz or closer than the gap.
const OLD_CROSSOVERS: [&[f64]; 4] = [
    &[10.0],
    &[1000.0, 25_000.0],
    &[1000.0, 1010.0],
    &[5.0, 30.0, 40_000.0],
];

/// A song with one frequency split per entry of [`OLD_CROSSOVERS`], each on its own track.
fn old_song() -> Project {
    let tracks = OLD_CROSSOVERS
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let k = i as u64;
            let RackNode::Split(mut s) = split(40 + k, (0..=c.len()).map(|_| Vec::new()).collect())
            else {
                unreachable!("split makes a split")
            };
            s.mode = SplitMode::Frequency;
            s.crossovers = c.to_vec();
            track(
                2 + k,
                TrackKind::Audio,
                Vec::new(),
                Some(Rack {
                    source: None,
                    fx: vec![RackNode::Split(s)],
                }),
            )
        })
        .collect();
    project(tracks)
}

fn old_crossovers(p: &Project) -> Vec<Vec<f64>> {
    (0..OLD_CROSSOVERS.len() as u64)
        .map(|k| crossovers_of(&p.tracks[&TrackId(2 + k)]))
        .collect()
}

#[test]
fn an_older_songs_crossovers_stay_as_they_were_saved_through_open_edit_save_and_open() {
    let want: Vec<Vec<f64>> = OLD_CROSSOVERS.iter().map(|c| c.to_vec()).collect();
    let (_d, dir) = dir("oldbands");
    let p = old_song();
    p.validate().unwrap();
    // The file as the older version wrote it, read on its own.
    Session::create(&dir, p.clone()).unwrap();
    std::fs::remove_dir_all(dir.join("history")).unwrap();
    Store::new(&dir).write_project(&p).unwrap();
    let (q, rows) = Store::new(&dir).read_project_report().unwrap();
    assert!(rows.is_empty(), "{rows:?}");
    assert_eq!(old_crossovers(&q), want);
    // Opened, something else changed, saved and opened again: still the same.
    let mut s = Session::open(&dir).unwrap();
    assert!(
        !s.notices.iter().any(|n| n.contains("クロスオーバー")),
        "{:?}",
        s.notices
    );
    assert_eq!(old_crossovers(s.project()), want);
    run(
        &mut s,
        "track.volume.set",
        json!({"ids": [TrackId(2).to_string(), TrackId(3).to_string()], "db": -6.0}),
    );
    s.save().unwrap();
    drop(s);
    let s = Session::open(&dir).unwrap();
    assert_eq!(old_crossovers(s.project()), want);
    assert_eq!(s.project().tracks[&TrackId(2)].volume_db, -6.0);
    s.project().validate().unwrap();
    let (q, rows) = Store::new(&dir).read_project_report().unwrap();
    assert!(rows.is_empty(), "{rows:?}");
    assert_eq!(old_crossovers(&q), want);
}

#[test]
fn a_branch_added_to_an_older_songs_split_keeps_its_crossovers() {
    use xirius_core::rack::crossovers_kept;
    let (_d, dir) = dir("oldadd");
    Session::create(&dir, old_song()).unwrap();
    let mut s = Session::open(&dir).unwrap();
    for (k, c) in OLD_CROSSOVERS.iter().enumerate() {
        run(
            &mut s,
            "rack.branch.add",
            json!({ "split": NodeId(40 + k as u64).to_string() }),
        );
        let now = crossovers_of(&s.project().tracks[&TrackId(2 + k as u64)]);
        assert_eq!(now[..c.len()], c[..], "{k}");
        assert!(crossovers_kept(&now, c.len() + 2), "{k}: {now:?}");
    }
}

/// Runs `name` on `s` as one undo step that undoes to the song before and redoes to the song
/// after, and leaves the song after.
fn undone(s: &mut Session, name: &str, args: Value) {
    let before = s.project().clone();
    run(s, name, args);
    let after = s.project().clone();
    assert_ne!(after, before, "{name} changed the song");
    s.undo(1).expect("undo");
    assert_eq!(*s.project(), before, "{name} undoes");
    s.redo(1).expect("redo");
    assert_eq!(*s.project(), after, "{name} redoes");
}

/// Fails `name` on `s` and leaves the song and its history as they were.
fn refused(s: &mut Session, name: &str, args: Value) -> String {
    let before = s.project().clone();
    let undo = s.can_undo();
    let e = match s.run_value(name, args, &RunOptions::human()) {
        Ok(r) => panic!("{name} should fail, said {}", r.message),
        Err(e) => e.message,
    };
    assert_eq!(*s.project(), before, "{name} left the song");
    assert_eq!(s.can_undo(), undo, "{name} left the history");
    e
}

#[test]
fn an_older_songs_crossover_outside_the_range_stays_or_moves_towards_it() {
    let (_d, dir) = dir("oldmove");
    Session::create(&dir, old_song()).unwrap();
    let mut s = Session::open(&dir).unwrap();
    let mode = |k: u64, c: &str| json!({ "id": NodeId(40 + k).to_string(), "mode": "frequency", "crossovers": c });
    let at = |s: &Session, k: u64| crossovers_of(&s.project().tracks[&TrackId(2 + k)]);
    // [1000, 25000]: the top one dragged down, still above 20 kHz, as the screen shows it.
    let down = 25_000.0 / 1.05_f64.powf(1.25);
    undone(&mut s, "rack.split.mode", mode(1, &format!("1000,{down}")));
    assert_eq!(at(&s, 1), [1000.0, down]);
    s.undo(1).unwrap();
    // The 1 kHz moved while the 25 kHz stays exactly as saved.
    undone(&mut s, "rack.split.mode", mode(1, "1500.25,25000"));
    assert_eq!(at(&s, 1), [1500.25, 25_000.0]);
    s.undo(1).unwrap();
    assert_eq!(at(&s, 1), [1000.0, 25_000.0]);
    // Moved further out, or a new one outside the range, is refused and changes nothing.
    for c in ["1000,26000", "1000,25000.5", "10,25000", "1000,20500,"] {
        let e = refused(&mut s, "rack.split.mode", mode(1, c));
        assert!(
            e.contains("20〜20000 Hz") || e.contains("読み取れません"),
            "{c}: {e}"
        );
    }
    // [10]: up towards 20 Hz is taken, down is not.
    undone(&mut s, "rack.split.mode", mode(0, "12.5"));
    s.undo(1).unwrap();
    refused(&mut s, "rack.split.mode", mode(0, "9"));
    // [5, 30, 40000]: the middle one moved, the two outside kept bit for bit.
    undone(
        &mut s,
        "rack.split.mode",
        mode(3, "5,31.000000000000004,40000"),
    );
    assert_eq!(at(&s, 3), [5.0, 31.000000000000004, 40_000.0]);
    s.undo(1).unwrap();
    // A normal split made frequency has none to keep: the range holds for every one.
    run(
        &mut s,
        "rack.split.mode",
        json!({ "id": NodeId(41).to_string(), "mode": "normal" }),
    );
    refused(&mut s, "rack.split.mode", mode(1, "1000,25000"));
    s.undo(1).unwrap();
    assert_eq!(old_crossovers(s.project())[1], [1000.0, 25_000.0]);
    s.project().validate().unwrap();
}

#[test]
fn an_older_songs_split_undoes_a_deleted_branch_and_a_switch_to_channel() {
    let (_d, dir) = dir("oldundo");
    Session::create(&dir, old_song()).unwrap();
    let mut s = Session::open(&dir).unwrap();
    let want: Vec<Vec<f64>> = OLD_CROSSOVERS.iter().map(|c| c.to_vec()).collect();
    for (k, c) in OLD_CROSSOVERS.iter().enumerate() {
        let split = 40 + k as u64;
        // The top branch deleted (the splitter comes off with two), and back.
        let top = NodeId(split * 10 + c.len() as u64 + 1);
        undone(
            &mut s,
            "rack.branch.remove",
            json!({ "id": top.to_string() }),
        );
        s.undo(1).unwrap();
        assert_eq!(old_crossovers(s.project()), want, "{k}");
        // Switched to channel and back by undo, then forward by redo.
        undone(
            &mut s,
            "rack.split.mode",
            json!({ "id": NodeId(split).to_string(), "mode": "channel" }),
        );
        assert!(crossovers_of(&s.project().tracks[&TrackId(2 + k as u64)]).is_empty());
        s.undo(1).unwrap();
        assert_eq!(old_crossovers(s.project()), want, "{k}");
    }
    s.project().validate().unwrap();
}

#[test]
fn a_branch_added_above_a_crossover_near_the_largest_number_keeps_it_or_changes_nothing() {
    use xirius_core::rack::crossovers_kept;
    let tops: [&[f64]; 3] = [&[1.75e308], &[1000.0, 1.75e308], &[f64::MAX]];
    let tracks = tops
        .iter()
        .enumerate()
        .map(|(k, c)| {
            let RackNode::Split(mut x) =
                split(60 + k as u64, (0..=c.len()).map(|_| Vec::new()).collect())
            else {
                unreachable!("split makes a split")
            };
            x.mode = SplitMode::Frequency;
            x.crossovers = c.to_vec();
            track(
                20 + k as u64,
                TrackKind::Audio,
                Vec::new(),
                Some(Rack {
                    source: None,
                    fx: vec![RackNode::Split(x)],
                }),
            )
        })
        .collect();
    let (_d, dir) = dir("bigtop");
    Session::create(&dir, project(tracks)).unwrap();
    let mut s = Session::open(&dir).unwrap();
    let at = |s: &Session, k: usize| crossovers_of(&s.project().tracks[&TrackId(20 + k as u64)]);
    for (k, c) in tops.iter().enumerate().take(2) {
        assert_eq!(at(&s, k), c.to_vec(), "{k}: kept on open");
        let add = json!({ "split": NodeId(60 + k as u64).to_string() });
        undone(&mut s, "rack.branch.add", add.clone());
        // Three more on top: every one there was stays, they rise and are numbers.
        for more in 0..3 {
            run(&mut s, "rack.branch.add", add.clone());
            let now = at(&s, k);
            assert_eq!(now[..c.len()], c[..], "{k} {more}");
            assert!(
                crossovers_kept(&now, c.len() + more + 3),
                "{k} {more}: {now:?}"
            );
        }
        s.undo(4).unwrap();
        assert_eq!(at(&s, k), c.to_vec(), "{k}: undone");
    }
    // Right at the largest number there is no room above: nothing changes, not even a reset.
    let e = refused(
        &mut s,
        "rack.branch.add",
        json!({ "split": NodeId(62).to_string() }),
    );
    assert!(e.contains("余地"), "{e}");
    assert_eq!(at(&s, 2), [f64::MAX]);
    s.project().validate().unwrap();
}

#[test]
fn frequency_crossovers_are_written_and_read_whole_for_any_number_of_branches() {
    use xirius_core::rack::default_crossovers;
    for n in [6, 64, 512] {
        let (_d, dir) = dir(&format!("bands-{n}"));
        let c = default_crossovers(n as usize);
        let p = project(vec![frequency_track(n, c.clone())]);
        p.validate().unwrap();
        Store::new(&dir).write_project(&p).unwrap();
        let (q, rows) = Store::new(&dir).read_project_report().unwrap();
        assert!(rows.is_empty(), "{n}: {rows:?}");
        assert_eq!(crossovers_of(&q.tracks[&TrackId(2)]), c, "{n}");
    }
}

#[test]
fn frequency_crossovers_are_made_to_match_the_branches() {
    let s = Split {
        id: NodeId(1),
        mode: SplitMode::Frequency,
        crossovers: vec![2000.0, 200.0, -5.0],
        branches: (0..3)
            .map(|i| Branch {
                id: NodeId(10 + i),
                fx: Vec::new(),
                gain_db: 0.0,
                pan: 0.0,
                mute: false,
            })
            .collect(),
    };
    // branches 3 => 2 ascending positive crossovers.
    let mut t = track(
        1,
        TrackKind::Audio,
        vec![plugin(1, PluginKind::Effect)],
        Some(Rack {
            source: None,
            fx: vec![RackNode::Split(s.clone())],
        }),
    );
    t.fix_rack(&mut fresh_id());
    let Some(Rack { fx, .. }) = &t.rack else {
        panic!("rack stays")
    };
    let RackNode::Split(s) = &fx[0] else {
        panic!("split stays")
    };
    assert_eq!(s.crossovers.len(), 2);
    assert!(s.crossovers[0] > 0.0 && s.crossovers[0] < s.crossovers[1]);
}

#[test]
fn a_multi_lets_a_track_hold_several_instruments() {
    let p = project(vec![track(
        1,
        TrackKind::Instrument,
        vec![
            plugin(1, PluginKind::Instrument),
            plugin(2, PluginKind::Instrument),
            plugin(3, PluginKind::Effect),
        ],
        Some(Rack {
            source: Some(multi(50, &[1, 2])),
            fx: vec![RackNode::Plugin(PluginId(3))],
        }),
    )]);
    p.validate().unwrap();
    // Two instruments without a multi is still refused.
    let bad = project(vec![track(
        1,
        TrackKind::Instrument,
        vec![
            plugin(1, PluginKind::Instrument),
            plugin(2, PluginKind::Instrument),
        ],
        None,
    )]);
    assert!(bad.validate().is_err());
}

/// A session whose track `TrackId(2)` of `kind` carries `inserts` and
/// `rack`, saved as a new song folder and opened again.
fn racked_session(
    name: &str,
    kind: TrackKind,
    inserts: Vec<Plugin>,
    rack: Option<Rack>,
) -> (tempfile::TempDir, Session, TrackId) {
    let (_d, dir) = dir(name);
    let mut p = project(vec![track(2, kind, inserts, rack)]);
    // Re-number the rack's node ids inside the project's own id space.
    fn bump(r: &mut Rack, p: &mut Project) {
        let mut map = |id: &mut NodeId| *id = NodeId(p.alloc());
        if let Some(Source::Multi { id, layers }) = &mut r.source {
            map(id);
            for l in layers {
                map(&mut l.id);
                for n in &mut l.fx {
                    if let RackNode::Split(s) = n {
                        map(&mut s.id);
                        for b in &mut s.branches {
                            map(&mut b.id);
                        }
                    }
                }
            }
        }
        for n in &mut r.fx {
            if let RackNode::Split(s) = n {
                map(&mut s.id);
                for b in &mut s.branches {
                    map(&mut b.id);
                }
            }
        }
    }
    let t = p.tracks.get_mut(&TrackId(2)).map(Arc::make_mut).unwrap();
    if let Some(mut r) = t.rack.take() {
        bump(&mut r, &mut p);
        p.tracks
            .get_mut(&TrackId(2))
            .map(Arc::make_mut)
            .unwrap()
            .rack = Some(r);
    }
    let s = Session::create(&dir, p).unwrap();
    (_d, s, TrackId(2))
}

#[test]
fn the_relaxed_instrument_rule_only_applies_to_a_multi() {
    // One instrument already sits in the multi's layer.
    let (_d, mut s, tid) = racked_session(
        "multi",
        TrackKind::Instrument,
        vec![plugin(3, PluginKind::Instrument)],
        Some(Rack {
            source: Some(multi(50, &[3])),
            fx: Vec::new(),
        }),
    );
    // plugin.add --kind instrument joins the multi as a new layer.
    let id: PluginId = run(
        &mut s,
        "plugin.add",
        json!({"track": tid.to_string(), "kind": "instrument", "name": "Synth"}),
    )["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let t = &s.project().tracks[&tid];
    let Some(Rack {
        source: Some(Source::Multi { layers, .. }),
        ..
    }) = &t.rack
    else {
        panic!("the multi stays: {:?}", t.rack)
    };
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[1].instrument, id);
    run(&mut s, "history.undo", json!({}));
    assert!(s.project().tracks[&tid].rack.as_ref().is_some_and(
        |r| matches!(&r.source, Some(Source::Multi { layers, .. }) if layers.len() == 1)
    ));
}

#[test]
fn plugin_commands_keep_the_rack_consistent() {
    let (_d, mut s, tid) = racked_session(
        "ops",
        TrackKind::Audio,
        Vec::new(),
        Some(Rack {
            source: None,
            fx: vec![split(50, vec![vec![], vec![]])],
        }),
    );
    let a: PluginId = run(
        &mut s,
        "plugin.add",
        json!({"track": tid.to_string(), "name": "EQ"}),
    )["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let b: PluginId = run(
        &mut s,
        "plugin.add",
        json!({"track": tid.to_string(), "name": "Comp"}),
    )["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    // Every insert is in the rack exactly once; the new ones filled the end.
    let t = &s.project().tracks[&tid];
    let flat = t.rack.as_ref().unwrap().flatten_order();
    let mut sorted = flat.clone();
    sorted.sort();
    let mut inserts: Vec<_> = t.inserts.iter().map(|x| x.id).collect();
    inserts.sort();
    assert_eq!(sorted, inserts);
    // plugin.move --index 0 puts the node first in the top chain.
    run(
        &mut s,
        "plugin.move",
        json!({"id": b.to_string(), "track": tid.to_string(), "index": 0}),
    );
    let t = &s.project().tracks[&tid];
    assert!(matches!(
        t.rack.as_ref().unwrap().fx.first(),
        Some(RackNode::Plugin(x)) if *x == b
    ));
    // plugin.remove drops the reference from the rack.
    run(&mut s, "plugin.remove", json!({"ids": a.to_string()}));
    let t = &s.project().tracks[&tid];
    assert!(!t.rack.as_ref().unwrap().uses_plugin(a));
    s.project().validate_deep().unwrap();
}

#[test]
fn filled_layers_get_their_own_node_ids() {
    let (_d, mut s, tid) = racked_session(
        "layerids",
        TrackKind::Instrument,
        vec![plugin(3, PluginKind::Instrument)],
        Some(Rack {
            source: Some(multi(50, &[3])),
            fx: Vec::new(),
        }),
    );
    for name in ["A", "B"] {
        run(
            &mut s,
            "plugin.add",
            json!({"track": tid.to_string(), "kind": "instrument", "name": name}),
        );
    }
    let t = &s.project().tracks[&tid];
    let Some(Rack {
        source: Some(Source::Multi { layers, .. }),
        ..
    }) = &t.rack
    else {
        panic!("the multi stays")
    };
    assert_eq!(layers.len(), 3);
    let ids: Vec<NodeId> = layers.iter().map(|l| l.id).collect();
    assert!(ids.iter().all(|i| i.0 > 0));
    assert_ne!(ids[1], ids[2], "each filled layer gets a fresh id");
    s.project().validate_deep().unwrap();
}

#[test]
fn an_instrument_moved_inside_its_track_keeps_its_layer() {
    let mut layer_multi = multi(50, &[3]);
    if let Source::Multi { layers, .. } = &mut layer_multi {
        layers[0].name = "LOW".into();
        layers[0].keys = (36, 59);
        layers[0].velocity = (1, 100);
        layers[0].transpose = -12;
        layers[0].gain_db = -3.0;
        layers[0].pan = -0.5;
    }
    let (_d, mut s, tid) = racked_session(
        "layermove",
        TrackKind::Instrument,
        vec![plugin(3, PluginKind::Instrument)],
        Some(Rack {
            source: Some(layer_multi),
            fx: Vec::new(),
        }),
    );
    // A no-op move (same track, no index) must not rebuild the layer.
    run(&mut s, "plugin.move", json!({"id": "plg_3"}));
    let t = &s.project().tracks[&tid];
    let Some(Rack {
        source: Some(Source::Multi { layers, .. }),
        ..
    }) = &t.rack
    else {
        panic!("the multi stays")
    };
    assert_eq!(layers.len(), 1);
    let l = &layers[0];
    assert_eq!(l.name, "LOW");
    assert_eq!(l.keys, (36, 59));
    assert_eq!(l.velocity, (1, 100));
    assert_eq!(l.transpose, -12);
    assert_eq!(l.gain_db, -3.0);
    assert_eq!(l.pan, -0.5);
    s.project().validate_deep().unwrap();
}

#[test]
fn a_track_preset_goes_on_a_racked_track() {
    let (_d, mut s, tid) = racked_session(
        "preset",
        TrackKind::Instrument,
        vec![
            plugin(3, PluginKind::Instrument),
            plugin(4, PluginKind::Effect),
            plugin(5, PluginKind::Effect),
        ],
        Some(Rack {
            source: Some(Source::Single(PluginId(3))),
            fx: vec![
                RackNode::Plugin(PluginId(4)),
                split(50, vec![vec![RackNode::Plugin(PluginId(5))], vec![]]),
            ],
        }),
    );
    // Saving the same track and putting it back used to leave the rack
    // pointing at the old instrument and fail the project check.
    let preset = xirius_core::track_preset::TrackPreset::snapshot(s.project(), tid, "x")
        .unwrap()
        .to_value();
    run(
        &mut s,
        "track.preset.put",
        json!({"ids": [tid.to_string()], "preset": preset.to_string()}),
    );
    let t = &s.project().tracks[&tid];
    // The seat kept its place: the new instrument is the source again.
    let Some(Rack {
        source: Some(Source::Single(inst)),
        ..
    }) = &t.rack
    else {
        panic!("the rack keeps its source: {:?}", t.rack)
    };
    assert_ne!(*inst, PluginId(3), "the new instrument took the seat");
    s.project().validate_deep().unwrap();
}

#[test]
fn rack_show_returns_the_tree() {
    let (_d, mut s, tid) = racked_session(
        "show",
        TrackKind::Audio,
        Vec::new(),
        Some(Rack {
            source: None,
            fx: vec![split(50, vec![vec![], vec![]])],
        }),
    );
    let v = run(&mut s, "rack.show", json!({"track": tid.to_string()}));
    assert_eq!(v["implicit"], false);
    assert!(v["fx"][0]["split"]["branches"].as_array().unwrap().len() == 2);
    // A track with no rack answers the implicit serial shape.
    let plain: TrackId = run(&mut s, "track.add.audio", json!({}))["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let v = run(&mut s, "rack.show", json!({"track": plain.to_string()}));
    assert_eq!(v["implicit"], true);
}

#[test]
fn a_repaired_rack_is_announced_when_the_song_opens() {
    let (_d, dir) = dir("notice");
    let p = project(vec![track(
        2,
        TrackKind::Audio,
        vec![plugin(1, PluginKind::Effect)],
        Some(Rack {
            source: None,
            fx: vec![RackNode::Plugin(PluginId(99))],
        }),
    )]);
    Session::create(&dir, p.clone()).unwrap();
    // project.json holds the inconsistent rack; the history must not shadow
    // it, so the open reads the file itself.
    std::fs::remove_dir_all(dir.join("history")).unwrap();
    Store::new(&dir).write_project(&p).unwrap();
    let s = Session::open(&dir).unwrap();
    assert!(
        s.notices.iter().any(|n| n.contains("ラック")),
        "notices: {:?}",
        s.notices
    );
}

#[test]
fn a_state_only_repair_is_announced_too() {
    // project.json is clean but the history state holds an inconsistent
    // rack: the open still has to say it repaired something.
    let (_d, dir) = dir("statenotice");
    let good = project(vec![track(
        2,
        TrackKind::Audio,
        vec![plugin(1, PluginKind::Effect)],
        Some(Rack {
            source: None,
            fx: vec![
                RackNode::Plugin(PluginId(1)),
                split(50, vec![vec![], vec![]]),
            ],
        }),
    )]);
    Session::create(&dir, good.clone()).unwrap();
    let store = Store::new(&dir);
    let cursor = store.read_index().unwrap().unwrap().cursor;
    let mut bad = good;
    Arc::make_mut(bad.tracks.get_mut(&TrackId(2)).unwrap()).rack = Some(Rack {
        source: None,
        fx: vec![
            RackNode::Plugin(PluginId(1)),
            split(50, vec![vec![RackNode::Plugin(PluginId(99))], vec![]]),
        ],
    });
    store.write_state(cursor, &bad).unwrap();
    let s = Session::open(&dir).unwrap();
    assert!(
        s.notices.iter().any(|n| n.contains("ラック")),
        "notices: {:?}",
        s.notices
    );
}
