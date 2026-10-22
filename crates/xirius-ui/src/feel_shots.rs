//! `--script feel`: the feel of editing (the Studio Pro form of the tools, scan and pick of
//! plugins, tempo and meter in the top bar, the roll's lines and zoom, scissors and mute
//! before the click, a clip's transpose, the mixer's master, a track's colour).
//! Every stage goes in through the window's own input path (no OS input).

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use xirius_design::Layout;

use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Hit, Studio};
use crate::tools::Tool;
use crate::topbar::BarButton;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    // 01: the arrow's alternative tool, underlined in the bar; lit while Ctrl is held.
    Shot {
        name: "f01a-arrange-alt-tool",
        acts: &[stage(0)],
    },
    Shot {
        name: "f01b-arrange-ctrl-held",
        acts: &[stage(1)],
    },
    Shot {
        name: "f01c-roll-ctrl-eraser",
        acts: &[stage(2)],
    },
    Shot {
        name: "f01d-roll-ctrl-shift-split",
        acts: &[stage(3)],
    },
    Shot {
        name: "f01e-alt-tool-cut",
        acts: &[stage(4)],
    },
    // 02: the scan row, the plugins that could not be used, the spinner of a plugin on its way
    // in, and the same scan rows in the instrument list.
    Shot {
        name: "f02a-browser-scanning",
        acts: &[Act::Reset, laid(0)],
    },
    Shot {
        name: "f02b-browser-unusable",
        acts: &[Act::Reset, laid(1)],
    },
    Shot {
        name: "f02c-browser-retrying",
        acts: &[Act::Reset, laid(2)],
    },
    Shot {
        name: "f02d-browser-loading",
        acts: &[Act::Reset, laid(3)],
    },
    Shot {
        name: "f02e-instrument-list-scanning",
        acts: &[Act::Reset, laid(4)],
    },
    // 03: the tempo and the meter of the bar.
    Shot {
        name: "f03a-bar-tempo-hover",
        acts: &[Act::Reset, stage(5)],
    },
    Shot {
        name: "f03b-bar-tempo-drag",
        acts: &[stage(6)],
    },
    Shot {
        name: "f03c-bar-tempo-typing",
        acts: &[stage(7)],
    },
    Shot {
        name: "f03d-bar-tempo-typed",
        acts: &[stage(8)],
    },
    Shot {
        name: "f03e-bar-meter-popup",
        acts: &[stage(9)],
    },
    Shot {
        name: "f03f-bar-meter-fields",
        acts: &[stage(10)],
    },
    // 05: the roll.
    Shot {
        name: "f05a-roll-selected",
        acts: &[Act::Reset, stage(11)],
    },
    Shot {
        name: "f05b-roll-grid-mid",
        acts: &[Act::Reset, stage(13)],
    },
    Shot {
        name: "f05b-roll-grid-near",
        acts: &[Act::Reset, stage(14)],
    },
    Shot {
        name: "f05b-roll-grid-wide",
        acts: &[Act::Reset, stage(12)],
    },
    Shot {
        name: "f05c-roll-end-handle",
        acts: &[Act::Reset, stage(15)],
    },
    Shot {
        name: "f05c-roll-grown",
        acts: &[Act::Reset, stage(17)],
    },
    Shot {
        name: "f05c-roll-veil",
        acts: &[Act::Reset, stage(16)],
    },
    // 05d: the zoom slider under the grid, and the ruler's numbers at three widths.
    Shot {
        name: "f05d-roll-zoom-bars",
        acts: &[Act::Reset, stage(18)],
    },
    Shot {
        name: "f05d-roll-zoom-beats",
        acts: &[Act::Reset, stage(19)],
    },
    Shot {
        name: "f05d-roll-zoom-sixteenths",
        acts: &[Act::Reset, stage(20)],
    },
    // 06: the cut line and the mute look before the press, in the arrangement and in the roll.
    Shot {
        name: "f06a-arrange-cut-line",
        acts: &[Act::Reset, stage(21)],
    },
    Shot {
        name: "f06b-arrange-mute-hover",
        acts: &[Act::Reset, stage(22)],
    },
    Shot {
        name: "f06c-arrange-mute-sweep",
        acts: &[Act::Reset, stage(23)],
    },
    Shot {
        name: "f06d-arrange-unmute-hover",
        acts: &[Act::Reset, stage(24)],
    },
    Shot {
        name: "f06e-roll-cut-line",
        acts: &[Act::Reset, stage(25)],
    },
    Shot {
        name: "f06f-roll-mute-hover",
        acts: &[Act::Reset, stage(26)],
    },
    Shot {
        name: "f06g-roll-unmute-hover",
        acts: &[Act::Reset, stage(27)],
    },
    // 07: the transpose field at the head of a MIDI clip's menu: empty, typed, applied.
    Shot {
        name: "f07a-clip-menu-transpose",
        acts: &[Act::Reset, stage(28)],
    },
    Shot {
        name: "f07b-clip-menu-typed",
        acts: &[Act::Reset, stage(29)],
    },
    Shot {
        name: "f07c-clip-transposed",
        acts: &[Act::Reset, stage(30)],
    },
    // 08: the mixer with wide channels: the master at the right edge, the bar under the strips.
    Shot {
        name: "f08a-mixer-master-pinned",
        acts: &[Act::Reset, stage(31)],
    },
    Shot {
        name: "f08b-mixer-bar-drag",
        acts: &[Act::Reset, stage(32)],
    },
    Shot {
        name: "f08c-mixer-scrolled-to-end",
        acts: &[Act::Reset, stage(33)],
    },
    // 09: the colour square at the left of a track's head; the choice it opens; the menu's
    // 「色」 row with the same square, and its squares with the check.
    Shot {
        name: "f09a-head-square",
        acts: &[Act::Reset, stage(34)],
    },
    Shot {
        name: "f09b-color-choice",
        acts: &[Act::Reset, stage(35)],
    },
    Shot {
        name: "f09c-menu-color-row",
        acts: &[Act::Reset, stage(36)],
    },
    Shot {
        name: "f09d-menu-color-squares",
        acts: &[Act::Reset, stage(37)],
    },
];

