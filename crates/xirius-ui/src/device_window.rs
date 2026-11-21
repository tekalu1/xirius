//! GPUI surfaces sharing the main Studio and its existing device views (UI-337).
use crate::{
    draw::Pen,
    instrument::window::Part,
    paint::Colors,
    popup::Popup,
    studio::{Geometry, Hit, Studio},
    tools::Mods,
};
use gpui::{
    App, Bounds, Context, FocusHandle, IntoElement, MouseButton, Render, Window, WindowControlArea,
    WindowHandle, canvas, div, point, prelude::*, px, size,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use xirius_core::id::PluginId;

pub(crate) use crate::independent::Positions;
use crate::independent::{Key, Kind, Over, Spec};

#[cfg(windows)]
pub(crate) mod drag_shots;
pub(crate) mod input_shots;
#[cfg(windows)]
pub(crate) mod verify_shots;

#[derive(Default)]
pub(crate) struct Windows {
    open: HashMap<PluginId, WindowHandle<DeviceWindow>>,
    channel: Option<WindowHandle<DeviceWindow>>,
    macros: Option<WindowHandle<DeviceWindow>>,
    save: Option<WindowHandle<DeviceWindow>>,
}

impl Windows {
    pub(crate) fn close(&mut self, cx: &mut App) {
        if let Some(handle) = self.save.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
        if let Some(handle) = self.macros.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
        if let Some(handle) = self.channel.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
        for (_, handle) in self.open.drain() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }

    /// Each independent window's kind and handle, in one list.
    fn all(&self) -> Vec<(Kind, WindowHandle<DeviceWindow>)> {
        let mut out: Vec<_> = self
            .open
            .iter()
            .map(|(id, h)| (Kind::Device(*id), *h))
            .collect();
        if let Some(h) = self.channel {
            out.push((Kind::Channel, h));
        }
        if let Some(h) = self.macros {
            out.push((Kind::Macros, h));
        }
        out
    }

    #[cfg(windows)]
    pub(crate) fn contains_hwnd(&self, hwnd: u64, cx: &mut App) -> bool {
        self.all().into_iter().any(|(_, handle)| {
            handle
                .update(cx, |_, window, _| hwnd_of(window) as u64)
                .is_ok_and(|window| window == hwnd)
        })
    }

    /// What is under the pointer, frontmost first (UI-353): the window that would see the
    /// cursor there, not the one that holds the mouse's events while a drag's button is
    /// down. `pos` is where the event said it is (main-local); the answer for an
    /// independent window comes back in its own coordinates.
    pub(crate) fn over(&self, main: &Window, _pos: (f32, f32), cx: &mut App) -> Over {
        let handles = self.all();
        // A headless test's windows are not the OS's, so its build takes the bounds way
        // on every OS.
        #[cfg(all(windows, not(test)))]
        {
            self.over_at_cursor(main, &handles, cx)
        }
        #[cfg(any(not(windows), test))]
        {
            self.over_by_bounds(main, _pos, &handles, cx)
        }
    }

    /// Each tracked window's frame for the position resolution: `screen` answers the
    /// client area's rectangle and how many screen units one of the window's logical
    /// pixels is, in the units `at` comes in.
    #[cfg(any(not(windows), test))]
    fn frames(
        handles: &[(Kind, WindowHandle<DeviceWindow>)],
        cx: &mut App,
        mut screen: impl FnMut(&Window) -> (crate::studio::Rect, f32),
    ) -> Vec<crate::independent::Frame> {
        handles
            .iter()
            .filter_map(|(kind, handle)| {
                handle
                    .update(cx, |_, window, _| {
                        let (rect, scale) = screen(window);
                        crate::independent::Frame {
                            kind: *kind,
                            rect,
                            scale,
                        }
                    })
                    .ok()
            })
            .collect()
    }

    /// Windows: the OS's own frontmost-window answer at the cursor (`WindowFromPoint`),
    /// so a device window behind the main one, or behind another app's, is not mistaken
    /// for being under it. The position inside it comes from the cursor's place on the
    /// screen every time (`ScreenToClient` over the window's scale), never from
    /// `Window::mouse_position` — mouse events go to the window that owns the drag, so
    /// what that window remembers is the stale place its pointer last left.
    #[cfg(all(windows, not(test)))]
    fn over_at_cursor(
        &self,
        main: &Window,
        handles: &[(Kind, WindowHandle<DeviceWindow>)],
        cx: &mut App,
    ) -> Over {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GA_ROOT, GetAncestor, GetCursorPos, WindowFromPoint,
        };
        let mut at = POINT { x: 0, y: 0 };
        // SAFETY: the GUI thread; `at` is a valid out-pointer for the call.
        let top = unsafe {
            if GetCursorPos(&mut at) == 0 {
                return Over::Main;
            }
            WindowFromPoint(at)
        };
        if top.is_null() {
            return Over::Elsewhere;
        }
        // SAFETY: `top` is a live HWND `WindowFromPoint` just returned.
        let root = unsafe { GetAncestor(top, GA_ROOT) } as isize;
        // Each tracked window's OS handle, and its client area in the physical pixels
        // `GetCursorPos` answers with.
        let mut hwnds = Vec::with_capacity(handles.len());
        let mut frames = Vec::with_capacity(handles.len());
        for (kind, handle) in handles {
            let Some(frame) = handle
                .update(cx, |_, window, _| {
                    let scale = window.scale_factor();
                    let mut o = POINT { x: 0, y: 0 };
                    // SAFETY: `hwnd` is this process's live window; `o` out-points it.
                    unsafe { ClientToScreen(hwnd_of(window) as *mut core::ffi::c_void, &mut o) };
                    let size = window.bounds().size;
                    (
                        hwnd_of(window),
                        crate::independent::Frame {
                            kind: *kind,
                            rect: crate::studio::Rect::new(
                                o.x as f32,
                                o.y as f32,
                                f32::from(size.width) * scale,
                                f32::from(size.height) * scale,
                            ),
                            scale,
                        },
                    )
                })
                .ok()
            else {
                continue;
            };
            hwnds.push(frame.0);
            frames.push(frame.1);
        }
        let top = if hwnd_of(main) == root {
            crate::independent::Top::Main
        } else {
            match hwnds.iter().position(|h| *h != 0 && *h == root) {
                Some(i) => crate::independent::Top::Window(i),
                None => crate::independent::Top::Foreign,
            }
        };
        crate::independent::resolve_over((at.x as f32, at.y as f32), top, &frames)
    }

    /// Where there is no frontmost-window query to the OS (macOS, headless tests): the
    /// bounds the open windows last reported answer instead — an independent window in
    /// front of the main one wins the point it covers. `pos` is inside the main
    /// window's body; the screen position it means has the window's origin and its top
    /// bar added back.
    #[cfg(any(not(windows), test))]
    fn over_by_bounds(
        &self,
        main: &Window,
        pos: (f32, f32),
        handles: &[(Kind, WindowHandle<DeviceWindow>)],
        cx: &mut App,
    ) -> Over {
        let rect = |window: &Window| {
            let b = window.bounds();
            (
                crate::studio::Rect::new(
                    f32::from(b.origin.x),
                    f32::from(b.origin.y),
                    f32::from(b.size.width),
                    f32::from(b.size.height),
                ),
                1.0,
            )
        };
        let frames = Self::frames(handles, cx, rect);
        let (main_rect, _) = rect(main);
        let at = (
            main_rect.x + pos.0,
            main_rect.y + pos.1 + xirius_design::Layout::TOP_BAR_H,
        );
        let top = crate::independent::top_by_bounds(at, main_rect, &frames);
        crate::independent::resolve_over(at, top, &frames)
    }

    /// Repaints the window `kind` is, when it is still open (UI-353): what a drag lit
    /// there changes even while the drag's events go to the main window.
    pub(crate) fn notify(&self, kind: Kind, cx: &mut App) {
        let handle = match kind {
            Kind::Device(id) => self.open.get(&id).copied(),
            Kind::Channel => self.channel,
            Kind::Macros => self.macros,
        };
        if let Some(handle) = handle {
            let _ = handle.update(cx, |_, _, cx| cx.notify());
        }
    }

    pub(crate) fn sync(&mut self, studio: &Rc<RefCell<Studio>>, main: &Window, cx: &mut App) {
        self.sync_save(studio, main, cx);
        self.sync_channel(studio, main, cx);
        self.sync_macros(studio, main, cx);
        let wanted: Vec<_> = {
            let s = studio.borrow();
            s.instruments
                .open
                .iter()
                .filter(|w| s.instruments.detached.contains(&w.plugin))
                .copied()
                .collect()
        };
        self.open.retain(|id, handle| {
            if wanted.iter().any(|w| w.plugin == *id) {
                return true;
            }
            let _ = handle.update(cx, |_, window, _| window.remove_window());
            false
        });
        for win in wanted {
            if let Some(handle) = self.open.get(&win.plugin) {
                let handle = *handle;
                // Updating this entity during Screen::render would register it as a
                // main-window dependency. Notify after drawing to avoid a feedback loop.
                cx.defer(move |cx| {
                    let _ = handle.update(cx, |_, _, cx| cx.notify());
                });
                continue;
            }
            let shared = studio.clone();
            let id = win.plugin;
            let title = format!("Xirius · {id}");
            let origin = main.bounds().origin;
            let positions = studio.borrow().instruments.positions.clone();
            let initial = Bounds::new(
                point(
                    origin.x + px(40.0 + 24.0 * win.seat as f32),
                    origin.y + px(60.0 + 24.0 * win.seat as f32),
                ),
                size(px(win.body.0), px(win.body.1 + crate::title_bar::BAR_H)),
            );
            let spec = Spec {
                kind: Kind::Device(id),
                title,
                initial,
                display: None,
                min_size: None,
                resizable: true,
                positions: Some(positions.clone()),
            };
            let result = crate::independent::open(cx, main, spec, move |window, cx| {
                let closing = shared.clone();
                window.on_window_should_close(cx, move |_, _| {
                    let mut s = closing.borrow_mut();
                    s.close_device_view(id);
                    true
                });
                let focus = cx.focus_handle();
                window.focus(&focus, cx);
                let size = window.bounds().size;
                if let Some(w) = shared
                    .borrow_mut()
                    .instruments
                    .open
                    .iter_mut()
                    .find(|w| w.plugin == id)
                {
                    w.body = (
                        size.width.into(),
                        f32::from(size.height) - crate::title_bar::BAR_H,
                    );
                }
                cx.new(|cx| {
                    DeviceWindow::new(
                        shared,
                        Some(id),
                        Some((positions, Key::Device(id))),
                        focus,
                        window,
                        cx,
                    )
                })
            });
            match result {
                Ok(handle) => {
                    self.open.insert(id, handle);
                }
                Err(error) => {
                    let mut s = studio.borrow_mut();
                    s.close_device_view(id);
                    s.error = Some(format!("独立ウィンドウを開けません: {error}"));
                }
            }
        }
        let activate = studio.borrow_mut().instruments.activate_device.take();
        if let Some((id, source)) = activate
            && let Some(&handle) = self.open.get(&id)
            && std::env::var_os(xirius_vst3::OFFSCREEN_ENV).is_none()
        {
            cx.defer(move |cx| {
                if !cfg!(windows) || (source != 0 && Studio::editor_foreground() == source) {
                    let _ = handle.update(cx, |_, window, _| window.activate_window());
                }
            });
        }
    }

    /// Saving keeps the shared library form (folders and instrument effects), separate
    /// from the host's list. This window exists only after an explicit save request.
    fn sync_save(&mut self, studio: &Rc<RefCell<Studio>>, main: &Window, cx: &mut App) {
        let request = studio.borrow_mut().mixer.preset_save.take();
        let Some(id) = request else {
            if let Some(handle) = self.save {
                cx.defer(move |cx| {
                    let _ = handle.update(cx, |_, _, cx| cx.notify());
                });
            }
            return;
        };
        let Some(mut popup) = studio.borrow_mut().popup.take() else {
            return;
        };
        popup.anchor = crate::studio::Rect::new(6.0, 0.0, 280.0, 0.0);
        if let Some(handle) = self.save.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
        let shared = studio.clone();
        let parked = std::env::var_os(xirius_vst3::OFFSCREEN_ENV).is_some();
        let initial = Bounds::new(
            main.bounds().origin + point(px(80.0), px(100.0)),
            size(px(300.0), px(440.0)),
        );
        let result = crate::independent::open_preset_save(cx, initial, move |window, cx| {
            window.resize(initial.size);
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            let entity = cx.new(|cx| {
                let mut view = DeviceWindow::new(shared.clone(), None, None, focus, window, cx);
                view.preset_save = true;
                view.popup = Some(popup);
                view
            });
            #[cfg(windows)]
            {
                use windows_sys::Win32::UI::WindowsAndMessaging::*;
                let hwnd = hwnd_of(window) as windows_sys::Win32::Foundation::HWND;
                if hwnd.is_null() {
                    return entity;
                }
                let owner = shared.borrow().mixer.editor_hwnd(id).unwrap_or(0);
                // SAFETY: only this new dialog is changed. The external frame remains
                // unowned; it never waits on the main window's GUI thread.
                unsafe {
                    if IsWindow(owner as _) != 0 {
                        SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, owner);
                    }
                    if parked {
                        SetWindowPos(
                            hwnd,
                            std::ptr::null_mut(),
                            -30000,
                            -30000,
                            0,
                            0,
                            SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER,
                        );
                        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                    } else {
                        ShowWindow(hwnd, SW_SHOW);
                        SetForegroundWindow(hwnd);
                    }
                }
            }
            #[cfg(not(windows))]
            {
                let _ = (id, parked);
                window.activate_window();
            }
            entity
        });
        match result {
            Ok(handle) => self.save = Some(handle),
            Err(error) => studio.borrow_mut().error = Some(error.to_string()),
        }
    }

    /// The Macro Controls: a window of a fixed size, to the right of the Channel Editor.
    fn sync_macros(&mut self, studio: &Rc<RefCell<Studio>>, main: &Window, cx: &mut App) {
        let wanted = studio.borrow().macro_pad.is_some();
        if !wanted {
            if let Some(handle) = self.macros.take() {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            }
            return;
        }
        if let Some(handle) = self.macros {
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, _, cx| cx.notify());
            });
            return;
        }
        let shared = studio.clone();
        let positions = studio.borrow().instruments.positions.clone();
        let (w, h) = (crate::macro_pad::W, crate::macro_pad::H);
        // To the right of the Channel Editor where it is; else near the main window's corner.
        // A place the person moved it to wins (`independent::open` takes the remembered one).
        let origin = positions
            .borrow()
            .get(&Key::Channel)
            .map(|b| point(b.origin.x + b.size.width + px(MACROS_GAP), b.origin.y))
            .unwrap_or_else(|| main.bounds().origin + point(px(120.0), px(100.0)));
        let spec = Spec {
            kind: Kind::Macros,
            title: "Xirius · Macro Controls".into(),
            initial: Bounds::new(origin, size(px(w), px(h))),
            display: None,
            min_size: None,
            resizable: false,
            positions: Some(positions.clone()),
        };
        let result = crate::independent::open(cx, main, spec, move |window, cx| {
            let closing = shared.clone();
            window.on_window_should_close(cx, move |_, _| {
                closing.borrow_mut().macro_pad = None;
                true
            });
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            cx.new(|cx| {
                let mut view = DeviceWindow::new(
                    shared,
                    None,
                    Some((positions, Key::Macros)),
                    focus,
                    window,
                    cx,
                );
                view.macros = true;
                view
            })
        });
        match result {
            Ok(handle) => self.macros = Some(handle),
            Err(error) => {
                let mut s = studio.borrow_mut();
                s.macro_pad = None;
                s.error = Some(format!("独立ウィンドウを開けません: {error}"));
            }
        }
    }

    fn sync_channel(&mut self, studio: &Rc<RefCell<Studio>>, main: &Window, cx: &mut App) {
        let wanted = studio.borrow().channel.is_some();
        if !wanted {
            if let Some(handle) = self.channel.take() {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            }
            return;
        }
        if let Some(handle) = self.channel {
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, _, cx| cx.notify());
            });
            return;
        }
        let shared = studio.clone();
        let origin = main.bounds().origin;
        let positions = studio.borrow().instruments.positions.clone();
        // It opens at its content's size and the bar's height; a size the person changed wins
        // (`independent::open` takes the remembered one).
        let (least, natural) = {
            let s = studio.borrow();
            let p = s.project();
            s.channel
                .as_ref()
                .map(|w| {
                    (
                        crate::channel::min_width(&p, w),
                        crate::channel::natural_size(&p, w),
                    )
                })
                .unwrap_or((crate::channel::MIN_W, None))
        };
        let natural = natural.unwrap_or((800.0, 600.0));
        let beside = studio.borrow().channel.as_ref().and_then(|w| w.beside);
        let remembered = positions.borrow().get(&Key::Channel).map(|b| b.size);
        let beside = beside.and_then(|id| {
            let extent = remembered.unwrap_or_else(|| size(px(natural.0), px(natural.1)));
            self.channel_placement(studio, cx, id, extent, size(px(least), px(CHANNEL_MIN_H)))
        });
        let spec = Spec {
            kind: Kind::Channel,
            title: "Xirius · Channel Editor".into(),
            initial: beside.map_or_else(
                || {
                    Bounds::new(
                        point(origin.x + px(60.0), origin.y + px(80.0)),
                        size(px(natural.0), px(natural.1)),
                    )
                },
                |(bounds, _)| bounds,
            ),
            display: beside.map(|(_, display)| display),
            min_size: Some(size(px(least), px(CHANNEL_MIN_H))),
            resizable: true,
            positions: beside.is_none().then(|| positions.clone()),
        };
        let result = crate::independent::open(cx, main, spec, move |window, cx| {
            let closing = shared.clone();
            window.on_window_should_close(cx, move |_, _| {
                let mut s = closing.borrow_mut();
                s.channel_note_off();
                s.channel = None;
                true
            });
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            cx.new(|cx| {
                DeviceWindow::new(
                    shared,
                    None,
                    Some((positions, Key::Channel)),
                    focus,
                    window,
                    cx,
                )
            })
        });
        match result {
            Ok(handle) => self.channel = Some(handle),
            Err(error) => {
                let mut s = studio.borrow_mut();
                s.channel_note_off();
                s.channel = None;
                s.error = Some(format!("独立ウィンドウを開けません: {error}"));
            }
        }
    }

    fn channel_placement(
        &self,
        studio: &Rc<RefCell<Studio>>,
        cx: &mut App,
        id: PluginId,
        extent: gpui::Size<gpui::Pixels>,
        minimum: gpui::Size<gpui::Pixels>,
    ) -> Option<(Bounds<gpui::Pixels>, gpui::DisplayId)> {
        if std::env::var_os(xirius_vst3::OFFSCREEN_ENV).is_some() {
            return None;
        }
        if let Some(handle) = self.open.get(&id) {
            return handle
                .update(cx, |_, window, cx| {
                    let display = window.display(cx)?;
                    Some((
                        beside_bounds(window.bounds(), display.visible_bounds(), extent, minimum),
                        display.id(),
                    ))
                })
                .ok()
                .flatten();
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::Foundation::RECT;
            use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromWindow};
            use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;
            let hwnd = studio.borrow().mixer.editor_hwnd(id)? as _;
            let mut r = RECT::default();
            // SAFETY: read-only position and display queries for a reported editor handle.
            let screen = unsafe {
                if GetWindowRect(hwnd, &mut r) == 0 {
                    return None;
                }
                let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                crate::independent::screens(cx)
                    .into_iter()
                    .find(|screen| u64::from(screen.id) as usize == monitor as usize)?
            };
            let bounds = |[l, t, r, b]: [i32; 4]| {
                Bounds::new(
                    point(px(l as f32 / screen.scale), px(t as f32 / screen.scale)),
                    size(
                        px((r - l) as f32 / screen.scale),
                        px((b - t) as f32 / screen.scale),
                    ),
                )
            };
            let source = bounds([r.left, r.top, r.right, r.bottom]);
            // Leave room for the native resize border outside the client area.
            let border = (8.0 * screen.scale).ceil() as i32;
            let [left, top, right, bottom] = screen.work;
            let work = bounds([left + border, top + border, right - border, bottom - border]);
            Some((beside_bounds(source, work, extent, minimum), screen.id))
        }
        #[cfg(not(windows))]
        {
            let _ = studio;
            None
        }
    }
}

