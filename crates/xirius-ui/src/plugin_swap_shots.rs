//! `--script plugin-swap`: a plugin's window stays open while its instance is swapped
//! (ENG-066 to ENG-071). The test VST3 runs in a plugin host process of its own, plays on Vocal,
//! and its window is opened as a person opens it, from its row in the mixer. Then, with the
//! window open, the value is moved in the window (which changes the plugin's latency), a track
//! is added, the plugin is moved to the shared host, into the app's process and back to a host
//! of its own: each makes a new mix with a new instance. After each the window must show the
//! new instance, with the value moved in it. Last, the window is closed while a new mix that
//! moves the plugin is on its way, once to the shared host and once into the app: it must not
//! open again when the new mix plays.
//!
//! The test VST3's window says which instance it is: `delay N (x.xx)  pid P  instance K`. A
//! stage reads that label from the window, takes the window's picture next to the shot as
//! `<shot>-editor.bmp`, and writes `plugin-check <shot> …` to stderr
//! (`tests/plugin_swap_window.rs` runs it), with `plugin-time <shot> …`: how long the window
//! took to open again, from the command. A stage panics when the window does not follow.
//!
//! With [`REAL_ENV`] set to a VST3 bundle, the same scenes run with that plugin instead (its
//! first class), told apart by the window's process and handle, as it has no label; the value
//! moved in the window does not change its latency, so that scene only moves it.

use std::cell::Cell;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::json;
use xirius_engine::server::DEFAULT_LATENCY;

use crate::plugin_reload_shots::{id, prepare, ready};
use crate::shot::{Act, Part, Play, Shot, Target};
use crate::studio::Studio;
use crate::topbar::BarButton;

/// A VST3 bundle to run the scenes with, instead of the test VST3.
pub const REAL_ENV: &str = "XIRIUS_SWAP_VST3";

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "swap01-open",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Studio(setup), 0),
            Act::Press(Target::Top(BarButton::Play)),
            Act::Stage(Play::Studio(stage), 0),
            Act::Press(Target::Strip(Part::Insert, "Vocal")),
            Act::Stage(Play::Studio(stage), 1),
        ],
    },
    Shot {
        name: "swap02-latency",
        acts: &[Act::Stage(Play::Studio(stage), 2)],
    },
    Shot {
        name: "swap03-track-added",
        acts: &[Act::Stage(Play::Studio(stage), 3)],
    },
    Shot {
        name: "swap04-shared",
        acts: &[Act::Stage(Play::Studio(stage), 4)],
    },
    Shot {
        name: "swap05-inprocess",
        acts: &[Act::Stage(Play::Studio(stage), 5)],
    },
    Shot {
        name: "swap06-dedicated",
        acts: &[Act::Stage(Play::Studio(stage), 6)],
    },
    Shot {
        name: "swap07-closed-shared",
        acts: &[Act::Stage(Play::Studio(stage), 7)],
    },
    Shot {
        name: "swap08-reopened",
        acts: &[
            Act::Press(Target::Strip(Part::Insert, "Vocal")),
            Act::Stage(Play::Studio(stage), 8),
        ],
    },
    Shot {
        name: "swap09-closed-inprocess",
        acts: &[
            Act::Stage(Play::Studio(stage), 9),
            Act::Press(Target::Top(BarButton::Stop)),
        ],
    },
];

/// How long a swap and the window that follows it may take.
const WAIT: Duration = Duration::from_secs(20);

/// The value moved in the window: the delay goes from 192 to 128 samples.
const MOVED: f64 = 0.5;

/// What the test VST3's window says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Label {
    delay: u32,
    pid: u32,
    instance: u32,
}

impl Label {
    fn parse(text: &str) -> Option<Label> {
        let w: Vec<&str> = text.split_whitespace().collect();
        match w.as_slice() {
            ["delay", d, _, "pid", p, "instance", i] => Some(Label {
                delay: d.parse().ok()?,
                pid: p.parse().ok()?,
                instance: i.parse().ok()?,
            }),
            _ => None,
        }
    }
}

/// The plugin's window as a stage saw it.
#[derive(Clone, Copy, Debug)]
struct Seen {
    hwnd: isize,
    /// The process the window is in.
    pid: u32,
    /// The test VST3's label; `None` for another plugin.
    label: Option<Label>,
}

thread_local! {
    /// The window the last stage saw.
    static SEEN: Cell<Option<Seen>> = const { Cell::new(None) };
}

