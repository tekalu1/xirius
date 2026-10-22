//! Editing gestures dispatched through the GPUI window.
use crate::{
    popup::PopupKind,
    shot::{Act, Play, Shot},
    studio::{DrawerTab, Geometry, Hit, Rect, Studio},
    tools::Tool,
    topbar::BarButton,
};
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use serde_json::json;
use std::{cell::RefCell, rc::Rc};
use xirius_design::Layout;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}
pub const EDIT3: &[Shot] = &[
    Shot {
        name: "e01-notes",
        acts: &[Act::Reset, stage(0)],
    },
    Shot {
        name: "e02-relative-velocity",
        acts: &[stage(1)],
    },
    Shot {
        name: "e03-note-join",
        acts: &[stage(2)],
    },
    Shot {
        name: "e04-double-delete",
        acts: &[stage(3)],
    },
    Shot {
        name: "e05-double-create",
        acts: &[stage(4)],
    },
    Shot {
        name: "e06-basic-arrow",
        acts: &[stage(5)],
    },
    Shot {
        name: "e07-keyboard-selection",
        acts: &[stage(6)],
    },
    Shot {
        name: "e08-keyboard-glissando",
        acts: &[stage(7)],
    },
    Shot {
        name: "e09-scale",
        acts: &[stage(8)],
    },
    Shot {
        name: "e10-velocity-shape",
        acts: &[stage(9)],
    },
    Shot {
        name: "e11-note-length",
        acts: &[stage(10)],
    },
    Shot {
        name: "e12-quantize-end",
        acts: &[stage(11)],
    },
    Shot {
        name: "e13-restore-timing",
        acts: &[stage(12)],
    },
    Shot {
        name: "e14-humanize",
        acts: &[stage(13)],
    },
    Shot {
        name: "e15-quantize-freeze",
        acts: &[stage(14)],
    },
    Shot {
        name: "e16-folded-lane",
        acts: &[stage(15)],
    },
    Shot {
        name: "e17-new-part",
        acts: &[stage(16)],
    },
    Shot {
        name: "e18-arrange-split",
        acts: &[Act::Reset, stage(17)],
    },
    Shot {
        name: "e19-arrange-join",
        acts: &[stage(18)],
    },
    Shot {
        name: "e20-link-range",
        acts: &[stage(19)],
    },
    Shot {
        name: "e21-range-split",
        acts: &[stage(20)],
    },
    Shot {
        name: "e22-track-selection",
        acts: &[stage(21)],
    },
    Shot {
        name: "e23-drawn-part",
        acts: &[stage(22)],
    },
    Shot {
        name: "e24-muted-sweep",
        acts: &[stage(23)],
    },
    Shot {
        name: "e25-middle-pan",
        acts: &[stage(24)],
    },
    Shot {
        name: "e26-erased-sweep",
        acts: &[stage(25)],
    },
    Shot {
        name: "e27-low-notes",
        acts: &[Act::Reset, stage(26)],
    },
    Shot {
        name: "e28-mousemap-eraser",
        acts: &[stage(27)],
    },
    Shot {
        name: "e29-alt-replacement",
        acts: &[stage(28)],
    },
    Shot {
        name: "e30-alternative-tool",
        acts: &[stage(29)],
    },
    Shot {
        name: "e31-roll-grid",
        acts: &[Act::Reset, stage(30)],
    },
    Shot {
        name: "e32-arrange-grid",
        acts: &[stage(31)],
    },
    Shot {
        name: "e33-external-groove",
        acts: &[stage(32)],
    },
    Shot {
        name: "e34-alt-part-cut",
        acts: &[Act::Reset, stage(33)],
    },
    Shot {
        name: "e35-context-eraser",
        acts: &[stage(34)],
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
fn down(
    w: &mut Window,
    cx: &mut App,
    p: (f32, f32),
    button: MouseButton,
    m: Modifiers,
    clicks: usize,
) {
    w.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button,
            position: point(px(p.0), px(p.1 + Layout::TOP_BAR_H)),
            modifiers: m,
            click_count: clicks,
            first_mouse: false,
        }),
        cx,
    );
}
fn up(
    w: &mut Window,
    cx: &mut App,
    p: (f32, f32),
    button: MouseButton,
    m: Modifiers,
    clicks: usize,
) {
    w.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button,
            position: point(px(p.0), px(p.1 + Layout::TOP_BAR_H)),
            modifiers: m,
            click_count: clicks,
        }),
        cx,
    );
}
fn moved(w: &mut Window, cx: &mut App, p: (f32, f32), button: MouseButton, m: Modifiers) {
    w.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: point(px(p.0), px(p.1 + Layout::TOP_BAR_H)),
            pressed_button: Some(button),
            modifiers: m,
        }),
        cx,
    );
}
fn click(w: &mut Window, cx: &mut App, p: (f32, f32), m: Modifiers, n: usize) {
    for clicks in 1..=n {
        down(w, cx, p, MouseButton::Left, m, clicks);
        up(w, cx, p, MouseButton::Left, m, clicks);
    }
}
fn drag(
    w: &mut Window,
    cx: &mut App,
    a: (f32, f32),
    b: (f32, f32),
    button: MouseButton,
    m: Modifiers,
) {
    down(w, cx, a, button, m, 1);
    for i in 1..=4 {
        let f = i as f32 / 4.0;
        moved(
            w,
            cx,
            (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f),
            button,
            m,
        );
    }
    up(w, cx, b, button, m, 1);
}
fn cell(s: &Rc<RefCell<Studio>>, w: &mut Window, cx: &mut App, text: &str) {
    let p = read(s, w, |s, g| {
        mid(s
            .popup
            .as_ref()
            .unwrap()
            .layout(g.size)
            .cells
            .iter()
            .find(|c| c.text == text)
            .unwrap_or_else(|| panic!("cell {text}"))
            .rect)
    });
    click(w, cx, p, Modifiers::default(), 1);
    assert!(s.borrow().error.is_none(), "{:?}", s.borrow().error);
}
fn palette(s: &Rc<RefCell<Studio>>, w: &mut Window, cx: &mut App, command: &str) {
    let p = read(s, w, |s, g| {
        s.center_of(g, Hit::Bar(BarButton::Palette)).unwrap()
    });
    click(w, cx, p, Modifiers::default(), 1);
    {
        let mut s = s.borrow_mut();
        let p = s.palette.as_mut().expect("palette opened by button");
        p.query = command.into();
    }
    let p = read(s, w, |s, g| {
        let p = s.palette.as_ref().unwrap();
        let hits = p.hits();
        let i = hits
            .iter()
            .position(|i| p.rows[*i].name == command)
            .expect("command in palette");
        mid(p
            .layout(g.size)
            .rows
            .iter()
            .find(|(n, _)| *n == i)
            .unwrap()
            .1)
    });
    click(w, cx, p, Modifiers::default(), 1);
}
pub(crate) fn roll_point(
    s: &Rc<RefCell<Studio>>,
    w: &Window,
    tick: i64,
    pitch: u8,
    f: f32,
) -> (f32, f32) {
    read(s, w, |s, g| {
        let c = s.roll_ctx(g).unwrap();
        let v = s.roll.view(&c);
        (v.x(tick), v.y(pitch) + v.row_h * f)
    })
}
fn grid(s: &Rc<RefCell<Studio>>, w: &mut Window, cx: &mut App) {
    let p = read(s, w, |s, g| {
        s.center_of(g, Hit::Bar(BarButton::Grid)).unwrap()
    });
    click(w, cx, p, Modifiers::default(), 1);
    cell(s, w, cx, "クオンタイズの設定…");
    assert_eq!(s.borrow().popup.as_ref().unwrap().kind, PopupKind::Quantize);
}
fn select_all(s: &Rc<RefCell<Studio>>, w: &Window) {
    let mut s = s.borrow_mut();
    let g = crate::screen::geometry(&s, w);
    let c = s.roll_ctx(&g).unwrap();
    s.roll.selected = c.notes().iter().map(|n| n.id).collect();
}
fn setup_roll(s: &mut Studio, g: &Geometry) {
    setup_roll_in(s, g, "extended");
}
/// The roll on three notes of Keys, the arrow in `arrow` ("basic" or "extended").
pub(crate) fn setup_roll_in(s: &mut Studio, g: &Geometry, arrow: &str) {
    s.select_clip_of("Keys");
    s.tab = DrawerTab::Roll;
    s.drawer_open = true;
    // A shut drawer slides open (UI-011): the roll is set up in the drawer it opens to.
    let g = &s.settled_geometry(g.size.0, g.size.1);
    s.roll.row_h = 24.0;
    s.roll.beat_px = 72.0;
    // The first shots press the notes' velocity and split edges, which are the extended arrow's
    // (the arrow starts basic since 2026-10-21); e06 then presses the arrow back to basic.
    s.rec_set(
        "prefs.set",
        json!({"key": "edit.arrow.mode", "value": arrow}),
    );
    s.apply_edit_prefs();
    let c = s.roll_ctx(g).unwrap();
    s.song
        .run("note.delete", json!({"ids":[c.clip.id.to_string()]}))
        .unwrap();
    s.song.run("note.put",json!({"clip":c.clip.id.to_string(),"notes":[{"at":960,"pitch":60,"length":960,"velocity":70},{"at":1920,"pitch":60,"length":960,"velocity":90},{"at":4000,"pitch":64,"length":487,"velocity":100}]})).unwrap();
    s.resized(g);
    s.roll.center(&s.roll_ctx(g).unwrap());
}

