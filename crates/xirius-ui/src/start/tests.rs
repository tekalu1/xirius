//! The new song's screen (UI-074・UI-350): the three entries, 「オーディオデバイスなし」 with no
//! device, and the test tone on 設定 › オーディオ › 出力.

use serde_json::json;

use super::*;
use crate::instrument::current;
use crate::song::Song;
use xirius_core::model::TrackKind;
use xirius_session::audio_settings::PC_KEYBOARD;

fn empty() -> (Studio, Geometry) {
    let s = Studio::new(Song::empty());
    let g = s.geometry(1600.0, 1000.0);
    (s, g)
}

/// Presses an entry where it shows, as a click does.
fn press(s: &mut Studio, g: &Geometry, e: Entry) -> bool {
    let (x, y) = s.center_of(g, Hit::Start(e)).expect("on screen");
    assert_eq!(s.hit(g, x, y), Hit::Start(e));
    s.press(g, Hit::Start(e))
}

#[test]
fn a_new_song_shows_the_entries_in_the_middle_of_the_body() {
    let (s, g) = empty();
    assert!(s.start_shown());
    let z = zone(&g);
    let (audio, _) = s.entry_rects(&g, Entry::Audio);
    let (drop, _) = s.entry_rects(&g, Entry::Drop);
    let middle = (audio.x + drop.right()) / 2.0;
    assert!((middle - (z.x + z.w / 2.0)).abs() < 0.5, "centred");
    let (midi, circle) = s.entry_rects(&g, Entry::Midi);
    assert_eq!((midi.w, circle.w, circle.h), (110.0, 46.0, 46.0));
    assert_eq!(
        midi.x - audio.right(),
        30.0,
        "the mock's gap between entries"
    );
    // The entries (and the line under them, with no device) are centred up and down too.
    let l = s.start_layout(&g);
    let bottom = l.no_device.map_or(audio.bottom(), |r| r.bottom());
    let (above, below) = (audio.y - z.y, z.bottom() - bottom);
    assert!((above - below).abs() < 0.5, "{above} {below}");
    // The drop entry is a label: it is not a button.
    let (x, y) = (drop.x + drop.w / 2.0, drop.y + 20.0);
    assert_ne!(s.hit(&g, x, y), Hit::Start(Entry::Drop));
}

#[test]
fn a_pc_key_that_plays_lights_the_pc_keyboard() {
    let (mut s, g) = empty();
    assert!(press(&mut s, &g, Entry::Midi));
    assert!(s.pc_key_down("g", false));
    let mut io = s.song.io.lock().unwrap();
    io.poll(std::time::Instant::now());
    assert_eq!(io.view.received, vec![PC_KEYBOARD.to_string()]);
    assert_eq!(io.view.notes.get(PC_KEYBOARD), Some(&67));
}

#[test]
fn midi_makes_an_armed_instrument_track_and_turns_the_pc_keys_on() {
    let (mut s, g) = empty();
    assert!(press(&mut s, &g, Entry::Midi));
    assert_eq!(s.error, None);
    let p = s.project();
    assert_eq!(p.root.len(), 1);
    let t = p
        .tracks
        .values()
        .find(|t| t.kind != TrackKind::Master)
        .unwrap();
    assert_eq!(t.kind, TrackKind::Instrument);
    assert!(t.armed, "armed to record");
    assert_eq!(current(t).map(|x| x.name.as_str()), Some(DEFAULT));
    assert_eq!(s.selected_row, Some(RowKind::Track(t.id)));
    assert_eq!(s.pc_target(), Some(t.id));
    assert!(s.pc_keys.on);
    assert!(
        s.notice_text()
            .is_some_and(|n| n.starts_with("Sampler を選択しました")),
        "{:?}",
        s.notice_text()
    );
    // The track shows: the entries go.
    assert!(!s.start_shown());
    let (r, _) = s.entry_rects(&g, Entry::Midi);
    assert_ne!(s.hit(&g, r.x + 40.0, r.y + 30.0), Hit::Start(Entry::Midi));
}

