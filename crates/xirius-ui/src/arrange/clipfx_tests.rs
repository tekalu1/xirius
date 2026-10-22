//! Fades, gain, crossfades, slip and the boundary of two clips, taken hold of
//! with the pointer: a press at a handle, a drag, one history step, and the
//! clip as the engine will play it (EDT-002, 017, 036, 037, 039, 040).

use serde_json::json;
use xirius_core::clipfx::ClipFx;

use super::clipfx::{FxPart, FxView, Handles, Side, db_per_px, db_y};
use super::probe::hold;
use super::*;
use crate::tools::SnapMode;

const BAR: Tick = 3840;

struct Rig {
    s: Studio,
    g: Geometry,
    track: TrackId,
    /// Clips added with [`Rig::clip`], in order.
    ids: Vec<ClipId>,
}

fn rig() -> Rig {
    let mut s = Studio::sample();
    s.snap_mode = SnapMode::Bar;
    s.drawer_open = false;
    s.beat_w = 40.0;
    let mut g = s.geometry(1440.0, 1000.0);
    s.resized(&g);
    let made = s
        .arrange_run("track.add.audio", json!({ "name": "Fx" }))
        .expect("a track");
    let track: TrackId = made["id"]
        .as_str()
        .or_else(|| made["ids"][0].as_str())
        .expect("its id")
        .parse()
        .unwrap();
    g = s.geometry(1440.0, 1000.0);
    s.resized(&g);
    Rig {
        s,
        g,
        track,
        ids: Vec::new(),
    }
}

impl Rig {
    /// An audio clip `at` ticks, `length` long, on the track.
    fn clip(&mut self, at: Tick, length: Tick) -> ClipId {
        let made = self
            .s
            .arrange_run(
                "clip.add.audio",
                json!({ "track": self.track.to_string(), "at": at, "length": length,
                        "file": "x.wav" }),
            )
            .expect("a clip");
        let id: ClipId = made["id"]
            .as_str()
            .or_else(|| made["ids"][0].as_str())
            .expect("its id")
            .parse()
            .unwrap();
        self.ids.push(id);
        self.g = self.s.geometry(1440.0, 1000.0);
        self.s.resized(&self.g);
        id
    }

    fn run(&mut self, command: &str, args: serde_json::Value) {
        self.s
            .arrange_run(command, args)
            .unwrap_or_else(|| panic!("{command}: {:?}", self.s.error));
        self.g = self.s.geometry(1440.0, 1000.0);
        self.s.resized(&self.g);
    }

    fn rect(&self, id: ClipId) -> Rect {
        let p = self.s.project();
        let clip = p.tracks[&self.track]
            .clips
            .iter()
            .find(|c| c.id == id)
            .unwrap();
        let row = *self.s.tree().row_of(RowKind::Track(self.track)).unwrap();
        self.s.clip_rect(&self.g, &row, clip)
    }

    fn view(&self, id: ClipId) -> FxView {
        let p = self.s.project();
        let track = &p.tracks[&self.track];
        let clip = track.clips.iter().find(|c| c.id == id).unwrap();
        FxView::of(&p, track, clip).unwrap()
    }

    fn handles(&self, id: ClipId) -> Handles {
        self.view(id).handles(&self.s, self.rect(id))
    }

    fn fx(&self, id: ClipId) -> ClipFx {
        self.s.project().fx(id).cloned().unwrap_or_default()
    }

    fn clip_of(&self, id: ClipId) -> xirius_core::model::Clip {
        self.s.project().tracks[&self.track]
            .clips
            .iter()
            .find(|c| c.id == id)
            .unwrap()
            .clone()
    }

    fn gain(&self, id: ClipId) -> f64 {
        match self.clip_of(id).body {
            ClipBody::Audio { gain_db, .. } => gain_db,
            _ => panic!("not audio"),
        }
    }

    fn offset(&self, id: ClipId) -> Tick {
        match self.clip_of(id).body {
            ClipBody::Audio { offset, .. } => offset,
            _ => panic!("not audio"),
        }
    }

