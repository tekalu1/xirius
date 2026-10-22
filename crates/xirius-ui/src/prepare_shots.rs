//! 音源を準備中 (ENG-064): the card while the song's plugins open, and the lit key whose
//! instrument is opening again. The card's plugins are the test plugin, those in a host made
//! to take a minute to open, so the shot catches them under way.

use serde_json::json;
use xirius_core::model::TrackKind;

use crate::{
    shot::{Act, Play, Shot},
    studio::{Geometry, Studio},
};

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "prepare01-card",
        acts: &[Act::Reset, Act::Stage(Play::Laid(stage), 0)],
    },
    Shot {
        name: "prepare02-key",
        acts: &[Act::Reset, Act::Stage(Play::Laid(stage), 1)],
    },
    Shot {
        name: "prepare03-key-after-later",
        acts: &[Act::Reset, Act::Stage(Play::Laid(stage), 2)],
    },
    Shot {
        name: "prepare04-key-in-a-swap",
        acts: &[Act::Reset, Act::Stage(Play::Laid(stage), 3)],
    },
];

/// How long the test plugin takes to open in a host process.
const SLOW_MS: &str = "60000";

fn stage(s: &mut Studio, g: &Geometry, n: u8) {
    match n {
        0 => card(s),
        1 => key(s, g),
        2 => key_after_later(s, g),
        _ => key_in_a_swap(s, g),
    }
}

/// Every track gets the test plugin: the first three in this process, the others (and the
/// Keys track's instrument) in hosts of their own.
fn with_test_plugin(s: &mut Studio) {
    #[cfg(test)]
    crate::plugin_reload_shots::require_plugin_host();
    let base = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let dll = [
        base.join("xirius_test_vst3.dll"),
        base.join("deps/xirius_test_vst3.dll"),
    ]
    .into_iter()
    .find(|p| p.is_file())
    .expect("build xirius-test-vst3 first");
    let entries = xirius_plugins::probe(&dll).unwrap();
    let entry = entries[0].clone();
    let keys = s.roll_target().map(|t| t.track.id);
    let p = s.project();
    let tracks: Vec<_> = p
        .tracks
        .values()
        .filter(|t| matches!(t.kind, TrackKind::Audio | TrackKind::Instrument))
        .map(|t| t.id)
        .collect();
    for (i, track) in tracks.iter().enumerate() {
        let instrument = Some(*track) == keys;
        assert!(s.run_command(
            "plugin.add",
            json!({
                "track":track.to_string(), "name":entry.name,
                "kind": if instrument { "instrument" } else { "effect" },
                "format":"vst3", "uid":entry.uid
            })
        ));
        // An instrument goes in front of the effects the track has.
        let inserts = &s.project().tracks[track].inserts;
        let id = if instrument {
            inserts[0].id
        } else {
            inserts.last().unwrap().id
        };
        let mode = if instrument || i >= 3 {
            "dedicated"
        } else {
            "inprocess"
        };
        assert!(s.run_command(
            "plugin.isolation.set",
            json!({"ids":[id.to_string()], "mode":mode})
        ));
    }
    s.use_plugins(xirius_plugins::Catalog {
        entries,
        failures: Vec::new(),
    });
}

/// Runs `f` with the plugins opened in a host started meanwhile taking a minute. The hosts
/// start on threads of their own, so they get a moment to start before it goes again: the
/// plugins opened in this process afterwards must not wait.
fn slow_hosts(s: &mut Studio, f: impl FnOnce(&mut Studio)) {
    // SAFETY: nothing else in the shot window reads the environment meanwhile.
    unsafe { std::env::set_var("XIRIUS_TEST_VST3_OPEN_MS", SLOW_MS) };
    f(s);
    std::thread::sleep(std::time::Duration::from_secs(2));
    unsafe { std::env::remove_var("XIRIUS_TEST_VST3_OPEN_MS") };
}