fn beside_bounds(
    source: Bounds<gpui::Pixels>,
    work: Bounds<gpui::Pixels>,
    extent: gpui::Size<gpui::Pixels>,
    minimum: gpui::Size<gpui::Pixels>,
) -> Bounds<gpui::Pixels> {
    let gap = px(8.0);
    let right = (source.right() + gap).max(work.left());
    let left = (source.left() - gap).min(work.right());
    let below = (source.bottom() + gap).max(work.top());
    let above = (source.top() - gap).min(work.bottom());
    let clear = [
        Bounds::new(
            point(right, work.top()),
            size(work.right() - right, work.size.height),
        ),
        Bounds::new(work.origin, size(left - work.left(), work.size.height)),
        Bounds::new(
            point(work.left(), below),
            size(work.size.width, work.bottom() - below),
        ),
        Bounds::new(work.origin, size(work.size.width, above - work.top())),
    ];
    let options = [
        point(right, source.top()),
        point(left - extent.width, source.top()),
        point(source.left(), below),
        point(source.left(), above - extent.height),
    ];
    // Preserve the desired size whenever it fits. A smaller clear side is preferable
    // to covering the editor, as long as the channel's controls still fit.
    for required in [extent, minimum] {
        for (origin, area) in options.into_iter().zip(clear) {
            if area.size.width >= required.width && area.size.height >= required.height {
                return crate::independent::fit_bounds(Bounds::new(origin, extent), area);
            }
        }
    }
    let mut best = None;
    for origin in options {
        let fitted = crate::independent::fit_bounds(Bounds::new(origin, extent), work);
        let width =
            (source.right().min(fitted.right()) - source.left().max(fitted.left())).max(px(0.0));
        let height =
            (source.bottom().min(fitted.bottom()) - source.top().max(fitted.top())).max(px(0.0));
        let overlap = f32::from(width) * f32::from(height);
        if best.is_none_or(|(_, area)| overlap < area) {
            best = Some((fitted, overlap));
        }
    }
    best.expect("four candidate sides").0
}

