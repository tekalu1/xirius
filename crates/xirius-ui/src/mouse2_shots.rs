//! KEY-016/017: the settings' mouse table for the controller lanes, the automation lanes and the
//! ruler, through the offscreen window's pointer handlers.
use crate::keymap_shots::{click, press, read};
use crate::settings::{Do, MenuKind, Section, Sub};
use crate::shot::{Act, Play, Shot};
use crate::studio::{Hit, Studio};
use gpui::{App, Window};
use std::cell::RefCell;
use std::rc::Rc;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const MOUSE2: &[Shot] = &[
    Shot {
        name: "01-lane",
        acts: &[stage(0)],
    },
    Shot {
        name: "02-lane-draw",
        acts: &[stage(1)],
    },
    Shot {
        name: "03-automation",
        acts: &[stage(2)],
    },
    Shot {
        name: "04-ruler",
        acts: &[stage(3)],
    },
    Shot {
        name: "05-ruler-wheel-remapped",
        acts: &[stage(4)],
    },
];

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    match n {
        0 => {
            // A scripted run has a private folder even if the caller's config is the real one.
            let dir =
                std::env::temp_dir().join(format!("xirius-mouse2-shots-{}", std::process::id()));
            studio.borrow_mut().song.load_settings(&dir);
            let at = read(studio, window, |s, g| {
                s.center_of(g, Hit::RailSettings).unwrap()
            });
            click(at, 1, window, cx);
            press(studio, Do::Nav(Section::Keys), window, cx);
            press(studio, Do::Sub(Sub::Mouse), window, cx);
            press(studio, Do::Area("lane".into()), window, cx);
            read(studio, window, |s, _| {
                assert_eq!(s.settings.as_ref().unwrap().area, "lane");
            });
        }
        1 => {
            press(studio, Do::MouseTool("draw".into()), window, cx);
            read(studio, window, |s, _| {
                assert_eq!(s.settings.as_ref().unwrap().tool, "draw");
            });
        }
        2 => {
            press(studio, Do::Area("automation".into()), window, cx);
            read(studio, window, |s, _| {
                let ui = s.settings.as_ref().unwrap();
                assert_eq!(
                    (ui.area.as_str(), ui.tool.as_str()),
                    ("automation", "select")
                );
            });
        }
        3 => press(studio, Do::Area("ruler".into()), window, cx),
        _ => {
            let context = "ruler.any.empty.wheel";
            press(
                studio,
                Do::Menu(MenuKind::MouseCell {
                    ctx: context.into(),
                    modifier: "Mod".into(),
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
                    .find(|c| c.action == Some(crate::popup::Action::Pick("view.zoom.time".into())))
                    .expect("the zoom choice");
                (
                    cell.rect.x + cell.rect.w / 2.0,
                    cell.rect.y + cell.rect.h / 2.0,
                )
            });
            click(at, 1, window, cx);
            read(studio, window, |s, _| {
                let settings = s.song.settings().lock().unwrap();
                assert_eq!(settings.mousemap.get(context, "Mod"), "view.zoom.time");
                assert!(
                    settings
                        .path(xirius_prefs::File::Mousemap)
                        .unwrap()
                        .exists()
                );
            });
        }
    }
}
