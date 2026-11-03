//! `--script ara-real`: a real ARA plugin on a clip, as a person uses it (EDT-023, PLG-036,
//! PLG-055; the approved screens of 2026-11-03): attached from the clip's right-click menu and
//! the list, the badge while it analyses and when it is done, its window from a double click,
//! the window closed and the song saved and opened again with the plugin's edits, and detached.
//!
//! Needs `XIRIUS_ARA_REAL_PLUGIN` set to the plugin's `.vst3`; without it, or when it names no
//! ARA plugin, the run fails at its first stage (it never passes by skipping). Each stage fails
//! when it does not see what a person would. `tests/ara_real_window.rs` runs it (`python
//! scripts/ci_tests.py --plugins`, before a release). Run with `--park --silent` and
//! `XIRIUS_EDITOR_OFFSCREEN=1`: the app's window and the plugin's are off every screen, nothing
//! is played to a speaker. The song is a tone written into a folder of its own under the temp
//! folder; no song of the user's is opened.
//!
//! The plugin must not need anyone: a window of the app's process other than the app's and the
//! plugin's editor (an activation, a question) ends the run at once, untouched, and the run
//! says which window it was. Each stage writes what it saw to stderr as `ara-check <shot> …`.
//! The only input to a window is the close message sent to the plugin's own editor.

use std::path::PathBuf;
use std::sync::OnceLock;

use serde_json::json;
use xirius_core::id::ClipId;
use xirius_plugins::{Catalog, Entry};
use xirius_session::ara::{Phase, Report};

use crate::arrange::probe::{clip_mid, track};
use crate::clip_ara::Badge;
use crate::shot::{Act, Play, Shot, run_for};
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;

const TRACK: &str = "ARA";
const RATE: u32 = 48_000;
const SECONDS: f64 = 8.0;

const fn laid(n: u8) -> Act {
    Act::Stage(Play::Laid(play), n)
}

pub const SHOTS: &[Shot] = &[
    // 処理 › of the tone's clip, with the row that adds the plugin.
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
    // A double click on the clip. The stage only asks: the plugin's window is owned by the
    // app's, so it opens while the app's window takes its messages between the shots.
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
    entry().map_or("", |e| e.uid.as_str())
}

fn name() -> &'static str {
    entry().map_or("", |e| e.name.as_str())
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
    let Some(entry) = plugin().and(entry()) else {
        panic!("ara-real stage={n}: XIRIUS_ARA_REAL_PLUGIN names no ARA plugin");
    };
    if n == 0 {
        s.drawer_open = false;
        s.mixer.scan.auto = false;
        s.use_plugins(Catalog {
            entries: vec![entry.clone()],
            failures: Vec::new(),
        });
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
        let ok = s.run_command(
            "clip.import.place",
            json!({ "file": wav.to_string_lossy(), "length": 4 * 4 * 960, "name": TRACK }),
        );
        assert!(ok, "{:?}", s.error);
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
            assert!(s.context_choose(g, "処理"));
            let last = s.context_rows().into_iter().rfind(|r| !r.gap);
            eprintln!(
                "ara-check {shot} last={:?} key={:?}",
                last.as_ref().map(|r| r.label.clone()),
                last.as_ref().map(|r| r.key.clone())
            );
            assert_eq!(last.map(|r| r.label).as_deref(), Some(crate::clip_ara::ADD));
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
            assert_eq!(badge(s), "ready");
        }
        5 => {
            let (x, y) = clip_mid(s, g, TRACK, 0);
            let asked = s.arrange_open(g, x, y, Mods::NONE);
            eprintln!("ara-check {shot} asked={asked}");
            assert!(asked, "the double click did not ask for the window");
        }
        6 => {
            wait(s, 30.0, |s| s.ara.editing.iter().any(|p| p == uid()));
            let editing = s.ara.editing.iter().any(|p| p == uid());
            let hwnd = crate::park::own_handle(&title());
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
            assert!(editing && hwnd.is_some(), "the window did not open");
            assert!(
                offscreen && !front,
                "the window came on a screen or to the front"
            );
        }
        7 => {
            if let Some(h) = crate::park::own_handle(&title()) {
                os::close(h);
            }
            wait(s, 30.0, |s| !s.ara.editing.iter().any(|p| p == uid()));
            let closed = !s.ara.editing.iter().any(|p| p == uid());
            // The window's edits come into the song once the plugin wrote its archive
            // (`take_archives`, at a later frame). The script edits nothing in the plugin's
            // window (the plugin takes no edit from the host), so whether there is one is the
            // plugin's: what is checked is that the one asked for comes, and that the song
            // written keeps it byte for byte through opening it again.
            let pending = |s: &Studio| s.transport.host().is_some_and(|h| h.take_ara_arrived());
            wait(s, 10.0, |s| !pending(s));
            assert!(!pending(s), "the plugin's archive asked for did not come");
            let saved = s.run_command("project.save", json!({}));
            wait(s, 30.0, |s| s.song.is_saved());
            assert!(s.song.is_saved(), "the song was not written: {:?}", s.error);
            let archives = s.project().ara.archives.clone();
            let archived = archives.len();
            let song = dir().join("song");
            let opened = s.run_command("project.open", json!({ "path": song.to_string_lossy() }));
            wait(s, 30.0, |s| clip(s).is_some());
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
        }
    }
}

#[cfg(windows)]
mod os {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
        PostMessageW, WM_CLOSE,
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
        // SAFETY: a message posted to the plugin's own window, of this process.
        unsafe { PostMessageW(hwnd as HWND, WM_CLOSE, 0, 0) };
    }
}

#[cfg(not(windows))]
mod os {
    pub fn other_window(_: &[&str]) -> Option<String> {
        None
    }

    pub fn close(_: isize) {}
}
