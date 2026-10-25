//! `--script ctx`: the right-click menus, pressed as real pointer events on the window. The
//! right button goes down on the spot, the menu has to open there, and a row is chosen by a
//! left press on its cell. The song is the sample song; nothing takes the PC's focus.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use xirius_design::Layout;

use crate::arrange::probe::{row_at, track};
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Studio};
use crate::tree::RowKind;

/// Stage `n` of this script, played by [`play`].
const fn staged(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const CTX: &[Shot] = &[
    // S01: an empty stretch of the Keys lane, pressed with the right button.
    Shot {
        name: "c01-lane-menu",
        acts: &[Act::Reset, staged(0), staged(1)],
    },
    // "MIDI クリップを作成" chosen: a one-bar clip stands where the press was.
    Shot {
        name: "c02-lane-clip-made",
        acts: &[staged(2)],
    },
    // "選択" chosen: the rows below take the menu's place.
    Shot {
        name: "c03-lane-select-rows",
        acts: &[staged(3), staged(4)],
    },
    // S03: a MIDI clip of Keys, pressed with the right button.
    Shot {
        name: "c04-midi-clip-menu",
        acts: &[Act::Reset, staged(0), staged(10)],
    },
    // "色" chosen: the squares of the palette, and the track's colour first.
    Shot {
        name: "c05-clip-colors",
        acts: &[staged(11)],
    },
    // A colour chosen: the clip carries it.
    Shot {
        name: "c06-clip-colored",
        acts: &[staged(12)],
    },
    // S02: an audio clip of Vocal.
    Shot {
        name: "c07-audio-clip-menu",
        acts: &[Act::Reset, staged(0), staged(13)],
    },
    // S04: a MIDI and an audio clip selected, the right button on one of them.
    Shot {
        name: "c08-clips-menu",
        acts: &[Act::Reset, staged(0), staged(14)],
    },
    // S06: the head of the Vocal track.
    Shot {
        name: "c09-track-menu",
        acts: &[Act::Reset, staged(0), staged(20)],
    },
    // "オートメーションを表示" chosen: the lanes the track can show.
    Shot {
        name: "c10-track-lanes",
        acts: &[staged(21)],
    },
    // "トラック名を変更" chosen: the menu is a box with the name in it.
    Shot {
        name: "c11-track-rename",
        acts: &[Act::Reset, staged(0), staged(20), staged(22), staged(23)],
    },
    // Enter: the track has the new name.
    Shot {
        name: "c12-track-renamed",
        acts: &[staged(24)],
    },
    // S07: Vocal and Guitar picked, the right button on one of them.
    Shot {
        name: "c13-tracks-menu",
        acts: &[Act::Reset, staged(0), staged(25)],
    },
    // S08: the Rhythm folder.
    Shot {
        name: "c14-folder-menu",
        acts: &[Act::Reset, staged(0), staged(26)],
    },
    // "色" chosen: the squares, and the row that paints the tracks inside too.
    Shot {
        name: "c15-folder-colors",
        acts: &[staged(27)],
    },
    // S09: under the last row of the column.
    Shot {
        name: "c16-column-menu",
        acts: &[Act::Reset, staged(0), staged(28)],
    },
    // "フォルダー" chosen.
    Shot {
        name: "c17-column-folders",
        acts: &[staged(29)],
    },
    // S10: the bar numbers of the ruler.
    Shot {
        name: "c18-ruler-menu",
        acts: &[Act::Reset, staged(0), staged(30)],
    },
    // "テンポを設定…" chosen: the menu is a box with the tempo in it.
    Shot {
        name: "c19-ruler-tempo",
        acts: &[staged(31)],
    },
    // S11: the loop band, with a loop on bars 5 to 8.
    Shot {
        name: "c20-loop-menu",
        acts: &[Act::Reset, staged(0), staged(32), staged(33)],
    },
    // S15: the second tempo point, slowing to 90 at bar 9.
    Shot {
        name: "c21-tempo-point-menu",
        acts: &[Act::Reset, staged(0), staged(40), staged(41)],
    },
    // "次のポイントまでの形" chosen.
    Shot {
        name: "c22-tempo-point-shape",
        acts: &[staged(42)],
    },
    // S16: the tempo row, off the points.
    Shot {
        name: "c23-tempo-row-menu",
        acts: &[Act::Reset, staged(0), staged(40), staged(43)],
    },
    // S17: the middle volume point of Bass.
    Shot {
        name: "c24-auto-point-menu",
        acts: &[Act::Reset, staged(0), staged(50), staged(51)],
    },
    // "次のポイントまでの形" chosen.
    Shot {
        name: "c25-auto-point-shape",
        acts: &[staged(52)],
    },
    // "値を入力…" chosen: the menu is the box with the value in it.
    Shot {
        name: "c26-auto-point-value",
        acts: &[Act::Reset, staged(0), staged(50), staged(51), staged(56)],
    },
    // S18: on the line between two points.
    Shot {
        name: "c27-auto-line-menu",
        acts: &[Act::Reset, staged(0), staged(50), staged(53)],
    },
    // S18: right of the last point, where there is no line.
    Shot {
        name: "c28-auto-lane-menu",
        acts: &[Act::Reset, staged(0), staged(50), staged(55)],
    },
    // S19: the head of the volume lane.
    Shot {
        name: "c29-auto-head-menu",
        acts: &[Act::Reset, staged(0), staged(50), staged(54)],
    },
    // "レーンを追加" chosen: the lanes the track can still show.
    Shot {
        name: "c30-auto-head-add",
        acts: &[staged(57)],
    },
    // S20: a note of the piano roll.
    Shot {
        name: "c31-note-menu",
        acts: &[Act::Reset, staged(60), staged(61)],
    },
    // "移調" chosen.
    Shot {
        name: "c32-note-transpose",
        acts: &[staged(62)],
    },
    // "ベロシティ" chosen.
    Shot {
        name: "c33-note-velocity",
        acts: &[Act::Reset, staged(60), staged(61), staged(63)],
    },
    // "長さ" chosen.
    Shot {
        name: "c34-note-length",
        acts: &[Act::Reset, staged(60), staged(61), staged(64)],
    },
    // S21: the empty part of the grid.
    Shot {
        name: "c35-roll-empty-menu",
        acts: &[Act::Reset, staged(60), staged(65)],
    },
    // "グリッド" chosen.
    Shot {
        name: "c36-roll-grid-rows",
        acts: &[staged(66)],
    },
    // S22: a key of the piano.
    Shot {
        name: "c37-keys-menu",
        acts: &[Act::Reset, staged(60), staged(67), staged(68)],
    },
    // S23: a velocity bar.
    Shot {
        name: "c38-velocity-bar-menu",
        acts: &[Act::Reset, staged(60), staged(67), staged(69)],
    },
    // "揃える" chosen.
    Shot {
        name: "c39-velocity-bar-shape",
        acts: &[staged(70)],
    },
    // S24: a point of a controller lane.
    Shot {
        name: "c40-point-menu",
        acts: &[Act::Reset, staged(60), staged(67), staged(71)],
    },
    // S25: the empty part of a controller lane.
    Shot {
        name: "c41-lane-empty-menu",
        acts: &[Act::Reset, staged(60), staged(67), staged(72)],
    },
    // S26: the tab of a controller lane.
    Shot {
        name: "c42-tab-menu",
        acts: &[Act::Reset, staged(60), staged(67), staged(73)],
    },
    // "種類を変更" chosen.
    Shot {
        name: "c43-tab-kinds",
        acts: &[staged(74)],
    },
    // S27: the name of the Vocal channel in the mixer.
    Shot {
        name: "c44-mixer-head-menu",
        acts: &[Act::Reset, staged(80), staged(81)],
    },
    // "オートメーションを表示" chosen: the lanes the channel can show.
    Shot {
        name: "c45-mixer-head-lanes",
        acts: &[staged(82)],
    },
    // S28: the fader of Vocal.
    Shot {
        name: "c46-mixer-fader-menu",
        acts: &[Act::Reset, staged(80), staged(83)],
    },
    // S29: the pan knob of Vocal.
    Shot {
        name: "c47-mixer-pan-menu",
        acts: &[Act::Reset, staged(80), staged(84)],
    },
    // S31: an effect of Vocal.
    Shot {
        name: "c48-mixer-plugin-menu",
        acts: &[Act::Reset, staged(80), staged(88)],
    },
    // "プリセット" chosen.
    Shot {
        name: "c49-mixer-plugin-presets",
        acts: &[staged(89)],
    },
    // "別のチャンネルへ移動" chosen.
    Shot {
        name: "c50-mixer-plugin-move",
        acts: &[Act::Reset, staged(80), staged(88), staged(95)],
    },
    // S32: the ＋ row of the inserts, with an effect copied.
    Shot {
        name: "c51-mixer-insert-menu",
        acts: &[Act::Reset, staged(80), staged(90)],
    },
    // S33: the level of a send, and its name.
    Shot {
        name: "c52-mixer-send-level-menu",
        acts: &[Act::Reset, staged(80), staged(85)],
    },
    Shot {
        name: "c53-mixer-send-menu",
        acts: &[Act::Reset, staged(80), staged(86)],
    },
    // S34: the ＋ row of the sends.
    Shot {
        name: "c54-mixer-add-send-menu",
        acts: &[Act::Reset, staged(80), staged(87)],
    },
    // S35: the head of a bus.
    Shot {
        name: "c55-mixer-bus-menu",
        acts: &[Act::Reset, staged(80), staged(92)],
    },
    // S36: the head of the master.
    Shot {
        name: "c56-mixer-master-menu",
        acts: &[Act::Reset, staged(80), staged(93)],
    },
    // S37: the room right of the last strip.
    Shot {
        name: "c57-mixer-room-menu",
        acts: &[Act::Reset, staged(80), staged(94)],
    },
    // S38: an instrument of the browser, with the Keys track selected.
    Shot {
        name: "c58-browser-instrument-menu",
        acts: &[Act::Reset, staged(0), staged(100), staged(101)],
    },
    // "新しいトラックで使う" chosen: a track stands with it.
    Shot {
        name: "c59-browser-instrument-new-track",
        acts: &[staged(102)],
    },
    // S39: an effect, with the Vocal track selected.
    Shot {
        name: "c60-browser-effect-menu",
        acts: &[Act::Reset, staged(0), staged(103), staged(104)],
    },
    // "新しいバスに追加" chosen.
    Shot {
        name: "c61-browser-effect-new-bus",
        acts: &[staged(105)],
    },
    // S40: a group's header of the list, then "すべて畳む" chosen.
    Shot {
        name: "c62-browser-list-menu",
        acts: &[Act::Reset, staged(0), staged(106)],
    },
    Shot {
        name: "c63-browser-list-folded",
        acts: &[staged(107)],
    },
    // S41: a sound file of a place, then "新しいトラックに置く" chosen.
    Shot {
        name: "c64-browser-file-menu",
        acts: &[Act::Reset, staged(0), staged(110), staged(111)],
    },
    Shot {
        name: "c65-browser-file-placed",
        acts: &[staged(112)],
    },
    // S42: a folder of a place.
    Shot {
        name: "c66-browser-folder-menu",
        acts: &[Act::Reset, staged(0), staged(110), staged(113)],
    },
    // S43: the tab of a place, then "場所名を変更" chosen.
    Shot {
        name: "c67-browser-tab-menu",
        acts: &[Act::Reset, staged(0), staged(110), staged(114)],
    },
    Shot {
        name: "c68-browser-tab-rename",
        acts: &[staged(115)],
    },
    // S44: a finished export in the export panel, its song unchanged since.
    Shot {
        name: "c69-export-card-menu",
        acts: &[Act::Reset, staged(0), staged(120), staged(121)],
    },
    // The song changed since: "書き直す" can be chosen.
    Shot {
        name: "c70-export-card-menu-stale",
        acts: &[staged(122), staged(123)],
    },
    // "ごみ箱へ" chosen: the one question.
    Shot {
        name: "c71-export-trash-ask",
        acts: &[staged(124)],
    },
    // "プラグインを複製" chosen: the effect is twice in the chain.
    Shot {
        name: "c72-mixer-plugin-duplicated",
        acts: &[Act::Reset, staged(80), staged(88), staged(96)],
    },
    // "別のチャンネルへ写す" then Keys chosen: the effect is on Keys too.
    Shot {
        name: "c73-mixer-plugin-copied-to",
        acts: &[Act::Reset, staged(80), staged(88), staged(98), staged(97)],
    },
];

fn at(p: (f32, f32)) -> gpui::Point<gpui::Pixels> {
    point(px(p.0), px(p.1 + Layout::TOP_BAR_H))
}

fn press(p: (f32, f32), button: MouseButton, window: &mut Window, cx: &mut App) {
    let p = at(p);
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
            button,
            position: p,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button,
            position: p,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}

/// The middle of the row `label` of the menu that is open, in the body's own coordinates.
fn cell(studio: &Rc<RefCell<Studio>>, window: &Window, label: &str) -> Option<(f32, f32)> {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    let layout = s.popup.as_ref()?.layout(g.size);
    let c = layout.cells.iter().find(|c| c.text == label)?;
    Some((c.rect.x + c.rect.w / 2.0, c.rect.y + c.rect.h / 2.0))
}

/// A spot of the empty lane of `name` at bar `bar`.
fn lane(s: &Studio, g: &Geometry, name: &str, bar: i64) -> (f32, f32) {
    let t = track(s, name);
    (
        s.tick_x(g, (bar - 1) * 3840) + 4.0,
        row_at(s, g, RowKind::Track(t), 0.5),
    )
}

fn choose(studio: &Rc<RefCell<Studio>>, label: &str, window: &mut Window, cx: &mut App) {
    match cell(studio, window, label) {
        Some(p) => press(p, MouseButton::Left, window, cx),
        None => eprintln!("no row {label} in the open menu"),
    }
}

fn play(stage: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    if stage == 0 {
        let mut s = studio.borrow_mut();
        let g = crate::screen::geometry(&s, window);
        s.drawer_open = false;
        s.snap_mode = crate::tools::SnapMode::Bar;
        s.resized(&g);
        return;
    }
    if stage >= 120 {
        return export(stage, studio, window, cx);
    }
    if stage >= 100 {
        return browse(stage, studio, window, cx);
    }
    if stage >= 80 {
        return mixer(stage, studio, window, cx);
    }
    if stage >= 60 {
        return roll(stage, studio, window, cx);
    }
    if stage >= 50 {
        return auto(stage, studio, window, cx);
    }
    if stage >= 40 {
        return tempo(stage, studio, window, cx);
    }
    if stage >= 30 {
        return ruler(stage, studio, window, cx);
    }
    if stage >= 20 {
        return column(stage, studio, window, cx);
    }
    if stage >= 10 {
        return clips(stage, studio, window, cx);
    }
    // The clip made at bar 13 stays: the second menu opens two bars further on.
    let spot = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        lane(&s, &g, "Keys", if stage < 3 { 13 } else { 15 })
    };
    match stage {
        1 => press(spot, MouseButton::Right, window, cx),
        2 => choose(studio, "MIDI クリップを作成", window, cx),
        3 => press(spot, MouseButton::Right, window, cx),
        4 => choose(studio, "選択", window, cx),
        _ => {}
    }
}

