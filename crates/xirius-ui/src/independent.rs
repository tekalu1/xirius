//! The one way an independent window is opened (UI-337): the built-in and plug-in device
//! windows, the Channel Editor and the Macro Controls all come through
//! [`open`], so what they share is set in one place: the client-drawn bar, the place it
//! remembers while the app is open, the fit to the display, the parking of a test's window,
//! and the owner (the main window).

use gpui::{
    App, Bounds, DisplayId, Entity, Pixels, Render, Size, TitlebarOptions, Window, WindowBounds,
    WindowDecorations, WindowHandle, WindowOptions, point, px,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use xirius_core::id::PluginId;

/// What an independent song window shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Device(PluginId),
    Channel,
    /// The Macro Controls: a window of a fixed size.
    Macros,
}

/// What is under the pointer, as the main window last looked (UI-353): the window that is
/// frontmost there decides what a drag let go does, not the window the mouse's events reach
/// (Windows sends a drag's moves and its letting-go to the window it was pressed in).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum Over {
    /// The main window: a drag let go does what the point under it would take.
    #[default]
    Main,
    /// An independent window, with the pointer in its coordinates.
    Window(Kind, (f32, f32)),
    /// Another app's window is on top there, or nothing of ours is.
    Elsewhere,
}

/// The windows whose place and size are remembered while the app is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Key {
    Device(PluginId),
    Channel,
    Macros,
}

impl Kind {
    pub(crate) fn key(self) -> Option<Key> {
        match self {
            Kind::Device(id) => Some(Key::Device(id)),
            Kind::Channel => Some(Key::Channel),
            Kind::Macros => Some(Key::Macros),
        }
    }
}

/// One tracked window's frame on the screen, in the units the pointer's position comes
/// in: where its client area sits, and how many of those units make one of the window's
/// own logical pixels (its scale factor; 1.0 where everything is logical already).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Frame {
    pub kind: Kind,
    pub rect: crate::studio::Rect,
    pub scale: f32,
}

/// The frontmost window at a point: what the OS answers where it can be asked, what the
/// remembered bounds answer where it cannot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Top {
    /// An independent window: its index in the `frames` given with it.
    Window(usize),
    /// The main window.
    Main,
    /// Another app's window, or nothing of ours.
    Foreign,
}

/// Where a drag let go at `at` lands: the frontmost window there decides, not the window
/// the drag's events go to. `at` and the `frames` are in the same screen units; the answer
/// for an independent window is its local position, in its own logical pixels.
pub(crate) fn resolve_over(at: (f32, f32), top: Top, frames: &[Frame]) -> Over {
    match top {
        Top::Main => Over::Main,
        Top::Foreign => Over::Elsewhere,
        Top::Window(i) => frames.get(i).map_or(Over::Elsewhere, |f| {
            Over::Window(
                f.kind,
                ((at.0 - f.rect.x) / f.scale, (at.1 - f.rect.y) / f.scale),
            )
        }),
    }
}

/// The frontmost window at `at` where the OS cannot be asked (macOS, tests): an
/// independent window whose remembered bounds cover it, else the main window's own.
/// The first frame wins where the windows overlap (the tracker lists them front to back).
#[cfg(any(not(windows), test))]
pub(crate) fn top_by_bounds(at: (f32, f32), main: crate::studio::Rect, frames: &[Frame]) -> Top {
    if let Some(i) = frames.iter().position(|f| f.rect.contains(at.0, at.1)) {
        return Top::Window(i);
    }
    if main.contains(at.0, at.1) {
        Top::Main
    } else {
        Top::Foreign
    }
}

/// The places of the windows, kept while the app is open (not saved across runs).
pub(crate) type Positions = Rc<RefCell<HashMap<Key, Bounds<Pixels>>>>;

pub(crate) struct Spec {
    pub kind: Kind,
    pub title: String,
    /// Where it opens the first time (a remembered place of its `kind` wins), in the logical
    /// pixels of its display.
    pub initial: Bounds<Pixels>,
    /// The display `initial` is on, where the caller knows it (from [`screens`]); else the
    /// display whose bounds hold its origin.
    pub display: Option<DisplayId>,
    pub native: Option<NativePlacement>,
    pub min_size: Option<Size<Pixels>>,
    pub resizable: bool,
    pub positions: Option<Positions>,
    pub restore: bool,
}