/// The Channel Editor's least height (the toolbar and a little of the flow under the bar).
const CHANNEL_MIN_H: f32 = 240.0;
/// The gap between the Channel Editor and the Macro Controls opened beside it.
const MACROS_GAP: f32 = 16.0;

struct DeviceWindow {
    studio: Rc<RefCell<Studio>>,
    id: Option<PluginId>,
    focus: FocusHandle,
    popup: Option<Popup>,
    preset_save: bool,
    /// The Macro Controls (`id` is `None` as for the Channel Editor).
    macros: bool,
    /// Files from another app are over this window (their light is cleaned on exit).
    files_dragged: bool,
    _activation: gpui::Subscription,
    _bounds: gpui::Subscription,
}

fn surface_geometry(s: &Studio, width: f32, height: f32) -> Geometry {
    s.settled_geometry(width, height)
}

/// The work area wins when a device's natural minimum is larger than the display.
/// Use this after fitting native input as well as before requesting a resize.
fn bounded_body(body: (f32, f32), work: Option<(f32, f32)>) -> (f32, f32) {
    work.map_or(body, |(width, height)| {
        (
            body.0.min(width),
            body.1.min((height - crate::title_bar::BAR_H).max(0.0)),
        )
    })
}

fn work_size(window: &Window, cx: &App) -> Option<(f32, f32)> {
    if std::env::var_os(xirius_vst3::OFFSCREEN_ENV).is_some() {
        return None;
    }
    window.display(cx).map(|display| {
        let work = display.visible_bounds();
        (work.size.width.into(), work.size.height.into())
    })
}