/// A PC with these audio devices, each `(name, inputs, outputs)`: nothing is opened, and the
/// output is silent.
fn with_devices(list: &[(&str, usize, usize)]) -> (Studio, Geometry) {
    use xirius_engine::devices::{Device, Simulated};
    struct Quiet;
    impl xirius_engine::record::AudioInput for Quiet {
        fn channels(&self) -> usize {
            2
        }
        fn read(&mut self, left: &mut [f32], right: &mut [f32]) {
            left.fill(0.0);
            right.fill(0.0);
        }
    }
    let (mut s, g) = empty();
    s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
    let devices = list
        .iter()
        .map(|(name, inputs, outputs)| Device {
            name: name.to_string(),
            inputs: (1..=*inputs).map(|i| format!("In {i}")).collect(),
            outputs: (1..=*outputs).map(|i| format!("Out {i}")).collect(),
        })
        .collect();
    s.song
        .io
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .replace(Box::new(Simulated::new(
            devices,
            Default::default(),
            Box::new(Quiet),
        )));
    s.poll_io();
    (s, g)
}

/// A PC whose only audio device (speakers) has no inputs.
fn speakers_only() -> (Studio, Geometry) {
    with_devices(&[("Speakers", 0, 2)])
}

/// Speakers, and an interface with two inputs.
fn speakers_and_interface() -> (Studio, Geometry) {
    with_devices(&[("Speakers", 0, 2), ("Interface A", 2, 2)])
}

/// The device in use.
fn device(s: &Studio) -> String {
    s.song.io.lock().unwrap().view.device.clone()
}

/// The audio tracks of the song.
fn audio_tracks(s: &Studio) -> Vec<std::sync::Arc<xirius_core::model::Track>> {
    s.project()
        .tracks
        .values()
        .filter(|t| t.kind == TrackKind::Audio)
        .cloned()
        .collect()
}

/// Presses the row of the open menu that says `text`, as a click does.
fn choose(s: &mut Studio, g: &Geometry, text: &str) {
    let l = s.popup.as_ref().expect("a menu is open").layout(g.size);
    let i = l.cells.iter().position(|c| c.text == text).unwrap();
    let (x, y) = s.center_of(g, Hit::Item(i)).unwrap();
    assert_eq!(s.hit(g, x, y), Hit::Item(i));
    assert!(s.press(g, Hit::Item(i)));
}

/// The rows of the open menu: their text and their hint.
fn menu_rows(s: &Studio, g: &Geometry) -> Vec<(String, String)> {
    let l = s.popup.as_ref().expect("a menu is open").layout(g.size);
    // The first cell is the list menu's search box (UI-320).
    l.cells
        .iter()
        .skip(1)
        .map(|c| (c.text.clone(), c.hint.clone()))
        .collect()
}

#[test]
fn audio_makes_an_armed_audio_track() {
    let (mut s, g) = speakers_and_interface();
    assert!(press(&mut s, &g, Entry::Audio));
    assert_eq!(s.error, None);
    let tracks = audio_tracks(&s);
    assert_eq!(tracks.len(), 1);
    assert!(tracks[0].armed);
    assert!(!s.pc_keys.on, "the PC keys are for instruments");
    assert!(s.popup.is_none());
}

#[test]
fn audio_on_a_device_without_inputs_makes_no_track_and_lists_the_devices_with_inputs() {
    let (mut s, g) = speakers_and_interface();
    // The interface is chosen by itself; the person picked the speakers.
    assert_eq!(device(&s), "Interface A");
    assert!(
        s.song
            .set("audio.set", json!({"key": "device", "value": "Speakers"}))
            .is_ok()
    );
    s.poll_io();
    assert_eq!(device(&s), "Speakers");
    assert!(!s.device_has_inputs());
    assert!(s.audio_entry_marked());

    assert!(press(&mut s, &g, Entry::Audio));
    assert_eq!(s.error, None);
    assert!(
        audio_tracks(&s).is_empty(),
        "nothing to record from: no track"
    );
    assert!(s.start_shown());
    let p = s.popup.as_ref().expect("the list of the devices opens");
    assert_eq!(p.kind, crate::popup::PopupKind::Device(true));
    let (audio, circle) = s.entry_rects(&g, Entry::Audio);
    assert_eq!(p.anchor, circle, "under the entry");
    assert!(audio.contains(p.anchor.x, p.anchor.y));
    assert_eq!(
        menu_rows(&s, &g),
        vec![("Interface A".to_string(), "入力 2 · 出力 2".to_string())],
        "only the devices with inputs"
    );

    // Choosing one makes the armed track on it.
    choose(&mut s, &g, "Interface A");
    assert_eq!(s.error, None);
    assert!(s.popup.is_none());
    assert_eq!(device(&s), "Interface A");
    let tracks = audio_tracks(&s);
    assert_eq!(tracks.len(), 1);
    assert!(tracks[0].armed);
}

