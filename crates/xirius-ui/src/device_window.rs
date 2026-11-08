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

#[derive(Default)]
pub(crate) struct Windows {
    open: HashMap<PluginId, WindowHandle<DeviceWindow>>,
    channel: Option<WindowHandle<DeviceWindow>>,
}

impl Windows {
    pub(crate) fn close(&mut self, cx: &mut App) {
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
        self.sync_channel(studio, main, cx);
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
            if self.open.contains_key(&win.plugin) {
                continue;
            }
            let shared = studio.clone();
            let id = win.plugin;
            let title = format!("Xirius · {id}");
            let origin = main.bounds().origin;
            let result = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(origin.x + px(40.0), origin.y + px(60.0)),
                        size(px(win.body.0), px(win.body.1 + crate::title_bar::BAR_H)),
                    ))),
                    titlebar: Some(TitlebarOptions {
                        title: Some(title.clone().into()),
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    window_decorations: Some(WindowDecorations::Client),
                    focus: false,
                    show: false,
                    is_minimizable: false,
                    is_resizable: false,
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
                    cx.new(|cx| DeviceWindow::new(shared, Some(id), focus, window, cx))
                },
            );
            match result {
                Ok(handle) => {
                    self.open.insert(id, handle);
                }
                Err(error) => {
                    let mut s = studio.borrow_mut();
                    s.instruments.detached.remove(&id);
                    s.error = Some(format!("独立ウィンドウを開けません: {error}"));
                }
            }
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
        if self.channel.is_some() {
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
                    closing.borrow_mut().channel = None;
                    true
                });
                let focus = cx.focus_handle();
                window.focus(&focus, cx);
                window.resize(size(px(800.0), px(600.0)));
                show("Xirius · Channel Editor");
                cx.new(|cx| DeviceWindow::new(shared, None, focus, window, cx))
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
    _activation: gpui::Subscription,
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

impl DeviceWindow {
    fn new(
        studio: Rc<RefCell<Studio>>,
        id: Option<PluginId>,
        focus: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let activation = cx.observe_window_activation(window, |view, window, cx| {
            if !window.is_window_active() && view.studio.borrow_mut().device_focus_lost() {
                cx.refresh_windows();
            }
        });
        Self {
            studio,
            id,
            focus,
            popup: None,
            _activation: activation,
        }
    }
    /// Input and painting share the very same Win. Only the coordinate system and
    /// popup belong to this surface; no song or audio instance is copied.
    fn with<R>(&mut self, window: &Window, f: impl FnOnce(&mut Studio, &Geometry) -> R) -> R {
        let mut s = self.studio.borrow_mut();
        let old = scope(&mut s, self.id);
        std::mem::swap(&mut s.popup, &mut self.popup);
        let viewport = window.viewport_size();
        let g = s.geometry(viewport.width.into(), viewport.height.into());
        let result = f(&mut s, &g);
        std::mem::swap(&mut s.popup, &mut self.popup);
        restore(&mut s, old);
        result
    }
}

impl Render for DeviceWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let live = {
            let s = self.studio.borrow();
            self.id.map_or_else(
                || s.channel.as_ref().is_some_and(|w| w.detached),
                |id| s.instruments.detached.contains(&id) && s.instruments.is_open(id),
            )
        };
        if !live {
            window.defer(cx, |window, _| window.remove_window());
        }
        if let Some(w) = self
            .studio
            .borrow()
            .instruments
            .open
            .iter()
            .find(|w| Some(w.plugin) == self.id)
        {
            let wanted = size(px(w.body.0), px(w.body.1 + crate::title_bar::BAR_H));
            if window.viewport_size() != wanted {
                window.resize(wanted);
            }
        }
        window.request_animation_frame();
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
                    let g = s.geometry(viewport.width.into(), viewport.height.into());
                    let mut pen = Pen {
                        window,
                        cx,
                        c: &colors,
                        origin: (0.0, 0.0),
                    };
                    if view.id.is_some() {
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
                        }
                        s.instrument_up();
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
                        }
                        s.instrument_up();
                    });
                    cx.refresh_windows();
                }),
            )
            .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                this.with(window, |s, g| {
                    let k = &e.keystroke;
                    if s.field_keys(g, &k.key, k.key_char.as_deref(), modifiers(k.modifiers)) {
                        return;
                    }
                    let m = modifiers(k.modifiers);
                    let picking = s.ime_pick_field().is_some();
                    // The list receives letters through the IME, just as in Screen.
                    if picking
                        && !m.ctrl
                        && !m.alt
                        && k.key_char
                            .as_deref()
                            .is_some_and(|c| !c.is_empty() && !c.chars().any(char::is_control))
                    {
                        return;
                    }
                    if !s.device_key(g, &k.key, m)
                        && !s.typed(
                            g,
                            &k.key,
                            if picking { None } else { k.key_char.as_deref() },
                        )
                        && !m.ctrl
                        && !m.alt
                        && !m.shift
                    {
                        s.pc_key_down(&k.key, e.is_held);
                    }
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
                            f64::from(f32::from(delta.y)) / 20.0,
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
        let previous = std::mem::replace(&mut s.popup, self.popup.clone());
        let result = s
            .ime_pick_field()
            .or_else(|| s.ime_field().cloned())
            .and_then(|f| f.marked_range());
        s.popup = previous;
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
            places.push((
                crate::instrument::menu::Menu::Destination(w.track, w.plugin),
                s.instruments
                    .part_rect(r, &p, track, plugin, def, Part::Destination),
            ));
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
