//! Saving and opening through the window, as a person does them (ARC-050): the keys and the
//! clicks go in where the OS hands them to the window (a headless platform's input), the save
//! is the real save worker, and the song saved is opened again by another process.

use crate::device_window::tests::headless::{HeadlessPlatform, HeadlessWindow, open_test};
use crate::popup::{Action, PopupKind};
use crate::screen::Screen;
use crate::shot::{Target, TestFolder};
use crate::song::Song;
use crate::studio::{FileRequest, Geometry, Hit, Studio};
use crate::topbar::BarButton;
use gpui::{
    Application, ApplicationHandle, Bounds, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers,
    MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput, point, px, size,
};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};
use xirius_design::Layout;

const W: f32 = 1280.0;
const H: f32 = 800.0;

/// One headless app; each studio gets a window of its own in it.
struct Desk {
    platform: Rc<HeadlessPlatform>,
    app: ApplicationHandle,
}

/// A studio in its window.
struct Win {
    studio: Rc<RefCell<Studio>>,
    window: HeadlessWindow,
}

impl Desk {
    fn new() -> Desk {
        let platform = HeadlessPlatform::new();
        let app = Application::with_platform(platform.clone()).run_embedded(|_| {});
        Desk { platform, app }
    }

    fn open(&self, studio: Studio) -> Win {
        let studio = Rc::new(RefCell::new(studio));
        let screen = studio.clone();
        self.app.update(|cx| {
            open_test(
                cx,
                Bounds::new(point(px(0.0), px(0.0)), size(px(W), px(H))),
                move |window, cx| Screen::for_test(screen, window, cx),
            )
            .unwrap();
        });
        let window = self.platform.windows.borrow().last().unwrap().clone();
        // The first frame sets up the screen's focus and its handlers.
        window.draw();
        Win { studio, window }
    }
}

impl Win {
    fn geometry(&self) -> Geometry {
        self.studio
            .borrow()
            .geometry(W, H - Layout::TOP_BAR_H - Layout::BOTTOM_BAR_H)
    }

    /// A key pressed and let go, as `Keystroke::parse` writes it (`ctrl-shift-s`).
    fn key(&self, key: &str) {
        let keystroke = Keystroke::parse(key).unwrap();
        self.window.dispatch(PlatformInput::KeyDown(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        }));
        self.window
            .dispatch(PlatformInput::KeyUp(KeyUpEvent { keystroke }));
        self.window.draw();
    }

    /// A left click at a point of the body.
    fn click(&self, (x, y): (f32, f32)) {
        let position = point(px(x), px(y + Layout::TOP_BAR_H));
        self.window
            .dispatch(PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }));
        self.window.dispatch(PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
        }));
        self.window.draw();
    }

    fn click_hit(&self, hit: Hit) {
        let g = self.geometry();
        let at = self.studio.borrow().center_of(&g, hit);
        self.click(at.unwrap_or_else(|| panic!("{hit:?} is not on the screen")));
    }

    /// The song's menu, opened by a click on the song's name in the top bar.
    fn song_menu(&self) {
        let hit = crate::shot::resolve(&self.studio.borrow(), Target::Top(BarButton::SongName))
            .expect("the song's name");
        self.click_hit(hit);
        let s = self.studio.borrow();
        assert_eq!(s.popup.as_ref().map(|p| p.kind), Some(PopupKind::Song));
    }

    /// Waits for a save and an open the window started, as its frames would.
    fn settle(&self) {
        {
            let mut s = self.studio.borrow_mut();
            if s.saving.is_some() {
                s.wait_for_save();
            }
            if s.opening.is_some() {
                s.wait_for_open_for_test();
            }
        }
        self.window.draw();
    }
}

fn song_json(s: &Studio) -> Value {
    serde_json::to_value(&*s.project()).unwrap()
}

const REOPEN: &str = "XIRIUS_SAVE_FLOW_REOPEN";

/// The other process: opens the folder in `XIRIUS_SAVE_FLOW_REOPEN` and writes its song next
/// to it as JSON.
#[test]
fn reopen_child() {
    let Some(dir) = std::env::var_os(REOPEN) else {
        return;
    };
    let dir = PathBuf::from(dir);
    let session =
        xirius_core::session::Session::open_with(&dir, xirius_session::commands::shared_registry())
            .unwrap();
    let song = serde_json::to_vec(session.project()).unwrap();
    std::fs::write(dir.with_extension("json"), song).unwrap();
}

