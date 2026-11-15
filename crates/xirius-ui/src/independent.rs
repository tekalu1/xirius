//! The one way an independent window is opened (UI-337): the built-in and plug-in device
//! windows, the Channel Editor, the Macro Controls and the Presets popup all come through
//! [`open`], so what they share is set in one place: the client-drawn bar, the place it
//! remembers while the app is open, the fit to the display, the parking of a test's window,
//! and the owner (the main window).

use gpui::{
    App, Bounds, Entity, Pixels, Render, Size, TitlebarOptions, Window, WindowBounds,
    WindowDecorations, WindowHandle, WindowOptions, point, px,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use xirius_core::id::PluginId;

/// What a window is. Presets is a menu in a window (`menu_only`), with no place to remember.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Device(PluginId),
    Channel,
    /// The Macro Controls: a window of a fixed size.
    Macros,
    Presets,
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
            Kind::Presets => None,
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
    /// Where it opens the first time (a remembered place of its `kind` wins).
    pub initial: Bounds<Pixels>,
    pub min_size: Option<Size<Pixels>>,
    pub resizable: bool,
    pub positions: Option<Positions>,
}

fn offscreen() -> bool {
    std::env::var_os(xirius_vst3::OFFSCREEN_ENV).is_some()
}

/// Opens an independent window. `build` makes the window's view; it runs inside the new
/// window, before it is shown (without taking the focus).
pub(crate) fn open<V: Render + 'static>(
    cx: &mut App,
    main: &Window,
    spec: Spec,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V> + 'static,
) -> gpui::Result<WindowHandle<V>> {
    let menu_only = spec.kind == Kind::Presets;
    let parked = offscreen();
    let mut initial = spec
        .kind
        .key()
        .zip(spec.positions.as_ref())
        .and_then(|(key, p)| p.borrow().get(&key).copied())
        .unwrap_or(spec.initial);
    if !parked
        && let Some(display) = cx
            .displays()
            .into_iter()
            .find(|d| d.bounds().contains(&initial.origin))
            .or_else(|| main.display(cx))
    {
        initial = fit_bounds(initial, display.visible_bounds());
    }
    if parked
        && menu_only
        && let Some(display) = main.display(cx)
    {
        // GPUI replaces an off-display initial rectangle with its defaults.
        // Create hidden on the display, then park before showing it.
        initial.origin = display.visible_bounds().origin;
    }
    let title = spec.title.clone();
    let spec_key = spec.kind.key();
    let positions = spec.positions.clone();
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(initial)),
            titlebar: Some(TitlebarOptions {
                title: Some(spec.title.into()),
                appears_transparent: true,
                // macOS: the buttons are moved out of the window (the bar draws its own).
                traffic_light_position: Some(point(px(-120.0), px(8.0))),
            }),
            window_decorations: Some(WindowDecorations::Client),
            focus: menu_only && !parked,
            show: menu_only && !parked,
            is_minimizable: false,
            is_resizable: spec.resizable,
            window_min_size: spec.min_size,
            ..Default::default()
        },
        move |window, cx| {
            let view = build(window, cx);
            if !menu_only {
                // A window created hidden keeps the system's default place and size until
                // GPUI applies its initial placement, which only an activation does, and
                // these windows are shown without one (a parked window is never activated
                // either). Place and size it now, so it opens as asked.
                window.resize(initial.size);
                place(&title, initial.origin, window.scale_factor());
                if let Some((key, positions)) = spec_key.zip(positions.as_ref()) {
                    // Remember it from the start, so a reopen finds it though it was never
                    // moved (and the Macro Controls find where the Channel Editor is).
                    positions.borrow_mut().insert(key, initial);
                }
            }
            set_owner(&title);
            if !menu_only || parked {
                show(&title);
            }
            view
        },
    )
}

/// Puts the client area of the window at `origin` (logical px). Windows only: elsewhere the
/// platform places the window at the initial bounds it is created with.
fn place(title: &str, origin: gpui::Point<Pixels>, scale: f32) {
    #[cfg(not(windows))]
    let _ = (title, origin, scale);
    #[cfg(windows)]
    if let Some(hwnd) = crate::park::own_handle(title) {
        use windows_sys::Win32::Foundation::{POINT, RECT};
        use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowRect, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
        };
        let mut at = POINT { x: 0, y: 0 };
        let mut frame = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: this process's window on the GUI thread; the out-pointers are valid for
        // the calls. Only its position changes (no size, no activation).
        unsafe {
            if ClientToScreen(hwnd as _, &mut at) == 0 || GetWindowRect(hwnd as _, &mut frame) == 0
            {
                return;
            }
            let want_x = (f32::from(origin.x) * scale).round() as i32;
            let want_y = (f32::from(origin.y) * scale).round() as i32;
            SetWindowPos(
                hwnd as _,
                std::ptr::null_mut(),
                frame.left + want_x - at.x,
                frame.top + want_y - at.y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
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
        assert_eq!(Kind::Presets.key(), None);
    }
}