const fn laid(n: u8) -> Act {
    Act::Stage(Play::Laid(scan_stage), n)
}

/// Stages 0..=4 of the scan shots, on the screen's state.
fn scan_stage(s: &mut Studio, g: &Geometry, n: u8) {
    use crate::browser::{BadPlugin, RailTab};
    use crate::mixer::MixerHit;
    s.browser.open = true;
    s.browser.tab = RailTab::Effects;
    let bad = |name: &str, why: &'static str| BadPlugin {
        path: format!("C:/Program Files/Common Files/VST3/{name}.vst3").into(),
        name: name.to_string(),
        why,
    };
    let unusable = || {
        vec![
            bad("Broken Delay", "読み込めませんでした"),
            bad("Old Synth 32", "スキャン中に終了しました"),
            bad("Unstable Reverb", "応答がありません"),
        ]
    };
    match n {
        0 => s.pose_scan(12, 48),
        1 | 2 => {
            s.browser.bad = unusable();
            if n == 2 {
                s.pose_scan(1, 1);
                s.browser.retrying = Some(s.browser.bad[1].path.clone());
            }
            // The end of the list is where the group is.
            let l = s.browser.layout(g.panel);
            s.browser.scroll = (l.content_h - l.list.h).max(0.0);
        }
        3 => {
            // The first scanned effect is the one on its way in.
            let name = s
                .browser
                .library()
                .into_iter()
                .find(|i| i.format != "builtin")
                .map_or("Compressor", |i| i.name);
            s.browser.loading = Some(crate::loading::Loading {
                name: name.to_string(),
                id: xirius_core::id::PluginId(1),
                since: xirius_core::store::now_ms(),
            });
            s.browser.spin = 0.12;
            s.browser.selected_item = Some(name);
        }
        _ => {
            s.drawer_open = true;
            s.tab = crate::studio::DrawerTab::Mixer;
            s.pose_scan(12, 48);
            let g = s.geometry(g.size.0, g.size.1);
            let t = s
                .project()
                .tracks
                .values()
                .find(|t| t.name == "Keys")
                .map(|t| t.id)
                .expect("an instrument track without an instrument");
            assert!(s.press(&g, Hit::Mixer(MixerHit::AddInstrument(t))));
        }
    }
}

pub(crate) fn ctrl() -> Modifiers {
    Modifiers {
        control: true,
        ..Modifiers::default()
    }
}

