//! ENG-008: the heavy plugin's mark in the mixer, the CPU list of the top bar and what its rows
//! do (the check turns the plugin off, a double press lights its slot, the right press opens its
//! menu), through the offscreen window's pointer handlers. The readings are laid by the script
//! (`Perf::fake`), so no sound device is asked.
use crate::arrange::probe::track;
use crate::cpu_list::{CAct, Fake};
use crate::popup::{Action, PopupKind};
use crate::shot::{Act, Play, Shot};
use crate::studio::{DrawerTab, Studio};
use crate::topbar::BarButton;
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use serde_json::json;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use xirius_design::Layout;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "01-heavy-mark",
        acts: &[Act::Reset, stage(0)],
    },
    Shot {
        name: "02-cpu-list",
        acts: &[stage(1)],
    },
    Shot {
        name: "03-row-menu",
        acts: &[stage(2)],
    },
    Shot {
        name: "04-deactivated",
        acts: &[stage(3), stage(4)],
    },
    Shot {
        name: "05-slot-lit",
        acts: &[stage(5)],
    },
    Shot {
        name: "06-activated-again",
        acts: &[stage(6)],
    },
];

fn press(p: (f32, f32), button: MouseButton, clicks: usize, window: &mut Window, cx: &mut App) {
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
            click_count: clicks,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position: p,
            button,
            modifiers: Modifiers::default(),
            click_count: clicks,
        }),
        cx,
    );
}

fn add_effect(s: &mut Studio, name: &str, effect: &str) {
    let t = track(s, name);
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": effect, "kind": "effect", "format": "builtin", "uid": effect }),
    ));
}

fn insert(s: &Studio, track_name: &str, name: &str) -> xirius_core::id::PluginId {
    s.project().tracks[&track(s, track_name)]
        .inserts
        .iter()
        .find(|p| p.name == name)
        .expect("the insert")
        .id
}

/// The middle of the list's cell with this action.
fn cell(studio: &Rc<RefCell<Studio>>, act: &Action, window: &mut Window) -> (f32, f32) {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    let layout = s.popup.as_ref().expect("the list").layout(g.size);
    let c = layout
        .cells
        .iter()
        .find(|c| c.action.as_ref() == Some(act))
        .expect("the cell");
    (c.rect.x + c.rect.w / 2.0, c.rect.y + c.rect.h / 2.0)
}

fn open_list(studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let at = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        let r = s.bar_rect(&g, BarButton::Cpu);
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    };
    press(at, MouseButton::Left, 1, window, cx);
    assert_eq!(
        studio.borrow().popup.as_ref().map(|p| p.kind),
        Some(PopupKind::Perf)
    );
}

fn choose(studio: &Rc<RefCell<Studio>>, label: &str, window: &mut Window, cx: &mut App) {
    let at = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        let layout = s.popup.as_ref().expect("menu").layout(g.size);
        let c = layout
            .cells
            .iter()
            .find(|c| c.text == label)
            .unwrap_or_else(|| panic!("missing row {label}"));
        assert!(c.live(), "disabled row {label}");
        (c.rect.x + c.rect.w / 2.0, c.rect.y + c.rect.h / 2.0)
    };
    press(at, MouseButton::Left, 1, window, cx);
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let compressor = |s: &Studio| insert(s, "Vocal", "Compressor");
    match n {
        0 => {
            let mut s = studio.borrow_mut();
            s.drawer_open = true;
            s.tab = DrawerTab::Mixer;
            add_effect(&mut s, "Vocal", "Compressor");
            add_effect(&mut s, "Vocal", "Reverb");
            add_effect(&mut s, "Guitar", "Delay");
            let loads = BTreeMap::from([
                (insert(&s, "Vocal", "Compressor"), 0.24),
                (insert(&s, "Vocal", "Reverb"), 0.09),
                (insert(&s, "Guitar", "Delay"), 0.04),
            ]);
            s.perf.fake = Some(Fake {
                load: 0.52,
                xruns: 0,
                loads,
            });
            s.poll_perf_now();
            let g = crate::screen::geometry(&s, window);
            s.resized(&g);
            assert!(s.perf.heavy.is_heavy(insert(&s, "Vocal", "Compressor")));
        }
        1 => {
            open_list(studio, window, cx);
            assert_eq!(studio.borrow().perf_view().rows.len(), 3);
        }
        2 => {
            let id = compressor(&studio.borrow());
            let at = cell(studio, &Action::Cpu(CAct::Pick(id)), window);
            press(at, MouseButton::Right, 1, window, cx);
            assert!(studio.borrow().context_open());
        }
        3 => choose(studio, "無効にする", window, cx),
        4 => {
            let id = compressor(&studio.borrow());
            assert!(
                studio
                    .borrow()
                    .project()
                    .tracks
                    .values()
                    .flat_map(|t| &t.inserts)
                    .any(|p| p.id == id && p.deactivated),
                "the plugin is off"
            );
            open_list(studio, window, cx);
        }
        5 => {
            let id = insert(&studio.borrow(), "Vocal", "Reverb");
            let at = cell(studio, &Action::Cpu(CAct::Pick(id)), window);
            press(at, MouseButton::Left, 2, window, cx);
            assert!(studio.borrow().perf.pinged(id));
        }
        _ => {
            let id = compressor(&studio.borrow());
            let at = cell(studio, &Action::Cpu(CAct::Toggle(id)), window);
            press(at, MouseButton::Left, 1, window, cx);
            assert!(
                !studio
                    .borrow()
                    .project()
                    .tracks
                    .values()
                    .flat_map(|t| &t.inserts)
                    .any(|p| p.id == id && p.deactivated)
            );
        }
    }
}
