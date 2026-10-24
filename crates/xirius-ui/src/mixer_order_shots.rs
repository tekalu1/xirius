//! UI-170: a channel's name dragged to the edge of another strip puts the channel there; the
//! line in the gap shows where it goes while it is held. The order is the song's, so it is the
//! same when the song is saved and opened again. Through the offscreen window's pointer
//! handlers; the song is saved to a folder of its own under the temporary folder.
use crate::arrange::probe::track;
use crate::name_drag::Drop;
use crate::shot::{Act, Play, Shot};
use crate::studio::{DrawerTab, FileRequest, Studio};
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use std::cell::RefCell;
use std::rc::Rc;

use xirius_design::Layout;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "01-mixer",
        acts: &[Act::Reset, stage(0)],
    },
    Shot {
        name: "02-name-held-at-the-edge",
        acts: &[stage(1)],
    },
    Shot {
        name: "03-moved",
        acts: &[stage(2)],
    },
    Shot {
        name: "04-opened-again",
        acts: &[stage(3)],
    },
];

/// The channel that moves, and the one it goes before.
const MOVED: &str = "Guitar";
const BEFORE: &str = "Keys";

fn at(p: (f32, f32)) -> gpui::Point<gpui::Pixels> {
    point(px(p.0), px(p.1 + Layout::TOP_BAR_H))
}

fn moved(p: (f32, f32), held: bool, window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at(p),
            pressed_button: held.then_some(MouseButton::Left),
            modifiers: Modifiers::default(),
        }),
        cx,
    );
}

/// A strip's name, and the left edge of a strip.
fn spots(studio: &Rc<RefCell<Studio>>, window: &mut Window) -> ((f32, f32), (f32, f32)) {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    let strips = s.mixer_strips(&g);
    let strip = |name: &str| {
        let t = track(&s, name);
        strips.iter().find(|x| x.track == t).expect("a strip")
    };
    let name = strip(MOVED).name;
    let edge = strip(BEFORE).rect;
    (
        (name.x + name.w / 2.0, name.y + name.h / 2.0),
        (edge.x + 3.0, edge.y + edge.h / 2.0),
    )
}

fn order(s: &Studio) -> Vec<String> {
    crate::mixer::strip_order(&s.project())
        .into_iter()
        .map(|t| s.project().tracks[&t].name.clone())
        .collect()
}

fn next_to(s: &Studio) -> bool {
    let o = order(s);
    let i = o.iter().position(|n| n == MOVED).expect("moved");
    o.get(i + 1).map(String::as_str) == Some(BEFORE)
}

fn folder() -> std::path::PathBuf {
    std::env::temp_dir()
        .join(format!("xirius-mixer-order-{}", std::process::id()))
        .join("order")
}

fn show_mixer(s: &mut Studio, window: &mut Window) {
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    let g = crate::screen::geometry(s, window);
    s.resized(&g);
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    match n {
        0 => {
            show_mixer(&mut studio.borrow_mut(), window);
            assert!(!next_to(&studio.borrow()));
        }
        1 => {
            let (from, to) = spots(studio, window);
            moved(from, false, window, cx);
            window.dispatch_event(
                PlatformInput::MouseDown(MouseDownEvent {
                    position: at(from),
                    button: MouseButton::Left,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                    first_mouse: false,
                }),
                cx,
            );
            for k in 1..=6 {
                let f = k as f32 / 6.0;
                moved(
                    (from.0 + (to.0 - from.0) * f, from.1 + (to.1 - from.1) * f),
                    true,
                    window,
                    cx,
                );
            }
            let s = studio.borrow();
            let g = crate::screen::geometry(&s, window);
            assert!(s.mixer.name_drag.is_some_and(|d| d.moved));
            assert_eq!(
                s.name_drop(&g, track(&s, MOVED), to.0, to.1),
                Some(Drop::Move {
                    anchor: track(&s, BEFORE),
                    after: false
                })
            );
        }
        2 => {
            let (_, to) = spots(studio, window);
            window.dispatch_event(
                PlatformInput::MouseUp(MouseUpEvent {
                    position: at(to),
                    button: MouseButton::Left,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                }),
                cx,
            );
            let s = studio.borrow();
            assert!(s.mixer.name_drag.is_none());
            assert!(next_to(&s), "{:?}", order(&s));
        }
        _ => {
            let mut s = studio.borrow_mut();
            let dir = folder();
            let _ = std::fs::remove_dir_all(dir.parent().expect("a parent"));
            let before = order(&s);
            s.command(|song| song.save_as(&dir));
            assert!(s.error.is_none(), "{:?}", s.error);
            s.new_song();
            s.after_open(move |s| {
                assert_ne!(order(s), before);
                s.finish_file_request(FileRequest::Open, &dir);
                s.after_open(move |s| {
                    assert_eq!(order(s), before);
                    s.drawer_open = true;
                    s.tab = DrawerTab::Mixer;
                });
            });
        }
    }
}
