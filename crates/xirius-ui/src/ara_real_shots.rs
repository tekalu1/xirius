//! `--script ara-real`: a real ARA plugin on a clip, as a person uses it (EDT-023, PLG-036,
//! PLG-055; the approved screens of 2026-11-03): attached from the clip's right-click menu and
//! the list, the badge while it analyses and when it is done, its automatically opened window,
//! the window closed and the song saved and opened again with its archive, and detached.
//!
//! Needs `XIRIUS_ARA_REAL_PLUGIN` set to the plugin's `.vst3`; without it, or when it names no
//! ARA plugin, the run fails at its first stage (it never passes by skipping). Each stage fails
//! when it does not see what a person would. `tests/ara_real_window.rs` runs it (`python
//! scripts/ci_tests.py --plugins`, before a release). Run with `--park --silent` and
//! `XIRIUS_EDITOR_OFFSCREEN=1`: the app's window and the plugin's are off every screen, nothing
//! is played to a speaker. The song is a tone written into a folder of its own under the temp
//! folder; no song of the user's is opened.
//!
//! An unexpected dialog in the app or plugin host (activation or a question) ends the run
//! without interacting with it. Each stage writes its observations to stderr as
//! `ara-check <shot> …`.
//! The only input to the plugin's window is a close message sent to its own frame.

use std::path::PathBuf;
use std::sync::OnceLock;

use serde_json::json;
use xirius_core::id::ClipId;
use xirius_plugins::{Catalog, Entry};
use xirius_session::ara::{Phase, Report};

use crate::ara_shots::run_ok;
use crate::arrange::probe::{clip_mid, track};
use crate::clip_ara::Badge;
use crate::shot::{Act, Play, Shot, run_for};
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;

const TRACK: &str = "ARA";
const RATE: u32 = 48_000;
const SECONDS: f64 = 8.0;
const FIXTURE_UID: &str = "xirius-native-ara-fixture";
const FIXTURE_NAME: &str = "ARA test plugin";

fn fixture() -> bool {
    cfg!(all(windows, debug_assertions)) && std::env::var_os("XIRIUS_ARA_NATIVE_FIXTURE").is_some()
}

const fn laid(n: u8) -> Act {
    Act::Stage(Play::Laid(play), n)
}

pub const SHOTS: &[Shot] = &[
    // オーディオ › of the tone's clip, with the row that adds the plugin.
    Shot {
        name: "ara-real01-menu",
        acts: &[Act::Reset, laid(0), laid(1)],
    },
    // The list, with the plugin.
    Shot {
        name: "ara-real02-pick",
        acts: &[laid(2)],
    },
    // Picked: the badge's ring while the plugin analyses.
    Shot {
        name: "ara-real03-busy",
        acts: &[laid(3)],
    },
    // Done: the plugin's mark.
    Shot {
        name: "ara-real04-ready",
        acts: &[laid(4)],
    },
    // Picking the plugin also asks for its editor.
    Shot {
        name: "ara-real05-open",
        acts: &[laid(5)],
    },
    // The plugin's window (its picture next to the shot).
    Shot {
        name: "ara-real06-window",
        acts: &[laid(6)],
    },
    // The window closed, the song saved and opened again: the clip has the plugin still.
    Shot {
        name: "ara-real07-reopened",
        acts: &[laid(7)],
    },
    // Detached: the badge is gone.
    Shot {
        name: "ara-real08-detached",
        acts: &[laid(8)],
    },
];

fn plugin() -> Option<PathBuf> {
    std::env::var_os("XIRIUS_ARA_REAL_PLUGIN")
        .map(PathBuf::from)
        .filter(|p| p.exists())
}

/// The plugin's ARA entry, as the scan finds it: what the list shows and the clip keeps.
fn entry() -> Option<&'static Entry> {
    static ENTRY: OnceLock<Option<Entry>> = OnceLock::new();
    ENTRY
        .get_or_init(|| {
            let found = xirius_plugins::probe(&plugin()?).ok()?;
            found.into_iter().find(|e| e.ara)
        })
        .as_ref()
}

