//! The empty song's screen (UI-074), and the recent songs under its entries (UI-261).

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
    // The column (entries, then the list) is centred up and down too.
    let l = s.start_layout(&g);
    let bottom = l.rows.last().unwrap().bottom();
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
    assert_eq!(s.rig().device.as_deref(), Some("Interface A"));
    assert!(
        s.song
            .set("audio.set", json!({"key": "device", "value": "Speakers"}))
            .is_ok()
    );
    s.poll_io();
    assert_eq!(s.rig().device.as_deref(), Some("Speakers"));
    assert!(s.rig().inputs.is_empty());

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
    assert_eq!(s.rig().device.as_deref(), Some("Interface A"));
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
    let row = s.start_layout(&g).rows[0];
    assert!(s.entrance_at(&g, row.x + 40.0, row.y + 10.0).is_none());
    assert!(s.center_of(&g, Hit::Entrance(Part::Sample)).is_none());
}

/// Presses a row of the list where it shows, as a click does.
fn press_row(s: &mut Studio, g: &Geometry, part: Part) -> bool {
    let (x, y) = s.center_of(g, Hit::Entrance(part)).expect("on screen");
    assert_eq!(s.hit(g, x, y), Hit::Entrance(part));
    s.press(g, Hit::Entrance(part))
}

#[test]
fn the_list_under_the_entries_ends_with_the_sample_which_a_press_opens() {
    let (mut s, g) = empty();
    let l = s.start_layout(&g);
    assert_eq!(l.rows.len(), 1, "no songs yet: only the sample");
    let row = l.rows[0];
    let z = zone(&g);
    assert_eq!((row.w, row.h), (PANEL, ROW));
    assert!((row.x + row.w / 2.0 - (z.x + z.w / 2.0)).abs() < 0.5);
    let (audio, _) = s.entry_rects(&g, Entry::Audio);
    assert!(l.heading.y >= audio.bottom() + 20.0, "under the entries");
    assert!(row.y > l.heading.bottom());
    assert_eq!(l.row(Part::Song(0)), None);

    assert!(press_row(&mut s, &g, Part::Sample));
    assert_eq!(s.error, None);
    assert!(s.song.is_sample());
    assert_eq!(s.project().name, crate::song::SAMPLE_NAME);
    assert!(!s.start_shown(), "the sample has tracks");
}

/// A studio that keeps its songs in a folder of the test's own.
fn kept(tag: &str) -> (Studio, Geometry, std::path::PathBuf, tempfile::TempDir) {
    let tmp = tempfile::Builder::new()
        .prefix(&format!("xirius-ui-start-{tag}-"))
        .tempdir()
        .unwrap();
    let root = tmp.path().to_path_buf();
    let mut s = Studio::new(Song::empty());
    s.keep = crate::keep::Keep::at(root.clone());
    s.keep.sync(&s.song.project());
    let g = s.geometry(1440.0, 900.0);
    (s, g, root, tmp)
}

#[test]
fn the_list_shows_the_last_five_songs_newest_first_and_a_press_opens_one() {
    let (mut s, g, root, _tmp) = kept("recent");
    let names = ["a", "b", "c", "d", "e", "f"];
    for name in names {
        assert!(s.run_command("track.add.audio", json!({ "name": name })));
        s.finish_file_request(crate::studio::FileRequest::SaveAs, &root.join(name));
        assert_eq!(s.error, None);
        s.new_song();
    }
    assert!(s.start_shown());
    let listed: Vec<_> = s.start_recent().into_iter().map(|r| r.dir).collect();
    let newest: Vec<_> = ["f", "e", "d", "c", "b"]
        .iter()
        .map(|n| root.join(n))
        .collect();
    assert_eq!(listed, newest, "five, newest first");
    let l = s.start_layout(&g);
    assert_eq!(l.rows.len(), SHOWN + 1, "the sample after the songs");
    assert_eq!(l.row(Part::Sample), l.rows.last().copied());
    assert!(l.row(Part::Song(SHOWN)).is_none());
    for w in l.rows.windows(2) {
        assert_eq!(w[1].y - w[0].y, ROW);
    }

    assert!(press_row(&mut s, &g, Part::Song(1)));
    assert_eq!(s.error, None);
    assert_eq!(s.song.record_dir(), Some(root.join("e")));
    assert!(s.project().tracks.values().any(|t| t.name == "e"));
    assert!(!s.start_shown());
}