/// The middle of the `i`-th clip of the track `name`, in the body's own coordinates.
fn clip_spot(studio: &Rc<RefCell<Studio>>, window: &Window, name: &str) -> (f32, f32) {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    crate::arrange::probe::clip_mid(&s, &g, name, 0)
}

/// The stages of the clip menus.
fn clips(stage: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    match stage {
        10 => press(
            clip_spot(studio, window, "Keys"),
            MouseButton::Right,
            window,
            cx,
        ),
        11 => choose(studio, "色", window, cx),
        12 => {
            // The fourth square of the palette.
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let layout = s.popup.as_ref().map(|p| p.layout(g.size));
                layout.and_then(|l| {
                    l.cells
                        .iter()
                        .filter(|c| matches!(c.look, crate::popup::Look::Swatch(_)))
                        .nth(3)
                        .map(|c| (c.rect.x + c.rect.w / 2.0, c.rect.y + c.rect.h / 2.0))
                })
            };
            match p {
                Some(p) => press(p, MouseButton::Left, window, cx),
                None => eprintln!("no colour square in the open menu"),
            }
        }
        13 => press(
            clip_spot(studio, window, "Vocal"),
            MouseButton::Right,
            window,
            cx,
        ),
        14 => {
            {
                let mut s = studio.borrow_mut();
                let vt = track(&s, "Vocal");
                let vc = s.project().tracks[&vt].clips[0].id;
                let kt = track(&s, "Keys");
                let kc = s.project().tracks[&kt].clips[0].id;
                s.arrange.clips = [(kt, kc), (vt, vc)].into();
                s.selected_clip = Some((kt, kc));
                s.arrange.touched();
            }
            press(
                clip_spot(studio, window, "Keys"),
                MouseButton::Right,
                window,
                cx,
            );
        }
        _ => {}
    }
}

