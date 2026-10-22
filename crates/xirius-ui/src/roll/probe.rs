//! Repeatable drag measurement on the production roll, with no audio output.
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;
use gpui::{
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput, point, px,
};
use serde_json::json;
use std::path::PathBuf;
use std::time::Instant;

pub struct Bench {
    out: PathBuf,
    warm: usize,
    drag_warm: usize,
    drag_frames: usize,
    last: Option<Instant>,
    samples: Vec<f64>,
    paints: Vec<f64>,
    roll_paints: Vec<f64>,
    press: Option<(f32, f32)>,
    steps: usize,
}

impl Bench {
    pub fn new(out: PathBuf) -> Self {
        Self {
            out,
            warm: 40,
            drag_warm: crate::bench::WARMUP_FRAMES,
            drag_frames: 0,
            last: None,
            samples: Vec::new(),
            paints: Vec::new(),
            roll_paints: Vec::new(),
            press: None,
            steps: 0,
        }
    }

    pub fn prepare(s: &mut Studio) {
        let g = s.geometry(1440.0, 820.0);
        let c = s.roll_ctx(&g).expect("MIDI clip");
        let ids = super::ids_json(c.notes().iter().map(|n| &n.id));
        s.song
            .run("note.delete", json!({"ids":ids}))
            .expect("clear fixture");
        let notes: Vec<_> = (0..10_000)
            .map(|i| json!({"at":(i/48)*60,"pitch":40+i%48,"length":45,"velocity":90}))
            .collect();
        s.song
            .run(
                "note.put",
                json!({"clip":c.clip.id.to_string(),"notes":notes}),
            )
            .expect("10k notes");
        s.resized(&g);
    }

    /// Called once before each real GPUI frame. Returns input to dispatch.
    pub fn tick(&mut self, s: &mut Studio, g: &Geometry) -> (Vec<PlatformInput>, bool) {
        if self.warm > 0 {
            self.warm -= 1;
            return (Vec::new(), false);
        }
        let now = Instant::now();
        if self.press.is_some() {
            self.drag_frames += 1;
        }
        if self.drag_warm > 0 && self.press.is_some() {
            self.drag_warm -= 1;
        } else if let Some(last) = self.last {
            self.samples
                .push(now.duration_since(last).as_secs_f64() * 1000.0);
            self.paints.push(s.roll.paint_us.replace(0) as f64 / 1000.0);
            self.roll_paints
                .push(s.roll.roll_paint_us.get() as f64 / 1000.0);
        }
        self.last = Some(now);
        let c = s.roll_ctx(g).expect("roll");
        if self.press.is_none() {
            s.roll.row_h = 20.0;
            s.roll.beat_px = 96.0;
            s.roll.center(&c);
            s.roll.key(&c, "a", Mods::ctrl());
            let v = s.roll.view(&c);
            let n = c
                .notes()
                .iter()
                .find(|n| {
                    v.y(n.pitch) > v.grid.y + 20.0
                        && v.y(n.pitch) < v.grid.bottom() - 20.0
                        && n.start > 960
                })
                .expect("visible note");
            let p = (
                v.x(n.start) + v.px(n.length) / 2.0,
                v.y(n.pitch) + v.row_h / 2.0,
            );
            self.press = Some(p);
            self.steps = s.song.steps();
            return (
                vec![PlatformInput::MouseDown(MouseDownEvent {
                    button: MouseButton::Left,
                    position: point(px(p.0), px(p.1 + xirius_design::Layout::TOP_BAR_H)),
                    modifiers: Modifiers::default(),
                    click_count: 1,
                    first_mouse: false,
                })],
                false,
            );
        }
        let p = self.press.unwrap();
        if self.samples.len() >= 600 {
            let summary = crate::frame::Distribution::from_samples(&self.samples);
            let criteria = crate::frame::Criteria::FPS_60;
            let fps = 1000.0 / summary.mean;
            let active = s.roll.selected.len() == 10_000
                && s.roll.preview.changed.len() == 10_000
                && s.song.steps() == self.steps;
            let data = json!({"notes":c.notes().len(),"frames":self.samples.len(),"warmup_drag_frames":crate::bench::WARMUP_FRAMES,"mean_ms":summary.mean,"fps":fps,"p95_ms":summary.p95,"p99_ms":summary.p99,"paint_mean_ms":self.paints.iter().sum::<f64>()/self.paints.len() as f64,"roll_paint_mean_ms":self.roll_paints.iter().sum::<f64>()/self.roll_paints.len() as f64,"history_during_drag":s.song.steps()-self.steps,"selected":s.roll.selected.len(),"preview_notes":s.roll.preview.changed.len(),"criteria":{"min_fps":criteria.min_fps,"max_p99_ms":criteria.max_p99_ms},"pass_60fps":active && fps>=criteria.min_fps && summary.p99<=criteria.max_p99_ms});
            let v = s.roll.view(&c);
            let visible = c
                .notes()
                .iter()
                .filter(|n| {
                    let r = super::notes::note_rect(&v, &s.roll.shown(n));
                    r.right() >= v.grid.x
                        && r.x < v.grid.right()
                        && r.bottom() >= v.grid.y
                        && r.y < v.grid.bottom()
                })
                .count();
            let mut data = data;
            data["visible_notes_at_last_frame"] = json!(visible);
            data["beat_px"] = json!(s.roll.beat_px);
            data["row_h"] = json!(s.roll.row_h);
            data["note_paint_last_us"] = json!(s.roll.note_paint_us.get());
            data["nonoverlapping_note_batch"] = json!(s.roll.disjoint_notes.get());
            std::fs::create_dir_all(&self.out).expect("bench output");
            std::fs::write(
                self.out.join("roll-bench.json"),
                serde_json::to_vec_pretty(&data).unwrap(),
            )
            .expect("bench report");
            println!("{data}");
            return (
                vec![PlatformInput::MouseUp(MouseUpEvent {
                    button: MouseButton::Left,
                    position: point(px(p.0 + 24.0), px(p.1 + xirius_design::Layout::TOP_BAR_H)),
                    modifiers: Modifiers::default(),
                    click_count: 1,
                })],
                true,
            );
        }
        let phase = self.drag_frames as f32 / 30.0;
        (
            vec![PlatformInput::MouseMove(MouseMoveEvent {
                position: point(
                    px(p.0 + 24.0 + phase.sin() * 20.0),
                    px(p.1 + phase.cos() * 14.0 + xirius_design::Layout::TOP_BAR_H),
                ),
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::default(),
            })],
            false,
        )
    }
}

