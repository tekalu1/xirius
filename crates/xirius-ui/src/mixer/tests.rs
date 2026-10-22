use std::path::PathBuf;
use std::time::{Duration, Instant};

use xirius_plugins::Entry;
use xirius_session::output::OutputMode;

use super::*;
use crate::popup::Action;

const W: f32 = 1600.0;
const H: f32 = 1000.0;

/// The sample song with the mixer open.
fn studio() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    let g = s.geometry(W, H);
    (s, g)
}

fn track(s: &Studio, name: &str) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == name)
        .map(|t| t.id)
        .unwrap_or_else(|| panic!("no track {name}"))
}

fn bus(s: &mut Studio, name: &str) -> TrackId {
    assert!(s.run_command("track.add.bus", json!({ "name": name, "into": "root" })));
    track(s, name)
}

/// Presses what `hit` names where the screen shows it.
fn press(s: &mut Studio, g: &Geometry, hit: MixerHit) -> bool {
    let (x, y) = s.mixer_center(g, hit).expect("on screen");
    assert_eq!(s.hit(g, x, y), Hit::Mixer(hit));
    s.press(g, Hit::Mixer(hit))
}

fn one_effect() -> Catalog {
    Catalog {
        entries: vec![Entry {
            uid: "0123456789ABCDEF0123456789ABCDEF".into(),
            name: "Comp".into(),
            vendor: "Test".into(),
            kind: PluginKind::Effect,
            format: "vst3".into(),
            category: "Fx|Dynamics".into(),
            version: "1.0".into(),
            path: PathBuf::from("C:/nowhere/Comp.vst3"),
            ara: false,
        }],
        failures: Vec::new(),
    }
}

#[test]
fn strips_run_tracks_then_buses_then_the_master() {
    let (mut s, _) = studio();
    let fx = bus(&mut s, "Fx");
    let p = s.project();
    let order = strip_order(&p);
    assert_eq!(order.last(), Some(&TrackId::MASTER));
    let kinds: Vec<TrackKind> = order.iter().map(|t| p.tracks[t].kind).collect();
    let first_bus = kinds.iter().position(|k| *k == TrackKind::Bus).unwrap();
    assert!(kinds[..first_bus].iter().all(|k| *k != TrackKind::Bus));
    assert_eq!(order[order.len() - 2], fx);
    // The strips on screen follow the same order.
    let g = s.geometry(W, H);
    let shown: Vec<TrackId> = s
        .mixer_strips(&g)
        .iter()
        .map(|x| x.track)
        .filter(|t| !t.is_master())
        .collect();
    assert_eq!(shown[..], order[..shown.len()]);
}

#[test]
fn a_fader_drag_moves_on_the_way_and_is_one_step_on_release() {
    let (mut s, g) = studio();
    let t = track(&s, "Keys");
    let before = s.song().steps();
    let was = s.project().tracks[&t].volume_db;
    let hit = MixerHit::Fader(t);
    let (_, y) = s.mixer_center(&g, hit).unwrap();
    assert!(s.drag_begin(&g, Hit::Mixer(hit), y));
    for dy in [10.0, 20.0, 30.0] {
        assert!(s.drag_move(y + dy, false));
    }
    let shown = s.mixer_value(hit).unwrap();
    assert!(shown < was, "{shown} < {was}");
    // Nothing reached the song yet.
    assert_eq!(s.song().steps(), before);
    assert_eq!(s.project().tracks[&t].volume_db, was);
    assert!(s.drag_end());
    assert_eq!(s.song().steps(), before + 1);
    assert_eq!(s.project().tracks[&t].volume_db, shown);
    // One undo puts it back.
    assert!(s.undo(&g));
    assert_eq!(s.project().tracks[&t].volume_db, was);
}

#[test]
fn a_fader_at_the_floor_is_silence_and_shift_moves_finer() {
    let (mut s, g) = studio();
    let t = track(&s, "Keys");
    let hit = MixerHit::Fader(t);
    let (_, y) = s.mixer_center(&g, hit).unwrap();
    s.drag_begin(&g, Hit::Mixer(hit), y);
    s.drag_move(y + 40.0, true);
    let fine = s.mixer_value(hit).unwrap();
    s.drag_move(y + 40.0, false);
    let coarse = s.mixer_value(hit).unwrap();
    assert!(coarse < fine);
    s.drag_move(y + 2000.0, false);
    assert_eq!(s.mixer_value(hit), Some(MIN_DB));
    s.drag_end();
    assert_eq!(s.project().tracks[&t].volume_db, MIN_DB);
}