pub(crate) fn click(
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

/// Moves the pointer to (x, y) of the content (below the top bar), and presses there when a
/// button is given.
pub(crate) fn pointer(
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

fn at(p: (f32, f32)) -> gpui::Point<gpui::Pixels> {
    point(px(p.0), px(p.1 + Layout::TOP_BAR_H))
}

/// Moves to `p` (content coordinates) and presses the left button there, `clicks` deep.
pub(crate) fn button_down(p: (f32, f32), clicks: usize, window: &mut Window, cx: &mut App) {
    let modifiers = Modifiers::default();
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at(p),
            pressed_button: None,
            modifiers,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            position: at(p),
            button: MouseButton::Left,
            modifiers,
            click_count: clicks,
            first_mouse: false,
        }),
        cx,
    );
}

/// Moves to `p` with the left button held.
pub(crate) fn drag_to(p: (f32, f32), window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at(p),
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::default(),
        }),
        cx,
    );
}

pub(crate) fn button_up(p: (f32, f32), clicks: usize, window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position: at(p),
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: clicks,
        }),
        cx,
    );
}

/// The centre of a button of the bar, in content coordinates.
fn bar_at(studio: &Rc<RefCell<Studio>>, window: &Window, b: BarButton) -> (f32, f32) {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    s.center_of(&g, Hit::Bar(b))
        .expect("the button is on the bar")
}

fn keys(text: &str, window: &mut Window, cx: &mut App) {
    for key in text.split(' ') {
        window.dispatch_keystroke(gpui::Keystroke::parse(key).unwrap(), cx);
    }
}

/// Stages 5..=10: the tempo and the meter.
fn bar_play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let tempo = bar_at(studio, window, BarButton::Tempo);
    match n {
        5 => pointer(tempo, Modifiers::default(), None, window, cx),
        6 => {
            button_down(tempo, 1, window, cx);
            drag_to((tempo.0, tempo.1 - 14.0), window, cx);
            assert_eq!(studio.borrow().bar_view().bpm, "127.0");
        }
        7 => {
            // The drag ends where it stopped; then the readout is pressed twice.
            button_up((tempo.0, tempo.1 - 14.0), 1, window, cx);
            assert_eq!(studio.borrow().bar_view().bpm, "127.0");
            button_down(tempo, 1, window, cx);
            button_up(tempo, 1, window, cx);
            button_down(tempo, 2, window, cx);
            button_up(tempo, 2, window, cx);
            assert!(matches!(
                studio.borrow().bar_view().tempo,
                crate::topbar::TempoMode::Edit { .. }
            ));
        }
        8 => {
            keys("9 6", window, cx);
            assert_eq!(studio.borrow().bar_view().bpm, "96");
        }
        9 => {
            keys("enter", window, cx);
            assert_eq!(studio.borrow().bar_view().bpm, "96.0");
            let meter = bar_at(studio, window, BarButton::Meter);
            button_down(meter, 1, window, cx);
            button_up(meter, 1, window, cx);
            assert_eq!(
                studio.borrow().popup.as_ref().map(|p| p.kind),
                Some(crate::popup::PopupKind::Meter)
            );
        }
        _ => {
            keys("7 up", window, cx);
            let s = studio.borrow();
            let m = s.popup.as_ref().and_then(|p| p.meter.as_ref()).unwrap();
            assert_eq!((m.num.as_str(), m.den), ("7", 8));
        }
    }
}

/// The middles of the notes of the roll's clip that are on screen, in content coordinates.
fn notes_on_screen(studio: &Rc<RefCell<Studio>>, window: &Window) -> Vec<(f32, f32)> {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    let c = s.roll_ctx(&g).expect("the roll is open");
    let v = s.roll.view(&c);
    c.notes()
        .iter()
        .map(|n| s.roll.shown(n))
        .map(|n| {
            (
                v.x(n.start) + v.px(n.length) / 2.0,
                v.y(n.pitch) + v.row_h / 2.0,
            )
        })
        .filter(|(x, y)| v.grid.contains(*x, *y))
        .collect()
}

