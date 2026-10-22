//! KEY-012: the rest of the Studio Pro key table, pressed as the window presses its keys, and the
//! screen's commands asked for over the session (an AI's) run by the window on its next frame.

use serde_json::json;
use xirius_core::id::{ClipId, TrackId};
use xirius_core::time::Tick;

use crate::browser::RailTab;
use crate::context::key_more::{Place, WindowAsk};
use crate::studio::{DrawerTab, Geometry, Studio};
use crate::tools::Mods;
use crate::tree::RowKind;

const W: f32 = 1400.0;
const H: f32 = 900.0;

fn ctrl_alt() -> Mods {
    Mods {
        ctrl: true,
        alt: true,
        ..Mods::NONE
    }
}

fn press(s: &mut Studio, g: &Geometry, k: &str, mods: Mods) -> bool {
    let character = (k.chars().count() == 1).then_some(k);
    s.mapped_key(g, k, character, mods)
}

fn track(s: &Studio, name: &str) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == name)
        .unwrap()
        .id
}

fn clips(s: &Studio, name: &str) -> Vec<ClipId> {
    s.project().tracks[&track(s, name)]
        .clips
        .iter()
        .map(|c| c.id)
        .collect()
}

/// The arrangement has the keys, the clips of `picked` selected.
fn arrange_with(s: &mut Studio, g: &Geometry, picked: &[(TrackId, ClipId)]) {
    s.arrange.focus = true;
    s.arrange.clips = picked.iter().copied().collect();
    s.selected_clip = picked.first().copied();
    assert_eq!(s.input_context(g), "arrange");
}

fn picked(s: &Studio) -> Vec<ClipId> {
    s.arrange.clips.iter().map(|(_, c)| *c).collect()
}

fn form(s: &Studio) -> Option<&'static str> {
    s.palette.as_ref()?.form.as_ref().map(|f| f.command)
}

fn bar(s: &Studio, n: u32) -> Tick {
    s.project().time_map().bar_start(n)
}

#[test]
fn the_global_solo_and_mute_keys_turn_every_track_off() {
    let mut s = Studio::sample();
    let g = s.geometry(W, H);
    let (drums, keys) = (track(&s, "Drums"), track(&s, "Keys"));
    s.song
        .run(
            "track.solo",
            json!({"ids": [drums.to_string(), keys.to_string()], "on": true}),
        )
        .unwrap();
    s.song
        .run(
            "track.mute",
            json!({"ids": [drums.to_string()], "on": true}),
        )
        .unwrap();
    // From the roll as from anywhere: the keys are the window's.
    assert_eq!(s.input_context(&g), "roll");
    assert!(press(&mut s, &g, "s", Mods::ctrl_shift()));
    assert!(s.project().tracks.values().all(|t| !t.solo));
    assert!(s.project().tracks[&drums].mute);
    assert!(press(&mut s, &g, "m", Mods::ctrl_shift()));
    assert!(s.project().tracks.values().all(|t| !t.mute));
}

#[test]
fn the_clips_of_the_selected_tracks_leave_the_selection() {
    let mut s = Studio::sample();
    let g = s.geometry(W, H);
    let (drums, keys) = (track(&s, "Drums"), track(&s, "Keys"));
    let d = clips(&s, "Drums");
    let k = clips(&s, "Keys")[0];
    arrange_with(&mut s, &g, &[(drums, d[0]), (drums, d[1]), (keys, k)]);
    s.arrange.tracks = vec![RowKind::Track(drums)];
    assert!(press(&mut s, &g, "d", Mods::ctrl_shift()));
    assert_eq!(picked(&s), vec![k]);
}