#[test]
fn a_pan_drag_snaps_to_the_centre_and_a_drag_back_adds_no_step() {
    let (mut s, g) = studio();
    let t = track(&s, "Keys");
    let hit = MixerHit::Pan(t);
    let (_, y) = s.mixer_center(&g, hit).unwrap();
    let before = s.song().steps();
    s.drag_begin(&g, Hit::Mixer(hit), y);
    s.drag_move(y - 30.0, false);
    assert!(s.mixer_value(hit).unwrap() > 0.0);
    s.drag_move(y - 1.0, false);
    assert_eq!(s.mixer_value(hit), Some(0.0));
    s.drag_end();
    assert_eq!(s.song().steps(), before);
}

#[test]
fn mute_and_solo_on_a_strip_go_through_the_song() {
    let (mut s, g) = studio();
    let t = track(&s, "Keys");
    assert!(press(&mut s, &g, MixerHit::Mute(t)));
    assert!(s.project().tracks[&t].mute);
    assert!(press(&mut s, &g, MixerHit::Solo(t)));
    assert!(s.project().tracks[&t].solo);
    assert_eq!(s.selected_row, Some(RowKind::Track(t)));
}

#[test]
fn the_output_list_leaves_out_what_would_feed_back() {
    let (mut s, _) = studio();
    let a = bus(&mut s, "A");
    let b = bus(&mut s, "B");
    assert!(s.run_command(
        "track.output.set",
        json!({ "ids": [a.to_string()], "to": b.to_string() })
    ));
    let p = s.project();
    let to = |c: Vec<Choice>| c.into_iter().map(|c| c.to).collect::<Vec<_>>();
    // A plays into B, so B cannot play into A, nor send to it.
    let outs = to(output_choices(&p, b));
    assert!(outs.contains(&"master".to_string()));
    assert!(!outs.contains(&a.to_string()), "{outs:?}");
    assert!(!to(send_choices(&p, b)).contains(&a.to_string()));
    // A track can play into either.
    let keys = track(&s, "Keys");
    let outs = to(output_choices(&p, keys));
    assert!(outs.contains(&a.to_string()) && outs.contains(&b.to_string()));
    // The list a press opens is the same one.
    s.drawer_open = true;
    let g = s.geometry(W, H);
    assert!(press(&mut s, &g, MixerHit::Output(b)));
    let popup = s.popup.as_ref().expect("the output list");
    assert!(!to(popup.choices.clone()).contains(&a.to_string()));
}

#[test]
fn an_insert_picked_from_the_scan_lands_on_the_strip_and_bypasses() {
    let (mut s, g) = studio();
    s.mixer.catalog = Some(Arc::new(one_effect()));
    let t = track(&s, "Vocal");
    assert!(press(&mut s, &g, MixerHit::AddInsert(t)));
    let popup = s.popup.as_ref().expect("the insert list");
    assert_eq!(popup.kind, PopupKind::Insert(t));
    assert_eq!(popup.choices.len(), 7);
    let chosen = popup
        .choices
        .iter()
        .find(|c| c.label.starts_with("Comp — Test"))
        .unwrap()
        .to
        .clone();
    // The last row scans again.
    assert_eq!(popup.choices.last().unwrap().to, scan::RESCAN);
    let before = s.song().steps();
    s.popup_action(&g, Action::Pick(chosen));
    assert!(s.popup.is_none());
    assert_eq!(s.song().steps(), before + 1);
    let plugin = s.project().tracks[&t].inserts[0].clone();
    assert_eq!(plugin.uid, "0123456789ABCDEF0123456789ABCDEF");
    assert_eq!(plugin.kind, PluginKind::Effect);
    assert!(press(&mut s, &g, MixerHit::Bypass(t, plugin.id)));
    assert!(s.project().tracks[&t].inserts[0].bypass);
    // Its window needs the plugin on this PC; without it the screen says so.
    assert!(press(&mut s, &g, MixerHit::Open(t, plugin.id)));
    assert!(!s.mixer.is_open(plugin.id));
    assert!(s.error.is_some());
}