/// The middle of a row of the track column, in the body's own coordinates.
fn row_spot(studio: &Rc<RefCell<Studio>>, window: &Window, kind: RowKind) -> (f32, f32) {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    crate::arrange::probe::row_mid(&s, &g, kind)
}

/// The stages of the track column's menus.
fn column(stage: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let track_row = |name: &str| RowKind::Track(track(&studio.borrow(), name));
    let right = |p, window: &mut Window, cx: &mut App| press(p, MouseButton::Right, window, cx);
    match stage {
        20 => right(row_spot(studio, window, track_row("Vocal")), window, cx),
        21 => choose(studio, "オートメーションを表示", window, cx),
        22 => choose(studio, "トラック名を変更", window, cx),
        23 => {
            let mut s = studio.borrow_mut();
            for _ in 0..16 {
                s.ask_key("backspace");
            }
            s.ask_text(None, "Lead Vocal", false);
        }
        24 => {
            studio.borrow_mut().ask_key("enter");
        }
        25 => {
            studio.borrow_mut().arrange.tracks = vec![track_row("Vocal"), track_row("Guitar")];
            right(row_spot(studio, window, track_row("Guitar")), window, cx);
        }
        26 => {
            let f = crate::arrange::probe::folder(&studio.borrow(), "Rhythm");
            right(row_spot(studio, window, RowKind::Folder(f)), window, cx);
        }
        27 => choose(studio, "色", window, cx),
        28 | 29 => {
            if stage == 29 {
                return choose(studio, "フォルダー", window, cx);
            }
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let row = *s.tree().rows.last().expect("rows");
                (g.column.x + 40.0, s.row_y(&g, &row) + row.h + 12.0)
            };
            right(p, window, cx);
        }
        _ => {}
    }
}