type Scoped = (Option<PluginId>, bool, bool);

fn scope(s: &mut Studio, id: Option<PluginId>) -> Scoped {
    scope_as(s, id, false)
}

/// `macros`: the surface is the Macro Controls (`id` is `None`, which is not the Channel Editor).
fn scope_as(s: &mut Studio, id: Option<PluginId>, macros: bool) -> Scoped {
    let old = (
        s.instruments.detached_target,
        s.instruments.detached_channel,
        s.instruments.detached_macros,
    );
    s.instruments.detached_target = id;
    s.instruments.detached_channel = id.is_none() && !macros;
    s.instruments.detached_macros = macros;
    old
}

fn restore(s: &mut Studio, old: Scoped) {
    s.instruments.detached_target = old.0;
    s.instruments.detached_channel = old.1;
    s.instruments.detached_macros = old.2;
}

/// Runs `f` with the Studio scoped to `id`'s independent surface: the pointer paths
/// then see its frame exactly as the real `DeviceWindow`'s render does.
pub(crate) fn with_surface<R>(
    s: &mut Studio,
    id: PluginId,
    f: impl FnOnce(&mut Studio, &Geometry) -> R,
) -> R {
    let body = s
        .instruments
        .open
        .iter()
        .find(|w| w.plugin == id)
        .unwrap()
        .body;
    let old = scope(s, Some(id));
    let g = surface_geometry(s, body.0, body.1 + crate::title_bar::BAR_H);
    let result = f(s, &g);
    restore(s, old);
    result
}

