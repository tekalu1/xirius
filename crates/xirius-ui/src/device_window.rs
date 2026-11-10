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
    App, Bounds, Context, FocusHandle, IntoElement, MouseButton, Render, TitlebarOptions, Window,
    WindowBounds, WindowDecorations, WindowHandle, WindowOptions, canvas, div, point, prelude::*,
    px, size,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use xirius_core::id::PluginId;

pub(crate) type Positions = Rc<RefCell<HashMap<PluginId, Bounds<gpui::Pixels>>>>;

#[cfg(windows)]
pub(crate) mod drag_shots;
pub(crate) mod input_shots;
#[cfg(windows)]
pub(crate) mod preset_shots;

#[derive(Default)]
pub(crate) struct Windows {
    open: HashMap<PluginId, WindowHandle<DeviceWindow>>,
    channel: Option<WindowHandle<DeviceWindow>>,
    preset: Option<WindowHandle<DeviceWindow>>,
}

impl Windows {
    pub(crate) fn close(&mut self, cx: &mut App) {
        if let Some(handle) = self.preset.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
        if let Some(handle) = self.channel.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
        for (_, handle) in self.open.drain() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }

    pub(crate) fn raise(&self, studio: &Studio) {
        #[cfg(not(windows))]
        let _ = studio;
        #[cfg(windows)]
        for title in studio
            .instruments
            .open
            .iter()
            .filter(|w| self.open.contains_key(&w.plugin))
            .map(|w| format!("Xirius · {}", w.plugin))
            .chain(
                self.channel
                    .iter()
                    .map(|_| "Xirius · Channel Editor".into()),
            )
        {
            if let Some(hwnd) = crate::park::own_handle(&title) {
                use windows_sys::Win32::UI::WindowsAndMessaging::*;
                // SAFETY: windows on this GUI thread; never TOPMOST and never activated.
                unsafe {
                    SetWindowPos(
                        hwnd as _,
                        HWND_TOP,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
                    );
                }
            }
        }
    }
    pub(crate) fn sync(&mut self, studio: &Rc<RefCell<Studio>>, main: &Window, cx: &mut App) {
        // Migrate views created by older entry points without recreating a device.
        {
            let mut s = studio.borrow_mut();
            let ids: Vec<_> = s
                .instruments
                .open
                .iter_mut()
                .map(|w| {
                    w.docked = false;
                    w.plugin
                })
                .collect();
            s.instruments.detached.extend(ids);
        }
        self.sync_channel(studio, main, cx);
        self.sync_preset(studio, main, cx);
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
            let remembered = positions.borrow().get(&id).copied();
            let initial = remembered.unwrap_or_else(|| {
                Bounds::new(
                    point(
                        origin.x + px(40.0 + 24.0 * win.seat as f32),
                        origin.y + px(60.0 + 24.0 * win.seat as f32),
                    ),
                    size(px(win.body.0), px(win.body.1 + crate::title_bar::BAR_H)),
                )
            });
            let initial = if std::env::var_os(xirius_vst3::OFFSCREEN_ENV).is_some() {
                initial
            } else {
                cx.displays()
                    .into_iter()
                    .find(|d| d.bounds().contains(&initial.origin))
                    .or_else(|| main.display(cx))
                    .map_or(initial, |display| {
                        fit_bounds(initial, display.visible_bounds())
                    })
            };
            if let Some(w) = studio
                .borrow_mut()
                .instruments
                .open
                .iter_mut()
                .find(|w| w.plugin == id)
            {
                w.body = (
                    initial.size.width.into(),
                    f32::from(initial.size.height) - crate::title_bar::BAR_H,
                );
            }
            let result = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(initial)),
                    titlebar: Some(TitlebarOptions {
                        title: Some(title.clone().into()),
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    window_decorations: Some(WindowDecorations::Client),
                    focus: false,
                    show: false,
                    is_minimizable: false,
                    is_resizable: true,
                    ..Default::default()
                },
                move |window, cx| {
                    let closing = shared.clone();
                    window.on_window_should_close(cx, move |_, _| {
                        let mut s = closing.borrow_mut();
                        s.close_device_view(id);
                        true
                    });
                    let focus = cx.focus_handle();
                    window.focus(&focus, cx);
                    show(&title);
                    cx.new(|cx| {
                        DeviceWindow::new(shared, Some(id), Some(positions), focus, window, cx)
                    })
                },
            );
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
    }

    fn sync_preset(&mut self, studio: &Rc<RefCell<Studio>>, main: &Window, cx: &mut App) {
        let request = studio.borrow_mut().mixer.preset_menu.take();
        let Some((track, id, x, y, dpi)) = request else {
            if let Some(handle) = self.preset {
                cx.defer(move |cx| {
                    let _ = handle.update(cx, |_, _, cx| cx.notify());
                });
            }
            return;
        };
        if let Some(handle) = self.preset.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
        let scale = dpi.max(96) as f32 / 96.0;
        let mut initial = Bounds::new(
            point(px(x as f32 / scale), px(y as f32 / scale)),
            size(px(332.0), px(420.0)),
        );
        if std::env::var_os(xirius_vst3::OFFSCREEN_ENV).is_none()
            && let Some(display) = cx
                .displays()
                .into_iter()
                .find(|d| d.bounds().contains(&initial.origin))
                .or_else(|| main.display(cx))
        {
            initial = fit_bounds(initial, display.visible_bounds());
        }
        let shared = studio.clone();
        let parked = std::env::var_os(xirius_vst3::OFFSCREEN_ENV).is_some();
        if parked && let Some(display) = main.display(cx) {
            // GPUI replaces an off-display initial rectangle with its defaults.
            // Create hidden on the display, then park before showing it.
            initial.origin = display.visible_bounds().origin;
        }
        let result = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(initial)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Presets".into()),
                    appears_transparent: true,
                    ..Default::default()
                }),
                window_decorations: Some(WindowDecorations::Client),
                focus: !parked,
                show: !parked,
                is_minimizable: false,
                is_resizable: false,
                ..Default::default()
            },
            move |window, cx| {
                let focus = cx.focus_handle();
                window.focus(&focus, cx);
                if parked {
                    show("Presets");
                }
                cx.new(|cx| {
                    let mut view = DeviceWindow::new(shared, Some(id), None, focus, window, cx);
                    view.menu_only = true;
                    view.with(window, |s, _| {
                        s.open_inst_menu(
                            crate::instrument::menu::Menu::Preset(track, id),
                            crate::studio::Rect {
                                x: 6.0,
                                y: 0.0,
                                w: 320.0,
                                h: 0.0,
                            },
                        )
                    });
                    view
                })
            },
        );
        match result {
            Ok(handle) => self.preset = Some(handle),
            Err(error) => studio.borrow_mut().error = Some(error.to_string()),
        }
    }

    fn sync_channel(&mut self, studio: &Rc<RefCell<Studio>>, main: &Window, cx: &mut App) {
        let wanted = studio.borrow().channel.as_ref().is_some_and(|w| w.detached);
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
        let result = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(origin.x + px(60.0), origin.y + px(80.0)),
                    size(px(800.0), px(600.0)),
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Xirius · Channel Editor".into()),
                    appears_transparent: true,
                    ..Default::default()
                }),
                window_decorations: Some(WindowDecorations::Client),
                focus: false,
                show: false,
                window_min_size: Some(size(px(crate::channel::MIN_W), px(240.0))),
                ..Default::default()
            },
            move |window, cx| {
                let closing = shared.clone();
                window.on_window_should_close(cx, move |_, _| {
                    let mut s = closing.borrow_mut();
                    s.channel_note_off();
                    s.channel = None;
                    true
                });
                let focus = cx.focus_handle();
                window.focus(&focus, cx);
                window.resize(size(px(800.0), px(600.0)));
                show("Xirius · Channel Editor");
                cx.new(|cx| DeviceWindow::new(shared, None, None, focus, window, cx))
            },
        );
        match result {
            Ok(handle) => self.channel = Some(handle),
            Err(error) => {
                let mut s = studio.borrow_mut();
                if let Some(w) = s.channel.as_mut() {
                    w.detached = false;
                }
                s.error = Some(format!("独立ウィンドウを開けません: {error}"));
            }
        }
    }
}

