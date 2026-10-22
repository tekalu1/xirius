//! `--script pool`: the pool tab (BRW-004) on a song of its own, every step through the real
//! (parked, silent) window's presses, drags and keys. The song is made in `XIRIUS_POOL_DIR` (or
//! `xirius-pool-shots` in the temp folder): recordings (one no clip uses), a file read in, one
//! outside the song, a bounce, a frozen track's file, and two files that are gone. Each stage
//! prints `pool-check stage=N ok=true` (`tests/pool_window.rs` reads the lines). The OS's file
//! window is never opened: the stage that asks for it answers in its place.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use xirius_core::id::{ClipId, Node, TrackId};
use xirius_core::model::{Clip, ClipBody, Frozen, Project, Track, TrackKind};
use xirius_design::Layout;

use crate::browser::{BRow, BrowserHit, RailTab};
use crate::shot::{Act, Play, Shot};
use crate::studio::{FileRequest, Geometry, Rect, Studio};

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "pool-00-tab",
        acts: &[stage(0)],
    },
    Shot {
        name: "pool-01-row-menu",
        acts: &[stage(1)],
    },
    Shot {
        name: "pool-02-missing-menu",
        acts: &[stage(2)],
    },
    Shot {
        name: "pool-03-located",
        acts: &[stage(3)],
    },
    Shot {
        name: "pool-04-more-menu",
        acts: &[stage(4)],
    },
    Shot {
        name: "pool-05-collected",
        acts: &[stage(5)],
    },
    Shot {
        name: "pool-06-keys",
        acts: &[stage(6)],
    },
    Shot {
        name: "pool-07-dragged",
        acts: &[stage(7)],
    },
    Shot {
        name: "pool-08-settings",
        acts: &[stage(8)],
    },
];

/// Where the song of the script is made.
fn root() -> PathBuf {
    std::env::var_os("XIRIUS_POOL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("xirius-pool-shots"))
}

/// The files of [`make_song`], as the song names them.
pub(crate) const VOCAL_1: &str = "Media/Recordings/Vocal_01.wav";
pub(crate) const VOCAL_2: &str = "Media/Recordings/Vocal_02.wav";
pub(crate) const KICK: &str = "Media/Imports/Kick.wav";
pub(crate) const MIX: &str = "Media/Bounces/Mix.wav";
pub(crate) const SYNTH: &str = "Media/Freeze/Synth.wav";

/// A short sound, so the clips have a wave to draw.
fn wav(path: &Path, hz: f32) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let rate = 48_000u32;
    let wave: Vec<f32> = (0..rate)
        .map(|i| {
            let t = i as f32 / rate as f32;
            (t * hz * std::f32::consts::TAU).sin() * 0.5 * (1.0 - t)
        })
        .collect();
    xirius_session::export::write_wav(path, &wave, &wave, rate, 16).unwrap();
}

fn audio(id: u64, start: i64, name: &str, file: &str) -> Clip {
    Clip {
        id: ClipId(id),
        name: name.into(),
        color: None,
        start,
        length: 1920,
        muted: false,
        take: None,
        controllers: Vec::new(),
        body: ClipBody::Audio {
            file: file.into(),
            offset: 0,
            gain_db: 0.0,
            process: Default::default(),
        },
    }
}

