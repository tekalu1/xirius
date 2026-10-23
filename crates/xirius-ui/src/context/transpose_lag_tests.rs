//! How long one change of a clip's transpose takes, measured without a window, and that a turn
//! of the field is one step of the history (UI-319).
//!
//! `cargo test -p xirius-ui transpose_lag -- --ignored --nocapture` prints the numbers.

use std::time::{Duration, Instant};

use serde_json::json;
use xirius_core::id::{ClipId, NoteId, TrackId};
use xirius_core::model::{ClipBody, Note};
use xirius_core::tempo::TempoMap;
use xirius_session::output::OutputMode;
use xirius_session::processed;

use super::fields::Kind;
use super::tests::studio;
use crate::arrange::probe::{clip_mid, track};
use crate::browser::RailTab;
use crate::studio::{Geometry, Hit, Studio};
use crate::tree::RowKind;

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// Times `f` `n` times and gives the middle one in ms.
fn time(n: usize, mut f: impl FnMut()) -> f64 {
    median(
        (0..n)
            .map(|_| {
                let t = Instant::now();
                f();
                ms(t.elapsed())
            })
            .collect(),
    )
}

/// `notes` notes in the first MIDI clip of Keys and `copies` copies of the track: a song the
/// size people work with.
fn heavy(s: &mut Studio, notes: usize, copies: usize) {
    let keys = track(s, "Keys");
    s.song
        .amend(|p| {
            let ids: Vec<u64> = (0..notes).map(|_| p.alloc()).collect();
            let c = std::sync::Arc::make_mut(p.tracks.get_mut(&keys).unwrap())
                .clips
                .iter_mut()
                .find(|c| c.is_midi())
                .unwrap();
            let len = c.length.max(480);
            if let ClipBody::Midi { notes: n, .. } = &mut c.body {
                let v = std::sync::Arc::make_mut(n);
                for (i, id) in ids.into_iter().enumerate() {
                    v.push(Note {
                        id: NoteId(id),
                        start: (i as i64 * 7) % (len - 60),
                        length: 60,
                        pitch: 40 + (i % 40) as u8,
                        velocity: 90,
                        muted: false,
                        ai: false,
                    });
                }
                v.sort_by_key(|n| (n.start, n.pitch, n.id.0));
            }
        })
        .unwrap();
    for _ in 0..copies {
        s.song
            .run("track.duplicate", json!({ "ids": [keys.to_string()] }))
            .unwrap();
    }
    s.after_edit();
}

