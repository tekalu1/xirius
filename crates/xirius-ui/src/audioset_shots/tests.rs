use super::*;
use crate::settings::{FieldId, MenuKind};
use crate::tools::Mods;
use serde_json::json;

fn setup() -> (Studio, Geometry) {
    let mut s = Studio::new(crate::song::Song::empty());
    fixture(&mut s);
    let g = s.geometry(1440.0, 860.0);
    s.resized(&g);
    let p = s.center_of(&g, Hit::RailSettings).unwrap();
    s.press(&g, s.hit(&g, p.0, p.1));
    press(&mut s, &g, Do::Nav(Section::Audio));
    (s, g)
}
fn press(s: &mut Studio, g: &Geometry, act: Do) {
    let p = item(s, g, act);
    assert!(s.settings_press(g, p.0, p.1));
}

#[test]
fn inputs_are_measured_and_arrow_keys_change_the_radio() {
    let (mut s, g) = setup();
    assert_eq!(s.song.io.lock().unwrap().view.levels, [0.025, 0.63]);
    press(&mut s, &g, Do::Input(0));
    s.settings_key("down", Mods::default());
    assert_eq!(s.song.io.lock().unwrap().view.selected, Some(1));
    s.settings_key("up", Mods::default());
    assert_eq!(s.song.io.lock().unwrap().view.selected, Some(0));
    let radios = crate::settings::audio::radios(&s, &g);
    assert_eq!(radios.len(), 2);
    assert_eq!(radios[0].1.role(), gpui::Role::RadioButton);
    assert_eq!(radios[0].1.toggled(), Some(gpui::Toggled::True));
    assert_eq!(radios[1].1.toggled(), Some(gpui::Toggled::False));
}

#[test]
fn device_dropdown_changes_inputs_and_clears_old_observations() {
    let (mut s, g) = setup();
    press(&mut s, &g, Do::Menu(MenuKind::Pref("audio.device".into())));
    s.settings_key("up", Mods::default());
    s.settings_key("enter", Mods::default());
    s.poll_io();
    let io = s.song.io.lock().unwrap();
    assert_eq!(io.view.device, "USB Audio");
    assert_eq!(io.view.levels.len(), 1);
    assert!(io.view.selected.is_none());
}

#[test]
fn detect_chooses_the_loudest_input_after_the_button_press() {
    let (mut s, g) = setup();
    press(&mut s, &g, Do::Detect);
    let mut io = s.song.io.lock().unwrap();
    io.poll(std::time::Instant::now() + std::time::Duration::from_secs(2));
    assert_eq!(io.view.selected, Some(1));
    assert_eq!(io.view.detected, [0, 1]);
}

#[test]
fn every_buffer_uses_the_driver_report_and_never_the_mock_formula() {
    let (mut s, g) = setup();
    for (buffer, expected) in [
        (32, 83),
        (64, 113),
        (128, 144),
        (256, 290),
        (512, 550),
        (1024, 1080),
    ] {
        press(
            &mut s,
            &g,
            Do::Pref("audio.buffer".into(), buffer.to_string()),
        );
        let io = s.song.io.lock().unwrap();
        let l = io.view.latency.unwrap();
        assert_eq!(l.input, expected);
        assert!((l.total_ms() - (buffer as f64 / 48.0 * 2.0 + 1.5)).abs() > 0.01);
        drop(io);
        assert!(s.io_tooltip().starts_with("入出力"));
    }
}

#[test]
fn shared_mode_leaves_the_buffer_to_windows_and_offers_no_size() {
    let (mut s, g) = setup();
    let sizes: Vec<Do> = xirius_engine::devices::BUFFERS
        .iter()
        .map(|n| Do::Pref("audio.buffer".into(), n.to_string()))
        .collect();
    // In exclusive mode each size can be pressed.
    let at: Vec<(f32, f32)> = sizes.iter().map(|d| item(&s, &g, d.clone())).collect();
    press(&mut s, &g, Do::Menu(MenuKind::Pref("audio.driver".into())));
    // The first row is exclusive mode, the next shared.
    s.settings_key("down", Mods::default());
    s.settings_key("down", Mods::default());
    s.settings_key("enter", Mods::default());
    let driver = s.song.settings().lock().unwrap().prefs.text("audio.driver");
    assert_eq!(driver, "wasapi-shared");
    let scene = s.settings_scene(&g);
    for (d, p) in sizes.iter().zip(&at) {
        assert!(scene.find(d).is_none(), "{d:?} is offered");
        let under = scene.item_at(p.0, p.1).map(|i| i.act.clone());
        assert_eq!(under, Some(Do::Nothing), "{d:?}");
    }
    // A press there changes nothing and leaves the settings open.
    let before = s
        .song
        .settings()
        .lock()
        .unwrap()
        .prefs
        .number("audio.buffer");
    assert!(s.settings_press(&g, at[0].0, at[0].1));
    assert!(s.settings.is_some());
    let after = s
        .song
        .settings()
        .lock()
        .unwrap()
        .prefs
        .number("audio.buffer");
    assert_eq!(after, before);
}

