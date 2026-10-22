//! CTX-018: the folder's "ステムを書き出す" row with its two rows, and the export panel the single
//! file row opens, pressed through the offscreen window's pointer handlers.
use crate::arrange::probe::{folder, row_mid};
use crate::shot::{Act, Play, Shot};
use crate::studio::Studio;
use crate::tree::RowKind;
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use serde_json::json;
use std::cell::RefCell;
use std::rc::Rc;
use xirius_design::Layout;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const CTXFOLDER: &[Shot] = &[
    Shot {
        name: "01-folder-menu",
        acts: &[Act::Reset, stage(10), stage(0)],
    },
    Shot {
        name: "02-stems-rows",
        acts: &[stage(1)],
    },
    Shot {
        name: "03-panel-one-file",
        acts: &[stage(2)],
    },
    Shot {
        name: "04-each-track",
        acts: &[stage(3), stage(4), stage(5)],
    },
];

pub(crate) fn press(p: (f32, f32), button: MouseButton, window: &mut Window, cx: &mut App) {
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
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position: p,
            button,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}

fn right_click(studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let at = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        let f = folder(&s, "Rhythm");
        row_mid(&s, &g, RowKind::Folder(f))
    };
    press(at, MouseButton::Right, window, cx);
    assert!(
        studio.borrow().context_open(),
        "the right-click opens a menu"
    );
}

pub(crate) fn choose(studio: &Rc<RefCell<Studio>>, label: &str, window: &mut Window, cx: &mut App) {
    let at = {
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
    press(at, MouseButton::Left, window, cx);
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    match n {
        10 => {
            // The arrangement alone, tall enough for every row; the folder gets a bus.
            let mut s = studio.borrow_mut();
            let g = crate::screen::geometry(&s, window);
            s.drawer_open = false;
            s.resized(&g);
            let f = folder(&s, "Rhythm");
            s.command(|song| {
                song.run("folder.bus.add", json!({ "id": f.to_string() }))
                    .map(|_| ())
            });
            assert!(s.error.is_none(), "{:?}", s.error);
        }
        0 => right_click(studio, window, cx),
        1 => choose(studio, "ステムを書き出す", window, cx),
        2 => {
            choose(studio, "このフォルダーを 1 本に…", window, cx);
            let s = studio.borrow();
            let f = folder(&s, "Rhythm");
            assert_eq!(s.export.folders, [f]);
            assert_eq!(s.export.picked, [s.project().folders[&f].bus.unwrap()]);
        }
        3 => studio.borrow_mut().popup = None,
        4 => {
            right_click(studio, window, cx);
            choose(studio, "ステムを書き出す", window, cx);
        }
        _ => {
            choose(studio, "中のトラックごと…", window, cx);
            let s = studio.borrow();
            assert!(s.export.folders.is_empty());
            assert!(s.export.picked.len() > 1);
        }
    }
}