#[test]
fn audio_with_only_speakers_lists_no_device_to_choose() {
    let (mut s, g) = speakers_only();
    assert!(press(&mut s, &g, Entry::Audio));
    assert_eq!(s.error, None);
    assert!(audio_tracks(&s).is_empty());
    assert_eq!(
        menu_rows(&s, &g),
        vec![(rig::NO_DEVICE.to_string(), String::new())]
    );
    // The row says so; a press on it does nothing.
    let l = s.popup.as_ref().unwrap().layout(g.size);
    let r = l.cells[0].rect;
    assert_eq!(s.hit(&g, r.x + r.w / 2.0, r.y + r.h / 2.0), Hit::PopupBody);
    // The cursor never sits on it, not even after ↓ (UI-338).
    for key in ["", "down", "up"] {
        if !key.is_empty() {
            s.typed(&g, key, None);
        }
        let l = s.popup.as_ref().unwrap().layout(g.size);
        let row = l.cells.iter().find(|c| c.text == rig::NO_DEVICE).unwrap();
        assert!(!row.cursor && !row.live(), "{key}");
        assert!(l.cells.iter().all(|c| !c.cursor));
    }
}

#[test]
fn a_song_with_tracks_has_no_entries() {
    let s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    assert!(!s.start_shown());
    let (r, _) = s.entry_rects(&g, Entry::Midi);
    assert!(s.start_at(&g, r.x + 40.0, r.y + 30.0).is_none());
    assert!(s.center_of(&g, Hit::Entrance(Part::NoDevice)).is_none());
}

/// Only the three entries, centred: no list, no panel (UI-350).
#[test]
fn the_new_song_shows_only_the_three_entries() {
    let (s, g) = speakers_and_interface();
    let l = s.start_layout(&g);
    assert_eq!(l.no_device, None, "a device: no line");
    assert!(s.center_of(&g, Hit::Entrance(Part::NoDevice)).is_none());
    let z = zone(&g);
    let (audio, _) = s.entry_rects(&g, Entry::Audio);
    let mid = audio.y + audio.h / 2.0;
    assert!((mid - (z.y + z.h / 2.0)).abs() < 0.5, "centred up and down");
}

/// A narrow body draws the entries narrower and closer: 110 / 30 become 96 / 8 (UI-350).
#[test]
fn a_narrow_body_draws_the_entries_closer() {
    let s = Studio::new(Song::empty());
    let mut w = 900.0;
    let g = loop {
        let g = s.geometry(w, 700.0);
        if zone(&g).w < 3.0 * 110.0 + 2.0 * 30.0 {
            break g;
        }
        w -= 10.0;
        assert!(w > 0.0);
    };
    let (audio, _) = s.entry_rects(&g, Entry::Audio);
    let (midi, circle) = s.entry_rects(&g, Entry::Midi);
    let (drop, _) = s.entry_rects(&g, Entry::Drop);
    assert_eq!((midi.w, circle.w), (96.0, 46.0));
    assert_eq!(midi.x - audio.right(), 8.0);
    assert_eq!(drop.x - midi.right(), 8.0);
    let z = zone(&g);
    let middle = (audio.x + drop.right()) / 2.0;
    assert!((middle - (z.x + z.w / 2.0)).abs() < 0.5, "centred");
}