/// Two steps in, the card shows the first tracks done and the rest under way.
fn card(s: &mut Studio) {
    with_test_plugin(s);
    slow_hosts(s, |s| {
        s.prepare_song();
        s.poll_preparing();
    });
    s.poll_preparing();
    s.poll_preparing();
}

/// A key of the roll held while its instrument opens again (as after it was swapped, or its
/// host went down): the lit key shows the spinner.
fn key(s: &mut Studio, g: &Geometry) {
    // The roll takes in its clip first: a clip it has not shown starts a fresh view, which
    // would let go of the key.
    if let Some(c) = s.roll_ctx(g) {
        s.roll.sync(&c);
    }
    let Some(target) = s.roll_target() else {
        return;
    };
    s.transport.not_ready = vec![target.track.id];
    let pitch = target.clip.notes().first().map(|n| n.pitch).unwrap_or(60);
    s.roll.audition = Some(pitch);
    s.sound_roll_audition();
    if let Some((want, _)) = s.pc_keys.waiting {
        let since = std::time::Instant::now() - std::time::Duration::from_secs(1);
        s.pc_keys.waiting = Some((want, since));
    }
}

/// The roll's key pressed, and the frames run for a moment as the window runs them: a key
/// whose instrument is not in shows the spinner after [`WAIT_SHOWN`] (ENG-065: nothing here
/// is set by hand, the instrument really is on its way).
fn press_and_wait(s: &mut Studio, g: &Geometry) {
    if let Some(c) = s.roll_ctx(g) {
        s.roll.sync(&c);
    }
    let Some(target) = s.roll_target() else {
        return;
    };
    let pitch = target.notes.first().map(|n| n.pitch).unwrap_or(60);
    s.roll.audition = Some(pitch);
    s.sound_roll_audition();
    let began = std::time::Instant::now();
    while began.elapsed() < std::time::Duration::from_millis(400) {
        s.tick();
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}

/// 後で読む, then the first key: the hosts are still opening the instrument, the window ran on.
fn key_after_later(s: &mut Studio, g: &Geometry) {
    with_test_plugin(s);
    s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
    slow_hosts(s, |s| {
        s.prepare_song();
        s.poll_preparing();
    });
    s.prepare_later();
    press_and_wait(s, g);
}

/// The instrument is playing; it is moved to the shared host (a swap that takes a minute to
/// open) and a key is pressed meanwhile: the spinner turns, the song goes on.
fn key_in_a_swap(s: &mut Studio, g: &Geometry) {
    with_test_plugin(s);
    // A plugin opened in this process takes the whole minute in the window's own frame (that is
    // the rule for them), so only hosts of their own are left: the swap is the one thing slow.
    let ids: Vec<_> = s
        .project()
        .tracks
        .values()
        .flat_map(|t| t.inserts.iter().map(|i| i.id))
        .collect();
    for id in ids {
        s.run_command(
            "plugin.isolation.set",
            json!({"ids":[id.to_string()], "mode":"dedicated"}),
        );
    }
    s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
    s.prepare_song();
    let began = std::time::Instant::now();
    while s.poll_preparing() {
        assert!(began.elapsed().as_secs() < 60, "the song did not open");
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    if let Some(c) = s.roll_ctx(g) {
        s.roll.sync(&c);
    }
    let Some(target) = s.roll_target() else {
        return;
    };
    let track = target.track.id;
    let (p, dir) = (s.project(), s.song.dir());
    let began = std::time::Instant::now();
    while s.transport.instrument_ready(&p, dir.clone(), track) != Ok(true) {
        assert!(
            began.elapsed().as_secs() < 60,
            "the instrument did not open"
        );
        s.tick();
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    let _ = s.transport.play(&p, dir, 0);
    let id = s.project().tracks[&track].inserts[0].id;
    slow_hosts(s, |s| {
        assert!(s.run_command(
            "plugin.isolation.set",
            json!({"ids":[id.to_string()], "mode":"shared"})
        ));
        // The window's first frames start the swap; the key is pressed in the middle of it.
        for _ in 0..4 {
            s.tick();
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
        press_and_wait(s, g);
    });
}
