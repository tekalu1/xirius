//! UI-337: the independent windows, checked in the real app. `--script device-verify` opens a
//! built-in instrument's window (the Sampler), the Channel Editor and the Macro Controls, parked
//! and silent, and a thread asks each of them what Windows asks a window. It is all plain window
//! messages and `PrintWindow`; no OS input is sent and no window is brought to the front.
//!
//! - the owner of each window is the main window, and it stays above the main window in the
//!   Z-order even when the main window is put on top;
//! - minimising the main window hides them with it and restoring it brings them back;
//! - `WM_NCHITTEST` on the empty bar, each button and the content; a double click on the bar;
//! - the opening size of the Channel Editor, the least width the system and the frame keep
//!   (520 without layers, 720 with), and the Macro Controls not being resizable;
//! - closing and opening each window again: the place and the size come back;
//! - the Multi Instrument's keyboard under the range map, and pictures of the windows.
//!
//! The result is `verify.json` and `verify.md` in the shots' folder (the pictures are `*.bmp`
//! there), read by `tests/independent_window_checks.rs`.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::drag_shots::{PATIENCE, find, rect, screen, zoomed};
use super::*;
use crate::chrome::{HTCAPTION, HTCLIENT, ht_name};
use crate::shot::{Act, Capture, Outcome, Play, Shot};
use gpui::{Modifiers, MouseDownEvent, MouseMoveEvent, PlatformInput, WindowHandle};
use serde_json::{Value, json};
use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GW_HWNDNEXT, GW_OWNER, GWL_STYLE, GetClientRect, GetTopWindow, GetWindow, GetWindowLongW,
    HWND_TOP, IsIconic, IsWindowVisible, MINMAXINFO, SC_RESTORE, SW_SHOWMINNOACTIVE,
    SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SendMessageW,
    SetWindowPos, ShowWindow, WM_GETMINMAXINFO, WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN,
    WM_SYSCOMMAND, WS_MAXIMIZEBOX, WS_THICKFRAME,
};
use xirius_core::rack::Source;

pub const SHOTS: &[Shot] = &[Shot {
    name: "verify-01-windows",
    acts: &[Act::Stage(Play::Window(stage), 0)],
}];

const NAMES: [&str; 3] = ["built-in instrument", "channel editor", "macro controls"];
const CHANNEL_TITLE: &str = "Xirius · Channel Editor";
const MACROS_TITLE: &str = "Xirius · Macro Controls";

#[derive(Clone, Debug)]
struct Query {
    label: String,
    x: f32,
    y: f32,
    expect: isize,
}

#[derive(Clone, Debug)]
struct Win {
    scale: f32,
    queries: Vec<Query>,
    head: (f32, f32),
}

enum Want {
    Info,
    Layout(usize),
    Mouse(usize, f32, f32),
    Press(usize, f32, f32, usize),
    Reopen(usize),
    /// The Keys track becomes a Multi Instrument with three layers.
    Layers,
    Keys,
}

enum Ack {
    Info(Value),
    Layout(Option<Win>),
    Done,
    Propagate(bool),
    Reopened,
    Keys(Value),
}

enum Phase {
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
        s.open_macros(track);
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

fn title_of(which: usize, id: PluginId) -> String {
    match which {
        0 => format!("Xirius · {id}"),
        1 => CHANNEL_TITLE.to_string(),
        _ => MACROS_TITLE.to_string(),
    }
}

/// The window `which` (0 the built-in's, 1 the channel editor's, 2 the macro controls').
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
            && match which {
                0 => view.id == Some(id),
                1 => view.id.is_none() && !view.macros,
                _ => view.macros,
            };
        mine.then_some(handle)
    })
}