fn uid() -> &'static str {
    if fixture() {
        FIXTURE_UID
    } else {
        entry().map_or("", |e| e.uid.as_str())
    }
}

fn name() -> &'static str {
    if fixture() {
        FIXTURE_NAME
    } else {
        entry().map_or("", |e| e.name.as_str())
    }
}

/// Where the run keeps its song.
fn dir() -> PathBuf {
    std::env::temp_dir().join("xirius-ara-real-shots")
}

fn title() -> String {
    format!("{} · ARA", name())
}

fn clip(s: &Studio) -> Option<ClipId> {
    let p = s.project();
    let t = p.tracks.values().find(|t| t.name == TRACK)?;
    t.clips.first().map(|c| c.id)
}

fn report(s: &Studio) -> Option<Report> {
    s.ara.reports.iter().find(|r| r.plugin == uid()).cloned()
}

fn badge(s: &Studio) -> String {
    match clip(s).and_then(|c| s.ara_badge(c)) {
        None => "none".into(),
        Some(Badge::Busy(n)) => format!("busy:{}", n.map_or(String::new(), |n| n.to_string())),
        Some(Badge::Ready) => "ready".into(),
        Some(Badge::Crashed) => "crashed".into(),
        Some(Badge::Failed(why)) => format!("failed:{why}"),
    }
}

/// Ticks the screen as its frames would, reading the plugin's state, until `done`.
fn wait(s: &mut Studio, secs: f32, done: impl Fn(&Studio) -> bool) {
    run_for(s, secs, |s| {
        stranger();
        done(s)
    });
}

fn assert_ready_repaints(s: &mut Studio) {
    assert_eq!(badge(s), "ready");
    // Hold the last busy badge on the UI side while the real host is already
    // idle. The poll consuming Ready must request a frame of its own.
    let key = clip(s).unwrap().0;
    let report = s
        .ara
        .reports
        .iter_mut()
        .find(|r| r.plugin == uid())
        .unwrap();
    let (_, phase) = report.phases.iter_mut().find(|(id, _)| *id == key).unwrap();
    *phase = Phase::Waiting;
    report.progress = 0.0;
    assert_eq!(badge(s), "busy:0");
    assert!(
        s.poll_ara(),
        "the final ARA status did not request a repaint"
    );
    assert_eq!(badge(s), "ready");
    eprintln!("ara-check ready_repaints=true");
}

/// Ends the run when a window of this process shows that is neither the app's nor the plugin's
/// editor: the plugin asking for someone. Nothing is clicked or typed; the window goes with
/// the process.
fn stranger() {
    if let Some(w) = os::other_window(&[crate::screen::WINDOW_TITLE, title().as_str()]) {
        eprintln!("ara-check stopped window={w}");
        std::process::exit(3);
    }
}

fn tone() -> Vec<f32> {
    let n = (f64::from(RATE) * SECONDS) as usize;
    (0..n)
        .map(|i| {
            let t = i as f64 / f64::from(RATE);
            // A held note that steps up a tone each second: something for the plugin to find.
            let f = 220.0 * 2f64.powf((t.floor() * 2.0) / 12.0);
            (0.3 * (std::f64::consts::TAU * f * t).sin()) as f32
        })
        .collect()
}

/// The plugin documents the host reports: whether each is this plugin's, open in a window,
/// down or not opened.
fn docs(s: &Studio) -> Vec<String> {
    s.ara
        .reports
        .iter()
        .map(|r| {
            format!(
                "{}:editing={},crashed={:?},failed={:?}",
                r.plugin == uid(),
                r.editing,
                r.crashed,
                r.failed
            )
        })
        .collect()
}