/// The OS's own handle of a GPUI window, as `WindowFromPoint` reports it.
#[cfg(windows)]
pub(crate) fn hwnd_of(window: &Window) -> isize {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(h) = HasWindowHandle::window_handle(window) else {
        return 0;
    };
    match h.as_raw() {
        RawWindowHandle::Win32(h) => h.hwnd.get(),
        _ => 0,
    }
}

impl DeviceWindow {
    fn new(
        studio: Rc<RefCell<Studio>>,
        id: Option<PluginId>,
        place: Option<(Positions, Key)>,
        focus: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let activation = cx.observe_window_activation(window, |view, window, cx| {
            if view.preset_save {
                return;
            }
            if !window.is_window_active() {
                let changed = view.with(window, |s, _| {
                    if s.instruments.detached_macros {
                        // the keys went to another window: the pad lets them go
                        if let Some(w) = s.macro_pad.as_mut() {
                            w.focused = false;
                        }
                        false
                    } else if s.instruments.detached_channel {
                        s.channel_note_off()
                    } else {
                        s.device_focus_lost()
                    }
                });
                if changed {
                    cx.refresh_windows();
                }
            }
        });
        let bounds = cx.observe_window_bounds(window, move |view, window, cx| {
            if let Some((positions, key)) = &place {
                positions.borrow_mut().insert(*key, window.bounds());
            }
            if let Some(id) = view.id {
                let viewport = window.viewport_size();
                let mut s = view.studio.borrow_mut();
                let p = s.project();
                let definition = s
                    .instruments
                    .open
                    .iter()
                    .find(|w| w.plugin == id)
                    .and_then(|w| crate::instrument::window::live(&p, w))
                    .map(|(_, plugin, def)| (plugin, def));
                if let Some((plugin, def)) = definition {
                    let body = bounded_body(
                        crate::instrument::window::fit_of(
                            def,
                            (
                                viewport.width.into(),
                                f32::from(viewport.height) - crate::title_bar::BAR_H,
                            ),
                            Some(plugin),
                        ),
                        work_size(window, cx),
                    );
                    if let Some(w) = s.instruments.open.iter_mut().find(|w| w.plugin == id) {
                        w.body = body;
                    }
                    s.instruments.sizes.insert(def.uid, body);
                }
            }
            cx.notify();
        });
        Self {
            studio,
            id,
            focus,
            popup: None,
            preset_save: false,
            macros: false,
            files_dragged: false,
            _activation: activation,
            _bounds: bounds,
        }
    }
    /// Input and painting share the very same Win. Only the coordinate system and
    /// popup belong to this surface; no song or audio instance is copied.
    fn with<R>(&mut self, window: &Window, f: impl FnOnce(&mut Studio, &Geometry) -> R) -> R {
        let mut s = self.studio.borrow_mut();
        let old = scope_as(&mut s, self.id, self.macros);
        std::mem::swap(&mut s.popup, &mut self.popup);
        let viewport = window.viewport_size();
        // This surface has its own viewport. Main-window geometry owns the shared
        // ViewMotion keys and viewport_size; using it here restarts those transitions.
        let g = surface_geometry(&s, viewport.width.into(), viewport.height.into());
        let result = f(&mut s, &g);
        std::mem::swap(&mut s.popup, &mut self.popup);
        restore(&mut s, old);
        result
    }

    /// Whether the device this window shows is one files drop onto (UI-333, UI-334): the
    /// Channel Editor and the Macro Controls are not. Without this a file
    /// let go on any other detached window would fall through to the song's lanes, whose
    /// geometry is the main window's, not this surface's.
    fn takes_files(&self) -> bool {
        if self.preset_save {
            return false;
        }
        let Some(id) = self.id else {
            return false;
        };
        let s = self.studio.borrow();
        let p = s.project();
        s.instruments
            .open
            .iter()
            .find(|w| w.plugin == id)
            .and_then(|w| crate::instrument::window::live(&p, w))
            .is_some_and(|(_, _, def)| crate::device::sampler::has_files(def))
    }

