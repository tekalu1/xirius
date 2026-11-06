//! A copy or a paste of inserts reads what the plug-ins hold now (MIX-045), with the test
//! VST3 in a host process and in this one, its patch changed without a word.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::json;
use xirius_core::clipboard::Clipboard;
use xirius_core::id::PluginId;

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

/// The plug-ins' answers to a question for their state, held back on this test's thread until
/// released or dropped: the copy or the paste waits however fast the host answers.
struct Hold {
    open: Arc<AtomicBool>,
    _observe: xirius_core::io_test::Observation,
}

impl Hold {
    fn new() -> Hold {
        let open = Arc::new(AtomicBool::new(false));
        let shut = open.clone();
        let test = std::thread::current().id();
        let _observe = xirius_core::io_test::observe(Arc::new(move |op, _| {
            if op == "capture_answer"
                && std::thread::current().id() == test
                && !shut.load(Ordering::SeqCst)
            {
                return Err(std::io::Error::other("held by the test"));
            }
            Ok(())
        }));
        Hold { open, _observe }
    }

    /// The answers are let in, as they came.
    fn release(&self) {
        self.open.store(true, Ordering::SeqCst);
    }
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

    let hold = Hold::new();
    drag(&mut s, &g, vocal, guitar);
    // the window goes on while the plug-ins answer; the keys wait with the paste
    assert!(s.inserts_wait.is_some());
    assert!(s.input_blocked());
    assert_eq!(s.project().tracks[&guitar].inserts[0].id, to);
    // what is typed meanwhile waits, as before a save's veil, and comes in once it ran
    let mut typed = HeldInput::default();
    assert!(typed.hold(s.input_hold(), Held::Text(None, "z".into())));
    assert!(typed.release(s.input_hold()).is_empty(), "it waits");
    hold.release();
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
    // what the plug-ins answered is in the song with no step of its own: the paste is one
    assert_eq!(s.song.steps(), before + 1);

    // One undo: the replaced plug-in comes back as it was a moment before.
    assert!(s.undo(&g));
    assert_eq!(s.song.steps(), before + 1, "undone, not gone");
    assert_eq!(s.project().tracks[&guitar].inserts[0].id, to);
    assert_eq!(held(&s, guitar), (0.625, Some(delay(0.625))));
    s.transport.pause();
}

/// The song's steps, where its history stands, whether it is unsaved, and whether a redo is
/// there.
fn history(s: &Studio) -> (usize, u64, bool, bool) {
    let session = s.song.session();
    (
        s.song.steps(),
        session.cursor(),
        session.is_dirty(),
        session.can_redo(),
    )
}

#[test]
fn a_copy_leaves_the_song_and_its_history_as_they_were() {
    copy_leaves_the_song("dedicated");
}

#[test]
fn a_copy_from_plugins_in_this_process_leaves_the_song_too() {
    copy_leaves_the_song("inprocess");
}

fn copy_leaves_the_song(mode: &str) {
    let Song {
        mut s,
        g,
        vocal,
        from,
        to,
        ..
    } = song(mode);
    // something to redo, which a copy must not take away
    assert!(s.run_command(
        "plugin.param.set",
        json!({ "id": to.to_string(), "param": "1", "value": 0.375 })
    ));
    assert!(s.undo(&g));
    turn_silently(&mut s, from, 0.5);
    let before = history(&s);
    assert!(before.3, "a redo is there");

    assert!(
        s.arrange_run("channel.inserts.copy", json!({ "id": vocal.to_string() }))
            .is_some()
    );
    crate::shot::run_for(&mut s, 5.0, |s| s.inserts_wait.is_none());
    assert!(s.inserts_wait.is_none());
    let Some(Clipboard::Inserts(c)) = &s.song.session().state.clipboard else {
        panic!("the inserts are copied");
    };
    let state = c.plugins[0].state.as_ref().expect("with a state");
    assert_eq!(
        xirius_plugins::decode_state(state).unwrap().component,
        delay(0.5)
    );
    assert_eq!(c.plugins[0].params["1"], 0.5, "the copy's value too");
    assert_eq!(held(&s, vocal), (0.75, None), "the song is as it was");
    assert_eq!(
        history(&s),
        before,
        "no step, nothing unsaved, the redo kept"
    );
    s.transport.pause();
}

