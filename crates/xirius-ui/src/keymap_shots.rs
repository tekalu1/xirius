//! Settings and inline naming through the offscreen window's pointer and input handlers.
use crate::settings::{Do, MenuKind, Section, Sub};
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Hit, RowParts, Studio};
use crate::tree::RowKind;
use gpui::{
    App, Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PlatformInput, Window, point, px,
};
use std::cell::RefCell;
use std::rc::Rc;
use xirius_design::Layout;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}
pub const SHOTS: &[Shot] = &[
    Shot {
        name: "01-keyboard",
        acts: &[stage(0)],
    },
    Shot {
        name: "02-quantize-remapped",
        acts: &[stage(1)],
    },
    Shot {
        name: "03-conflict",
        acts: &[stage(2)],
    },
    Shot {
        name: "04-swapped",
        acts: &[stage(3)],
    },
    Shot {
        name: "05-mouse",
        acts: &[stage(4)],
    },
    Shot {
        name: "06-mouse-remapped",
        acts: &[stage(5)],
    },
    Shot {
        name: "07-track-name-open",
        acts: &[stage(6)],
    },
    Shot {
        name: "08-track-name-input",
        acts: &[stage(7)],
    },
    Shot {
        name: "09-name-and-snap",
        acts: &[stage(8)],
    },
    Shot {
        name: "10-automation-keys",
        acts: &[stage(9)],
    },
    Shot {
        name: "11-zoom-keys",
        acts: &[stage(10)],
    },
];

