//! Mixer operations driven through the window's mouse and keyboard events.
use crate::browser::{BRow, RailTab};
use crate::mixer::{List, MixerHit, list_row};
use crate::shot::{Act, Play, Shot};
use crate::studio::{DrawerTab, Geometry, Hit, Rect, Studio};
use gpui::{
    App, KeyDownEvent, Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PlatformInput, ScrollDelta, ScrollWheelEvent, TouchPhase, Window, point, px,
};
use serde_json::json;
use std::cell::RefCell;
use std::rc::Rc;
use xirius_core::id::{PluginId, TrackId};
use xirius_design::Layout;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}
pub const MIXER: &[Shot] = &[
    Shot {
        name: "m01-browser-to-track",
        acts: &[stage(0)],
    },
    Shot {
        name: "m02-browser-to-master",
        acts: &[stage(1)],
    },
    Shot {
        name: "m03-alt-copy",
        acts: &[stage(2)],
    },
    Shot {
        name: "m04-move",
        acts: &[stage(3)],
    },
    Shot {
        name: "m05-picker-search-enter",
        acts: &[stage(4)],
    },
    Shot {
        name: "m06-picker-drag",
        acts: &[stage(5)],
    },
    Shot {
        name: "m07-reorder-and-remove",
        acts: &[stage(6)],
    },
    Shot {
        name: "m08-header-chain",
        acts: &[stage(7)],
    },
    Shot {
        name: "m09-preset-file",
        acts: &[stage(8)],
    },
    Shot {
        name: "m10-many-inserts-top",
        acts: &[stage(9)],
    },
    Shot {
        name: "m11-many-inserts-bottom",
        acts: &[stage(10)],
    },
    Shot {
        name: "m12-automation",
        acts: &[stage(11)],
    },
    Shot {
        name: "m13-vst3-from-browser",
        acts: &[stage(12)],
    },
    Shot {
        name: "m14-vst3-window",
        acts: &[stage(13)],
    },
    Shot {
        name: "m15-vst3-preset-file",
        acts: &[stage(14)],
    },
    Shot {
        name: "m16-browser-click-enter",
        acts: &[stage(15)],
    },
];

fn read<T>(s: &Rc<RefCell<Studio>>, w: &Window, f: impl FnOnce(&Studio, &Geometry) -> T) -> T {
    let s = s.borrow();
    let g = crate::screen::geometry(&s, w);
    f(&s, &g)
}
fn mid(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}
fn send(s: (f32, f32), to: (f32, f32), drag: bool, alt: bool, w: &mut Window, cx: &mut App) {
    let mods = Modifiers {
        alt,
        ..Modifiers::default()
    };
    let at = |p: (f32, f32)| point(px(p.0), px(p.1 + Layout::TOP_BAR_H));
    w.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at(s),
            pressed_button: None,
            modifiers: mods,
        }),
        cx,
    );
    w.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: at(s),
            modifiers: mods,
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    if drag {
        for i in 1..=4 {
            let f = i as f32 / 4.0;
            w.dispatch_event(
                PlatformInput::MouseMove(MouseMoveEvent {
                    position: at((s.0 + (to.0 - s.0) * f, s.1 + (to.1 - s.1) * f)),
                    pressed_button: Some(MouseButton::Left),
                    modifiers: mods,
                }),
                cx,
            );
        }
    }
    w.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: at(to),
            modifiers: mods,
            click_count: 1,
        }),
        cx,
    );
}
fn click(p: (f32, f32), w: &mut Window, cx: &mut App) {
    send(p, p, false, false, w, cx);
}
fn key(k: &str, ch: Option<&str>, w: &mut Window, cx: &mut App) {
    w.dispatch_event(
        PlatformInput::KeyDown(KeyDownEvent {
            keystroke: Keystroke {
                modifiers: Modifiers::default(),
                key: k.into(),
                key_char: ch.map(str::to_owned),
            },
            is_held: false,
            prefer_character_input: false,
        }),
        cx,
    );
}
fn track(s: &Studio, name: &str) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == name)
        .unwrap()
        .id
}
fn list(s: &Studio, g: &Geometry, t: TrackId) -> Rect {
    s.mixer_strips(g)
        .into_iter()
        .find(|r| r.track == t)
        .unwrap()
        .inserts
}
fn origin(s: &Studio, g: &Geometry, t: TrackId, id: PluginId) -> (f32, f32) {
    let k = s.project().tracks[&t]
        .inserts
        .iter()
        .position(|p| p.id == id)
        .unwrap()
        + crate::instrument::gap(&s.project().tracks[&t]);
    let r = list_row(list(s, g, t), k - s.mixer.offset(t, List::Inserts));
    (r.x + 24.0, r.y + 11.0)
}
fn browser(s: &Studio, g: &Geometry, name: &str) -> (f32, f32) {
    let l = s.browser.layout(g.panel);
    let r = l
        .rows
        .iter()
        .find(|r| matches!(&r.row,BRow::Item{item,..} if item.name==name))
        .unwrap();
    let r = s.browser.row_rect(&l, r);
    (r.x + 40.0, r.y + r.h / 2.0)
}
fn clear(s: &Rc<RefCell<Studio>>) {
    let mut s = s.borrow_mut();
    s.instruments.open.clear();
    s.popup = None;
    s.mixer.preset_panel = None;
}