fn show(title: &str) {
    #[cfg(not(windows))]
    let _ = title;
    #[cfg(windows)]
    if let Some(hwnd) = crate::park::own_handle(title) {
        if std::env::var_os(xirius_vst3::OFFSCREEN_ENV).is_some() {
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

struct DeviceWindow {
    studio: Rc<RefCell<Studio>>,
    id: Option<PluginId>,
    focus: FocusHandle,
    popup: Option<Popup>,
    menu_only: bool,
    _activation: gpui::Subscription,
    _bounds: gpui::Subscription,
}

fn fit_bounds(
    mut bounds: Bounds<gpui::Pixels>,
    work: Bounds<gpui::Pixels>,
) -> Bounds<gpui::Pixels> {
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

fn scope(s: &mut Studio, id: Option<PluginId>) -> (Option<PluginId>, bool, bool) {
    let old = (
        s.instruments.detached_target,
        s.instruments.detached_channel,
        s.channel.as_ref().is_some_and(|w| w.detached_surface),
    );
    s.instruments.detached_target = id;
    s.instruments.detached_channel = id.is_none();
    if let Some(w) = &mut s.channel {
        w.detached_surface = id.is_none();
        if id.is_none() {
            w.top = true;
        }
    }
    old
}

fn restore(s: &mut Studio, old: (Option<PluginId>, bool, bool)) {
    s.instruments.detached_target = old.0;
    s.instruments.detached_channel = old.1;
    if let Some(w) = &mut s.channel {
        w.detached_surface = old.2;
    }
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

impl DeviceWindow {
    fn new(
        studio: Rc<RefCell<Studio>>,
        id: Option<PluginId>,
        positions: Option<Positions>,
        focus: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let activation = cx.observe_window_activation(window, |view, window, cx| {
            if !window.is_window_active() && view.menu_only {
                view.popup = None;
                cx.notify();
                return;
            }
            if !window.is_window_active() {
                let changed = view.with(window, |s, _| {
                    if s.instruments.detached_channel {
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
            if view.menu_only {
                return;
            }
            if let Some(id) = view.id {
                if let Some(positions) = &positions {
                    positions.borrow_mut().insert(id, window.bounds());
                }
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
            menu_only: false,
            _activation: activation,
            _bounds: bounds,
        }
    }
    /// Input and painting share the very same Win. Only the coordinate system and
    /// popup belong to this surface; no song or audio instance is copied.
    fn with<R>(&mut self, window: &Window, f: impl FnOnce(&mut Studio, &Geometry) -> R) -> R {
        let mut s = self.studio.borrow_mut();
        let old = scope(&mut s, self.id);
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
}

impl Render for DeviceWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let live = if self.menu_only {
            self.popup.is_some()
                && self.id.is_some_and(|id| {
                    self.studio
                        .borrow()
                        .mixer
                        .editors
                        .iter()
                        .any(|e| e.id == id)
                })
        } else {
            let s = self.studio.borrow();
            self.id.map_or_else(
                || s.channel.as_ref().is_some_and(|w| w.detached),
                |id| s.instruments.detached.contains(&id) && s.instruments.is_open(id),
            )
        };
        if !live {
            window.defer(cx, |window, _| window.remove_window());
        }
        if self.menu_only
            && let Some(popup) = &self.popup
        {
            let area = work_size(window, cx).unwrap_or((332.0, 420.0));
            let rect = popup.layout((area.0.min(332.0), area.1.min(420.0))).rect;
            let wanted = size(px(rect.x + rect.w + 6.0), px(rect.y + rect.h + 6.0));
            if window.viewport_size() != wanted {
                window.resize(wanted);
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
                    let old = scope(&mut s, view.id);
                    std::mem::swap(&mut s.popup, &mut view.popup);
                    let viewport = window.viewport_size();
                    let g = surface_geometry(&s, viewport.width.into(), viewport.height.into());
                    let mut pen = Pen {
                        window,
                        cx,
                        c: &colors,
                        origin: (0.0, 0.0),
                    };
                    if view.menu_only {
                        s.refresh_popup();
                    } else if view.id.is_some() {
                        crate::instrument::window::paint(&mut pen, &s, &g);
                    } else {
                        crate::channel::paint::paint(&mut pen, &s, &g, true);
                    }
                    paint_tip(&mut pen, &s, &g);
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
        let shared = self.studio.clone();
        let owner = cx.entity().downgrade();
        let marks = self.with(window, |s, g| {
            accessible_for(&shared, s, g, 0.0, Some(owner))
        });
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
                        if s.instruments.detached_channel {
                            let head = s
                                .channel_view(g)
                                .and_then(|v| crate::channel::part_at(&v, x, y))
                                == Some(crate::channel::Part::Head);
                            if head {
                                return true;
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
                        s.instrument_press(g, x, y, e.click_count as u32, modifiers(e.modifiers));
                        false
                    });
                    if drag {
                        window.start_window_move();
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
                        if s.instruments.detached_channel {
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
                    if s.instruments.detached_channel {
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
                        if s.instruments.detached_channel {
                            s.channel_up(g);
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
                        if s.instruments.detached_channel {
                            s.channel_up(g);
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
                    if s.ime_pick_field().is_some() && m.ctrl && !m.alt && k.key == "v" {
                        if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
                            s.pick_paste(&text);
                        }
                        true
                    } else {
                        false
                    }
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
                    if s.instruments.detached_channel {
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
            .child(body)
            .children(marks)
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
    if s.popup.is_none() && s.field_keys(g, &k.key, k.key_char.as_deref(), m) {
        return;
    }
    let picking = s.ime_pick_field().is_some();
    if !picking
        && s.popup.is_none()
        && k.key == "escape"
        && s.instruments.detached_channel
        && s.channel
            .as_ref()
            .is_some_and(|w| w.mouse_note.is_some() || w.keyboard_drag.is_some())
    {
        s.channel_note_off();
        return;
    }
    // Empty devices have no parameter stop; Tab still belongs to this window.
    if k.key == "tab" && !m.ctrl && !m.alt {
        return;
    }
    // Search text arrives through the input handler, never as a song shortcut.
    if picking && k.key == "space" && !m.ctrl && !m.alt {
        return;
    }
    if picking
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
                if picking { None } else { k.key_char.as_deref() },
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
            let field = s.ime_pick_field().or_else(|| s.ime_field().cloned())?;
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
                range: s
                    .ime_pick_field()
                    .or_else(|| s.ime_field().cloned())?
                    .selected_range(),
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
        let old = scope(&mut s, self.id);
        let previous = std::mem::replace(&mut s.popup, self.popup.clone());
        let result = s
            .ime_pick_field()
            .or_else(|| s.ime_field().cloned())
            .and_then(|f| f.marked_range());
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

pub(crate) fn accessible(
    studio: &Rc<RefCell<Studio>>,
    g: &Geometry,
) -> Vec<gpui::Stateful<gpui::Div>> {
    accessible_for(
        studio,
        &studio.borrow(),
        g,
        xirius_design::Layout::TOP_BAR_H,
        None,
    )
}

fn accessible_for(
    shared: &Rc<RefCell<Studio>>,
    s: &Studio,
    g: &Geometry,
    offset: f32,
    owner: Option<gpui::WeakEntity<DeviceWindow>>,
) -> Vec<gpui::Stateful<gpui::Div>> {
    let p = s.auto_shown();
    let mut places = Vec::new();
    for (i, w) in s.instruments.open.iter().enumerate() {
        let r = s.instruments.frame(i, g);
        if r.w <= 0.0 {
            continue;
        }
        if let Some((track, plugin, def)) = crate::instrument::window::live(&p, w) {
            let destination = s
                .instruments
                .part_rect(r, &p, track, plugin, def, Part::Destination);
            if destination.w > 0.0 {
                places.push((
                    crate::instrument::menu::Menu::Destination(w.track, w.plugin),
                    destination,
                ));
            }
        }
    }
    if s.instruments.detached_target.is_none()
        && let Some(v) = s.channel_view(g)
        && let Some(w) = &s.channel
    {
        places.push((
            crate::instrument::menu::Menu::ChannelDestination(w.track),
            v.bar.destination,
        ));
    }
    places
        .into_iter()
        .map(|(menu, b)| {
            let shared = shared.clone();
            let owner = owner.clone();
            div()
                .id(gpui::SharedString::from(format!("destination-{menu:?}")))
                .absolute()
                .left(px(b.x))
                .top(px(b.y + offset))
                .w(px(b.w))
                .h(px(b.h))
                .role(gpui::accesskit::Role::Button)
                .aria_label("表示先")
                .on_a11y_action(gpui::accesskit::Action::Click, move |_, window, cx| {
                    if let Some(owner) = &owner {
                        let _ = owner.update(cx, |view, cx| {
                            view.with(window, |s, _| s.open_inst_menu(menu, b));
                            cx.refresh_windows();
                        });
                    } else {
                        shared.borrow_mut().open_inst_menu(menu, b);
                        cx.refresh_windows();
                    }
                })
        })
        .collect()
}

pub(crate) fn paint_tip(pen: &mut Pen, s: &Studio, g: &Geometry) {
    if s.popup.is_some() {
        return;
    }
    if s.instruments.detached_target.is_none()
        && s.channel
            .as_ref()
            .is_some_and(|w| w.hot == Some(crate::channel::Part::Destination))
        && let Some(v) = s.channel_view(g)
    {
        crate::hover_tip::balloon(
            pen,
            "表示先",
            v.bar.destination,
            crate::studio::Rect::new(0.0, 0.0, g.size.0, g.size.1),
        );
        return;
    }
    let Some((id, Part::Destination)) = s.instruments.hot else {
        return;
    };
    let Some(i) = s.instruments.open.iter().position(|w| w.plugin == id) else {
        return;
    };
    let r = s.instruments.frame(i, g);
    if r.w <= 0.0 {
        return;
    }
    let p = s.auto_shown();
    if let Some((track, plugin, def)) = crate::instrument::window::live(&p, &s.instruments.open[i])
    {
        let b = s
            .instruments
            .part_rect(r, &p, track, plugin, def, Part::Destination);
        crate::hover_tip::balloon(
            pen,
            "表示先",
            b,
            crate::studio::Rect::new(0.0, 0.0, g.size.0, g.size.1),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_work_area_smaller_than_the_device_minimum_is_a_stable_resize_target() {
        let def = &xirius_params::SAMPLER;
        let work = Some((200.0, 150.0));
        let mut body = (800.0, 600.0);
        for _ in 0..12 {
            let rendered = bounded_body(body, work);
            let observed =
                bounded_body(crate::instrument::window::fit_of(def, rendered, None), work);
            assert_eq!(rendered, observed);
            body = observed;
        }
    }

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
    fn an_independent_device_can_grow_beyond_its_current_viewport() {
        use crate::instrument::{Pick, current, window::Part};
        let mut s = Studio::sample();
        let track = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Keys")
            .unwrap()
            .id;
        assert!(s.set_instrument(track, &Pick::built_in(crate::instrument::DEFAULT)));
        let id = current(&s.project().tracks[&track]).unwrap().id;
        s.show_instrument(track, id);
        let before = s.instruments.open[0].body;
        let old = scope(&mut s, Some(id));
        let g = surface_geometry(&s, before.0, before.1 + crate::title_bar::BAR_H);
        let (x, y) = s.instrument_center(&g, id, Part::Resize).unwrap();
        assert!(s.instrument_down(&g, x, y));
        assert!(s.instrument_moved(&g, x + 120.0, y + 80.0, false));
        s.instrument_up();
        assert!(s.instruments.open[0].body.0 > before.0);
        assert!(s.instruments.open[0].body.1 > before.1);
        restore(&mut s, old);
    }

    #[test]
    fn independent_surfaces_leave_main_viewport_and_animation_targets_untouched() {
        let mut s = Studio::sample();
        s.motion.enabled.set(true);
        let main = s.geometry(1280.0, 800.0);
        // Start a real panel transition before opening differently sized editors.
        s.browser.open = !s.browser.open;
        s.geometry(1280.0, 800.0);
        let motion = format!("{:?}", s.motion);
        let viewport = s.viewport_size.get();
        let id = PluginId(1);
        for _ in 0..6 {
            for (target, width, height) in [
                (Some(id), 360.0, 440.0),
                (None, 900.0, 600.0),
                (Some(id), 750.0, 720.0),
            ] {
                let old = scope(&mut s, target);
                let local = surface_geometry(&s, width, height);
                assert_eq!(local.size, (width, height));
                restore(&mut s, old);
                assert_eq!(s.viewport_size.get(), viewport);
                assert_eq!(format!("{:?}", s.motion), motion);
                assert_eq!(s.instruments.detached_target, None);
                assert!(!s.instruments.detached_channel);
            }
        }
        assert_eq!(s.geometry(1280.0, 800.0).size, main.size);
        assert_eq!(format!("{:?}", s.motion), motion);
    }

    #[test]
    fn independent_shortcuts_do_not_inherit_the_main_browser_context() {
        let mut s = Studio::sample();
        let main = s.geometry(1280.0, 800.0);
        s.browser.open = true;
        s.input_map.browser_focus = true;
        assert_eq!(s.input_context(&main), "browser");
        let old = scope(&mut s, Some(PluginId(1)));
        assert_eq!(s.input_context(&main), "global");
        restore(&mut s, old);
        assert_eq!(s.input_context(&main), "browser");
    }

    #[test]
    fn tab_in_an_empty_device_does_not_advance_main_focus_or_layout() {
        let mut s = Studio::sample();
        s.geometry(1280.0, 800.0);
        s.input_map.browser_focus = true;
        let viewport = s.viewport_size.get();
        let motion = format!("{:?}", s.motion);
        let old = scope(&mut s, Some(PluginId(1)));
        let local = surface_geometry(&s, 360.0, 240.0);
        for key in ["tab", "shift-tab"] {
            device_key_event(
                &mut s,
                &local,
                &gpui::KeyDownEvent {
                    keystroke: gpui::Keystroke::parse(key).unwrap(),
                    is_held: false,
                    prefer_character_input: false,
                },
                None,
            );
            assert!(s.input_map.browser_focus);
            assert_eq!(s.instruments.focus, None);
            assert_eq!(s.viewport_size.get(), viewport);
            assert_eq!(format!("{:?}", s.motion), motion);
        }
        restore(&mut s, old);
    }

    #[test]
    fn channel_window_keeps_local_values_but_ignores_main_text_and_browser_enter() {
        use crate::settings::{Field, FieldId};
        let mut s = Studio::sample();
        let track = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Keys")
            .unwrap()
            .id;
        s.open_channel(track);
        s.input_map.name = Some(crate::track_name::Name {
            track,
            field: Field::new(FieldId::Search, "Main".into()),
        });
        s.browser.open = true;
        s.browser.tab = crate::browser::RailTab::Effects;
        s.browser.selected_item = Some("Reverb");
        s.mixer.browser_fx_focus = true;
        let old = scope(&mut s, None);
        let local = surface_geometry(&s, 800.0, 600.0);
        assert_eq!(s.ime_target(), None);
        s.ime_replace(None, "ignored", false);
        assert_eq!(s.input_map.name.as_ref().unwrap().field.text(), "Main");
        assert!(!s.pick_typed(&local, "enter", None));
        let viewport = s.viewport_size.get();
        let motion = format!("{:?}", s.motion);
        for key in ["enter", "escape", "backspace", "tab"] {
            device_key_event(
                &mut s,
                &local,
                &gpui::KeyDownEvent {
                    keystroke: gpui::Keystroke::parse(key).unwrap(),
                    is_held: false,
                    prefer_character_input: false,
                },
                None,
            );
            assert_eq!(s.input_map.name.as_ref().unwrap().field.text(), "Main");
            assert_eq!(s.viewport_size.get(), viewport);
            assert_eq!(format!("{:?}", s.motion), motion);
        }
        s.head_fader.entry = Some(crate::head_fader::Entry {
            track,
            field: Field::new(FieldId::Search, "-3".into()),
            fresh: false,
        });
        assert_eq!(s.ime_target(), Some(crate::interaction::ImeTarget::Head));
        s.ime_replace(None, ".5", false);
        assert_eq!(s.head_fader.entry.as_ref().unwrap().field.text(), "-3.5");
        restore(&mut s, old);
    }

    #[test]
    fn independent_ime_does_not_edit_a_main_field_or_another_device() {
        use crate::settings::{Field, FieldId};
        let mut s = Studio::sample();
        s.input_map.name = Some(crate::track_name::Name {
            track: xirius_core::id::TrackId::MASTER,
            field: Field::new(FieldId::Search, "Main".into()),
        });
        s.instruments.entry = Some(crate::device::input::Entry {
            plugin: PluginId(2),
            k: 0,
            field: Field::new(FieldId::Search, "Other".into()),
            fresh: false,
        });
        let old = scope(&mut s, Some(PluginId(1)));
        assert_eq!(s.ime_target(), None);
        s.ime_replace(None, "ignored", false);
        assert_eq!(s.input_map.name.as_ref().unwrap().field.text(), "Main");
        assert_eq!(s.instruments.entry.as_ref().unwrap().field.text(), "Other");
        s.instruments.entry.as_mut().unwrap().plugin = PluginId(1);
        assert_eq!(s.ime_target(), Some(crate::interaction::ImeTarget::Device));
        s.ime_replace(None, " local", false);
        assert_eq!(
            s.instruments.entry.as_ref().unwrap().field.text(),
            "Other local"
        );
        restore(&mut s, old);
        assert_eq!(s.ime_target(), Some(crate::interaction::ImeTarget::Name));
    }
}