/// A saved song under `root` (in `root/曲`) for the tab, and its folder:
/// - `ボーカル` plays [`VOCAL_1`] twice; [`VOCAL_2`] is a take no clip uses.
/// - `ドラム` plays [`KICK`], [`MIX`] and `root/外部/Pad.wav` (outside the song).
/// - `ベース` plays `root/なくした/Bass.wav` and `Gtr.wav`, which are not there; the files of
///   the same names are in `root/見つけた`.
/// - `シンセ` is frozen to [`SYNTH`] and has no clips.
pub(crate) fn make_song(root: &Path) -> PathBuf {
    let dir = root.join("曲");
    let lost = root.join("なくした");
    let text = |p: PathBuf| p.to_string_lossy().into_owned();
    let mut p = Project::new("プールの撮影");
    p.next_id = 1000;
    let mut vocal = Track::new(TrackId(100), TrackKind::Audio, "ボーカル");
    vocal.clips = vec![
        audio(300, 0, "Vocal_01", VOCAL_1),
        audio(301, 3840, "Vocal_01", VOCAL_1),
    ];
    let mut drums = Track::new(TrackId(101), TrackKind::Audio, "ドラム");
    drums.clips = vec![
        audio(310, 0, "Kick", KICK),
        audio(311, 1920, "Pad", &text(root.join("外部/Pad.wav"))),
        audio(312, 3840, "Mix", MIX),
    ];
    let mut bass = Track::new(TrackId(102), TrackKind::Audio, "ベース");
    bass.clips = vec![
        audio(320, 0, "Bass", &text(lost.join("Bass.wav"))),
        audio(321, 1920, "Gtr", &text(lost.join("Gtr.wav"))),
    ];
    let mut synth = Track::new(TrackId(103), TrackKind::Audio, "シンセ");
    synth.frozen = Some(Frozen {
        file: SYNTH.into(),
        start: 0,
    });
    for t in [vocal, drums, bass, synth] {
        p.root.push(Node::Track(t.id));
        p.tracks.insert(t.id, Arc::new(t));
    }
    drop(xirius_core::session::Session::create(&dir, p).unwrap());
    for (f, hz) in [
        (VOCAL_1, 220.0),
        (VOCAL_2, 247.0),
        (KICK, 60.0),
        (MIX, 330.0),
        (SYNTH, 440.0),
    ] {
        wav(&dir.join(f), hz);
    }
    wav(&root.join("外部/Pad.wav"), 294.0);
    wav(&root.join("見つけた/Bass.wav"), 82.0);
    wav(&root.join("見つけた/Gtr.wav"), 165.0);
    dir
}

/// The row of the tab whose file is named `name`, and its place on the list.
pub(crate) fn pool_row(s: &Studio, g: &Geometry, name: &str) -> Option<usize> {
    let l = s.browser.layout(g.panel);
    l.rows
        .iter()
        .position(|p| matches!(&p.row, BRow::Pool { name: n, .. } if n == name))
}

/// The middle of the row of `name`.
pub(crate) fn pool_mid(s: &Studio, g: &Geometry, name: &str) -> (f32, f32) {
    let i = pool_row(s, g, name).unwrap_or_else(|| panic!("no row {name}"));
    let l = s.browser.layout(g.panel);
    s.browser
        .center_of(&l, BrowserHit::Pool(i))
        .expect("on the list")
}

type Studio1 = Rc<RefCell<Studio>>;

fn at(p: (f32, f32)) -> gpui::Point<gpui::Pixels> {
    point(px(p.0), px(p.1 + Layout::TOP_BAR_H))
}

fn hover(p: (f32, f32), w: &mut Window, cx: &mut App) {
    w.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at(p),
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
}

fn down(p: (f32, f32), b: MouseButton, w: &mut Window, cx: &mut App) {
    w.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: b,
            position: at(p),
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
}

fn up(p: (f32, f32), b: MouseButton, w: &mut Window, cx: &mut App) {
    w.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: b,
            position: at(p),
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}

fn click(p: (f32, f32), w: &mut Window, cx: &mut App) {
    hover(p, w, cx);
    down(p, MouseButton::Left, w, cx);
    up(p, MouseButton::Left, w, cx);
}

fn right_click(p: (f32, f32), w: &mut Window, cx: &mut App) {
    hover(p, w, cx);
    down(p, MouseButton::Right, w, cx);
    up(p, MouseButton::Right, w, cx);
}

/// Presses at `a`, moves to `b` in steps with the button held, stops there, lets go.
fn drag(a: (f32, f32), b: (f32, f32), w: &mut Window, cx: &mut App) {
    hover(a, w, cx);
    down(a, MouseButton::Left, w, cx);
    for i in 1..=6 {
        let f = i as f32 / 6.0;
        w.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position: at((a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f)),
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::default(),
            }),
            cx,
        );
    }
    up(b, MouseButton::Left, w, cx);
}

fn keys(text: &str, w: &mut Window, cx: &mut App) {
    for key in text.split(' ') {
        w.dispatch_keystroke(gpui::Keystroke::parse(key).unwrap(), cx);
    }
}

fn geometry(s: &Studio1, w: &Window) -> Geometry {
    crate::screen::geometry(&s.borrow(), w)
}

