//! MIX-007 / MIX-030 / PRJ-007: copying a channel's settings and the track presets, through the
//! offscreen window's pointer handlers (the right press opens the menu, a row is pressed at
//! its cell, the notice is pressed at its button, a name is dragged across the strips).
//! Run with `XIRIUS_TRACK_PRESETS` set to an empty folder, so the presets of the person
//! running it are not touched.
use crate::arrange::probe::track;
use crate::notice::{button_rect, place};
use crate::shot::{Act, Play, Shot};
use crate::studio::{DrawerTab, Studio};
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::rc::Rc;
use xirius_core::id::TrackId;
use xirius_design::Layout;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "01-channel-menu",
        acts: &[Act::Reset, stage(0), stage(1)],
    },
    Shot {
        name: "02-copied-paste-menu",
        acts: &[stage(2), stage(3)],
    },
    Shot {
        name: "03-pasted",
        acts: &[stage(4)],
    },
    Shot {
        name: "04-undone",
        acts: &[stage(5)],
    },
    Shot {
        name: "05-name-dragged",
        acts: &[stage(6)],
    },
    Shot {
        name: "06-name-dropped",
        acts: &[stage(7)],
    },
    Shot {
        name: "07-save-as-preset",
        acts: &[stage(8)],
    },
    Shot {
        name: "08-preset-saved",
        acts: &[stage(9)],
    },
    Shot {
        name: "09-apply-list",
        acts: &[stage(10), stage(11)],
    },
    Shot {
        name: "10-applied-with-adjustments",
        acts: &[stage(12), stage(13)],
    },
    Shot {
        name: "11-adjusted-list",
        acts: &[stage(14)],
    },
    Shot {
        name: "12-track-column-add",
        acts: &[stage(15), stage(16)],
    },
    Shot {
        name: "13-add-from-preset-list",
        acts: &[stage(17)],
    },
    Shot {
        name: "14-added-from-preset",
        acts: &[stage(18)],
    },
];

fn press(p: (f32, f32), button: MouseButton, window: &mut Window, cx: &mut App) {
    let p = point(px(p.0), px(p.1 + Layout::TOP_BAR_H));
    let pressed_button = None;
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: p,
            pressed_button,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
    down(p, button, window, cx);
    up(p, button, window, cx);
}

fn down(p: gpui::Point<gpui::Pixels>, button: MouseButton, window: &mut Window, cx: &mut App) {
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
}

fn up(p: gpui::Point<gpui::Pixels>, button: MouseButton, window: &mut Window, cx: &mut App) {
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

fn moved(p: (f32, f32), held: bool, window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: point(px(p.0), px(p.1 + Layout::TOP_BAR_H)),
            pressed_button: held.then_some(MouseButton::Left),
            modifiers: Modifiers::default(),
        }),
        cx,
    );
}

