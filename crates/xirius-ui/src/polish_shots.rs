//! Real-window pointer routes for the MVP UI finishing work.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use xirius_design::Layout;

use crate::mixer::MixerHit;
use crate::shot::{Act, Play, Shot};
use crate::studio::{DrawerTab, Hit, Studio};
use crate::tools::Tool;
use crate::topbar::BarButton;
use crate::tree::RowKind;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "p01-dock-closed",
        acts: &[stage(0)],
    },
    Shot {
        name: "p02-dock-reopened",
        acts: &[stage(1)],
    },
    Shot {
        name: "p03-mixer-selected",
        acts: &[stage(2)],
    },
    Shot {
        name: "p04-add-bus-menu",
        acts: &[stage(3)],
    },
    Shot {
        name: "p05-bus-added",
        acts: &[stage(4)],
    },
    Shot {
        name: "p06-mixer-1280",
        acts: &[stage(5)],
    },
    Shot {
        name: "p07-roll-1280",
        acts: &[stage(6)],
    },
    Shot {
        name: "p08-controller-draw",
        acts: &[stage(7)],
    },
    Shot {
        name: "p09-icon-tip",
        acts: &[stage(8)],
    },
    Shot {
        name: "p10-grid-popover",
        acts: &[stage(9)],
    },
    Shot {
        name: "p11-disabled-context",
        acts: &[stage(10)],
    },
    Shot {
        name: "p12-disabled-palette",
        acts: &[stage(11)],
    },
    Shot {
        name: "p13-track-selection",
        acts: &[stage(12)],
    },
    Shot {
        name: "p14-group-mute",
        acts: &[stage(13)],
    },
    Shot {
        name: "p15-group-solo",
        acts: &[stage(14)],
    },
    Shot {
        name: "p16-arm-and-monitor",
        acts: &[stage(15)],
    },
    Shot {
        name: "p17-group-mixer",
        acts: &[stage(16)],
    },
    Shot {
        name: "p18-group-volume",
        acts: &[stage(17)],
    },
    Shot {
        name: "p19-export-open",
        acts: &[stage(18)],
    },
    Shot {
        name: "p20-export-finished",
        acts: &[stage(19)],
    },
    Shot {
        name: "p21-export-stale-dot",
        acts: &[stage(20)],
    },
    Shot {
        name: "p22-ctrl-tool",
        acts: &[stage(21)],
    },
    Shot {
        name: "p23-grid-zoomed-out",
        acts: &[stage(22)],
    },
    Shot {
        name: "p24-960-bottom",
        acts: &[stage(23)],
    },
    Shot {
        name: "p25-960-padding",
        acts: &[stage(24)],
    },
    Shot {
        name: "p26-reduced-motion",
        acts: &[stage(25)],
    },
    Shot {
        name: "p27-arrange-grid-eighth",
        acts: &[stage(26)],
    },
    Shot {
        name: "p28-roll-grid-eighth",
        acts: &[stage(27)],
    },
    Shot {
        name: "p29-controller-tip",
        acts: &[stage(28)],
    },
    Shot {
        name: "p30-ai-permission-tip",
        acts: &[stage(29)],
    },
    Shot {
        name: "p31-disabled-monitor-tip",
        acts: &[stage(30)],
    },
];

fn click(
    hit: Hit,
    modifiers: Modifiers,
    studio: &Rc<RefCell<Studio>>,
    window: &mut Window,
    cx: &mut App,
) {
    let (x, y) = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        s.center_of(&g, hit).expect("visible pointer target")
    };
    pointer((x, y), modifiers, Some(MouseButton::Left), window, cx);
}

fn pointer(
    (x, y): (f32, f32),
    modifiers: Modifiers,
    button: Option<MouseButton>,
    window: &mut Window,
    cx: &mut App,
) {
    let position = point(px(x), px(y + Layout::TOP_BAR_H));
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers,
        }),
        cx,
    );
    let Some(button) = button else {
        return;
    };
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            position,
            button,
            modifiers,
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position,
            button,
            modifiers,
            click_count: 1,
        }),
        cx,
    );
}