#[test]
fn the_panel_of_the_devices_sits_between_the_entries_and_the_list() {
    let (s, g) = speakers_and_interface();
    let l = s.start_layout(&g);
    let (audio, _) = s.entry_rects(&g, Entry::Audio);
    let r = &l.rig;
    let z = zone(&g);
    assert_eq!(r.panel.w, PANEL);
    assert!((r.panel.x + r.panel.w / 2.0 - (z.x + z.w / 2.0)).abs() < 0.5);
    assert!(r.panel.y >= audio.bottom() + 20.0, "under the entries");
    assert!(l.heading.y >= r.panel.bottom() + 20.0, "over the list");
    // A row each: the output, the inputs, MIDI, the plugins, one under the other.
    for w in r.icons.windows(2) {
        assert!(w[1].y > w[0].y);
    }
    for part in [r.device, r.tone, r.meter, r.plugins] {
        assert!(
            r.panel.x < part.x && part.right() <= r.panel.right(),
            "{part:?}"
        );
        assert!(
            r.panel.y < part.y && part.bottom() < r.panel.bottom(),
            "{part:?}"
        );
    }
    assert!(r.device.right() < r.tone.x && r.tone.right() < r.meter.x);
    assert_eq!(r.inputs.len(), 2, "a chip per input");
    assert_eq!(r.midi.len(), 1, "the PC keys, with no MIDI port");

    let rig = s.rig();
    assert_eq!(rig.device.as_deref(), Some("Interface A"));
    let names: Vec<_> = rig.inputs.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["In 1", "In 2"]);
    assert_eq!(rig.midi, vec![(PC_KEYBOARD.to_string(), false)]);
    assert!(!rig.tone && rig.out == [0.0; 2]);
}

#[test]
fn the_panel_shows_the_rate_the_latency_the_midi_light_and_the_plugins() {
    let (s, _g) = speakers_and_interface();
    {
        let mut io = s.song.io.lock().unwrap();
        io.view.latency = Some(xirius_engine::devices::Latency {
            rate: 48_000,
            input: 144,
            output: 144,
            conversion: 38,
        });
        io.view.ports = vec!["Keystation 49".into()];
        io.receiving(PC_KEYBOARD, std::time::Instant::now());
        io.poll(std::time::Instant::now());
    }
    let rig = s.rig();
    assert_eq!(rig.info, "48 kHz · 6.8 ms");
    assert_eq!(
        rig.midi,
        vec![
            (PC_KEYBOARD.to_string(), true),
            ("Keystation 49".to_string(), false)
        ]
    );
    assert_eq!(rig.plugins_text(), "プラグイン 0");
    let scanning = Rig {
        scanning: Some((12, 40)),
        ..rig.clone()
    };
    assert_eq!(scanning.plugins_text(), "プラグイン 12 / 40");
    let l = RigLayout::of(&scanning, 0.0, 0.0, PANEL);
    let bar = l.bar.expect("the scan's bar");
    assert!(bar.x > l.plugins.right() && bar.right() <= PANEL - 12.0 + 0.5);
    let failed = Rig {
        found: 31,
        failed: 2,
        ..rig
    };
    let l = RigLayout::of(&failed, 0.0, 0.0, PANEL);
    assert!(l.bar.is_none() && l.bad.is_some());
}

#[test]
fn with_no_device_the_panel_says_so_and_its_list_has_a_row_that_says_so() {
    let (mut s, g) = with_devices(&[]);
    let rig = s.rig();
    assert_eq!(rig.device, None);
    assert!(rig.inputs.is_empty());
    let l = s.start_layout(&g);
    assert_eq!(l.rig.tone.w, 0.0, "no tone without a device");
    assert_eq!(l.row(Part::Tone), None);
    assert!(press_row(&mut s, &g, Part::Device));
    assert_eq!(
        s.popup.as_ref().map(|p| p.kind),
        Some(crate::popup::PopupKind::Device(false))
    );
    assert_eq!(
        menu_rows(&s, &g),
        vec![(rig::NO_DEVICE.to_string(), String::new())]
    );
}

