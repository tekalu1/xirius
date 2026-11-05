//! A copy or a paste of inserts reads what the plug-ins hold now (MIX-045), with the test
//! VST3 in a host process and in this one, its patch changed without a word.

use serde_json::json;
use xirius_core::clipboard::Clipboard;
use xirius_core::id::PluginId;
use xirius_session::host::PULL_WAIT;

use super::*;
use crate::held_input::{Held, HeldInput};
use crate::mixer::MixerHit;
use crate::mixer::flow::Area;
use crate::studio::Geometry;
use crate::tools::Mods;

/// The fixture saves its "Latency" as a delay of 256 × the value, in 4 little-endian bytes.
fn delay(v: f64) -> Vec<u8> {
    ((v * 256.0) as u32).to_le_bytes().to_vec()
}

struct Song {
    s: Studio,
    g: Geometry,
    vocal: TrackId,
    guitar: TrackId,
    /// The fixture on Vocal (0.75) and on Guitar (0.25).
    from: PluginId,
    to: PluginId,
}

/// The fixture on Vocal and Guitar, isolated as `mode`, playing silently.
fn song(mode: &str) -> Song {
    let mut s = Studio::sample();
    crate::plugin_reload_shots::prepare(&mut s, 0);
    let from = crate::plugin_reload_shots::id(&s);
    let p = s.project();
    let vocal = p.tracks.values().find(|t| t.name == "Vocal").unwrap().id;
    let guitar = p.tracks.values().find(|t| t.name == "Guitar").unwrap().id;
    let fixture = p.tracks[&vocal].inserts[0].clone();
    assert!(s.run_command(
        "plugin.add",
        json!({
            "track": guitar.to_string(), "name": fixture.name, "kind": "effect",
            "format": "vst3", "uid": fixture.uid
        })
    ));
    let to = s.project().tracks[&guitar].inserts[0].id;
    assert!(s.run_command(
        "plugin.isolation.set",
        json!({ "ids": [from.to_string(), to.to_string()], "mode": mode })
    ));
    assert!(s.run_command(
        "plugin.param.set",
        json!({ "id": to.to_string(), "param": "1", "value": 0.25 })
    ));
    let g = s.geometry(1600.0, 1000.0);
    s.resized(&g);
    assert!(s.toggle_play());
    let loaded = |s: &Studio| {
        let Some(host) = s.transport.host() else {
            return false;
        };
        let plugins = host.plugins();
        let reports = plugins.reports();
        [from, to].iter().all(|id| {
            plugins.local(*id).is_some()
                || reports.iter().any(|r| {
                    r.id == *id
                        && r.status.latency > xirius_engine::server::DEFAULT_LATENCY
                        && !r.status.crashed
                })
        })
    };
    crate::shot::run_for(&mut s, 15.0, loaded);
    assert!(loaded(&s), "{:?}", s.error);
    Song {
        s,
        g,
        vocal,
        guitar,
        from,
        to,
    }
}

/// The plug-in's "Latency" turned in its own window: the song hears nothing of it.
fn turn_silently(s: &mut Studio, id: PluginId, v: f64) {
    let host = s.transport.host().unwrap();
    if let Some(link) = host.plugins().link(id) {
        host.plugins()
            .server_of(id)
            .unwrap()
            .set_param(&link, "Latency", v);
    } else {
        let (plugin, _) = host.plugins().local(id).expect("in process");
        assert!(plugin.set_param(1, v));
    }
    crate::shot::run_for(s, 0.5, |_| false);
}

/// The insert's value of "Latency" and the delay its state says, in the song.
fn held(s: &Studio, t: TrackId) -> (f64, Option<Vec<u8>>) {
    let p = s.project();
    let x = &p.tracks[&t].inserts[0];
    let state = x
        .state
        .as_ref()
        .map(|b| xirius_plugins::decode_state(b).unwrap().component);
    (x.params["1"], state)
}

fn drag(s: &mut Studio, g: &Geometry, from: TrackId, to: TrackId) {
    let r = s
        .mixer_rect(g, MixerHit::Heading(from, Area::Insert))
        .expect("the heading is on screen");
    let (x, y) = (r.x + 12.0, r.y + r.h / 2.0);
    s.insert_drag_down(g, x, y);
    let a = s
        .mixer_strips(g)
        .into_iter()
        .find(|x| x.track == to)
        .unwrap()
        .inserts;
    let at = (a.x + a.w / 2.0, a.y + a.h / 2.0);
    assert!(s.insert_drag_move(at.0, at.1, false));
    assert!(s.insert_drag_up(g, at.0, at.1, false));
}