#[test]
fn alt_shift_home_and_end_grow_the_selection_to_the_songs_ends() {
    let mut s = Studio::sample();
    let g = s.geometry(W, H);
    let drums = track(&s, "Drums");
    let d = clips(&s, "Drums");
    arrange_with(&mut s, &g, &[(drums, d[1])]);
    assert!(press(&mut s, &g, "home", Mods::alt_shift()));
    let mut want = d.clone();
    want.sort();
    let mut got = picked(&s);
    got.sort();
    assert_eq!(got, want);
    arrange_with(&mut s, &g, &[(drums, d[0])]);
    assert!(press(&mut s, &g, "end", Mods::alt_shift()));
    let mut got = picked(&s);
    got.sort();
    assert_eq!(got, want);
}

#[test]
fn alt_h_and_alt_g_play_the_next_and_the_previous_take() {
    let mut s = Studio::sample();
    let g = s.geometry(W, H);
    let keys = track(&s, "Keys");
    let first = s.project().tracks[&keys].clips[0].clone();
    let mut takes = vec![first.id];
    for _ in 0..2 {
        let made = s
            .song
            .run(
                "clip.add.midi",
                json!({"track": keys.to_string(), "at": first.start, "length": first.length}),
            )
            .unwrap();
        takes.push(made["id"].as_str().unwrap().parse().unwrap());
    }
    s.song
        .run(
            "clip.mute",
            json!({"ids": [takes[1].to_string(), takes[2].to_string()], "on": true}),
        )
        .unwrap();
    let plays = |s: &Studio| -> Vec<bool> {
        takes
            .iter()
            .map(|c| !s.project().clip(*c).unwrap().1.muted)
            .collect()
    };
    assert_eq!(plays(&s), [true, false, false]);
    arrange_with(&mut s, &g, &[(keys, takes[0])]);
    assert!(press(&mut s, &g, "h", Mods::alt()));
    assert_eq!(plays(&s), [false, true, false]);
    // From the take that plays, whichever of the stack is selected.
    arrange_with(&mut s, &g, &[(keys, takes[0])]);
    assert!(press(&mut s, &g, "g", Mods::alt()));
    assert_eq!(plays(&s), [true, false, false]);
    // Before the first, the last.
    arrange_with(&mut s, &g, &[(keys, takes[0])]);
    assert!(press(&mut s, &g, "g", Mods::alt()));
    assert_eq!(plays(&s), [false, false, true]);
    // With no clip selected there is no take to step from.
    arrange_with(&mut s, &g, &[]);
    s.selected_row = None;
    assert!(!press(&mut s, &g, "h", Mods::alt()));
}

#[test]
fn page_down_and_page_up_scroll_the_view_in_front_by_its_height() {
    let mut s = Studio::sample();
    // Low, so neither the roll nor the arrangement shows all of itself.
    let g = s.geometry(W, 560.0);
    assert_eq!(s.input_context(&g), "roll");
    let top = s.roll.scroll.1;
    assert!(press(&mut s, &g, "pagedown", Mods::NONE));
    assert!(s.roll.scroll.1 > top, "{} {top}", s.roll.scroll.1);
    assert!(press(&mut s, &g, "pageup", Mods::NONE));
    assert_eq!(s.roll.scroll.1, top);

    let roll = s.roll.scroll;
    arrange_with(&mut s, &g, &[]);
    s.scroll.1 = 0.0;
    assert!(press(&mut s, &g, "pagedown", Mods::NONE));
    assert!(s.scroll.1 > 0.0);
    assert!(press(&mut s, &g, "pageup", Mods::NONE));
    assert_eq!(s.scroll.1, 0.0);
    // At the top, there is no page above.
    assert!(!press(&mut s, &g, "pageup", Mods::NONE));
    assert_eq!(s.roll.scroll, roll, "the roll stays where it was");
}

#[test]
fn tab_and_shift_tab_go_round_the_parts_that_are_shown() {
    let mut s = Studio::sample();
    s.browser.open = true;
    s.drawer_open = true;
    s.arrange.focus = true;
    let g = s.geometry(W, H);
    assert_eq!(s.place(), Place::Arrange);
    assert!(press(&mut s, &g, "tab", Mods::NONE));
    assert_eq!(s.place(), Place::Dock);
    assert!(press(&mut s, &g, "tab", Mods::NONE));
    assert_eq!(s.place(), Place::Browser);
    assert_eq!(s.input_context(&g), "browser");
    assert!(press(&mut s, &g, "tab", Mods::NONE));
    assert_eq!(s.place(), Place::Arrange);
    assert!(press(&mut s, &g, "tab", Mods::shift()));
    assert_eq!(s.place(), Place::Browser);
    assert!(press(&mut s, &g, "tab", Mods::shift()));
    assert_eq!(s.place(), Place::Dock);
}

