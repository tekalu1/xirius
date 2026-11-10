//! UI-337: an independent window (a built-in device's, the channel editor's) moves by the empty
//! part of its bar. `--script device-drag` opens a built-in instrument's window and the detached
//! channel editor in the real app, parked and silent, and a thread asks each of them what
//! Windows asks a window: `WM_NCHITTEST` on the empty bar, on each button, on the content and on
//! the four edges. All of it is plain window messages to the windows themselves; no OS input is
//! sent and no window is brought to the front.
//!
//! GPUI answers a hit test from its last mouse position, so the thread asks the UI (once per
//! frame, from the gate [`ready`]) to move that window's mouse to the point first. It also checks
//!
//! - what the app does with a press on the empty bar: the first press goes on (the system
//!   moves the window), the second (the double click) is taken, so the system does not
//!   maximise;
//! - `WM_NCLBUTTONDBLCLK` there: the window is not maximised and keeps its size;
//! - a move by `SetWindowPos`: the position changes; a size by `SetWindowPos` (what the system's
//!   snap does) stays instead of being set back by the frame's own resize;
//! - closing and opening again: where the window comes back.
//!
//! A drag with the mouse is *not* exercised: that needs OS mouse input. The result is
//! `drag.json` and `drag.md` in the shots' folder, read by `tests/detached_window.rs`.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::*;
use crate::chrome::{HTBOTTOM, HTCAPTION, HTCLIENT, HTLEFT, HTRIGHT, HTTOP, ht_name};
use crate::shot::{Act, Play, Shot};
use gpui::{Modifiers, MouseDownEvent, MouseMoveEvent, PlatformInput, WindowHandle};
use serde_json::{Value, json};
use windows_sys::Win32::Foundation::{POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, IsZoomed, SC_RESTORE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SendMessageW, SetWindowPos, WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_SYSCOMMAND,
};

pub const SHOTS: &[Shot] = &[Shot {
    name: "drag-01-probe",
    acts: &[Act::Stage(Play::Window(stage), 0)],
}];

const TITLES: [&str; 2] = [
    "Xirius drag probe: built-in",
    "Xirius drag probe: channel editor",
];
const NAMES: [&str; 2] = ["built-in instrument", "channel editor"];

/// One point to ask `WM_NCHITTEST` about, in logical pixels from the window's top-left.
#[derive(Clone, Debug)]
struct Query {
    label: String,
    x: f32,
    y: f32,
    expect: isize,
}

/// What the UI tells the thread about a window.
#[derive(Clone, Debug)]
struct Win {
    scale: f32,
    queries: Vec<Query>,
    /// A point of the empty bar.
    head: (f32, f32),
}

/// What the thread asks of the UI.
enum Want {
    Layout,
    Mouse(usize, f32, f32),
    Press(usize, f32, f32, usize),
    Reopen(usize),
}

enum Ack {
    Layout(Vec<Win>),
    Done,
    Propagate(bool),
    /// The reopened window's title.
    Reopened(String),
}

enum Phase {
    /// Frames to wait for the windows to open and draw.
    Opening(u32),
    Running {
        asks: Receiver<Want>,
        acks: Sender<Ack>,
        done: Arc<AtomicBool>,
        job: Option<Reopen>,
        since: Instant,
    },
    Done,
}

/// A window being closed and opened again: the UI closes it, waits until it is gone, opens it,
/// waits until it is there.
struct Reopen {
    which: usize,
    step: Step,
}

#[derive(PartialEq)]
enum Step {
    Close,
    Gone,
    Open,
}

