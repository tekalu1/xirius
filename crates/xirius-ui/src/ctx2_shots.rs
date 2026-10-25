//! Menu actions through the window's pointer entry, with one-step undo assertions.
use crate::shot::{Act, Play, Shot};
use crate::studio::Studio;
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use serde_json::json;
use std::cell::RefCell;
use std::rc::Rc;
use xirius_core::model::Project;
use xirius_design::Layout;

thread_local! { static BEFORE: RefCell<Option<Project>> = const { RefCell::new(None) }; }
const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const CTX2: &[Shot] = &[
    Shot {
        name: "c201-ruler-marker",
        acts: &[Act::Reset, stage(0), stage(1)],
    },
    Shot {
        name: "c202-marker-added",
        acts: &[stage(2)],
    },
    Shot {
        name: "c203-marker-add-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c204-marker-menu",
        acts: &[stage(4)],
    },
    Shot {
        name: "c205-marker-loop",
        acts: &[stage(5)],
    },
    Shot {
        name: "c206-marker-loop-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c207-marker-deleted",
        acts: &[stage(6)],
    },
    Shot {
        name: "c208-marker-delete-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c209-marker-renamed",
        acts: &[stage(10)],
    },
    Shot {
        name: "c210-marker-rename-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c211-marker-colored",
        acts: &[stage(12)],
    },
    Shot {
        name: "c212-marker-color-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c213-take-menu",
        acts: &[Act::Reset, stage(0), stage(20)],
    },
    Shot {
        name: "c214-take-active",
        acts: &[stage(21)],
    },
    Shot {
        name: "c215-take-active-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c216-takes-cleaned",
        acts: &[stage(22)],
    },
    Shot {
        name: "c217-takes-clean-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c218-take-deleted",
        acts: &[stage(23)],
    },
    Shot {
        name: "c219-take-delete-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c220-track-copy",
        acts: &[Act::Reset, stage(0), stage(30)],
    },
    Shot {
        name: "c221-track-pasted",
        acts: &[stage(31)],
    },
    Shot {
        name: "c222-track-paste-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c223-tools-middle-click",
        acts: &[Act::Reset, stage(0), stage(40)],
    },
    Shot {
        name: "c224-eraser-selected",
        acts: &[stage(41)],
    },
    Shot {
        name: "c225-tempo-menu",
        acts: &[Act::Reset, stage(0), stage(50)],
    },
    Shot {
        name: "c226-tempo-delete",
        acts: &[stage(51)],
    },
    Shot {
        name: "c227-tempo-delete-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c228-note-values-menu",
        acts: &[Act::Reset, stage(60)],
    },
    Shot {
        name: "c229-note-velocity",
        acts: &[stage(61)],
    },
    Shot {
        name: "c230-note-velocity-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c231-send-menu",
        acts: &[Act::Reset, stage(70)],
    },
    Shot {
        name: "c232-send-muted",
        acts: &[stage(71)],
    },
    Shot {
        name: "c233-send-mute-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c234-send-retargeted",
        acts: &[stage(72)],
    },
    Shot {
        name: "c235-send-target-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c236-note-start",
        acts: &[Act::Reset, stage(60), stage(62)],
    },
    Shot {
        name: "c237-note-start-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c238-note-length",
        acts: &[stage(60), stage(63)],
    },
    Shot {
        name: "c239-note-length-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c240-note-pitch",
        acts: &[stage(60), stage(64)],
    },
    Shot {
        name: "c241-note-pitch-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c242-ruler-seconds",
        acts: &[Act::Reset, stage(0), stage(82)],
    },
    Shot {
        name: "c243-file-favorite",
        acts: &[Act::Reset, stage(80)],
    },
    Shot {
        name: "c244-folder-favorite",
        acts: &[stage(81)],
    },
    Shot {
        name: "c245-mixer-wide",
        acts: &[Act::Reset, stage(70), stage(83)],
    },
    Shot {
        name: "c246-clip-range-export",
        acts: &[Act::Reset, stage(0), stage(84)],
    },
    Shot {
        name: "c247-send-delete-key",
        acts: &[Act::Reset, stage(70), stage(73)],
    },
    Shot {
        name: "c248-send-delete-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c249-send-existing-target",
        acts: &[Act::Reset, stage(70), stage(74)],
    },
    Shot {
        name: "c250-send-existing-target-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c251-send-pre",
        acts: &[Act::Reset, stage(70), stage(75)],
    },
    Shot {
        name: "c252-send-pre-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c253-muted-send-curve",
        acts: &[Act::Reset, stage(70), stage(76)],
    },
    Shot {
        name: "c254-muted-send-curve-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c255-audio-rename",
        acts: &[Act::Reset, stage(0), stage(90)],
    },
    Shot {
        name: "c256-audio-rename-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c257-midi-rename",
        acts: &[Act::Reset, stage(0), stage(91)],
    },
    Shot {
        name: "c258-midi-rename-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c259-midi-quantize-restore",
        acts: &[Act::Reset, stage(0), stage(92)],
    },
    Shot {
        name: "c260-midi-restore-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c261-adjacent-clips-joined",
        acts: &[Act::Reset, stage(0), stage(93)],
    },
    Shot {
        name: "c262-clip-join-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c263-empty-lane-marker",
        acts: &[Act::Reset, stage(0), stage(94)],
    },
    Shot {
        name: "c264-empty-marker-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c265-note-right-eraser",
        acts: &[Act::Reset, stage(60), stage(95)],
    },
    Shot {
        name: "c266-mixer-track-pasted",
        acts: &[Act::Reset, stage(70), stage(86)],
    },
    Shot {
        name: "c267-mixer-paste-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c268-mixer-paste-key",
        acts: &[stage(87)],
    },
    Shot {
        name: "c269-mixer-key-paste-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c270-marker-insert-key",
        acts: &[Act::Reset, stage(0), stage(110)],
    },
    Shot {
        name: "c271-marker-key-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c272-export-trash-button-confirm",
        acts: &[Act::Reset, stage(100)],
    },
    Shot {
        name: "c273-seconds-ruler-zoom",
        acts: &[Act::Reset, stage(0), stage(82), stage(111)],
    },
    Shot {
        name: "c274-muted-send-drag",
        acts: &[Act::Reset, stage(70), stage(71), stage(112)],
    },
    Shot {
        name: "c275-muted-send-drag-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c276-insert-recent-menu",
        acts: &[Act::Reset, stage(70), stage(113)],
    },
    Shot {
        name: "c277-insert-recent-list",
        acts: &[stage(114)],
    },
    Shot {
        name: "c278-insert-recent-added",
        acts: &[stage(115)],
    },
    Shot {
        name: "c279-insert-recent-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c280-plugin-key-pasted",
        acts: &[Act::Reset, stage(70), stage(116)],
    },
    Shot {
        name: "c281-plugin-key-paste-undone",
        acts: &[stage(3)],
    },
    Shot {
        name: "c282-send-excludes-feedback",
        acts: &[Act::Reset, stage(70), stage(117)],
    },
    Shot {
        name: "c283-filtered-delete-separator",
        acts: &[Act::Reset, stage(60), stage(118)],
    },
];