/// The switch sits where three entries at 110 / 30 stop fitting, so the smallest window the
/// app opens (960×600, UI-300) keeps them wide: the narrow form only shows in the shots
/// (UI-350 review).
#[test]
fn the_smallest_window_keeps_the_wide_entries() {
    let s = Studio::new(Song::empty());
    let g = s.geometry(960.0, 600.0);
    assert!(zone(&g).w >= 3.0 * WIDTH + 2.0 * BETWEEN);
    let (audio, _) = s.entry_rects(&g, Entry::Audio);
    let (midi, _) = s.entry_rects(&g, Entry::Midi);
    assert_eq!(midi.w, WIDTH);
    assert_eq!(midi.x - audio.right(), BETWEEN);
    // Just under the room the three need, they draw closer.
    let mut w = 960.0;
    while zone(&s.geometry(w, 600.0)).w >= 3.0 * WIDTH + 2.0 * BETWEEN {
        w -= 1.0;
    }
    let g = s.geometry(w, 600.0);
    assert_eq!(s.entry_rects(&g, Entry::Midi).0.w, NARROW_WIDTH);
    let g = s.geometry(w + 1.0, 600.0);
    assert_eq!(s.entry_rects(&g, Entry::Midi).0.w, WIDTH);
}

/// No audio device: one line under the entries, whose menu rescans or opens the audio
/// settings (UI-350).
#[test]
fn with_no_device_the_new_song_says_so_and_its_menu_rescans_or_opens_the_settings() {
    let (mut s, g) = with_devices(&[]);
    assert!(!s.has_audio_device());
    // Said once: the line, not also the mark on the recording entry.
    assert!(!s.audio_entry_marked());
    let line = s.start_layout(&g).no_device.expect("the line");
    let (audio, _) = s.entry_rects(&g, Entry::Audio);
    assert!(line.y >= audio.bottom(), "under the entries");
    let z = zone(&g);
    assert!((line.x + line.w / 2.0 - (z.x + z.w / 2.0)).abs() < 0.5);
    let (x, y) = s.center_of(&g, Hit::Entrance(Part::NoDevice)).unwrap();
    assert_eq!(s.hit(&g, x, y), Hit::Entrance(Part::NoDevice));
    assert!(s.press(&g, Hit::Entrance(Part::NoDevice)));
    assert_eq!(
        s.popup.as_ref().map(|p| p.kind),
        Some(crate::popup::PopupKind::NoDevice)
    );
    let l = s.popup.as_ref().unwrap().layout(g.size);
    let texts: Vec<_> = l.cells.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(texts, ["再スキャン", "オーディオの設定…"]);
    choose(&mut s, &g, "再スキャン");
    assert!(s.popup.is_none());
    assert_eq!(s.error, None);
    assert!(s.press(&g, Hit::Entrance(Part::NoDevice)));
    choose(&mut s, &g, "オーディオの設定…");
    assert!(s.popup.is_none());
    assert_eq!(
        s.settings.as_ref().map(|u| u.section),
        Some(crate::settings::Section::Audio)
    );
}

