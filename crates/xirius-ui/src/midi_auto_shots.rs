use crate::{
    shot::{Act, Play, Shot, Target},
    studio::Studio,
    tree::RowKind,
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use xirius_core::id::TrackId;
use xirius_engine::record::MockMidi;
use xirius_session::host::Strip;

thread_local! {
    static WATCH: std::cell::RefCell<Option<std::sync::mpsc::Sender<Vec<String>>>> = const { std::cell::RefCell::new(None) };
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "midi01-sampler",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Studio(prepare), 0),
            Act::Press(Target::Track("Keys")),
        ],
    },
    Shot {
        name: "midi02-connected",
        acts: &[Act::Stage(Play::Studio(plug), 0)],
    },
    Shot {
        name: "midi03-unplugged",
        acts: &[Act::Stage(Play::Studio(unplug), 0)],
    },
];
pub(crate) fn track(s: &Studio, name: &str) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == name)
        .unwrap()
        .id
}
pub(crate) fn setup(s: &mut Studio, name: &str, instrument: &str, pitch: u8) -> Arc<AtomicUsize> {
    s.drawer_open = false;
    s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
    let t = track(s, name);
    assert!(s.run_command(
        "plugin.replace",
        json!({"track":t,"slot":"instrument","name":instrument,"format":"builtin","uid":instrument})
    ));
    assert!(!s.project().tracks[&t].armed);
    if instrument == "Sampler" {
        let id = crate::instrument::current(&s.project().tracks[&t])
            .unwrap()
            .id;
        for (param, plain) in [("sound", 2.0), ("release", 50.0)] {
            let value = xirius_params::find("Sampler")
                .unwrap()
                .param(param)
                .unwrap()
                .1
                .to_norm(plain);
            assert!(s.run_command(
                "plugin.param.set",
                json!({"id":id,"param":param,"value":value})
            ));
        }
    }
    let opened = Arc::new(AtomicUsize::new(0));
    let count = opened.clone();
    s.midi_opener(move || {
        count.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(MockMidi::with_ports(
            vec!["Mock Keyboard".into()],
            vec![(0, 0, [0x90, pitch, 100])],
        )))
    });
    opened
}
fn prepare(s: &mut Studio, _: u8) {
    setup(s, "Keys", "Sampler", 60);
}
pub(crate) fn connect(s: &mut Studio, name: &str) {
    let t = track(s, name);
    assert_eq!(s.selected_row, Some(RowKind::Track(t)));
    let (tx, rx) = std::sync::mpsc::channel();
    s.watch_midi_ports_from(rx);
    tx.send(Vec::new()).unwrap();
    s.idle();
    tx.send(vec!["Mock Keyboard".into()]).unwrap();
    s.idle();
    settle_open(s);
    WATCH.with(|watch| *watch.borrow_mut() = Some(tx));
    assert!(s.live_midi.input.is_some());
    assert_eq!(s.midi_ports.known(), ["Mock Keyboard"]);
    crate::shot::run_for(s, 5.0, |s| {
        s.mixer.meters.get(Strip::Track(t)).hold[0] > 0.0
    });
    assert!(
        s.mixer.meters.get(Strip::Track(t)).hold[0] > 0.0,
        "{:?}",
        s.error
    );
    assert!(!s.transport.playing && !s.transport.recording);
    assert!(s.error.is_none(), "{:?}", s.error);
}