    fn steps(&self) -> usize {
        self.s.song.steps()
    }

    fn spot(&self, at: (f32, f32)) -> Spot {
        self.s.arrange_spot(&self.g, at.0, at.1)
    }

    fn drag(&mut self, from: (f32, f32), to: (f32, f32), mods: Mods) {
        hold(&mut self.s, &self.g, from, to, mods);
        self.s.arrange_up(&self.g, to.0, to.1, mods);
    }

    fn click(&mut self, at: (f32, f32), mods: Mods) {
        self.s.arrange_down(&self.g, at.0, at.1, mods);
        self.s.arrange_up(&self.g, at.0, at.1, mods);
    }

    fn bars(&self, n: f32) -> f32 {
        self.s.ticks_to_px(BAR) * n
    }

    fn select(&mut self, ids: &[ClipId]) {
        self.s.arrange.clips = ids.iter().map(|c| (self.track, *c)).collect();
        self.s.selected_clip = ids.first().map(|c| (self.track, *c));
    }
}

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

fn none() -> Mods {
    Mods::default()
}

fn shift() -> Mods {
    Mods {
        shift: true,
        ..Mods::default()
    }
}

fn alt() -> Mods {
    Mods {
        alt: true,
        ..Mods::default()
    }
}

fn ctrl_alt() -> Mods {
    Mods {
        ctrl: true,
        alt: true,
        ..Mods::default()
    }
}

#[test]
fn the_handles_are_found_where_they_are_drawn() {
    let mut r = rig();
    let a = r.clip(2 * BAR, 2 * BAR);
    r.run(
        "clip.fade.set",
        json!({ "ids": [a.to_string()], "in": BAR / 2, "out": BAR / 2 }),
    );
    let h = r.handles(a);
    let at = |x: f32, y: f32| r.spot((x, y));
    let on = |part| Spot::Fx {
        track: r.track,
        clip: a,
        part,
    };
    let f = h.flag_in.unwrap();
    assert_eq!(at(f.x + 4.0, f.y + 4.0), on(FxPart::Flag(Side::In)));
    let f = h.flag_out.unwrap();
    assert_eq!(at(f.x + 4.0, f.y + 4.0), on(FxPart::Flag(Side::Out)));
    let m = h.mid_in.unwrap();
    assert_eq!(at(m.0, m.1), on(FxPart::Mid(Side::In)));
    let m = h.mid_out.unwrap();
    assert_eq!(at(m.0, m.1), on(FxPart::Mid(Side::Out)));
    assert_eq!(at(h.gain.0, h.gain.1), on(FxPart::Gain));
    // The body, away from every handle, is still the clip's.
    let rect = r.rect(a);
    let body = (rect.x + rect.w * 0.3, rect.y + rect.h * 0.6);
    assert!(
        matches!(
            at(body.0, body.1),
            Spot::Clip {
                grip: Grip::Body,
                ..
            }
        ),
        "{:?}",
        at(body.0, body.1)
    );
}

#[test]
fn dragging_the_fade_in_flag_sets_the_fade_in_one_step_and_undoes() {
    let mut r = rig();
    let a = r.clip(2 * BAR, 2 * BAR);
    let n = r.steps();
    let f = r.handles(a).flag_in.unwrap();
    let from = centre(f);
    let to = (from.0 + r.bars(1.0), from.1);
    hold(&mut r.s, &r.g, from, to, none());
    // Held: the clip shows the fade, and the balloon says how long.
    let plan = r.s.plan(&r.g, r.s.arrange.drag.as_ref().unwrap()).unwrap();
    assert!(matches!(plan.preview, Preview::Fx(_)), "{:?}", plan.preview);
    assert!(plan.tip.contains("4.00"), "{}", plan.tip);
    r.s.arrange_up(&r.g, to.0, to.1, none());
    assert_eq!(r.fx(a).fade_in.length, BAR);
    assert_eq!(r.steps(), n + 1);
    // The handle moved with the fade.
    assert!(r.handles(a).flag_in.unwrap().x > f.x + r.bars(0.9));
    r.s.undo(&r.g);
    assert_eq!(r.fx(a).fade_in.length, 0);
}