    /// Files dragged over the window: the light of what a drop there would do.
    fn files_over(
        &mut self,
        ev: &gpui::DragMoveEvent<gpui::ExternalPaths>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.files_dragged = true;
        let paths = ev.drag(cx).paths();
        let (x, y) = (
            f32::from(ev.event.position.x),
            f32::from(ev.event.position.y),
        );
        if self.with(window, |s, g| {
            s.files_moved(g, x, y, paths, ev.event.modifiers.shift)
        }) {
            // What was dropped on or told lives on the main window: every surface redraws.
            cx.refresh_windows();
        }
    }

    /// Files dropped on the window (UI-333, UI-334): the samples of the device it shows.
    fn files_dropped(
        &mut self,
        paths: &gpui::ExternalPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.files_dragged = false;
        let position = window.mouse_position();
        let (x, y) = (f32::from(position.x), f32::from(position.y));
        let shift = window.modifiers().shift;
        self.with(window, |s, g| {
            if s.input_blocked() {
                s.drop_hover(false);
            } else {
                s.files_landed(g, x, y, paths.paths(), shift);
            }
        });
        // The word of what was taken (and what was not) is the main window's toast.
        cx.refresh_windows();
    }

    /// Shift pressed or let go while files are dragged over the window: what they would
    /// do changes where the pointer is (N3-kit-1).
    fn drag_modifiers(
        &mut self,
        ev: &gpui::ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.files_dragged {
            return;
        }
        if self.with(window, |s, g| {
            if s.input_blocked() {
                false
            } else {
                s.drag_shift(g, ev.modifiers.shift)
            }
        }) {
            cx.refresh_windows();
        }
    }
}

