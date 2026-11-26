//! Writing automation from the mixer while the song plays (AUT-002).
//!
//! A pass starts when play starts and ends when it stops; what it wrote is
//! laid into the lanes by one `auto.write`, so one undo takes it back. The
//! rules (Touch, Latch, Write) are [`xirius_session::autowrite`]'s; here the
//! mixer's drags are turned into touches and the player is given the song as
//! it should be heard while a control writes.

use serde_json::json;
use xirius_core::auto::AutoTarget;
use xirius_core::model::Project;
use xirius_core::time::Tick;
use xirius_session::autowrite::{Pass, Phase};

use crate::mixer::MixerHit;
use crate::studio::Studio;

/// The pass of the play in progress.
#[derive(Clone, Debug)]
pub struct Writing {
    pass: Pass,
    /// The last position seen while playing: where stop ends the pass.
    last: Tick,
}

/// The lane a mixer control writes to.
pub(crate) fn target_of(hit: MixerHit) -> Option<AutoTarget> {
    match hit {
        MixerHit::Fader(_) => Some(AutoTarget::Volume),
        MixerHit::Pan(_) => Some(AutoTarget::Pan),
        MixerHit::SendLevel(_, s) => Some(AutoTarget::Send(s)),
        MixerHit::Control(_, c) => Some(AutoTarget::Control(c)),
        _ => None,
    }
}

impl Studio {
    /// Tracks whose control changes the base value during this pass.
    pub(crate) fn auto_direct_targets(&self, hit: MixerHit) -> Vec<xirius_core::id::TrackId> {
        let mut ids = self.drag_tracks(hit);
        // The channels whose lane holds the control stay as they are (UI-063).
        if matches!(hit, MixerHit::Fader(_) | MixerHit::Pan(_)) {
            ids.retain(|t| {
                let h = match hit {
                    MixerHit::Fader(_) => MixerHit::Fader(*t),
                    _ => MixerHit::Pan(*t),
                };
                !self.mixer_locked(h)
            });
        }
        if self.transport.playing
            && target_of(hit).is_some()
            && let Some(w) = &self.arrange.writing
        {
            ids.retain(|t| w.pass.mode(*t).is_none());
        }
        ids
    }
    /// A mixer control was taken hold of, moved or let go while playing.
    /// True when its track writes automation: the value went into the pass
    /// and the player heard it, so the caller changes nothing in the song.
    pub(crate) fn auto_touch(&mut self, hit: MixerHit, phase: Phase, value: Option<f64>) -> bool {
        let Some(target) = target_of(hit).filter(|_| self.transport.playing) else {
            return false;
        };
        let at = self.transport.tick_now().unwrap_or(self.playhead);
        self.auto_advance(at);
        let ids = self.drag_tracks(hit);
        // Each selected track writes its own value, moved as far as the dragged one (MIX-050).
        let values: Vec<Option<f64>> = ids
            .iter()
            .map(|t| {
                let h = crate::mixer::group::same_on(hit, *t);
                value.map(|v| self.dragged_to(h, self.mixer_rest(h), v))
            })
            .collect();
        let Some(w) = self.arrange.writing.as_mut() else {
            return false;
        };
        let count = ids
            .iter()
            .zip(values)
            .filter(|(t, v)| w.pass.touch(**t, target.clone(), phase, at, *v))
            .count();
        if count == 0 {
            return false;
        }
        w.last = at;
        let p = self.project();
        let heard = self.auto_heard(&p);
        self.transport.sync(&heard, self.song().dir());
        count == ids.len()
    }