#[test]
fn dragging_the_fade_out_flag_left_sets_the_fade_out() {
    let mut r = rig();
    let a = r.clip(2 * BAR, 2 * BAR);
    let f = r.handles(a).flag_out.unwrap();
    let from = centre(f);
    r.drag(from, (from.0 - r.bars(1.0), from.1), none());
    assert_eq!(r.fx(a).fade_out.length, BAR);
    assert_eq!(r.fx(a).fade_in.length, 0);
}

#[test]
fn a_flag_dragged_past_the_clip_stops_at_its_length() {
    let mut r = rig();
    let a = r.clip(2 * BAR, 2 * BAR);
    let from = centre(r.handles(a).flag_in.unwrap());
    r.drag(from, (from.0 + r.bars(5.0), from.1), none());
    assert_eq!(r.fx(a).fade_in.length, 2 * BAR);
}

#[test]
fn fade_flags_can_be_dragged_again_in_both_directions_and_undone() {
    for (linked, side) in [
        (false, Side::In),
        (false, Side::Out),
        (true, Side::In),
        (true, Side::Out),
    ] {
        let mut r = rig();
        r.run("view.link", json!({ "on": linked, "off": !linked }));
        let a = r.clip(BAR, 4 * BAR);
        let n = r.steps();
        for (step, bars) in [1.0, 1.0, -1.0, -1.0].into_iter().enumerate() {
            let h = r.handles(a);
            let flag = match side {
                Side::In => h.flag_in.unwrap(),
                Side::Out => h.flag_out.unwrap(),
            };
            let from = centre(flag);
            let dx = r.bars(bars) * if side == Side::In { 1.0 } else { -1.0 };
            r.drag(from, (from.0 + dx, from.1), none());
            let expected = [BAR, 2 * BAR, BAR, 0][step];
            let fx = r.fx(a);
            let fade = if side == Side::In {
                fx.fade_in
            } else {
                fx.fade_out
            };
            assert_eq!(
                fade.length, expected,
                "{side:?}, step {step}: {:?}",
                r.s.error
            );
            assert_eq!(r.steps(), n + step + 1);
        }
        r.s.undo(&r.g);
        let fx = r.fx(a);
        assert_eq!(
            if side == Side::In {
                fx.fade_in.length
            } else {
                fx.fade_out.length
            },
            BAR
        );
    }
}

#[test]
fn a_fade_curve_is_shaped_by_the_mid_square_and_shift_also_moves_its_length() {
    let mut r = rig();
    r.run("view.link", json!({ "on": true }));
    let a = r.clip(2 * BAR, 2 * BAR);
    r.run(
        "clip.fade.set",
        json!({ "ids": [a.to_string()], "in": BAR }),
    );
    let n = r.steps();
    let m = r.handles(a).mid_in.unwrap();
    r.drag(m, (m.0, m.1 - 20.0), none());
    let up = r.fx(a).fade_in;
    assert!(up.curve > 0.0, "{up:?}");
    assert_eq!(up.length, BAR, "up and down leave the length");
    assert_eq!(r.steps(), n + 1);
    // Down the other way.
    let m = r.handles(a).mid_in.unwrap();
    r.drag(m, (m.0, m.1 + 60.0), none());
    assert!(r.fx(a).fade_in.curve < up.curve);
    // Shift: the length follows the sideways move as well.
    let m = r.handles(a).mid_in.unwrap();
    r.drag(m, (m.0 + r.bars(0.5), m.1), shift());
    assert!(r.fx(a).fade_in.length > BAR, "{:?}", r.fx(a).fade_in);
}