pub(crate) struct NativePlacement {
    work: [i32; 4],
    /// Reopened and replacement editors use their remembered origin, with only the work-area clamp.
    occupied: Vec<[i32; 4]>,
}

fn offscreen() -> bool {
    std::env::var_os(xirius_vst3::OFFSCREEN_ENV).is_some()
}

/// Creates the hidden save form. The caller assigns the editor as its owner before showing it.
pub(crate) fn open_preset_save<V: Render + 'static>(
    cx: &mut App,
    initial: Bounds<Pixels>,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V> + 'static,
) -> gpui::Result<WindowHandle<V>> {
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(initial)),
            titlebar: Some(TitlebarOptions {
                title: Some("Save preset".into()),
                ..Default::default()
            }),
            show: false,
            focus: false,
            is_resizable: false,
            is_minimizable: false,
            ..Default::default()
        },
        build,
    )
}

/// Opens an independent window. `build` makes the window's view; it runs inside the new
/// window, before it is shown (without taking the focus).
pub(crate) fn open<V: Render + 'static>(
    cx: &mut App,
    main: &Window,
    spec: Spec,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V> + 'static,
) -> gpui::Result<WindowHandle<V>> {
    let parked = offscreen();
    let mut initial = spec
        .kind
        .key()
        .zip(spec.positions.as_ref())
        .filter(|_| spec.restore)
        .and_then(|(key, p)| p.borrow().get(&key).copied())
        .unwrap_or(spec.initial);
    let display = spec
        .display
        .and_then(|id| cx.find_display(id))
        .or_else(|| {
            cx.displays()
                .into_iter()
                .find(|d| d.bounds().contains(&initial.origin))
        })
        .or_else(|| main.display(cx));
    if !parked && let Some(display) = &display {
        initial = fit_bounds(initial, display.visible_bounds());
    }
    // Windows: the bounds are the display's logical pixels, and GPUI checks them against the
    // display it is given, the primary one when none is: on another display it would put the
    // window in the middle of the primary one at its default size. Elsewhere the platform's
    // own placement is kept as it was.
    let display = display.filter(|_| cfg!(windows) && !parked);
    // The window is created on the display the system picks; `place` moves it in the pixels
    // of the display it goes to, not those of the one it was created on.
    let scale = display.as_ref().and_then(|d| {
        screens(cx)
            .into_iter()
            .find(|s| s.id == d.id())
            .map(|s| s.scale)
    });
    let title = spec.title.clone();
    let spec_key = spec.kind.key();
    let positions = spec.positions.clone();
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(initial)),
            display_id: display.map(|d| d.id()),
            titlebar: Some(TitlebarOptions {
                title: Some(spec.title.into()),
                appears_transparent: true,
                // macOS: the buttons are moved out of the window (the bar draws its own).
                traffic_light_position: Some(point(px(-120.0), px(8.0))),
            }),
            window_decorations: Some(WindowDecorations::Client),
            focus: false,
            show: false,
            is_minimizable: false,
            is_resizable: spec.resizable,
            window_min_size: spec.min_size,
            ..Default::default()
        },
        move |window, cx| {
            let view = build(window, cx);
            {
                // A window created hidden keeps the system's default place and size until
                // GPUI applies its initial placement, which only an activation does, and
                // these windows are shown without one (a parked window is never activated
                // either). Place and size it now, so it opens as asked.
                if let Some(placed) = place(
                    &title,
                    initial,
                    scale.unwrap_or_else(|| window.scale_factor()),
                    matches!(spec.kind, Kind::Device(_)),
                    spec.native.as_ref(),
                ) {
                    initial = placed;
                } else {
                    window.resize(initial.size);
                }
                if let Some((key, positions)) = spec_key.zip(positions.as_ref()) {
                    // Remember it from the start, so a reopen finds it though it was never
                    // moved (and the Macro Controls find where the Channel Editor is).
                    positions.borrow_mut().insert(key, initial);
                }
            }
            set_owner(&title);
            show(&title);
            if cfg!(windows) {
                // The native move and resize can finish before GPUI registers the window.
                // Read the resulting client size before the first layout and hit test.
                window.bounds_changed(cx);
            }
            view
        },
    )
}