/// Stages 11..: the roll.
fn roll_play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    match n {
        11 => {
            let shift = Modifiers {
                shift: true,
                ..Modifiers::default()
            };
            {
                let mut s = studio.borrow_mut();
                let g = crate::screen::geometry(&s, window);
                let c = s.roll_ctx(&g).expect("the roll is open");
                s.roll.center(&c);
            }
            let on = notes_on_screen(studio, window);
            pointer(
                on[0],
                Modifiers::default(),
                Some(MouseButton::Left),
                window,
                cx,
            );
            for k in [2, 4] {
                pointer(on[k], shift, Some(MouseButton::Left), window, cx);
            }
            assert_eq!(studio.borrow().roll.selected.len(), 3);
            pointer((20.0, 20.0), Modifiers::default(), None, window, cx);
        }
        // The grid at three zooms: bar > beat > finer lines at 1 px each.
        12..=14 => {
            let mut s = studio.borrow_mut();
            s.roll.beat_px = [10.0, 36.0, 140.0][usize::from(n - 12)];
            let g = crate::screen::geometry(&s, window);
            let c = s.roll_ctx(&g).expect("the roll is open");
            s.roll.center(&c);
        }
        // The clip's end: the handle held and pulled in, the clip at rest, the pencil
        // drawing past the end.
        15..=17 => {
            {
                let mut s = studio.borrow_mut();
                let g = crate::screen::geometry(&s, window);
                let c = s.roll_ctx(&g).expect("the roll is open");
                // Fit the clip in the left two thirds of the grid: there is room past its end.
                let f = s.roll.frame(c.body);
                let beats = c.clip_len() as f32 / c.ppq() as f32;
                s.roll.beat_px = (f.grid.w * 0.62 / beats).floor();
                s.roll.center(&c);
                if n == 17 {
                    s.tool = crate::tools::Tool::Draw;
                }
            }
            let (end, head, row) = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let c = s.roll_ctx(&g).expect("the roll is open");
                let f = s.roll.frame(c.body);
                let v = s.roll.view(&c);
                (
                    (v.x(c.clip_len()), f.ruler.y + 20.0),
                    v.x(0),
                    f.grid.y + f.grid.h * 0.45,
                )
            };
            match n {
                15 => {
                    let to = (end.0 - studio.borrow().roll.beat_px * 4.0, end.1);
                    pointer(end, Modifiers::default(), None, window, cx);
                    button_down(end, 1, window, cx);
                    drag_to(to, window, cx);
                    assert!(studio.borrow().roll.preview.clip_end.is_some());
                }
                16 => {
                    pointer((head - 40.0, 20.0), Modifiers::default(), None, window, cx);
                }
                _ => {
                    let one_bar = studio.borrow().roll.beat_px * 4.0;
                    let p = (end.0 + one_bar * 0.6, row);
                    let len = {
                        let s = studio.borrow();
                        let g = crate::screen::geometry(&s, window);
                        s.roll_ctx(&g).expect("the roll is open").clip_len()
                    };
                    pointer(p, Modifiers::default(), Some(MouseButton::Left), window, cx);
                    let s = studio.borrow();
                    let g = crate::screen::geometry(&s, window);
                    let c = s.roll_ctx(&g).expect("the roll is open");
                    assert!(c.clip_len() > len, "the clip grew");
                }
            }
        }
        // The zoom slider pressed at three places: bars (40 px a beat), beats and 16ths.
        18..=20 => {
            let width = [40.0, 120.0, 620.0][usize::from(n - 18)];
            let on = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let c = s.roll_ctx(&g).expect("the roll is open");
                let t = s.roll.frame(c.body).zoom_track;
                let run = t.w - crate::roll::ZOOM_THUMB;
                (
                    t.x + crate::roll::ZOOM_THUMB / 2.0 + run * crate::roll::zoom_pos(width),
                    t.y + t.h / 2.0,
                )
            };
            pointer(
                on,
                Modifiers::default(),
                Some(MouseButton::Left),
                window,
                cx,
            );
            let beat_px = studio.borrow().roll.beat_px;
            assert!((beat_px - width).abs() / width < 0.05, "{beat_px}");
            pointer((20.0, 20.0), Modifiers::default(), None, window, cx);
        }
        21..=24 => arrange_tools_play(n, studio, window, cx),
        25..=27 => {
            let tool = if n == 25 { Tool::Cut } else { Tool::Mute };
            {
                let mut s = studio.borrow_mut();
                let g = crate::screen::geometry(&s, window);
                let c = s.roll_ctx(&g).expect("the roll is open");
                s.roll.center(&c);
            }
            pick_tool(tool, studio, window, cx);
            let on = notes_on_screen(studio, window);
            // A press on a note with the mute tool, then the pointer back on it, a little off
            // the middle (the unmute look).
            let (x, y) = on[1];
            if n == 27 {
                pointer(
                    (x, y),
                    Modifiers::default(),
                    Some(MouseButton::Left),
                    window,
                    cx,
                );
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let c = s.roll_ctx(&g).expect("the roll is open");
                assert!(
                    c.notes().iter().any(|n| n.muted),
                    "the press muted the note"
                );
            }
            pointer((x + 14.0, y), Modifiers::default(), None, window, cx);
            let s = studio.borrow();
            let g = crate::screen::geometry(&s, window);
            let c = s.roll_ctx(&g).expect("the roll is open");
            if n == 25 {
                assert!(s.roll.cut_line(&c).is_some(), "the cut line is up");
            } else {
                assert!(s.roll.mute_target(&c).is_some(), "the note is marked");
            }
        }
        28..=30 => transpose_play(n, studio, window, cx),
        31..=33 => master_play(n, studio, window, cx),
        34..=37 => color_play(n, studio, window, cx),
        _ => unreachable!("no stage {n}"),
    }
}

