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
use crate::independent::{Key, Kind, Spec};

#[cfg(windows)]
pub(crate) mod drag_shots;
pub(crate) mod input_shots;
#[cfg(windows)]
pub(crate) mod preset_shots;
#[cfg(windows)]
pub(crate) mod verify_shots;

#[derive(Default)]
pub(crate) struct Windows {
    open: HashMap<PluginId, WindowHandle<DeviceWindow>>,
    channel: Option<WindowHandle<DeviceWindow>>,
    macros: Option<WindowHandle<DeviceWindow>>,
    preset: Option<WindowHandle<DeviceWindow>>,
}

impl Windows {
    pub(crate) fn close(&mut self, cx: &mut App) {
        if let Some(handle) = self.preset.take() {
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

    pub(crate) fn sync(&mut self, studio: &Rc<RefCell<Studio>>, main: &Window, cx: &mut App) {
        self.sync_channel(studio, main, cx);
        self.sync_macros(studio, main, cx);
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
        let initial = Bounds::new(
            point(px(x as f32 / scale), px(y as f32 / scale)),
            size(px(332.0), px(420.0)),
        );
        let shared = studio.clone();
        let spec = Spec {
            kind: Kind::Presets,
            title: "Presets".into(),
            initial,
            min_size: None,
            resizable: false,
            positions: None,
        };
        let result = crate::independent::open(cx, main, spec, move |window, cx| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
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
        });
        match result {
            Ok(handle) => self.preset = Some(handle),
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
        let spec = Spec {
            kind: Kind::Channel,
            title: "Xirius · Channel Editor".into(),
            initial: Bounds::new(
                point(origin.x + px(60.0), origin.y + px(80.0)),
                size(px(natural.0), px(natural.1)),
            ),
            min_size: Some(size(px(least), px(CHANNEL_MIN_H))),
            resizable: true,
            positions: Some(positions.clone()),
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
    menu_only: bool,
    /// The Macro Controls (`id` is `None` as for the Channel Editor).
    macros: bool,
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
            if !window.is_window_active() && view.menu_only {
                view.popup = None;
                cx.notify();
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
            if view.menu_only {
                return;
            }
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
            menu_only: false,
            macros: false,
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
        if self.macros {
            // The window is exactly its panel: whatever size the system gave it, the surface
            // is brought to the content's.
            let wanted = size(px(crate::macro_pad::W), px(crate::macro_pad::H));
            if window.viewport_size() != wanted {
                window.resize(wanted);
            }
        }
        if self.id.is_none() && !self.menu_only && !self.macros {
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
                    if view.menu_only {
                        s.refresh_popup();
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
        // very functions the press hit test uses, so the two cannot drift. The Presets popup has no
        // bar.
        let drags = if self.menu_only {
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
        // The Macro Controls' marks belong to this window's own tree, in its own coordinates.
        let marks = if self.macros {
            crate::macro_pad::marks(&self.studio.borrow())
        } else {
            Vec::new()
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
    if s.popup.is_none() && s.field_keys(g, &k.key, k.key_char.as_deref(), m) {
        return;
    }
    let picking = s.ime_pick_field().is_some();
    if !picking
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
        let old = scope_as(&mut s, self.id, self.macros);
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