fn press(p: (f32, f32), button: MouseButton, window: &mut Window, cx: &mut App) {
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
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
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

fn choose(studio: &Rc<RefCell<Studio>>, label: &str, window: &mut Window, cx: &mut App) {
    let p = {
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
    press(p, MouseButton::Left, window, cx);
}

fn remember(s: &Studio) {
    BEFORE.with(|p| *p.borrow_mut() = Some(s.project().as_ref().clone()));
}

fn undo(studio: &Rc<RefCell<Studio>>, window: &Window) {
    let mut s = studio.borrow_mut();
    let g = crate::screen::geometry(&s, window);
    s.undo(&g);
    BEFORE.with(|p| {
        let before = p.borrow();
        let before = before.as_ref().expect("before an edit");
        let mut now = s.project().as_ref().clone();
        now.next_id = before.next_id;
        assert_eq!(&now, before, "one undo restores the song");
    });
}

fn marker_press(studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let p = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        let r = s.marker_rect(&g, s.project().markers[0].id).unwrap();
        (r.x + 4.0, r.y + 4.0)
    };
    press(p, MouseButton::Right, window, cx);
    assert!(studio.borrow().context_open());
}

fn take_press(studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    {
        // The take rows start folded (UI-296): the clip's count of takes opens them.
        let mut s = studio.borrow_mut();
        let t = crate::arrange::probe::track(&s, "Vocal");
        if !s.takes_open.contains(&t) {
            s.rec_down(t, crate::rec_area::RecPart::Fold);
        }
    }
    let p = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        let t = crate::arrange::probe::track(&s, "Vocal");
        let row = s
            .tree()
            .row_of(crate::tree::RowKind::Track(t))
            .copied()
            .unwrap();
        let area = crate::rec_area::RecArea::of(&s.project().tracks[&t], true);
        (
            s.tick_x(&g, 2 * 3840) + 16.0,
            s.row_y(&g, &row)
                + row.h
                + row.lanes as f32 * crate::tree::LANE_H
                + area.take_y(0)
                + 12.0,
        )
    };
    press(p, MouseButton::Right, window, cx);
    assert!(studio.borrow().context_open());
}