#[test]
fn a_drag_carries_what_the_plugin_holds_now_and_an_undo_brings_back_what_it_replaced() {
    let Song {
        mut s,
        g,
        vocal,
        guitar,
        from,
        to,
    } = song("dedicated");
    turn_silently(&mut s, from, 0.5);
    turn_silently(&mut s, to, 0.625);
    assert_eq!(held(&s, vocal), (0.75, None), "nothing said, nothing kept");
    assert_eq!(held(&s, guitar), (0.25, None));
    let before = s.song.steps();

    drag(&mut s, &g, vocal, guitar);
    // the window goes on while the plug-ins answer; the keys wait with the paste
    assert!(s.inserts_wait.is_some());
    assert!(s.input_blocked());
    assert_eq!(s.project().tracks[&guitar].inserts[0].id, to);
    // what is typed meanwhile waits, as before a save's veil, and comes in once it ran
    let mut typed = HeldInput::default();
    assert!(typed.hold(s.input_hold(), Held::Text(None, "z".into())));
    assert!(typed.release(s.input_hold()).is_empty(), "it waits");
    crate::shot::run_for(&mut s, 5.0, |s| s.inserts_wait.is_none());
    assert!(s.inserts_wait.is_none(), "the paste ran");
    assert_eq!(s.input_hold(), None);
    let typed = typed.release(s.input_hold());
    assert!(matches!(typed.as_slice(), [Held::Text(None, z)] if z == "z"));

    let copied = &s.project().tracks[&guitar].inserts[0];
    assert_ne!(copied.id, to);
    assert_ne!(copied.id, from);
    assert_eq!(held(&s, guitar), (0.5, Some(delay(0.5))), "the state now");
    assert_eq!(held(&s, vocal), (0.5, Some(delay(0.5))));
    assert_eq!(s.notice_text(), Some("インサートを貼り付けました"));
    // what the plug-ins answered is a step, and the paste one more
    assert_eq!(s.song.steps(), before + 2);

    // One undo: the replaced plug-in comes back as it was a moment before.
    assert!(s.undo(&g));
    assert_eq!(s.project().tracks[&guitar].inserts[0].id, to);
    assert_eq!(held(&s, guitar), (0.625, Some(delay(0.625))));
    s.transport.pause();
}

#[test]
fn ctrl_c_on_the_heading_copies_what_the_plugin_holds_now() {
    let Song {
        mut s,
        g,
        vocal,
        from,
        ..
    } = song("dedicated");
    turn_silently(&mut s, from, 0.5);
    let r = s
        .mixer_rect(&g, MixerHit::Heading(vocal, Area::Insert))
        .unwrap();
    let hit = s.mixer_hit(&g, r.x + 12.0, r.y + r.h / 2.0).unwrap();
    s.mixer_press(&g, hit);
    let ctrl = Mods {
        ctrl: true,
        ..Mods::NONE
    };
    assert!(s.mapped_key(&g, "c", None, ctrl));
    crate::shot::run_for(&mut s, 5.0, |s| s.inserts_wait.is_none());
    let Some(Clipboard::Inserts(c)) = &s.song.session().state.clipboard else {
        panic!("the inserts are copied");
    };
    let state = c.plugins[0].state.as_ref().expect("with a state");
    assert_eq!(
        xirius_plugins::decode_state(state).unwrap().component,
        delay(0.5)
    );
    s.transport.pause();
}

#[test]
fn a_plugin_that_does_not_answer_in_time_is_copied_as_the_song_last_had_it() {
    let Song {
        mut s,
        g,
        vocal,
        guitar,
        from,
        ..
    } = song("inprocess");
    turn_silently(&mut s, from, 0.5);
    drag(&mut s, &g, vocal, guitar);
    // an in-process capture is not ready before it has been given its time
    let wait = s.inserts_wait.as_mut().expect("waiting");
    wait.since -= PULL_WAIT;
    assert!(s.poll_inserts_wait());
    assert!(s.inserts_wait.is_none(), "not waited for any longer");
    assert_eq!(held(&s, guitar), (0.75, None), "the song's last state");
    assert_eq!(s.notice_text(), Some("インサートを貼り付けました"));
    s.transport.pause();
}

#[test]
fn a_plugin_whose_host_went_down_is_copied_as_the_song_last_had_it() {
    let Song {
        mut s,
        g,
        vocal,
        guitar,
        from,
        ..
    } = song("dedicated");
    let host = s.transport.host().unwrap();
    let link = host.plugins().link(from).unwrap();
    host.plugins()
        .server_of(from)
        .unwrap()
        .fault(&link, xirius_engine::wire::Fault::Crash, 1);
    crate::shot::run_for(&mut s, 15.0, |s| s.health.crashed(from));
    s.poll_plugins_now();
    assert!(s.health.crashed(from));
    drag(&mut s, &g, vocal, guitar);
    crate::shot::run_for(&mut s, 5.0, |s| s.inserts_wait.is_none());
    assert!(s.inserts_wait.is_none());
    assert_eq!(held(&s, guitar), (0.75, None));
    s.transport.pause();
}

#[test]
fn built_in_effects_alone_are_copied_at_once() {
    let mut s = Studio::sample();
    s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
    s.drawer_open = true;
    s.tab = crate::studio::DrawerTab::Mixer;
    let g = s.geometry(1600.0, 1000.0);
    s.resized(&g);
    let p = s.project();
    let vocal = p.tracks.values().find(|t| t.name == "Vocal").unwrap().id;
    let guitar = p.tracks.values().find(|t| t.name == "Guitar").unwrap().id;
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": vocal.to_string(), "name": "EQ", "kind": "effect", "format": "builtin", "uid": "EQ" }),
    ));
    assert!(s.toggle_play());
    crate::shot::run_for(&mut s, 5.0, |s| s.transport.host().is_some());
    assert!(s.transport.host().is_some());
    drag(&mut s, &g, vocal, guitar);
    assert!(s.inserts_wait.is_none(), "nothing to ask");
    assert_eq!(s.project().tracks[&guitar].inserts[0].name, "EQ");
    s.transport.pause();
}