#[test]
fn shift_f_and_ctrl_q_ask_the_window_for_full_screen_and_to_quit() {
    let mut s = Studio::sample();
    let g = s.geometry(W, H);
    assert!(press(&mut s, &g, "f", Mods::shift()));
    assert_eq!(s.window_ask.take(), Some(WindowAsk::Fullscreen));
    for (k, mods) in [
        ("q", Mods::ctrl()),
        ("w", Mods::ctrl()),
        ("f4", Mods::ctrl()),
    ] {
        assert!(press(&mut s, &g, k, mods), "{k}");
        assert_eq!(s.window_ask.take(), Some(WindowAsk::Quit), "{k}");
    }
}

#[test]
fn shift_insert_asks_for_the_name_of_a_marker_at_the_playhead() {
    let mut s = Studio::sample();
    let g = s.geometry(W, H);
    arrange_with(&mut s, &g, &[]);
    assert!(press(&mut s, &g, "insert", Mods::shift()));
    assert_eq!(form(&s), Some("marker.add"));
    let seed = &s.palette.as_ref().unwrap().form.as_ref().unwrap().seed;
    assert_eq!(seed["at"], json!(s.playhead));
    s.palette = None;
    assert!(press(&mut s, &g, "y", Mods::shift()));
    assert_eq!(form(&s), Some("marker.add"));
}

#[test]
fn alt_r_makes_the_selected_clips_times_the_range() {
    let mut s = Studio::sample();
    let g = s.geometry(W, H);
    let (drums, keys) = (track(&s, "Drums"), track(&s, "Keys"));
    let d = clips(&s, "Drums")[0];
    let k = clips(&s, "Keys")[0];
    arrange_with(&mut s, &g, &[(drums, d), (keys, k)]);
    assert!(press(&mut s, &g, "r", Mods::alt()));
    // Drums is bars 1–4 and Keys bars 5–8: one range over both.
    assert_eq!(s.arrange.range, Some((bar(&s, 1), bar(&s, 9))));
    assert_eq!(s.arrange.ranges, vec![(bar(&s, 1), bar(&s, 9))]);
    assert!(s.arrange.clips.is_empty());
    // With no clip selected there is nothing to make it from.
    s.arrange.range = None;
    assert!(!press(&mut s, &g, "r", Mods::shift()));
    assert_eq!(s.arrange.range, None);
}

#[test]
fn ctrl_f_opens_the_browser_and_types_in_its_search() {
    let mut s = Studio::sample();
    s.browser.open = false;
    let g = s.geometry(W, H);
    assert!(press(&mut s, &g, "f", Mods::ctrl()));
    assert!(s.browser.open);
    assert!(s.browser.typing);
    assert_eq!(s.input_context(&g), "text");
}

#[test]
fn f4_f6_f7_and_f9_show_the_browsers_tabs_and_shut_the_one_shown() {
    let mut s = Studio::sample();
    s.browser.open = false;
    let g = s.geometry(W, H);
    for (k, tab) in [
        ("f4", RailTab::Inspector),
        ("f6", RailTab::Instruments),
        ("f7", RailTab::Effects),
        ("f9", RailTab::Files),
    ] {
        assert!(press(&mut s, &g, k, Mods::NONE), "{k}");
        assert!(s.browser.open, "{k}");
        assert_eq!(s.browser.tab, tab, "{k}");
    }
    // The tab in front, again: the browser shuts, as its rail's button does.
    assert!(press(&mut s, &g, "f9", Mods::NONE));
    assert!(!s.browser.open);
}

