//! MIX-045 / MIX-044: the insert heading dragged onto another strip (the ring, the dim copy over
//! the source, the line with Shift, the toast), its menu, the ＋ of the headings and the list it
//! opens, the rows that add to a track, at the strip's 116 and 252, through the offscreen
//! window's pointer handlers.
use crate::arrange::probe::track;
use crate::mixer::MixerHit;
use crate::mixer::flow::Area;
use crate::shot::{Act, Play, Shot};
use crate::studio::{DrawerTab, Studio};
use crate::tree::RowKind;
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use serde_json::json;
use std::cell::RefCell;
use std::rc::Rc;
use xirius_core::id::TrackId;
use xirius_design::Layout;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "01-plus-116",
        acts: &[Act::Reset, stage(0)],
    },
    Shot {
        name: "02-heading-menu",
        acts: &[stage(1)],
    },
    Shot {
        name: "03-heading-drag-ring",
        acts: &[stage(2), stage(3)],
    },
    Shot {
        name: "04-drag-over-source",
        acts: &[stage(4)],
    },
    Shot {
        name: "05-pasted-toast",
        acts: &[stage(5)],
    },
    Shot {
        name: "06-shift-line",
        acts: &[stage(6), stage(15)],
    },
    Shot {
        name: "07-appended-toast",
        acts: &[stage(7)],
    },
    Shot {
        name: "08-insert-list",
        acts: &[stage(8)],
    },
    Shot {
        name: "09-list-nothing-matches",
        acts: &[stage(9)],
    },
    Shot {
        name: "10-splitter-added",
        acts: &[stage(10)],
    },
    Shot {
        name: "11-effect-row-menu",
        acts: &[stage(11)],
    },
    Shot {
        name: "12-track-head-menu",
        acts: &[stage(12)],
    },
    Shot {
        name: "13-plus-252",
        acts: &[stage(13)],
    },
    Shot {
        name: "14-drag-252",
        acts: &[stage(14)],
    },
];

fn mods(shift: bool) -> Modifiers {
    Modifiers {
        shift,
        ..Modifiers::default()
    }
}

fn at(p: (f32, f32)) -> gpui::Point<gpui::Pixels> {
    point(px(p.0), px(p.1 + Layout::TOP_BAR_H))
}

fn moved(p: (f32, f32), held: bool, shift: bool, window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at(p),
            pressed_button: held.then_some(MouseButton::Left),
            modifiers: mods(shift),
        }),
        cx,
    );
}

fn down(p: (f32, f32), button: MouseButton, window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            position: at(p),
            button,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
}

fn up(p: (f32, f32), button: MouseButton, shift: bool, window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position: at(p),
            button,
            modifiers: mods(shift),
            click_count: 1,
        }),
        cx,
    );
}

fn press(p: (f32, f32), button: MouseButton, window: &mut Window, cx: &mut App) {
    moved(p, false, false, window, cx);
    down(p, button, window, cx);
    up(p, button, false, window, cx);
}

/// Left of the ＋ on a strip's insert heading.
fn heading(studio: &Rc<RefCell<Studio>>, t: TrackId, window: &mut Window) -> (f32, f32) {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    let r = s
        .mixer_rect(&g, MixerHit::Heading(t, Area::Insert))
        .expect("the heading on screen");
    (r.x + 12.0, r.y + r.h / 2.0)
}

fn center(studio: &Rc<RefCell<Studio>>, hit: MixerHit, window: &mut Window) -> (f32, f32) {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    s.mixer_center(&g, hit).expect("on screen")
}

/// The middle of a strip's insert area.
fn area(studio: &Rc<RefCell<Studio>>, t: TrackId, window: &mut Window) -> (f32, f32) {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    let r = s
        .mixer_strips(&g)
        .into_iter()
        .find(|x| x.track == t)
        .expect("a strip")
        .inserts;
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

/// From the heading of `from` to `to`, held, in a few steps.
fn carry(
    studio: &Rc<RefCell<Studio>>,
    from: TrackId,
    to: (f32, f32),
    shift: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let a = heading(studio, from, window);
    moved(a, false, false, window, cx);
    down(a, MouseButton::Left, window, cx);
    for k in 1..=6 {
        let f = k as f32 / 6.0;
        moved(
            (a.0 + (to.0 - a.0) * f, a.1 + (to.1 - a.1) * f),
            true,
            shift,
            window,
            cx,
        );
    }
    assert!(studio.borrow().mixer.insert_drag.is_some_and(|d| d.moved));
}

fn choose(studio: &Rc<RefCell<Studio>>, label: &str, window: &mut Window, cx: &mut App) {
    let p = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        let layout = s.popup.as_ref().expect("menu").layout(g.size);
        let cell = layout
            .cells
            .iter()
            .find(|c| c.text == label && !matches!(c.look, crate::popup::Look::Input))
            .unwrap_or_else(|| panic!("missing row {label}"));
        assert!(cell.live(), "disabled row {label}");
        (
            cell.rect.x + cell.rect.w / 2.0,
            cell.rect.y + cell.rect.h / 2.0,
        )
    };
    press(p, MouseButton::Left, window, cx);
}