/// States used by the screenshot driver, through the production input methods.
pub fn shot(s: &mut Studio, g: &Geometry, stage: u8) {
    match stage {
        0 => {
            let c = open_keys(s, g);
            let v = s.roll.view(&c);
            let n = c
                .notes()
                .iter()
                .find(|n| v.y(n.pitch) >= v.grid.y && v.y(n.pitch) + v.row_h < v.grid.bottom())
                .unwrap();
            let p = (
                v.x(n.start) + v.px(n.length) / 2.0,
                v.y(n.pitch) + v.row_h / 2.0,
            );
            let out = s.roll.down(&c, p.0, p.1, Mods::NONE, 1);
            s.apply_roll(g, out);
            let out = s.roll.moved(&c, p.0 + 48.0, p.1 - 14.0, Mods::NONE);
            s.apply_roll(g, out);
        }
        1 => {
            let c = s.roll_ctx(g).unwrap();
            let out = s.roll.mods_changed(&c, Mods::alt());
            s.apply_roll(g, out);
        }
        2 => {
            s.roll.cancel();
            let c = s.roll_ctx(g).unwrap();
            for kind in [
                xirius_core::model::CtrlKind::PitchBend,
                xirius_core::model::CtrlKind::Cc(1),
            ] {
                s.song.run("cc.replace",json!({"clip":c.clip.id.to_string(),"lane":kind.key(),"from":0,"to":4000,"points":[{"tick":0,"value":0},{"tick":1000,"value":60},{"tick":3000,"value":100}]})).unwrap();
            }
            s.resized(g);
        }
        3 => {
            s.roll.menu = Some(super::Menu::Add);
        }
        4 => {
            s.roll.menu = None;
            s.roll.lanes_folded = true;
        }
        5 => {
            s.roll.lanes_folded = false;
            let c = s.roll_ctx(g).unwrap();
            s.roll.center(&c);
            let v = s.roll.view(&c);
            let n = c
                .notes()
                .iter()
                .find(|n| v.y(n.pitch) >= v.grid.y && v.y(n.pitch) + v.row_h < v.grid.bottom())
                .unwrap();
            s.scene_context(g, v.x(n.start) + 10.0, v.y(n.pitch) + v.row_h / 2.0);
        }
        // The roll as it opens, for the shots of the bar's tool and grid.
        6 => {
            open_keys(s, g);
        }
        _ => {}
    }
}

/// Opens the roll on the Keys clip, centred on its notes.
fn open_keys(s: &mut Studio, g: &Geometry) -> super::Ctx {
    s.select_clip_of("Keys");
    s.drawer_open = true;
    s.tab = crate::studio::DrawerTab::Roll;
    s.resized(g);
    let c = s.roll_ctx(g).unwrap();
    s.roll.center(&c);
    c
}