#[test]
fn with_no_plugins_scanned_the_insert_list_still_offers_builtins() {
    let (mut s, g) = studio();
    s.mixer.catalog = Some(Arc::new(Catalog::default()));
    let t = track(&s, "Vocal");
    press(&mut s, &g, MixerHit::AddInsert(t));
    let choices = s.popup.as_ref().unwrap().choices.clone();
    assert!(choices[0].to.starts_with("builtin:"));
    // The other row offers the scan.
    assert_eq!(choices.last().unwrap().to, scan::RESCAN);
    let before = s.song().steps();
    s.popup_action(&g, Action::Pick(choices[0].to.clone()));
    assert_eq!(s.song().steps(), before + 1);
    assert!(!s.instruments.open.is_empty());
}

#[test]
fn a_send_is_added_from_its_list_and_turns_pre_fader() {
    let (mut s, _) = studio();
    let fx = bus(&mut s, "Fx");
    let t = track(&s, "Keys");
    let g = s.geometry(W, H);
    assert!(press(&mut s, &g, MixerHit::AddSend(t)));
    let choices = s.popup.as_ref().unwrap().choices.clone();
    assert_eq!(choices.len(), 2);
    assert_eq!(choices[0].to, fx.to_string());
    assert_eq!(choices[1].to, NEW_BUS);
    s.popup_action(&g, Action::Pick(fx.to_string()));
    let send = s.project().tracks[&t].sends[0].clone();
    assert_eq!(send.to, fx);
    assert!(!send.pre);
    assert!(press(&mut s, &g, MixerHit::SendPre(t, send.id)));
    assert!(s.project().tracks[&t].sends[0].pre);
    // Its level drags like a fader.
    let hit = MixerHit::SendLevel(t, send.id);
    let (_, y) = s.mixer_center(&g, hit).unwrap();
    s.drag_begin(&g, Hit::Mixer(hit), y);
    s.drag_move(y + 30.0, false);
    s.drag_end();
    assert!(s.project().tracks[&t].sends[0].level_db < send.level_db);
    // Every bus is taken now; only a new one is left.
    let left = send_choices(&s.project(), t);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].to, NEW_BUS);
}

#[test]
fn dragging_a_muted_send_keeps_the_preview_silent_and_changes_its_saved_level() {
    let (mut s, g) = studio();
    // Undo here must land exactly on the state the test kept; the
    // instrument-input arm that would follow the selection is not
    // what this test checks (REC-024).
    s.rec_set(
        "prefs.set",
        serde_json::json!({"key": "midi.follow-selection", "value": "false"}),
    );
    let fx = bus(&mut s, "Fx");
    let t = track(&s, "Keys");
    s.song
        .run(
            "send.add",
            json!({"ids":[t.to_string()],"to":fx.to_string(),"level":-9}),
        )
        .unwrap();
    let id = s.project().tracks[&t].sends[0].id;
    s.song
        .run("send.mute", json!({"id":id.to_string()}))
        .unwrap();
    let hit = MixerHit::SendLevel(t, id);
    assert_eq!(s.mixer_value(hit), Some(-9.0));
    let before = s.project();
    let (_, y) = s.mixer_center(&g, hit).unwrap();
    assert!(s.drag_begin(&g, Hit::Mixer(hit), y));
    s.drag_move(y + 30.0, false);
    let mut preview = s.project().as_ref().clone();
    apply(&mut preview, hit, s.mixer.drag.unwrap().value);
    assert_eq!(preview.tracks[&t].sends[0].level_db, MIN_DB);
    assert_ne!(preview.tracks[&t].sends[0].muted_level, Some(-9.0));
    s.drag_end();
    assert_eq!(s.project().tracks[&t].sends[0].level_db, MIN_DB);
    assert_ne!(s.project().tracks[&t].sends[0].muted_level, Some(-9.0));
    s.undo(&g);
    assert_eq!(s.project(), before);
}

#[test]
fn a_send_to_a_new_bus_makes_the_bus_and_sends_to_it() {
    let (mut s, g) = studio();
    let t = track(&s, "Keys");
    let before = s.song().steps();
    assert!(press(&mut s, &g, MixerHit::AddSend(t)));
    s.popup_action(&g, Action::Pick(NEW_BUS.into()));
    let p = s.project();
    let send = &p.tracks[&t].sends[0];
    let bus = &p.tracks[&send.to];
    assert_eq!(bus.kind, TrackKind::Bus);
    assert_eq!(bus.name, "バス 1");
    // The bus's strip sits before the master.
    let order = strip_order(&p);
    assert_eq!(order[order.len() - 2], bus.id);
    // One step: one undo takes the bus and the send away together (MIX-025).
    assert_eq!(s.song().steps(), before + 1);
    // A bus is not offered a new bus to send to.
    assert!(send_choices(&p, bus.id).iter().all(|c| c.to != NEW_BUS));
    let bus = bus.id;
    assert!(s.undo(&g));
    let p = s.project();
    assert!(!p.tracks.contains_key(&bus));
    assert!(p.tracks[&t].sends.is_empty());
}

