//! Checks, without sending any keyboard or mouse input from the OS, that the
//! window with its own title bar behaves like a normal Windows window.
//!
//! What it asks the window itself (all of it plain window messages, none of it
//! reaches whatever window the person is using):
//!
//! - `WM_NCHITTEST` at points on the title bar, the three buttons, the
//!   content and the borders. The answer is what Windows uses to decide what a
//!   mouse press there does (`HTCAPTION` = drag the window and maximise on a
//!   double click, `HTMAXBUTTON` = maximise button and the Windows 11 snap
//!   layouts, `HTLEFT` = resize, ...).
//! - the window style bits that the system's snap (Win+arrow, dragging to an
//!   edge) needs;
//! - `WM_SYSCOMMAND` maximise and restore, and whether the window follows;
//! - `WM_NCLBUTTONDBLCLK` on the empty part of the bar (the message the system sends for a
//!   double click on the caption): the window maximises, and restores on the second.
//!
//! A real drag with the mouse is *not* exercised: that needs OS mouse input.

/// A point to ask about, in logical pixels from the top-left of the client
/// area (negative or past the size means "on the border").
#[derive(Clone, Debug)]
pub struct Query {
    pub label: &'static str,
    pub x: f32,
    pub y: f32,
    /// The `HT*` code the window should answer with.
    pub expect: isize,
}

/// The points to ask about for a window of this logical size.
pub fn queries(width: f32, height: f32, caption_button_w: f32, top_bar_h: f32) -> Vec<Query> {
    let bar_y = top_bar_h / 2.0;
    let q = |label, x, y, expect| Query {
        label,
        x,
        y,
        expect,
    };
    vec![
        q("title bar, empty part", width / 2.0, bar_y, HTCAPTION),
        q(
            "minimise button",
            width - caption_button_w * 2.5,
            bar_y,
            HTMINBUTTON,
        ),
        q(
            "maximise button",
            width - caption_button_w * 1.5,
            bar_y,
            HTMAXBUTTON,
        ),
        q(
            "close button",
            width - caption_button_w * 0.5,
            bar_y,
            HTCLOSE,
        ),
        q("content (timeline)", width / 2.0, height / 2.0, HTCLIENT),
        q("top edge", width / 2.0, 0.5, HTTOP),
        q("left border", -1.0, height / 2.0, HTLEFT),
        q("right border", width + 1.0, height / 2.0, HTRIGHT),
        q("bottom border", width / 2.0, height + 1.0, HTBOTTOM),
    ]
}

pub const HTCLIENT: isize = 1;
pub const HTCAPTION: isize = 2;
pub const HTMINBUTTON: isize = 8;
pub const HTMAXBUTTON: isize = 9;
pub const HTLEFT: isize = 10;
pub const HTRIGHT: isize = 11;
pub const HTTOP: isize = 12;
pub const HTBOTTOM: isize = 15;
pub const HTCLOSE: isize = 20;

pub fn ht_name(code: isize) -> &'static str {
    match code {
        0 => "HTNOWHERE",
        1 => "HTCLIENT",
        2 => "HTCAPTION",
        8 => "HTMINBUTTON",
        9 => "HTMAXBUTTON",
        10 => "HTLEFT",
        11 => "HTRIGHT",
        12 => "HTTOP",
        13 => "HTTOPLEFT",
        14 => "HTTOPRIGHT",
        15 => "HTBOTTOM",
        16 => "HTBOTTOMLEFT",
        17 => "HTBOTTOMRIGHT",
        20 => "HTCLOSE",
        _ => "other",
    }
}

#[cfg(windows)]
pub use imp::Probe;

#[cfg(not(windows))]
pub use other::Probe;

#[cfg(not(windows))]
mod other {
    use super::Query;

    /// Windows only; elsewhere there is nothing to ask.
    pub struct Probe;

    impl Probe {
        pub fn start(
            _title: &'static str,
            _scale: f32,
            _queries: Vec<Query>,
            _out: std::path::PathBuf,
        ) -> Option<Probe> {
            None
        }

        pub fn pending(&self) -> Option<(usize, f32, f32)> {
            None
        }

        pub fn positioned(&self, _index: usize) {}

        pub fn frame(&self) {}