fn layout(h: WindowHandle<DeviceWindow>, cx: &mut App) -> Option<Win> {
    h.update(cx, |view, window, _| {
        let scale = window.scale_factor();
        let viewport = window.viewport_size();
        let (w, hh): (f32, f32) = (viewport.width.into(), viewport.height.into());
        view.with(window, |s, g| {
            let bar = if s.instruments.detached_macros {
                s.macro_view()?.bar
            } else if s.instruments.detached_channel {
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
            }
            queries.push(q(
                "content".into(),
                w / 2.0,
                crate::title_bar::BAR_H + (hh - crate::title_bar::BAR_H) / 2.0,
                HTCLIENT,
            ));
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

/// What the Channel Editor opened at, and the numbers the app computes for it.
fn info(studio: &Rc<RefCell<Studio>>, id: PluginId, cx: &mut App) -> Value {
    let (natural, least) = {
        let s = studio.borrow();
        let p = s.project();
        s.channel
            .as_ref()
            .map(|w| {
                (
                    crate::channel::natural_size(&p, w),
                    crate::channel::min_width(&p, w),
                )
            })
            .unwrap_or((None, 0.0))
    };
    let scale = surface(studio, id, 1, cx)
        .and_then(|h| h.update(cx, |_, w, _| w.scale_factor()).ok())
        .unwrap_or(1.0);
    json!({
        "scale": scale,
        "natural": natural.map(|(w, h)| [w, h]),
        "least_width": least,
        "bar_h": crate::title_bar::BAR_H,
        "macro_size": [crate::macro_pad::W, crate::macro_pad::H],
        "titles": (0..3).map(|k| title_of(k, id)).collect::<Vec<_>>(),
    })
}

/// The Multi Instrument's figure and keyboard, as the real window lays them out.
fn keys(studio: &Rc<RefCell<Studio>>, id: PluginId, cx: &mut App) -> Value {
    let Some(h) = surface(studio, id, 1, cx) else {
        return json!({"error": "no channel editor"});
    };
    h.update(cx, |view, window, _| {
        let viewport = window.viewport_size();
        let size: (f32, f32) = (viewport.width.into(), viewport.height.into());
        view.with(window, |s, g| {
            let Some(v) = s.channel_view(g) else {
                return json!({"error": "no view"});
            };
            let Some(k) = v.keys else {
                return json!({"error": "no key figure", "window": [size.0, size.1]});
            };
            let (m, b, p) = (&k.map, &k.board.view, &k.panel);
            json!({
                "window": [size.0, size.1],
                "panel": [p.x, p.y, p.w, p.h],
                "map": [m.x, m.y, m.w, m.h],
                "board": [b.x, b.y, b.w, b.h],
                "gap": b.y - (m.y + m.h),
                "notes": k.board.keys.len(),
                "low": k.board.spec.low,
                "high": k.board.spec.high,
                "white_pitch": k.board.spec.pitch,
                "key_height": k.board.spec.height,
                "extent": k.board.extent,
                "scroll": k.board.scroll,
                "layers": k.boxes.len(),
            })
        })
    })
    .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

fn add_layers(studio: &Rc<RefCell<Studio>>, track: xirius_core::id::TrackId) {
    let mut s = studio.borrow_mut();
    for name in ["Bass", "Piano", "Strings"] {
        s.run_command("rack.layer.add", json!({ "track": track, "name": name }));
    }
    let layers = match &s.project().tracks[&track].rack.as_ref().unwrap().source {
        Some(Source::Multi { layers, .. }) => layers.clone(),
        other => panic!("{other:?}"),
    };
    for (l, (keys, velocity)) in
        layers
            .iter()
            .zip([("E0-B1", "1-127"), ("C2-C6", "1-100"), ("C2-C6", "80-127")])
    {
        let args = json!({ "id": l.id.to_string(), "keys": keys, "velocity": velocity });
        s.run_command("rack.layer.set", args);
    }
    s.open_channel(track);
}

/// The gate of the shot: it answers at most one thing the thread asked for per frame.
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
                if !(0..3).all(|k| surface(studio, id, k, cx).is_some()) {
                    return false;
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
                    .name("verify-probe".into())
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
                    since.elapsed() < Duration::from_secs(300),
                    "the verify probe timed out"
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
                            match which {
                                0 => s.close_device_view(id),
                                1 => s.channel = None,
                                _ => s.macro_pad = None,
                            }
                            j.step = Step::Gone;
                            drop(s);
                            cx.refresh_windows();
                        }
                        Step::Gone if there.is_none() => {
                            let mut s = studio.borrow_mut();
                            match which {
                                0 => s.show_instrument(track, id),
                                1 => s.open_channel(track),
                                _ => s.open_macros(track),
                            }
                            j.step = Step::Open;
                            drop(s);
                            cx.refresh_windows();
                        }
                        Step::Open if there.is_some() => {
                            let _ = acks.send(Ack::Reopened);
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
                    Want::Info => {
                        let _ = acks.send(Ack::Info(info(studio, id, cx)));
                    }
                    Want::Layout(k) => {
                        let win = surface(studio, id, k, cx).and_then(|h| layout(h, cx));
                        let _ = acks.send(Ack::Layout(win));
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
                    Want::Layers => {
                        add_layers(studio, track);
                        cx.refresh_windows();
                        let _ = acks.send(Ack::Done);
                    }
                    Want::Keys => {
                        let _ = acks.send(Ack::Keys(keys(studio, id, cx)));
                    }
                }
                false
            }
        }
    })
}

struct Link {
    asks: Sender<Want>,
    acks: Mutex<Receiver<Ack>>,
    frames: Arc<AtomicU64>,
}

impl Link {
    fn ask(&self, want: Want) -> Option<Ack> {
        self.asks.send(want).ok()?;
        self.acks.lock().ok()?.recv_timeout(PATIENCE).ok()
    }

    fn frames(&self, n: u64) {
        let until = self.frames.load(Ordering::Acquire) + n;
        let deadline = Instant::now() + PATIENCE;
        while self.frames.load(Ordering::Acquire) < until && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn mouse(&self, k: usize, x: f32, y: f32) {
        self.ask(Want::Mouse(k, x, y));
        self.frames(8);
    }

    fn info(&self) -> Value {
        match self.ask(Want::Info) {
            Some(Ack::Info(v)) => v,
            _ => json!({}),
        }
    }

    fn keys(&self) -> Value {
        match self.ask(Want::Keys) {
            Some(Ack::Keys(v)) => v,
            _ => json!({"error": "no answer"}),
        }
    }
}

/// The windows from the top of the Z-order down, as handles.
fn z_order() -> Vec<isize> {
    let mut out = Vec::new();
    // SAFETY: plain queries about the top-level windows.
    unsafe {
        let mut h = GetTopWindow(std::ptr::null_mut());
        while !h.is_null() && out.len() < 4000 {
            out.push(h as isize);
            h = GetWindow(h, GW_HWNDNEXT);
        }
    }
    out
}

fn owner(h: isize) -> isize {
    // SAFETY: a plain query about a window of this process.
    unsafe { GetWindow(h as _, GW_OWNER) as isize }
}

fn visible(h: isize) -> bool {
    // SAFETY: as above.
    unsafe { IsWindowVisible(h as _) != 0 }
}

fn iconic(h: isize) -> bool {
    // SAFETY: as above.
    unsafe { IsIconic(h as _) != 0 }
}

fn client(h: isize) -> [i32; 2] {
    let mut r = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: as above; the out-pointer is valid for the call.
    unsafe { GetClientRect(h as _, &mut r) };
    [r.right - r.left, r.bottom - r.top]
}

fn style(h: isize) -> u32 {
    // SAFETY: as above.
    unsafe { GetWindowLongW(h as _, GWL_STYLE) as u32 }
}

/// What the window asks the system for: its least and greatest size (physical px).
fn min_max(h: isize) -> Value {
    let mut m: MINMAXINFO = unsafe { std::mem::zeroed() };
    // SAFETY: `WM_GETMINMAXINFO` takes a pointer to a MINMAXINFO that lives across the call;
    // the window is this process's and answers on the UI thread.
    unsafe {
        SendMessageW(
            h as _,
            WM_GETMINMAXINFO,
            0,
            &mut m as *mut MINMAXINFO as isize,
        )
    };
    json!({
        "min": [m.ptMinTrackSize.x, m.ptMinTrackSize.y],
        "max": [m.ptMaxTrackSize.x, m.ptMaxTrackSize.y],
    })
}

fn set_size(h: isize, w: i32, hh: i32) {
    // SAFETY: as above.
    unsafe {
        SetWindowPos(
            h as _,
            std::ptr::null_mut(),
            0,
            0,
            w,
            hh,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
}

fn wait_until(what: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if what() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    what()
}

fn shot(title: &str, name: &str, dir: &std::path::Path) -> Value {
    let path = dir.join(format!("{name}.bmp"));
    let capture = Capture::start(title, path);
    let deadline = Instant::now() + PATIENCE;
    while capture.outcome() == Outcome::Pending && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    json!({"name": name, "ok": capture.outcome() == Outcome::Saved})
}

fn out_dir() -> Option<std::path::PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    args.windows(2)
        .find(|a| a[0] == "--shot")
        .map(|a| std::path::PathBuf::from(&a[1]))
}

fn zinfo(hs: &[isize], main: isize) -> Value {
    let z = z_order();
    let at = |h: isize| z.iter().position(|x| *x == h);
    json!({
        "main_index": at(main),
        "indexes": hs.iter().map(|h| at(*h)).collect::<Vec<_>>(),
        "all_above_main": hs.iter().all(|h| at(*h) < at(main) && at(*h).is_some()),
    })
}

fn run(link: &Link) -> Value {
    let dir = out_dir().unwrap_or_else(std::env::temp_dir);
    let info0 = link.info();
    let scale = info0["scale"].as_f64().unwrap_or(1.0) as f32;
    let titles: Vec<String> = info0["titles"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|t| t.as_str().unwrap_or("").to_string())
                .collect()
        })
        .unwrap_or_default();
    if titles.len() != 3 {
        return json!({"error": "no titles"});
    }
    let main = find(crate::screen::WINDOW_TITLE).unwrap_or(0);
    let hs: Vec<isize> = titles.iter().map(|t| find(t).unwrap_or(0)).collect();
    link.frames(10);

    // a) the owner and the Z-order
    let mut owners = Vec::new();
    for (k, h) in hs.iter().enumerate() {
        owners.push(json!({
            "window": NAMES[k], "owner_is_main": owner(*h) == main && main != 0,
            "visible": visible(*h),
        }));
    }
    let z_before = zinfo(&hs, main);
    // The main window is put on top of everything without activating it: an owned window
    // must stay in front of its owner.
    // SAFETY: a plain window position request to the main window of this process.
    unsafe {
        SetWindowPos(
            main as _,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    };
    link.frames(10);
    let z_after_raise = zinfo(&hs, main);

    // e) the opening size, the least width, not resizable
    let natural = info0["natural"].clone();
    let bar_h = info0["bar_h"].as_f64().unwrap_or(0.0);
    let ce_client = client(hs[1]);
    let opening = json!({
        "ce_client_logical": [ce_client[0] as f32 / scale, ce_client[1] as f32 / scale],
        "natural_with_bar": natural, "bar_h": bar_h,
        "ce_rect": rect(hs[1]),
        "scale": scale,
    });
    let mut sizes = Vec::new();
    for (k, h) in hs.iter().enumerate() {
        let st = style(*h);
        sizes.push(json!({
            "window": NAMES[k],
            "thick_frame": st & WS_THICKFRAME != 0,
            "maximize_box": st & WS_MAXIMIZEBOX != 0,
            "minmax": min_max(*h),
            "least_width_logical": info0["least_width"],
        }));
    }
    let mut pictures = vec![shot(&titles[0], "sampler-device", &dir)];

    // c) the hit tests and the double click
    let mut hits = Vec::new();
    let mut press = Vec::new();
    let mut double = Vec::new();
    for k in 0..3 {
        let Some(Ack::Layout(Some(w))) = link.ask(Want::Layout(k)) else {
            hits.push(json!({"window": NAMES[k], "label": "layout", "ok": false}));
            continue;
        };
        for q in &w.queries {
            link.mouse(k, q.x, q.y);
            // SAFETY: a plain query message to a window of this process, answered by the UI
            // thread's window procedure.
            let answer = unsafe {
                SendMessageW(
                    hs[k] as _,
                    WM_NCHITTEST,
                    0,
                    screen(hs[k], w.scale, q.x, q.y),
                )
            };
            hits.push(json!({
                "window": NAMES[k], "label": q.label, "x": q.x, "y": q.y,
                "expect": ht_name(q.expect), "answer": ht_name(answer),
                "ok": answer == q.expect,
            }));
        }
        let (x, y) = w.head;
        let first = matches!(
            link.ask(Want::Press(k, x, y, 1)),
            Some(Ack::Propagate(true))
        );
        let second = matches!(
            link.ask(Want::Press(k, x, y, 2)),
            Some(Ack::Propagate(true))
        );
        press.push(json!({"window": NAMES[k], "first_goes_on": first, "second_goes_on": second}));
        link.mouse(k, x, y);
        let before = rect(hs[k]);
        let at = screen(hs[k], w.scale, x, y);
        // SAFETY: plain messages to a window of this process; the first press has no hit code
        // so the system does not start its move loop.
        unsafe {
            SendMessageW(hs[k] as _, WM_NCLBUTTONDOWN, 0, at);
            SendMessageW(hs[k] as _, WM_NCLBUTTONDBLCLK, HTCAPTION as usize, at);
        }
        link.frames(15);
        let maximised = zoomed(hs[k]);
        let after = rect(hs[k]);
        if maximised {
            // SAFETY: as above.
            unsafe { SendMessageW(hs[k] as _, WM_SYSCOMMAND, SC_RESTORE as usize, 0) };
            link.frames(15);
        }
        double.push(json!({
            "window": NAMES[k], "maximised": maximised,
            "size_same": before[2..] == after[2..],
        }));
    }

    // b) minimise the main window: the owned windows go with it, and come back with it
    // SAFETY: ShowWindow on the main window of this process, without activating it.
    unsafe { ShowWindow(main as _, SW_SHOWMINNOACTIVE) };
    let hidden = wait_until(|| iconic(main) && hs.iter().all(|h| !visible(*h)));
    let during = json!({
        "main_iconic": iconic(main),
        "owned_visible": hs.iter().map(|h| visible(*h)).collect::<Vec<_>>(),
    });
    // SAFETY: as above.
    unsafe { ShowWindow(main as _, SW_SHOWNOACTIVATE) };
    let back = wait_until(|| !iconic(main) && hs.iter().all(|h| visible(*h)));
    link.frames(10);
    let minimise = json!({
        "hidden_with_main": hidden, "during": during,
        "restored_with_main": back,
        "after": {
            "main_iconic": iconic(main),
            "owned_visible": hs.iter().map(|h| visible(*h)).collect::<Vec<_>>(),
            "z": zinfo(&hs, main),
        },
    });

    // d) place and size: move and size each, close, open again
    let mut reopened = Vec::new();
    let mut least = Vec::new();
    for k in 0..3 {
        let before = rect(hs[k]);
        // SAFETY: as above.
        unsafe {
            SetWindowPos(
                hs[k] as _,
                std::ptr::null_mut(),
                before[0] + 60,
                before[1] + 40,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        link.frames(15);
        if k < 2 {
            set_size(hs[k], before[2] + 120, before[3] + 80);
            link.frames(40);
        } else {
            // A size asked of the fixed-size window: what the frame keeps.
            set_size(hs[k], before[2] + 120, before[3] + 80);
            link.frames(40);
        }
        let placed = rect(hs[k]);
        let sized = json!({
            "asked": [before[2] + 120, before[3] + 80], "got": [placed[2], placed[3]],
        });
        if k == 1 {
            // The least width: ask for far less than the Channel Editor needs.
            set_size(hs[k], (100.0 * scale) as i32, placed[3]);
            link.frames(40);
            let narrow = rect(hs[k]);
            least.push(json!({
                "window": "channel editor, no layers", "asked_width_logical": 100,
                "got_width_logical": narrow[2] as f32 / scale,
                "least_logical": info0["least_width"],
            }));
            set_size(hs[k], placed[2], placed[3]);
            link.frames(40);
        }
        let placed = rect(hs[k]);
        let reopen_ok = matches!(link.ask(Want::Reopen(k)), Some(Ack::Reopened));
        link.frames(15);
        let again = find(&titles[k]).map(rect).unwrap_or_default();
        let h2 = find(&titles[k]).unwrap_or(0);
        reopened.push(json!({
            "window": NAMES[k], "reopened": reopen_ok, "before": placed, "after": again,
            // A parked window is put back at the parking place whatever was remembered, so
            // the place can only be seen as "parked again"; the size is the remembered one.
            "same_place": again[..2] == [crate::park::PARK_AT, crate::park::PARK_AT],
            "same_size": placed[2..] == again[2..],
            "sized": sized,
            "owner_is_main": owner(h2) == main && main != 0,
        }));
    }

    // f) the Multi Instrument: layers, the window at its opening size, then other widths
    let hs: Vec<isize> = titles.iter().map(|t| find(t).unwrap_or(0)).collect();
    link.ask(Want::Layers);
    link.frames(30);
    // The window of the track now has layers: it is closed and opened again so it opens at
    // the size of its content (a remembered size would win).
    let info1 = link.info();
    let ce = hs[1];
    let multi_sizes = json!({
        "least_width_logical": info1["least_width"],
        "natural": info1["natural"],
        "minmax": min_max(ce),
        "rect_before": rect(ce),
    });
    let mut multi = Vec::new();
    // The remembered size is the last one; give it the natural one first.
    let (nw, nh) = (
        info1["natural"][0].as_f64().unwrap_or(720.0) as f32,
        info1["natural"][1].as_f64().unwrap_or(600.0) as f32,
    );
    for (label, w) in [
        ("natural", nw),
        ("1280", 1280.0),
        ("1000", 1000.0),
        ("720", 720.0),
        ("600-asked", 600.0),
        ("400-asked", 400.0),
    ] {
        set_size(ce, (w * scale).round() as i32, (nh * scale).round() as i32);
        link.frames(40);
        let r = rect(ce);
        let k = link.keys();
        let p = shot(CHANNEL_TITLE, &format!("ce-multi-{label}"), &dir);
        multi.push(json!({
            "label": label, "asked_width_logical": w,
            "got_logical": [r[2] as f32 / scale, r[3] as f32 / scale],
            "keys": k, "picture": p,
        }));
    }
    pictures.push(shot(MACROS_TITLE, "macro-controls", &dir));
    json!({
        "scale": scale, "owner": owners, "z_before": z_before, "z_after_raise": z_after_raise,
        "opening": opening, "sizes": sizes, "hits": hits, "press": press,
        "double_click": double, "minimise": minimise, "reopen": reopened,
        "least_width": least, "multi_sizes": multi_sizes, "multi": multi,
        "pictures": pictures,
    })
}

fn write(report: &Value) {
    let Some(dir) = out_dir() else {
        return;
    };
    let _ = std::fs::write(
        dir.join("verify.json"),
        serde_json::to_string_pretty(report).unwrap_or_default(),
    );
    let mut out = String::from("## Independent windows, checked in the real app\n");
    if let Some(map) = report.as_object() {
        for (key, value) in map {
            out.push_str(&format!("\n### {key}\n\n```json\n{value}\n```\n"));
        }
    }
    let _ = std::fs::write(dir.join("verify.md"), out);
}