/// The stages of the ruler's menus.
fn ruler(stage: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let at_bar = |bar: i64, band: bool| {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        let x = s.tick_x(&g, (bar - 1) * 3840) + 2.0;
        (
            x,
            if band {
                g.ruler.y + 4.0
            } else {
                g.ruler.y + g.ruler.h - 4.0
            },
        )
    };
    match stage {
        30 => press(at_bar(9, false), MouseButton::Right, window, cx),
        31 => choose(studio, "テンポを設定…", window, cx),
        32 => {
            let mut s = studio.borrow_mut();
            let end = 8 * 3840;
            let _ = s.song.run(
                "loop.set",
                serde_json::json!({ "start": 4 * 3840, "end": end }),
            );
            s.after_edit();
        }
        33 => press(at_bar(6, true), MouseButton::Right, window, cx),
        _ => {}
    }
}

/// The stages of the tempo row's menus.
fn tempo(stage: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let spot = |bar: Option<i64>| {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        match bar {
            Some(bar) => (s.tick_x(&g, (bar - 1) * 3840) + 2.0, g.tempo.y + 3.0),
            None => {
                let t = s.project().tempo[1].clone();
                s.tempo_xy(&g, t.tick, t.bpm)
            }
        }
    };
    match stage {
        40 => {
            let mut s = studio.borrow_mut();
            let _ = s.song.run(
                "tempo.set",
                serde_json::json!({ "at": 8 * 3840, "bpm": 90 }),
            );
            s.after_edit();
        }
        41 => press(spot(None), MouseButton::Right, window, cx),
        42 => choose(studio, "次のポイントまでの形", window, cx),
        43 => press(spot(Some(14)), MouseButton::Right, window, cx),
        _ => {}
    }
}