fn reveal_header(s: &Rc<RefCell<Studio>>, t: TrackId, w: &mut Window, cx: &mut App) {
    let (p, dy) = read(s, w, |s, g| {
        let row = s.tree().row_of(crate::tree::RowKind::Track(t)).unwrap();
        (
            (g.column.x + 12.0, g.column.y + 20.0),
            (s.row_y(g, row) + row.h - g.column.bottom() + 16.0).max(0.0),
        )
    });
    if dy > 0.0 {
        w.dispatch_event(
            PlatformInput::ScrollWheel(ScrollWheelEvent {
                position: point(px(p.0), px(p.1 + Layout::TOP_BAR_H)),
                delta: ScrollDelta::Pixels(point(px(0.0), px(-dy))),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
            }),
            cx,
        );
    }
}

fn play(n: u8, s: &Rc<RefCell<Studio>>, w: &mut Window, cx: &mut App) {
    clear(s);
    if n == 0 {
        {
            let mut s = s.borrow_mut();
            s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
            s.browser.open = true;
            s.browser.tab = RailTab::Effects;
            s.browser.effect_view = crate::browser::View::Name;
            s.use_plugins(xirius_plugins::Catalog::default());
        }
        let tab = read(s, w, |s, g| {
            s.center_of(g, Hit::Tab(DrawerTab::Mixer)).unwrap()
        });
        click(tab, w, cx);
    }
    let (a, b) = read(s, w, |s, _| (track(s, "Guitar"), track(s, "Bass")));
    match n {
        0 | 1 => {
            let t = if n == 0 { a } else { TrackId::MASTER };
            let (from, to) = read(s, w, |s, g| (browser(s, g, "EQ"), mid(list(s, g, t))));
            send(from, to, true, false, w, cx);
            assert!(
                s.borrow().project().tracks[&t]
                    .inserts
                    .iter()
                    .any(|p| p.name == "EQ")
            );
            assert!(!s.borrow().instruments.open.is_empty());
        }
        2 | 3 => {
            let id = s.borrow().project().tracks[&a]
                .inserts
                .iter()
                .find(|p| p.name == "EQ")
                .unwrap()
                .id;
            s.borrow_mut().mixer.set_insert_offset(a, 0);
            let (from, to) = read(s, w, |s, g| (origin(s, g, a, id), mid(list(s, g, b))));
            send(from, to, true, n == 2, w, cx);
            let p = s.borrow().project();
            assert!(p.tracks[&b].inserts.iter().any(|p| p.name == "EQ"));
            assert_eq!(p.tracks[&a].inserts.iter().any(|p| p.id == id), n == 2);
        }
        4 => {
            let at = read(s, w, |s, g| {
                s.mixer_center(g, MixerHit::AddInsert(a)).unwrap()
            });
            click(at, w, cx);
            key("d", Some("Delay"), w, cx);
            key("enter", None, w, cx);
            assert!(
                s.borrow().project().tracks[&a]
                    .inserts
                    .iter()
                    .any(|p| p.name == "Delay")
            );
            assert!(!s.borrow().instruments.open.is_empty());
        }
        5 => {
            let at = read(s, w, |s, g| {
                s.mixer_center(g, MixerHit::AddInsert(a)).unwrap()
            });
            click(at, w, cx);
            let (from, to) = read(s, w, |s, g| {
                let p = s.popup.as_ref().unwrap();
                let c=p.layout(g.size).cells.into_iter().find(|c|matches!(&c.action,Some(crate::popup::Action::Pick(t)) if t=="builtin:reverb")).unwrap();
                (mid(c.rect), mid(list(s, g, TrackId::MASTER)))
            });
            send(from, to, true, false, w, cx);
            assert!(
                s.borrow().project().tracks[&TrackId::MASTER]
                    .inserts
                    .iter()
                    .any(|p| p.name == "Reverb")
            );
        }
        6 => {
            let ids = s.borrow().project().tracks[&b]
                .inserts
                .iter()
                .map(|p| p.id)
                .collect::<Vec<_>>();
            s.borrow_mut().mixer.set_insert_offset(b, 0);
            let (from, to) = read(s, w, |s, g| {
                (origin(s, g, b, ids[0]), {
                    let r = list(s, g, b);
                    (r.x + 24.0, r.bottom() - 2.0)
                })
            });
            send(from, to, true, false, w, cx);
            assert_ne!(s.borrow().project().tracks[&b].inserts[0].id, ids[0]);
            let r = read(s, w, |s, g| {
                crate::fx_chain::remove_part(list_row(
                    list(s, g, b),
                    crate::instrument::gap(&s.project().tracks[&b]),
                ))
            });
            let before = s.borrow().project().tracks[&b].inserts.len();
            click(mid(r), w, cx);
            assert_eq!(s.borrow().project().tracks[&b].inserts.len(), before - 1);
        }
        7 => {
            reveal_header(s, a, w, cx);
            let at = read(s, w, |s, g| s.center_of(g, Hit::Widen(a)).unwrap());
            click(at, w, cx);
            {
                let mut s = s.borrow_mut();
                s.mixer.set_insert_offset(a, 0);
            }
            reveal_header(s, a, w, cx);
            let tab = read(s, w, |s, g| {
                s.center_of(g, Hit::Tab(DrawerTab::Roll)).unwrap()
            });
            click(tab, w, cx);
            let (from, to) = read(s, w, |s, g| {
                (
                    browser(s, g, "Compressor"),
                    mid(s.fx_header_rect(g, a).unwrap()),
                )
            });
            send(from, to, true, false, w, cx);
            assert!(
                s.borrow().project().tracks[&a]
                    .inserts
                    .iter()
                    .any(|p| p.name == "Compressor")
            );
        }
        8 => {
            let tab = read(s, w, |s, g| {
                s.center_of(g, Hit::Tab(DrawerTab::Mixer)).unwrap()
            });
            click(tab, w, cx);
            {
                let mut s = s.borrow_mut();
                // The drawer takes the mixer's height in a slide (UI-011); the hand moves once it has come to rest.
                s.motion.enabled.set(false);
                crate::screen::geometry(&s, w);
                s.motion.enabled.set(true);
                s.mixer.set_insert_offset(a, 0);
            }
            let (id, part) = read(s, w, |s, g| {
                (
                    s.project().tracks[&a].inserts[0].id,
                    crate::fx_preset::part(list_row(list(s, g, a), 0)),
                )
            });
            click(mid(part), w, cx);
            let r = s.borrow().mixer.preset_panel.unwrap().rect;
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/mixer-preset.xirius-preset");
            s.borrow_mut().mixer.preset_file = Some(path.clone());
            click((r.x + 20.0, r.y + 38.0), w, cx);
            let before = s.borrow().project().tracks[&a].inserts[0].clone();
            s.borrow_mut().run_command(
                "plugin.preset.apply",
                json!({"id":id.to_string(),"values":[]}),
            );
            click(mid(part), w, cx);
            let r = s.borrow().mixer.preset_panel.unwrap().rect;
            s.borrow_mut().mixer.preset_file = Some(path);
            click((r.x + 20.0, r.y + 12.0), w, cx);
            assert_eq!(s.borrow().project().tracks[&a].inserts[0], before);
        }
        9 => {
            let mut s = s.borrow_mut();
            let drums = track(&s, "Drums");
            assert!(s.run_command(
                "plugin.add",
                json!({"track":drums.to_string(),"name":"Sampler","kind":"instrument"})
            ));
            let bus = s
                .arrange_run(
                    "track.add.bus",
                    json!({"name":"FX","sources":[a.to_string()]}),
                )
                .unwrap()["id"]
                .as_str()
                .unwrap()
                .to_owned();
            for _ in 0..9 {
                assert!(s.run_command(
                    "send.add",
                    json!({"ids":[a.to_string()],"to":bus,"level":-120.0})
                ));
            }
            for _ in 0..64 {
                assert!(s.run_command("plugin.add", json!({"track":a.to_string(),"name":"EQ"})));
            }
            s.mixer.set_insert_offset(a, 0);
        }
        10 => {
            let before = read(s, w, |s, g| {
                s.mixer_strips(g)
                    .into_iter()
                    .find(|r| r.track == a)
                    .unwrap()
            });
            let p = mid(before.inserts);
            for _ in 0..100 {
                w.dispatch_event(
                    PlatformInput::ScrollWheel(ScrollWheelEvent {
                        position: point(px(p.0), px(p.1 + Layout::TOP_BAR_H)),
                        delta: ScrollDelta::Pixels(point(px(0.0), px(-1.0))),
                        modifiers: Modifiers::default(),
                        touch_phase: TouchPhase::Moved,
                    }),
                    cx,
                );
            }
            let after = read(s, w, |s, g| {
                s.mixer_strips(g)
                    .into_iter()
                    .find(|r| r.track == a)
                    .unwrap()
            });
            assert_eq!(before, after);
            assert!(s.borrow().mixer.offset(a, List::Inserts) > 60);
        }
        11 => {
            let mut s = s.borrow_mut();
            for (target, value) in [("volume", -6.0), ("pan", 0.2)] {
                s.run_command(
                    "auto.lane.add",
                    json!({"track":a.to_string(),"target":target}),
                );
                s.run_command(
                    "auto.point.add",
                    json!({"track":a.to_string(),"target":target,"at":0,"value":value}),
                );
                s.run_command(
                    "auto.lane.hide",
                    json!({"track":a.to_string(),"target":target}),
                );
            }
        }
        12 => {
            let path =
                std::path::Path::new("C:/Program Files/Common Files/VST3/Kilohearts/kHs Gain.vst3");
            let entries =
                xirius_plugins::probe(path).expect("kHs Gain must be installed for this shot");
            let name = entries[0].name.clone();
            s.borrow_mut().use_plugins(xirius_plugins::Catalog {
                entries,
                failures: Vec::new(),
            });
            let (from, to) = read(s, w, |s, g| {
                (browser(s, g, &name), mid(list(s, g, TrackId::MASTER)))
            });
            send(from, to, true, false, w, cx);
            assert!(s.borrow().error.is_none(), "{:?}", s.borrow().error);
        }
        13 => {
            let mut s = s.borrow_mut();
            let p = s.project();
            let id = p.tracks[&TrackId::MASTER]
                .inserts
                .iter()
                .find(|p| p.format == "vst3")
                .unwrap()
                .id;
            let hwnd = s.mixer.editor_hwnd(id).expect("VST3 editor HWND");
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/mixer-vst3-editor.bmp");
            let started = std::time::Instant::now();
            let image = loop {
                s.mixer.pump_editors(std::time::Duration::from_millis(100));
                let image = xirius_vst3::capture_window(hwnd, &path).unwrap();
                if image.distinct_colors > 8 {
                    break image;
                }
                assert!(
                    started.elapsed() < std::time::Duration::from_secs(30),
                    "VST3 editor did not draw"
                );
            };
            eprintln!(
                "mixer-vst3 hwnd={hwnd} colors={} file={}",
                image.distinct_colors,
                path.display()
            );
        }
        14 => {
            let (id, pos) = {
                let s = s.borrow();
                let p = s.project();
                let (pos, x) = p.tracks[&TrackId::MASTER]
                    .inserts
                    .iter()
                    .enumerate()
                    .find(|(_, p)| p.format == "vst3")
                    .unwrap();
                (x.id, pos)
            };
            s.borrow_mut().mixer.set_insert_offset(TrackId::MASTER, pos);
            let part = read(s, w, |s, g| {
                crate::fx_preset::part(list_row(list(s, g, TrackId::MASTER), 0))
            });
            click(mid(part), w, cx);
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/mixer-vst3-preset.xirius-preset");
            let panel = s.borrow().mixer.preset_panel.unwrap().rect;
            s.borrow_mut().mixer.preset_file = Some(path.clone());
            click((panel.x + 20.0, panel.y + 38.0), w, cx);
            let before = s.borrow().project().tracks[&TrackId::MASTER]
                .inserts
                .iter()
                .find(|p| p.id == id)
                .unwrap()
                .clone();
            assert!(s.borrow_mut().run_command(
                "plugin.param.set",
                json!({"id":id.to_string(),"param":"Gain","value":0.4})
            ));
            click(mid(part), w, cx);
            let panel = s.borrow().mixer.preset_panel.unwrap().rect;
            s.borrow_mut().mixer.preset_file = Some(path);
            click((panel.x + 20.0, panel.y + 12.0), w, cx);
            assert_eq!(
                s.borrow().project().tracks[&TrackId::MASTER]
                    .inserts
                    .iter()
                    .find(|p| p.id == id)
                    .unwrap(),
                &before
            );
        }
        15 => {
            s.borrow_mut().selected_row = Some(crate::tree::RowKind::Track(a));
            let from = read(s, w, |s, g| browser(s, g, "Limiter"));
            click(from, w, cx);
            assert!(
                s.borrow().project().tracks[&a]
                    .inserts
                    .iter()
                    .any(|p| p.name == "Limiter")
            );
            clear(s);
            let before = s.borrow().project().tracks[&a].inserts.len();
            key("enter", None, w, cx);
            assert_eq!(s.borrow().project().tracks[&a].inserts.len(), before + 1);
        }
        _ => unreachable!(),
    }
    let s = s.borrow();
    eprintln!(
        "mixer-check stage={n} inserts={} editors={} builtins={} error={:?}",
        s.project().tracks[&a].inserts.len(),
        s.mixer.open_editors(),
        s.instruments.open.len(),
        s.error
    );
}
