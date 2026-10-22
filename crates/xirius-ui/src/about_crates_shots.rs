//! `--script about-crates`: 「Xirius について」's Rust libraries (LIC-015): the row opened by a
//! press on its head, the list of the libraries with their versions and licences, the search
//! typed through the window's keys and the wheel on the list, the copied code that is not a crate
//! (LIC-021) found by its name; then the app menu's item opening the panel. Everything goes
//! through the window's own input path (no OS input).

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    OwnedMenuItem, PlatformInput, ScrollDelta, ScrollWheelEvent, TouchPhase, Window, point, px,
};
use xirius_design::Layout;

use crate::about::{AboutLook, Act as About, CRATE_ROWS, Item, libraries};
use crate::popup::{Action, Look, PopupKind};
use crate::shot::{Act, Play, Shot};
use crate::studio::{Hit, Studio};
use crate::topbar::BarButton;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    // The row open: the search, the first libraries and 「ライセンス文」.
    Shot {
        name: "ac01-rust-open",
        acts: &[Act::Reset, stage(0)],
    },
    // The wheel moved the list down three rows.
    Shot {
        name: "ac02-wheel",
        acts: &[stage(1)],
    },
    // 「serde」 typed: the libraries whose name holds it.
    Shot {
        name: "ac03-search",
        acts: &[stage(2)],
    },
    // 「signalsmith」 typed instead: the two copies, MIT, the one with no version of its own
    // without one (LIC-021).
    Shot {
        name: "ac04-copied",
        acts: &[stage(3)],
    },
    // The app menu's 「Xirius について」 opened the panel again.
    Shot {
        name: "ac05-menu",
        acts: &[stage(4)],
    },
];

fn at(p: (f32, f32)) -> gpui::Point<gpui::Pixels> {
    point(px(p.0), px(p.1 + Layout::TOP_BAR_H))
}

fn press(p: (f32, f32), window: &mut Window, cx: &mut App) {
    let p = at(p);
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
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position: p,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}

fn type_key(key: &str, ch: Option<&str>, window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        PlatformInput::KeyDown(gpui::KeyDownEvent {
            keystroke: Keystroke {
                modifiers: Modifiers::default(),
                key: key.to_string(),
                key_char: ch.map(str::to_string),
            },
            is_held: false,
            prefer_character_input: false,
        }),
        cx,
    );
}

/// The names of the libraries the panel lists.
fn rows(studio: &Rc<RefCell<Studio>>, window: &Window) -> Vec<String> {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    s.popup
        .as_ref()
        .map(|p| p.layout(g.size).cells)
        .unwrap_or_default()
        .into_iter()
        .filter(|c| matches!(c.look, Look::About(AboutLook::Crate { .. })))
        .map(|c| c.text)
        .collect()
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let all: Vec<&str> = libraries().iter().map(|c| c.name).collect();
    match n {
        0 => {
            let logo = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                s.center_of(&g, Hit::Bar(BarButton::Logo))
                    .expect("the logo is on the bar")
            };
            press(logo, window, cx);
            let head = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let p = s.popup.as_ref().expect("the panel is open");
                assert_eq!(p.kind, PopupKind::About);
                let r = p
                    .layout(g.size)
                    .cells
                    .into_iter()
                    .find(|c| c.action == Some(Action::About(About::Toggle(Item::Rust))))
                    .expect("the Rust row")
                    .rect;
                (r.x + r.w / 2.0, r.y + r.h / 2.0)
            };
            press(head, window, cx);
            assert_eq!(rows(studio, window), all[..CRATE_ROWS]);
        }
        1 => {
            let list = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let r = s
                    .popup
                    .as_ref()
                    .unwrap()
                    .layout(g.size)
                    .cells
                    .into_iter()
                    .find(|c| matches!(c.look, Look::About(AboutLook::Crate { .. })))
                    .unwrap()
                    .rect;
                (r.x + r.w / 2.0, r.y + r.h / 2.0)
            };
            for _ in 0..3 {
                window.dispatch_event(
                    PlatformInput::ScrollWheel(ScrollWheelEvent {
                        position: at(list),
                        delta: ScrollDelta::Pixels(point(px(0.0), px(-40.0))),
                        modifiers: Modifiers::default(),
                        touch_phase: TouchPhase::Moved,
                    }),
                    cx,
                );
            }
            assert_eq!(rows(studio, window), all[3..3 + CRATE_ROWS]);
        }
        2 => {
            for ch in ["s", "e", "r", "d", "e"] {
                type_key(ch, Some(ch), window, cx);
            }
            let found = rows(studio, window);
            assert!(!found.is_empty());
            assert!(found.iter().all(|n| n.contains("serde")), "{found:?}");
            let first = all.iter().find(|n| n.contains("serde")).unwrap();
            assert_eq!(found[0], *first, "the list starts again from its top");
        }
        3 => {
            for _ in "serde".chars() {
                type_key("backspace", None, window, cx);
            }
            for ch in "signalsmith".chars().map(String::from) {
                type_key(&ch, Some(&ch), window, cx);
            }
            assert_eq!(
                rows(studio, window),
                ["signalsmith-linear", "signalsmith-stretch"]
            );
        }
        4 => {
            let menus = cx.get_menus().expect("the platform keeps the menus");
            let item = menus[0]
                .items
                .iter()
                .find_map(|i| match i {
                    OwnedMenuItem::Action { name, action, .. } => {
                        (name == crate::app_menu::ABOUT).then(|| action.boxed_clone())
                    }
                    _ => None,
                })
                .expect("the app menu has 「Xirius について」");
            studio.borrow_mut().popup = None;
            // The window runs the action on its next turn: the check waits behind it.
            window.dispatch_action(item, cx);
            let studio = studio.clone();
            cx.defer(move |_| {
                let s = studio.borrow();
                let p = s.popup.as_ref().expect("the menu opened the panel");
                assert_eq!(p.kind, PopupKind::About);
            });
        }
        _ => {}
    }
}