fn settle_open(s: &mut Studio) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while s.live_midi.opening.is_some() {
        s.poll_live_midi();
        assert!(
            std::time::Instant::now() < deadline,
            "MIDI opening watchdog"
        );
        std::thread::yield_now();
    }
}
fn plug(s: &mut Studio, _: u8) {
    connect(s, "Keys");
}
pub(crate) fn disconnect(s: &mut Studio, name: &str) {
    WATCH.with(|watch| watch.borrow_mut().take().unwrap().send(Vec::new()).unwrap());
    assert!(s.idle().changed);
    assert!(s.live_midi.input.is_none());
    let t = track(s, name);
    let start = std::time::Instant::now();
    while fresh_peak(s, t) > 1e-6 {
        assert!(
            start.elapsed() < std::time::Duration::from_secs(3),
            "note still sounds after unplug"
        );
    }
    crate::shot::run_for(s, 5.0, |s| {
        s.mixer.meters.get(Strip::Track(t)).level[0] == 0.0
    });
    assert_eq!(s.mixer.meters.get(Strip::Track(t)).level, [0.0, 0.0]);
}
fn fresh_peak(s: &mut Studio, t: TrackId) -> f32 {
    let started = std::time::Instant::now();
    loop {
        let host = s.transport.host_mut().unwrap();
        host.poll();
        if let Some(level) = host.take_levels().get(&Strip::Track(t)) {
            return level.peak[0].max(level.peak[1]);
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "no fresh DSP meter"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
fn unplug(s: &mut Studio, _: u8) {
    disconnect(s, "Keys");
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::Hit;
    #[test]
    fn repeated_open_failures_preserve_another_error_and_empty_success_repaints() {
        let mut s = Studio::sample();
        let count = Arc::new(AtomicUsize::new(0));
        let attempts = count.clone();
        s.midi_opener(move || {
            let n = attempts.fetch_add(1, Ordering::Relaxed);
            if n < 3 {
                return Err(format!("MIDI error {n}"));
            }
            Ok(Box::new(MockMidi::with_ports(
                vec!["Mock Keyboard".into()],
                Vec::new(),
            )))
        });
        s.midi_ports_now(Vec::new());
        s.midi_ports_now(vec!["Mock Keyboard".into()]);
        settle_open(&mut s);
        s.live_midi.retry_at = Some(std::time::Instant::now());
        s.idle();
        settle_open(&mut s);
        assert_eq!(s.error.as_deref(), Some("MIDI error 1"));
        s.error = Some("別のエラー".into());
        s.live_midi.retry_at = Some(std::time::Instant::now());
        s.idle();
        settle_open(&mut s);
        assert_eq!(s.error.as_deref(), Some("別のエラー"));
        s.live_midi.retry_at = Some(std::time::Instant::now());
        s.idle();
        settle_open(&mut s);
        assert_eq!(s.error.as_deref(), Some("別のエラー"));
        assert_eq!(count.load(Ordering::Relaxed), 4);
    }
    #[test]
    fn recording_does_not_lose_a_pending_open_retry() {
        let mut s = Studio::sample();
        setup(&mut s, "Keys", "Sampler", 60);
        let attempts = Arc::new(AtomicUsize::new(0));
        let count = attempts.clone();
        s.midi_opener(move || {
            if count.fetch_add(1, Ordering::Relaxed) == 0 {
                return Err("MIDI入力を開けません".into());
            }
            Ok(Box::new(MockMidi::with_ports(
                vec!["Mock Keyboard".into()],
                Vec::new(),
            )))
        });
        s.midi_ports_now(Vec::new());
        s.midi_ports_now(vec!["Mock Keyboard".into()]);
        settle_open(&mut s);
        let t = track(&s, "Keys");
        assert!(s.run_command("track.arm", json!({"ids":[t],"on":true})));
        assert!(s.toggle_record());
        assert!(s.live_midi.resume);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !s.transport.recording {
            s.tick();
            assert!(
                std::time::Instant::now() < deadline,
                "record broker watchdog: {:?}",
                s.error
            );
            std::thread::yield_now();
        }
        assert_eq!(attempts.load(Ordering::Relaxed), 2);
        assert!(s.toggle_record());
        s.live_midi.retry_at = Some(std::time::Instant::now());
        s.idle();
        settle_open(&mut s);
        assert!(s.live_midi.input.is_some());
        assert_eq!(attempts.load(Ordering::Relaxed), 3);
        s.midi_ports_now(Vec::new());
        s.transport.reset();
    }
    #[test]
    fn a_port_that_is_not_ready_retries_and_clears_its_own_error() {
        let mut s = Studio::sample();
        setup(&mut s, "Keys", "Sampler", 60);
        let attempts = Arc::new(AtomicUsize::new(0));
        let count = attempts.clone();
        s.midi_opener(move || {
            if count.fetch_add(1, Ordering::Relaxed) == 0 {
                return Err("MIDI入力を開けません".into());
            }
            Ok(Box::new(MockMidi::with_ports(
                vec!["Mock Keyboard".into()],
                vec![(0, 0, [0x90, 60, 100])],
            )))
        });
        s.midi_ports_now(Vec::new());
        s.midi_ports_now(vec!["Mock Keyboard".into()]);
        settle_open(&mut s);
        assert!(s.live_midi.input.is_none());
        assert!(s.live_midi.resume);
        assert!(s.error.is_some());
        s.live_midi.retry_at = Some(std::time::Instant::now());
        s.idle();
        settle_open(&mut s);
        assert!(s.live_midi.input.is_some());
        assert!(s.error.is_none());
        assert_eq!(attempts.load(Ordering::Relaxed), 2);
        s.midi_ports_now(Vec::new());
        s.transport.reset();
    }
    #[test]
    fn hotplug_opens_a_port_and_plays_the_selected_sampler_without_arming() {
        let mut s = Studio::sample();
        let opens = setup(&mut s, "Keys", "Sampler", 60);
        let t = track(&s, "Keys");
        let g = s.geometry(1600.0, 1000.0);
        assert!(s.press(&g, Hit::Row(RowKind::Track(t))));
        let steps = s.song.steps();
        connect(&mut s, "Keys");
        let held = std::time::Instant::now();
        while held.elapsed() < std::time::Duration::from_secs(2) {
            assert!(
                fresh_peak(&mut s, t) > 1e-4,
                "sustained sample ended without disconnect"
            );
        }
        assert_eq!(opens.load(Ordering::Relaxed), 1);
        assert!(!s.midi_ports_now(vec!["Mock Keyboard".into()]));
        assert_eq!(opens.load(Ordering::Relaxed), 1);
        assert_eq!(s.song.steps(), steps);
        disconnect(&mut s, "Keys");
        s.transport.reset();
    }
}