/// The bundle of [`REAL_ENV`], when the scenes run with another plugin.
fn real() -> Option<PathBuf> {
    std::env::var_os(REAL_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Vocal's plugin, in a host of its own: the test VST3 with its delay at 192, or the plugin of
/// [`REAL_ENV`].
fn setup(s: &mut Studio, n: u8) {
    let Some(bundle) = real() else {
        return prepare(s, n);
    };
    let entries = xirius_plugins::probe(&bundle)
        .unwrap_or_else(|e| panic!("{}: not a VST3 plugin: {e}", bundle.display()));
    let entry = entries
        .first()
        .unwrap_or_else(|| panic!("{}: no plugin in it", bundle.display()))
        .clone();
    eprintln!(
        "plugin-check real name={:?} bundle={}",
        entry.name,
        bundle.display()
    );
    let track = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Vocal")
        .unwrap()
        .id;
    let prior = s.project().tracks[&track]
        .inserts
        .iter()
        .map(|p| p.id.to_string())
        .collect::<Vec<_>>();
    if !prior.is_empty() {
        assert!(s.run_command("plugin.remove", json!({"ids":prior})));
    }
    assert!(s.run_command(
        "plugin.add",
        json!({
            "track":track.to_string(), "name":entry.name, "kind":"effect",
            "format":"vst3", "uid":entry.uid
        })
    ));
    assert!(s.run_command(
        "plugin.isolation.set",
        json!({"ids":[id(s).to_string()], "mode":"dedicated"})
    ));
    s.use_plugins(xirius_plugins::Catalog {
        entries,
        failures: Vec::new(),
    });
    s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
    s.drawer_open = true;
    s.tab = crate::studio::DrawerTab::Mixer;
}

/// The plugin plays in its host.
fn ready_real(s: &mut Studio) {
    let id = id(s);
    let up = |s: &Studio| {
        s.transport.host().is_some_and(|h| {
            h.plugins().link(id).is_some_and(|l| l.opened().is_some())
                && h.plugins()
                    .reports()
                    .iter()
                    .any(|r| r.id == id && !r.status.crashed && r.status.error.is_none())
        })
    };
    crate::shot::run_for(s, 30.0, up);
    assert!(s.error.is_none(), "{:?}", s.error);
    assert!(up(s), "the plugin does not play in its host");
}

fn stage(s: &mut Studio, n: u8) {
    let id = id(s);
    let app = std::process::id();
    let real = real().is_some();
    let before = SEEN.get();
    match n {
        0 if real => ready_real(s),
        0 => ready(s),
        1 => {
            let seen = follow(s, "swap01-open", Instant::now(), |w| {
                w.pid != app && (real || w.label.is_some_and(|l| l.delay == 192))
            });
            check("swap01-open", "opened", &seen);
        }
        2 if real => {
            // A value moved in the window, as the plugin's own edit: the plugin's state is taken
            // into the song (the song names none of its values), and the window stays open.
            let host = s.transport.host().unwrap();
            let link = host.plugins().link(id).unwrap();
            let param = link.opened().unwrap().params.first().map(|(p, _)| *p);
            let Some(param) = param else {
                eprintln!("plugin-check swap02-latency skipped: the plugin has no parameters");
                return;
            };
            let was = state(s);
            host.plugins()
                .server_of(id)
                .unwrap()
                .simulate_edit(&link, param, &[0.37]);
            crate::shot::run_for(s, WAIT.as_secs_f32(), |s| state(s) != was);
            assert!(state(s) != was, "the moved value is in the song");
            let before = before.unwrap();
            let seen = follow(s, "swap02-latency", Instant::now(), |w| w.pid == before.pid);
            eprintln!(
                "plugin-check swap02-latency state-taken same-window={}",
                seen.hwnd == before.hwnd
            );
            check("swap02-latency", "moved", &seen);
        }
        2 => {
            // As the window does when its value is moved: the plugin's own edit.
            let host = s.transport.host().unwrap();
            let link = host.plugins().link(id).unwrap();
            host.plugins()
                .server_of(id)
                .unwrap()
                .simulate_edit(&link, 1, &[MOVED]);
            let at = Instant::now();
            let before = before.and_then(|b| b.label).unwrap();
            crate::shot::run_for(s, WAIT.as_secs_f32(), |s| {
                value(s, "1") == Some(MOVED) && latency(s) == Some(DEFAULT_LATENCY + 128)
            });
            assert_eq!(value(s, "1"), Some(MOVED), "the moved value is in the song");
            assert_eq!(latency(s), Some(DEFAULT_LATENCY + 128));
            let seen = follow(s, "swap02-latency", at, |w| {
                w.label
                    .is_some_and(|l| l.delay == 128 && l.instance != before.instance)
            });
            check("swap02-latency", "swapped", &seen);
        }
        3 => {
            let at = Instant::now();
            assert!(s.run_command("track.add.audio", json!({"name":"Swap"})));
            let before = before.unwrap();
            // The test VST3's new window is the one whose label names the new instance: a
            // window found before it has drawn its label is not taken for it.
            let seen = follow(s, "swap03-track-added", at, |w| {
                w.pid == before.pid
                    && match before.label.filter(|_| !real) {
                        Some(b) => w
                            .label
                            .is_some_and(|l| l.delay == 128 && l.instance != b.instance),
                        None => w.hwnd != before.hwnd,
                    }
            });
            if !real {
                assert_eq!(value(s, "1"), Some(MOVED));
            }
            check("swap03-track-added", "swapped", &seen);
        }
        4..=6 => {
            let (shot, mode) = match n {
                4 => ("swap04-shared", "shared"),
                5 => ("swap05-inprocess", "inprocess"),
                _ => ("swap06-dedicated", "dedicated"),
            };
            let before = before.unwrap();
            let at = isolate(s, mode);
            // Until the new mix plays, the window is the old instance's, and is open.
            assert!(s.editor_shown(id), "{shot}: the window counts as open");
            let seen = follow(s, shot, at, |w| {
                let moved = match n {
                    5 => w.pid == app,
                    _ => w.pid != app && w.pid != before.pid,
                };
                moved && (real || w.label.is_some_and(|l| l.delay == 128))
            });
            if !real {
                assert_eq!(value(s, "1"), Some(MOVED));
                assert_eq!(seen.label.unwrap().pid, seen.pid, "{shot}");
            }
            check(shot, "moved", &seen);
        }
        7 => closed(s, "swap07-closed-shared", "shared"),
        8 => {
            let seen = follow(s, "swap08-reopened", Instant::now(), |w| w.pid != app);
            check("swap08-reopened", "opened", &seen);
        }
        9 => closed(s, "swap09-closed-inprocess", "inprocess"),
        _ => unreachable!(),
    }
}

/// Moves the plugin to `mode`; when it was asked.
fn isolate(s: &mut Studio, mode: &str) -> Instant {
    let at = Instant::now();
    assert!(s.run_command(
        "plugin.isolation.set",
        json!({"ids":[id(s).to_string()], "mode":mode})
    ));
    assert!(
        s.transport.host().is_some_and(|h| h.swap_pending()),
        "a new mix is on its way"
    );
    at
}

/// The plugin is moved to `mode` and, while the new mix is on its way, the window is closed in
/// its host as a person closes it: it does not open again when the new mix plays.
fn closed(s: &mut Studio, shot: &str, mode: &str) {
    let id = id(s);
    let before = SEEN.get().unwrap();
    assert_eq!(s.mixer.editors.len(), 1, "{shot}: the window is open");
    // Until its host says the window is open, the screen takes a window it asked for as open,
    // whatever the host says next.
    let t = Instant::now();
    while !s.host_said_editor_open(id) {
        assert!(
            t.elapsed() < WAIT,
            "{shot}: the host never said the window is open"
        );
        s.tick();
        std::thread::sleep(Duration::from_millis(10));
    }
    // The instance that plays now, to know when its host has let go of it.
    let (old_server, old_link) = s
        .transport
        .host()
        .and_then(|h| Some((h.plugins().server_of(id)?.clone(), h.plugins().link(id)?)))
        .expect("it plays in a host");
    let _ = isolate(s, mode);
    assert!(
        close_window(before.hwnd),
        "{shot}: the window took no close"
    );
    // The close came while the new mix was on its way.
    let pending = s.transport.host().is_some_and(|h| h.swap_pending());
    let mut noticed = false;
    let t = Instant::now();
    while t.elapsed() < WAIT
        && (s.transport.host().is_some_and(|h| h.swap_pending()) || !s.mixer.editors.is_empty())
    {
        s.tick();
        noticed |=
            s.mixer.editors.is_empty() && s.transport.host().is_some_and(|h| h.swap_pending());
        std::thread::sleep(Duration::from_millis(10));
    }
    let still = s.transport.host().is_some_and(|h| h.swap_pending());
    assert!(!still, "{shot}: the new mix never came in");
    // It does not open again on the new instance, in its host or in the app: watched until the
    // old host has let go of the instance that played (all it said about its window is in by
    // then), and for the frame that follows.
    let title = title(s);
    let mut opened = false;
    let t = Instant::now();
    loop {
        let gone = old_server.has_let_go(&old_link);
        s.tick();
        opened |= !s.mixer.editors.is_empty() || crate::park::any_window(&title).is_some();
        if gone {
            break;
        }
        assert!(t.elapsed() < WAIT, "{shot}: the old host never let go");
        std::thread::sleep(Duration::from_millis(10));
    }
    s.tick();
    opened |= !s.mixer.editors.is_empty() || crate::park::any_window(&title).is_some();
    let new_open = s.transport.host().is_some_and(|h| {
        h.plugin_statuses()
            .iter()
            .any(|(other, _, st)| *other == id && st.editor_open)
    });
    opened |= new_open;
    let placement = s
        .transport
        .host()
        .and_then(|h| h.plugins().placement_of(id));
    eprintln!(
        "plugin-check {shot} closed pending={pending} noticed={noticed} reopened={opened} placement={placement:?}"
    );
    assert!(!opened, "{shot}: the window opened again");
    assert!(
        pending,
        "{shot}: the window was not closed while the new mix was on its way"
    );
    assert!(!s.editor_shown(id), "{shot}");
}

fn check(shot: &str, what: &str, w: &Seen) {
    match w.label {
        Some(l) => eprintln!(
            "plugin-check {shot} {what} delay={} pid={} instance={} window-pid={}",
            l.delay, l.pid, l.instance, w.pid
        ),
        None => eprintln!(
            "plugin-check {shot} {what} window-pid={} hwnd={:#x}",
            w.pid, w.hwnd
        ),
    }
}

/// The value of the plugin's parameter `key` in the song.
fn value(s: &Studio, key: &str) -> Option<f64> {
    let id = id(s);
    s.project()
        .tracks
        .values()
        .flat_map(|t| &t.inserts)
        .find(|p| p.id == id)
        .and_then(|p| p.params.get(key).copied())
}

/// The plugin's state in the song.
fn state(s: &Studio) -> Option<String> {
    let id = id(s);
    s.project()
        .tracks
        .values()
        .flat_map(|t| &t.inserts)
        .find(|p| p.id == id)
        .and_then(|p| p.state.clone())
}

/// The latency the playing instance reports.
fn latency(s: &Studio) -> Option<u32> {
    let id = id(s);
    s.transport
        .host()?
        .plugins()
        .reports()
        .into_iter()
        .find(|r| r.id == id)
        .map(|r| r.status.latency)
}

/// The title of the plugin's window.
fn title(s: &Studio) -> String {
    let id = id(s);
    let p = s.project();
    let x = p
        .tracks
        .values()
        .flat_map(|t| &t.inserts)
        .find(|p| p.id == id)
        .unwrap();
    s.mixer
        .editors
        .iter()
        .find(|e| e.id == id)
        .map(|e| e.title.clone())
        .unwrap_or_else(|| format!("{} · Vocal", x.name))
}

/// Waits, ticking the studio, until the plugin's window is open and `done`; then takes its
/// picture and checks that it is the studio's one window, off every screen and not in front.
/// `at` is when the window was asked to change: how long it took goes to stderr.
fn follow(s: &mut Studio, shot: &str, at: Instant, done: impl Fn(&Seen) -> bool) -> Seen {
    let id = id(s);
    let mut last = None;
    let found = loop {
        s.tick();
        let title = s
            .mixer
            .editors
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.title.clone());
        let seen = title
            .as_deref()
            .and_then(crate::park::any_window)
            .map(|hwnd| Seen {
                hwnd,
                pid: window_pid(hwnd),
                label: label_of(hwnd),
            });
        last = seen.or(last);
        if let Some(seen) = seen
            && done(&seen)
        {
            break Some(seen);
        }
        if at.elapsed() > WAIT {
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let Some(seen) = found else {
        panic!(
            "{shot}: the window did not follow; saw {last:?}, editors {}, shown {}",
            s.mixer.editors.len(),
            s.editor_shown(id)
        );
    };
    let found_ms = at.elapsed().as_millis();
    let hwnd = seen.hwnd;
    assert!(s.editor_shown(id), "{shot}");
    assert_eq!(s.mixer.editors.len(), 1, "{shot}");
    assert!(!crate::park::is_foreground(hwnd), "{shot}: in front");
    let edge = xirius_vst3::Placement::offscreen().at.map_or(0, |(x, _)| x);
    assert!(
        crate::park::origin(hwnd).is_some_and(|(x, _)| x >= edge),
        "{shot}: on a screen"
    );
    let path = dir().join(format!("{shot}-editor.bmp"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let t = Instant::now();
    let mut taken = xirius_vst3::capture_window(hwnd, &path);
    while t.elapsed() < WAIT && taken.as_ref().is_ok_and(|c| c.distinct_colors <= 1) {
        s.tick();
        std::thread::sleep(Duration::from_millis(20));
        taken = xirius_vst3::capture_window(hwnd, &path);
    }
    let drawn_ms = at.elapsed().as_millis();
    let c = taken.unwrap_or_else(|e| panic!("{shot}: no picture: {e}"));
    assert!(c.distinct_colors > 1, "{shot}: a blank picture");
    eprintln!(
        "plugin-check {shot} editor={} width={} height={} colors={}",
        path.display(),
        c.width,
        c.height,
        c.distinct_colors
    );
    eprintln!("plugin-time {shot} found_ms={found_ms} drawn_ms={drawn_ms}");
    SEEN.set(Some(seen));
    seen
}

/// The shots' folder (`--shot`), where the windows' pictures go too.
fn dir() -> PathBuf {
    let args: Vec<_> = std::env::args().collect();
    args.windows(2)
        .find(|v| v[0] == "--shot")
        .map(|v| PathBuf::from(&v[1]))
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../target/plugin-swap-{}", std::process::id()))
        })
}

/// The process a window is in.
#[cfg(windows)]
fn window_pid(hwnd: isize) -> u32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    let mut pid = 0u32;
    // SAFETY: `pid` outlives the call; a gone window leaves it 0.
    unsafe { GetWindowThreadProcessId(hwnd as _, &mut pid) };
    pid
}

#[cfg(not(windows))]
fn window_pid(_: isize) -> u32 {
    0
}

/// Closes a window, as its close box does (a message to it, not a click). True when the window
/// took it.
#[cfg(windows)]
fn close_window(hwnd: isize) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_CLOSE,
    };
    let mut out = 0usize;
    // SAFETY: the message carries no pointers; another process's window is asked with a time
    // limit, so a hung host does not hang the app.
    unsafe { SendMessageTimeoutW(hwnd as _, WM_CLOSE, 0, 0, SMTO_ABORTIFHUNG, 2000, &mut out) != 0 }
}