/// Applies the size and origin before showing the window. Device windows use their
/// frame origin so it agrees with the origin used by the external editor host.
fn place(
    title: &str,
    bounds: Bounds<Pixels>,
    scale: f32,
    frame_origin: bool,
    native: Option<&NativePlacement>,
) -> Option<Bounds<Pixels>> {
    #[cfg(not(windows))]
    let _ = (
        title,
        bounds,
        scale,
        frame_origin,
        native.map(|p| (&p.work, &p.occupied)),
    );
    #[cfg(windows)]
    if let Some(hwnd) = crate::park::own_handle(title) {
        use windows_sys::Win32::Foundation::{POINT, RECT};
        use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetClientRect, GetWindowRect, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
        };
        let mut at = POINT { x: 0, y: 0 };
        let mut frame = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let mut client = RECT::default();
        // SAFETY: this process's window on the GUI thread. All out-pointers are valid.
        unsafe {
            if ClientToScreen(hwnd as _, &mut at) == 0
                || GetWindowRect(hwnd as _, &mut frame) == 0
                || GetClientRect(hwnd as _, &mut client) == 0
            {
                return None;
            }
            let physical = |px| (f32::from(px) * scale).round() as i32;
            let want_x = physical(bounds.origin.x);
            let want_y = physical(bounds.origin.y);
            let border_width = frame.right - frame.left - client.right;
            let border_height = frame.bottom - frame.top - client.bottom;
            let (mut x, mut y) = if frame_origin {
                (want_x, want_y)
            } else {
                (frame.left + want_x - at.x, frame.top + want_y - at.y)
            };
            let mut width = physical(bounds.size.width) + border_width;
            let mut height = physical(bounds.size.height) + border_height;
            if let Some(native) = native {
                let [l, t, r, b] = native.work;
                width = width.min(r - l);
                height = height.min(b - t);
                (x, y) = xirius_vst3::place_beside(
                    (x, y),
                    (width, height),
                    native.work,
                    &native.occupied,
                    (24.0 * scale).round() as i32,
                );
            }
            let placed = SetWindowPos(
                hwnd as _,
                std::ptr::null_mut(),
                x,
                y,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            ) != 0;
            return placed.then(|| {
                let logical = |value| px(value as f32 / scale);
                Bounds::new(
                    if frame_origin {
                        point(logical(x), logical(y))
                    } else {
                        bounds.origin
                    },
                    gpui::size(
                        logical(width - border_width),
                        logical(height - border_height),
                    ),
                )
            });
        }
    }
    None
}

/// Makes the main window the owner: the window is always in front of it, and goes with it
/// when it is minimised. Windows only; macOS (`addChildWindow:`) needs a direct objc
/// dependency this crate does not have, and Linux has no such request in GPUI.
fn set_owner(title: &str) {
    #[cfg(not(windows))]
    let _ = title;
    #[cfg(windows)]
    if let (Some(hwnd), Some(main)) = (
        crate::park::own_handle(title),
        crate::park::own_handle(crate::screen::WINDOW_TITLE),
    ) && hwnd != main
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GWLP_HWNDPARENT, SetWindowLongPtrW};
        // SAFETY: two windows of this process on the GUI thread; for a top-level window this
        // sets its owner (it does not make it a child).
        unsafe {
            SetWindowLongPtrW(hwnd as _, GWLP_HWNDPARENT, main);
        }
    }
}

fn show(title: &str) {
    #[cfg(not(windows))]
    let _ = title;
    #[cfg(windows)]
    if let Some(hwnd) = crate::park::own_handle(title) {
        if offscreen() {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                GWL_EXSTYLE, GetWindowLongW, SetWindowLongW, WS_EX_NOACTIVATE,
            };
            // SAFETY: only this test's parked window, before it is shown.
            unsafe {
                SetWindowLongW(
                    hwnd as _,
                    GWL_EXSTYLE,
                    GetWindowLongW(hwnd as _, GWL_EXSTYLE) | WS_EX_NOACTIVATE as i32,
                );
            }
            crate::park::park(title, crate::park::PARK_AT, crate::park::PARK_AT);
        }
        // SAFETY: the title lookup accepts only this process's window; never activates it.
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(
                hwnd as _,
                windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE,
            );
        }
    }
}