fn inserts(s: &Studio, name: &str) -> Vec<String> {
    s.project().tracks[&track(s, name)]
        .inserts
        .iter()
        .map(|p| p.name.clone())
        .collect()
}

fn add(s: &mut Studio, name: &str, effect: &str, kind: &str) {
    let t = track(s, name);
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": effect, "kind": kind, "format": "builtin", "uid": effect }),
    ));
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let id = |name: &str| track(&studio.borrow(), name);
    match n {
        0 => {
            let mut s = studio.borrow_mut();
            s.drawer_open = true;
            s.tab = DrawerTab::Mixer;
            s.drawer_size.mixer = Some(520.0);
            add(&mut s, "Vocal", "Compressor", "effect");
            add(&mut s, "Vocal", "EQ", "effect");
            add(&mut s, "Guitar", "Delay", "effect");
            add(&mut s, "Keys", "Sampler", "instrument");
            s.hover = crate::studio::Hit::None;
            let g = crate::screen::geometry(&s, window);
            s.resized(&g);
        }
        1 => {
            let p = heading(studio, id("Vocal"), window);
            press(p, MouseButton::Right, window, cx);
            assert!(studio.borrow().context_open());
        }
        2 => studio.borrow_mut().popup = None,
        3 => {
            let to = area(studio, id("Guitar"), window);
            carry(studio, id("Vocal"), to, false, window, cx);
        }
        4 => {
            let to = area(studio, id("Vocal"), window);
            moved(to, true, false, window, cx);
            let s = studio.borrow();
            let g = crate::screen::geometry(&s, window);
            assert!(s.insert_drag_refused(&g));
        }
        5 => {
            let to = area(studio, id("Guitar"), window);
            moved(to, true, false, window, cx);
            up(to, MouseButton::Left, false, window, cx);
            let s = studio.borrow();
            assert_eq!(inserts(&s, "Guitar"), ["Compressor", "EQ"]);
            assert_eq!(s.notice_text(), Some("インサートを貼り付けました"));
        }
        6 => {
            // the paste taken back (the window's history runs on the worker)
            let mut s = studio.borrow_mut();
            let g = crate::screen::geometry(&s, window);
            assert!(s.undo(&g));
            crate::shot::run_for(&mut s, 10.0, |s| inserts(s, "Guitar") == ["Delay"]);
            assert_eq!(inserts(&s, "Guitar"), ["Delay"]);
        }
        15 => {
            // Guitar has its Delay back, and Vocal's inserts go after it.
            let to = area(studio, id("Guitar"), window);
            carry(studio, id("Vocal"), to, true, window, cx);
            assert!(studio.borrow().mixer.insert_drag.unwrap().append);
        }
        7 => {
            let to = area(studio, id("Guitar"), window);
            up(to, MouseButton::Left, true, window, cx);
            let s = studio.borrow();
            assert_eq!(inserts(&s, "Guitar"), ["Delay", "Compressor", "EQ"]);
            assert_eq!(s.notice_text(), Some("インサートを後ろに貼り付けました"));
        }
        8 => {
            studio.borrow_mut().notice = None;
            let p = center(studio, MixerHit::AddInsert(id("Vocal")), window);
            press(p, MouseButton::Left, window, cx);
            let s = studio.borrow();
            let first = s.popup.as_ref().expect("the list").choices()[0].clone();
            assert_eq!(first.to, crate::mixer::scan::SPLITTER);
        }
        9 => {
            let mut s = studio.borrow_mut();
            assert!(s.fx_typed("z", Some("zzzz")));
        }
        10 => {
            {
                let mut s = studio.borrow_mut();
                s.fx_typed("escape", None);
                s.fx_typed("escape", None);
                assert!(s.popup.is_none());
            }
            let p = heading(studio, id("Vocal"), window);
            press(p, MouseButton::Right, window, cx);
            choose(studio, "スプリッターを追加", window, cx);
        }
        11 => {
            let comp = {
                let s = studio.borrow();
                s.project().tracks[&id("Vocal")].inserts[0].id
            };
            let p = center(studio, MixerHit::Open(id("Vocal"), comp), window);
            press(p, MouseButton::Right, window, cx);
            assert!(studio.borrow().context_open());
        }
        12 => {
            let p = {
                let mut s = studio.borrow_mut();
                s.popup = None;
                s.ctx = None;
                let keys = track(&s, "Keys");
                s.scroll.1 = s.tree().row_of(RowKind::Track(keys)).unwrap().y;
                let g = crate::screen::geometry(&s, window);
                crate::arrange::probe::row_mid(&s, &g, RowKind::Track(keys))
            };
            press(p, MouseButton::Right, window, cx);
            assert!(studio.borrow().context_open());
        }
        13 => {
            let mut s = studio.borrow_mut();
            s.popup = None;
            s.ctx = None;
            s.scroll.1 = 0.0;
            for name in ["Vocal", "Guitar", "Keys"] {
                let t = track(&s, name);
                s.mixer.wide.insert(t);
            }
            s.hover = crate::studio::Hit::None;
            let g = crate::screen::geometry(&s, window);
            s.resized(&g);
        }
        _ => {
            let to = area(studio, id("Keys"), window);
            carry(studio, id("Vocal"), to, true, window, cx);
        }
    }
}