        pub fn done(&self) -> bool {
            true
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::fmt::Write as _;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use super::{Query, ht_name};

    type Hwnd = *mut c_void;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn FindWindowW(class: *const u16, title: *const u16) -> Hwnd;
        fn SendMessageW(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> isize;
        fn ClientToScreen(hwnd: Hwnd, point: *mut Point) -> i32;
        fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
        fn GetWindowLongW(hwnd: Hwnd, index: i32) -> i32;
        fn IsZoomed(hwnd: Hwnd) -> i32;
    }

    const WM_NCHITTEST: u32 = 0x0084;
    const WM_NCLBUTTONDBLCLK: u32 = 0x00A3;
    const WM_SYSCOMMAND: u32 = 0x0112;
    const SC_MAXIMIZE: usize = 0xF030;
    const SC_RESTORE: usize = 0xF120;
    const GWL_STYLE: i32 = -16;
    const WS_CAPTION: u32 = 0x00C0_0000;
    const WS_SYSMENU: u32 = 0x0008_0000;
    const WS_THICKFRAME: u32 = 0x0004_0000;
    const WS_MINIMIZEBOX: u32 = 0x0002_0000;
    const WS_MAXIMIZEBOX: u32 = 0x0001_0000;

    const NONE: usize = usize::MAX;

    struct Shared {
        /// Index of the query the thread wants the UI to set up (the mouse
        /// moved there), or `NONE`.
        want: AtomicUsize,
        /// Index the UI has moved the mouse to.
        positioned: AtomicUsize,
        /// Frames the UI has begun, and the frame in which it moved the mouse.
        frames: AtomicU64,
        positioned_at: AtomicU64,
        queries: Vec<Query>,
    }

    /// The UI thread's half: it moves GPUI's mouse position where the thread
    /// asks, because GPUI answers a hit test from its last mouse position.
    pub struct Probe {
        shared: Arc<Shared>,
        done: Arc<AtomicBool>,
    }

    impl Probe {
        /// Starts the asking thread. `scale` is the window's scale factor.
        pub fn start(
            title: &'static str,
            scale: f32,
            queries: Vec<Query>,
            out: PathBuf,
        ) -> Option<Probe> {
            let shared = Arc::new(Shared {
                want: AtomicUsize::new(NONE),
                positioned: AtomicUsize::new(NONE),
                frames: AtomicU64::new(0),
                positioned_at: AtomicU64::new(0),
                queries,
            });
            let done = Arc::new(AtomicBool::new(false));
            std::thread::Builder::new()
                .name("chrome-probe".into())
                .spawn({
                    let shared = shared.clone();
                    let done = done.clone();
                    move || {
                        let report = run(title, scale, &shared);
                        // design-lint: allow(ui-blocking-io) PRJ-031: chrome-probe worker output.
                        println!("\n{report}");
                        // design-lint: allow(ui-blocking-io) PRJ-031: offline/startup or explicit screenshot/benchmark harness, outside live Studio callbacks.
                        let wrote = std::fs::create_dir_all(&out)
                            // design-lint: allow(ui-blocking-io) PRJ-031: offline/startup or explicit screenshot/benchmark harness, outside live Studio callbacks.
                            .and_then(|()| std::fs::write(out.join("chrome.md"), &report));
                        if let Err(e) = wrote {
                            // design-lint: allow(ui-blocking-io) PRJ-031: chrome-probe worker output.
                            eprintln!("could not write chrome.md: {e}");
                        }
                        done.store(true, Ordering::Release);
                    }
                })
                .ok()?;
            Some(Probe { shared, done })
        }

        /// The query to set up now: (index, logical x, logical y).
        pub fn pending(&self) -> Option<(usize, f32, f32)> {
            if self.done.load(Ordering::Acquire) {
                return None;
            }
            let want = self.shared.want.load(Ordering::Acquire);
            if want == NONE || self.shared.positioned.load(Ordering::Acquire) == want {
                return None;
            }
            let q = self.shared.queries.get(want)?;
            Some((want, q.x, q.y))
        }

        pub fn positioned(&self, index: usize) {
            let frame = self.shared.frames.load(Ordering::Acquire);
            self.shared.positioned_at.store(frame, Ordering::Release);
            self.shared.positioned.store(index, Ordering::Release);
        }

        /// Called at the start of every frame; the thread waits on frames, not on time.
        pub fn frame(&self) {
            self.shared.frames.fetch_add(1, Ordering::AcqRel);
        }

        /// Whether the report is written (or could not be); the app stays open until then.
        pub fn done(&self) -> bool {
            self.done.load(Ordering::Acquire)
        }
    }

    /// How long a busy machine may take to draw a frame or to show a window change.
    const PATIENCE: Duration = Duration::from_secs(20);

    /// Waits until `frames` frames have begun after frame `since` (or [`PATIENCE`] passed).
    fn frames_after(shared: &Shared, since: u64, frames: u64) {
        let deadline = Instant::now() + PATIENCE;
        while shared.frames.load(Ordering::Acquire) < since + frames && Instant::now() < deadline {
            // design-lint: allow(ui-blocking-io) PRJ-031: offline/startup or explicit screenshot/benchmark harness, outside live Studio callbacks.
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Waits until the UI has moved the mouse to query `k` and drawn two frames since, so
    /// GPUI's hit test of that position is fresh.
    fn position(shared: &Shared, k: usize) {
        shared.want.store(k, Ordering::Release);
        let deadline = Instant::now() + PATIENCE;
        while shared.positioned.load(Ordering::Acquire) != k && Instant::now() < deadline {
            // design-lint: allow(ui-blocking-io) PRJ-031: offline/startup or explicit screenshot/benchmark harness, outside live Studio callbacks.
            std::thread::sleep(Duration::from_millis(5));
        }
        frames_after(shared, shared.positioned_at.load(Ordering::Acquire), 3);
    }

    /// Waits until the window is (or is no longer) maximised, then for the UI to draw at
    /// the new size; returns whether it got there.
    fn becomes_zoomed(hwnd: Hwnd, shared: &Shared, want: bool) -> bool {
        let deadline = Instant::now() + PATIENCE;
        while zoomed(hwnd) != want && Instant::now() < deadline {
            // design-lint: allow(ui-blocking-io) PRJ-031: offline/startup or explicit screenshot/benchmark harness, outside live Studio callbacks.
            std::thread::sleep(Duration::from_millis(5));
        }
        let got = zoomed(hwnd) == want;
        frames_after(shared, shared.frames.load(Ordering::Acquire), 3);
        got
    }

    fn find(title: &str) -> Option<Hwnd> {
        let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            // SAFETY: `wide` is a NUL-terminated UTF-16 string; a null class matches any.
            let hwnd = unsafe { FindWindowW(std::ptr::null(), wide.as_ptr()) };
            if !hwnd.is_null() {
                return Some(hwnd);
            }
            // design-lint: allow(ui-blocking-io) PRJ-031: offline/startup or explicit screenshot/benchmark harness, outside live Studio callbacks.
            std::thread::sleep(Duration::from_millis(100));
        }
        None
    }

    fn style(hwnd: Hwnd) -> u32 {
        // SAFETY: `hwnd` is a window of this process.
        unsafe { GetWindowLongW(hwnd, GWL_STYLE) as u32 }
    }

    fn zoomed(hwnd: Hwnd) -> bool {
        // SAFETY: as above.
        unsafe { IsZoomed(hwnd) != 0 }
    }

    fn run(title: &'static str, scale: f32, shared: &Shared) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "## Window chrome check (no OS input was sent)\n");
        let Some(hwnd) = find(title) else {
            let _ = writeln!(out, "window {title:?} not found");
            return out;
        };
        // Let the first frames draw the bar.
        frames_after(shared, 0, 10);

        let st = style(hwnd);
        let bit = |m: u32| if st & m == m { "yes" } else { "no" };
        let _ = writeln!(
            out,
            "Style bits: WS_THICKFRAME {} / WS_MAXIMIZEBOX {} / WS_MINIMIZEBOX {} / WS_SYSMENU {} / WS_CAPTION {}",
            bit(WS_THICKFRAME),
            bit(WS_MAXIMIZEBOX),
            bit(WS_MINIMIZEBOX),
            bit(WS_SYSMENU),
            bit(WS_CAPTION)
        );

        let mut rect = Rect::default();
        let mut origin = Point::default();
        // SAFETY: both out-pointers are valid for the call.
        unsafe {
            GetWindowRect(hwnd, &mut rect);
            ClientToScreen(hwnd, &mut origin);
        }
        let _ = writeln!(
            out,
            "Client area starts {} px below the top of the window and {} px right of its left edge (0 = no OS title bar).\n",
            origin.y - rect.top,
            origin.x - rect.left
        );

        let _ = writeln!(
            out,
            "| point | asked at (logical px) | expected | answer | ok |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|");
        let mut all_ok = true;
        for (k, q) in shared.queries.iter().enumerate() {
            position(shared, k);
            let px = origin.x + (q.x * scale).round() as i32;
            let py = origin.y + (q.y * scale).round() as i32;
            let lparam = (((py & 0xFFFF) as isize) << 16) | ((px & 0xFFFF) as isize);
            // SAFETY: a plain query message to a window of this process,
            // answered by the UI thread's window procedure.
            let answer = unsafe { SendMessageW(hwnd, WM_NCHITTEST, 0, lparam) };
            let ok = answer == q.expect;
            all_ok &= ok;
            let _ = writeln!(
                out,
                "| {} | ({:.1}, {:.1}) | {} | {} | {} |",
                q.label,
                q.x,
                q.y,
                ht_name(q.expect),
                ht_name(answer),
                if ok { "ok" } else { "NO" }
            );
        }
        shared.want.store(NONE, Ordering::Release);

        // SAFETY: plain system commands to a window of this process.
        unsafe { SendMessageW(hwnd, WM_SYSCOMMAND, SC_MAXIMIZE, 0) };
        let max_ok = becomes_zoomed(hwnd, shared, true);
        // SAFETY: as above.
        unsafe { SendMessageW(hwnd, WM_SYSCOMMAND, SC_RESTORE, 0) };
        let restore_ok = becomes_zoomed(hwnd, shared, false);
        let _ = writeln!(
            out,
            "\nSC_MAXIMIZE -> maximised: {}. SC_RESTORE -> restored: {}.",
            if max_ok { "yes" } else { "NO" },
            if restore_ok { "yes" } else { "NO" }
        );

        // A double click on the empty part of the bar: the mouse goes there (GPUI hit tests from
        // its last position), then the message the system sends for a double click on the caption.
        // The app sees it first as a press; it must leave it to the system, which maximises on the
        // first and restores on the second.
        let (dbl_max, dbl_restore) = match shared
            .queries
            .iter()
            .position(|q| q.expect == super::HTCAPTION)
        {
            Some(k) => {
                position(shared, k);
                let q = &shared.queries[k];
                let px = origin.x + (q.x * scale).round() as i32;
                let py = origin.y + (q.y * scale).round() as i32;
                let lparam = (((py & 0xFFFF) as isize) << 16) | ((px & 0xFFFF) as isize);
                let was = zoomed(hwnd);
                // SAFETY: plain messages to a window of this process.
                unsafe {
                    SendMessageW(hwnd, WM_NCLBUTTONDBLCLK, super::HTCAPTION as usize, lparam)
                };
                let max = becomes_zoomed(hwnd, shared, true);
                // The bar moved with the window: the second double click is at the same
                // place in the window, which now is somewhere else on the screen.
                let mut now = Point::default();
                // SAFETY: the out-pointer is valid for the call.
                unsafe { ClientToScreen(hwnd, &mut now) };
                let px = now.x + (q.x * scale).round() as i32;
                let py = now.y + (q.y * scale).round() as i32;
                let lparam = (((py & 0xFFFF) as isize) << 16) | ((px & 0xFFFF) as isize);
                // SAFETY: as above.
                unsafe {
                    SendMessageW(hwnd, WM_NCLBUTTONDBLCLK, super::HTCAPTION as usize, lparam)
                };
                let restored = becomes_zoomed(hwnd, shared, false);
                shared.want.store(NONE, Ordering::Release);
                (!was && max, restored)
            }
            None => (false, false),
        };
        let _ = writeln!(
            out,
            "Double click on the empty bar -> maximised: {}. Again -> restored: {}.",
            if dbl_max { "yes" } else { "NO" },
            if dbl_restore { "yes" } else { "NO" }
        );
        let _ = writeln!(
            out,
            "Hit tests: {}.",
            if all_ok {
                "all as expected"
            } else {
                "SOME DIFFER"
            }
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caption_buttons_sit_inside_the_bar_right_to_left() {
        let qs = queries(1440.0, 900.0, 46.0, 32.0);
        let x = |code| qs.iter().find(|q| q.expect == code).map(|q| q.x).unwrap();
        assert!(x(HTMINBUTTON) < x(HTMAXBUTTON) && x(HTMAXBUTTON) < x(HTCLOSE));
        assert!(x(HTCLOSE) < 1440.0);
        assert!(
            qs.iter()
                .filter(|q| q.expect == HTCAPTION)
                .all(|q| q.y < 32.0)
        );
    }

    #[test]
    fn border_points_are_outside_or_on_the_edge() {
        let qs = queries(800.0, 600.0, 46.0, 32.0);
        for q in qs
            .iter()
            .filter(|q| [HTLEFT, HTRIGHT, HTBOTTOM].contains(&q.expect))
        {
            assert!(q.x < 0.0 || q.x > 800.0 || q.y > 600.0, "{}", q.label);
        }
    }
}