/// Finds a spot of the window from the studio and its geometry.
type Finder<'a> = dyn Fn(&Studio, &Geometry) -> Option<(f32, f32)> + 'a;

/// The stages of the mixer's menus. Vocal has a compressor and an EQ, and sends to the bus Fx.
fn mixer(stage: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    use crate::mixer::{List, MixerHit};
    let right = |p, window: &mut Window, cx: &mut App| press(p, MouseButton::Right, window, cx);
    let on_screen = |f: &Finder<'_>| {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        f(&s, &g)
    };
    let vocal = || track(&studio.borrow(), "Vocal");
    let effect = |n: usize| {
        let t = vocal();
        studio.borrow().project().tracks[&t].inserts[n].id
    };
    let send = || {
        let t = vocal();
        studio.borrow().project().tracks[&t].sends[0].id
    };
    let name_of = |t: xirius_core::id::TrackId| {
        move |s: &Studio, g: &Geometry| {
            s.mixer_strips(g)
                .into_iter()
                .find(|x| x.track == t)
                .map(|x| (x.name.x + x.name.w / 2.0, x.name.y + x.name.h / 2.0))
        }
    };
    let _ = List::Inserts;
    match stage {
        80 => {
            let mut s = studio.borrow_mut();
            let v = track(&s, "Vocal");
            for n in ["Compressor", "EQ"] {
                let _ = s.song.run(
                    "plugin.add",
                    serde_json::json!({ "track": v.to_string(), "name": n, "kind": "effect",
                                        "format": "builtin", "uid": n }),
                );
            }
            let _ = s.song.run(
                "track.add.bus",
                serde_json::json!({ "name": "Fx", "into": "root" }),
            );
            let bus = track(&s, "Fx");
            let _ = s.song.run(
                "send.add",
                serde_json::json!({ "ids": [v.to_string()], "to": bus.to_string() }),
            );
            s.drawer_open = true;
            s.tab = crate::studio::DrawerTab::Mixer;
            s.after_edit();
            let g = crate::screen::geometry(&s, window);
            s.resized(&g);
        }
        81 => {
            if let Some(p) = on_screen(&name_of(vocal())) {
                right(p, window, cx);
            }
        }
        82 => choose(studio, "オートメーションを表示", window, cx),
        83 => {
            let t = vocal();
            if let Some(p) = on_screen(&|s, g| s.mixer_center(g, MixerHit::Fader(t))) {
                right(p, window, cx);
            }
        }
        84 => {
            let t = vocal();
            if let Some(p) = on_screen(&|s, g| s.mixer_center(g, MixerHit::Pan(t))) {
                right(p, window, cx);
            }
        }
        85 => {
            let (t, sid) = (vocal(), send());
            if let Some(p) = on_screen(&|s, g| s.mixer_center(g, MixerHit::SendLevel(t, sid))) {
                right(p, window, cx);
            }
        }
        86 => {
            let (t, sid) = (vocal(), send());
            if let Some(p) = on_screen(&|s, g| s.mixer_center(g, MixerHit::SendPre(t, sid))) {
                right(p, window, cx);
            }
        }
        87 => {
            let t = vocal();
            if let Some(p) = on_screen(&|s, g| s.mixer_center(g, MixerHit::AddSend(t))) {
                right(p, window, cx);
            }
        }
        88 => {
            let (t, id) = (vocal(), effect(0));
            if let Some(p) = on_screen(&|s, g| s.mixer_center(g, MixerHit::Open(t, id))) {
                right(p, window, cx);
            }
        }
        89 => choose(studio, "プリセット", window, cx),
        90 => {
            // The compressor is copied, then the ＋ row of Keys is pressed.
            let (t, id) = (vocal(), effect(0));
            {
                let mut s = studio.borrow_mut();
                let _ = s
                    .song
                    .run("edit.copy", serde_json::json!({ "plugin": id.to_string() }));
                let _ = t;
            }
            let keys = track(&studio.borrow(), "Keys");
            if let Some(p) = on_screen(&|s, g| s.mixer_center(g, MixerHit::AddInsert(keys))) {
                right(p, window, cx);
            }
        }
        92 => {
            let bus = track(&studio.borrow(), "Fx");
            if let Some(p) = on_screen(&name_of(bus)) {
                right(p, window, cx);
            }
        }
        93 => {
            if let Some(p) = on_screen(&name_of(xirius_core::id::TrackId::MASTER)) {
                right(p, window, cx);
            }
        }
        94 => {
            let p = on_screen(&|s, g| {
                let last = s.mixer_strips(g).into_iter().last()?.rect;
                Some((last.x + last.w + 60.0, last.y + 80.0))
            });
            if let Some(p) = p {
                right(p, window, cx);
            }
        }
        95 => choose(studio, "別のチャンネルへ移動", window, cx),
        // The compressor is duplicated, and an effect is copied onto Keys from the menu.
        96 => choose(studio, "プラグインを複製", window, cx),
        97 => choose(studio, "Keys", window, cx),
        98 => choose(studio, "別のチャンネルへ写す", window, cx),
        _ => {}
    }
}