pub(crate) fn read<T>(
    studio: &Rc<RefCell<Studio>>,
    window: &Window,
    f: impl FnOnce(&Studio, &Geometry) -> T,
) -> T {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    f(&s, &g)
}
pub(crate) fn click(at: (f32, f32), clicks: usize, window: &mut Window, cx: &mut App) {
    let position = point(px(at.0), px(at.1 + Layout::TOP_BAR_H));
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: clicks,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: clicks,
        }),
        cx,
    );
}
fn key(source: &str, window: &mut Window, cx: &mut App) {
    window.dispatch_keystroke(Keystroke::parse(source).unwrap(), cx);
}
pub(crate) fn press(studio: &Rc<RefCell<Studio>>, act: Do, window: &mut Window, cx: &mut App) {
    let at = read(studio, window, |s, g| {
        let scene = s.settings_scene(g);
        scene
            .items
            .iter()
            .find_map(|i| {
                let at = (i.r.x + i.r.w / 2.0, i.r.y + i.r.h / 2.0);
                (i.act == act && scene.item_at(at.0, at.1).is_some_and(|hit| hit.act == act))
                    .then_some(at)
            })
            .unwrap_or_else(|| panic!("settings part is not visible: {act:?}"))
    });
    click(at, 1, window, cx);
}
pub(crate) fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    match n {
        0 => {
            // A scripted run has a private folder even if the caller's config is the real one.
            let dir =
                std::env::temp_dir().join(format!("xirius-keymap-shots-{}", std::process::id()));
            studio.borrow_mut().song.load_settings(&dir);
            let at = read(studio, window, |s, g| {
                s.center_of(g, Hit::RailSettings).unwrap()
            });
            click(at, 1, window, cx);
            press(studio, Do::Nav(Section::Keys), window, cx);
            press(studio, Do::KbKey("Q".into()), window, cx);
        }
        1 => {
            press(
                studio,
                Do::Capture {
                    ctx: "roll".into(),
                    cmd: "note.quantize".into(),
                    from: Some(xirius_prefs::Chord::parse("Q", xirius_prefs::Os::Win).unwrap()),
                },
                window,
                cx,
            );
            key("ctrl-alt-q", window, cx);
            let settings = studio.borrow().song.settings().clone();
            let settings = settings.lock().unwrap();
            assert!(
                settings
                    .keymap
                    .keys_of("roll", "note.quantize")
                    .iter()
                    .any(|k| k.to_string() == "Mod+Alt+Q")
            );
            let text = std::fs::read_to_string(settings.path(xirius_prefs::File::Keymap).unwrap())
                .unwrap();
            assert!(text.contains("note.quantize") && !text.contains("history.undo"));
        }
        2 => {
            press(studio, Do::KbKey("Delete".into()), window, cx);
            press(
                studio,
                Do::Capture {
                    ctx: "roll".into(),
                    cmd: "note.delete".into(),
                    from: Some(
                        xirius_prefs::Chord::parse("Delete", xirius_prefs::Os::Win).unwrap(),
                    ),
                },
                window,
                cx,
            );
            key("ctrl-alt-q", window, cx);
            press(studio, Do::ClearQuery, window, cx);
            key("ctrl-alt-q", window, cx);
            read(studio, window, |s, _| {
                assert!(
                    s.song
                        .settings()
                        .lock()
                        .unwrap()
                        .keymap
                        .conflicts(xirius_prefs::Os::Win)
                        .iter()
                        .any(|i| matches!(i, xirius_prefs::keymap::Issue::Conflict { .. }))
                )
            });
        }
        3 => {
            press(studio, Do::ClearQuery, window, cx);
            key("ctrl-alt-q", window, cx);
            press(
                studio,
                Do::Swap {
                    ctx: "roll".into(),
                    cmd: "note.delete".into(),
                    key: xirius_prefs::Chord::parse("Ctrl+Alt+Q", xirius_prefs::Os::Win).unwrap(),
                },
                window,
                cx,
            );
            read(studio, window, |s, _| {
                assert!(
                    !s.song
                        .settings()
                        .lock()
                        .unwrap()
                        .keymap
                        .conflicts(xirius_prefs::Os::Win)
                        .iter()
                        .any(|i| matches!(i, xirius_prefs::keymap::Issue::Conflict { .. }))
                )
            });
        }
        4 => press(studio, Do::Sub(Sub::Mouse), window, cx),
        5 => {
            let context = "roll.select.note-body.drag";
            press(
                studio,
                Do::Menu(MenuKind::MouseCell {
                    ctx: context.into(),
                    modifier: "Alt".into(),
                }),
                window,
                cx,
            );
            let at = read(studio, window, |s, _| {
                let menu = s.settings.as_ref().unwrap().menu.as_ref().unwrap();
                let cell = menu
                    .menu
                    .layout
                    .cells
                    .iter()
                    .find(|c| c.action == Some(crate::popup::Action::Pick("note.move".into())))
                    .expect("move choice");
                (
                    cell.rect.x + cell.rect.w / 2.0,
                    cell.rect.y + cell.rect.h / 2.0,
                )
            });
            click(at, 1, window, cx);
            read(studio, window, |s, _| {
                assert_eq!(
                    s.song
                        .settings()
                        .lock()
                        .unwrap()
                        .mousemap
                        .get(context, "Alt"),
                    "note.move"
                );
                assert!(
                    s.song
                        .settings()
                        .lock()
                        .unwrap()
                        .path(xirius_prefs::File::Mousemap)
                        .unwrap()
                        .exists()
                )
            });
        }
        6 => {
            press(studio, Do::Close, window, cx);
            let at = read(studio, window, |s, g| {
                let RowKind::Track(t) = s.selected_row.unwrap() else {
                    panic!("track");
                };
                let row = s.tree().row_of(RowKind::Track(t)).unwrap();
                let parts = RowParts::of(row);
                (
                    g.column.x + parts.name_x + 3.0,
                    g.column.y + row.y + RowParts::LINE_1_Y + 3.0,
                )
            });
            click(at, 2, window, cx);
            assert!(studio.borrow().input_map.name.is_some());
        }
        7 => {
            let before = studio.borrow().song.steps();
            for letter in ["q", "1", "5"] {
                key(letter, window, cx);
            }
            let s = studio.borrow();
            assert!(
                s.input_map
                    .name
                    .as_ref()
                    .unwrap()
                    .field
                    .value
                    .ends_with("q15")
            );
            assert_eq!(s.song.steps(), before);
        }
        8 => {
            key("enter", window, cx);
            let before = studio.borrow().song.snap();
            key("n", window, cx);
            let s = studio.borrow();
            assert_eq!(s.song.snap(), !before);
            assert!(s.error.is_none(), "{:?}", s.error);
        }
        9 => {
            let at = read(studio, window, |s, g| {
                let (t, c) = s.selected_clip.unwrap();
                let clip = s.project().tracks[&t]
                    .clips
                    .iter()
                    .find(|clip| clip.id == c)
                    .unwrap()
                    .clone();
                let row = s.tree().row_of(RowKind::Track(t)).unwrap();
                (
                    s.tick_x(g, clip.start) + 20.0,
                    g.lanes.y + row.y - s.scroll.1 + row.h / 2.0,
                )
            });
            click(at, 1, window, cx);
            for k in ["a", "j", "k", "h", "shift-h"] {
                key(k, window, cx);
            }
            read(studio, window, |s, _| {
                let RowKind::Track(t) = s.selected_row.unwrap() else {
                    panic!("track")
                };
                assert!(s.arrange.open.contains(&t));
                assert_eq!(
                    s.project().tracks[&t].auto_mode,
                    xirius_core::auto::AutoMode::Touch
                );
                assert!(s.error.is_none(), "{:?}", s.error);
            });
        }
        _ => {
            let before = studio.borrow().zoom_snapshot();
            for k in [
                "e", "shift-e", "alt-s", "shift-s", "shift-s", "alt-w", "alt-e",
            ] {
                key(k, window, cx);
            }
            assert_ne!(studio.borrow().zoom_snapshot(), before);
            assert!(studio.borrow().error.is_none());
        }
    }
}
