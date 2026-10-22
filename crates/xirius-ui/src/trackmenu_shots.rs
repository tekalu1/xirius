//! MIX-027 / MIX-028 / MIX-029: the track column's right-click rows (new bus from the picked
//! tracks, pack into a folder, unpack, duplicate with only the settings), pressed through the
//! offscreen window's pointer handlers, each followed by one undo.
use crate::arrange::probe::{folder, row_mid, track};
use crate::shot::{Act, Play, Shot};
use crate::studio::Studio;
use crate::tree::RowKind;
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use std::cell::RefCell;
use std::rc::Rc;
use xirius_core::model::{Project, TrackKind};
use xirius_design::Layout;

thread_local! { static BEFORE: RefCell<Option<Project>> = const { RefCell::new(None) }; }

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const TRACKMENU: &[Shot] = &[
    Shot {
        name: "01-rows-menu",
        acts: &[Act::Reset, stage(10), stage(0)],
    },
    Shot {
        name: "02-bus-made",
        acts: &[stage(1)],
    },
    Shot {
        name: "03-bus-undone",
        acts: &[stage(2)],
    },
    Shot {
        name: "04-packed",
        acts: &[stage(3), stage(4)],
    },
    Shot {
        name: "05-pack-undone",
        acts: &[stage(2)],
    },
    Shot {
        name: "06-folder-menu",
        acts: &[stage(5)],
    },
    Shot {
        name: "07-unpacked",
        acts: &[stage(6)],
    },
    Shot {
        name: "08-unpack-undone",
        acts: &[stage(2)],
    },
    Shot {
        name: "09-duplicate-menu",
        acts: &[stage(7), stage(8)],
    },
    Shot {
        name: "10-settings-duplicated",
        acts: &[stage(9)],
    },
    Shot {
        name: "11-duplicate-undone",
        acts: &[stage(2)],
    },
];

fn press(p: (f32, f32), button: MouseButton, window: &mut Window, cx: &mut App) {
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

fn right_click(studio: &Rc<RefCell<Studio>>, kind: RowKind, window: &mut Window, cx: &mut App) {
    let at = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        row_mid(&s, &g, kind)
    };
    press(at, MouseButton::Right, window, cx);
    assert!(
        studio.borrow().context_open(),
        "the right-click opens a menu"
    );
}

fn choose(studio: &Rc<RefCell<Studio>>, label: &str, window: &mut Window, cx: &mut App) {
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

fn remember(s: &Studio) {
    BEFORE.with(|p| *p.borrow_mut() = Some(s.project().as_ref().clone()));
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let track_row = |name: &str| RowKind::Track(track(&studio.borrow(), name));
    match n {
        10 => {
            // The arrangement alone, tall enough for every row.
            let mut s = studio.borrow_mut();
            let g = crate::screen::geometry(&s, window);
            s.drawer_open = false;
            s.resized(&g);
        }
        0 => {
            let rows = vec![track_row("Vocal"), track_row("Guitar")];
            studio.borrow_mut().arrange.tracks = rows;
            remember(&studio.borrow());
            right_click(studio, track_row("Guitar"), window, cx);
        }
        1 => {
            choose(studio, "新しいバスへまとめる", window, cx);
            let s = studio.borrow();
            let p = s.project();
            let bus = p
                .tracks
                .values()
                .filter(|t| t.kind == TrackKind::Bus)
                .max_by_key(|t| t.id)
                .map(|t| t.id)
                .expect("the new bus");
            let (v, g) = (track(&s, "Vocal"), track(&s, "Guitar"));
            let to = Some(xirius_core::model::Output::Bus(bus));
            assert_eq!(p.tracks[&v].output, to);
            assert_eq!(p.tracks[&g].output, to);
        }
        2 => {
            let mut s = studio.borrow_mut();
            let g = crate::screen::geometry(&s, window);
            s.undo(&g);
            BEFORE.with(|p| {
                let before = p.borrow();
                let before = before.as_ref().expect("before an edit");
                let mut now = s.project().as_ref().clone();
                now.next_id = before.next_id;
                assert_eq!(&now, before, "one undo restores the song");
            });
        }
        3 => {
            let rows = vec![track_row("Vocal"), track_row("Guitar")];
            studio.borrow_mut().arrange.tracks = rows;
            remember(&studio.borrow());
            right_click(studio, track_row("Guitar"), window, cx);
        }
        4 => {
            let folders = studio.borrow().project().folders.len();
            choose(studio, "フォルダーにまとめる", window, cx);
            let s = studio.borrow();
            assert_eq!(s.project().folders.len(), folders + 1);
        }
        5 => {
            remember(&studio.borrow());
            let f = folder(&studio.borrow(), "Latin");
            right_click(studio, RowKind::Folder(f), window, cx);
        }
        6 => {
            let folders = studio.borrow().project().folders.len();
            choose(studio, "まとめを解く", window, cx);
            let s = studio.borrow();
            assert_eq!(s.project().folders.len(), folders - 1);
        }
        7 => {
            remember(&studio.borrow());
            right_click(studio, track_row("Vocal"), window, cx);
        }
        8 => choose(studio, "トラックを複製", window, cx),
        _ => {
            let tracks = studio.borrow().project().tracks.len();
            choose(studio, "設定だけ", window, cx);
            let s = studio.borrow();
            assert_eq!(s.project().tracks.len(), tracks + 1);
            let copy = s
                .project()
                .tracks
                .values()
                .find(|t| t.name == "Vocal コピー")
                .expect("the copy")
                .clone();
            assert!(copy.clips.is_empty(), "only the settings");
        }
    }
}
