//! UI-171: a double press on a row of the CPU list brings its slot into sight and lights it,
//! though the mixer looks at the far end and the slot is below the strip's visible rows, then
//! back up to the first one. Through the offscreen window's pointer handlers; the readings are
//! laid by the script (`Perf::fake`), so no sound device is asked. Taken with `--size 1600,960`:
//! the CPU button is not in a narrower bar.
use crate::cpu_list::{CAct, Fake};
use crate::mixer::MixerHit;
use crate::popup::{Action, PopupKind};
use crate::shot::{Act, Play, Shot};
use crate::studio::{DrawerTab, Studio};
use crate::topbar::BarButton;
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use serde_json::json;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use xirius_core::id::{PluginId, TrackId};
use xirius_design::Layout;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "01-mixer-elsewhere",
        acts: &[Act::Reset, stage(0)],
    },
    Shot {
        name: "02-cpu-list",
        acts: &[stage(1)],
    },
    Shot {
        name: "03-last-slot-lit",
        acts: &[stage(2)],
    },
    Shot {
        name: "04-first-slot-lit",
        acts: &[stage(3)],
    },
];

const EFFECTS: [&str; 6] = [
    "Compressor",
    "Reverb",
    "Delay",
    "Compressor",
    "Reverb",
    "Delay",
];

fn press(p: (f32, f32), button: MouseButton, clicks: usize, window: &mut Window, cx: &mut App) {
    let p = point(px(p.0), px(p.1 + Layout::TOP_BAR_H));
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: p,
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            position: p,
            button,
            modifiers: Modifiers::default(),
            click_count: clicks,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position: p,
            button,
            modifiers: Modifiers::default(),
            click_count: clicks,
        }),
        cx,
    );
}

/// The first channel of the mixer, the one the effects go on.
fn first(s: &Studio) -> TrackId {
    crate::mixer::strip_order(&s.project())[0]
}

fn inserts(s: &Studio) -> Vec<PluginId> {
    s.project().tracks[&first(s)]
        .inserts
        .iter()
        .map(|p| p.id)
        .collect()
}

fn open_list(studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let at = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        let r = s.bar_rect(&g, BarButton::Cpu);
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    };
    press(at, MouseButton::Left, 1, window, cx);
    assert_eq!(
        studio.borrow().popup.as_ref().map(|p| p.kind),
        Some(PopupKind::Perf)
    );
}

/// A double press on the row of `id` in the list, opened first if it is not.
fn ping(studio: &Rc<RefCell<Studio>>, id: PluginId, window: &mut Window, cx: &mut App) {
    let open = studio.borrow().popup.as_ref().map(|p| p.kind) == Some(PopupKind::Perf);
    if !open {
        open_list(studio, window, cx);
    }
    let at = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        let layout = s.popup.as_ref().expect("the list").layout(g.size);
        let c = layout
            .cells
            .iter()
            .find(|c| c.action.as_ref() == Some(&Action::Cpu(CAct::Pick(id))))
            .expect("the row");
        (c.rect.x + c.rect.w / 2.0, c.rect.y + c.rect.h / 2.0)
    };
    press(at, MouseButton::Left, 2, window, cx);
    assert!(studio.borrow().perf.pinged(id));
}

fn shown(studio: &Rc<RefCell<Studio>>, id: PluginId, window: &mut Window) -> bool {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    s.mixer_center(&g, MixerHit::Open(first(&s), id)).is_some()
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    match n {
        0 => {
            let mut s = studio.borrow_mut();
            s.drawer_open = true;
            s.tab = DrawerTab::Mixer;
            // More channels than the mixer holds, so it can look away from the first.
            for i in 1..=8 {
                assert!(s.run_command("track.add.audio", json!({ "name": format!("Take {i}") })));
            }
            let t = first(&s);
            for name in EFFECTS {
                assert!(s.run_command(
                    "plugin.add",
                    json!({ "track": t.to_string(), "name": name, "kind": "effect",
                            "format": "builtin", "uid": name }),
                ));
            }
            let ids = inserts(&s);
            assert!(ids.len() > crate::mixer::Rows::of(&s.project()).inserts);
            let loads = ids
                .iter()
                .enumerate()
                .map(|(i, &id)| (id, 0.02 + 0.01 * i as f64))
                .collect::<BTreeMap<_, _>>();
            s.perf.fake = Some(Fake {
                load: 0.35,
                xruns: 0,
                loads,
            });
            s.poll_perf_now();
            // The mixer looks at the far end, away from the strip.
            let g = crate::screen::geometry(&s, window);
            s.resized(&g);
            s.mixer.scroll = s.mixer_max_scroll(&g);
            assert!(s.mixer.scroll > 0.0, "the channels do not all fit");
            s.resized(&g);
        }
        1 => {
            let last = *inserts(&studio.borrow()).last().unwrap();
            assert!(!shown(studio, last, window), "the slot is out of sight");
            open_list(studio, window, cx);
        }
        2 => {
            let ids = inserts(&studio.borrow());
            let last = *ids.last().unwrap();
            ping(studio, last, window, cx);
            assert!(shown(studio, last, window), "the last slot is in sight");
            assert!(!shown(studio, ids[0], window));
        }
        _ => {
            let first_id = inserts(&studio.borrow())[0];
            ping(studio, first_id, window, cx);
            assert!(
                shown(studio, first_id, window),
                "the first slot is in sight"
            );
            let last = *inserts(&studio.borrow()).last().unwrap();
            assert!(!shown(studio, last, window));
        }
    }
}