struct State {
    phase: Phase,
    frames: Arc<AtomicU64>,
    track: xirius_core::id::TrackId,
    id: PluginId,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn stage(_: u8, studio: &Rc<RefCell<Studio>>, _: &mut Window, cx: &mut App) {
    let track = studio
        .borrow()
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .expect("the sample song's Keys track")
        .id;
    let id = {
        let mut s = studio.borrow_mut();
        assert!(s.set_instrument(
            track,
            &crate::instrument::Pick::built_in(crate::instrument::DEFAULT)
        ));
        let id = crate::instrument::current(&s.project().tracks[&track])
            .unwrap()
            .id;
        s.show_instrument(track, id);
        s.open_channel(track);
        id
    };
    cx.refresh_windows();
    STATE.with(|cell| {
        *cell.borrow_mut() = Some(State {
            phase: Phase::Opening(30),
            frames: Arc::new(AtomicU64::new(0)),
            track,
            id,
        });
    });
}

/// The window `which` (0 the built-in's, 1 the channel editor's).
fn surface(
    studio: &Rc<RefCell<Studio>>,
    id: PluginId,
    which: usize,
    cx: &App,
) -> Option<WindowHandle<DeviceWindow>> {
    cx.windows().into_iter().find_map(|handle| {
        let handle = handle.downcast::<DeviceWindow>()?;
        let view = handle.read(cx).ok()?;
        let mine = Rc::ptr_eq(&view.studio, studio)
            && !view.menu_only
            && if which == 0 {
                view.id == Some(id)
            } else {
                view.id.is_none()
            };
        mine.then_some(handle)
    })
}

/// The points to ask about in the window `which`, from the layout the window itself draws.
fn layout(h: WindowHandle<DeviceWindow>, cx: &mut App) -> Option<Win> {
    h.update(cx, |view, window, _| {
        let scale = window.scale_factor();
        let viewport = window.viewport_size();
        let (w, hh): (f32, f32) = (viewport.width.into(), viewport.height.into());
        view.with(window, |s, g| {
            let bar = if s.instruments.detached_channel {
                s.channel_view(g)?.bar
            } else {
                let p = s.auto_shown();
                s.instruments.surface_bar(&p, g)?.2
            };
            let mid = crate::title_bar::BAR_H / 2.0;
            let q = |label: String, x: f32, y: f32, expect: isize| Query {
                label,
                x,
                y,
                expect,
            };
            let mut queries = vec![
                q(
                    "bar, empty stretch".into(),
                    bar.space.x + bar.space.w / 2.0,
                    mid,
                    HTCAPTION,
                ),
                q(
                    "bar, name".into(),
                    bar.name.x + bar.name.w / 2.0,
                    mid,
                    HTCAPTION,
                ),
            ];
            for (name, r) in bar.buttons() {
                queries.push(q(
                    format!("button: {name}"),
                    r.x + r.w / 2.0,
                    r.y + r.h / 2.0,
                    HTCLIENT,
                ));
                // Under the button, inside the bar: the bar's own strip.
                queries.push(q(
                    format!("bar, under {name}"),
                    r.x + r.w / 2.0,
                    crate::title_bar::BAR_H - 2.0,
                    HTCAPTION,
                ));
            }
            queries.push(q(
                "content".into(),
                w / 2.0,
                crate::title_bar::BAR_H + (hh - crate::title_bar::BAR_H) / 2.0,
                HTCLIENT,
            ));
            queries.push(q("top edge".into(), w / 2.0, 0.5, HTTOP));
            queries.push(q("left edge".into(), -1.0, hh / 2.0, HTLEFT));
            queries.push(q("right edge".into(), w + 1.0, hh / 2.0, HTRIGHT));
            queries.push(q("bottom edge".into(), w / 2.0, hh + 1.0, HTBOTTOM));
            Some(Win {
                scale,
                queries,
                head: (bar.space.x + bar.space.w / 2.0, mid),
            })
        })
    })
    .ok()
    .flatten()
}

/// The gate of the shot: the picture waits until the thread has asked everything. It never
/// blocks the window thread: each frame it answers at most one thing the thread asked for.
pub(crate) fn ready(studio: &Rc<RefCell<Studio>>, _: &mut Window, cx: &mut App) -> bool {
    STATE.with(|cell| {
        let mut state = cell.borrow_mut();
        let Some(state) = state.as_mut() else {
            return true;
        };
        state.frames.fetch_add(1, Ordering::AcqRel);
        let (track, id) = (state.track, state.id);
        match &mut state.phase {
            Phase::Done => true,
            Phase::Opening(n) => {
                if *n > 0 {
                    *n -= 1;
                    return false;
                }
                if !(0..2).all(|k| surface(studio, id, k, cx).is_some()) {
                    return false;
                }
                for (k, title) in TITLES.iter().enumerate() {
                    let h = surface(studio, id, k, cx).unwrap();
                    let _ = h.update(cx, |_, window, _| window.set_window_title(title));
                }
                let (ask_tx, asks) = channel();
                let (acks, ack_rx) = channel();
                let done = Arc::new(AtomicBool::new(false));
                let link = Link {
                    asks: ask_tx,
                    acks: Mutex::new(ack_rx),
                    frames: state.frames.clone(),
                };
                let finished = done.clone();
                std::thread::Builder::new()
                    .name("drag-probe".into())
                    .spawn(move || {
                        let report = run(&link);
                        write(&report);
                        finished.store(true, Ordering::Release);
                    })
                    .expect("the probe thread");
                state.phase = Phase::Running {
                    asks,
                    acks,
                    done,
                    job: None,
                    since: Instant::now(),
                };
                false
            }
            Phase::Running {
                asks,
                acks,
                done,
                job,
                since,
            } => {
                assert!(
                    since.elapsed() < Duration::from_secs(240),
                    "the drag probe timed out"
                );
                if done.load(Ordering::Acquire) {
                    state.phase = Phase::Done;
                    return true;
                }
                if let Some(j) = job {
                    let which = j.which;
                    let there = surface(studio, id, which, cx);
                    match j.step {
                        Step::Close => {
                            let mut s = studio.borrow_mut();
                            if which == 0 {
                                s.close_device_view(id);
                            } else {
                                s.channel = None;
                            }
                            j.step = Step::Gone;
                            drop(s);
                            cx.refresh_windows();
                        }
                        Step::Gone if there.is_none() => {
                            let mut s = studio.borrow_mut();
                            if which == 0 {
                                s.show_instrument(track, id);
                            } else {
                                s.open_channel(track);
                            }
                            j.step = Step::Open;
                            drop(s);
                            cx.refresh_windows();
                        }
                        Step::Open if there.is_some() => {
                            let title = if which == 0 {
                                format!("Xirius · {id}")
                            } else {
                                "Xirius · Channel Editor".to_string()
                            };
                            let _ = acks.send(Ack::Reopened(title));
                            *job = None;
                        }
                        _ => {}
                    }
                    return false;
                }
                let Ok(want) = asks.try_recv() else {
                    return false;
                };
                match want {
                    Want::Layout => {
                        let wins: Vec<Win> = (0..2)
                            .filter_map(|k| layout(surface(studio, id, k, cx)?, cx))
                            .collect();
                        let _ = acks.send(Ack::Layout(wins));
                    }
                    Want::Mouse(k, x, y) => {
                        if let Some(h) = surface(studio, id, k, cx) {
                            let _ = cx.update_window(h.into(), |_, window, cx| {
                                window.dispatch_event(
                                    PlatformInput::MouseMove(MouseMoveEvent {
                                        position: point(px(x), px(y)),
                                        pressed_button: None,
                                        modifiers: Modifiers::default(),
                                    }),
                                    cx,
                                );
                                window.refresh();
                            });
                        }
                        let _ = acks.send(Ack::Done);
                    }
                    Want::Press(k, x, y, clicks) => {
                        let propagate = surface(studio, id, k, cx)
                            .and_then(|h| {
                                cx.update_window(h.into(), |_, window, cx| {
                                    window
                                        .dispatch_event(
                                            PlatformInput::MouseDown(MouseDownEvent {
                                                button: MouseButton::Left,
                                                position: point(px(x), px(y)),
                                                modifiers: Modifiers::default(),
                                                click_count: clicks,
                                                first_mouse: false,
                                            }),
                                            cx,
                                        )
                                        .propagate
                                })
                                .ok()
                            })
                            .unwrap_or(false);
                        let _ = acks.send(Ack::Propagate(propagate));
                    }
                    Want::Reopen(which) => {
                        *job = Some(Reopen {
                            which,
                            step: Step::Close,
                        });
                    }
                }
                false
            }
        }
    })
}

/// The thread's half of the conversation with the UI.
struct Link {
    asks: Sender<Want>,
    acks: Mutex<Receiver<Ack>>,
    frames: Arc<AtomicU64>,
}

/// How long a busy machine may take to draw a frame or to show a window change.
pub(super) const PATIENCE: Duration = Duration::from_secs(30);

impl Link {
    fn ask(&self, want: Want) -> Option<Ack> {
        self.asks.send(want).ok()?;
        self.acks.lock().ok()?.recv_timeout(PATIENCE).ok()
    }