impl Render for DeviceWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Files dragged out of the window again leave no light behind.
        if self.files_dragged && !cx.has_active_drag() {
            self.files_dragged = false;
            self.studio.borrow_mut().drop_hover(false);
        }
        let live = if self.preset_save {
            self.popup.is_some()
        } else if self.macros {
            self.studio.borrow().macro_pad.is_some()
        } else {
            let s = self.studio.borrow();
            self.id.map_or_else(
                || s.channel.is_some(),
                |id| s.instruments.detached.contains(&id) && s.instruments.is_open(id),
            )
        };
        if !live {
            window.defer(cx, |window, _| window.remove_window());
        }
        if self.macros {
            // The window is exactly its panel: whatever size the system gave it, the surface
            // is brought to the content's.
            let wanted = size(px(crate::macro_pad::W), px(crate::macro_pad::H));
            if window.viewport_size() != wanted {
                window.resize(wanted);
            }
        }
        if self.id.is_none() && !self.macros && !self.preset_save {
            // The least width follows the layers: one added while it is open widens the least.
            let least = {
                let s = self.studio.borrow();
                s.channel
                    .as_ref()
                    .map(|w| crate::channel::min_width(&s.project(), w))
            };
            let now = window.viewport_size();
            if let Some(least) = least
                && f32::from(now.width) < least
            {
                window.resize(size(px(least), now.height));
            }
        }
        {
            let mut s = self.studio.borrow_mut();
            if let Some(w) = s
                .instruments
                .open
                .iter_mut()
                .find(|w| Some(w.plugin) == self.id)
            {
                w.body = bounded_body(w.body, work_size(window, cx));
                let wanted = size(px(w.body.0), px(w.body.1 + crate::title_bar::BAR_H));
                if window.viewport_size() != wanted {
                    window.resize(wanted);
                }
            }
        }
        // Main-window state updates and local input invalidate this surface.
        // An idle editor must not run an independent perpetual render loop.
        let entity = cx.entity();
        let body = canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| {
                entity.update(cx, |view, cx| {
                    let theme = view.studio.borrow().caption.map_or_else(
                        || view.studio.borrow().wanted_theme(crate::theme::THEME),
                        |c| {
                            if c.dark {
                                xirius_design::Theme::Dark
                            } else {
                                xirius_design::Theme::Light
                            }
                        },
                    );
                    let colors = Colors::new(theme);
                    let mut s = view.studio.borrow_mut();
                    let old = scope_as(&mut s, view.id, view.macros);
                    std::mem::swap(&mut s.popup, &mut view.popup);
                    let viewport = window.viewport_size();
                    let g = surface_geometry(&s, viewport.width.into(), viewport.height.into());
                    let mut pen = Pen {
                        window,
                        cx,
                        c: &colors,
                        origin: (0.0, 0.0),
                    };
                    if view.preset_save {
                        // The shared preset form below is the complete content.
                    } else if view.macros {
                        crate::macro_pad::paint::paint(&mut pen, &s);
                    } else if view.id.is_some() {
                        crate::instrument::window::paint(&mut pen, &s, &g);
                    } else {
                        crate::channel::paint::paint(&mut pen, &s, &g);
                    }
                    if let Some(p) = &s.popup {
                        let hot = match s.hover {
                            Hit::Item(i) => Some(i),
                            _ => None,
                        };
                        crate::popup::paint(&mut pen, &p.layout(g.size), hot);
                    }
                    if s.ime_target().is_some() {
                        pen.window.handle_input(
                            &view.focus,
                            gpui::ElementInputHandler::new(bounds, entity.clone()),
                            pen.cx,
                        );
                    }
                    std::mem::swap(&mut s.popup, &mut view.popup);
                    restore(&mut s, old);
                });
            },
        )
        .size_full();
        // The bar's empty parts, which the system moves the window by (UI-337). They come from the
        // very functions the press hit test uses, so the two cannot drift.
        let drags = if self.preset_save {
            Vec::new()
        } else {
            self.with(window, |s, g| {
                if s.instruments.detached_macros {
                    s.macro_view()
                        .map(|v| crate::macro_pad::drag_rects(&v))
                        .unwrap_or_default()
                } else if s.instruments.detached_channel {
                    s.channel_view(g)
                        .map(|v| crate::channel::drag_rects(&v))
                        .unwrap_or_default()
                } else {
                    s.instruments.drag_rects(&s.auto_shown(), g)
                }
            })
        };
        let background = {
            let s = self.studio.borrow();
            let theme = s.caption.map_or_else(
                || s.wanted_theme(crate::theme::THEME),
                |c| {
                    if c.dark {
                        xirius_design::Theme::Dark
                    } else {
                        xirius_design::Theme::Light
                    }
                },
            );
            Colors::new(theme).surface_1
        };
        // The marks of what this window shows (Macro Controls, a device, the Channel Editor) belong
        // to its own tree, in its own coordinates.
        let marks = if self.preset_save {
            Vec::new()
        } else if self.macros {
            crate::macro_pad::marks(&self.studio.borrow())
        } else if self.id.is_some() {
            self.with(window, |s, g| crate::a11y::device_marks(s, g))
        } else {
            self.with(window, |s, g| crate::channel::marks(s, g))
        };
        div()
            .size_full()
            .bg(background)
            .track_focus(&self.focus)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &gpui::MouseDownEvent, window, cx| {
                    let (x, y) = (f32::from(e.position.x), f32::from(e.position.y));
                    let drag = this.with(window, |s, g| {
                        if let Some(p) = &s.popup {
                            let hit = p
                                .layout(g.size)
                                .cells
                                .iter()
                                .position(|c| c.rect.contains(x, y));
                            s.press(g, hit.map_or(Hit::None, Hit::Item));
                            return false;
                        }
                        if s.instruments.detached_macros {
                            let head = s
                                .macro_view()
                                .and_then(|v| crate::macro_pad::part_at(&v, x, y))
                                == Some(crate::macro_pad::Part::Head);
                            if head {
                                return true;
                            }
                            s.macro_press(x, y);
                            return false;
                        }
                        if s.instruments.detached_channel {
                            let head = s
                                .channel_view(g)
                                .and_then(|v| crate::channel::part_at(&v, x, y))
                                == Some(crate::channel::Part::Head);
                            if head {
                                return true;
                            }
                            // A press away from a branch value puts what is typed in; on one it
                            // takes the keys, or starts the pan's drag.
                            s.value_blur(g, x, y);
                            if s.value_press(g, x, y, e.click_count as u32, modifiers(e.modifiers))
                            {
                                return false;
                            }
                            s.channel_press(g, x, y, e.click_count as u32);
                            return false;
                        }
                        if s.instruments
                            .at(&s.auto_shown(), g, x, y)
                            .is_some_and(|(_, part)| part == Part::Head)
                        {
                            return true;
                        }
                        // What a drop on this window said has its button under it (UI-353).
                        if s.notice_press(g, x, y) {
                            return false;
                        }
                        s.instrument_press(g, x, y, e.click_count as u32, modifiers(e.modifiers));
                        false
                    });
                    if drag {
                        if e.click_count >= 2 {
                            // A double click on the bar does nothing: the system would maximise.
                            cx.stop_propagation();
                        } else {
                            window.start_window_move();
                        }
                    }
                    cx.refresh_windows();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, e: &gpui::MouseDownEvent, window, cx| {
                    this.with(window, |s, g| {
                        if s.input_blocked() || s.popup.is_some() {
                            return;
                        }
                        let (x, y) = (e.position.x.into(), e.position.y.into());
                        if s.instruments.detached_macros {
                            s.macro_right(x, y);
                        } else if s.instruments.detached_channel {
                            s.channel_right(g, x, y);
                        } else {
                            s.instrument_right(g, x, y);
                        }
                    });
                    cx.refresh_windows();
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &gpui::MouseMoveEvent, window, cx| {
                this.with(window, |s, g| {
                    let (x, y) = (e.position.x.into(), e.position.y.into());
                    if let Some(p) = &s.popup {
                        s.hover = p
                            .layout(g.size)
                            .cells
                            .iter()
                            .position(|c| c.rect.contains(x, y))
                            .map_or(Hit::None, Hit::Item);
                        return;
                    }
                    if s.instruments.detached_macros {
                        if s.mixer.level_drag.is_some() || s.mixer.key_drag.is_some() {
                            s.level_move(x, e.modifiers.shift);
                            return;
                        }
                        s.macro_moved(x, y);
                        s.macro_hover(x, y);
                        return;
                    }
                    if s.instruments.detached_channel {
                        if s.mixer.level_drag.is_some() || s.mixer.key_drag.is_some() {
                            s.level_move(x, e.modifiers.shift);
                            return;
                        }
                        s.channel_moved(g, x, y);
                        s.channel_hover(g, x, y);
                        return;
                    }
                    s.instrument_moved(g, x, y, e.modifiers.shift);
                    s.instrument_hover(g, x, y);
                });
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseUpEvent, window, cx| {
                    this.with(window, |s, g| {
                        if s.instruments.detached_macros {
                            if !s.level_up() {
                                s.macro_up();
                            }
                        } else if s.instruments.detached_channel {
                            if !s.level_up() {
                                s.channel_up(g);
                            }
                        } else {
                            s.instrument_up();
                        }
                    });
                    cx.refresh_windows();
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseUpEvent, window, cx| {
                    this.with(window, |s, g| {
                        if s.instruments.detached_macros {
                            if !s.level_up() {
                                s.macro_up();
                            }
                        } else if s.instruments.detached_channel {
                            if !s.level_up() {
                                s.channel_up(g);
                            }
                        } else {
                            s.instrument_up();
                        }
                    });
                    cx.refresh_windows();
                }),
            )
            .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                let native = crate::native_key::take();
                let pasted = this.with(window, |s, _| {
                    let k = &e.keystroke;
                    let m = modifiers(k.modifiers);
                    crate::screen::text_key_with_clipboard(s, &k.key, m, cx)
                });
                if pasted {
                    cx.refresh_windows();
                    return;
                }
                this.with(window, |s, g| {
                    device_key_event(s, g, e, native.as_ref());
                });
                cx.refresh_windows();
            }))
            .on_key_up(cx.listener(|this, e: &gpui::KeyUpEvent, window, cx| {
                this.with(window, |s, _| {
                    s.pc_key_up(&e.keystroke.key);
                    s.key_sound_up();
                });
                cx.refresh_windows();
            }))
            .on_scroll_wheel(cx.listener(|this, e: &gpui::ScrollWheelEvent, window, cx| {
                this.with(window, |s, g| {
                    let (x, y) = (e.position.x.into(), e.position.y.into());
                    let delta = e.delta.pixel_delta(px(20.0));
                    if s.popup.is_some() {
                        s.window_wheel(
                            g,
                            (x, y),
                            delta.x.into(),
                            delta.y.into(),
                            modifiers(e.modifiers),
                            e.modifiers.control,
                        );
                        return;
                    }
                    if s.instruments.detached_macros {
                        // the pad has no wheel; the values it holds are set by the pad
                    } else if s.instruments.detached_channel {
                        s.channel_wheel(g, x, y, delta.x.into(), delta.y.into());
                    } else {
                        s.device_wheel(
                            g,
                            x,
                            y,
                            f64::from(if f32::from(delta.x).abs() > f32::from(delta.y).abs() {
                                f32::from(delta.x)
                            } else {
                                f32::from(delta.y)
                            }) / 20.0,
                            e.modifiers.shift,
                        );
                    }
                });
                cx.refresh_windows();
            }))
            .when(self.takes_files(), |d| {
                d.on_drag_move(cx.listener(Self::files_over))
                    .on_drop(cx.listener(Self::files_dropped))
                    .on_modifiers_changed(cx.listener(Self::drag_modifiers))
            })
            .child(body)
            .children(
                marks
                    .into_iter()
                    .map(|m| crate::screen::mark(m, self.studio.clone())),
            )
            .children(drags.into_iter().map(|r| {
                div()
                    .absolute()
                    .left(px(r.x))
                    .top(px(r.y))
                    .w(px(r.w))
                    .h(px(r.h))
                    .window_control_area(WindowControlArea::Drag)
            }))
    }
}

