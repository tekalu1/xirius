//! KEY-012: the commands the rest of the Studio Pro key table asks for — clearing every solo or
//! mute, taking the clips of tracks out of the selection, selecting up to the song's start or end,
//! and playing the next or the previous take.
use serde_json::json;
use xirius_core::session::{RunOptions, Session};

fn add_track(s: &mut Session, name: &str) -> String {
    s.run_value(
        "track.add.instrument",
        json!({"name": name}),
        &RunOptions::human(),
    )
    .unwrap()
    .data["id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn add_clip(s: &mut Session, track: &str, at: i64, length: i64) -> String {
    s.run_value(
        "clip.add.midi",
        json!({"track": track, "at": at, "length": length}),
        &RunOptions::human(),
    )
    .unwrap()
    .data["id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn selected(s: &Session) -> Vec<String> {
    let mut v: Vec<String> = s.state.selection.iter().map(ToString::to_string).collect();
    v.sort();
    v
}

fn sorted(mut v: Vec<&String>) -> Vec<String> {
    v.sort();
    v.into_iter().cloned().collect()
}

fn muted(s: &Session, clip: &str) -> bool {
    s.project()
        .tracks
        .values()
        .flat_map(|t| &t.clips)
        .find(|c| c.id.to_string() == clip)
        .unwrap()
        .muted
}

#[test]
fn clearing_solo_and_mute_turns_every_track_off_in_one_undo() {
    let mut s = Session::new("key more");
    let opts = RunOptions::human();
    let a = add_track(&mut s, "A");
    let b = add_track(&mut s, "B");
    let c = add_track(&mut s, "C");
    s.run_value("track.solo", json!({"ids": [&a, &b], "on": true}), &opts)
        .unwrap();
    s.run_value("track.mute", json!({"ids": [&c], "on": true}), &opts)
        .unwrap();
    let r = s.run_value("track.solo.clear", json!({}), &opts).unwrap();
    assert_eq!(r.message, "2 トラックのソロを解除しました");
    assert!(s.project().tracks.values().all(|t| !t.solo));
    // Clearing the solo leaves the mute alone.
    assert!(s.project().tracks.values().any(|t| t.mute));
    let r = s.run_value("track.mute.clear", json!({}), &opts).unwrap();
    assert_eq!(r.message, "1 トラックのミュートを解除しました");
    assert!(s.project().tracks.values().all(|t| !t.mute));
    // Nothing to clear is not an error.
    let r = s.run_value("track.mute.clear", json!({}), &opts).unwrap();
    assert_eq!(r.message, "0 トラックのミュートを解除しました");
    s.run_value("history.undo", json!({}), &opts).unwrap();
    s.run_value("history.undo", json!({}), &opts).unwrap();
    assert!(s.project().tracks.values().any(|t| t.mute));
    s.run_value("history.undo", json!({}), &opts).unwrap();
    assert_eq!(s.project().tracks.values().filter(|t| t.solo).count(), 2);
}

#[test]
fn the_clips_of_the_given_or_selected_tracks_leave_the_selection() {
    let mut s = Session::new("key more");
    let opts = RunOptions::human();
    let a = add_track(&mut s, "A");
    let b = add_track(&mut s, "B");
    let a1 = add_clip(&mut s, &a, 0, 1920);
    let a2 = add_clip(&mut s, &a, 3840, 1920);
    let b1 = add_clip(&mut s, &b, 0, 1920);
    s.run_value("select.set", json!({"ids": [&a1, &a2, &b1]}), &opts)
        .unwrap();
    let r = s
        .run_value("select.off-tracks", json!({"ids": [&a]}), &opts)
        .unwrap();
    assert_eq!(r.message, "2 件を選択から外しました");
    assert_eq!(selected(&s), std::slice::from_ref(&b1));
    // Without ids, the tracks in the selection; the tracks stay selected.
    s.run_value("select.set", json!({"ids": [&a1, &b1, &b]}), &opts)
        .unwrap();
    s.run_value("select.off-tracks", json!({}), &opts).unwrap();
    assert_eq!(selected(&s), sorted(vec![&a1, &b]));
    // No track at all says so.
    s.run_value("select.set", json!({"ids": [&a1]}), &opts)
        .unwrap();
    let e = s
        .run_value("select.off-tracks", json!({}), &opts)
        .unwrap_err();
    assert!(e.to_string().contains("トラックを選んでから"), "{e}");
}

#[test]
fn the_selection_grows_to_the_songs_start_or_end_on_each_track() {
    let mut s = Session::new("key more");
    let opts = RunOptions::human();
    let a = add_track(&mut s, "A");
    let b = add_track(&mut s, "B");
    let a: Vec<String> = (0..4)
        .map(|i| add_clip(&mut s, &a, i * 3840, 1920))
        .collect();
    let b: Vec<String> = (0..3)
        .map(|i| add_clip(&mut s, &b, i * 3840, 1920))
        .collect();
    s.run_value("select.set", json!({"ids": [&a[1], &a[2], &b[1]]}), &opts)
        .unwrap();
    s.run_value("select.span", json!({"way": "start"}), &opts)
        .unwrap();
    // Up to the latest selected clip of each track.
    assert_eq!(
        selected(&s),
        sorted(vec![&a[0], &a[1], &a[2], &b[0], &b[1]])
    );
    s.run_value("select.set", json!({"ids": [&a[1], &a[2]]}), &opts)
        .unwrap();
    s.run_value("select.span", json!({"way": "end"}), &opts)
        .unwrap();
    assert_eq!(selected(&s), sorted(vec![&a[1], &a[2], &a[3]]));
    s.run_value("select.clear", json!({}), &opts).unwrap();
    let e = s
        .run_value("select.span", json!({"way": "end"}), &opts)
        .unwrap_err();
    assert!(e.to_string().contains("クリップを選んでから"), "{e}");
    assert!(
        s.run_value("select.span", json!({"way": "sideways"}), &opts)
            .is_err()
    );
}

#[test]
fn the_next_and_previous_take_play_alone_and_wrap_around() {
    let mut s = Session::new("key more");
    let opts = RunOptions::human();
    let t = add_track(&mut s, "Vocal");
    // Three takes over one another; the last one recorded plays.
    let takes: Vec<String> = (0..3).map(|_| add_clip(&mut s, &t, 0, 3840)).collect();
    s.run_value(
        "clip.mute",
        json!({"ids": [&takes[1], &takes[2]], "on": true}),
        &opts,
    )
    .unwrap();
    let plays = |s: &Session| -> Vec<bool> { takes.iter().map(|c| !muted(s, c)).collect() };
    assert_eq!(plays(&s), [true, false, false]);
    let r = s
        .run_value("take.next", json!({"id": &takes[2]}), &opts)
        .unwrap();
    assert_eq!(r.message, "テイク 2 を再生します");
    assert_eq!(plays(&s), [false, true, false]);
    s.run_value("take.next", json!({"id": &takes[0]}), &opts)
        .unwrap();
    assert_eq!(plays(&s), [false, false, true]);
    // After the last, the first.
    s.run_value("take.next", json!({"id": &takes[0]}), &opts)
        .unwrap();
    assert_eq!(plays(&s), [true, false, false]);
    // Before the first, the last.
    let r = s
        .run_value("take.prev", json!({"id": &takes[1]}), &opts)
        .unwrap();
    assert_eq!(r.message, "テイク 3 を再生します");
    assert_eq!(r.data["clip"], json!(takes[2]));
    assert_eq!(plays(&s), [false, false, true]);
    s.run_value("history.undo", json!({}), &opts).unwrap();
    assert_eq!(plays(&s), [true, false, false]);
    // A clip with nothing over it has no takes.
    let alone = add_clip(&mut s, &t, 7680, 1920);
    let e = s
        .run_value("take.next", json!({"id": alone}), &opts)
        .unwrap_err();
    assert!(e.to_string().contains("テイクが重なっていません"), "{e}");
}