/// Stages 28..=30: a right press on the Keys clip, then the number typed into the menu's
/// transpose field, then Enter. The field is the clip's own value (UI-311): the notes do not
/// move. While typing, the field shows the digits as typed (「5」); Enter hands them over and
/// the menu stays with the field showing 「+5」, as an audio clip's fields do. 30 then closes
/// the menu (one Escape closes it, since nothing is half typed), so the clip shows
/// 「+5」 at the right of its name.
fn transpose_play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let keys_pitches = |studio: &Rc<RefCell<Studio>>| -> Vec<u8> {
        let s = studio.borrow();
        let t = crate::arrange::probe::track(&s, "Keys");
        let project = s.project();
        project.tracks[&t].clips[0]
            .notes()
            .iter()
            .map(|n| n.pitch)
            .collect()
    };
    let clip_semis = |studio: &Rc<RefCell<Studio>>| -> i8 {
        let s = studio.borrow();
        let t = crate::arrange::probe::track(&s, "Keys");
        s.project().tracks[&t].clips[0].transpose()
    };
    // The field of the open menu: the first value's field takes the keys as the menu opens
    // (UI-313), so the caret is already there.
    let said = |studio: &Rc<RefCell<Studio>>| -> Option<String> {
        studio
            .borrow()
            .context_rows()
            .into_iter()
            .find_map(|r| r.field.map(|f| f.text))
    };
    let before = keys_pitches(studio);
    let spot = on_clip(studio, window, "Keys", 0.5, 12.0);
    pointer(
        spot,
        Modifiers::default(),
        Some(MouseButton::Right),
        window,
        cx,
    );
    assert!(
        studio.borrow().context_open(),
        "the right press opens the menu"
    );
    assert_eq!(clip_semis(studio), 0);
    assert_eq!(said(studio).as_deref(), Some("0"));
    assert!(
        studio.borrow().field_typing().is_some(),
        "the field takes the keys when the menu opens"
    );
    if n >= 29 {
        keys("5", window, cx);
        assert_eq!(
            said(studio).as_deref(),
            Some("5"),
            "the digit went to the field as typed"
        );
    }
    if n == 30 {
        keys("enter", window, cx);
        assert!(studio.borrow().context_open(), "the menu stays");
        assert_eq!(said(studio).as_deref(), Some("+5"));
        assert!(
            studio.borrow().field_typing().is_some_and(|t| t.fresh),
            "the field keeps the keys"
        );
        assert_eq!(clip_semis(studio), 5);
        assert_eq!(keys_pitches(studio), before, "the notes stay as written");
        keys("escape", window, cx);
        assert!(!studio.borrow().context_open(), "the menu is closed");
        let s = studio.borrow();
        let t = crate::arrange::probe::track(&s, "Keys");
        let project = s.project();
        let track = &project.tracks[&t];
        assert!(
            crate::clip_marks::marks(track, &track.clips[0])
                .contains(&crate::clip_marks::Mark::Text("+5".into())),
            "the clip shows its transpose at the right of its name"
        );
    }
}

