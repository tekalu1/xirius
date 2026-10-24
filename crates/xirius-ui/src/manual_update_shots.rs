//! UI-344: the three manual entries through this window's input path, plus the shared
//! discovered state. Uses GitHub for checks only; never downloads or starts an installer.
use std::{cell::RefCell, rc::Rc};

use gpui::{
    App, Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PlatformInput, Window, point, px,
};
use xirius_design::Layout;

use crate::{
    about,
    settings::{Do, Section},
    shot::{Act, Play, Shot},
    studio::{Hit, Studio},
    topbar::BarButton,
};

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}
pub const SHOTS: &[Shot] = &[
    Shot {
        name: "manual-01-settings",
        acts: &[stage(0)],
    },
    Shot {
        name: "manual-02-about",
        acts: &[stage(1)],
    },
    Shot {
        name: "manual-03-palette",
        acts: &[stage(2)],
    },
    Shot {
        name: "manual-04-palette-result",
        acts: &[stage(3)],
    },
    Shot {
        name: "manual-05-discovered-settings",
        acts: &[stage(4)],
    },
    Shot {
        name: "manual-06-discovered-about",
        acts: &[stage(5)],
    },
    Shot {
        name: "manual-07-command-result",
        acts: &[stage(6)],
    },
    Shot {
        name: "manual-08-checking-ready",
        acts: &[stage(7)],
    },
    Shot {
        name: "manual-09-checking-download",
        acts: &[stage(8)],
    },
];
fn key(text: &str, w: &mut Window, cx: &mut App) {
    w.dispatch_keystroke(Keystroke::parse(text).unwrap(), cx);
}
fn click(p: (f32, f32), w: &mut Window, cx: &mut App) {
    let position = point(px(p.0), px(p.1 + Layout::TOP_BAR_H));
    w.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
    w.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    w.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}
fn settle(s: &mut Studio) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(12);
    while s.update.asking() && std::time::Instant::now() < until {
        s.poll_update();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!s.update.asking());
}
thread_local! {
    static CHECK_GATE: RefCell<Option<std::sync::mpsc::Sender<()>>> = const { RefCell::new(None) };
}
fn play(n: u8, studio: &Rc<RefCell<Studio>>, w: &mut Window, cx: &mut App) {
    let g = studio.borrow().geometry(
        f32::from(w.viewport_size().width),
        f32::from(w.viewport_size().height) - Layout::TOP_BAR_H - Layout::BOTTOM_BAR_H,
    );
    CHECK_GATE.with(|gate| {
        if let Some(send) = gate.borrow_mut().take() {
            let _ = send.send(());
            settle(&mut studio.borrow_mut());
        }
    });
    match n {
        0 => {
            let p = studio.borrow().center_of(&g, Hit::RailSettings).unwrap();
            click(p, w, cx);
            if std::env::var_os("XIRIUS_UPDATE_SHOTS_LIGHT").is_some() {
                let p = crate::audioset_shots::item(
                    &studio.borrow(),
                    &g,
                    Do::Pref("ui.theme".into(), "light".into()),
                );
                click(p, w, cx);
            }
            let p = crate::audioset_shots::item(
                &studio.borrow(),
                &g,
                Do::Pref(xirius_session::app::UPDATE_PREF.into(), "false".into()),
            );
            click(p, w, cx);
            let p =
                crate::audioset_shots::item(&studio.borrow(), &g, Do::Update(about::Act::Check));
            click(p, w, cx);
            settle(&mut studio.borrow_mut());
        }
        1 => {
            key("escape", w, cx);
            let p = studio
                .borrow()
                .center_of(&g, Hit::Bar(BarButton::Logo))
                .unwrap();
            click(p, w, cx);
            let p = {
                let s = studio.borrow();
                let l = s.popup.as_ref().unwrap().layout(g.size);
                let c = l
                    .cells
                    .iter()
                    .find(|c| c.action == Some(crate::popup::Action::About(about::Act::Check)))
                    .unwrap();
                (c.rect.x + c.rect.w / 2.0, c.rect.y + c.rect.h / 2.0)
            };
            click(p, w, cx);
            settle(&mut studio.borrow_mut());
        }
        2 => {
            key("escape", w, cx);
            key("ctrl-k", w, cx);
            for c in "更新".chars() {
                key(&c.to_string(), w, cx);
            }
        }
        3 => {
            key("enter", w, cx);
            settle(&mut studio.borrow_mut());
            assert!(
                studio
                    .borrow()
                    .popup
                    .as_ref()
                    .is_some_and(|p| p.about.is_some())
            );
        }
        4 => {
            key("escape", w, cx);
            // Same fixture as UI-282's update script; no downloads or installation.
            crate::update_shots::stage(&mut studio.borrow_mut(), 0);
            let p = studio.borrow().center_of(&g, Hit::RailSettings).unwrap();
            click(p, w, cx);
            assert_eq!(
                studio.borrow().settings.as_ref().unwrap().section,
                Section::General
            );
        }
        5 => {
            key("escape", w, cx);
            let p = studio
                .borrow()
                .center_of(&g, Hit::Bar(BarButton::Logo))
                .unwrap();
            click(p, w, cx);
        }
        6 => {
            key("escape", w, cx);
            key("ctrl-k", w, cx);
            for c in "project.get".chars() {
                key(&c.to_string(), w, cx);
            }
            key("enter", w, cx);
            assert!(
                studio
                    .borrow()
                    .palette
                    .as_ref()
                    .and_then(|p| p.form.as_ref())
                    .is_some_and(|f| f.output)
            );
        }
        7 | 8 => {
            key("escape", w, cx);
            let mut s = studio.borrow_mut();
            crate::update_shots::stage(&mut s, if n == 7 { 2 } else { 0 });
            let (send, receive) = std::sync::mpsc::channel();
            CHECK_GATE.with(|gate| *gate.borrow_mut() = Some(send));
            let receive = std::sync::Mutex::new(receive);
            s.update.fetch = std::sync::Arc::new(move |_| {
                receive
                    .lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(30))
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::json!({"tag_name":"v1.1.0", "html_url":"https://github.com/tekalu1/xirius/releases/tag/v1.1.0"}).to_string())
            });
            s.manual_update_check();
            let anchor = s.about_anchor(&g);
            s.open_about(anchor);
            let cells = about::update_cells(&s.update_view(), 440.0);
            assert!(!cells.iter().any(|c| matches!(
                c.action,
                Some(crate::popup::Action::About(
                    about::Act::Download | about::Act::UpdateNow
                ))
            )));
        }
        _ => unreachable!(),
    }
}