fn named(s: &Studio, name: &str) -> xirius_core::id::TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == name)
        .map(|t| t.id)
        .expect("sample track")
}
fn item(text: &str, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let hit = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        let p = s.popup.as_ref().unwrap().layout(g.size);
        Hit::Item(
            p.cells
                .iter()
                .position(|c| c.text == text)
                .unwrap_or_else(|| panic!("missing {text}: {:?}", p.cells)),
        )
    };
    click(hit, Modifiers::default(), studio, window, cx);
}
fn eighth(studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    click(
        Hit::Bar(BarButton::Grid),
        Modifiers::default(),
        studio,
        window,
        cx,
    );
    if studio.borrow().popup.as_ref().unwrap().grid.ends_with('t') {
        let hit = {
            let s = studio.borrow();
            let g = crate::screen::geometry(&s, window);
            let l = s.popup.as_ref().unwrap().layout(g.size);
            Hit::Item(
                l.cells
                    .iter()
                    .position(|c| c.action == Some(crate::popup::Action::Triplet))
                    .unwrap(),
            )
        };
        click(hit, Modifiers::default(), studio, window, cx);
    }
    item("1/8", studio, window, cx);
}
fn close(studio: &Rc<RefCell<Studio>>) {
    let mut s = studio.borrow_mut();
    s.popup = None;
    s.ctx = None;
    s.palette = None;
}
fn drag(from: (f32, f32), to: (f32, f32), modifiers: Modifiers, window: &mut Window, cx: &mut App) {
    let position = |p: (f32, f32)| point(px(p.0), px(p.1 + Layout::TOP_BAR_H));
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: position(from),
            modifiers,
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: position(to),
            pressed_button: Some(MouseButton::Left),
            modifiers,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: position(to),
            modifiers,
            click_count: 1,
        }),
        cx,
    );
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    match n {
        0 | 1 => {
            if n == 0 {
                // The 1280-wide shots (p06, p07) do not depend on the window size the run was started at.
                window.resize(gpui::size(px(1280.0), px(816.0)));
            }
            click(
                Hit::Tab(DrawerTab::Roll),
                Modifiers::default(),
                studio,
                window,
                cx,
            );
            assert_eq!(studio.borrow().drawer_open, n == 1);
        }
        2 => {
            click(
                Hit::Tab(DrawerTab::Mixer),
                Modifiers::default(),
                studio,
                window,
                cx,
            );
            assert!(studio.borrow().drawer_open);
            assert_eq!(studio.borrow().tab, DrawerTab::Mixer);
        }
        3 => click(Hit::AddTrack, Modifiers::default(), studio, window, cx),
        4 => item("バス", studio, window, cx),
        5 => {
            let s = studio.borrow();
            let g = crate::screen::geometry(&s, window);
            assert_eq!(g.size.0, 1280.0);
            let strips = s.mixer_strips(&g);
            assert!(
                strips.iter().any(
                    |r| s.project().tracks[&r.track].kind == xirius_core::model::TrackKind::Bus
                )
            );
            // The master is pinned to the right edge, so only the channels sit a gap apart.
            let channels: Vec<_> = strips.iter().filter(|r| !r.track.is_master()).collect();
            assert!(
                channels.windows(2).all(|p| (p[1].rect.x
                    - p[0].rect.right()
                    - crate::mixer::STRIP_GAP)
                    .abs()
                    < 0.1)
            );
        }
        6 => click(
            Hit::Tab(DrawerTab::Roll),
            Modifiers::default(),
            studio,
            window,
            cx,
        ),
        7 => {
            click(
                Hit::Bar(BarButton::Tool(Tool::Draw)),
                Modifiers::default(),
                studio,
                window,
                cx,
            );
            let (from, to, before) = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let f = s.roll.frame(g.drawer_body);
                assert_eq!(f.grid.right(), g.drawer_body.right());
                let r = f.lanes[0].plot;
                (
                    (r.x + 60.0, r.y + 12.0),
                    (r.x + 230.0, r.y + 5.0),
                    s.song.steps(),
                )
            };
            drag(from, to, Modifiers::default(), window, cx);
            assert!(studio.borrow().song.steps() > before);
        }
        8 => {
            let s = studio.borrow();
            let g = crate::screen::geometry(&s, window);
            let at = s.center_of(&g, Hit::Bar(BarButton::Export)).unwrap();
            drop(s);
            pointer(at, Modifiers::default(), None, window, cx);
            assert!(
                crate::hover_tip::text(&studio.borrow(), Hit::Bar(BarButton::Export)).is_some()
            );
        }
        9 => click(
            Hit::Bar(BarButton::Grid),
            Modifiers::default(),
            studio,
            window,
            cx,
        ),
        10 => {
            close(studio);
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let c = s.roll_ctx(&g).unwrap();
                let v = s.roll.view(&c);
                (v.grid.right() - 30.0, v.grid.y + 6.0)
            };
            pointer(
                p,
                Modifiers::default(),
                Some(MouseButton::Right),
                window,
                cx,
            );
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let l = s.popup.as_ref().unwrap().layout(g.size);
                let c = l
                    .cells
                    .iter()
                    .find(|c| !c.enabled)
                    .expect("unavailable menu row");
                assert!(!c.tip.is_empty());
                (c.rect.x + 15.0, c.rect.y + c.rect.h / 2.0)
            };
            pointer(p, Modifiers::default(), None, window, cx);
        }
        11 => {
            close(studio);
            click(
                Hit::Bar(BarButton::Palette),
                Modifiers::default(),
                studio,
                window,
                cx,
            );
            let p = {
                let mut s = studio.borrow_mut();
                let g = crate::screen::geometry(&s, window);
                let p = s.palette.as_mut().unwrap();
                p.query = "clip.delete".into();
                p.selected = 0;
                p.offset = 0.0;
                let l = p.layout(g.size);
                let r = l.rows[0].1;
                (r.x + 15.0, r.y + r.h / 2.0)
            };
            pointer(p, Modifiers::default(), None, window, cx);
            assert!(studio.borrow().palette.as_ref().unwrap().hover.is_some());
        }
        12 => {
            close(studio);
            let (a, b) = {
                let s = studio.borrow();
                (named(&s, "Drums"), named(&s, "Bass"))
            };
            click(
                Hit::Row(RowKind::Track(a)),
                Modifiers::default(),
                studio,
                window,
                cx,
            );
            click(
                Hit::Row(RowKind::Track(b)),
                Modifiers {
                    shift: true,
                    ..Modifiers::default()
                },
                studio,
                window,
                cx,
            );
            assert_eq!(studio.borrow().track_targets(a).len(), 2);
        }
        13 | 14 => {
            let a = named(&studio.borrow(), "Drums");
            click(
                if n == 13 { Hit::Mute(a) } else { Hit::Solo(a) },
                Modifiers::default(),
                studio,
                window,
                cx,
            );
            let s = studio.borrow();
            assert!(s.track_targets(a).iter().all(|t| if n == 13 {
                s.project().tracks[t].mute
            } else {
                s.project().tracks[t].solo
            }));
        }
        15 => {
            let a = named(&studio.borrow(), "Drums");
            click(Hit::Arm(a), Modifiers::default(), studio, window, cx);
            let monitor = named(&studio.borrow(), "Vocal");
            {
                let mut s = studio.borrow_mut();
                let g = crate::screen::geometry(&s, window);
                let row = *s.tree().row_of(RowKind::Track(monitor)).unwrap();
                s.scroll.1 = (row.y + row.full_h() - g.lanes.h + 20.0).max(0.0);
                s.clamp_scroll(&g);
            }
            click(Hit::Arm(monitor), Modifiers::default(), studio, window, cx);
            click(
                Hit::Monitor(monitor),
                Modifiers::default(),
                studio,
                window,
                cx,
            );
            assert!(studio.borrow().project().tracks[&a].armed);
            assert!(studio.borrow().project().tracks[&monitor].monitor);
        }
        16 => click(
            Hit::Tab(DrawerTab::Mixer),
            Modifiers::default(),
            studio,
            window,
            cx,
        ),
        17 => {
            let (from, to, ids) = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let a = named(&s, "Drums");
                let r = s.mixer_rect(&g, MixerHit::Fader(a)).unwrap();
                (
                    (r.x + r.w / 2.0, r.y + r.h / 2.0),
                    (r.x + r.w / 2.0, r.y + r.h / 2.0 + 20.0),
                    s.track_targets(a),
                )
            };
            drag(from, to, Modifiers::default(), window, cx);
            let s = studio.borrow();
            assert_eq!(
                s.project().tracks[&ids[0]].volume_db,
                s.project().tracks[&ids[1]].volume_db
            );
        }
        18 => {
            {
                let mut s = studio.borrow_mut();
                let dir = std::env::current_dir()
                    .unwrap()
                    .join(format!("target/polish-export-song-{}", std::process::id()));
                s.command(|song| song.save_as(&dir));
                assert!(s.error.is_none(), "{:?}", s.error);
            }
            click(
                Hit::Bar(BarButton::Export),
                Modifiers::default(),
                studio,
                window,
                cx,
            );
        }
        19 => {
            let hit = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let l = s.popup.as_ref().unwrap().layout(g.size);
                Hit::Item(
                    l.cells
                        .iter()
                        .position(|c| {
                            c.action == Some(crate::popup::Action::Xp(crate::export::Act::Go))
                        })
                        .unwrap(),
                )
            };
            click(hit, Modifiers::default(), studio, window, cx);
            let start = std::time::Instant::now();
            loop {
                let mut s = studio.borrow_mut();
                s.poll_export();
                if !s.export.running() {
                    break;
                }
                assert!(start.elapsed().as_secs() < 30, "export timeout");
                drop(s);
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert_eq!(
                studio.borrow().export_view().cards[0].state,
                crate::export::State::Done
            );
            assert!(!studio.borrow().bar_view().export_stale);
        }
        20 => {
            close(studio);
            {
                let mut s = studio.borrow_mut();
                let g = crate::screen::geometry(&s, window);
                s.scroll_by(&g, 0.0, 10000.0, false);
            }
            let a = named(&studio.borrow(), "Drums");
            click(Hit::Mute(a), Modifiers::default(), studio, window, cx);
            assert!(studio.borrow().bar_view().export_stale);
        }
        21 => {
            click(
                Hit::Bar(BarButton::Tool(Tool::Select)),
                Modifiers::default(),
                studio,
                window,
                cx,
            );
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                (g.lanes.x + 200.0, g.lanes.y + 60.0)
            };
            pointer(
                p,
                Modifiers {
                    control: true,
                    ..Modifiers::default()
                },
                None,
                window,
                cx,
            );
            assert!(studio.borrow().arrange.mods.ctrl);
        }
        22 => {
            let mut s = studio.borrow_mut();
            let g = crate::screen::geometry(&s, window);
            s.zoom(&g, g.lanes.x, -20.0);
        }
        23 => {
            window.resize(gpui::size(px(960.0), px(740.0)));
            let mut s = studio.borrow_mut();
            s.motion.enabled.set(false);
            let g = s.geometry(960.0, 740.0 - Layout::TOP_BAR_H - Layout::BOTTOM_BAR_H);
            s.scroll_by(&g, 0.0, -10000.0, false);
        }
        24 => {
            let mut s = studio.borrow_mut();
            let g = crate::screen::geometry(&s, window);
            s.scroll_by(&g, 0.0, -10000.0, false);
            let r = s.tree().rows.last().unwrap();
            assert!(
                g.lanes.bottom() - s.row_y(&g, r) - r.full_h()
                    >= xirius_design::Space::X6.px() - 0.1
            );
        }
        25 => {
            studio.borrow_mut().ai.motion.reduced = true;
            studio.borrow().motion.enabled.set(true);
            click(
                Hit::Tab(DrawerTab::Mixer),
                Modifiers::default(),
                studio,
                window,
                cx,
            );
            assert!(!studio.borrow().motion.moving());
        }
        26 => {
            close(studio);
            window.resize(gpui::size(px(1280.0), px(816.0)));
            let p = {
                let mut s = studio.borrow_mut();
                let g = crate::screen::geometry(&s, window);
                s.scroll_by(&g, 0.0, 10000.0, false);
                (g.lanes.right() - 12.0, g.lanes.y + 20.0)
            };
            pointer(p, Modifiers::default(), Some(MouseButton::Left), window, cx);
            eighth(studio, window, cx);
            assert_eq!(studio.borrow().song.grid_for("arrange"), "1/8");
            close(studio);
        }
        27 => {
            click(
                Hit::Tab(DrawerTab::Roll),
                Modifiers::default(),
                studio,
                window,
                cx,
            );
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let r = s.roll.frame(g.drawer_body).grid;
                (r.right() - 12.0, r.y + 12.0)
            };
            pointer(p, Modifiers::default(), Some(MouseButton::Left), window, cx);
            eighth(studio, window, cx);
            assert_eq!(studio.borrow().song.grid_for("roll"), "1/8");
            close(studio);
        }
        28 => {
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let r = s.roll.frame(g.drawer_body).plus;
                (r.x + 5.0, r.y + 5.0)
            };
            pointer(p, Modifiers::default(), None, window, cx);
        }
        29 => {
            click(
                Hit::Bar(BarButton::Ai),
                Modifiers::default(),
                studio,
                window,
                cx,
            );
            let hit =
                {
                    let s = studio.borrow();
                    let g = crate::screen::geometry(&s, window);
                    let l = s.popup.as_ref().unwrap().layout(g.size);
                    assert!(!l.cells.iter().any(|c| matches!(
                        c.look,
                        crate::popup::Look::Ai(crate::ai::AiLook::About)
                    )));
                    Hit::Item(
                        l.cells
                            .iter()
                            .position(|c| {
                                matches!(
                                    c.action,
                                    Some(crate::popup::Action::Ai(crate::ai::Act::Level(_)))
                                )
                            })
                            .unwrap(),
                    )
                };
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                s.center_of(&g, hit).unwrap()
            };
            pointer(p, Modifiers::default(), None, window, cx);
        }
        30 => {
            close(studio);
            let (p, t) = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let t = named(&s, "Drums");
                let row = s.tree().row_of(RowKind::Track(t)).unwrap();
                let r = crate::studio::RowParts::of(row).ear.unwrap();
                (
                    (
                        g.column.x + r.x + r.w / 2.0,
                        s.row_y(&g, row) + r.y + r.h / 2.0,
                    ),
                    t,
                )
            };
            pointer(p, Modifiers::default(), None, window, cx);
            assert_eq!(studio.borrow().hover, Hit::None);
            assert!(crate::hover_tip::text(&studio.borrow(), Hit::Monitor(t)).is_some());
        }
        _ => unreachable!(),
    }
}