/// Stages 31..=33: the mixer with wide channels, which do not fit beside the master. 31 is
/// the start; 32 holds the bar's thumb part-way along; 33 has let go at the end.
fn master_play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    {
        let mut s = studio.borrow_mut();
        s.drawer_open = true;
        s.tab = crate::studio::DrawerTab::Mixer;
        s.song
            .run("view.channel.width", serde_json::json!({"width": "wide"}))
            .expect("wide channels");
        // The window is wide: busses make enough channels that they do not fit.
        for _ in 0..8 {
            s.command(|song| song.add_track("bus"));
        }
        assert!(s.error.is_none());
    }
    let bar = |studio: &Rc<RefCell<Studio>>, window: &Window| {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        s.mixer_bar(&g).expect("the channels do not fit")
    };
    let (groove, thumb) = bar(studio, window);
    pointer((0.0, 0.0), Modifiers::default(), None, window, cx);
    if n == 31 {
        return;
    }
    let grab = (thumb.x + thumb.w / 2.0, thumb.y + thumb.h / 2.0);
    button_down(grab, 1, window, cx);
    assert!(studio.borrow().mixer.bar_grab.is_some());
    if n == 32 {
        drag_to((grab.0 + (groove.w - thumb.w) * 0.4, grab.1), window, cx);
        assert!(studio.borrow().mixer.scroll > 0.0);
    } else {
        let to = (groove.right() + 40.0, grab.1);
        drag_to(to, window, cx);
        button_up(to, 1, window, cx);
        let max = {
            let s = studio.borrow();
            s.mixer_max_scroll(&crate::screen::geometry(&s, window))
        };
        assert_eq!(studio.borrow().mixer.scroll, max);
        assert!(studio.borrow().mixer.bar_grab.is_none());
    }
}

/// Stages 34..=37: the Keys track's colour square under the pointer (34); pressed, with the
/// choice open and one square under the pointer (35); the head's right-press menu (36) and its
/// 「色」 row pressed (37).
fn color_play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let keys = crate::arrange::probe::track(&studio.borrow(), "Keys");
    let centre = |hit: Hit, window: &Window| {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        s.center_of(&g, hit).expect("visible pointer target")
    };
    let swatch = centre(Hit::TrackColor(keys), window);
    if n == 34 {
        pointer(swatch, Modifiers::default(), None, window, cx);
        assert_eq!(studio.borrow().hover, Hit::TrackColor(keys));
        return;
    }
    if n == 35 {
        click(
            Hit::TrackColor(keys),
            Modifiers::default(),
            studio,
            window,
            cx,
        );
        assert!(
            studio.borrow().popup.is_some(),
            "the press opened the choice"
        );
        let over = {
            let s = studio.borrow();
            let g = crate::screen::geometry(&s, window);
            let layout = s.popup.as_ref().unwrap().layout(g.size);
            assert_eq!(layout.cells.len(), 16);
            let i = layout.cells.iter().position(|c| !c.on).unwrap() + 5;
            s.center_of(&g, Hit::Item(i)).expect("a square")
        };
        pointer(over, Modifiers::default(), None, window, cx);
        return;
    }
    // On the name, right of the square and the icon.
    let name = (swatch.0 + 70.0, swatch.1);
    pointer(
        name,
        Modifiers::default(),
        Some(MouseButton::Right),
        window,
        cx,
    );
    assert!(
        studio.borrow().context_open(),
        "the right press opens the menu"
    );
    let row = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        s.popup
            .as_ref()
            .unwrap()
            .layout(g.size)
            .cells
            .iter()
            .position(|c| c.text == "色")
            .expect("the colour row")
    };
    if n == 37 {
        click(Hit::Item(row), Modifiers::default(), studio, window, cx);
        assert!(studio.borrow().context_open());
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        let cells = s.popup.as_ref().unwrap().layout(g.size).cells;
        assert_eq!(cells.iter().filter(|c| c.on).count(), 1);
    } else {
        let over = centre(Hit::Item(row), window);
        pointer(over, Modifiers::default(), None, window, cx);
    }
}

/// Presses a tool's button in the top bar.
fn pick_tool(tool: Tool, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    click(
        Hit::Bar(BarButton::Tool(tool)),
        Modifiers::default(),
        studio,
        window,
        cx,
    );
    assert_eq!(studio.borrow().tool, tool);
}