#[test]
fn the_settings_audio_page_has_the_test_tone_too() {
    use crate::settings::{Do, Section};
    let (mut s, g) = speakers_and_interface();
    s.open_settings(Section::Audio);
    let scene = s.settings_scene(&g);
    assert!(scene.has_text("出力"));
    let p = crate::audioset_shots::item(&s, &g, Do::Tone);
    // The meters show only while it sounds (UI-350).
    assert!(!scene.has_text("L") && !scene.has_text("R"));
    assert!(s.settings_press(&g, p.0, p.1));
    assert_eq!(s.error, None);
    assert!(s.rig_tone, "the button is lit while it sounds");
    let scene = s.settings_scene(&g);
    assert!(scene.has_text(rig::TONE));
    assert!(scene.has_text("L") && scene.has_text("R"));

    // The page's meter follows the output, left first, at about -18 dBFS.
    let level = 10f32.powf(xirius_engine::tone::LEVEL_DB / 20.0);
    std::thread::sleep(std::time::Duration::from_millis(150));
    assert!(s.rig_tick(), "moving while it sounds on the settings page");
    let [l, r] = s.rig_out;
    assert!((20.0 * (l / level).log10()).abs() < 0.2, "{l}");
    assert_eq!(r, 0.0);
    let start = std::time::Instant::now();
    let mut polls = Vec::new();
    while s.transport.tone_playing() {
        assert!(start.elapsed().as_secs() < 10, "the tone ends");
        std::thread::sleep(std::time::Duration::from_millis(20));
        polls.push(s.transport.take_output_peak());
    }
    polls.push(s.transport.take_output_peak());
    // Then the right, one side at a time.
    let within = |a: f32| (20.0 * (a / level).log10()).abs() < 0.2;
    assert!(polls.iter().any(|p| within(p[1])), "the right sounds");
    assert!(
        polls.iter().all(|p| p[0] < 1e-3 || p[1] < 1e-3),
        "{polls:?}"
    );
    // Over: the meters go.
    for _ in 0..200 {
        s.rig_tick();
    }
    assert!(!s.rig_tone);
    assert_eq!(s.rig_out, [0.0; 2]);
    let scene = s.settings_scene(&g);
    assert!(!scene.has_text("L") && !scene.has_text("R"));
}

#[test]
fn a_test_tone_that_cannot_start_is_said_in_the_settings_and_they_stay_open() {
    use crate::settings::{Do, Section};
    let (mut s, g) = speakers_and_interface();
    s.open_settings(Section::Audio);
    // The output cannot be had: the tone cannot start. The bar under the modal would not
    // be seen, so the settings say it in a toast instead.
    s.transport.mode = xirius_session::output::OutputMode::None;
    let p = crate::audioset_shots::item(&s, &g, Do::Tone);
    assert!(s.settings_press(&g, p.0, p.1));
    assert!(s.settings.is_some(), "the settings stay open");
    assert_eq!(s.error, None, "nothing on the bar under the modal");
    let scene = s.settings_scene(&g);
    assert!(
        scene.has_text("オーディオ出力がありません"),
        "the reason is said in the settings"
    );
}

#[test]
fn the_settings_audio_page_has_no_test_tone_without_a_device() {
    use crate::settings::{Do, Section};
    let (mut s, g) = with_devices(&[]);
    s.open_settings(Section::Audio);
    assert!(s.settings_scene(&g).find(&Do::Tone).is_none());
}

/// The lines at the foot have no lanes to sit under on the new song's screen: they are in the
/// middle of the body, at the distance from its bottom they keep over the lanes.
#[test]
fn the_lines_at_the_foot_are_in_the_middle_of_the_body() {
    let (mut s, g) = empty();
    let foot = s.foot(&g);
    assert_eq!(foot, Rect::new(0.0, 0.0, g.size.0, g.size.1));
    let middle = |r: Rect| (r.x + r.w / 2.0 - g.size.0 / 2.0).abs() < 0.5;
    s.error = Some("だめでした".into());
    let notice = crate::notice::place(&s, &g, "片付けました", None);
    assert!(middle(notice), "{notice:?}");
    assert!(
        notice.bottom() <= g.size.1 - 36.0,
        "above the strip: {notice:?}"
    );
    s.warn("「drumloop」を伸縮できません".into(), None);
    let toast = crate::toast::layout(&s, &g).expect("shown").rect;
    assert!(middle(toast), "{toast:?}");
    assert!(toast.bottom() <= g.size.1 - 36.0, "{toast:?}");
    s.live_midi.assign.learning = Some(xirius_prefs::midimap::Target::Volume("1".into()));
    let (pill, _) = s.midi_pill(&g).expect("learning");
    assert!(middle(pill), "{pill:?}");
    assert!(pill.bottom() <= g.size.1 - 36.0, "{pill:?}");
}

/// Over the lanes, the foot is theirs, above the drawer.
#[test]
fn over_the_lanes_the_foot_is_theirs() {
    let s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    let foot = s.foot(&g);
    assert_eq!(
        (foot.x, foot.w, foot.bottom()),
        (g.lanes.x, g.lanes.w, g.drawer.y)
    );
}