#[test]
fn a_paste_that_would_be_refused_asks_nothing_and_leaves_no_step() {
    let Song {
        mut s, vocal, from, ..
    } = song("dedicated");
    assert!(
        s.arrange_run("channel.inserts.copy", json!({ "id": vocal.to_string() }))
            .is_some()
    );
    crate::shot::run_for(&mut s, 5.0, |s| s.inserts_wait.is_none());
    turn_silently(&mut s, from, 0.625);
    let before = history(&s);

    // onto the channel it was copied from: refused at once
    let pasted = s.arrange_run(
        "channel.inserts.paste",
        json!({ "ids": [vocal.to_string()] }),
    );
    assert!(pasted.is_none());
    assert!(s.inserts_wait.is_none(), "nothing asked");
    assert!(s.error.is_some(), "the refusal is said");
    assert_eq!(history(&s), before, "no step, nothing unsaved");
    assert_eq!(held(&s, vocal), (0.75, None));
    s.transport.pause();
}

#[test]
fn closing_the_window_puts_a_waiting_paste_in_before_asking_to_save() {
    let Song {
        mut s,
        g,
        vocal,
        guitar,
        to,
        ..
    } = song("dedicated");
    let _hold = Hold::new();
    drag(&mut s, &g, vocal, guitar);
    assert!(s.inserts_wait.is_some());
    assert!(!s.request_close());
    assert!(
        s.inserts_wait.is_none(),
        "not left running beside the close"
    );
    let pasted = &s.project().tracks[&guitar].inserts[0];
    assert_ne!(pasted.id, to, "the paste is in the song being closed");
    assert!(!s.poll_inserts_wait(), "and does not run again");
    s.transport.pause();
}