/// Where on a track's clip a point lies: `across` of its width from the left, `down` px from
/// its top (content coordinates).
fn on_clip(
    studio: &Rc<RefCell<Studio>>,
    window: &Window,
    name: &str,
    across: f32,
    down: f32,
) -> (f32, f32) {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    let t = crate::arrange::probe::track(&s, name);
    let project = s.project();
    let c = &project.tracks[&t].clips[0];
    let row = *s.tree().row_of(crate::tree::RowKind::Track(t)).unwrap();
    let r = s.clip_rect(&g, &row, c);
    (r.x + r.w * across, r.y + down)
}

/// The clip under the arrangement's pointer.
fn hover_clip(s: &Studio) -> xirius_core::id::ClipId {
    match s.arrange.hover {
        crate::arrange::Spot::Clip { clip, .. } | crate::arrange::Spot::Fx { clip, .. } => clip,
        _ => panic!("the pointer is not over a clip"),
    }
}

/// Stages 21..=24: the arrangement's cut line and mute look.
fn arrange_tools_play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let plain = Modifiers::default();
    pick_tool(
        if n == 21 { Tool::Cut } else { Tool::Mute },
        studio,
        window,
        cx,
    );
    let spot = on_clip(studio, window, "Conga", 0.42, 20.0);
    match n {
        21 => {
            pointer(spot, plain, None, window, cx);
            let s = studio.borrow();
            let g = crate::screen::geometry(&s, window);
            assert!(s.cut_preview(&g).is_some(), "the cut line is up");
        }
        22 => {
            pointer(spot, plain, None, window, cx);
            let s = studio.borrow();
            let id = hover_clip(&s);
            assert!(s.clip_muted_shown(id, false));
        }
        23 => {
            let keys = on_clip(studio, window, "Keys", 0.5, 12.0);
            button_down(spot, 1, window, cx);
            drag_to((spot.0, (spot.1 + keys.1) / 2.0), window, cx);
            drag_to(keys, window, cx);
            assert!(
                studio.borrow().arrange.swept.len() >= 2,
                "the sweep passed two clips"
            );
        }
        _ => {
            // A press on the clip, and the pointer back on it: the way back.
            pointer(spot, plain, Some(MouseButton::Left), window, cx);
            pointer((spot.0 + 14.0, spot.1), plain, None, window, cx);
            let s = studio.borrow();
            let id = hover_clip(&s);
            assert!(!s.clip_muted_shown(id, true));
        }
    }
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    if n >= 11 {
        return roll_play(n, studio, window, cx);
    }
    if n >= 5 {
        return bar_play(n, studio, window, cx);
    }
    match n {
        0..=1 => {
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                (g.lanes.x + 220.0, g.lanes.y + 70.0)
            };
            if n == 0 {
                // A press in the arrange makes it the editor in use.
                pointer(p, Modifiers::default(), Some(MouseButton::Left), window, cx);
                assert_eq!(studio.borrow().tool_area(), crate::tools::Area::Arrange);
            }
            pointer(
                p,
                if n == 1 { ctrl() } else { Modifiers::default() },
                None,
                window,
                cx,
            );
            assert_eq!(studio.borrow().arrange.mods.ctrl, n == 1);
        }
        2..=3 => {
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                (g.drawer_body.x + 320.0, g.drawer_body.y + 120.0)
            };
            let mods = Modifiers {
                shift: n == 3,
                ..ctrl()
            };
            if n == 2 {
                // A press in the roll makes it the editor in use.
                pointer(p, Modifiers::default(), Some(MouseButton::Left), window, cx);
                assert_eq!(studio.borrow().tool_area(), crate::tools::Area::Roll);
            }
            pointer(p, mods, None, window, cx);
            assert!(studio.borrow().roll.mods.ctrl);
        }
        4 => {
            // The arrow pressed again, with the arrange in use: the next alternative tool (cut).
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                (g.lanes.x + 220.0, g.lanes.y + 70.0)
            };
            pointer(p, Modifiers::default(), Some(MouseButton::Left), window, cx);
            click(
                Hit::Bar(BarButton::Tool(Tool::Select)),
                Modifiers::default(),
                studio,
                window,
                cx,
            );
            assert_eq!(studio.borrow().alt_tool(), Tool::Cut);
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                (g.lanes.x + 220.0, g.lanes.y + 70.0)
            };
            pointer(p, ctrl(), None, window, cx);
        }
        _ => {}
    }
}