#[test]
fn the_drawer_grows_for_the_mixer_and_scrolls_its_strips() {
    let (mut s, g) = studio();
    s.tab = DrawerTab::Roll;
    let roll = s.geometry(W, H);
    assert!(g.drawer_body.h > roll.drawer_body.h);
    s.tab = DrawerTab::Mixer;
    let narrow = s.geometry(700.0, H);
    let max = s.mixer_max_scroll(&narrow);
    assert!(max > 0.0);
    let (x, y) = (narrow.drawer_body.x + 10.0, narrow.drawer_body.y + 4.0);
    assert!(s.mixer_wheel(&narrow, x, y, 0.0, -10_000.0));
    assert_eq!(s.mixer.scroll, max);
}

#[test]
fn levels_from_the_player_reach_the_strip_meters() {
    let (mut s, _) = studio();
    s.transport.mode = OutputMode::Silent { paced: true };
    assert!(s.toggle_play());
    let t = Instant::now();
    let mut heard = false;
    // Until the meter moves; the minute is only a bound for a busy machine.
    while t.elapsed() < Duration::from_secs(60) {
        s.tick();
        let m = s.mixer.meters.get(Strip::Master);
        if m.level[0] > 0.0 || m.level[1] > 0.0 {
            heard = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    s.stop_transport();
    assert!(heard, "the master meter moved while playing");
}

#[test]
fn a_read_lane_moves_the_fader_and_the_knob_with_the_playhead() {
    let (mut s, _) = studio();
    let t = track(&s, "Bass");
    let id = t.to_string();
    let bar = i64::from(s.project().ppq) * 4;
    for (target, at, v) in [
        ("volume", 4 * bar, -24.0),
        ("volume", 6 * bar, 0.0),
        ("pan", 4 * bar, -0.6),
        ("pan", 8 * bar, 0.6),
    ] {
        assert!(s.run_command(
            "auto.point.add",
            json!({ "track": id, "target": target, "at": at, "value": v })
        ));
    }
    let own = s.project().tracks[&t].volume_db;
    s.seek(4 * bar);
    assert_eq!(s.mixer_value(MixerHit::Fader(t)), Some(-24.0));
    s.seek(6 * bar);
    assert_eq!(s.mixer_value(MixerHit::Fader(t)), Some(0.0));
    let pan = s.mixer_value(MixerHit::Pan(t)).unwrap();
    assert!((pan - 0.0).abs() < 1e-9, "{pan}");
    // Off: the lanes are kept but the strip shows its own value.
    assert!(s.run_command("auto.mode", json!({ "ids": [id], "mode": "off" })));
    assert_eq!(s.mixer_value(MixerHit::Fader(t)), Some(own));
}

#[test]
fn the_arrangement_leaves_presses_on_the_mixer_alone() {
    let (s, g) = studio();
    let t = track(&s, "Keys");
    for hit in [
        MixerHit::Fader(t),
        MixerHit::Pan(t),
        MixerHit::Mute(t),
        MixerHit::Output(t),
        MixerHit::AddInsert(t),
    ] {
        let (x, y) = s.mixer_center(&g, hit).expect("on screen");
        assert_eq!(
            s.arrange_spot(&g, x, y),
            crate::arrange::Spot::None,
            "{hit:?}"
        );
    }
}

#[test]
fn the_input_row_of_an_audio_track_picks_its_channels() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    assert!(press(&mut s, &g, MixerHit::Input(vocal)));
    let popup = s.popup.as_ref().expect("the input list");
    assert_eq!(popup.kind, PopupKind::Input(vocal));
    // Without a device the list has the test tone's two channels, never a
    // microphone's.
    let labels: Vec<_> = popup.choices.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["既定（In 1+2）", "In 1+2", "In 1", "In 2"]);
    assert_eq!(popup.current, "none");
    let before = s.song().steps();
    s.popup_action(&g, Action::Input("In 2".into()));
    assert!(s.popup.is_none());
    assert_eq!(s.song().steps(), before + 1);
    assert_eq!(s.project().tracks[&vocal].input.as_deref(), Some("In 2"));
    assert!(press(&mut s, &g, MixerHit::Input(vocal)));
    assert_eq!(s.popup.as_ref().unwrap().current, "In 2");
    s.popup_action(&g, Action::Input("none".into()));
    assert_eq!(s.project().tracks[&vocal].input, None);
}