#[test]
fn a_paste_waiting_when_the_song_is_replaced_does_not_run_in_the_next_one() {
    let Song {
        mut s,
        g,
        vocal,
        guitar,
        ..
    } = song("dedicated");
    let _hold = Hold::new();
    drag(&mut s, &g, vocal, guitar);
    assert!(s.inserts_wait.is_some());
    s.new_song_sync();
    assert!(s.inserts_wait.is_none(), "it went with its song");
    assert!(!s.input_blocked());
    assert!(!s.poll_inserts_wait());
    assert_ne!(s.notice_text(), Some("インサートを貼り付けました"));
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
fn a_wait_cut_short_still_reads_the_plugins_in_this_process() {
    let Song {
        mut s,
        g,
        vocal,
        guitar,
        from,
        to,
    } = song("inprocess");
    turn_silently(&mut s, from, 0.5);
    turn_silently(&mut s, to, 0.625);
    // a copy reads them itself: nothing to wait for
    assert!(
        s.arrange_run("channel.inserts.copy", json!({ "id": vocal.to_string() }))
            .is_some()
    );
    assert!(s.inserts_wait.is_none(), "copied at once");
    drag(&mut s, &g, vocal, guitar);
    assert!(s.inserts_wait.is_some());
    // the window closed or the song left at once: what they hold is still read
    s.inserts_wait_flush();
    assert!(s.inserts_wait.is_none());
    assert_eq!(held(&s, guitar), (0.5, Some(delay(0.5))));
    assert_eq!(s.notice_text(), Some("インサートを貼り付けました"));
    assert!(s.undo(&g));
    assert_eq!(s.project().tracks[&guitar].inserts[0].id, to);
    assert_eq!(held(&s, guitar), (0.625, Some(delay(0.625))));
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
        to,
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
    turn_silently(&mut s, to, 0.625);
    drag(&mut s, &g, vocal, guitar);
    crate::shot::run_for(&mut s, 5.0, |s| s.inserts_wait.is_none());
    assert!(s.inserts_wait.is_none());
    assert_eq!(held(&s, guitar), (0.75, None));
    // the one that answered is still read: an undo brings it back as it answered
    assert!(s.undo(&g));
    assert_eq!(s.project().tracks[&guitar].inserts[0].id, to);
    assert_eq!(held(&s, guitar), (0.625, Some(delay(0.625))));
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

fn has_track(s: &Studio, name: &str) -> bool {
    s.project().tracks.values().any(|t| t.name == name)
}

#[test]
fn a_macro_copies_what_the_plugin_holds_now_before_its_next_step() {
    let Song {
        mut s,
        g,
        vocal,
        from,
        ..
    } = song("dedicated");
    turn_silently(&mut s, from, 0.5);
    s.song.settings().lock().unwrap().macros.set(
        "Copy",
        vec![
            format!("channel.inserts.copy --id {vocal}"),
            "track.add.audio --name After".into(),
        ],
    );
    let hold = Hold::new();
    assert!(s.run_macro(&g, "Copy"));
    assert!(s.inserts_wait.is_some(), "the step waits for the plug-in");
    assert!(!has_track(&s, "After"), "the next step waits too");
    hold.release();
    crate::shot::run_for(&mut s, 5.0, |s| has_track(s, "After"));
    assert_eq!(s.error, None);
    let Some(Clipboard::Inserts(c)) = &s.song.session().state.clipboard else {
        panic!("the inserts are copied");
    };
    let state = c.plugins[0].state.as_ref().expect("with a state");
    assert_eq!(
        xirius_plugins::decode_state(state).unwrap().component,
        delay(0.5)
    );
    assert!(has_track(&s, "After"), "and the macro went on");
    assert!(s.macro_run.is_none());
    s.transport.pause();
}

#[test]
fn a_macro_pastes_what_the_plugin_holds_now_before_its_next_step() {
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
    s.song.settings().lock().unwrap().macros.set(
        "Paste",
        vec![
            format!("channel.inserts.copy --id {vocal}"),
            // the words a macro writes: the plug-in it replaces is asked too
            format!("channel.inserts.paste --from {vocal} --ids {guitar}"),
            "track.add.audio --name After".into(),
        ],
    );
    assert!(s.run_macro(&g, "Paste"));
    crate::shot::run_for(&mut s, 10.0, |s| has_track(s, "After"));
    assert_eq!(s.error, None);
    assert!(has_track(&s, "After"));
    assert_eq!(
        held(&s, guitar),
        (0.5, Some(delay(0.5))),
        "pasted as it is now"
    );
    // the macro is one step: an undo takes it all back, the replaced one as it answered
    let steps = s.song.session().steps();
    let tail: Vec<_> = steps[steps.len() - 2..]
        .iter()
        .map(|x| (x.n, x.command.clone(), x.group))
        .collect();
    assert!(s.undo(&g));
    assert!(!has_track(&s, "After"));
    assert_eq!(
        s.project().tracks[&guitar].inserts[0].id,
        to,
        "one undo took the whole macro back; its last steps {tail:?}, error {:?}",
        s.error
    );
    assert_eq!(held(&s, guitar), (0.625, Some(delay(0.625))));
    s.transport.pause();
}

#[test]
fn a_macro_paste_that_cannot_run_after_its_wait_is_said_and_stops_the_macro() {
    let Song {
        mut s,
        g,
        vocal,
        guitar,
        ..
    } = song("dedicated");
    s.song.settings().lock().unwrap().macros.set(
        "Paste",
        vec![
            format!("channel.inserts.paste --from {vocal} --ids {guitar}"),
            "track.add.audio --name After".into(),
        ],
    );
    let hold = Hold::new();
    assert!(s.run_macro(&g, "Paste"));
    assert!(s.inserts_wait.is_some());
    // the target goes while it waits (only a script could do so)
    s.song
        .run("track.delete", json!({ "ids": [guitar.to_string()] }))
        .unwrap();
    hold.release();
    crate::shot::run_for(&mut s, 5.0, |s| {
        s.inserts_wait.is_none() && s.macro_run.is_none()
    });
    assert!(s.inserts_wait.is_none());
    assert!(s.macro_run.is_none(), "the macro stopped");
    assert!(s.error.is_some(), "and says why");
    assert!(!has_track(&s, "After"), "its next step did not run");
    s.transport.pause();
}

#[test]
fn input_held_for_a_paste_whose_song_is_replaced_is_dropped_with_it() {
    let Song {
        mut s,
        g,
        vocal,
        guitar,
        ..
    } = song("dedicated");
    let _hold = Hold::new();
    drag(&mut s, &g, vocal, guitar);
    assert!(s.inserts_wait.is_some());
    let mut typed = HeldInput::default();
    assert!(typed.hold(s.input_hold(), Held::Text(None, "z".into())));
    assert!(!s.take_held_input_dropped());
    s.new_song_sync();
    assert!(s.take_held_input_dropped(), "the window throws it away");
    assert!(!s.take_held_input_dropped(), "once");
    // as the window does with it (Screen's render): nothing typed reaches the next song
    typed.forget();
    assert!(typed.release(s.input_hold()).is_empty());
    s.transport.pause();
}

#[test]
fn leaving_for_a_new_song_puts_a_waiting_paste_in_this_one_first() {
    let Song {
        mut s,
        g,
        vocal,
        guitar,
        to,
        ..
    } = song("dedicated");
    let _hold = Hold::new();
    drag(&mut s, &g, vocal, guitar);
    assert!(s.inserts_wait.is_some());
    s.start_leaving(crate::saving::Leave::New);
    assert!(
        s.inserts_wait.is_none(),
        "not left running beside the leaving"
    );
    assert_ne!(
        s.project().tracks[&guitar].inserts[0].id,
        to,
        "the paste is in the song being left"
    );
    assert!(
        !s.take_held_input_dropped(),
        "what was typed meanwhile still goes to it"
    );
    assert!(!s.poll_inserts_wait(), "and it does not run again");
    s.transport.pause();
}

/// The browser lists the devices, so the screen reads the presets every half second: a read
/// while a macro waits for its paste must not end the macro's undo step.
#[test]
fn a_macro_stays_one_undo_step_while_the_screen_reads_presets_during_its_wait() {
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
    s.song.settings().lock().unwrap().macros.set(
        "Paste",
        vec![
            format!("channel.inserts.copy --id {vocal}"),
            format!("channel.inserts.paste --from {vocal} --ids {guitar}"),
            "track.add.audio --name After".into(),
        ],
    );
    let hold = Hold::new();
    assert!(s.run_macro(&g, "Paste"));
    // longer than the half second between two reads of the presets
    crate::shot::run_for(&mut s, 1.2, |_| false);
    assert!(s.inserts_wait.is_some(), "the paste is still waiting");
    hold.release();
    crate::shot::run_for(&mut s, 10.0, |s| has_track(s, "After"));
    let steps = s.song.session().steps();
    let tail = &steps[steps.len() - 2..];
    assert!(
        tail[0].group != 0 && tail[0].group == tail[1].group,
        "the paste and the track after it are one group: {:?}",
        tail.iter()
            .map(|x| (x.n, x.command.as_str(), x.group))
            .collect::<Vec<_>>()
    );
    assert!(s.undo(&g));
    assert!(!has_track(&s, "After"));
    assert_eq!(s.project().tracks[&guitar].inserts[0].id, to);
    s.transport.pause();
}