#[test]
fn the_brace_keys_page_the_mixer_by_whole_strips() {
    let mut s = Studio::sample();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    s.selected_row = None;
    // Narrow, so the strips do not all fit.
    let g = s.geometry(700.0, H);
    assert_eq!(s.input_context(&g), "mixer");
    assert!(s.mixer_max_scroll(&g) > 0.0);
    assert_eq!(s.mixer.scroll, 0.0);
    assert!(s.mapped_key(&g, "]", Some("}"), Mods::shift()));
    let one = s.mixer.scroll;
    let w = crate::mixer::STRIP_W + crate::mixer::STRIP_GAP;
    let page = (s.mixer_view(&g).w / w).floor() * w;
    assert!(page >= w);
    assert_eq!(one, page.min(s.mixer_max_scroll(&g)));
    assert!(s.mapped_key(&g, "[", Some("{"), Mods::shift()));
    assert_eq!(s.mixer.scroll, 0.0);
    // At the first bank, there is none before it.
    assert!(!s.mapped_key(&g, "[", Some("{"), Mods::shift()));
}

#[test]
fn the_hash_key_ends_every_note_the_keys_hold() {
    let mut s = Studio::sample();
    let g = s.geometry(W, H);
    let keys = track(&s, "Keys");
    s.pc_keys.down.push(("a".into(), keys, 60, 60));
    assert!(s.mapped_key(&g, "3", Some("#"), Mods::shift()));
    assert!(s.pc_keys.down.is_empty());
}

#[test]
fn caps_lock_turns_the_pc_keyboard_on_and_off() {
    let mut s = Studio::sample();
    let g = s.geometry(W, H);
    assert!(!s.pc_keys.on);
    assert!(press(&mut s, &g, "capslock", Mods::NONE));
    assert!(s.pc_keys.on);
    assert!(press(&mut s, &g, "capslock", Mods::NONE));
    assert!(!s.pc_keys.on);
}

#[test]
fn ctrl_t_asks_for_a_time_and_its_enter_moves_the_playhead() {
    let mut s = Studio::sample();
    let g = s.geometry(W, H);
    assert!(press(&mut s, &g, "t", Mods::ctrl()));
    assert_eq!(form(&s), Some("transport.seek"));
    let shown = s.project().time_map().format(s.playhead);
    let to = s.project().time_map().format(bar(&s, 9));
    let palette = s.palette.as_mut().unwrap();
    assert_eq!(palette.form.as_ref().unwrap().fields[0].value, shown);
    assert_eq!(palette.query, shown);
    palette.query = to;
    assert!(s.palette_form_enter(&g));
    assert!(s.palette.is_none());
    assert_eq!(s.playhead, bar(&s, 9));
}

#[test]
fn ctrl_numpad_dot_asks_for_a_marker_and_its_enter_goes_there() {
    let mut s = Studio::sample();
    let g = s.geometry(W, H);
    let at = bar(&s, 11);
    s.song.run("marker.add", json!({"at": at})).unwrap();
    assert!(press(&mut s, &g, "numpad.", Mods::ctrl()));
    assert_eq!(form(&s), Some("transport.goto"));
    let f = s.palette.as_ref().unwrap().form.as_ref().unwrap();
    assert_eq!(f.fields[0].value, "marker-1");
    assert!(s.palette_form_enter(&g));
    assert_eq!(s.playhead, at);
}

#[test]
fn ctrl_alt_t_and_ctrl_alt_c_ask_which_track_to_show() {
    let mut s = Studio::sample();
    let g = s.geometry(W, H);
    assert!(press(&mut s, &g, "t", ctrl_alt()));
    assert_eq!(form(&s), Some("track.reveal"));
    s.palette = None;
    assert!(press(&mut s, &g, "c", ctrl_alt()));
    assert_eq!(form(&s), Some("track.reveal"));
    let seed = &s.palette.as_ref().unwrap().form.as_ref().unwrap().seed;
    assert_eq!(seed["in"], json!("mixer"));
}