fn play(s: &mut Studio, g: &Geometry, n: u8) {
    if n == 0 {
        s.drawer_open = false;
        s.mixer.scan.auto = false;
        if fixture() {
            #[cfg(all(windows, debug_assertions))]
            xirius_session::ara::register_native_fixture(FIXTURE_UID, FIXTURE_NAME);
            s.use_plugins(Catalog::default());
        } else {
            let entry = entry().expect("XIRIUS_ARA_REAL_PLUGIN names no ARA plugin");
            // Preparation and reopening read the catalogue from disk too. Keep this fixture
            // in the isolated profile, so the run never depends on the user's scan cache.
            xirius_plugins::CacheFile {
                bundles: vec![xirius_plugins::BundleRecord {
                    path: entry.path.clone(),
                    fingerprint: xirius_plugins::fingerprint(&entry.path),
                    entries: vec![entry.clone()],
                    failure: None,
                    probe_ms: 0,
                }],
                ..Default::default()
            }
            .write(s.song.plugin_cache())
            .unwrap();
            s.use_plugins(Catalog {
                entries: vec![entry.clone()],
                failures: Vec::new(),
            });
        }
        let dir = dir();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("tone.wav");
        let v = tone();
        xirius_session::export::write_wav(&wav, &v, &v, RATE, 24).unwrap();
        // The sample's own tracks go: their sounds stay where the sample is, not in the song's
        // new folder.
        let ids: Vec<String> = s
            .project()
            .tracks
            .values()
            .filter(|t| t.kind != xirius_core::model::TrackKind::Master)
            .map(|t| t.id.to_string())
            .collect();
        s.song.run("track.delete", json!({ "ids": ids })).unwrap();
        // The window's song is saved by its owner (the save on the worker), as the folder
        // dialog's answer would.
        s.start_file_request(crate::studio::FileRequest::SaveAs, &dir.join("song"));
        wait(s, 30.0, |s| s.saving.is_none());
        assert!(
            s.song.dir().is_some(),
            "the song was not saved: {:?}",
            s.error
        );
        // Four bars at 120: the tone's eight seconds.
        run_ok(
            s,
            "clip.import.place",
            json!({ "file": wav.to_string_lossy(), "length": 4 * 4 * 960, "name": TRACK }),
        );
        let g = &s.geometry(g.size.0, g.size.1);
        s.resized(g);
        // The player a song gets when it is opened (ENG-064); the plugin runs in it.
        s.prepare_song();
        wait(s, 60.0, |s| s.transport.host().is_some());
        eprintln!("ara-check prepared host={}", s.transport.host().is_some());
        assert!(
            s.transport.host().is_some(),
            "the song's player did not start"
        );
        return;
    }
    let g = &s.geometry(g.size.0, g.size.1);
    let Some(c) = clip(s) else {
        panic!("ara-real stage={n}: the song has no clip");
    };
    let shot = SHOTS[usize::from(n) - 1].name;
    match n {
        1 => {
            s.popup = None;
            s.ctx = None;
            let (x, y) = clip_mid(s, g, TRACK, 0);
            assert!(s.scene_context(g, x, y));
            assert!(s.context_choose(g, crate::context::layout::Category::Audio.label()));
            assert!(
                s.context_rows()
                    .iter()
                    .any(|row| row.label == crate::clip_ara::ADD && row.enabled),
                "the audio menu has no enabled ARA add row"
            );
        }
        2 => {
            assert!(s.context_choose(g, crate::clip_ara::ADD));
            let picking = s
                .popup
                .as_ref()
                .is_some_and(|p| p.kind == crate::popup::PopupKind::AraPlugin);
            eprintln!("ara-check {shot} picking={picking}");
            assert!(picking, "the list of plugins did not open");
        }
        3 => {
            s.typed(g, "enter", None);
            let attached = s.project().ara.clips.contains_key(&c);
            wait(s, 60.0, |s| {
                report(s).is_some_and(|r| r.phases.iter().any(|(_, p)| *p != Phase::Waiting))
                    || badge(s).starts_with("busy:")
                    || badge(s) == "ready"
            });
            eprintln!(
                "ara-check {shot} attached={attached} list_closed={} badge={} host={} error={:?}",
                s.popup.is_none(),
                badge(s),
                s.transport.host().is_some(),
                s.error
            );
            assert!(attached && s.popup.is_none(), "{:?}", s.error);
        }
        4 => {
            wait(s, 180.0, |s| badge(s) == "ready");
            eprintln!("ara-check {shot} badge={}", badge(s));
            plugin_status(s, shot);
            assert_eq!(badge(s), "ready");
        }
        5 => {
            eprintln!("ara-check {shot} docs={:?}", docs(s));
        }
        6 => {
            wait(s, 30.0, |s| s.ara.editing.iter().any(|p| p == uid()));
            let editing = s.ara.editing.iter().any(|p| p == uid());
            let hwnd = editor_window();
            let (mut offscreen, mut front) = (false, false);
            if let Some(h) = hwnd {
                let edge = xirius_vst3::Placement::offscreen().at.map_or(0, |(x, _)| x);
                offscreen = crate::park::origin(h).is_some_and(|(x, _)| x >= edge);
                front = crate::park::is_foreground(h);
                let pic = dir().join(format!("{shot}-editor.bmp"));
                let taken = xirius_vst3::capture_window(h, &pic);
                eprintln!(
                    "ara-check {shot} picture={} colors={:?}",
                    pic.display(),
                    taken.map(|c| c.distinct_colors)
                );
            }
            eprintln!(
                "ara-check {shot} editing={editing} window={} offscreen={offscreen} foreground={front}",
                hwnd.is_some()
            );
            eprintln!("ara-check {shot} docs={:?} error={:?}", docs(s), s.error);
            plugin_status(s, shot);
            assert!(editing && hwnd.is_some(), "the window did not open");
            assert!(
                offscreen && !front,
                "the window came on a screen or to the front"
            );
            #[cfg(windows)]
            {
                let h = hwnd.unwrap();
                let main = crate::park::own_handle(crate::screen::WINDOW_TITLE).unwrap();
                assert!(os::independent(h, main));
                os::sink(h);
                wait(s, 5.0, |_| os::above(main, h));
                assert!(os::above(main, h), "the editor was not below Studio");
                s.raise_editors_over(main as u64);
                wait(s, 5.0, |_| os::above(h, main));
                assert!(
                    os::above(h, main),
                    "Studio did not restore its independent editor"
                );
                os::sink(h);
                wait(s, 5.0, |_| os::above(main, h));
                assert!(os::above(main, h));
                assert!(s.open_ara_editor_for(&[c]));
                wait(s, 5.0, |_| os::above(h, main));
                assert!(
                    os::above(h, main),
                    "selecting the clip did not raise the editor"
                );
                assert!(!crate::park::is_foreground(h));
                eprintln!("ara-check {shot} independent_raise=true");
            }
        }
        7 => {
            #[cfg(all(windows, debug_assertions))]
            if fixture() {
                run_ok(s, "project.save", json!({}));
                wait(s, 30.0, |s| s.saving.is_none());
                wait(s, 10.0, |s| s.save_mark() != crate::keep::SaveMark::Saving);
                assert_eq!(s.save_mark(), crate::keep::SaveMark::Saved);
                let archived = s.project().ara.archives.clone();
                assert!(s.transport.host().unwrap().edit_ara_fixture(uid(), c));
                wait(s, 10.0, |s| s.project().ara.archives != archived);
                assert_ne!(
                    s.project().ara.archives,
                    archived,
                    "the host did not collect the edit"
                );
                assert_eq!(
                    s.save_mark(),
                    crate::keep::SaveMark::Unsaved,
                    "ARA edit did not mark the song unsaved"
                );
                eprintln!("ara-check {shot} edit_unsaved=true");
            }
            let h = editor_window().expect("the editor is gone");
            os::close(h);
            wait(s, 30.0, |s| !s.ara.editing.iter().any(|p| p == uid()));
            let closed = !s.ara.editing.iter().any(|p| p == uid());
            // Save through the same path as a real edit, waiting for the plugin's archive.
            let pending = |s: &Studio| s.transport.host().is_some_and(|h| h.take_ara_arrived());
            wait(s, 10.0, |s| !pending(s));
            assert!(!pending(s), "the plugin's archive asked for did not come");
            run_ok(s, "project.save", json!({}));
            wait(s, 30.0, |s| s.song.is_saved());
            let saved = s.song.is_saved() && s.error.is_none();
            assert!(saved, "the song was not written: {:?}", s.error);
            let archives = s.project().ara.archives.clone();
            let archived = archives.len();
            // Empty and empty would compare equal: the plugin's archive must be in the song.
            assert!(
                archives.contains_key(uid()),
                "the song was written without the plugin's archive ({archived} archives)"
            );
            let song = dir().join("song");
            assert!(s.close_to_home());
            wait(s, 30.0, Studio::home_shown);
            assert!(s.home_shown(), "the song did not close: {:?}", s.error);
            run_ok(s, "project.open", json!({ "path": song.to_string_lossy() }));
            wait(s, 30.0, |s| s.opening.is_none() && clip(s).is_some());
            let opened = s.opening.is_none() && s.error.is_none();
            let back = clip(s).is_some_and(|c| s.project().ara.clips.contains_key(&c));
            let kept = s.project().ara.archives.len();
            assert!(
                s.project().ara.archives == archives,
                "the plugin's archive changed through saving and opening"
            );
            wait(s, 180.0, |s| badge(s) == "ready");
            eprintln!(
                "ara-check {shot} closed={closed} archives={archived} saved={saved} opened={opened} attached={back} archives_after={kept} badge={}",
                badge(s)
            );
            eprintln!("ara-check {shot} docs={:?} error={:?}", docs(s), s.error);
            assert!(closed && saved && opened && back, "{:?}", s.error);
            assert_eq!(badge(s), "ready");
            assert_ready_repaints(s);
        }
        _ => {
            s.popup = None;
            s.ctx = None;
            s.select_clip_here(track(s, TRACK), c);
            s.mapped_key(
                g,
                "m",
                None,
                Mods {
                    ctrl: true,
                    shift: false,
                    alt: true,
                },
            );
            wait(s, 10.0, |s| s.project().ara.clips.is_empty());
            eprintln!(
                "ara-check {shot} detached={} badge={}",
                s.project().ara.clips.is_empty(),
                badge(s)
            );
            assert!(s.project().ara.clips.is_empty(), "the clip did not let go");
            run_ok(s, "history.undo", json!({}));
            wait(s, 30.0, |s| s.saving.is_none() && badge(s) == "ready");
            assert_eq!(badge(s), "ready", "idle after undo: {:?}", docs(s));
            assert_ready_repaints(s);
            wait(s, 10.0, |s| s.save_mark() != crate::keep::SaveMark::Saving);
            assert_eq!(
                s.save_mark(),
                crate::keep::SaveMark::Saved,
                "undo returns to the saved state"
            );
            eprintln!("ara-check {shot} undo_ready=true");
            run_ok(s, "history.redo", json!({}));
            wait(s, 10.0, |s| s.project().ara.clips.is_empty());
            assert!(s.project().ara.clips.is_empty());
            wait(s, 10.0, |s| s.save_mark() != crate::keep::SaveMark::Saving);
            assert_eq!(s.save_mark(), crate::keep::SaveMark::Unsaved);
            eprintln!("ara-check {shot} detach_unsaved=true");
            assert!(s.close_to_home());
            wait(s, 10.0, |s| s.unsaved.is_some());
            assert!(s.unsaved.is_some(), "closing did not ask to save");
            s.answer_unsaved(false);
            assert!(s.opening.is_none() && !s.home_shown());
            assert!(s.close_to_home());
            wait(s, 10.0, |s| s.unsaved.is_some());
            assert!(s.unsaved.is_some());
            s.answer_unsaved(true);
            wait(s, 30.0, Studio::home_shown);
            assert!(
                s.home_shown(),
                "the song did not close after saving: {:?}",
                s.error
            );
            eprintln!("ara-check {shot} close_prompt=true");
        }
    }
}