#[test]
fn an_input_the_song_names_stays_in_the_list_when_it_is_gone() {
    let names = |c: Vec<Choice>| c.into_iter().map(|c| c.to).collect::<Vec<_>>();
    assert_eq!(
        names(input_choices(vec!["In 1".into()], Some("In 3".into()))),
        ["none", "In 1", "In 3"]
    );
    assert_eq!(
        names(input_choices(vec!["In 1".into()], Some("In 1".into()))),
        ["none", "In 1"]
    );
}

#[test]
fn a_frozen_tracks_instrument_and_inserts_are_locked_and_a_press_offers_to_unfreeze() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    for (name, kind, uid) in [("Sampler", "instrument", "sampler"), ("EQ", "effect", "eq")] {
        assert!(s.run_command(
            "plugin.add",
            json!({ "track": keys.to_string(), "name": name, "kind": kind,
                    "format": "builtin", "uid": uid }),
        ));
    }
    assert!(s.run_command(
        "track.freeze.apply",
        json!({ "tracks": [{ "id": keys.to_string(), "file": "frozen.wav", "start": 0 }] }),
    ));
    let inserts = s.project().tracks[&keys].inserts.clone();
    let eq = inserts.iter().find(|p| p.name == "EQ").unwrap().id;
    let n = s.song().steps();
    for hit in [
        MixerHit::Open(keys, inserts[0].id),
        MixerHit::Open(keys, eq),
        MixerHit::Bypass(keys, eq),
    ] {
        s.toast = None;
        let (x, y) = s.mixer_center(&g, hit).expect("on screen");
        assert_eq!(s.hit(&g, x, y), Hit::Mixer(hit));
        // The window asks the chain first, then the press.
        assert!(s.chain_down(&g, x, y));
        assert!(s.mixer.fx_drag.is_none(), "a locked row is not dragged");
        assert!(s.press(&g, Hit::Mixer(hit)));
        let toast = s.toast_shown().expect("the offer to unfreeze");
        assert_eq!(toast.text, crate::toast::FROZEN);
        assert_eq!(toast.button.as_ref().unwrap().0, "フリーズを解除");
        assert!(s.popup.is_none() && !s.mixer.is_open(eq));
    }
    assert_eq!(s.song().steps(), n);
    assert!(!s.project().tracks[&keys].inserts[1].bypass);
    // There is no row to add one to.
    assert!(s.mixer_center(&g, MixerHit::AddInsert(keys)).is_none());
    // The fader still moves while frozen.
    assert!(s.run_command(
        "track.volume.set",
        json!({ "ids": [keys.to_string()], "db": -3.0 }),
    ));

    s.toast_answer();
    assert!(s.project().tracks[&keys].frozen.is_none());
    assert!(press(&mut s, &g, MixerHit::AddInsert(keys)));
    assert_eq!(s.popup.as_ref().unwrap().kind, PopupKind::Insert(keys));
}

/// UI-183: the plugin windows' captions are the top bar's colours in the theme in use, from the
/// first one a window opens with, and they change with the theme.
#[test]
fn the_plugin_window_caption_is_the_top_bars_in_both_themes() {
    use xirius_design::{Surface, Theme};
    let (mut s, _) = studio();
    assert_eq!(s.caption, None, "nothing is asked before a frame is drawn");
    for (theme, dark) in [
        (Theme::Dark, true),
        (Theme::Light, false),
        (Theme::Dark, true),
    ] {
        s.set_caption_theme(theme);
        let bar = Surface::S0.strong_text().resolve(theme);
        let c = s.caption.expect("set");
        assert_eq!(c.dark, dark);
        assert_eq!(c.fill, bar.surface.to_rgb_u32());
        assert_eq!(c.text, bar.ink.to_rgb_u32());
        assert_eq!(
            s.editor_placement().caption,
            Some(c),
            "a window opening now"
        );
    }
    s.set_caption_theme(Theme::Dark);
    let dark = s.caption.unwrap();
    s.set_caption_theme(Theme::Light);
    let light = s.caption.unwrap();
    assert_ne!(dark.fill, light.fill);
    assert_ne!(dark.text, light.text);
}