    /// The song as the player should hear it: while a control writes, its
    /// lane does not move it.
    pub(crate) fn auto_heard(&self, p: &Project) -> Project {
        let mut heard = self
            .arrange
            .writing
            .as_ref()
            .and_then(|w| w.pass.heard(p))
            .unwrap_or_else(|| p.clone());
        for track in heard.tracks.values_mut() {
            if !track.sends.iter().any(|s| s.muted_level.is_some()) {
                continue;
            }
            for send in &mut std::sync::Arc::make_mut(track).sends {
                if send.muted_level.is_some() {
                    send.level_db = xirius_core::model::MIN_DB;
                }
            }
        }
        // What a device's window is turning is heard until it goes into the song (M-ui-1).
        let turned = self.instruments.turned();
        if !turned.is_empty() {
            let held = |x: &xirius_core::model::Plugin| turned.iter().any(|(id, ..)| *id == x.id);
            for track in heard.tracks.values_mut() {
                if !track.inserts.iter().any(held) {
                    continue;
                }
                let t = std::sync::Arc::make_mut(track);
                for x in &mut t.inserts {
                    for (_, k, v) in turned.iter().filter(|(id, ..)| *id == x.id) {
                        let def = self.instruments.def_of(x);
                        if let Some(spec) = def.and_then(|d| d.params.get(*k)) {
                            x.params.insert(spec.id.to_string(), *v);
                        }
                    }
                }
            }
        }
        heard
    }

    /// The song as the devices' windows show it: the lanes' values at the playhead, as the
    /// mixer's faders show theirs (a band's switch follows its lane), and what a pass is
    /// writing (a latched value) as it is heard.
    pub(crate) fn auto_shown(&self) -> std::sync::Arc<Project> {
        let p = self.project();
        let p = match self.arrange.writing.as_ref().and_then(|w| w.pass.heard(&p)) {
            Some(heard) => std::sync::Arc::new(heard),
            None => p,
        };
        match xirius_session::automation::song_at(&p, self.playhead as f64) {
            std::borrow::Cow::Owned(heard) => std::sync::Arc::new(heard),
            std::borrow::Cow::Borrowed(_) => p.clone(),
        }
    }

    fn auto_advance(&mut self, at: Tick) {
        let range = self
            .transport
            .looping
            .then(|| self.project().loop_range)
            .flatten();
        if let Some(w) = &mut self.arrange.writing {
            if at < w.last
                && let Some(range) = range
            {
                w.pass.restart_segment(range.start, range.end);
            }
            w.last = at;
        }
    }

    pub(crate) fn auto_seek(&mut self, at: Tick) {
        if self.transport.playing
            && let Some(w) = &mut self.arrange.writing
        {
            w.pass.restart_segment(at, w.last);
            w.last = at;
        }
    }