fn editor_window() -> Option<isize> {
    let title = title();
    crate::park::any_window(&title).filter(|h| crate::park::own_handle(&title) != Some(*h))
}

fn plugin_status(s: &mut Studio, shot: &str) {
    let status = xirius_session::commands::run(
        s.song.session_mut(),
        s.transport.host_mut(),
        "plugin.status",
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    eprintln!("ara-check {shot} plugin.status={}", status.data);
}

#[cfg(windows)]
mod os {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GW_HWNDPREV, GW_OWNER, GetClassNameW, GetWindow, GetWindowTextW,
        GetWindowThreadProcessId, HWND_BOTTOM, IsWindowVisible, PostMessageW, SWP_ASYNCWINDOWPOS,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos, WM_CLOSE,
    };

    struct Find<'a> {
        known: &'a [&'a str],
        found: Option<String>,
    }

    /// A shown window of this process, titled or a dialog, that is none of `known`.
    pub fn other_window(known: &[&str]) -> Option<String> {
        unsafe extern "system" fn each(hwnd: HWND, data: LPARAM) -> i32 {
            // SAFETY: `hwnd` comes from EnumWindows; `data` is the `Find` below, alive across it.
            unsafe {
                let find = &mut *(data as *mut Find);
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, &mut pid);
                if pid != std::process::id() || IsWindowVisible(hwnd) == 0 {
                    return 1;
                }
                let mut class = [0u16; 256];
                let mut text = [0u16; 256];
                let c = GetClassNameW(hwnd, class.as_mut_ptr(), 256).max(0) as usize;
                let t = GetWindowTextW(hwnd, text.as_mut_ptr(), 256).max(0) as usize;
                let class = String::from_utf16_lossy(&class[..c]);
                let text = String::from_utf16_lossy(&text[..t]);
                if find.known.contains(&text.as_str()) || (text.is_empty() && class != "#32770") {
                    return 1;
                }
                find.found = Some(format!("class {class:?}, title {text:?}"));
            }
            0
        }
        let mut find = Find { known, found: None };
        // SAFETY: the callback reads `find` only during the call.
        unsafe { EnumWindows(Some(each), &mut find as *mut Find as LPARAM) };
        find.found
    }

    /// Asks the window to close, as its close box does.
    pub fn close(hwnd: isize) {
        // SAFETY: a message posted to the plugin host's editor window.
        unsafe { PostMessageW(hwnd as HWND, WM_CLOSE, 0, 0) };
    }

    pub fn independent(hwnd: isize, main: isize) -> bool {
        // SAFETY: queries the editor's owner and the two windows' process ids.
        unsafe {
            let mut editor_pid = 0;
            let mut main_pid = 0;
            GetWindowThreadProcessId(hwnd as HWND, &mut editor_pid);
            GetWindowThreadProcessId(main as HWND, &mut main_pid);
            editor_pid != 0
                && main_pid != 0
                && editor_pid != main_pid
                && GetWindow(hwnd as HWND, GW_OWNER).is_null()
        }
    }

    pub fn sink(hwnd: isize) {
        // SAFETY: only changes the parked editor's order, asynchronously and without focus.
        unsafe {
            SetWindowPos(
                hwnd as HWND,
                HWND_BOTTOM,
                0,
                0,
                0,
                0,
                SWP_ASYNCWINDOWPOS | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
            )
        };
    }

    pub fn above(hwnd: isize, base: isize) -> bool {
        let mut cur = base as HWND;
        for _ in 0..4096 {
            // SAFETY: only reads the current z-order.
            cur = unsafe { GetWindow(cur, GW_HWNDPREV) };
            if cur.is_null() {
                break;
            }
            if cur as isize == hwnd {
                return true;
            }
        }
        false
    }
}

#[cfg(not(windows))]
mod os {
    pub fn other_window(_: &[&str]) -> Option<String> {
        None
    }

    pub fn close(_: isize) {}
}