/// The middle of a channel's name in its strip.
fn name_of(studio: &Rc<RefCell<Studio>>, t: TrackId, window: &mut Window) -> (f32, f32) {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    let r = s
        .mixer_strips(&g)
        .into_iter()
        .find(|x| x.track == t)
        .expect("a strip")
        .name;
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

fn right_click(studio: &Rc<RefCell<Studio>>, t: TrackId, window: &mut Window, cx: &mut App) {
    let at = name_of(studio, t, window);
    press(at, MouseButton::Right, window, cx);
    assert!(
        studio.borrow().context_open(),
        "the right press opens a menu"
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

fn press_notice(studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let at = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        let text = s.notice_text().expect("a notice").to_string();
        let label = s.notice_button().expect("a button").to_string();
        let r = button_rect(place(&s, &g, &text, Some(&label)), &label);
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    };
    press(at, MouseButton::Left, window, cx);
}

fn inserts(s: &Studio, name: &str) -> Vec<String> {
    s.project().tracks[&track(s, name)]
        .inserts
        .iter()
        .map(|p| p.name.clone())
        .collect()
}

fn add_effect(s: &mut Studio, name: &str, effect: &str) {
    let t = track(s, name);
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": effect, "kind": "effect", "format": "builtin", "uid": effect }),
    ));
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let id = |name: &str| track(&studio.borrow(), name);
    match n {
        0 => {
            let mut s = studio.borrow_mut();
            s.drawer_open = true;
            s.tab = DrawerTab::Mixer;
            add_effect(&mut s, "Vocal", "Compressor");
            add_effect(&mut s, "Vocal", "Reverb");
            let g = crate::screen::geometry(&s, window);
            s.resized(&g);
        }
        1 => right_click(studio, id("Vocal"), window, cx),
        2 => choose(studio, "チャンネルの設定をコピー", window, cx),
        3 => {
            right_click(studio, id("Guitar"), window, cx);
            assert!(studio.borrow().popup.is_some());
        }
        4 => {
            choose(studio, "チャンネルの設定を貼り付け", window, cx);
            let s = studio.borrow();
            assert_eq!(inserts(&s, "Guitar"), ["Compressor", "Reverb"]);
            assert_eq!(s.notice_button(), Some("元に戻す"));
        }
        5 => {
            press_notice(studio, window, cx);
            assert!(inserts(&studio.borrow(), "Guitar").is_empty());
        }
        6 => {
            // The name of Vocal held and carried over the strip of Guitar.
            let from = name_of(studio, id("Vocal"), window);
            let to = name_of(studio, id("Guitar"), window);
            let p = point(px(from.0), px(from.1 + Layout::TOP_BAR_H));
            moved(from, false, window, cx);
            down(p, MouseButton::Left, window, cx);
            for k in 1..=6 {
                let f = k as f32 / 6.0;
                let at = (from.0 + (to.0 - from.0) * f, from.1 + (to.1 - from.1) * f);
                moved(at, true, window, cx);
            }
            assert!(studio.borrow().mixer.name_drag.is_some_and(|d| d.moved));
        }
        7 => {
            let to = name_of(studio, id("Guitar"), window);
            up(
                point(px(to.0), px(to.1 + Layout::TOP_BAR_H)),
                MouseButton::Left,
                window,
                cx,
            );
            assert_eq!(
                inserts(&studio.borrow(), "Guitar"),
                ["Compressor", "Reverb"]
            );
        }
        8 => {
            right_click(studio, id("Vocal"), window, cx);
            choose(studio, "トラックのプリセットとして保存…", window, cx);
            let mut s = studio.borrow_mut();
            assert!(s.ask_field().is_some(), "the menu asks for the name");
            for _ in 0..40 {
                s.ask_key("backspace");
            }
            s.ask_text(None, "ボーカル", false);
        }
        9 => {
            studio.borrow_mut().ask_key("enter");
            let s = studio.borrow();
            assert_eq!(
                s.notice_text(),
                Some("トラックのプリセット「ボーカル」を保存しました")
            );
        }
        10 => {
            // The song of another version: a volume out of range and a key not known here.
            let file = {
                let mut s = studio.borrow_mut();
                let data = s.song.run("track.preset.list", json!({})).unwrap();
                data["presets"][0]["file"].as_str().unwrap().to_string()
            };
            let mut v: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
            v["volume_db"] = json!(60.0);
            v["from_the_future"] = json!(1);
            std::fs::write(&file, serde_json::to_vec(&v).unwrap()).unwrap();
            // Guitar is cleared again so the applied preset has something to show.
            let g = id("Guitar");
            let mut s = studio.borrow_mut();
            let ids: Vec<String> = s.project().tracks[&g]
                .inserts
                .iter()
                .map(|p| p.id.to_string())
                .collect();
            assert!(s.run_command("plugin.remove", json!({ "ids": ids })));
            drop(s);
            right_click(studio, g, window, cx);
        }
        11 => choose(studio, "トラックのプリセットを適用", window, cx),
        12 => choose(studio, "ボーカル", window, cx),
        13 => {
            let s = studio.borrow();
            assert_eq!(s.notice_button(), Some("詳しく"));
        }
        14 => {
            press_notice(studio, window, cx);
            assert_eq!(
                studio.borrow().popup.as_ref().map(|p| p.kind),
                Some(crate::popup::PopupKind::Report)
            );
        }
        15 => {
            // The report is closed, and the track column is shown for its "トラックを追加".
            studio.borrow_mut().popup = None;
            let mut s = studio.borrow_mut();
            s.drawer_open = false;
            let g = crate::screen::geometry(&s, window);
            s.resized(&g);
        }
        16 => {
            let s = studio.borrow();
            let g = crate::screen::geometry(&s, window);
            let at = (g.column.x + g.column.w / 2.0, g.column.bottom() - 6.0);
            drop(s);
            press(at, MouseButton::Right, window, cx);
        }
        17 => choose(studio, "トラックを追加", window, cx),
        _ => {
            let before = studio.borrow().project().tracks.len();
            choose(studio, "ボーカル", window, cx);
            assert_eq!(studio.borrow().project().tracks.len(), before + 1);
        }
    }
}