    /// Once a frame: a pass starts with play and is written at stop.
    pub(crate) fn auto_settle(&mut self) {
        let playing = self.transport.playing && self.transport.is_live();
        if playing {
            self.auto_advance(self.playhead);
        }
        match (&mut self.arrange.writing, playing) {
            (None, true) => {
                let p = self.project();
                let start = self.transport.from;
                let mut controls = p.as_ref().clone();
                for track in controls.tracks.values_mut() {
                    if !track.sends.iter().any(|s| s.muted_level.is_some()) {
                        continue;
                    }
                    for send in &mut std::sync::Arc::make_mut(track).sends {
                        send.level_db = crate::mixer::send_level_db(send);
                    }
                }
                let pass = Pass::begin(&controls, start);
                self.arrange.writing = Some(Writing {
                    pass,
                    last: self.playhead.max(start),
                });
                let heard = self.auto_heard(&p);
                self.transport.sync(&heard, self.song().dir());
            }
            (Some(w), true) => w.last = self.playhead,
            (Some(_), false) => {
                let w = self.arrange.writing.take().expect("matched above");
                let wrote = !w.pass.is_empty();
                let writes = w.pass.finish(w.last);
                if !writes.is_empty() {
                    // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                    self.run_command("auto.write", json!({ "writes": writes }));
                } else if wrote {
                    let p = self.project();
                    self.transport.sync(&p, self.song().dir());
                }
            }
            (None, false) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use serde_json::json;
    use xirius_core::auto::{AutoLane, AutoTarget};
    use xirius_core::id::{SendId, TrackId};
    use xirius_core::time::Tick;
    use xirius_session::output::OutputMode;

    use super::{Pass, Writing};
    use crate::mixer::MixerHit;
    use crate::studio::{DrawerTab, Geometry, Hit, Studio};

    const BAR: Tick = 3840;

    /// The sample song playing to a silent output, the mixer open, and
    /// `name`'s track in `mode`.
    fn studio(name: &str, mode: &str) -> (Studio, Geometry, TrackId) {
        let mut s = Studio::sample();
        s.transport.mode = OutputMode::Silent { paced: true };
        s.drawer_open = true;
        s.tab = DrawerTab::Mixer;
        let g = s.geometry(1600.0, 1000.0);
        let t = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == name)
            .map(|t| t.id)
            .unwrap();
        assert!(s.run_command("auto.mode", json!({ "ids": [t.to_string()], "mode": mode })));
        (s, g, t)
    }

    /// Ticks once a frame for `ms`.
    fn play_for(s: &mut Studio, ms: u64) {
        let t = Instant::now();
        while t.elapsed() < Duration::from_millis(ms) {
            s.tick();
            std::thread::sleep(Duration::from_millis(16));
        }
    }

    /// Ticks once a frame until `done`, or fails after a minute (a bound for a busy machine).
    fn tick_until(s: &mut Studio, what: &str, done: impl Fn(&Studio) -> bool) {
        let t = Instant::now();
        while !done(s) {
            assert!(t.elapsed() < Duration::from_secs(60), "{what}");
            s.tick();
            std::thread::sleep(Duration::from_millis(16));
        }
    }

    fn start(s: &mut Studio) {
        assert!(s.toggle_play());
        assert!(s.transport.playing, "{:?}", s.error);
        tick_until(s, "the pass starts writing", |s| {
            s.arrange.writing.is_some()
        });
        play_for(s, 150);
    }

    fn stop(s: &mut Studio) {
        s.stop_transport();
        tick_until(s, "the pass ends", |s| s.arrange.writing.is_none());
    }

    fn volume_lane(s: &Studio, t: TrackId) -> Option<AutoLane> {
        s.project().tracks[&t]
            .automation
            .iter()
            .find(|l| l.target == AutoTarget::Volume)
            .cloned()
    }

    /// A pass from `at` as play starts one, without the player: the test moves the playhead.
    fn begin_pass(s: &mut Studio, at: Tick) {
        s.arrange.writing = Some(Writing {
            pass: Pass::begin(&s.project(), at),
            last: at,
        });
        s.playhead = at;
        s.transport.playing = true;
    }

    /// Plays on to `at`, as a frame does while playing, and stops there: the pass is written.
    fn end_pass(s: &mut Studio, at: Tick) {
        s.playhead = at;
        s.auto_advance(at);
        s.transport.playing = false;
        s.auto_settle();
    }

    /// A send from `t` to a new bus at -9 dB, muted. Undo must land exactly on the state the
    /// test keeps, so the instrument input does not follow the selection (REC-024).
    fn muted_send(s: &mut Studio, t: TrackId) -> SendId {
        s.rec_set(
            "prefs.set",
            json!({"key": "midi.follow-selection", "value": "false"}),
        );
        let bus = s.song.run("track.add.bus", json!({"name":"Fx"})).unwrap();
        s.song
            .run(
                "send.add",
                json!({"ids":[t.to_string()],"to":bus["id"],"level":-9}),
            )
            .unwrap();
        let id = s.project().tracks[&t].sends[0].id;
        s.song
            .run("send.mute", json!({"id":id.to_string()}))
            .unwrap();
        id
    }

    /// Drags the fader down by `dy` points in steps, playing between them.
    fn drag_fader(s: &mut Studio, g: &Geometry, t: TrackId, dy: f32) -> f64 {
        let hit = MixerHit::Fader(t);
        let (_, y) = s.mixer_center(g, hit).unwrap();
        assert!(s.drag_begin(g, Hit::Mixer(hit), y));
        for i in 1..=4 {
            s.drag_move(y + dy * i as f32 / 4.0, false);
            play_for(s, 60);
        }
        s.mixer.drag.unwrap().value
    }

    /// The one test that plays for real: play starts the pass once the engine is live, from
    /// the controls as they are heard (a muted send at the level it keeps), the drags take
    /// their ticks from the engine, and stop writes the whole pass as one step. The rules of
    /// each mode are the direct-pass tests' below.
    #[test]
    fn touch_writes_the_fader_and_a_muted_send_while_held_in_one_step_of_a_real_play() {
        let (mut s, g, t) = studio("Keys", "touch");
        let id = muted_send(&mut s, t);
        let before = s.project();
        let own = before.tracks[&t].volume_db;
        let steps = s.song().steps();
        start(&mut s);
        let low = drag_fader(&mut s, &g, t, 40.0);
        assert!(low < own - 1.0, "{low} < {own}");
        assert!(s.drag_end());
        let hit = MixerHit::SendLevel(t, id);
        let (_, y) = s.mixer_center(&g, hit).unwrap();
        assert!(s.drag_begin(&g, Hit::Mixer(hit), y));
        assert!(s.drag_move(y + 30.0, false));
        let sent = s.mixer.drag.unwrap().value;
        assert!(sent < -9.0, "{sent}");
        assert!(s.drag_end());
        play_for(&mut s, 300);
        stop(&mut s);

        // One step; the fader's own value and the send are untouched.
        assert_eq!(s.song().steps(), steps + 1);
        assert_eq!(s.project().tracks[&t].volume_db, own);
        assert_eq!(s.project().tracks[&t].sends, before.tracks[&t].sends);
        let lane = volume_lane(&s, t).expect("a volume lane");
        let pts = &lane.points;
        assert!(pts.len() >= 2, "{pts:?}");
        assert!(pts.iter().any(|p| (p.value - low).abs() < 1e-6), "{pts:?}");
        // Let go, the lane goes back to where it was (the fader's value).
        assert!((pts.last().unwrap().value - own).abs() < 1e-6, "{pts:?}");
        let p = s.project();
        let lane = p.tracks[&t]
            .automation
            .iter()
            .find(|l| l.target == AutoTarget::Send(id))
            .expect("a send lane");
        assert!(lane.suspended);
        assert!(lane.points.iter().any(|pt| (pt.value - sent).abs() < 1e-6));
        // The muted send goes back to the level it keeps, not to silence.
        assert_eq!(lane.points.last().unwrap().value, -9.0);

        // One undo takes the whole pass back.
        assert!(s.undo(&g));
        let mut restored = s.project().as_ref().clone();
        restored.next_id = before.next_id;
        assert_eq!(restored, *before);
    }

    #[test]
    fn a_muted_send_records_touch_latch_and_write_without_becoming_audible() {
        for mode in ["touch", "latch", "write"] {
            let (mut s, g, t) = studio("Keys", mode);
            let id = muted_send(&mut s, t);
            let before = s.project();
            let steps = s.song().steps();
            begin_pass(&mut s, 0);
            let hit = MixerHit::SendLevel(t, id);
            let (_, y) = s.mixer_center(&g, hit).unwrap();
            assert!(s.drag_begin(&g, Hit::Mixer(hit), y), "{mode}");
            s.playhead = BAR;
            assert!(s.drag_move(y + 30.0, false), "{mode}");
            let value = s.mixer.drag.unwrap().value;
            assert!(value < -9.0, "{mode}: {value}");
            let heard = s.auto_heard(&s.project());
            assert_eq!(
                heard.tracks[&t].sends[0].level_db,
                xirius_core::model::MIN_DB,
                "{mode}"
            );
            // What is heard moves the kept level, as a macro does (MIX-039); the stored
            // project keeps -9 until the pass is written.
            assert_eq!(heard.tracks[&t].sends[0].muted_level, Some(value), "{mode}");
            assert_eq!(
                s.project().tracks[&t].sends[0].muted_level,
                Some(-9.0),
                "{mode}"
            );
            s.playhead = 2 * BAR;
            assert!(s.drag_end(), "{mode}");
            assert_eq!(
                s.auto_heard(&s.project()).tracks[&t].sends[0].level_db,
                xirius_core::model::MIN_DB,
                "{mode}"
            );
            end_pass(&mut s, 3 * BAR);
            let p = s.project();
            let lane = p.tracks[&t]
                .automation
                .iter()
                .find(|l| l.target == AutoTarget::Send(id))
                .unwrap();
            assert!(lane.suspended, "{mode}");
            assert!(
                lane.points.iter().any(|pt| (pt.value - value).abs() < 1e-6),
                "{mode}: {:?}",
                lane.points
            );
            assert_eq!(p.tracks[&t].sends, before.tracks[&t].sends, "{mode}");
            assert_eq!(s.song().steps(), steps + 1, "{mode}");
            assert!(s.undo(&g), "{mode}");
            let mut restored = s.project().as_ref().clone();
            restored.next_id = before.next_id;
            assert_eq!(restored, *before, "{mode}");
        }
    }

    /// The rule of each mode for the fader, on a pass the test drives (the real play is the
    /// test above): Latch keeps the value it was let go at until stop; Write, untouched, covers
    /// the pass from where it starts with the fader's value and keeps the lane past it (a lane
    /// that is not there yet is made only by touching its control); Read sends the drag to the
    /// fader. Each is one step, and one undo takes it back.
    #[test]
    fn each_mode_writes_a_held_fader_by_its_own_rule() {
        let mut failures = Vec::new();
        for mode in ["latch", "write", "read"] {
            let (mut s, g, t) = studio("Keys", mode);
            // Undo must land exactly on the kept state: the input does not follow (REC-024).
            s.rec_set(
                "prefs.set",
                json!({"key": "midi.follow-selection", "value": "false"}),
            );
            let own = s.project().tracks[&t].volume_db;
            let far = 64 * BAR;
            if mode == "write" {
                assert!(s.run_command(
                    "auto.point.add",
                    json!({ "track": t.to_string(), "target": "volume", "at": far, "value": -30.0 })
                ));
            }
            let before = s.project();
            let steps = s.song().steps();
            begin_pass(&mut s, 0);
            let mut low = own;
            if mode != "write" {
                let hit = MixerHit::Fader(t);
                let (_, y) = s.mixer_center(&g, hit).unwrap();
                assert!(s.drag_begin(&g, Hit::Mixer(hit), y), "{mode}");
                s.playhead = BAR;
                assert!(s.drag_move(y + 40.0, false), "{mode}");
                low = s.mixer.drag.unwrap().value;
                s.playhead = 2 * BAR;
                assert!(s.drag_end(), "{mode}");
                if low >= own - 1.0 {
                    failures.push(format!("{mode}: the drag went to {low} from {own}"));
                }
            }
            let end = 4 * BAR;
            end_pass(&mut s, end);
            let lane = volume_lane(&s, t);
            let fader = s.project().tracks[&t].volume_db;
            let points = lane.as_ref().map(|l| l.points.clone()).unwrap_or_default();
            let kept = match mode {
                "latch" => {
                    fader == own
                        && points
                            .iter()
                            .any(|p| (p.value - low).abs() < 1e-6 && p.tick >= end - 30)
                }
                "write" => {
                    fader == own
                        && points.iter().any(|p| (p.tick, p.value) == (0, own))
                        && lane
                            .as_ref()
                            .is_some_and(|l| l.value_at(end as f64 - 30.0) == Some(own))
                        && points.iter().any(|p| p.tick == far)
                        && s.project().tracks[&t]
                            .automation
                            .iter()
                            .all(|l| l.target == AutoTarget::Volume)
                }
                _ => fader == low && points.is_empty(),
            };
            if !kept {
                failures.push(format!(
                    "{mode}: fader {fader}, own {own}, low {low}: {points:?}"
                ));
            }
            if s.song().steps() != steps + 1 {
                failures.push(format!("{mode}: {} steps", s.song().steps() - steps));
            }
            assert!(s.undo(&g), "{mode}");
            let mut restored = s.project().as_ref().clone();
            restored.next_id = before.next_id;
            if restored != *before {
                failures.push(format!("{mode}: one undo does not take the pass back"));
            }
        }
        assert!(failures.is_empty(), "each mode's rule: {failures:#?}");
    }
    /// AUT-016 (the review's medium 3): after a Latch lets go, the windows show the value
    /// it latched, as it is heard, not the one the song keeps under the pass.
    #[test]
    fn a_latched_value_shows_as_it_is_heard() {
        use super::{Pass, Phase, Writing};
        let mut s = Studio::sample();
        let t = s
            .project()
            .tracks
            .values()
            .find(|t| !t.id.is_master())
            .unwrap()
            .id;
        assert!(s.run_command("auto.mode", json!({"ids":[t.to_string()],"mode":"latch"})));
        let kept = s.project().tracks[&t].volume_db;
        assert_ne!(kept, -12.0);
        s.arrange.writing = Some(Writing {
            pass: Pass::begin(&s.project(), 0),
            last: 0,
        });
        s.transport.playing = true;
        assert!(s.auto_touch(MixerHit::Fader(t), Phase::Begin, Some(-6.0)));
        s.playhead = 960;
        assert!(s.auto_touch(MixerHit::Fader(t), Phase::Change, Some(-12.0)));
        s.playhead = 1920;
        assert!(s.auto_touch(MixerHit::Fader(t), Phase::End, Some(-12.0)));
        s.playhead = 2880;
        assert_eq!(s.project().tracks[&t].volume_db, kept);
        assert_eq!(s.auto_shown().tracks[&t].volume_db, -12.0);
        s.transport.playing = false;
        s.auto_settle();
    }
    /// A drag on one fader of a selection of two, in each mode: every writer in the selection
    /// gets a lane and keeps its own value in the song; a Read track in it moves as far as the
    /// drag, where one --by takes it (MIX-050), in a step of its own.
    #[test]
    fn a_selections_drag_writes_every_writer_and_moves_a_read_track() {
        let mut failures = Vec::new();
        for (mode, other) in [
            ("touch", "touch"),
            ("latch", "latch"),
            ("write", "write"),
            ("touch", "read"),
            ("latch", "read"),
            ("write", "read"),
        ] {
            let row = format!("{mode} with {other}");
            let mut s = Studio::sample();
            let ids: Vec<_> = s
                .project()
                .tracks
                .values()
                .filter(|t| !t.id.is_master())
                .take(2)
                .map(|t| t.id)
                .collect();
            s.arrange.tracks = ids
                .iter()
                .copied()
                .map(crate::tree::RowKind::Track)
                .collect();
            assert!(s.run_command("auto.mode", json!({"ids":[ids[0].to_string()],"mode":mode})));
            assert!(s.run_command(
                "auto.mode",
                json!({"ids":[ids[1].to_string()],"mode":other})
            ));
            let read = other == "read";
            let base = s.project().tracks[&ids[0]].volume_db;
            let other_base = s.project().tracks[&ids[1]].volume_db;
            let steps = s.song.steps();
            s.drawer_open = true;
            s.tab = DrawerTab::Mixer;
            let g = s.geometry(1600.0, 1000.0);
            begin_pass(&mut s, 0);
            assert!(
                s.drag_begin(&g, Hit::Mixer(MixerHit::Fader(ids[0])), 100.0),
                "{row}"
            );
            s.playhead = 960;
            assert!(s.drag_move(130.0, false), "{row}");
            let value = s.mixer.drag.unwrap().value;
            assert!(s.drag_end(), "{row}");
            // Both move as far as the drag, each where one --by takes it (MIX-050).
            let landed = xirius_core::model::volume_moved(base, value - base);
            let moved = xirius_core::model::volume_moved(other_base, value - base);
            let held = (
                s.project().tracks[&ids[0]].volume_db,
                s.project().tracks[&ids[1]].volume_db,
                s.song.steps() - steps,
            );
            let want = if read {
                (base, moved, 1)
            } else {
                (base, other_base, 0)
            };
            if held != want {
                failures.push(format!("{row}: let go {held:?}, want {want:?}"));
            }
            end_pass(&mut s, 1920);
            let lane = |s: &Studio, id| volume_lane(s, id).map(|l| l.points).unwrap_or_default();
            let ends = s.song.steps() - steps;
            if s.project().tracks[&ids[0]].volume_db != base
                || ends != want.2 + 1
                || !lane(&s, ids[0]).iter().any(|p| p.value == landed)
                || lane(&s, ids[1]).is_empty() == !read
            {
                failures.push(format!(
                    "{row}: {ends} steps, lanes {:?} / {:?}",
                    lane(&s, ids[0]),
                    lane(&s, ids[1])
                ));
            }
            for _ in 0..ends {
                assert!(s.undo(&g), "{row}");
            }
            if s.project().tracks[&ids[1]].volume_db != other_base {
                failures.push(format!("{row}: undo leaves the second track moved"));
            }
        }
        assert!(failures.is_empty(), "a selection's drag: {failures:#?}");
    }
    #[test]
    fn the_xy_pad_writes_both_knobs_lanes_and_a_read_lane_holds_it() {
        use super::{Pass, Writing};
        use xirius_core::controls::ControlId;
        let mut s = Studio::sample();
        s.instruments.open.clear();
        let t = s
            .project()
            .tracks
            .values()
            .find(|t| !t.id.is_master())
            .unwrap()
            .id;
        assert!(s.run_command("auto.mode", json!({"ids":[t.to_string()],"mode":"latch"})));
        s.open_macros(t);
        let pad = s.macro_view().unwrap().pad;
        s.arrange.writing = Some(Writing {
            pass: Pass::begin(&s.project(), 0),
            last: 0,
        });
        s.playhead = 0;
        s.transport.playing = true;
        assert!(s.macro_press(pad.x + pad.w * 0.25, pad.y + pad.h * 0.5));
        s.playhead = 960;
        assert!(s.macro_moved(pad.x + pad.w * 0.75, pad.y + pad.h * 0.25));
        s.playhead = 1920;
        assert!(s.macro_up());
        s.transport.playing = false;
        s.auto_settle();
        let knobs = [ControlId::Knob(0), ControlId::Knob(1)];
        for k in knobs {
            assert!(
                s.project().tracks[&t]
                    .automation
                    .iter()
                    .any(|l| l.target == AutoTarget::Control(k) && !l.points.is_empty()),
                "{k}"
            );
        }
        // Read: the lanes hold the knobs, and the pad does not move by hand
        assert!(s.run_command("auto.mode", json!({"ids":[t.to_string()],"mode":"read"})));
        let steps = s.song().steps();
        assert!(s.macro_press(pad.x + 4.0, pad.bottom() - 4.0));
        assert!(!s.macro_up(), "nothing was held");
        s.macro_pad.as_mut().unwrap().focused = true;
        assert!(s.macro_key("right", crate::tools::Mods::default()));
        assert_eq!(s.song().steps(), steps);
    }
}