/// The song in `dir`, as another process opens it.
fn reopened_elsewhere(dir: &Path) -> Value {
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .args(["--exact", "save_flow_tests::reopen_child", "--nocapture"])
        .env(REOPEN, dir)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        child.creation_flags(0x08000000);
    }
    let mut child = child.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "the other process: {status}");
            break;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the other process never opened the song");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    serde_json::from_slice(&std::fs::read(dir.with_extension("json")).unwrap()).unwrap()
}

fn track_named(project: &Value, name: &str) -> Value {
    project["tracks"]
        .as_object()
        .unwrap()
        .values()
        .find(|t| t["name"] == name)
        .unwrap_or_else(|| panic!("no track {name}"))
        .clone()
}

/// The song's folder comes from 別名で保存 by its key; then notes, a plug-in's knob, a mute, a
/// track's name and the tempo change, and Ctrl+S saves them all: another process opens the
/// same song (test audit §6 保存, types A and D).
#[test]
fn ctrl_s_saves_every_kind_of_change_and_another_process_opens_the_same_song() {
    let tmp = TestFolder::new("xirius-ui-save-flow-");
    let dir = tmp.path().join("Saved");
    let desk = Desk::new();
    let mut s = Studio::sample();
    s.keep = crate::keep::Keep::at(tmp.path().join("keep"));
    s.keep.sync(&s.song.project());
    s.save_test_destination = Some(dir.clone());
    let w = desk.open(s);

    w.key("ctrl-shift-s");
    assert!(
        w.studio.borrow().saving.is_some(),
        "Ctrl+Shift+S answered by the folder starts the save"
    );
    w.settle();
    {
        let s = w.studio.borrow();
        assert_eq!(s.error, None);
        assert_eq!(s.song.dir(), Some(dir.clone()));
        assert!(s.song.is_saved());
    }

    // Every kind of change, each by its command as the window's own controls run them.
    let keys = {
        let mut s = w.studio.borrow_mut();
        let p = s.project();
        let keys = p.tracks.values().find(|t| t.name == "Keys").unwrap();
        let (keys, clip, at) = (keys.id, keys.clips[0].id, keys.clips[0].start);
        let notes = p.tracks[&keys].clips[0].notes().len();
        let bass = p
            .tracks
            .values()
            .find(|t| !t.id.is_master() && t.id != keys)
            .unwrap()
            .id;
        assert!(s.run_command(
            "note.add",
            json!({"clip": clip.to_string(), "at": at, "pitch": 72, "length": 240}),
        ));
        let comp = s
            .song
            .run(
                "plugin.add",
                json!({"track": bass.to_string(), "name": "Compressor"}),
            )
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(s.run_command(
            "plugin.param.set",
            json!({"id": comp, "param": "threshold", "plain": "-30"}),
        ));
        assert!(s.run_command("track.mute", json!({"ids": [bass.to_string()], "on": true})));
        assert!(s.run_command(
            "track.rename",
            json!({"id": keys.to_string(), "name": "Piano"}),
        ));
        assert!(s.run_command("tempo.set", json!({"at": 0, "bpm": 150})));
        assert!(!s.song.is_saved());
        (bass, comp, notes)
    };

    w.key("ctrl-s");
    assert!(w.studio.borrow().saving.is_some(), "Ctrl+S starts the save");
    w.settle();
    let here = {
        let s = w.studio.borrow();
        assert_eq!(s.error, None);
        assert!(s.song.is_saved(), "the song is saved");
        song_json(&s)
    };
    // The window lets the song go before another process opens its folder.
    drop(w);
    drop(desk);

    let there = reopened_elsewhere(&dir);
    let differ = crate::e2e_shots::differences(&here, &there);
    let shown: Vec<_> = differ
        .iter()
        .map(|at| (at, here.pointer(at), there.pointer(at)))
        .collect();
    assert!(differ.is_empty(), "the reopened song differs at {shown:?}");
    // Opening restores the song from its history: project.json itself must hold it too, as
    // the saved song a lost history falls back to.
    let file = xirius_core::store::Store::new(&dir).read_project().unwrap();
    let file = serde_json::to_value(&file).unwrap();
    let differ = crate::e2e_shots::differences(&here, &file);
    let shown: Vec<_> = differ
        .iter()
        .map(|at| (at, here.pointer(at), file.pointer(at)))
        .collect();
    assert!(differ.is_empty(), "project.json differs at {shown:?}");
    // And each change is really in it, not only equal on both sides.
    let (bass, comp, notes) = keys;
    let piano = track_named(&there, "Piano");
    assert_eq!(
        piano["clips"][0]["notes"].as_array().map(Vec::len),
        Some(notes + 1),
        "the note"
    );
    let bass = &there["tracks"][bass.to_string()];
    assert_eq!(bass["mute"], true, "the mute");
    let comp = bass["inserts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == comp.as_str())
        .expect("the plug-in");
    let threshold = comp["params"]["threshold"].as_f64().expect("the knob");
    let spec = xirius_params::find("compressor").unwrap();
    let knob = spec.params.iter().find(|p| p.id == "threshold").unwrap();
    assert!(
        (knob.to_plain(threshold) - -30.0).abs() < 0.01,
        "the knob: {threshold}"
    );
    assert_eq!(there["tempo"][0]["bpm"], 150.0, "the tempo");
}

/// Ctrl+Shift+S asks where to save as 別名で保存, whatever the song (UI-378: it turned every
/// solo off instead).
#[test]
fn ctrl_shift_s_asks_where_to_save_the_song_as() {
    let desk = Desk::new();
    let tmp = TestFolder::new("xirius-ui-save-as-key-");
    let mut saved = Song::sample();
    saved.save_as(&tmp.path().join("Song")).unwrap();
    for song in [Song::sample(), saved] {
        let w = desk.open(Studio::new(song));
        let first = solo_one(&w);
        let before = song_json(&w.studio.borrow());
        w.key("ctrl-shift-s");
        let s = w.studio.borrow();
        assert_eq!(s.file_request, Some(FileRequest::SaveAs));
        assert_eq!(song_json(&s), before, "nothing else changed");
        assert!(s.project().tracks[&first].solo);
    }
}

fn solo_one(w: &Win) -> xirius_core::id::TrackId {
    let mut s = w.studio.borrow_mut();
    let first = s
        .project()
        .tracks
        .values()
        .find(|t| !t.id.is_master())
        .unwrap()
        .id;
    assert!(s.run_command(
        "track.solo",
        json!({"ids": [first.to_string()], "on": true})
    ));
    first
}

/// The key a menu's hint names, as `Keystroke::parse` writes it: `Ctrl+Shift+S` is
/// `ctrl-shift-s`. None for a hint that names no key (a recent song's age).
fn hint_key(hint: &str) -> Option<String> {
    let mut parts: Vec<&str> = hint.split('+').collect();
    let key = parts.pop()?;
    let key = match key {
        "Del" => "delete".to_string(),
        k if k.chars().count() == 1 && k.is_ascii() => k.to_ascii_lowercase(),
        k if k.starts_with('F') && k[1..].parse::<u8>().is_ok() => k.to_ascii_lowercase(),
        _ => return None,
    };
    let mods = parts
        .into_iter()
        .map(|m| match m {
            "Ctrl" => Some("ctrl"),
            "Shift" => Some("shift"),
            "Alt" => Some("alt"),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    Some(
        mods.into_iter()
            .chain([key.as_str()])
            .collect::<Vec<_>>()
            .join("-"),
    )
}

#[test]
fn a_hint_names_its_key() {
    assert_eq!(hint_key("Ctrl+Shift+S").as_deref(), Some("ctrl-shift-s"));
    assert_eq!(hint_key("Ctrl+,").as_deref(), Some("ctrl-,"));
    assert_eq!(hint_key("Shift+F11").as_deref(), Some("shift-f11"));
    assert_eq!(hint_key("Del").as_deref(), Some("delete"));
    assert_eq!(hint_key("3 分前"), None);
    assert_eq!(hint_key(""), None);
}

/// What a press or a key did, to compare: what it asks for, what opened, what waits, and
/// the song.
fn did(s: &Studio) -> String {
    format!(
        "file {:?}, popup {:?}, unsaved {}, saving {}, opening {}, home {}, error {:?}, \
         saved {}, dir {}, song {}",
        s.file_request,
        s.popup.as_ref().map(|p| p.kind),
        s.unsaved.is_some(),
        s.saving.is_some(),
        s.opening.is_some(),
        s.home.is_some(),
        s.error,
        s.song.is_saved(),
        s.song.dir().is_some(),
        song_json(s),
    )
}

/// A saved song with a solo, a mute and an edit since: what a key may wrongly touch shows,
/// and 保存 has something to save.
fn menu_studio(root: &Path, n: usize) -> Studio {
    let mut song = Song::sample();
    song.save_as(&root.join(format!("Song{n}")).join("Song"))
        .unwrap();
    let mut s = Studio::new(song);
    s.keep = crate::keep::Keep::at(root.join(format!("keep{n}")));
    s.keep.sync(&s.song.project());
    let ids: Vec<String> = s
        .project()
        .tracks
        .values()
        .filter(|t| !t.id.is_master())
        .take(2)
        .map(|t| t.id.to_string())
        .collect();
    assert!(s.run_command("track.solo", json!({"ids": [ids[0]], "on": true})));
    assert!(s.run_command("track.mute", json!({"ids": [ids[1]], "on": true})));
    assert!(s.run_command("tempo.set", json!({"at": 0, "bpm": 133})));
    s
}

/// Every row of the song's menu that shows a key does what that key does (test audit §6
/// 別名で保存, type B; UI-378, where 別名で保存's Ctrl+Shift+S turned every solo off): one
/// window clicks the row, another presses the key, and both end the same.
#[test]
fn every_key_the_song_menu_shows_does_what_its_row_does() {
    let tmp = TestFolder::new("xirius-ui-menu-keys-");
    let desk = Desk::new();
    let rows: Vec<(usize, String, String)> = {
        let w = desk.open(menu_studio(tmp.path(), 0));
        w.song_menu();
        let s = w.studio.borrow();
        let g = w.geometry();
        let l = s.popup.as_ref().unwrap().layout(g.size);
        l.cells
            .iter()
            .enumerate()
            .filter(|(_, c)| c.live())
            .filter_map(|(i, c)| Some((i, c.text.to_string(), hint_key(&c.hint)?)))
            .collect()
    };
    let shown: Vec<&str> = rows.iter().map(|(_, _, k)| k.as_str()).collect();
    for key in ["ctrl-n", "ctrl-o", "ctrl-w", "ctrl-s", "ctrl-shift-s"] {
        assert!(shown.contains(&key), "the menu shows {key}: {shown:?}");
    }
    let mut n = 1;
    for (cell, text, key) in rows {
        let row = desk.open(menu_studio(tmp.path(), n));
        let pressed = desk.open(menu_studio(tmp.path(), n + 1));
        n += 2;
        row.song_menu();
        row.click_hit(Hit::Item(cell));
        row.settle();
        pressed.key(&key);
        pressed.settle();
        let (by_row, by_key) = (did(&row.studio.borrow()), did(&pressed.studio.borrow()));
        assert_eq!(by_row, by_key, "「{text}」 and its key {key} do the same");
    }
}

/// The start screen's buttons that show a key do what that key does there (UI-350).
#[test]
fn every_key_the_start_screen_shows_does_what_its_button_does() {
    use crate::home::{Button, Home, Spot};
    let tmp = TestFolder::new("xirius-ui-home-keys-");
    let desk = Desk::new();
    let start = |n: usize| {
        let mut s = Studio::new(Song::empty());
        s.keep = crate::keep::Keep::at(tmp.path().join(format!("keep{n}")));
        s.keep.sync(&s.song.project());
        s.home = Some(Home::default());
        s.home_tick();
        s
    };
    let mut n = 0;
    let mut shown = 0;
    for button in Button::ALL {
        let Some(key) = hint_key(button.keys()) else {
            continue;
        };
        shown += 1;
        let clicked = desk.open(start(n));
        let pressed = desk.open(start(n + 1));
        n += 2;
        clicked.click_hit(Hit::Home(Spot::Button(button)));
        clicked.settle();
        pressed.key(&key);
        pressed.settle();
        assert_eq!(
            did(&clicked.studio.borrow()),
            did(&pressed.studio.borrow()),
            "{button:?} and its key {key} do the same"
        );
    }
    assert!(shown >= 2, "新規 and 開く… show their keys");
}

/// The songs at `names`, saved under `root`.
fn songs(root: &Path, names: &[&str]) -> Vec<PathBuf> {
    names
        .iter()
        .map(|name| {
            let dir = root.join(name);
            let mut song = Song::empty();
            song.run("track.add.audio", json!({"name": format!("in {name}")}))
                .unwrap();
            song.save_as(&dir).unwrap();
            dir
        })
        .collect()
}

/// A studio on an empty song whose own keep remembers `dirs`, oldest first.
fn studio_keeping(root: &Path, n: usize, dirs: &[PathBuf]) -> Studio {
    let keep = crate::keep::Keep::at(root.join(format!("keep{n}")));
    for (i, dir) in dirs.iter().enumerate() {
        let name = dir.file_name().unwrap().to_string_lossy();
        keep.remember(dir, &name, false, 1 + i as u64);
    }
    let mut s = Studio::new(Song::empty());
    s.keep = keep;
    s.keep.sync(&s.song.project());
    s.keep.settle_recent();
    s.keep.poll_recent();
    s
}

/// 開く… in the song's menu asks for a folder; the answer opens that song, the same as it
/// was saved (test audit §6 開く, type A).
#[test]
fn open_in_the_song_menu_opens_the_folder_answered_with_the_same_song() {
    let tmp = TestFolder::new("xirius-ui-open-flow-");
    let dirs = songs(tmp.path(), &["Other"]);
    let dir = tmp.path().join("Chosen");
    let mut song = Song::sample();
    song.run("tempo.set", json!({"at": 0, "bpm": 141})).unwrap();
    song.save_as(&dir).unwrap();
    let saved = serde_json::to_value(&*song.project()).unwrap();
    drop(song);

    let desk = Desk::new();
    let w = desk.open(studio_keeping(tmp.path(), 0, &dirs));
    w.song_menu();
    let open = {
        let s = w.studio.borrow();
        let l = s.popup.as_ref().unwrap().layout(w.geometry().size);
        l.cells
            .iter()
            .position(|c| c.live() && c.text == "開く…")
            .expect("開く…")
    };
    w.click_hit(Hit::Item(open));
    {
        let mut s = w.studio.borrow_mut();
        assert_eq!(s.file_request.take(), Some(FileRequest::Open));
        assert!(s.popup.is_none());
        s.start_file_request(FileRequest::Open, &dir);
    }
    w.settle();
    let s = w.studio.borrow();
    assert_eq!(s.error, None);
    assert_eq!(s.song.dir(), Some(dir.clone()));
    let differ = crate::e2e_shots::differences(&saved, &song_json(&s));
    assert!(differ.is_empty(), "the opened song differs at {differ:?}");
    assert!(s.song.is_saved());
}

/// The points over a rect, a few px in from its edges.
fn grid(r: crate::studio::Rect) -> Vec<(f32, f32)> {
    let xs = [
        r.x + 2.0,
        r.x + r.w * 0.25,
        r.x + r.w / 2.0,
        r.x + r.w * 0.75,
        r.x + r.w - 2.0,
    ];
    let ys = [r.y + 2.0, r.y + r.h / 2.0, r.y + r.h - 2.0];
    xs.iter()
        .flat_map(|&x| ys.iter().map(move |&y| (x, y)))
        .collect()
}

/// A recent song's row in the song's menu takes the press anywhere over it, and a click on
/// it opens that song (test audit §6 開く, type E).
#[test]
fn a_click_on_a_recent_row_in_the_song_menu_opens_that_song() {
    let tmp = TestFolder::new("xirius-ui-menu-recent-");
    let dirs = songs(tmp.path(), &["a", "b", "c"]);
    let desk = Desk::new();
    let rows = {
        let w = desk.open(studio_keeping(tmp.path(), 0, &dirs));
        w.song_menu();
        let s = w.studio.borrow();
        let g = w.geometry();
        let l = s.popup.as_ref().unwrap().layout(g.size);
        let rows: Vec<(usize, usize, crate::studio::Rect)> = l
            .cells
            .iter()
            .enumerate()
            .filter_map(|(cell, c)| match c.action {
                Some(Action::OpenRecent(i)) => Some((cell, i, c.rect)),
                _ => None,
            })
            .collect();
        assert_eq!(rows.len(), dirs.len(), "a row for each song");
        for &(cell, i, r) in &rows {
            for (x, y) in grid(r) {
                assert_eq!(
                    s.hit(&g, x, y),
                    Hit::Item(cell),
                    "row {i} takes the press at {x},{y}"
                );
            }
        }
        rows
    };
    for (n, (cell, i, _)) in rows.into_iter().enumerate() {
        let w = desk.open(studio_keeping(tmp.path(), n + 1, &dirs));
        let want = w.studio.borrow().keep.listed()[i].dir.clone();
        w.song_menu();
        w.click_hit(Hit::Item(cell));
        w.settle();
        let s = w.studio.borrow();
        assert_eq!(s.error, None);
        assert_eq!(s.song.dir(), Some(want.clone()), "row {i} opens its song");
        assert!(s.popup.is_none());
    }
}

/// Each row of the start screen takes the press anywhere over it, though the first lies over
/// the hidden tempo band; a click on it opens its song, adds no tempo point, and makes no new
/// song (test audit §6 開く, type E; UI-350, where the band took the press and 「新しい曲
/// （保存前）」 was made instead).
#[test]
fn a_click_on_a_row_of_the_start_screen_opens_that_song() {
    use crate::home::{Home, Spot};
    let tmp = TestFolder::new("xirius-ui-home-recent-");
    let dirs = songs(tmp.path(), &["a", "b", "c"]);
    let start = |n: usize| {
        let mut s = studio_keeping(tmp.path(), n, &dirs);
        s.home = Some(Home::default());
        s.home_tick();
        s
    };
    let desk = Desk::new();
    let rows = {
        let w = desk.open(start(0));
        let s = w.studio.borrow();
        let g = w.geometry();
        let l = s.home_layout(&g).expect("the start screen");
        let first = l.rows[0].1;
        let (x, y) = (first.x + first.w / 2.0, first.y + first.h / 2.0);
        assert_eq!(
            s.arrange_spot(&g, x, y),
            crate::arrange::Spot::TempoGap,
            "the tempo band is under the first row"
        );
        // A row for each song, newest first, then the sample's.
        assert_eq!(l.rows.len(), dirs.len() + 1);
        for (at, &(i, r)) in l.rows.iter().enumerate() {
            let spot = Spot::Row(i, l.row_keys[at]);
            for (x, y) in grid(r) {
                assert_eq!(s.hit(&g, x, y), Hit::Home(spot), "row {i} at {x},{y}");
            }
        }
        l.rows[..dirs.len()]
            .iter()
            .enumerate()
            .map(|(at, &(i, _))| Spot::Row(i, l.row_keys[at]))
            .collect::<Vec<_>>()
    };
    for (n, spot) in rows.into_iter().enumerate() {
        let w = desk.open(start(n + 1));
        let Spot::Row(i, _) = spot else {
            unreachable!()
        };
        let (tempo, want) = {
            let s = w.studio.borrow();
            (s.project().tempo.len(), s.keep.listed()[i].dir.clone())
        };
        w.click_hit(Hit::Home(spot));
        {
            let s = w.studio.borrow();
            assert!(s.opening.is_some(), "row {i} starts to open its song");
            assert_eq!(
                s.project().tempo.len(),
                tempo,
                "no tempo point under row {i}"
            );
        }
        w.settle();
        let s = w.studio.borrow();
        assert_eq!(s.error, None);
        assert_eq!(s.song.dir(), Some(want.clone()), "row {i} opens its song");
        assert!(!s.home_shown());
        assert!(
            s.keep.settled_recent().iter().all(|r| !r.draft),
            "no new song was made"
        );
    }
}