/// A display in the units the OS gives a point on the screen in (physical pixels of the
/// whole desktop): all of it, the part windows may cover (without the taskbar), and how many
/// physical pixels make one of its logical ones. Each display's logical pixels are its
/// physical ones divided by its own scale, as GPUI counts them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Screen {
    pub id: DisplayId,
    /// Left, top, right, bottom.
    pub area: [i32; 4],
    pub work: [i32; 4],
    pub scale: f32,
}

/// The displays GPUI knows, with their physical bounds and scale. Windows only (GPUI names a
/// display by its monitor handle there); elsewhere none, and the callers keep to GPUI's
/// logical bounds.
pub(crate) fn screens(cx: &App) -> Vec<Screen> {
    #[cfg(not(windows))]
    {
        let _ = cx;
        Vec::new()
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::RECT;
        use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, HMONITOR, MONITORINFO};
        use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
        let edges = |r: RECT| [r.left, r.top, r.right, r.bottom];
        cx.displays()
            .into_iter()
            .filter_map(|d| {
                let monitor = u64::from(d.id()) as usize as HMONITOR;
                // SAFETY: plain queries; a handle that names no monitor fails them.
                unsafe {
                    let mut info: MONITORINFO = std::mem::zeroed();
                    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
                    let (mut dpi, mut dpi_y) = (0, 0);
                    if GetMonitorInfoW(monitor, &mut info) == 0
                        || GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y) != 0
                    {
                        return None;
                    }
                    Some(Screen {
                        id: d.id(),
                        area: edges(info.rcMonitor),
                        work: edges(info.rcWork),
                        scale: dpi.max(96) as f32 / 96.0,
                    })
                }
            })
            .collect()
    }
}

/// Choose an independent device or loading window's place, in the destination monitor's
/// logical pixels. Replacements inherit the old window's origin and keep their own size.
pub(crate) fn device_bounds(
    studio: &crate::studio::Studio,
    main: &Window,
    cx: &App,
    id: PluginId,
    mut initial: Bounds<Pixels>,
    source: u64,
) -> (Bounds<Pixels>, Option<DisplayId>, Option<NativePlacement>) {
    if offscreen() {
        return (initial, None, None);
    }
    let positions = studio.instruments.positions.borrow();
    let previous = studio.normal_view_before(id);
    #[cfg(windows)]
    let screens = screens(cx);
    #[cfg(windows)]
    let native = |hwnd| {
        let area = xirius_vst3::EditorArea::from_window(hwnd)?;
        let screen = screens.iter().find(|screen| screen.work == area.work)?;
        Some((
            point(
                px(area.main[0] as f32 / screen.scale),
                px(area.main[1] as f32 / screen.scale),
            ),
            screen.id,
        ))
    };
    if let Some(old) = previous
        && let Some(bounds) = positions.get(&Key::Device(old))
    {
        initial.origin = bounds.origin;
    }
    #[cfg(windows)]
    let display = if let Some(old) = previous {
        studio
            .device_window_handle(old)
            .and_then(native)
            .map(|(origin, id)| {
                initial.origin = origin;
                id
            })
    } else {
        native(source as isize).map(|(_, id)| id)
    };
    #[cfg(not(windows))]
    let display = {
        let _ = source;
        None
    };
    let display = display
        .and_then(|id| cx.find_display(id))
        .or_else(|| {
            if previous.is_some() {
                return cx
                    .displays()
                    .into_iter()
                    .find(|d| d.bounds().contains(&initial.origin));
            }
            main.display(cx)
        })
        .or_else(|| main.display(cx));
    let Some(display) = display else {
        return (initial, None, None);
    };
    let work = display.visible_bounds();
    initial = fit_bounds(initial, work);
    #[cfg(windows)]
    {
        let native = screens
            .iter()
            .find(|s| s.id == display.id())
            .map(|&screen| {
                let occupied = if previous.is_some() || positions.contains_key(&Key::Device(id)) {
                    Vec::new()
                } else {
                    xirius_vst3::visible_editors(
                        crate::device_window::hwnd_of(main),
                        &studio.editor_stack(),
                    )
                };
                NativePlacement {
                    work: screen.work,
                    occupied,
                }
            });
        (initial, Some(display.id()), native)
    }
    #[cfg(not(windows))]
    {
        if previous.is_some() {
            return (initial, Some(display.id()), None);
        }
        let to_int = |value: Pixels| f32::from(value).round() as i32;
        let edges = |bounds: Bounds<Pixels>| {
            [bounds.left(), bounds.top(), bounds.right(), bounds.bottom()].map(to_int)
        };
        let occupied: Vec<_> = positions
            .iter()
            .filter_map(|(key, &bounds)| {
                let visible = match *key {
                    Key::Device(other) => {
                        other != id
                            && studio.instruments.detached.contains(&other)
                            && !studio.instruments.pending.contains(&other)
                    }
                    Key::Channel => studio.channel.is_some(),
                    Key::Macros => studio.instruments.detached_macros,
                };
                visible.then(|| edges(bounds))
            })
            .collect();
        let (x, y) = xirius_vst3::place_beside(
            (to_int(initial.origin.x), to_int(initial.origin.y)),
            (to_int(initial.size.width), to_int(initial.size.height)),
            edges(work),
            &occupied,
            24,
        );
        initial.origin = point(px(x as f32), px(y as f32));
        (initial, Some(display.id()), None)
    }
}