fn play(n: u8, s: &Rc<RefCell<Studio>>, w: &mut Window, cx: &mut App) {
    let m = Modifiers::default();
    match n {
        0 => {
            let mut s = s.borrow_mut();
            let g = crate::screen::geometry(&s, w);
            setup_roll(&mut s, &g);
        }
        1 => {
            select_all(s, w);
            let p = roll_point(s, w, 1440, 60, 0.04);
            drag(w, cx, p, (p.0, p.1 - 10.0), MouseButton::Left, m);
            assert_eq!(
                read(s, w, |s, g| s.roll_ctx(g).unwrap().notes()[0].velocity),
                81
            );
        }
        2 => {
            let p = roll_point(s, w, 1920, 60, 0.96);
            click(w, cx, p, m, 1);
            assert_eq!(read(s, w, |s, g| s.roll_ctx(g).unwrap().notes().len()), 2);
        }
        3 => {
            let p = roll_point(s, w, 1440, 60, 0.5);
            click(w, cx, p, m, 2);
            assert_eq!(read(s, w, |s, g| s.roll_ctx(g).unwrap().notes().len()), 1);
        }
        4 => {
            let p = roll_point(s, w, 3360, 62, 0.5);
            click(w, cx, p, m, 2);
            assert_eq!(read(s, w, |s, g| s.roll_ctx(g).unwrap().notes().len()), 2);
        }
        5 => {
            let p = read(s, w, |s, g| {
                s.center_of(g, Hit::Bar(BarButton::Tool(Tool::Select)))
                    .unwrap()
            });
            click(w, cx, p, m, 1);
            assert!(!s.borrow().roll.extended);
        }
        6 => {
            let p = read(s, w, |s, g| {
                let c = s.roll_ctx(g).unwrap();
                let v = s.roll.view(&c);
                (s.roll.frame(c.body).keys.x + 12.0, v.y(62) + v.row_h / 2.0)
            });
            click(w, cx, p, Modifiers { shift: true, ..m }, 1);
            assert_eq!(s.borrow().roll.selected.len(), 1);
        }
        7 => {
            let p = read(s, w, |s, g| {
                let c = s.roll_ctx(g).unwrap();
                let v = s.roll.view(&c);
                (s.roll.frame(c.body).keys.x + 12.0, v.y(62) + v.row_h / 2.0)
            });
            down(w, cx, p, MouseButton::Left, m, 1);
            moved(w, cx, (p.0, p.1 - 24.0), MouseButton::Left, m);
            assert_eq!(s.borrow().roll.audition, Some(63));
        }
        8 => {
            let p = s.borrow().roll.pointer.unwrap();
            up(w, cx, p, MouseButton::Left, m, 1);
            palette(s, w, cx, "view.scale");
            cell(s, w, cx, "C メジャー");
            assert!(s.borrow().roll.in_scale(60));
        }
        9 => {
            select_all(s, w);
            palette(s, w, cx, "note.velocity.shape");
            cell(s, w, cx, "クレッシェンド");
        }
        10 => {
            palette(s, w, cx, "note.resize");
            cell(s, w, cx, "2 倍");
        }
        11 => {
            grid(s, w, cx);
            cell(s, w, cx, "長さも");
            cell(s, w, cx, "選択をクオンタイズ");
        }
        12 => cell(s, w, cx, "タイミングを復元"),
        13 => cell(s, w, cx, "ヒューマナイズ"),
        14 => cell(s, w, cx, "クオンタイズを固定"),
        15 => {
            s.borrow_mut().popup = None;
            let p = read(s, w, |s, g| {
                mid(s.roll.frame(s.roll_ctx(g).unwrap().body).lanes[0].fold)
            });
            click(w, cx, p, m, 1);
            assert!(s.borrow().roll.lanes[0].folded);
        }
        16 => {
            s.borrow_mut().roll.beat_px = 16.0;
            let (a, b, old) = read(s, w, |s, g| {
                let c = s.roll_ctx(g).unwrap();
                let v = s.roll.view(&c);
                let y = v.y(62) + v.row_h / 2.0;
                (
                    (v.x(c.clip.length + 960), y),
                    (v.x(c.clip.length + 2880), y),
                    c.clip.id,
                )
            });
            drag(w, cx, a, b, MouseButton::Left, m);
            assert_ne!(s.borrow().selected_clip.unwrap().1, old);
        }
        17 => {
            s.borrow_mut().drawer_open = false;
            let p = read(s, w, |s, g| {
                let t = crate::arrange::probe::track(s, "Keys");
                let c = &s.project().tracks[&t].clips[0];
                let r = s.clip_rect(
                    g,
                    s.tree().row_of(crate::tree::RowKind::Track(t)).unwrap(),
                    c,
                );
                (s.tick_x(g, c.start + c.length / 2), r.bottom() - 2.0)
            });
            click(w, cx, p, m, 1);
            assert_eq!(
                read(s, w, |s, _| s.project().tracks
                    [&crate::arrange::probe::track(s, "Keys")]
                    .clips
                    .len()),
                2
            );
        }
        18 => {
            let p = read(s, w, |s, g| {
                let t = crate::arrange::probe::track(s, "Keys");
                let c = s.project().tracks[&t]
                    .clips
                    .iter()
                    .min_by_key(|c| c.start)
                    .unwrap()
                    .clone();
                let r = s.clip_rect(
                    g,
                    s.tree().row_of(crate::tree::RowKind::Track(t)).unwrap(),
                    &c,
                );
                (r.right() - 1.0, r.bottom() - 2.0)
            });
            click(w, cx, p, m, 1);
            assert_eq!(
                read(s, w, |s, _| s.project().tracks
                    [&crate::arrange::probe::track(s, "Keys")]
                    .clips
                    .len()),
                1
            );
        }
        19 => {
            palette(s, w, cx, "view.link");
            let (a, b) = read(s, w, |s, g| {
                let t = crate::arrange::probe::track(s, "Keys");
                let c = s.project().tracks[&t].clips[0].clone();
                let r = s.clip_rect(
                    g,
                    s.tree().row_of(crate::tree::RowKind::Track(t)).unwrap(),
                    &c,
                );
                (
                    (r.x + r.w * 0.2, r.y + r.h / 4.0),
                    (r.x + r.w * 0.6, r.y + r.h / 4.0),
                )
            });
            drag(w, cx, a, b, MouseButton::Left, m);
            assert!(s.borrow().arrange.range.is_some());
        }
        20 => {
            s.borrow_mut().tool = Tool::Range;
            let p = read(s, w, |s, g| {
                let range = s.arrange.range.unwrap();
                let t = crate::arrange::probe::track(s, "Keys");
                (
                    s.tick_x(g, (range.0 + range.1) / 2),
                    crate::arrange::probe::clip_mid(s, g, "Keys", 0)
                        .1
                        .max(s.tree().row_of(crate::tree::RowKind::Track(t)).unwrap().y),
                )
            });
            click(w, cx, p, m, 2);
            assert_eq!(
                read(s, w, |s, _| s.project().tracks
                    [&crate::arrange::probe::track(s, "Keys")]
                    .clips
                    .len()),
                3
            );
        }
        21 => {
            s.borrow_mut().tool = Tool::Select;
            let p = read(s, w, |s, g| {
                crate::arrange::probe::clip_mid(s, g, "Keys", 0)
            });
            click(w, cx, p, Modifiers { shift: true, ..m }, 2);
            assert_eq!(s.borrow().arrange.clips.len(), 3);
        }
        22 => {
            s.borrow_mut().tool = Tool::Draw;
            let (a, b) = read(s, w, |s, g| {
                let t = crate::arrange::probe::track(s, "Keys");
                let r = s.tree().row_of(crate::tree::RowKind::Track(t)).unwrap();
                let y = s.row_y(g, r) + r.h / 2.0;
                ((s.tick_x(g, 3840), y), (s.tick_x(g, 5760), y))
            });
            drag(w, cx, a, b, MouseButton::Left, m);
        }
        23 => {
            s.borrow_mut().tool = Tool::Mute;
            let (a, b) = read(s, w, |s, g| {
                (
                    crate::arrange::probe::clip_mid(s, g, "Keys", 0),
                    crate::arrange::probe::clip_mid(s, g, "Keys", 1),
                )
            });
            drag(w, cx, a, b, MouseButton::Left, m);
            assert!(
                read(s, w, |s, _| s.project().tracks
                    [&crate::arrange::probe::track(s, "Keys")]
                    .clips
                    .iter()
                    .filter(|c| c.muted)
                    .count())
                    >= 2
            );
        }
        24 => {
            let p = read(s, w, |_, g| (g.lanes.x + 500.0, g.lanes.y + 30.0));
            drag(w, cx, p, (p.0 - 120.0, p.1), MouseButton::Middle, m);
            assert!(s.borrow().scroll.0 > 0.0);
        }
        25 => {
            s.borrow_mut().tool = Tool::Eraser;
            let (a, b) = read(s, w, |s, g| {
                (
                    crate::arrange::probe::clip_mid(s, g, "Keys", 0),
                    crate::arrange::probe::clip_mid(s, g, "Keys", 1),
                )
            });
            drag(w, cx, a, b, MouseButton::Left, m);
        }
        26 => {
            {
                let mut s = s.borrow_mut();
                let g = crate::screen::geometry(&s, w);
                setup_roll(&mut s, &g);
                s.roll.row_h = 10.0;
                let ctx = s.roll_ctx(&g).unwrap();
                s.roll.center(&ctx);
            }
            let p = roll_point(s, w, 1440, 60, 0.04);
            down(w, cx, p, MouseButton::Left, m, 1);
            assert_eq!(
                s.borrow().roll.drag.as_ref().unwrap().mode,
                crate::roll::Mode::Move
            );
            up(w, cx, p, MouseButton::Left, m, 1);
        }
        27 => {
            let dir = std::env::current_dir()
                .unwrap()
                .join("target/edit3-mousemap");
            std::fs::create_dir_all(&dir).unwrap();
            let mut map = xirius_prefs::Mousemap::default();
            map.set("roll.select.hold", "Alt", "tool:erase").unwrap();
            std::fs::write(dir.join("mousemap.toml"), map.to_toml()).unwrap();
            s.borrow_mut().song.load_settings(&dir);
            let p = roll_point(s, w, 1440, 60, 0.5);
            click(w, cx, p, Modifiers { alt: true, ..m }, 1);
            assert_eq!(read(s, w, |s, g| s.roll_ctx(g).unwrap().notes().len()), 2);
        }
        28 => {
            s.borrow_mut().drawer_open = false;
            let top = std::env::current_dir()
                .unwrap()
                .join("target/edit3-replace-place");
            std::fs::create_dir_all(&top).unwrap();
            let path = top.join("replacement.wav");
            let mut wav = b"RIFF".to_vec();
            wav.extend(38u32.to_le_bytes());
            wav.extend(b"WAVEfmt ");
            wav.extend(16u32.to_le_bytes());
            wav.extend(1u16.to_le_bytes());
            wav.extend(1u16.to_le_bytes());
            wav.extend(48000u32.to_le_bytes());
            wav.extend(96000u32.to_le_bytes());
            wav.extend(2u16.to_le_bytes());
            wav.extend(16u16.to_le_bytes());
            wav.extend(b"data");
            wav.extend(2u32.to_le_bytes());
            wav.extend([0, 0]);
            std::fs::write(&path, wav).unwrap();
            {
                let mut s = s.borrow_mut();
                s.browser.press_rail(crate::browser::RailTab::Files);
                s.browser.open = true;
                s.browser.add_place(&top).unwrap();
            }
            let (from, p, clip) = read(s, w, |s, g| {
                let i = crate::import_shots::file_row(s, g, "replacement.wav").unwrap();
                let from = s
                    .browser
                    .center_of(
                        &s.browser.layout(g.panel),
                        crate::browser::BrowserHit::File(i),
                    )
                    .unwrap();
                let t = crate::arrange::probe::track(s, "Vocal");
                (
                    from,
                    crate::arrange::probe::clip_mid(s, g, "Vocal", 0),
                    s.project().tracks[&t].clips[0].id,
                )
            });
            down(w, cx, from, MouseButton::Left, m, 1);
            assert!(s.borrow().browser.file_drag.is_some());
            moved(w, cx, p, MouseButton::Left, Modifiers { alt: true, ..m });
            up(w, cx, p, MouseButton::Left, Modifiers { alt: true, ..m }, 1);
            assert!(read(
                s,
                w,
                |s, _| matches!(&s.project().clip(clip).unwrap().1.body,xirius_core::model::ClipBody::Audio{file,..} if std::path::Path::new(file)==path)
            ));
        }
        29 => {
            s.borrow_mut().tool = Tool::Select;
            s.borrow()
                .song
                .settings()
                .lock()
                .unwrap()
                .mousemap
                .set("arrange.select.hold", "Mod", "tool:draw")
                .unwrap();
            let (a, b) = read(s, w, |s, g| {
                let t = crate::arrange::probe::track(s, "Keys");
                let row = s.tree().row_of(crate::tree::RowKind::Track(t)).unwrap();
                let y = s.row_y(g, row) + row.h / 2.0;
                ((s.tick_x(g, 3840), y), (s.tick_x(g, 5760), y))
            });
            drag(
                w,
                cx,
                a,
                b,
                MouseButton::Left,
                Modifiers { control: true, ..m },
            );
            assert_eq!(s.borrow().tool, Tool::Select);
        }
        30 => {
            {
                let mut s = s.borrow_mut();
                let g = crate::screen::geometry(&s, w);
                setup_roll(&mut s, &g);
            }
            let p = read(s, w, |s, g| {
                s.center_of(g, Hit::Bar(BarButton::Grid)).unwrap()
            });
            click(w, cx, p, m, 1);
            cell(s, w, cx, "1/8");
            cell(s, w, cx, "オフ");
            assert_eq!(s.borrow().song.grid_for("arrange"), "1/16");
            assert!(s.borrow().song.snap_for("arrange"));
        }
        31 => {
            s.borrow_mut().popup = None;
            let p = read(s, w, |s, g| {
                crate::arrange::probe::clip_mid(s, g, "Keys", 0)
            });
            click(w, cx, p, m, 1);
            let p = read(s, w, |s, g| {
                s.center_of(g, Hit::Bar(BarButton::Grid)).unwrap()
            });
            click(w, cx, p, m, 1);
            cell(s, w, cx, "1/4");
            assert_eq!(s.borrow().song.grid_for("roll"), "1/8");
            assert!(!s.borrow().song.snap_for("roll"));
        }
        32 => {
            {
                let mut s = s.borrow_mut();
                s.popup = None;
                s.arrange.focus = false;
                s.choose_editor();
            }
            select_all(s, w);
            grid(s, w, cx);
            let path = std::env::current_dir()
                .unwrap()
                .join("target/edit3-groove.mid");
            let mut bytes = b"MThd".to_vec();
            bytes.extend([0, 0, 0, 6, 0, 0, 0, 1, 0, 96]);
            bytes.extend(b"MTrk");
            bytes.extend([
                0, 0, 0, 12, 0, 0x90, 60, 80, 96, 0x80, 60, 0, 0, 0xff, 0x2f, 0,
            ]);
            std::fs::write(&path, bytes).unwrap();
            {
                let mut s = s.borrow_mut();
                let g = crate::screen::geometry(&s, w);
                let r = s.popup.as_ref().unwrap().layout(g.size).rect;
                s.drop_files(&g, r.x + 20.0, r.y + 20.0, &[path]);
            }
            cell(s, w, cx, "選択をクオンタイズ");
            assert!(
                s.borrow()
                    .song
                    .session()
                    .state
                    .view
                    .roll
                    .external_groove
                    .is_some()
            );
        }
        33 => {
            s.borrow_mut().drawer_open = false;
            {
                let mut s = s.borrow_mut();
                let t = crate::arrange::probe::track(&s, "Keys");
                let c = s.project().tracks[&t].clips[0].clone();
                s.song.run("note.add",json!({"clip":c.id.to_string(),"at":c.start+c.length/2-120,"pitch":85,"length":240})).unwrap();
                s.tool = Tool::Cut;
            }
            let p = read(s, w, |s, g| {
                crate::arrange::probe::clip_mid(s, g, "Keys", 0)
            });
            click(w, cx, p, Modifiers { alt: true, ..m }, 1);
            assert_eq!(
                read(s, w, |s, _| s.project().tracks
                    [&crate::arrange::probe::track(s, "Keys")]
                    .clips
                    .len()),
                2
            );
        }
        34 => {
            {
                let mut s = s.borrow_mut();
                let g = crate::screen::geometry(&s, w);
                setup_roll(&mut s, &g);
                s.tool = Tool::Select;
                // The hand points into the roll once the drawer has come to rest.
                s.motion.enabled.set(false);
                crate::screen::geometry(&s, w);
                s.motion.enabled.set(true);
            }
            let p = roll_point(s, w, 1440, 60, 0.5);
            moved(w, cx, p, MouseButton::Left, m);
            w.dispatch_keystroke(gpui::Keystroke::parse("4").unwrap(), cx);
            assert_eq!(s.borrow().tool, Tool::Eraser);
        }
        _ => unreachable!(),
    }
    assert!(
        s.borrow().error.is_none(),
        "stage {n}: {:?}",
        s.borrow().error
    );
}