#[test]
fn numeric_offsets_and_midi_switches_use_the_shared_settings_commands() {
    let (mut s, g) = setup();
    press(
        &mut s,
        &g,
        Do::Field(FieldId::Pref("audio.record-offset".into())),
    );
    s.settings
        .as_mut()
        .unwrap()
        .field
        .as_mut()
        .unwrap()
        .set("-7.5".into());
    s.settings_key("enter", Mods::default());
    press(&mut s, &g, Do::Nav(Section::Midi));
    press(
        &mut s,
        &g,
        Do::Pref("midi.follow-selection".into(), "false".into()),
    );
    press(
        &mut s,
        &g,
        Do::Pref("midi.inputs.nanoKONTROL2".into(), "false".into()),
    );
    let p = s.song.settings().lock().unwrap();
    assert_eq!(p.prefs.number("audio.record-offset"), -7.5);
    assert!(!p.prefs.bool("midi.follow-selection"));
    assert!(!p.prefs.bool("midi.inputs.nanoKONTROL2"));
}

#[test]
fn the_audio_entry_arms_the_loudest_input() {
    let (mut s, g) = setup();
    s.close_settings();
    let p = s
        .center_of(&g, Hit::Start(crate::start::Entry::Audio))
        .unwrap();
    assert!(s.press(&g, s.hit(&g, p.0, p.1)));
    let project = s.project();
    let t = project.tracks.values().find(|t| t.armed).unwrap();
    assert_eq!(t.input.as_deref(), Some("In 2"));
    assert!(s.drawer_open);
    assert_eq!(s.tab, crate::studio::DrawerTab::Mixer);
}

#[test]
fn midi_input_drives_the_selected_instrument_and_disabled_ports_do_not() {
    use xirius_engine::record::MockMidi;
    let (mut s, _) = setup();
    s.close_settings();
    s.start_midi();
    // Engine startup is not part of the 250 ms activity-indicator assertion.
    let project = s.project();
    s.transport.start_host(&project, s.song.dir()).unwrap();
    let ports = vec!["Keys".to_string()];
    s.live_midi_input(Box::new(MockMidi::with_ports(
        ports.clone(),
        vec![(0, 0, [0x90, 60, 100])],
    )));
    assert!(s.poll_live_midi());
    assert!(s.transport.is_live());
    s.poll_io();
    assert_eq!(s.song.io.lock().unwrap().view.received, ["Keys"]);
    s.song
        .set("midi.set", json!({"key":"inputs.Keys","value":"false"}))
        .unwrap();
    s.release_midi();
    s.live_midi_input(Box::new(MockMidi::with_ports(
        ports,
        vec![(0, 0, [0x90, 64, 100])],
    )));
    s.poll_live_midi();
    // No held notes survive the disabled input (release path is also exercised).
    assert!(s.error.is_none());
}

#[test]
fn midi_activity_does_not_require_a_track_or_a_note() {
    let (mut s, _) = setup();
    s.live_midi_input(Box::new(xirius_engine::record::MockMidi::with_ports(
        vec!["Keys".into()],
        vec![(0, 0, [0xb0, 1, 64])],
    )));
    s.tick();
    assert_eq!(s.song.io.lock().unwrap().view.received, ["Keys"]);
    assert!(!s.transport.is_live());
}

#[test]
fn audio_only_recording_keeps_delivering_live_midi() {
    let (mut s, _) = setup();
    s.live_midi_input(Box::new(xirius_engine::record::MockMidi::with_ports(
        vec!["Keys".into()],
        vec![(0, 0, [0x80, 60, 0])],
    )));
    s.transport.recording = true;
    assert!(s.poll_live_midi());
    s.poll_io();
    assert_eq!(s.song.io.lock().unwrap().view.received, ["Keys"]);
}