fn device_key_event(
    s: &mut Studio,
    g: &Geometry,
    event: &gpui::KeyDownEvent,
    native: Option<&crate::native_key::Native>,
) {
    let k = &event.keystroke;
    let m = modifiers(k.modifiers);
    if k.key == "escape"
        && s.instruments.detached_channel
        && matches!(
            s.popup.as_ref().map(|p| p.kind),
            Some(crate::popup::PopupKind::Insert(_))
        )
    {
        if !s.pick_typed(g, &k.key, None) {
            s.popup = None;
            s.close_wheel_group();
        }
        return;
    }
    if s.popup.is_none() && s.field_keys(g, &k.key, k.key_char.as_deref(), m) {
        return;
    }
    let field_ime = s.ime_target().is_some();
    if !field_ime
        && s.popup.is_none()
        && k.key == "escape"
        && s.instruments.detached_channel
        && s.channel_escape()
    {
        return;
    }
    // Empty devices have no parameter stop; Tab still belongs to this window.
    if k.key == "tab" && !m.ctrl && !m.alt {
        return;
    }
    // The field's text arrives through the input handler, never as a song shortcut.
    if field_ime && k.key == "space" && !m.ctrl && !m.alt {
        return;
    }
    if field_ime
        && !m.ctrl
        && !m.alt
        && k.key_char
            .as_deref()
            .is_some_and(|c| !c.is_empty() && !c.chars().any(char::is_control))
    {
        return;
    }
    let key = native.and_then(|n| n.key.as_deref()).unwrap_or(&k.key);
    let character = native
        .and_then(|n| n.character.as_deref())
        .or(k.key_char.as_deref());
    let code = native.and_then(|n| n.code.as_deref());
    let keypad = key != k.key;
    if (s.popup.is_none() && s.device_key(g, &k.key, m))
        || (!keypad
            && !m.ctrl
            && s.typed(
                g,
                &k.key,
                if field_ime {
                    None
                } else {
                    k.key_char.as_deref()
                },
            ))
        || (s.popup.is_none()
            && !keypad
            && !m.ctrl
            && !m.alt
            && !m.shift
            && s.pc_key_down(&k.key, event.is_held))
    {
        return;
    }
    // Song commands may rebuild the main layout. They must never use this
    // surface's dimensions or write its viewport into the shared ViewMotion.
    let (width, height) = s.viewport_size.get();
    let main = s.settled_geometry(width, height);
    s.mapped_key_code(&main, key, character, m, code);
}

fn modifiers(m: gpui::Modifiers) -> Mods {
    Mods {
        ctrl: m.control || m.platform,
        shift: m.shift,
        alt: m.alt,
    }
}

impl gpui::EntityInputHandler for DeviceWindow {
    fn text_for_range(
        &mut self,
        range: std::ops::Range<usize>,
        adjusted: &mut Option<std::ops::Range<usize>>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        self.with(window, |s, _| {
            let field = s.ime_field().cloned()?;
            let (text, real) = field.text_for_range(range);
            *adjusted = Some(real);
            Some(text)
        })
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<gpui::UTF16Selection> {
        self.with(window, |s, _| {
            Some(gpui::UTF16Selection {
                range: s.ime_field()?.selected_range(),
                reversed: false,
            })
        })
    }
    fn marked_text_range(
        &self,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<std::ops::Range<usize>> {
        let mut s = self.studio.borrow_mut();
        let old = scope_as(&mut s, self.id, self.macros);
        let previous = std::mem::replace(&mut s.popup, self.popup.clone());
        let result = s.ime_field().cloned().and_then(|f| f.marked_range());
        s.popup = previous;
        restore(&mut s, old);
        result
    }
    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.with(window, |s, _| s.ime_unmark());
        cx.refresh_windows();
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.with(window, |s, _| {
            if !s.input_blocked() {
                s.ime_replace(range, text, false);
            }
        });
        cx.refresh_windows();
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        text: &str,
        _: Option<std::ops::Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.with(window, |s, _| {
            if !s.input_blocked() {
                s.ime_replace(range, text, true);
            }
        });
        cx.refresh_windows();
    }
    fn bounds_for_range(
        &mut self,
        _: std::ops::Range<usize>,
        _: Bounds<gpui::Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<gpui::Pixels>> {
        None
    }
    fn character_index_for_point(
        &mut self,
        _: gpui::Point<gpui::Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

#[cfg(test)]
mod tests;