fn mid(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

/// The middle of the row `label` of the menu that is open.
fn menu_row(s: &Studio1, g: &Geometry, label: &str) -> (f32, f32) {
    let s = s.borrow();
    let layout = s.popup.as_ref().expect("a menu is open").layout(g.size);
    let cell = layout
        .cells
        .iter()
        .find(|c| c.text == label)
        .unwrap_or_else(|| panic!("no row {label}"));
    mid(cell.rect)
}

/// The labels of the menu that is open, without its gaps.
fn labels(s: &Studio1) -> Vec<String> {
    s.borrow()
        .context_rows()
        .into_iter()
        .filter(|r| !r.gap)
        .map(|r| r.label)
        .collect()
}

fn enabled(s: &Studio1, label: &str) -> bool {
    s.borrow()
        .context_rows()
        .iter()
        .find(|r| r.label == label)
        .unwrap_or_else(|| panic!("no row {label}"))
        .enabled
}

/// The clips of the song that play `file` (as the song names it).
fn using(s: &Studio, file: &str) -> usize {
    s.project()
        .tracks
        .values()
        .flat_map(|t| t.clips.iter())
        .filter(|c| matches!(&c.body, ClipBody::Audio { file: f, .. } if f == file))
        .count()
}

/// The OS's file window answers `paths` to what the press asked for.
fn answer(s: &Studio1, want: &FileRequest, paths: &[PathBuf]) {
    let asked = s.borrow_mut().file_request.take();
    assert_eq!(asked.as_ref(), Some(want));
    s.borrow_mut().finish_picked(asked.unwrap(), paths);
}

fn notice(s: &Studio1) -> String {
    s.borrow().notice_text().unwrap_or_default().to_string()
}

fn play(n: u8, s: &Studio1, w: &mut Window, cx: &mut App) {
    let root = root();
    match n {
        0 => {
            if root.exists() {
                std::fs::remove_dir_all(&root).unwrap();
            }
            let dir = make_song(&root);
            assert!(s.borrow_mut().command(|song| song.open(&dir)));
            s.borrow_mut().drawer_open = false;
            let g = geometry(s, w);
            let tab = crate::browser::rail_button(g.rail, RailTab::SongFiles);
            click(mid(tab), w, cx);
            s.borrow_mut().follow_browser();
            let g = geometry(s, w);
            // The pointer leaves the rail, whose tip would cover the headings.
            hover(
                (g.panel.x + g.panel.w / 2.0, g.panel.bottom() - 40.0),
                w,
                cx,
            );
            let st = s.borrow();
            assert!(st.pool_shown());
            assert_eq!(st.browser.pool_missing(), 2, "the rail's dot");
            let l = st.browser.layout(g.panel);
            let headings: Vec<&str> = l
                .rows
                .iter()
                .filter_map(|p| match &p.row {
                    BRow::Group { name, .. } => Some(*name),
                    _ => None,
                })
                .collect();
            assert_eq!(headings, ["録音", "インポート", "バウンス", "フリーズ"]);
        }
        1 => {
            let g = geometry(s, w);
            let at = pool_mid(&s.borrow(), &g, "Vocal_01.wav");
            click(at, w, cx);
            let key = root.join("曲").join(VOCAL_1);
            let selected = s.borrow().browser.selected_file.clone().unwrap();
            assert_eq!(Path::new(&selected), key);
            right_click(at, w, cx);
            assert_eq!(
                labels(s),
                [
                    "新しいトラックに置く",
                    "選んだトラックに置く",
                    "使っているクリップを選択",
                    "エクスプローラーで表示",
                    "名前を変更",
                    "ごみ箱へ",
                ]
            );
            // 使っているクリップを選択: both clips of Vocal_01 are selected.
            let at = menu_row(s, &g, "使っているクリップを選択");
            click(at, w, cx);
            assert_eq!(s.borrow().arrange.clips.len(), 2);
            let at = pool_mid(&s.borrow(), &g, "Vocal_01.wav");
            right_click(at, w, cx);
        }
        2 => {
            keys("escape", w, cx);
            assert!(!s.borrow().context_open());
            let g = geometry(s, w);
            let at = pool_mid(&s.borrow(), &g, "Bass.wav");
            right_click(at, w, cx);
            assert_eq!(labels(s), ["場所を指定…", "使っているクリップを選択"]);
        }
        3 => {
            let g = geometry(s, w);
            let at = menu_row(s, &g, "場所を指定…");
            click(at, w, cx);
            let lost = root
                .join("なくした")
                .join("Bass.wav")
                .to_string_lossy()
                .into_owned();
            answer(
                s,
                &FileRequest::Locate(lost),
                &[root.join("見つけた/Bass.wav")],
            );
            assert_eq!(s.borrow().error, None);
            assert_eq!(notice(s), "Bass.wav ほか 1 個の場所を指定しました");
            s.borrow_mut().follow_browser();
            assert_eq!(s.borrow().browser.pool_missing(), 0, "the dot is gone");
        }
        4 => {
            let g = geometry(s, w);
            let more = s.borrow().browser.layout(g.panel).more.expect("the 「…」");
            click(mid(more), w, cx);
            assert_eq!(
                labels(s),
                [
                    "見つからないファイルを探す…",
                    "外部のファイルを曲のフォルダーにコピー",
                    "曲のフォルダーをエクスプローラーで表示",
                    "使われていない録音を整理…",
                ]
            );
            assert!(!enabled(s, "見つからないファイルを探す…"));
            assert!(enabled(s, "外部のファイルを曲のフォルダーにコピー"));
        }
        5 => {
            let g = geometry(s, w);
            let at = menu_row(s, &g, "外部のファイルを曲のフォルダーにコピー");
            click(at, w, cx);
            assert_eq!(s.borrow().error, None);
            // Pad.wav, and Bass.wav and Gtr.wav where stage 3 found them.
            assert_eq!(notice(s), "ファイル 3 個を曲のフォルダーにコピーしました");
            s.borrow_mut().follow_browser();
            assert!(s.borrow().browser.pool.iter().all(|e| !e.external));
        }
        6 => {
            // The keys go to the list once it was pressed: ↑ from Vocal_01 is the heading, ←
            // folds it, → opens it again, ↓ is the first file again.
            let g = geometry(s, w);
            let at = pool_mid(&s.borrow(), &g, "Vocal_01.wav");
            click(at, w, cx);
            keys("up", w, cx);
            assert_eq!(
                s.borrow().browser.pool_heading.as_deref(),
                Some("SongFiles/record")
            );
            keys("left", w, cx);
            assert!(pool_row(&s.borrow(), &geometry(s, w), "Vocal_01.wav").is_none());
            keys("right down", w, cx);
            let selected = s.borrow().browser.selected_file.clone().unwrap();
            assert_eq!(Path::new(&selected), root.join("曲").join(VOCAL_1));
            keys("shift-f10", w, cx);
            assert!(s.borrow().context_open());
            assert_eq!(labels(s)[0], "新しいトラックに置く");
            keys("escape", w, cx);
        }
        7 => {
            let g = geometry(s, w);
            let tracks = s.borrow().project().tracks.len();
            // Vocal_01 is selected with its band above Kick.wav: the press closes the band and
            // the row moves up under the pointer, and the drag still holds Kick.wav.
            let from = pool_mid(&s.borrow(), &g, "Kick.wav");
            let to = (g.lanes.x + g.lanes.w * 0.6, g.lanes.bottom() - 30.0);
            drag(from, to, w, cx);
            assert_eq!(s.borrow().error, None);
            assert_eq!(s.borrow().project().tracks.len(), tracks + 1);
            assert_eq!(using(&s.borrow(), KICK), 2);
        }
        8 => {
            assert!(
                s.borrow_mut()
                    .open_palette_form("project.settings", serde_json::json!({}))
            );
            let g = geometry(s, w);
            let field = |name: &str| {
                let st = s.borrow();
                let p = st.palette.as_ref().unwrap();
                let i = p
                    .form
                    .as_ref()
                    .unwrap()
                    .fields
                    .iter()
                    .position(|f| f.name == name)
                    .unwrap();
                let r = p
                    .layout(g.size)
                    .rows
                    .iter()
                    .find(|(j, _)| *j == i)
                    .unwrap()
                    .1;
                (i, r)
            };
            let value = |i: usize| {
                s.borrow()
                    .palette
                    .as_ref()
                    .unwrap()
                    .form
                    .as_ref()
                    .unwrap()
                    .fields[i]
                    .value
                    .clone()
            };
            let (copy, r) = field("copy_imports");
            let was = value(copy);
            click(mid(r), w, cx);
            assert_ne!(value(copy), was, "one press flips the switch");
            let (folder, r) = field("media_folder");
            click(mid(crate::palette_args::folder_button(r)), w, cx);
            let dir = root.join("曲");
            answer(s, &FileRequest::MediaFolder, &[dir.join("素材置き場")]);
            assert_eq!(value(folder), "素材置き場");
            // The pointer rests on the switch: the folder button's tip would cover its value.
            let (_, r) = field("copy_imports");
            hover(mid(r), w, cx);
        }
        _ => unreachable!(),
    }
    let error = s.borrow_mut().error.take();
    assert_eq!(error, None);
    eprintln!("pool-check stage={n} ok=true");
}
