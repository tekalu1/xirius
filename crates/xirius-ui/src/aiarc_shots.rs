use crate::{
    shot::{Act, Play, Shot, Target},
    studio::{Rect, Studio},
    topbar::BarButton,
};
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput, Window, point, px,
};
use std::cell::RefCell;
use std::rc::Rc;
use xirius_design::Layout;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const AIARC: &[Shot] = &[
    Shot {
        name: "00-rename-command",
        acts: &[
            Act::Press(Target::Track("Keys")),
            Act::Press(Target::Top(BarButton::Palette)),
            stage(0),
        ],
    },
    Shot {
        name: "01-rename-form",
        acts: &[stage(1)],
    },
    Shot {
        name: "02-name-entered",
        acts: &[stage(2)],
    },
    Shot {
        name: "03-renamed",
        acts: &[stage(3)],
    },
    Shot {
        name: "04-undone",
        acts: &[Act::Key("ctrl-z"), stage(4)],
    },
    Shot {
        name: "05-project-info",
        acts: &[Act::Press(Target::Top(BarButton::Palette)), stage(5)],
    },
    Shot {
        name: "06-tempo-value",
        acts: &[
            Act::Key("escape"),
            Act::Press(Target::Top(BarButton::Palette)),
            stage(6),
        ],
    },
    Shot {
        name: "07-invalid-value",
        acts: &[Act::Key("enter"), stage(7)],
    },
    Shot {
        name: "08-value-corrected",
        acts: &[stage(8)],
    },
    Shot {
        name: "09-tempo-applied",
        acts: &[Act::Key("enter"), stage(9)],
    },
    Shot {
        name: "10-tempo-undone",
        acts: &[Act::Key("ctrl-z"), stage(10)],
    },
];

pub(crate) fn click(rect: Rect, window: &mut Window, cx: &mut App) {
    let at = point(px(rect.x + 12.0), px(rect.y + Layout::TOP_BAR_H + 12.0));
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let size = window.viewport_size();
    let g = studio.borrow().geometry(
        f32::from(size.width),
        f32::from(size.height) - Layout::TOP_BAR_H - Layout::BOTTOM_BAR_H,
    );
    match n {
        0 => studio
            .borrow_mut()
            .palette
            .as_mut()
            .unwrap()
            .replace_text(None, "track.rename"),
        1 => {
            let row = studio
                .borrow()
                .palette
                .as_ref()
                .unwrap()
                .layout(g.size)
                .rows[0]
                .1;
            click(row, window, cx);
            assert!(studio.borrow().palette.as_ref().unwrap().form.is_some());
        }
        2 => studio
            .borrow_mut()
            .palette
            .as_mut()
            .unwrap()
            .replace_text(None, "Piano"),
        3 => {
            let row = studio
                .borrow()
                .palette
                .as_ref()
                .unwrap()
                .layout(g.size)
                .rows
                .last()
                .unwrap()
                .1;
            click(row, window, cx);
            assert!(studio.borrow().palette.is_none());
            assert!(
                studio
                    .borrow()
                    .project()
                    .tracks
                    .values()
                    .any(|t| t.name == "Piano")
            );
        }
        4 => assert!(
            studio
                .borrow()
                .project()
                .tracks
                .values()
                .any(|t| t.name == "Keys")
        ),
        5 | 6 => {
            studio
                .borrow_mut()
                .palette
                .as_mut()
                .unwrap()
                .replace_text(None, if n == 5 { "project.get" } else { "tempo.set" });
            let row = studio
                .borrow()
                .palette
                .as_ref()
                .unwrap()
                .layout(g.size)
                .rows[0]
                .1;
            click(row, window, cx);
            if n == 5 {
                assert!(
                    studio
                        .borrow()
                        .palette
                        .as_ref()
                        .unwrap()
                        .form
                        .as_ref()
                        .unwrap()
                        .output
                );
            } else {
                studio
                    .borrow_mut()
                    .palette
                    .as_mut()
                    .unwrap()
                    .replace_text(None, "-1");
            }
        }
        7 => {
            assert!(
                studio
                    .borrow()
                    .palette
                    .as_ref()
                    .unwrap()
                    .form
                    .as_ref()
                    .unwrap()
                    .error
                    .is_some()
            );
            assert_eq!(studio.borrow().project().tempo[0].bpm, 120.0);
        }
        8 => studio
            .borrow_mut()
            .palette
            .as_mut()
            .unwrap()
            .replace_text(Some(0..2), "128"),
        9 => {
            assert!(studio.borrow().palette.is_none());
            assert_eq!(studio.borrow().project().tempo[0].bpm, 128.0);
        }
        10 => assert_eq!(studio.borrow().project().tempo[0].bpm, 120.0),
        _ => unreachable!(),
    }
    assert!(
        matches!(n, 7 | 8) || studio.borrow().error.is_none(),
        "{:?}",
        studio.borrow().error
    );
    eprintln!("aiarc-check stage={n} ok=true");
}