#[cfg(not(windows))]
fn close_window(_: isize) -> bool {
    false
}

/// The label of the test VST3's window `frame`.
#[cfg(windows)]
fn label_of(frame: isize) -> Option<Label> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumChildWindows, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_GETTEXT,
    };
    unsafe extern "system" fn each(hwnd: HWND, found: LPARAM) -> i32 {
        let mut text = [0u16; 128];
        let mut len = 0usize;
        // SAFETY: the buffer outlives the call and its length is given; another process's
        // window is asked with a time limit, so a hung host does not hang the app.
        let answered = unsafe {
            SendMessageTimeoutW(
                hwnd,
                WM_GETTEXT,
                text.len(),
                text.as_mut_ptr() as LPARAM,
                SMTO_ABORTIFHUNG,
                500,
                &mut len,
            )
        };
        if answered == 0 {
            return 1;
        }
        let label = Label::parse(&String::from_utf16_lossy(&text[..len.min(text.len())]));
        match label {
            Some(l) => {
                // SAFETY: `found` is the `Option<Label>` of `label_of`, alive across the walk.
                unsafe { *(found as *mut Option<Label>) = Some(l) };
                0
            }
            None => 1,
        }
    }
    let mut found: Option<Label> = None;
    // SAFETY: the callback writes only through the pointer to `found`, which outlives the walk.
    unsafe {
        EnumChildWindows(
            frame as HWND,
            Some(each),
            &mut found as *mut Option<Label> as LPARAM,
        )
    };
    found
}

#[cfg(not(windows))]
fn label_of(_: isize) -> Option<Label> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_label_of_the_test_plugin_reads_back() {
        assert_eq!(
            Label::parse("delay 128 (0.50)  pid 4120  instance 3"),
            Some(Label {
                delay: 128,
                pid: 4120,
                instance: 3
            })
        );
        assert_eq!(Label::parse("size=400x300"), None);
    }
}