/// The stages of the automation lanes' menus.
fn auto(stage: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let bass = || track(&studio.borrow(), "Bass");
    let on_screen = |f: &dyn Fn(&Studio, &Geometry) -> (f32, f32)| {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        f(&s, &g)
    };
    let right = |p, window: &mut Window, cx: &mut App| press(p, MouseButton::Right, window, cx);
    match stage {
        50 => {
            let mut s = studio.borrow_mut();
            let g = crate::screen::geometry(&s, window);
            crate::arrange::probe::two_lanes(&mut s, &g, "Bass");
        }
        51 => {
            let t = bass();
            right(
                on_screen(&|s, g| crate::arrange::probe::point_xy(s, g, t, 0, 1)),
                window,
                cx,
            );
        }
        52 => choose(studio, "次のポイントまでの形", window, cx),
        53 => {
            let t = bass();
            right(
                on_screen(&|s, g| {
                    let (xa, ya) = crate::arrange::probe::point_xy(s, g, t, 0, 0);
                    let (xb, yb) = crate::arrange::probe::point_xy(s, g, t, 0, 1);
                    ((xa + xb) / 2.0, (ya + yb) / 2.0)
                }),
                window,
                cx,
            );
        }
        54 => {
            let t = bass();
            right(
                on_screen(&|s, g| {
                    let row = *s.tree().row_of(RowKind::Track(t)).expect("row");
                    (g.column.x + 20.0, s.lane_top(g, &row, 0) + 24.0)
                }),
                window,
                cx,
            );
        }
        55 => {
            let t = bass();
            right(
                on_screen(&|s, g| {
                    let (x, y) = crate::arrange::probe::point_xy(s, g, t, 0, 2);
                    (x + 160.0, y)
                }),
                window,
                cx,
            );
        }
        56 => choose(studio, "値を入力…", window, cx),
        57 => choose(studio, "レーンを追加", window, cx),
        _ => {}
    }
}

/// A spot of the piano roll's grid in the body's own coordinates: the middle of the first
/// note, or a pitch row nothing sits on.
fn roll_spot(studio: &Rc<RefCell<Studio>>, window: &Window, note: bool) -> (f32, f32) {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    let c = s.roll_ctx(&g).expect("the roll is open");
    let v = s.roll.view(&c);
    let first = c.notes().iter().min_by_key(|n| n.start).expect("a note");
    if note {
        return (
            (v.x(first.start) + v.x(first.start + first.length)) / 2.0,
            v.y(first.pitch) + v.row_h / 2.0,
        );
    }
    // The nearest pitch row that shows in the grid and has no note.
    let free = (1..12i16)
        .flat_map(|d| [d, -d])
        .map(|d| (i16::from(first.pitch) + d).clamp(0, 127) as u8)
        .find(|&p| {
            let y = v.y(p) + v.row_h / 2.0;
            y > v.grid.y && y < v.grid.bottom() && c.notes().iter().all(|n| n.pitch != p)
        })
        .expect("a free row");
    (v.x(first.start) + 40.0, v.y(free) + v.row_h / 2.0)
}