/// Saves the song to a folder of its own and writes `secs` of tone where the Vocal clip's file
/// is.
fn with_sound(s: &mut Studio, name: &str, secs: f64) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let dir = root.join(format!("target/lag-audio-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    s.command(|song| song.save_as(&dir));
    assert!(s.error.is_none(), "{:?}", s.error);
    let t = track(s, "Vocal");
    let p = s.project();
    let ClipBody::Audio { file, .. } = &p.tracks[&t].clips[0].body else {
        panic!("audio clip");
    };
    let path = dir.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    crate::drop_audio::tests::tone(
        path.parent().unwrap(),
        path.file_name().unwrap().to_str().unwrap(),
        secs,
    );
}

fn play_silently(s: &mut Studio) {
    s.transport.mode = OutputMode::Silent { paced: true };
    assert!(s.toggle_play(), "{:?}", s.error);
    for _ in 0..5 {
        s.tick();
        std::thread::sleep(Duration::from_millis(16));
    }
}

fn first_clip(s: &Studio, name: &str, midi: bool) -> (TrackId, ClipId) {
    let t = track(s, name);
    let c = s.project().tracks[&t]
        .clips
        .iter()
        .find(|c| c.is_midi() == midi)
        .unwrap()
        .id;
    (t, c)
}

/// Where the inspector's transpose field is, the clip chosen.
fn inspector_semis(s: &mut Studio, g: &Geometry, (t, c): (TrackId, ClipId)) -> (f32, f32) {
    s.selected_row = Some(RowKind::Track(t));
    s.selected_clip = Some((t, c));
    s.arrange.clips = std::collections::BTreeSet::from([(t, c)]);
    if !s.inspector_open() {
        s.press(g, Hit::Rail(RailTab::Inspector));
    }
    let r = s
        .inspector_layout(g)
        .rows
        .iter()
        .find(|r| r.kind == Kind::Semis)
        .unwrap()
        .rect;
    (r.x + 30.0, r.y + r.h / 2.0)
}

/// The row of the open menu's transpose field.
fn menu_semis(s: &Studio) -> usize {
    s.context_rows()
        .iter()
        .position(|r| r.field.as_ref().is_some_and(|f| f.kind == Kind::Semis))
        .expect("the transpose field")
}

/// Times the parts one change goes through, on the song as it is now. Each run moves the value
/// up and down by one, so it is a change even at the end of the range.
fn parts(s: &mut Studio, g: &Geometry, c: ClipId) {
    let ids = json!([c.to_string()]);
    let flip = std::cell::Cell::new(1_i64);
    let args = || {
        flip.set(-flip.get());
        json!({ "ids": ids, "by": flip.get() })
    };
    let core = time(6, || {
        s.song.run("clip.transpose", args()).unwrap();
    });
    let check = time(6, || {
        s.song.check("clip.transpose", args()).unwrap();
    });
    let arranged = time(6, || {
        s.arrange_run("clip.transpose", args()).unwrap();
    });
    let p = s.project();
    let hash = time(5, || {
        std::hint::black_box(p.content_hash());
    });
    let clone = time(5, || {
        std::hint::black_box((*p).clone());
    });
    let mut rebuild = Vec::new();
    let mut write = Vec::new();
    let mut sync = Vec::new();
    let mut swap = Vec::new();
    for _ in 0..6 {
        s.song.run("clip.transpose", args()).unwrap();
        let t = Instant::now();
        s.rebuild(g);
        rebuild.push(ms(t.elapsed()));
        let t = Instant::now();
        let _ = s.song.check("clip.transpose.write", json!({ "ids": ids }));
        write.push(ms(t.elapsed()));
        let t = Instant::now();
        let heard = s.auto_heard(&s.project());
        s.transport.sync(&heard, s.song.dir());
        sync.push(ms(t.elapsed()));
        let t = Instant::now();
        s.tick();
        swap.push(ms(t.elapsed()));
    }
    let popup = time(5, || s.refresh_popup());
    let history = time(5, || {
        std::hint::black_box(s.song.history());
    });
    eprintln!(
        "  core run {core:.2} / dry run {check:.2} / arrange_run {arranged:.2} / content_hash \
         {hash:.2} / project clone {clone:.2} / rebuild {:.2} / engine sync {:.2} / next tick \
         {:.2} / the menu's dry run of writing into the notes {:.2} / refresh_popup {popup:.2} / \
         history {history:.2} ms",
        median(rebuild),
        median(sync),
        median(swap),
        median(write),
    );
}

/// Turns the wheel `n` notches 10 ms apart through `turn` and times each, the tick after it
/// included.
fn notches(s: &mut Studio, n: usize, mut turn: impl FnMut(&mut Studio, Instant) -> bool) {
    let mut now = Instant::now();
    let mut each = Vec::new();
    let mut ticks = Vec::new();
    let before = s.song.steps();
    for _ in 0..n {
        now += Duration::from_millis(10);
        let t = Instant::now();
        assert!(turn(s, now));
        each.push(ms(t.elapsed()));
        let t = Instant::now();
        s.tick();
        ticks.push(ms(t.elapsed()));
    }
    eprintln!(
        "  {n} notches: one takes {:.2} ms (max {:.2}), the tick after it {:.2} ms; undo steps \
         +{}",
        median(each.clone()),
        each.iter().copied().fold(0.0, f64::max),
        median(ticks),
        s.song.steps() - before,
    );
}

#[test]
#[ignore = "a measurement: run with --ignored --nocapture"]
fn transpose_lag_midi() {
    let (mut s, g) = studio();
    heavy(&mut s, 20_000, 15);
    play_silently(&mut s);
    let p = s.project();
    let notes: usize = p
        .tracks
        .values()
        .flat_map(|t| &t.clips)
        .map(|c| match &c.body {
            ClipBody::Midi { notes, .. } => notes.len(),
            _ => 0,
        })
        .sum();
    eprintln!(
        "MIDI: {} tracks, {notes} notes, {} KiB of JSON",
        p.tracks.len(),
        serde_json::to_vec(&*p).unwrap().len() / 1024
    );
    let clip = first_clip(&s, "Keys", true);
    let at = inspector_semis(&mut s, &g, clip);
    eprintln!(" inspector:");
    notches(&mut s, 12, |s, now| {
        s.inspector_wheel_at(&g, at, 60.0, false, false, now)
    });
    s.popup = None;
    let (x, y) = clip_mid(&s, &g, "Keys", 0);
    assert!(s.scene_context(&g, x, y));
    let i = menu_semis(&s);
    eprintln!(" clip menu:");
    notches(&mut s, 12, |s, now| {
        s.context_field_wheel_at(&g, i, 60.0, false, false, now)
    });
    eprintln!(" parts:");
    parts(&mut s, &g, clip.1);
}

#[test]
#[ignore = "a measurement: run with --ignored --nocapture"]
fn transpose_lag_audio() {
    let (mut s, g) = studio();
    with_sound(&mut s, "measure", 60.0);
    heavy(&mut s, 20_000, 15);
    play_silently(&mut s);
    let clip = first_clip(&s, "Vocal", false);
    let at = inspector_semis(&mut s, &g, clip);
    eprintln!("audio (a 60 s source):");
    let start = Instant::now();
    notches(&mut s, 12, |s, now| {
        s.inspector_wheel_at(&g, at, 60.0, false, false, now)
    });
    let rate = s.transport.host().unwrap().rate();
    let key_of = |s: &Studio| {
        let p = s.project();
        let c = p.clip(clip.1).unwrap().1.clone();
        processed::clip_key(
            &c,
            &TempoMap::from_project(&p),
            rate,
            s.song.dir().as_deref(),
        )
        .unwrap()
    };
    let last = key_of(&s);
    let turned = Instant::now();
    let mut ticks = Vec::new();
    while !matches!(processed::state(&last), Some(processed::State::Ready(_))) {
        assert!(turned.elapsed() < Duration::from_secs(120), "never ready");
        let t = Instant::now();
        s.tick();
        ticks.push(ms(t.elapsed()));
        std::thread::sleep(Duration::from_millis(16));
    }
    eprintln!(
        "  the last value sounds {:.0} ms after the last notch ({:.0} ms after the first); \
         ticks meanwhile {:.2} ms (max {:.2})",
        ms(turned.elapsed()),
        ms(start.elapsed()),
        median(ticks.clone()),
        ticks.iter().copied().fold(0.0, f64::max),
    );
    eprintln!(" parts:");
    parts(&mut s, &g, clip.1);
}

/// How far the clip is transposed, either way (the tests do not care which way is up).
fn semis(s: &Studio, (_, c): (TrackId, ClipId)) -> i8 {
    s.project().clip(c).unwrap().1.transpose().abs()
}

#[test]
fn a_turn_of_the_inspectors_transpose_is_one_step_and_one_undo() {
    let (mut s, g) = studio();
    let clip = first_clip(&s, "Keys", true);
    let at = inspector_semis(&mut s, &g, clip);
    let steps = s.song.steps();
    let mut now = Instant::now();
    for _ in 0..12 {
        now += Duration::from_millis(10);
        assert!(s.inspector_wheel_at(&g, at, 60.0, false, false, now));
    }
    assert_eq!(semis(&s, clip), 12);
    assert_eq!(
        s.song.steps(),
        steps + 1,
        "one turn, one step of the history"
    );
    assert!(s.song.undo());
    assert_eq!(semis(&s, clip), 0);
}

#[test]
fn a_turn_of_the_menus_transpose_is_one_step_and_one_undo() {
    let (mut s, g) = studio();
    let clip = first_clip(&s, "Keys", true);
    let (x, y) = clip_mid(&s, &g, "Keys", 0);
    assert!(s.scene_context(&g, x, y));
    let i = menu_semis(&s);
    let steps = s.song.steps();
    let mut now = Instant::now();
    for _ in 0..12 {
        now += Duration::from_millis(10);
        assert!(s.context_field_wheel_at(&g, i, 60.0, false, false, now));
    }
    assert_eq!(semis(&s, clip), 12);
    assert_eq!(
        s.song.steps(),
        steps + 1,
        "one turn, one step of the history"
    );
    // A turn that starts after the pause is a step of its own.
    now += Duration::from_millis(super::fields::WHEEL_GROUP_MS + 10);
    assert!(s.context_field_wheel_at(&g, i, -60.0, false, false, now));
    assert_eq!(semis(&s, clip), 11);
    assert_eq!(s.song.steps(), steps + 2);
    s.close_wheel_group();
    assert!(s.song.undo());
    assert_eq!(semis(&s, clip), 12);
    assert!(s.song.undo());
    assert_eq!(semis(&s, clip), 0);
}

#[test]
fn the_screens_edits_do_not_write_the_song_out_for_a_hash() {
    let (mut s, _) = studio();
    let (_, c) = first_clip(&s, "Keys", true);
    let data = s
        .song
        .run("clip.transpose", json!({ "ids": [c.to_string()], "by": 1 }))
        .unwrap();
    assert!(data.get("state_hash").is_none(), "{data}");
}
