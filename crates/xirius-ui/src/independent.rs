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
            set_owner(&title);
            if !menu_only || parked {
                show(&title);
            }
            view
        },
    )
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
