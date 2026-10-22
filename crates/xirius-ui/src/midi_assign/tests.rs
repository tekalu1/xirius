use super::*;
use crate::song::Song;
use crate::studio::Hit;
use serde_json::json;
use xirius_engine::record::MockMidi;

const BLOCK: u64 = 256;

fn studio(tracks: usize) -> (Studio, Vec<TrackId>) {
    let mut s = Studio::new(Song::empty());
    let ids = (0..tracks)
        .map(|i| {
            let v = s
                .song
                .run("track.add.audio", json!({ "name": format!("Part {i}") }))
                .unwrap();
            v["id"].as_str().unwrap().parse().unwrap()
        })
        .collect();
    (s, ids)
}

/// A controller that sends `events` (poll number, message) on the port "Pad".
fn plug(s: &mut Studio, events: &[(u64, [u8; 3])]) {
    s.live_midi_input(Box::new(MockMidi::with_ports(
        vec!["Pad".into()],
        events.iter().map(|&(n, m)| (n * BLOCK, 0, m)).collect(),
    )));
}

fn polls(s: &mut Studio, n: usize) {
    for _ in 0..n {
        s.poll_live_midi();
    }
}

fn learn(s: &mut Studio, target: &str) -> bool {
    let args = json!({ "target": target });
    s.midi_learn(args.as_object().unwrap())
}

fn volume(s: &Studio, t: TrackId) -> f64 {
    s.project().tracks[&t].volume_db
}

#[test]
fn a_part_takes_the_first_knob_that_moves_after_learn() {
    let (mut s, ids) = studio(1);
    let t = ids[0];
    plug(&mut s, &[(2, [0xb0, 21, 10]), (3, [0xb0, 21, 90])]);
    assert!(learn(&mut s, &format!("track.volume:{t}")));
    assert!(s.live_midi.assign.learning.is_some());
    polls(&mut s, 2);
    assert!(s.live_midi.assign.learning.is_some());
    polls(&mut s, 1);
    assert!(s.live_midi.assign.learning.is_none());
    let map = s.midimap();
    assert_eq!(map.rows().len(), 1);
    assert_eq!(map.rows()[0].message, Message::Cc(21));
    assert_eq!(map.rows()[0].target, Target::Volume(t.to_string()));
    // The first message only gave the knob; it did not move the part.
    assert_eq!(s.project().tracks[&t].volume_db, 0.0);
}

#[test]
fn learn_without_an_input_or_a_part_is_refused() {
    let (mut s, ids) = studio(1);
    assert!(!learn(&mut s, &format!("track.volume:{}", ids[0])));
    assert!(s.error.is_some());
    s.error = None;
    plug(&mut s, &[]);
    assert!(!learn(&mut s, "track.volume:nope"));
    assert!(!learn(&mut s, "transport.play"));
    assert!(s.live_midi.assign.learning.is_none());
}

#[test]
fn a_knob_moves_its_part_in_one_step_and_the_notes_do_not_get_it() {
    let (mut s, ids) = studio(1);
    let t = ids[0];
    s.song
        .run(
            "midimap.set",
            json!({"message": "cc:7", "target": format!("track.volume:{t}")}),
        )
        .unwrap();
    plug(
        &mut s,
        &[(0, [0xb0, 7, 40]), (0, [0xb0, 7, 80]), (0, [0xb0, 7, 120])],
    );
    polls(&mut s, 1);
    let moved = volume(&s, t);
    assert!(moved > 0.0 - 0.001 && s.mixer.drag.is_some(), "{moved}");
    // The sweep ends after the knob has been still.
    s.mixer.knob_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(1));
    polls(&mut s, 1);
    assert!(s.mixer.drag.is_none());
    let end = volume(&s, t);
    assert_ne!(end, 0.0);
    assert!(s.song.undo());
    assert_eq!(volume(&s, t), 0.0);
}

#[test]
fn a_knob_that_moves_another_part_asks_before_it_is_taken_over() {
    let (mut s, ids) = studio(2);
    let (a, b) = (ids[0], ids[1]);
    s.song
        .run(
            "midimap.set",
            json!({"message": "cc:21", "target": format!("track.volume:{a}")}),
        )
        .unwrap();
    plug(&mut s, &[(1, [0xb0, 21, 64])]);
    assert!(learn(&mut s, &format!("track.volume:{b}")));
    polls(&mut s, 2);
    let c = s.live_midi.assign.clash.clone().expect("a clash");
    assert_eq!(c.holder, Target::Volume(a.to_string()));
    assert_eq!(c.target, Target::Volume(b.to_string()));
    // Nothing is written until the answer.
    assert!(
        s.midimap()
            .of_target(&Target::Volume(a.to_string()))
            .is_some()
    );
    assert!(
        s.midimap()
            .of_target(&Target::Volume(b.to_string()))
            .is_none()
    );
    let g = s.geometry(1440.0, 860.0);
    s.resized(&g);
    assert!(s.run_midi_commands(&g));
    assert!(s.ctx.is_some());
    assert!(s.context_choose(&g, "置き換える"));
    let map = s.midimap();
    assert!(map.of_target(&Target::Volume(a.to_string())).is_none());
    assert!(map.of_target(&Target::Volume(b.to_string())).is_some());
}

#[test]
fn cancel_gives_the_learning_up_with_the_pill_button_and_esc() {
    let (mut s, ids) = studio(1);
    plug(&mut s, &[]);
    assert!(learn(&mut s, &format!("track.volume:{}", ids[0])));
    let g = s.geometry(1440.0, 860.0);
    s.resized(&g);
    let p = s.center_of(&g, Hit::MidiCancel).unwrap();
    assert_eq!(s.hit(&g, p.0, p.1), Hit::MidiCancel);
    assert!(s.press(&g, s.hit(&g, p.0, p.1)));
    assert!(s.live_midi.assign.learning.is_none());
    assert!(learn(&mut s, &format!("track.pan:{}", ids[0])));
    assert!(s.midi_learn(json!({"cancel": true}).as_object().unwrap()));
    assert!(s.live_midi.assign.learning.is_none());
}

#[test]
fn a_pad_runs_its_command() {
    let (mut s, _) = studio(1);
    s.song
        .run(
            "midimap.set",
            json!({"message": "note:36", "target": "transport.play"}),
        )
        .unwrap();
    plug(&mut s, &[(0, [0x90, 36, 100]), (0, [0x80, 36, 0])]);
    polls(&mut s, 1);
    assert_eq!(s.live_midi.assign.commands, ["transport.play"]);
}

#[test]
fn the_settings_list_shows_and_frees_the_assignments() {
    let (mut s, ids) = studio(1);
    let t = ids[0];
    s.song
        .run(
            "midimap.set",
            json!({"message": "cc:21", "target": format!("track.volume:{t}")}),
        )
        .unwrap();
    let g = s.geometry(1440.0, 860.0);
    s.resized(&g);
    s.open_settings(crate::settings::Section::Midi);
    let scene = s.settings_scene(&g);
    let texts = &scene.texts;
    assert!(texts.iter().any(|x| x == "コントローラーの割り当て"));
    assert!(texts.iter().any(|x| x == "CC 21"));
    assert!(texts.iter().any(|x| x == "Part 0 · ボリューム"));
    let (x, y) = crate::audioset_shots::item(
        &s,
        &g,
        crate::settings::Do::MidiUnset(format!("track.volume:{t}")),
    );
    assert!(s.settings_press(&g, x, y));
    assert!(s.midimap().is_empty());
    let texts = s.settings_scene(&g).texts;
    assert!(texts.iter().any(|x| x == "未設定"));
}