#[test]
fn the_volume_square_sets_the_gain_of_every_selected_clip_and_alt_of_one() {
    let mut r = rig();
    let a = r.clip(BAR, BAR);
    let b = r.clip(3 * BAR, BAR);
    r.select(&[a, b]);
    let n = r.steps();
    let sq = r.handles(a).gain;
    let per = db_per_px(r.rect(a));
    r.drag(sq, (sq.0, sq.1 - 10.0), none());
    let want = (10.0 * per * 10.0).round() / 10.0;
    assert!((r.gain(a) - want).abs() < 0.11, "{}", r.gain(a));
    assert!((r.gain(b) - want).abs() < 0.11, "{}", r.gain(b));
    assert_eq!(r.steps(), n + 1, "one drag, one step");
    // Alt: just the one that was taken.
    let before = r.gain(b);
    let sq = r.handles(a).gain;
    r.drag(sq, (sq.0, sq.1 + 10.0), alt());
    assert!(r.gain(a) < want - 1.0);
    assert_eq!(r.gain(b), before);
    r.s.undo(&r.g);
    r.s.undo(&r.g);
    assert_eq!(r.gain(a), 0.0);
}

#[test]
fn a_click_on_the_gain_line_adds_a_point_only_to_a_clip_already_selected() {
    let mut r = rig();
    let a = r.clip(BAR, 2 * BAR);
    let b = r.clip(4 * BAR, 2 * BAR);
    r.select(&[b]);
    let rect = r.rect(a);
    let on_line = (rect.x + rect.w * 0.3, db_y(rect, 0.0));
    assert!(matches!(
        r.spot(on_line),
        Spot::Fx {
            part: FxPart::Line { armed: false },
            ..
        }
    ));
    r.click(on_line, none());
    assert!(r.fx(a).points.is_empty(), "the first click only selects");
    assert!(r.s.arrange.clips.contains(&(r.track, a)));
    let n = r.steps();
    r.click(on_line, none());
    let pts = r.fx(a).points;
    assert_eq!(pts.len(), 1, "{pts:?}");
    assert_eq!(r.steps(), n + 1);
    // It sits where the click was.
    let at = r.s.px_to_ticks(on_line.0 - rect.x);
    assert!(
        (pts[0].tick - at).abs() < BAR / 8,
        "{} vs {at}",
        pts[0].tick
    );
}

#[test]
fn a_gain_point_is_dragged_and_alt_press_erases_it() {
    let mut r = rig();
    let a = r.clip(BAR, 2 * BAR);
    r.run(
        "clip.gain.point.add",
        json!({ "id": a.to_string(), "at": BAR, "db": 0.0 }),
    );
    r.select(&[a]);
    let n = r.steps();
    let p0 = r.handles(a).points[0];
    assert!(matches!(
        r.spot(p0),
        Spot::Fx {
            part: FxPart::Point(0),
            ..
        }
    ));
    r.drag(p0, (p0.0 + r.bars(0.5), p0.1 - 12.0), shift());
    let moved = r.fx(a).points[0];
    assert!(moved.db > 1.0, "{moved:?}");
    assert!(moved.tick > BAR, "{moved:?}");
    assert_eq!(r.steps(), n + 1);
    let p1 = r.handles(a).points[0];
    r.click(p1, alt());
    assert!(r.fx(a).points.is_empty());
    r.s.undo(&r.g);
    assert_eq!(r.fx(a).points.len(), 1);
}

#[test]
fn x_makes_a_crossfade_of_the_overlap_and_the_centre_drags_it() {
    let mut r = rig();
    let a = r.clip(BAR, 2 * BAR);
    let b = r.clip(2 * BAR + BAR / 2, 2 * BAR);
    r.select(&[b]);
    r.s.arrange.focus = true;
    let n = r.steps();
    assert!(r.s.arrange_key(&r.g, "x", none()));
    assert!(r.fx(b).cross.is_some());
    assert_eq!(r.steps(), n + 1);
    assert!(r.handles(b).cross.is_some(), "the centre shows");
    // Sideways: the crossfade moves, the join of the two clips with it.
    let c = r.handles(b).cross.unwrap();
    let before = (r.clip_of(a).length, r.clip_of(b).start);
    assert!(matches!(
        r.spot(c),
        Spot::Fx {
            part: FxPart::Cross,
            ..
        }
    ));
    r.drag(c, (c.0 + r.bars(0.5), c.1), none());
    let after = (r.clip_of(a).length, r.clip_of(b).start);
    assert_ne!(before, after, "{after:?}");
    assert_eq!(r.steps(), n + 2);
    // Up: the shape.
    let curve = r.fx(b).cross.unwrap().curve;
    let c = r.handles(b).cross.unwrap();
    r.drag(c, (c.0, c.1 - 20.0), none());
    assert!(r.fx(b).cross.unwrap().curve > curve);
}