    /// Waits until `n` frames have begun.
    fn frames(&self, n: u64) {
        let until = self.frames.load(Ordering::Acquire) + n;
        let deadline = Instant::now() + PATIENCE;
        while self.frames.load(Ordering::Acquire) < until && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Moves the window's mouse to the point and waits for it to draw there.
    fn mouse(&self, k: usize, x: f32, y: f32) {
        self.ask(Want::Mouse(k, x, y));
        self.frames(8);
    }
}

pub(super) fn find(title: &str) -> Option<isize> {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        if let Some(h) = crate::park::own_handle(title) {
            return Some(h);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

pub(super) fn rect(hwnd: isize) -> [i32; 4] {
    let mut r = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: the out-pointer is valid for the call; the window is this process's.
    unsafe { GetWindowRect(hwnd as _, &mut r) };
    [r.left, r.top, r.right - r.left, r.bottom - r.top]
}

pub(super) fn origin(hwnd: isize) -> (i32, i32) {
    let mut p = POINT { x: 0, y: 0 };
    // SAFETY: as above.
    unsafe { ClientToScreen(hwnd as _, &mut p) };
    (p.x, p.y)
}

pub(super) fn lparam(x: i32, y: i32) -> isize {
    (((y & 0xFFFF) as isize) << 16) | ((x & 0xFFFF) as isize)
}

pub(super) fn zoomed(hwnd: isize) -> bool {
    // SAFETY: as above.
    unsafe { IsZoomed(hwnd as _) != 0 }
}

/// The screen point of a logical point of the window.
pub(super) fn screen(hwnd: isize, scale: f32, x: f32, y: f32) -> isize {
    let (ox, oy) = origin(hwnd);
    lparam(
        ox + (x * scale).round() as i32,
        oy + (y * scale).round() as i32,
    )
}

fn run(link: &Link) -> Value {
    let Some(Ack::Layout(wins)) = link.ask(Want::Layout) else {
        return json!({"error": "no layout"});
    };
    let hwnds: Vec<isize> = TITLES.iter().map(|t| find(t).unwrap_or(0)).collect();
    link.frames(10);
    let mut hits = Vec::new();
    for (k, w) in wins.iter().enumerate() {
        for q in &w.queries {
            link.mouse(k, q.x, q.y);
            // SAFETY: a plain query message to a window of this process, answered by the UI
            // thread's window procedure.
            let answer = unsafe {
                SendMessageW(
                    hwnds[k] as _,
                    WM_NCHITTEST,
                    0,
                    screen(hwnds[k], w.scale, q.x, q.y),
                )
            };
            hits.push(json!({
                "window": NAMES[k], "label": q.label, "x": q.x, "y": q.y,
                "expect": ht_name(q.expect), "answer": ht_name(answer),
                "ok": answer == q.expect,
            }));
        }
    }
    let mut press = Vec::new();
    let mut double = Vec::new();
    for (k, w) in wins.iter().enumerate() {
        let (x, y) = w.head;
        let first = matches!(
            link.ask(Want::Press(k, x, y, 1)),
            Some(Ack::Propagate(true))
        );
        let second = matches!(
            link.ask(Want::Press(k, x, y, 2)),
            Some(Ack::Propagate(true))
        );
        press.push(json!({
            "window": NAMES[k], "first_goes_on": first, "second_goes_on": second,
        }));
        // The system's double click: a press, then the double-click message, at the same point.
        link.mouse(k, x, y);
        let before = rect(hwnds[k]);
        let at = screen(hwnds[k], w.scale, x, y);
        // SAFETY: plain messages to a window of this process. The first press has no hit code
        // (`HTNOWHERE`) so the system does not start its move loop, which waits for the mouse.
        unsafe {
            SendMessageW(hwnds[k] as _, WM_NCLBUTTONDOWN, 0, at);
            SendMessageW(hwnds[k] as _, WM_NCLBUTTONDBLCLK, HTCAPTION as usize, at);
        }
        link.frames(15);
        let maximised = zoomed(hwnds[k]);
        let after = rect(hwnds[k]);
        if maximised {
            // SAFETY: as above.
            unsafe { SendMessageW(hwnds[k] as _, WM_SYSCOMMAND, SC_RESTORE as usize, 0) };
            link.frames(15);
        }
        double.push(json!({
            "window": NAMES[k], "maximised": maximised,
            "size_before": [before[2], before[3]], "size_after": [after[2], after[3]],
            "size_same": before[2..] == after[2..],
        }));
    }
    let mut moved = Vec::new();
    let mut resized = Vec::new();
    let mut reopened = Vec::new();
    for k in 0..wins.len() {
        let before = rect(hwnds[k]);
        // SAFETY: as above.
        unsafe {
            SetWindowPos(
                hwnds[k] as _,
                std::ptr::null_mut(),
                before[0] + 60,
                before[1] + 40,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        link.frames(15);
        let after = rect(hwnds[k]);
        moved.push(json!({
            "window": NAMES[k], "from": [before[0], before[1]], "to": [after[0], after[1]],
            "moved": after[0] == before[0] + 60 && after[1] == before[1] + 40,
            "size_same": before[2..] == after[2..],
        }));
        // What the system's snap does: it gives the window another size. The frame's own
        // resize must not set it back.
        let (aw, ah) = (before[2] + 120, before[3] + 80);
        // SAFETY: as above.
        unsafe {
            SetWindowPos(
                hwnds[k] as _,
                std::ptr::null_mut(),
                0,
                0,
                aw,
                ah,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        link.frames(40);
        let got = rect(hwnds[k]);
        resized.push(json!({
            "window": NAMES[k], "asked": [aw, ah], "got": [got[2], got[3]],
            "sticks": got[2] == aw && got[3] == ah,
        }));
        let placed = rect(hwnds[k]);
        let title = match link.ask(Want::Reopen(k)) {
            Some(Ack::Reopened(title)) => title,
            _ => String::new(),
        };
        link.frames(15);
        let again = find(&title).map(rect).unwrap_or_default();
        reopened.push(json!({
            "window": NAMES[k], "title": title, "before": placed, "after": again,
            "same_place": placed[..2] == again[..2],
            "same_size": placed[2..] == again[2..],
        }));
        // The window of the other kind is still the one found by its first title.
    }
    json!({
        "hits": hits, "press": press, "double_click": double,
        "move": moved, "resize": resized, "reopen": reopened,
    })
}

fn write(report: &Value) {
    let args: Vec<String> = std::env::args().collect();
    let Some(dir) = args
        .windows(2)
        .find(|a| a[0] == "--shot")
        .map(|a| a[1].clone())
    else {
        return;
    };
    let dir = std::path::Path::new(&dir);
    let _ = std::fs::write(
        dir.join("drag.json"),
        serde_json::to_string_pretty(report).unwrap_or_default(),
    );
    let _ = std::fs::write(dir.join("drag.md"), markdown(report));
}

fn markdown(report: &Value) -> String {
    let mut out =
        String::from("## Independent windows: move by the bar (no OS input was sent)\n\n");
    out.push_str(
        "| window | point | asked at (logical px) | expected | answer | ok |\n|---|---|---|---|---|---|\n",
    );
    for h in report["hits"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "| {} | {} | ({:.1}, {:.1}) | {} | {} | {} |\n",
            h["window"].as_str().unwrap_or(""),
            h["label"].as_str().unwrap_or(""),
            h["x"].as_f64().unwrap_or(0.0),
            h["y"].as_f64().unwrap_or(0.0),
            h["expect"].as_str().unwrap_or(""),
            h["answer"].as_str().unwrap_or(""),
            if h["ok"].as_bool() == Some(true) {
                "ok"
            } else {
                "NO"
            },
        ));
    }
    for key in ["press", "double_click", "move", "resize", "reopen"] {
        out.push_str(&format!("\n### {key}\n\n```json\n{}\n```\n", report[key]));
    }
    out
}
