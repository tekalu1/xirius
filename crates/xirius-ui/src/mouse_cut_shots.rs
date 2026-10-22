//! `--script mouse-cut`: KEY-019's mouse table on the piano roll, where 「消しゴム（一時的）」 is
//! wider than its column: blue, ended with 「…」, and whole in the tooltip under the pointer.
use crate::keymap_shots::{click, press, read};
use crate::settings::{Do, Section, Sub, scene::Op};
use crate::shot::{Act, Play, Shot};
use crate::studio::{Hit, Studio};
use gpui::{App, Modifiers, MouseMoveEvent, PlatformInput, Window, point, px};
use std::cell::RefCell;
use std::rc::Rc;
use xirius_design::Layout;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "01-cut",
        acts: &[stage(0)],
    },
    Shot {
        name: "02-tooltip",
        acts: &[stage(1)],
    },
];

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    if n == 0 {
        let dir = std::env::temp_dir().join(format!("xirius-mouse-cut-{}", std::process::id()));
        studio.borrow_mut().song.load_settings(&dir);
        let at = read(studio, window, |s, g| {
            s.center_of(g, Hit::RailSettings).unwrap()
        });
        click(at, 1, window, cx);
        press(studio, Do::Nav(Section::Keys), window, cx);
        press(studio, Do::Sub(Sub::Mouse), window, cx);
        press(studio, Do::Area("roll".into()), window, cx);
        return;
    }
    let at = read(studio, window, |s, g| {
        fn cut(ops: &[Op]) -> Option<(f32, f32)> {
            ops.iter().find_map(|o| match o {
                Op::Text(t) if t.s.starts_with("消しゴム（") => {
                    Some((t.r.x + 6.0, t.r.y + t.r.h / 2.0))
                }
                Op::Mask(_, inner) => cut(inner),
                _ => None,
            })
        }
        cut(&s.settings_scene(g).ops).expect("the eraser's held cell")
    });
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: point(px(at.0), px(at.1 + Layout::TOP_BAR_H)),
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
}