#[test]
fn the_device_button_lists_every_device_and_a_choice_is_used() {
    let (mut s, g) = speakers_and_interface();
    assert!(press_row(&mut s, &g, Part::Device));
    let p = s.popup.as_ref().unwrap();
    assert_eq!(p.kind, crate::popup::PopupKind::Device(false));
    assert_eq!(p.current(), "Interface A");
    assert_eq!(
        menu_rows(&s, &g),
        vec![
            ("Speakers".to_string(), "入力 0 · 出力 2".to_string()),
            ("Interface A".to_string(), "入力 2 · 出力 2".to_string()),
        ]
    );
    let l = s.popup.as_ref().unwrap().layout(g.size);
    assert_eq!(l.cells[2].icon, Some("check"), "the one in use");
    assert_eq!(l.cells[1].icon, Some(crate::popup::BLANK));
    assert!(l.cells[2].on);

    choose(&mut s, &g, "Speakers");
    assert_eq!(s.error, None);
    assert!(s.popup.is_none());
    assert_eq!(s.rig().device.as_deref(), Some("Speakers"));
    assert!(
        audio_tracks(&s).is_empty(),
        "the device button makes no track"
    );
    // The entry is marked: nothing to record from.
    assert!(s.rig().inputs.is_empty());
}

#[test]
fn the_test_tone_sounds_left_then_right_at_minus_eighteen_dbfs() {
    let (mut s, g) = speakers_and_interface();
    assert!(press_row(&mut s, &g, Part::Tone));
    assert_eq!(s.error, None);
    assert!(s.rig().tone, "the button is lit while it sounds");
    let level = 10f32.powf(xirius_engine::tone::LEVEL_DB / 20.0);
    let within = |a: f32| (20.0 * (a / level).log10()).abs() < 0.2;

    // The panel's meter follows the output.
    std::thread::sleep(std::time::Duration::from_millis(150));
    assert!(s.rig_tick(), "moving while it sounds");
    let [l0, r0] = s.rig_out;
    assert!(within(l0), "the left first: {l0}");
    assert_eq!(r0, 0.0);

    // Then the output, polled until the tone is over.
    let start = std::time::Instant::now();
    let mut polls = Vec::new();
    while s.transport.tone_playing() {
        assert!(start.elapsed().as_secs() < 10, "the tone ends");
        std::thread::sleep(std::time::Duration::from_millis(20));
        polls.push(s.transport.take_output_peak());
    }
    polls.push(s.transport.take_output_peak());
    let left = polls.iter().position(|p| within(p[0]));
    let right = polls.iter().position(|p| within(p[1]));
    assert!(left.is_some() || l0 > 0.0);
    let right = right.expect("the right sounds");
    assert!(left.is_none_or(|l| l < right), "the left before the right");
    let top = |i: usize| polls.iter().map(|p| p[i]).fold(0f32, f32::max);
    assert!(within(top(1)), "{}", top(1));
    assert!(top(0) <= level * 1.03);
    // One side at a time.
    assert!(
        polls.iter().all(|p| p[0] < 1e-3 || p[1] < 1e-3),
        "{polls:?}"
    );

    // Over: the button goes out and the meter falls.
    for _ in 0..200 {
        s.rig_tick();
    }
    assert!(!s.rig().tone);
    assert_eq!(s.rig_out, [0.0; 2]);
    assert!(!s.rig_tick());
}

#[test]
fn the_settings_audio_page_has_the_test_tone_too() {
    use crate::settings::{Do, Section};
    let (mut s, g) = speakers_and_interface();
    s.open_settings(Section::Audio);
    let scene = s.settings_scene(&g);
    assert!(scene.has_text("出力"));
    let p = crate::audioset_shots::item(&s, &g, Do::Tone);
    assert!(s.settings_press(&g, p.0, p.1));
    assert_eq!(s.error, None);
    assert!(s.rig_tone, "the button is lit while it sounds");
    assert!(s.settings_scene(&g).has_text(rig::TONE));

    // The page's meter follows the output, left first, at about -18 dBFS.
    let level = 10f32.powf(xirius_engine::tone::LEVEL_DB / 20.0);
    std::thread::sleep(std::time::Duration::from_millis(150));
    assert!(s.rig_tick(), "moving while it sounds on the settings page");
    let [l, r] = s.rig_out;
    assert!((20.0 * (l / level).log10()).abs() < 0.2, "{l}");
    assert_eq!(r, 0.0);
    let start = std::time::Instant::now();
    while s.transport.tone_playing() {
        assert!(start.elapsed().as_secs() < 10, "the tone ends");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
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