/// Gives the Keys clip a CC1 lane of three points, as the lane menus need one.
fn with_cc1(studio: &Rc<RefCell<Studio>>, window: &Window) {
    let mut s = studio.borrow_mut();
    let g = crate::screen::geometry(&s, window);
    let c = s.roll_ctx(&g).expect("the roll is open");
    s.song
        .run(
            "cc.replace",
            serde_json::json!({ "clip": c.clip.id.to_string(), "lane": "cc1", "from": 0, "to": 1920,
                "points": [{ "tick": 0, "value": 10 }, { "tick": 480, "value": 90 },
                           { "tick": 960, "value": 40 }] }),
        )
        .expect("the lane is written");
    // The bars differ, so the shaping rows are live.
    let first = c.notes().iter().min_by_key(|n| n.start).expect("a note");
    s.song
        .run(
            "note.velocity.set",
            serde_json::json!({ "ids": [first.id.to_string()], "value": 40 }),
        )
        .expect("the velocity is set");
    s.resized(&g);
}

/// What a right press at a lane spot of the roll lands on.
enum Spot {
    Key,
    Bar,
    Point,
    Empty,
    Tab,
}

/// A spot of the keys, a velocity bar or the CC1 lane, in the body's own coordinates.
fn lane_spot(studio: &Rc<RefCell<Studio>>, window: &Window, what: Spot) -> (f32, f32) {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    let c = s.roll_ctx(&g).expect("the roll is open");
    let f = s.roll.frame(c.body);
    let v = s.roll.view(&c);
    let first = c.notes().iter().min_by_key(|n| n.start).expect("a note");
    let cc = s
        .roll
        .lanes
        .iter()
        .position(|l| l.kind == crate::roll::LaneKind::Ctrl(xirius_core::model::CtrlKind::Cc(1)))
        .expect("the CC1 lane");
    let plot = f.lanes[cc].plot;
    let kind = s.roll.lanes[cc].kind;
    match what {
        Spot::Key => (f.keys.x + f.keys.w / 2.0, v.y(first.pitch) + v.row_h / 2.0),
        Spot::Bar => (
            v.x(first.start),
            f.lanes[0].plot.y + f.lanes[0].plot.h / 2.0,
        ),
        Spot::Point => (v.x(480), crate::roll::y_of(kind, &plot, 90)),
        Spot::Empty => (v.x(1440), plot.y + plot.h / 2.0),
        Spot::Tab => {
            let r = f.tabs[cc];
            (r.x + r.w / 2.0, r.y + r.h / 2.0)
        }
    }
}

/// The stages of the piano roll's menus (the roll of the sample song stays open).
fn roll(stage: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    match stage {
        60 => {
            let mut s = studio.borrow_mut();
            let g = crate::screen::geometry(&s, window);
            s.resized(&g);
        }
        61 => press(
            roll_spot(studio, window, true),
            MouseButton::Right,
            window,
            cx,
        ),
        62 => choose(studio, "トランスポーズ", window, cx),
        63 => choose(studio, "ベロシティ", window, cx),
        64 => choose(studio, "長さ", window, cx),
        65 => press(
            roll_spot(studio, window, false),
            MouseButton::Right,
            window,
            cx,
        ),
        66 => choose(studio, "グリッド", window, cx),
        67 => with_cc1(studio, window),
        68 => press(
            lane_spot(studio, window, Spot::Key),
            MouseButton::Right,
            window,
            cx,
        ),
        69 => {
            // Every bar picked, as the shaping rows need two or more.
            {
                let mut s = studio.borrow_mut();
                let g = crate::screen::geometry(&s, window);
                let c = s.roll_ctx(&g).expect("the roll is open");
                s.roll.selected = c.notes().iter().map(|n| n.id).collect();
            }
            press(
                lane_spot(studio, window, Spot::Bar),
                MouseButton::Right,
                window,
                cx,
            );
        }
        70 => choose(studio, "揃える", window, cx),
        71 => press(
            lane_spot(studio, window, Spot::Point),
            MouseButton::Right,
            window,
            cx,
        ),
        72 => press(
            lane_spot(studio, window, Spot::Empty),
            MouseButton::Right,
            window,
            cx,
        ),
        73 => press(
            lane_spot(studio, window, Spot::Tab),
            MouseButton::Right,
            window,
            cx,
        ),
        74 => choose(studio, "種類を変更", window, cx),
        _ => {}
    }
}

/// A place of sound files for the browser's file menus.
fn place() -> std::path::PathBuf {
    std::env::temp_dir().join("xirius-ctx-shots").join("素材")
}