#[test]
fn shift_x_gives_the_ends_of_the_selected_clips_a_short_fade() {
    let mut r = rig();
    let a = r.clip(BAR, 2 * BAR);
    r.select(&[a]);
    r.s.arrange.focus = true;
    assert!(r.s.arrange_key(&r.g, "x", shift()));
    let fx = r.fx(a);
    assert!(fx.fade_in.length > 0 && fx.fade_out.length > 0, "{fx:?}");
}

#[test]
fn dragging_the_join_of_two_touching_clips_moves_both_ends_in_one_step() {
    let mut r = rig();
    let a = r.clip(BAR, 2 * BAR);
    let b = r.clip(3 * BAR, 2 * BAR);
    let n = r.steps();
    let rect = r.rect(a);
    let join = (rect.right(), rect.y + rect.h * 0.7);
    assert!(matches!(
        r.spot(join),
        Spot::Fx {
            part: FxPart::Boundary(id),
            ..
        } if id == b
    ));
    r.drag(join, (join.0 + r.bars(1.0), join.1), none());
    assert_eq!(r.clip_of(a).end(), 4 * BAR);
    assert_eq!(r.clip_of(b).start, 4 * BAR);
    assert_eq!(r.clip_of(b).end(), 5 * BAR, "the right clip's end stays");
    assert_eq!(r.steps(), n + 1);
    r.s.undo(&r.g);
    assert_eq!(r.clip_of(a).end(), 3 * BAR);
    assert_eq!(r.clip_of(b).start, 3 * BAR);
}

#[test]
fn ctrl_alt_drag_slips_the_audio_inside_the_clip() {
    let mut r = rig();
    let a = r.clip(2 * BAR, 2 * BAR);
    r.run("clip.slip", json!({ "ids": [a.to_string()], "by": -BAR }));
    assert_eq!(r.offset(a), BAR);
    let n = r.steps();
    let before = r.clip_of(a);
    let from = centre(r.rect(a));
    r.drag(from, (from.0 + r.bars(0.5), from.1), ctrl_alt());
    let after = r.clip_of(a);
    assert_eq!((after.start, after.length), (before.start, before.length));
    assert!((r.offset(a) - BAR / 2).abs() <= 8, "{}", r.offset(a));
    assert_eq!(r.steps(), n + 1);
    // Back past the start of the file stops there.
    r.drag(from, (from.0 + r.bars(3.0), from.1), ctrl_alt());
    assert_eq!(r.offset(a), 0);
}

#[test]
fn other_tools_treat_the_handles_as_the_clip() {
    let mut r = rig();
    let a = r.clip(2 * BAR, 2 * BAR);
    r.s.tool = Tool::Eraser;
    let f = centre(r.handles(a).flag_in.unwrap());
    r.click(f, none());
    assert!(r.s.project().tracks[&r.track].clips.is_empty());
}

#[test]
fn a_clip_too_narrow_for_handles_keeps_its_edges() {
    let mut r = rig();
    let a = r.clip(2 * BAR, BAR / 64);
    let rect = r.rect(a);
    assert!(rect.w < super::clipfx::MIN_W);
    let at = (rect.x + rect.w / 2.0, rect.y + 3.0);
    assert!(!matches!(r.spot(at), Spot::Fx { .. }), "{:?}", r.spot(at));
}