fn type_number(studio: &Rc<RefCell<Studio>>, value: &str) {
    let mut s = studio.borrow_mut();
    assert!(s.ask_field().is_some());
    for _ in 0..40 {
        s.ask_key("backspace");
    }
    s.ask_text(None, value, false);
    s.ask_key("enter");
    assert!(!s.context_open());
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    match n {
        0 => {
            let mut s = studio.borrow_mut();
            s.drawer_open = false;
            s.snap_mode = crate::tools::SnapMode::Bar;
            let g = crate::screen::geometry(&s, window);
            s.resized(&g);
        }
        1 => {
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                (s.tick_x(&g, 7680) + 2.0, g.ruler.bottom() - 4.0)
            };
            press(p, MouseButton::Right, window, cx);
            assert!(studio.borrow().context_open());
        }
        2 => {
            remember(&studio.borrow());
            choose(studio, "マーカーを追加", window, cx);
            assert_eq!(studio.borrow().project().markers.len(), 1);
        }
        3 => undo(studio, window),
        4 => {
            {
                let mut s = studio.borrow_mut();
                s.song
                    .run("marker.add", json!({"at": 7680, "name": "Verse"}))
                    .unwrap();
                s.song
                    .run("marker.add", json!({"at": 15360, "name": "Chorus"}))
                    .unwrap();
            }
            marker_press(studio, window, cx);
        }
        5 => {
            remember(&studio.borrow());
            choose(studio, "次のマーカーまでをループに", window, cx);
            assert_eq!(studio.borrow().project().loop_range.unwrap().end, 15360);
        }
        6 => {
            marker_press(studio, window, cx);
            remember(&studio.borrow());
            choose(studio, "マーカーを削除", window, cx);
            assert_eq!(studio.borrow().project().markers.len(), 1);
        }
        10 => {
            marker_press(studio, window, cx);
            remember(&studio.borrow());
            choose(studio, "マーカー名を変更", window, cx);
            type_number(studio, "Bridge");
            assert_eq!(studio.borrow().project().markers[0].name, "Bridge");
        }
        12 => {
            marker_press(studio, window, cx);
            remember(&studio.borrow());
            choose(studio, "色", window, cx);
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let layout = s.popup.as_ref().unwrap().layout(g.size);
                let c = layout
                    .cells
                    .iter()
                    .filter(|c| matches!(c.look, crate::popup::Look::Swatch(_)))
                    .nth(3)
                    .unwrap();
                (c.rect.x + c.rect.w / 2.0, c.rect.y + c.rect.h / 2.0)
            };
            press(p, MouseButton::Left, window, cx);
            assert!(studio.borrow().project().markers[0].color.is_some());
        }
        20 => {
            {
                let mut s = studio.borrow_mut();
                let t = crate::arrange::probe::track(&s, "Vocal");
                let file = match &s.project().tracks[&t].clips[0].body {
                    xirius_core::model::ClipBody::Audio { file, .. } => file.clone(),
                    _ => unreachable!(),
                };
                s.song.run("record.take.add",json!({"takes":[{"track":t.to_string(),"start":7680,"length":7680,"file":file,"name":"Take 1"},{"track":t.to_string(),"start":7680,"length":7680,"file":file,"name":"Take 2"},{"track":t.to_string(),"start":7680,"length":7680,"file":file,"name":"Take 3"}]})).unwrap();
                let g = crate::screen::geometry(&s, window);
                s.rebuild(&g);
                let row = s.tree().row_of(crate::tree::RowKind::Track(t)).unwrap();
                s.scroll.1 = (row.y - 20.0).max(0.0);
            }
            take_press(studio, window, cx);
        }
        21 => {
            remember(&studio.borrow());
            choose(studio, "このテイクを使用", window, cx);
            let s = studio.borrow();
            let t = crate::arrange::probe::track(&s, "Vocal");
            assert_eq!(
                s.project().tracks[&t]
                    .clips
                    .iter()
                    .filter(|c| c.name.starts_with("Take") && !c.muted)
                    .count(),
                1
            );
        }
        22 | 23 => {
            take_press(studio, window, cx);
            remember(&studio.borrow());
            choose(
                studio,
                if n == 22 {
                    "未使用のテイクをすべて削除"
                } else {
                    "テイクを削除"
                },
                window,
                cx,
            );
            let s = studio.borrow();
            let t = crate::arrange::probe::track(&s, "Vocal");
            assert_eq!(
                s.project().tracks[&t]
                    .clips
                    .iter()
                    .filter(|c| c.name.starts_with("Take"))
                    .count(),
                if n == 22 { 1 } else { 2 }
            );
        }
        30 => {
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                (
                    g.column.x + 50.0,
                    crate::arrange::probe::row_at(
                        &s,
                        &g,
                        crate::tree::RowKind::Track(crate::arrange::probe::track(&s, "Vocal")),
                        0.5,
                    ),
                )
            };
            press(p, MouseButton::Right, window, cx);
        }
        31 => {
            choose(studio, "コピー", window, cx);
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let row = s.tree().rows.last().copied().unwrap();
                (g.column.x + 50.0, s.row_y(&g, &row) + row.h + 10.0)
            };
            press(p, MouseButton::Right, window, cx);
            remember(&studio.borrow());
            let count = studio.borrow().project().tracks.len();
            choose(studio, "貼り付け", window, cx);
            assert_eq!(studio.borrow().project().tracks.len(), count + 1);
        }
        40 => {
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                (
                    s.tick_x(&g, 12 * 3840) + 4.0,
                    crate::arrange::probe::row_at(
                        &s,
                        &g,
                        crate::tree::RowKind::Track(crate::arrange::probe::track(&s, "Keys")),
                        0.5,
                    ),
                )
            };
            press(p, MouseButton::Middle, window, cx);
            assert!(studio.borrow().context_open());
        }
        41 => {
            choose(studio, "消しゴムツール", window, cx);
            assert_eq!(studio.borrow().tool, crate::tools::Tool::Eraser);
        }
        50 => {
            let p = {
                let mut s = studio.borrow_mut();
                s.browser.typing = true;
                s.input_map.browser_focus = true;
                s.tempo_visible = true;
                s.song
                    .run("tempo.set", json!({"at":7680,"bpm":90}))
                    .unwrap();
                let g = crate::screen::geometry(&s, window);
                s.resized(&g);
                s.tempo_xy(&g, 7680, 90.0)
            };
            press(p, MouseButton::Right, window, cx);
            assert!(studio.borrow().context_open());
        }
        51 => {
            remember(&studio.borrow());
            let mut s = studio.borrow_mut();
            s.popup = None;
            let g = crate::screen::geometry(&s, window);
            assert!(s.mapped_key(&g, "delete", None, crate::tools::Mods::NONE));
            assert_eq!(s.project().tempo.len(), 1);
        }
        60 => {
            let p = {
                let mut s = studio.borrow_mut();
                s.tab = crate::studio::DrawerTab::Roll;
                s.drawer_open = true;
                let g = crate::screen::geometry(&s, window);
                s.resized(&g);
                let c = s.roll_ctx(&g).unwrap();
                let v = s.roll.view(&c);
                let n = &c.notes()[0];
                (
                    (v.x(n.start) + v.x(n.start + n.length)) / 2.0,
                    v.y(n.pitch) + v.row_h / 2.0,
                )
            };
            press(p, MouseButton::Right, window, cx);
            choose(studio, "値を入力", window, cx);
        }
        61 => {
            remember(&studio.borrow());
            choose(studio, "ベロシティ…", window, cx);
            type_number(studio, "77");
            assert_eq!(studio.borrow().roll.next_vel, 77);
        }
        62..=64 => {
            remember(&studio.borrow());
            choose(
                studio,
                match n {
                    62 => "位置…",
                    63 => "長さ…",
                    _ => "ピッチ…",
                },
                window,
                cx,
            );
            type_number(
                studio,
                match n {
                    62 => "120",
                    63 => "720",
                    _ => "61",
                },
            );
        }
        70 => {
            let p = {
                let mut s = studio.borrow_mut();
                s.tab = crate::studio::DrawerTab::Mixer;
                s.drawer_open = true;
                let t = crate::arrange::probe::track(&s, "Vocal");
                let bus = s
                    .song
                    .run("track.add.bus", json!({"name":"Send Bus"}))
                    .unwrap()["id"]
                    .as_str()
                    .unwrap()
                    .to_string();
                s.song
                    .run(
                        "send.add",
                        json!({"ids":[t.to_string()],"to":bus,"level":-9}),
                    )
                    .unwrap();
                let id = s.project().tracks[&t].sends.last().unwrap().id;
                let g = crate::screen::geometry(&s, window);
                s.resized(&g);
                s.mixer_center(&g, crate::mixer::MixerHit::SendPre(t, id))
                    .unwrap()
            };
            press(p, MouseButton::Right, window, cx);
        }
        71 => {
            remember(&studio.borrow());
            choose(studio, "ミュート", window, cx);
            let s = studio.borrow();
            let t = crate::arrange::probe::track(&s, "Vocal");
            assert!(
                s.project().tracks[&t]
                    .sends
                    .last()
                    .unwrap()
                    .muted_level
                    .is_some()
            );
        }
        72 => {
            let p = {
                let s = studio.borrow();
                let t = crate::arrange::probe::track(&s, "Vocal");
                let id = s.project().tracks[&t].sends.last().unwrap().id;
                let g = crate::screen::geometry(&s, window);
                s.mixer_center(&g, crate::mixer::MixerHit::SendPre(t, id))
                    .unwrap()
            };
            press(p, MouseButton::Right, window, cx);
            remember(&studio.borrow());
            choose(studio, "送り先", window, cx);
            choose(studio, "新しいバス", window, cx);
        }
        73 => {
            remember(&studio.borrow());
            let mut s = studio.borrow_mut();
            s.popup = None;
            let g = crate::screen::geometry(&s, window);
            assert!(s.mapped_key(&g, "delete", None, crate::tools::Mods::NONE));
            let t = crate::arrange::probe::track(&s, "Vocal");
            assert!(s.project().tracks[&t].sends.is_empty());
        }
        74 => {
            let p = {
                let mut s = studio.borrow_mut();
                s.song
                    .run("track.add.bus", json!({"name":"Other Bus"}))
                    .unwrap();
                let t = crate::arrange::probe::track(&s, "Vocal");
                let id = s.project().tracks[&t].sends.last().unwrap().id;
                let g = crate::screen::geometry(&s, window);
                s.resized(&g);
                s.mixer_center(&g, crate::mixer::MixerHit::SendPre(t, id))
                    .unwrap()
            };
            press(p, MouseButton::Right, window, cx);
            remember(&studio.borrow());
            choose(studio, "送り先", window, cx);
            choose(studio, "Other Bus", window, cx);
            let s = studio.borrow();
            let t = crate::arrange::probe::track(&s, "Vocal");
            assert_eq!(
                s.project().tracks[&t].sends.last().unwrap().to,
                crate::arrange::probe::track(&s, "Other Bus")
            );
        }
        75 => {
            studio.borrow_mut().popup = None;
            let p = {
                let s = studio.borrow();
                let t = crate::arrange::probe::track(&s, "Vocal");
                let id = s.project().tracks[&t].sends.last().unwrap().id;
                let g = crate::screen::geometry(&s, window);
                s.mixer_center(&g, crate::mixer::MixerHit::SendPre(t, id))
                    .unwrap()
            };
            remember(&studio.borrow());
            press(p, MouseButton::Left, window, cx);
            let s = studio.borrow();
            let t = crate::arrange::probe::track(&s, "Vocal");
            assert!(s.project().tracks[&t].sends.last().unwrap().pre);
        }
        76 => {
            studio.borrow_mut().popup = None;
            let p = {
                let mut s = studio.borrow_mut();
                s.drawer_open = false;
                let t = crate::arrange::probe::track(&s, "Vocal");
                let id = s.project().tracks[&t].sends.last().unwrap().id;
                let target = xirius_core::auto::AutoTarget::Send(id);
                s.song
                    .run("send.mute", json!({"id":id.to_string()}))
                    .unwrap();
                for (at, value) in [(15360, -18), (23040, -6)] {
                    s.song.run("auto.point.add",json!({"track":t.to_string(),"target":target.to_string(),"at":at,"value":value})).unwrap();
                }
                let g = crate::screen::geometry(&s, window);
                s.arrange.open.insert(t);
                s.rebuild(&g);
                let i = s
                    .arrange
                    .lanes_of(&s.project().tracks[&t])
                    .iter()
                    .position(|l| l.target == target)
                    .unwrap();
                let a = crate::arrange::probe::point_xy(&s, &g, t, i, 0);
                let b = crate::arrange::probe::point_xy(&s, &g, t, i, 1);
                ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)
            };
            press(p, MouseButton::Right, window, cx);
            choose(studio, "この線の形", window, cx);
            remember(&studio.borrow());
            choose(studio, "階段", window, cx);
        }
        80 => {
            let p = {
                let mut s = studio.borrow_mut();
                let top = std::env::current_dir()
                    .unwrap()
                    .join("target/ctx2-browser-fixture");
                crate::import_shots::make_place(&top);
                s.browser.press_rail(crate::browser::RailTab::Files);
                s.browser.add_place_fixture(&top).unwrap();
                s.browser
                    .open_dirs
                    .insert(top.join("Loops").to_string_lossy().into_owned());
                let g = crate::screen::geometry(&s, window);
                s.resized(&g);
                let i = crate::import_shots::file_row(&s, &g, "loop_120bpm.wav").unwrap();
                s.browser
                    .center_of(
                        &s.browser.layout(g.panel),
                        crate::browser::BrowserHit::File(i),
                    )
                    .unwrap()
            };
            press(p, MouseButton::Right, window, cx);
            choose(studio, "お気に入り", window, cx);
            press(p, MouseButton::Right, window, cx);
            assert!(
                studio
                    .borrow()
                    .context_rows()
                    .iter()
                    .any(|r| r.label == "お気に入り" && r.on)
            );
        }
        81 => {
            studio.borrow_mut().popup = None;
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let layout = s.browser.layout(g.panel);
                let i = layout
                    .rows
                    .iter()
                    .position(
                        |r| matches!(&r.row,crate::browser::BRow::Dir{name,..} if name=="Drums"),
                    )
                    .unwrap();
                s.browser
                    .center_of(
                        &s.browser.layout(g.panel),
                        crate::browser::BrowserHit::Dir(i),
                    )
                    .unwrap()
            };
            press(p, MouseButton::Right, window, cx);
            choose(studio, "お気に入り", window, cx);
            press(p, MouseButton::Right, window, cx);
            assert!(
                studio
                    .borrow()
                    .context_rows()
                    .iter()
                    .any(|r| r.label == "お気に入り" && r.on)
            );
        }
        82 => {
            play(1, studio, window, cx);
            choose(studio, "目盛り", window, cx);
            choose(studio, "分と秒", window, cx);
            assert!(studio.borrow().song.session().state.view.ruler_seconds);
        }
        83 => {
            studio.borrow_mut().popup = None;
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let last = s.mixer_strips(&g).last().unwrap().rect;
                (last.right() + 15.0, last.y + 60.0)
            };
            press(p, MouseButton::Right, window, cx);
            choose(studio, "チャンネルの幅", window, cx);
            choose(studio, "広い", window, cx);
            assert!(studio.borrow().song.session().state.view.mixer_wide);
        }
        84 => {
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                crate::arrange::probe::clip_mid(&s, &g, "Vocal", 0)
            };
            press(p, MouseButton::Right, window, cx);
            choose(studio, "選択範囲を書き出す…", window, cx);
            assert_eq!(
                studio.borrow().export.range,
                crate::export::Range::Selection
            );
        }
        86 | 87 => {
            let p = {
                let mut s = studio.borrow_mut();
                s.popup = None;
                s.tab = crate::studio::DrawerTab::Mixer;
                s.drawer_open = true;
                let t = crate::arrange::probe::track(&s, "Keys");
                s.song
                    .run("edit.copy", json!({"tracks":[t.to_string()]}))
                    .unwrap();
                let g = crate::screen::geometry(&s, window);
                s.mixer.scroll = s.mixer_max_scroll(&g);
                let last = s.mixer_strips(&g).last().unwrap().rect;
                (
                    (last.right() + 4.0).min(g.drawer_body.right() - 1.0),
                    last.y + 60.0,
                )
            };
            press(p, MouseButton::Right, window, cx);
            assert!(
                studio
                    .borrow()
                    .context_rows()
                    .iter()
                    .any(|r| r.label == "貼り付け" && r.key == "Ctrl+V")
            );
            remember(&studio.borrow());
            if n == 86 {
                choose(studio, "貼り付け", window, cx);
            } else {
                let mut s = studio.borrow_mut();
                s.popup = None;
                let g = crate::screen::geometry(&s, window);
                assert!(s.mapped_key(&g, "v", None, crate::tools::Mods::ctrl()));
            }
        }
        100 => {
            let p = {
                let mut s = studio.borrow_mut();
                let dir = std::env::current_dir()
                    .unwrap()
                    .join(format!("target/ctx2-export-fixture-{}", std::process::id()));
                s.command(|song| song.save_as(&dir));
                assert!(s.error.is_none(), "{:?}", s.error);
                let g = crate::screen::geometry(&s, window);
                s.open_export(crate::studio::Rect::new(g.size.0 - 60.0, -40.0, 28.0, 28.0));
                assert!(s.export_action(crate::export::Act::Go));
                crate::shot::run_for(&mut s, 30.0, |s| !s.export.running());
                s.refresh_popup();
                let cell = s
                    .popup
                    .as_ref()
                    .unwrap()
                    .layout(g.size)
                    .cells
                    .into_iter()
                    .find(|c| {
                        matches!(
                            c.action,
                            Some(crate::popup::Action::Xp(crate::export::Act::Delete(_)))
                        )
                    })
                    .unwrap();
                (
                    cell.rect.x + cell.rect.w / 2.0,
                    cell.rect.y + cell.rect.h / 2.0,
                )
            };
            press(p, MouseButton::Left, window, cx);
            let s = studio.borrow();
            assert!(s.context_open());
            assert_eq!(s.export.jobs.len(), 1);
            assert!(
                std::path::Path::new(s.export.jobs[0]["result"]["path"].as_str().unwrap()).exists()
            );
        }
        111 => {
            let mut s = studio.borrow_mut();
            s.beat_w = 8.0;
            let g = crate::screen::geometry(&s, window);
            s.resized(&g);
            assert!(s.song.session().state.view.ruler_seconds);
        }
        110 => {
            remember(&studio.borrow());
            let mut s = studio.borrow_mut();
            s.arrange.focus = true;
            s.playhead = 4 * 3840;
            let g = crate::screen::geometry(&s, window);
            assert!(s.mapped_key(&g, "insert", None, crate::tools::Mods::NONE));
            assert_eq!(s.project().markers.len(), 1);
            assert_eq!(s.project().markers[0].tick, 4 * 3840);
        }
        90 | 91 => {
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                crate::arrange::probe::clip_mid(&s, &g, if n == 90 { "Vocal" } else { "Keys" }, 0)
            };
            press(p, MouseButton::Right, window, cx);
            remember(&studio.borrow());
            choose(studio, "クリップ名を変更", window, cx);
            type_number(studio, "Lead");
        }
        92 => {
            let p = {
                let mut s = studio.borrow_mut();
                let t = crate::arrange::probe::track(&s, "Keys");
                let c = s.project().tracks[&t].clips[0].id;
                let note = s.project().clip(c).unwrap().1.notes()[0].id;
                s.song
                    .run("note.move", json!({"ids":[note.to_string()],"by":137}))
                    .unwrap();
                s.song
                    .run(
                        "note.quantize",
                        json!({"ids":[c.to_string()],"grid":"1/16"}),
                    )
                    .unwrap();
                let g = crate::screen::geometry(&s, window);
                s.rebuild(&g);
                crate::arrange::probe::clip_mid(&s, &g, "Keys", 0)
            };
            press(p, MouseButton::Right, window, cx);
            choose(studio, "クオンタイズ", window, cx);
            remember(&studio.borrow());
            choose(studio, "元に戻す", window, cx);
        }
        93 => {
            let p = {
                let mut s = studio.borrow_mut();
                let t = crate::arrange::probe::track(&s, "Keys");
                let mut ids = Vec::new();
                for at in [12 * 3840, 13 * 3840] {
                    let r = s
                        .song
                        .run(
                            "clip.add.midi",
                            json!({"track":t.to_string(),"at":at,"bars":1}),
                        )
                        .unwrap();
                    ids.push(
                        r["id"]
                            .as_str()
                            .unwrap()
                            .parse::<xirius_core::id::ClipId>()
                            .unwrap(),
                    );
                }
                s.arrange.clips = ids.iter().map(|id| (t, *id)).collect();
                let g = crate::screen::geometry(&s, window);
                s.rebuild(&g);
                let i = s.project().tracks[&t]
                    .clips
                    .iter()
                    .position(|c| c.id == ids[0])
                    .unwrap();
                crate::arrange::probe::clip_mid(&s, &g, "Keys", i)
            };
            press(p, MouseButton::Right, window, cx);
            remember(&studio.borrow());
            choose(studio, "クリップを結合", window, cx);
        }
        94 => {
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let t = crate::arrange::probe::track(&s, "Keys");
                (
                    s.tick_x(&g, 12 * 3840) + 2.0,
                    crate::arrange::probe::row_at(&s, &g, crate::tree::RowKind::Track(t), 0.5),
                )
            };
            press(p, MouseButton::Right, window, cx);
            remember(&studio.borrow());
            choose(studio, "マーカーを追加", window, cx);
        }
        95 => {
            choose(studio, "戻る", window, cx);
            choose(studio, "消しゴムツール", window, cx);
            assert_eq!(studio.borrow().tool, crate::tools::Tool::Eraser);
        }
        112 => {
            remember(&studio.borrow());
            let p = {
                let s = studio.borrow();
                let t = crate::arrange::probe::track(&s, "Vocal");
                let id = s.project().tracks[&t].sends.last().unwrap().id;
                let g = crate::screen::geometry(&s, window);
                s.mixer_center(&g, crate::mixer::MixerHit::SendLevel(t, id))
                    .unwrap()
            };
            let from = point(px(p.0), px(p.1 + Layout::TOP_BAR_H));
            let to = point(px(p.0), px(p.1 + 30.0 + Layout::TOP_BAR_H));
            window.dispatch_event(
                PlatformInput::MouseDown(MouseDownEvent {
                    position: from,
                    button: MouseButton::Left,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                    first_mouse: false,
                }),
                cx,
            );
            window.dispatch_event(
                PlatformInput::MouseMove(MouseMoveEvent {
                    position: to,
                    pressed_button: Some(MouseButton::Left),
                    modifiers: Modifiers::default(),
                }),
                cx,
            );
            window.dispatch_event(
                PlatformInput::MouseUp(MouseUpEvent {
                    position: to,
                    button: MouseButton::Left,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                }),
                cx,
            );
            let s = studio.borrow();
            let t = crate::arrange::probe::track(&s, "Vocal");
            let p = s.project();
            let send = p.tracks[&t].sends.last().unwrap();
            assert_eq!(send.level_db, xirius_core::model::MIN_DB);
            assert_ne!(send.muted_level, Some(-9.0));
        }
        113 => {
            let p = {
                let mut s = studio.borrow_mut();
                s.popup = None;
                s.mixer.recent = vec![
                    crate::instrument::Pick {
                        name: "Compressor".into(),
                        format: "builtin".into(),
                        uid: "compressor".into(),
                    },
                    crate::instrument::Pick {
                        name: "EQ".into(),
                        format: "builtin".into(),
                        uid: "eq".into(),
                    },
                ];
                let t = crate::arrange::probe::track(&s, "Keys");
                let g = crate::screen::geometry(&s, window);
                s.mixer_center(&g, crate::mixer::MixerHit::AddInsert(t))
                    .unwrap()
            };
            press(p, MouseButton::Right, window, cx);
        }
        114 => choose(studio, "最近使ったもの", window, cx),
        115 => {
            remember(&studio.borrow());
            let t = crate::arrange::probe::track(&studio.borrow(), "Keys");
            let n = studio.borrow().project().tracks[&t].inserts.len();
            choose(studio, "Compressor", window, cx);
            assert_eq!(studio.borrow().project().tracks[&t].inserts.len(), n + 1);
        }
        116 => {
            let (p, id) = {
                let mut s = studio.borrow_mut();
                s.popup = None;
                let t = crate::arrange::probe::track(&s, "Vocal");
                let id=s.song.run("plugin.add",json!({"track":t.to_string(),"format":"builtin","uid":"compressor","name":"Compressor"})).unwrap()["id"].as_str().unwrap().parse().unwrap();
                let g = crate::screen::geometry(&s, window);
                (
                    s.mixer_center(&g, crate::mixer::MixerHit::Open(t, id))
                        .unwrap(),
                    id,
                )
            };
            press(p, MouseButton::Right, window, cx);
            assert!(
                studio
                    .borrow()
                    .context_rows()
                    .iter()
                    .any(|r| r.label == "コピー" && r.key == "Ctrl+C")
            );
            choose(studio, "コピー", window, cx);
            let (p, t) = {
                let s = studio.borrow();
                let t = crate::arrange::probe::track(&s, "Keys");
                let g = crate::screen::geometry(&s, window);
                (
                    s.mixer_center(&g, crate::mixer::MixerHit::AddInsert(t))
                        .unwrap(),
                    t,
                )
            };
            press(p, MouseButton::Right, window, cx);
            assert!(
                studio
                    .borrow()
                    .context_rows()
                    .iter()
                    .any(|r| r.label == "貼り付け" && r.key == "Ctrl+V")
            );
            remember(&studio.borrow());
            let mut s = studio.borrow_mut();
            s.popup = None;
            let before = s.project().tracks[&t].inserts.len();
            let g = crate::screen::geometry(&s, window);
            assert!(s.mapped_key(
                &g,
                "v",
                None,
                crate::tools::Mods {
                    ctrl: true,
                    ..crate::tools::Mods::NONE
                }
            ));
            assert_eq!(s.project().tracks[&t].inserts.len(), before + 1);
            assert_ne!(s.project().tracks[&t].inserts.last().unwrap().id, id);
        }
        117 => {
            let p = {
                let mut s = studio.borrow_mut();
                s.popup = None;
                let mut bus = Vec::new();
                for name in ["A", "B", "C"] {
                    bus.push(
                        s.song.run("track.add.bus", json!({"name":name})).unwrap()["id"]
                            .as_str()
                            .unwrap()
                            .parse::<xirius_core::id::TrackId>()
                            .unwrap(),
                    );
                }
                s.song
                    .run(
                        "send.add",
                        json!({"ids":[bus[0].to_string()],"to":bus[2].to_string()}),
                    )
                    .unwrap();
                s.song
                    .run(
                        "send.add",
                        json!({"ids":[bus[1].to_string()],"to":bus[0].to_string()}),
                    )
                    .unwrap();
                let id = s.project().tracks[&bus[0]].sends[0].id;
                let g = crate::screen::geometry(&s, window);
                s.mixer.scroll = s.mixer_max_scroll(&g);
                s.mixer_center(&g, crate::mixer::MixerHit::SendPre(bus[0], id))
                    .unwrap()
            };
            press(p, MouseButton::Right, window, cx);
            choose(studio, "送り先", window, cx);
            let rows = studio.borrow().context_rows();
            assert!(!rows.iter().any(|r| r.label == "A" || r.label == "B"));
            assert!(rows.iter().any(|r| r.label == "C"));
        }
        118 => {
            choose(studio, "戻る", window, cx);
            let mut s = studio.borrow_mut();
            assert!(s.context_query().is_some());
            s.ask_text(None, "ノート", false);
            let items = &s.ctx.as_ref().unwrap().items;
            assert_eq!(items.last().unwrap().label, "ノートを削除");
            assert!(matches!(
                items[items.len() - 2].run,
                crate::context::Run::Gap
            ));
        }
        _ => unreachable!("stage {n}"),
    }
}