/// The stages of the browser's menus (100s: instruments and effects, 110s: files).
fn browse(stage: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    use crate::browser::{BRow, BrowserHit, RailTab};
    type Make = fn(usize) -> BrowserHit;
    let right = |p, window: &mut Window, cx: &mut App| press(p, MouseButton::Right, window, cx);
    // The middle of the hit that `find` picks among the browser's rows.
    let spot = |window: &Window, find: &dyn Fn(&BRow) -> Option<Make>| {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        let l = s.browser.layout(g.panel);
        let (i, hit) = l
            .rows
            .iter()
            .enumerate()
            .find_map(|(i, p)| find(&p.row).map(|h| (i, h)))?;
        s.browser.center_of(&l, hit(i))
    };
    let item = |name: &'static str| {
        move |r: &BRow| match r {
            BRow::Item { item, .. } if item.name == name => Some(BrowserHit::Item as Make),
            _ => None,
        }
    };
    let select = |name: &str| {
        let mut s = studio.borrow_mut();
        let t = track(&s, name);
        s.selected_row = Some(RowKind::Track(t));
    };
    match stage {
        100 => select("Keys"),
        101 => {
            if let Some(p) = spot(window, &item("Sampler")) {
                right(p, window, cx);
            }
        }
        102 => choose(studio, "新しいトラックで使う", window, cx),
        103 => {
            select("Vocal");
            let mut s = studio.borrow_mut();
            s.browser.press_rail(RailTab::Effects);
            let g = crate::screen::geometry(&s, window);
            s.resized(&g);
        }
        104 => {
            if let Some(p) = spot(window, &item("EQ")) {
                right(p, window, cx);
            }
        }
        105 => choose(studio, "新しいバスに追加", window, cx),
        106 => {
            let p = spot(window, &|r| match r {
                BRow::Group { .. } => Some(BrowserHit::Group as fn(usize) -> BrowserHit),
                _ => None,
            });
            if let Some(p) = p {
                right(p, window, cx);
            }
        }
        107 => choose(studio, "すべて畳む", window, cx),
        110 => {
            let top = place();
            crate::import_shots::make_place(&top);
            let mut s = studio.borrow_mut();
            s.browser.press_rail(RailTab::Files);
            if let Err(e) = s.browser.add_place_fixture(&top) {
                s.error = Some(e);
            }
            s.browser.toggle_dir(&top.join("Loops").to_string_lossy());
            let g = crate::screen::geometry(&s, window);
            s.resized(&g);
        }
        111 => {
            let p = spot(window, &|r| match r {
                BRow::File { name, .. } if name == "loop_120bpm.wav" => {
                    Some(BrowserHit::File as fn(usize) -> BrowserHit)
                }
                _ => None,
            });
            if let Some(p) = p {
                right(p, window, cx);
            }
        }
        112 => choose(studio, "新しいトラックに置く", window, cx),
        113 => {
            let p = spot(window, &|r| match r {
                BRow::Dir { name, .. } if name == "Drums" => {
                    Some(BrowserHit::Dir as fn(usize) -> BrowserHit)
                }
                _ => None,
            });
            if let Some(p) = p {
                right(p, window, cx);
            }
        }
        114 => {
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let l = s.browser.layout(g.panel);
                s.browser.center_of(&l, BrowserHit::Loc(1))
            };
            if let Some(p) = p {
                right(p, window, cx);
            }
        }
        115 => choose(studio, "場所名を変更", window, cx),
        _ => {}
    }
}

/// S44: the menu of a card of the export panel. The song is saved to a scratch folder first,
/// so it can export; the files stay there (the trash row is only asked, never answered).
fn export(stage: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    use crate::studio::Rect;
    let anchor = |s: &Studio, window: &Window| {
        let g = crate::screen::geometry(s, window);
        Rect::new(g.size.0 - 60.0, -40.0, 28.0, 28.0)
    };
    match stage {
        120 => {
            let mut s = studio.borrow_mut();
            let dir = std::env::temp_dir().join("xirius-ctx-export-shots");
            let _ = std::fs::remove_dir_all(&dir);
            s.command(|song| song.save_as(&dir));
            let a = anchor(&s, window);
            s.open_export(a);
            s.export_action(crate::export::Act::Go);
            crate::shot::run_for(&mut s, 30.0, |s| !s.export.running());
            s.refresh_popup();
        }
        121 | 123 => {
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                s.popup.as_ref().and_then(|p| {
                    p.layout(g.size)
                        .cells
                        .into_iter()
                        .find_map(|c| match c.look {
                            crate::popup::Look::Xp(crate::export::XpLook::Card(_)) => {
                                Some((c.rect.x + 60.0, c.rect.y + c.rect.h / 2.0))
                            }
                            _ => None,
                        })
                })
            };
            match p {
                Some(p) => press(p, MouseButton::Right, window, cx),
                None => eprintln!("no card in the export panel"),
            }
        }
        122 => {
            let mut s = studio.borrow_mut();
            let t = track(&s, "Vocal");
            s.command(|song| song.toggle_mute(t));
            s.popup = None;
            s.ctx = None;
            let a = anchor(&s, window);
            s.open_export(a);
        }
        124 => choose(studio, "ごみ箱へ", window, cx),
        _ => {}
    }
}
