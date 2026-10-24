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
        heard
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
    use xirius_core::id::TrackId;
    use xirius_session::output::OutputMode;

    use crate::mixer::MixerHit;
    use crate::studio::{DrawerTab, Geometry, Hit, Studio};

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

    #[test]
    fn touch_writes_while_the_fader_is_held_in_one_step() {
        let (mut s, g, t) = studio("Keys", "touch");
        let own = s.project().tracks[&t].volume_db;
        let steps = s.song().steps();
        start(&mut s);
        let low = drag_fader(&mut s, &g, t, 40.0);
        assert!(low < own - 1.0, "{low} < {own}");
        assert!(s.drag_end());
        play_for(&mut s, 300);
        stop(&mut s);

        // One step; the fader's own value is untouched.
        assert_eq!(s.song().steps(), steps + 1);
        assert_eq!(s.project().tracks[&t].volume_db, own);
        let lane = volume_lane(&s, t).expect("a volume lane");
        let pts = &lane.points;
        assert!(pts.len() >= 2, "{pts:?}");
        assert!(pts.iter().any(|p| (p.value - low).abs() < 1e-6), "{pts:?}");
        // Let go, the lane goes back to where it was (the fader's value).
        assert!((pts.last().unwrap().value - own).abs() < 1e-6, "{pts:?}");

        // One undo takes the whole pass back.
        assert!(s.undo(&g));
        assert!(volume_lane(&s, t).is_none_or(|l| l.points.is_empty()));
    }

    #[test]
    fn a_muted_send_records_touch_latch_and_write_without_becoming_audible() {
        for mode in ["touch", "latch", "write"] {
            let (mut s, g, t) = studio("Keys", mode);
            // Undo here must land exactly on the state the test kept; the
            // instrument-input arm that would follow the selection is not
            // what this test checks (REC-024).
            s.rec_set(
                "prefs.set",
                serde_json::json!({"key": "midi.follow-selection", "value": "false"}),
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
            let before = s.project();
            let steps = s.song().steps();
            start(&mut s);
            let hit = MixerHit::SendLevel(t, id);
            let (_, y) = s.mixer_center(&g, hit).unwrap();
            assert!(s.drag_begin(&g, Hit::Mixer(hit), y));
            assert!(s.drag_move(y + 30.0, false));
            let value = s.mixer.drag.unwrap().value;
            assert!(value < -9.0);
            let heard = s.auto_heard(&s.project());
            assert_eq!(
                heard.tracks[&t].sends[0].level_db,
                xirius_core::model::MIN_DB
            );
            assert_eq!(heard.tracks[&t].sends[0].muted_level, Some(-9.0));
            assert!(s.drag_end());
            play_for(&mut s, 100);
            assert_eq!(
                s.auto_heard(&s.project()).tracks[&t].sends[0].level_db,
                xirius_core::model::MIN_DB
            );
            stop(&mut s);
            let p = s.project();
            let lane = p.tracks[&t]
                .automation
                .iter()
                .find(|l| l.target == AutoTarget::Send(id))
                .unwrap();
            assert!(lane.suspended);
            assert!(lane.points.iter().any(|pt| (pt.value - value).abs() < 1e-6));
            if mode == "touch" {
                assert_eq!(lane.points.last().unwrap().value, -9.0);
            }
            assert_eq!(p.tracks[&t].sends, before.tracks[&t].sends);
            assert_eq!(s.song().steps(), steps + 1);
            assert!(s.undo(&g));
            let mut restored = s.project().as_ref().clone();
            restored.next_id = before.next_id;
            assert_eq!(restored, *before);
        }
    }

    #[test]
    fn latch_keeps_the_value_it_was_let_go_at_until_stop() {
        let (mut s, g, t) = studio("Keys", "latch");
        let own = s.project().tracks[&t].volume_db;
        start(&mut s);
        let low = drag_fader(&mut s, &g, t, 40.0);
        s.drag_end();
        let let_go = s.playhead;
        play_for(&mut s, 400);
        let end = s.playhead;
        assert!(end > let_go + 100, "{end} > {let_go}");
        stop(&mut s);

        let lane = volume_lane(&s, t).expect("a volume lane");
        let last = *lane
            .points
            .iter()
            .rev()
            .find(|p| (p.value - low).abs() < 1e-6)
            .unwrap();
        assert!((last.value - low).abs() < 1e-6, "{last:?}");
        assert!(last.tick >= end - 30, "{} >= {end}", last.tick);
        assert_eq!(s.project().tracks[&t].volume_db, own);
    }

    #[test]
    fn write_covers_the_pass_and_read_writes_nothing() {
        let (mut s, g, t) = studio("Keys", "write");
        // A lane already there is written over from where play starts; one
        // that is not there yet is made only by touching its control.
        let own = s.project().tracks[&t].volume_db;
        let far = i64::from(s.project().ppq) * 64;
        assert!(s.run_command(
            "auto.point.add",
            json!({ "track": t.to_string(), "target": "volume", "at": far, "value": -30.0 })
        ));
        let from = s.playhead;
        start(&mut s);
        play_for(&mut s, 300);
        let end = s.playhead;
        stop(&mut s);
        let lane = volume_lane(&s, t).expect("a volume lane");
        let first = lane.points.iter().find(|p| p.tick == from).unwrap();
        assert_eq!((first.tick, first.value), (from, own));
        assert!(lane.value_at(end as f64 - 30.0) == Some(own));
        assert!(
            lane.points.iter().any(|p| p.tick == far),
            "{:?}",
            lane.points
        );
        assert!(
            s.project().tracks[&t]
                .automation
                .iter()
                .all(|l| l.target == AutoTarget::Volume)
        );
        assert!(s.undo(&g));

        // Read: the drag goes to the fader as before.
        assert!(s.undo(&g));
        assert!(s.run_command(
            "auto.mode",
            json!({ "ids": [t.to_string()], "mode": "read" })
        ));
        let steps = s.song().steps();
        start(&mut s);
        let low = drag_fader(&mut s, &g, t, 40.0);
        s.drag_end();
        stop(&mut s);
        assert_eq!(s.song().steps(), steps + 1);
        assert!(volume_lane(&s, t).is_none_or(|l| l.points.is_empty()));
        assert_eq!(s.project().tracks[&t].volume_db, low);
    }
    #[test]
    fn all_selected_controls_write_in_touch_latch_and_write_modes() {
        use super::{Pass, Phase, Writing};
        for mode in ["touch", "latch", "write"] {
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
            assert!(s.run_command(
                "auto.mode",
                json!({"ids":ids.iter().map(ToString::to_string).collect::<Vec<_>>(),"mode":mode})
            ));
            s.arrange.writing = Some(Writing {
                pass: Pass::begin(&s.project(), 0),
                last: 0,
            });
            s.transport.playing = true;
            assert!(s.auto_touch(MixerHit::Fader(ids[0]), Phase::Begin, Some(-6.0)));
            s.playhead = 960;
            assert!(s.auto_touch(MixerHit::Fader(ids[0]), Phase::Change, Some(-12.0)));
            s.playhead = 1920;
            assert!(s.auto_touch(MixerHit::Fader(ids[0]), Phase::End, Some(-12.0)));
            s.transport.playing = false;
            s.auto_settle();
            for id in ids {
                assert!(
                    s.project().tracks[&id]
                        .automation
                        .iter()
                        .any(|l| l.target == AutoTarget::Volume && !l.points.is_empty()),
                    "{mode}: {id}"
                );
            }
        }
    }
    #[test]
    fn mixed_read_and_writing_selection_keeps_the_writers_base_value() {
        use super::{Pass, Writing};
        for mode in ["touch", "latch", "write"] {
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
                json!({"ids":[ids[1].to_string()],"mode":"read"})
            ));
            let base = s.project().tracks[&ids[0]].volume_db;
            let read_base = s.project().tracks[&ids[1]].volume_db;
            let steps = s.song.steps();
            s.drawer_open = true;
            s.tab = DrawerTab::Mixer;
            let g = s.geometry(1600.0, 1000.0);
            s.arrange.writing = Some(Writing {
                pass: Pass::begin(&s.project(), 0),
                last: 0,
            });
            s.playhead = 0;
            s.transport.playing = true;
            assert!(s.drag_begin(&g, Hit::Mixer(MixerHit::Fader(ids[0])), 100.0));
            s.playhead = 960;
            assert!(s.drag_move(130.0, false));
            let value = s.mixer.drag.unwrap().value;
            assert!(s.drag_end());
            assert_eq!(s.project().tracks[&ids[0]].volume_db, base);
            // Both move as far as the drag, each where one --by takes it (MIX-050).
            let landed = xirius_core::model::volume_moved(base, value - base);
            let moved = xirius_core::model::volume_moved(read_base, value - base);
            assert_eq!(s.project().tracks[&ids[1]].volume_db, moved);
            assert_eq!(s.song.steps(), steps + 1);
            s.playhead = 1920;
            s.transport.playing = false;
            s.auto_settle();
            assert_eq!(s.project().tracks[&ids[0]].volume_db, base);
            assert_eq!(s.song.steps(), steps + 2);
            assert!(
                volume_lane(&s, ids[0])
                    .unwrap()
                    .points
                    .iter()
                    .any(|p| p.value == landed)
            );
            assert!(s.undo(&g));
            assert!(s.undo(&g));
            assert_eq!(s.project().tracks[&ids[1]].volume_db, read_base);
        }
    }
}