pub(crate) fn fit_bounds(mut bounds: Bounds<Pixels>, work: Bounds<Pixels>) -> Bounds<Pixels> {
    bounds.size.width = bounds.size.width.min(work.size.width);
    bounds.size.height = bounds.size.height.min(work.size.height);
    bounds.origin.x = bounds.origin.x.clamp(
        work.origin.x,
        work.origin.x + work.size.width - bounds.size.width,
    );
    bounds.origin.y = bounds.origin.y.clamp(
        work.origin.y,
        work.origin.y + work.size.height - bounds.size.height,
    );
    bounds
}

#[cfg(all(test, windows))]
mod window_tests {
    use super::*;
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GetClientRect, GetSystemMetrics, GetWindowRect,
        SM_XVIRTUALSCREEN, WS_OVERLAPPEDWINDOW,
    };

    struct HiddenFrame(HWND);
    impl Drop for HiddenFrame {
        fn drop(&mut self) {
            // SAFETY: this thread created and owns the window.
            unsafe { DestroyWindow(self.0) };
        }
    }

    #[test]
    fn the_full_native_frame_fits_in_the_gap_at_fractional_scale() {
        let title = "Xirius physical placement regression";
        let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<_>>();
        // SAFETY: built-in window class, no parent, and created hidden off the desktop.
        let (frame, left) = unsafe {
            let left = GetSystemMetrics(SM_XVIRTUALSCREEN) - 2500;
            let frame = CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                wide(title).as_ptr(),
                WS_OVERLAPPEDWINDOW,
                left,
                40,
                720,
                510,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            assert!(!frame.is_null());
            (HiddenFrame(frame), left)
        };
        let mut before = RECT::default();
        let mut client = RECT::default();
        // SAFETY: the hidden fixture is alive, and the two outputs are writable.
        unsafe {
            assert_ne!(GetWindowRect(frame.0, &mut before), 0);
            assert_ne!(GetClientRect(frame.0, &mut client), 0);
        }
        let scale = 1.75;
        let width = 700 + before.right - before.left - client.right;
        let height = 455 + before.bottom - before.top - client.bottom;
        let top = 40;
        let native = NativePlacement {
            work: [left, top, left + 2 * width, top + height],
            occupied: vec![[left + width, top, left + 2 * width, top + height]],
        };
        let to_logical = |value| px(value as f32 / scale);
        let desired = Bounds::new(
            point(to_logical(left + width - 700), to_logical(top)),
            gpui::size(px(400.0), px(260.0)),
        );
        let mut placed = RECT::default();
        // With only the client width in the placement check, this origin looked clear.
        assert!(place(title, desired, scale, true, None).is_some());
        // SAFETY: the hidden fixture is alive, and the output is writable.
        assert_ne!(unsafe { GetWindowRect(frame.0, &mut placed) }, 0);
        assert!(placed.right > native.occupied[0][0]);

        assert!(place(title, desired, scale, true, Some(&native)).is_some());
        // SAFETY: the same hidden fixture is still alive.
        assert_ne!(unsafe { GetWindowRect(frame.0, &mut placed) }, 0);
        assert_eq!(
            [placed.left, placed.top, placed.right, placed.bottom],
            [left, top, left + width, top + height]
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    #[test]
    fn remembered_bounds_fit_the_current_work_area() {
        let work = Bounds::new(point(px(100.0), px(50.0)), size(px(1200.0), px(800.0)));
        let old = Bounds::new(point(px(2000.0), px(-500.0)), size(px(700.0), px(500.0)));
        let fitted = fit_bounds(old, work);
        assert_eq!(fitted.origin, point(px(600.0), px(50.0)));
        assert_eq!(fitted.size, old.size);
        assert_eq!(
            fit_bounds(Bounds::new(old.origin, size(px(2000.0), px(1500.0))), work),
            work
        );
    }

    #[test]
    fn only_windows_with_a_place_have_a_key() {
        let id = PluginId(1);
        assert_eq!(Kind::Device(id).key(), Some(Key::Device(id)));
        assert_eq!(Kind::Channel.key(), Some(Key::Channel));
        assert_eq!(Kind::Macros.key(), Some(Key::Macros));
    }

    fn rect(x: f32, y: f32, w: f32, h: f32) -> crate::studio::Rect {
        crate::studio::Rect::new(x, y, w, h)
    }

    #[test]
    fn the_frontmost_window_at_the_point_decides() {
        let id = PluginId(1);
        let device = Frame {
            kind: Kind::Device(id),
            rect: rect(600.0, 60.0, 520.0, 500.0),
            scale: 1.0,
        };
        let channel = Frame {
            kind: Kind::Channel,
            rect: rect(40.0, 80.0, 460.0, 560.0),
            scale: 1.0,
        };
        let frames = [device, channel];
        let main = rect(0.0, 0.0, 1440.0, 850.0);
        // Over the device window: the answer is in its own coordinates.
        assert_eq!(
            resolve_over((720.0, 200.0), Top::Window(0), &frames),
            Over::Window(Kind::Device(id), (120.0, 140.0))
        );
        // The bounds-derived top reaches the same answer.
        assert_eq!(top_by_bounds((720.0, 200.0), main, &frames), Top::Window(0));
        // The window's edge, just in and just out.
        assert_eq!(
            top_by_bounds((1119.9, 200.0), main, &frames),
            Top::Window(0)
        );
        assert_eq!(top_by_bounds((1120.0, 200.0), main, &frames), Top::Main);
        // Between two independent windows and off every window of ours.
        assert_eq!(top_by_bounds((540.0, 200.0), main, &frames), Top::Main);
        assert_eq!(top_by_bounds((2000.0, 200.0), main, &frames), Top::Foreign);
        assert_eq!(
            resolve_over((2000.0, 200.0), Top::Foreign, &frames),
            Over::Elsewhere
        );
        // A window the OS names frontmost is local even where the event's window would
        // not reach it.
        assert_eq!(
            resolve_over((50.0, 85.0), Top::Window(1), &frames),
            Over::Window(Kind::Channel, (10.0, 5.0))
        );
    }

    #[test]
    fn the_scale_divides_the_screen_position_into_local_pixels() {
        let id = PluginId(7);
        // A 175% display: the client area spans 1.75 physical pixels per logical one.
        let frames = [Frame {
            kind: Kind::Device(id),
            rect: rect(1050.0, 105.0, 910.0, 875.0),
            scale: 1.75,
        }];
        assert_eq!(
            resolve_over((1050.0, 105.0), Top::Window(0), &frames),
            Over::Window(Kind::Device(id), (0.0, 0.0))
        );
        assert_eq!(
            resolve_over((1400.0, 280.0), Top::Window(0), &frames),
            Over::Window(Kind::Device(id), (200.0, 100.0))
        );
        // A window on a screen to the left (a negative origin) the same way.
        let left = [Frame {
            kind: Kind::Device(id),
            rect: rect(-1600.0, 40.0, 800.0, 600.0),
            scale: 1.0,
        }];
        assert_eq!(
            top_by_bounds((-900.0, 200.0), rect(0.0, 0.0, 1440.0, 850.0), &left),
            Top::Window(0)
        );
        assert_eq!(
            resolve_over((-900.0, 200.0), Top::Window(0), &left),
            Over::Window(Kind::Device(id), (700.0, 160.0))
        );
    }
}