#[test]
fn f11_opens_the_selected_tracks_instrument_and_shuts_it_again() {
    let mut s = Studio::sample();
    let g = s.geometry(W, H);
    let keys = track(&s, "Keys");
    // No plugin on the track: nothing to open.
    assert!(!press(&mut s, &g, "f11", Mods::NONE));
    assert!(s.set_instrument(
        keys,
        &crate::instrument::Pick::built_in(crate::instrument::DEFAULT)
    ));
    let id = crate::instrument::current(&s.project().tracks[&keys])
        .unwrap()
        .id;
    assert!(!s.instruments.is_open(id));
    assert!(press(&mut s, &g, "f11", Mods::NONE));
    assert!(s.instruments.is_open(id));
    assert!(press(&mut s, &g, "f11", Mods::NONE));
    assert!(!s.instruments.is_open(id));
    // No track chosen: nothing to open.
    s.selected_row = None;
    s.selected_clip = None;
    assert!(!press(&mut s, &g, "f11", Mods::NONE));
}

#[test]
fn ctrl_p_makes_an_empty_part_on_the_selected_instrument_track() {
    let mut s = Studio::sample();
    let g = s.geometry(W, H);
    let keys = track(&s, "Keys");
    arrange_with(&mut s, &g, &[]);
    s.selected_row = Some(RowKind::Track(keys));
    s.playhead = bar(&s, 10) + 240;
    let before = clips(&s, "Keys").len();
    assert!(press(&mut s, &g, "p", Mods::ctrl()));
    let p = s.project();
    let made = p.tracks[&keys]
        .clips
        .iter()
        .find(|c| c.start == bar(&s, 10))
        .expect("a part in the playhead's bar");
    assert_eq!(made.length, bar(&s, 11) - bar(&s, 10));
    assert_eq!(clips(&s, "Keys").len(), before + 1);
    assert_eq!(picked(&s), vec![made.id]);
    // Not on an audio track.
    s.selected_row = Some(RowKind::Track(track(&s, "Vocal")));
    s.selected_clip = None;
    assert!(!press(&mut s, &g, "p", Mods::ctrl()));
}

#[test]
fn ctrl_dot_opens_the_songs_settings_and_ctrl_shift_g_toggles_the_snap() {
    let mut s = Studio::sample();
    let g = s.geometry(W, H);
    assert!(press(&mut s, &g, ".", Mods::ctrl()));
    assert_eq!(form(&s), Some("project.settings"));
    s.palette = None;
    let snap = s.song.snap();
    assert!(press(&mut s, &g, "g", Mods::ctrl_shift()));
    assert_eq!(s.song.snap(), !snap);
    assert!(press(&mut s, &g, "n", Mods::NONE));
    assert_eq!(s.song.snap(), snap);
}

#[test]
fn a_screen_command_run_over_the_session_waits_for_the_window() {
    let mut s = Studio::sample();
    let g = s.geometry(W, H);
    s.song.run("view.fullscreen", json!({})).unwrap();
    assert!(s.has_asked());
    assert_eq!(s.window_ask, None);
    // The window's next frame runs it.
    assert!(s.run_asked(&g));
    assert!(!s.has_asked());
    assert_eq!(s.window_ask, Some(WindowAsk::Fullscreen));
    assert!(!s.run_asked(&g));
    // With its arguments: the browser's tab.
    s.song
        .run("browser.tab", json!({"tab": "effects"}))
        .unwrap();
    assert!(s.run_asked(&g));
    assert!(s.browser.open);
    assert_eq!(s.browser.tab, RailTab::Effects);
}

#[test]
fn a_screen_command_asked_while_the_window_runs_them_is_refused() {
    let mut s = Studio::sample();
    s.song.asked().lock().unwrap().busy = true;
    let e = s.song.run("view.fullscreen", json!({})).unwrap_err();
    assert!(
        e.contains("view.fullscreen は画面で実行できませんでした"),
        "{e}"
    );
    assert!(!s.has_asked());
}
