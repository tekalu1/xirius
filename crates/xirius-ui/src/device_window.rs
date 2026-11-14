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
    /// Channel Editor, the Macro Controls and the preset menu are not. Without this a file
    /// let go on any other detached window would fall through to the song's lanes, whose
    /// geometry is the main window's, not this surface's.
    fn takes_files(&self) -> bool {
        if self.menu_only {
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
        // The marks of what this window shows (Macro Controls, a device, the Channel Editor) belong
        // to its own tree, in its own coordinates.
        let marks = if self.menu_only {
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

    /// A field in an independent window takes its letter once: the key event's
    /// own path puts nothing in a field the IME owns — `WM_CHAR` comes through
    /// the `EntityInputHandler` (UI-338).
    #[test]
    fn a_channel_window_field_takes_a_letter_once() {
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
        let old = scope(&mut s, None);
        let local = surface_geometry(&s, 800.0, 600.0);
        s.head_fader.entry = Some(crate::head_fader::Entry {
            track,
            field: Field::new(FieldId::Search, "-3".into()),
            fresh: false,
        });
        assert_eq!(s.ime_target(), Some(crate::interaction::ImeTarget::Head));
        device_key_event(
            &mut s,
            &local,
            &gpui::KeyDownEvent {
                keystroke: gpui::Keystroke::parse("k->k").unwrap(),
                is_held: false,
                prefer_character_input: false,
            },
            None,
        );
        assert_eq!(s.head_fader.entry.as_ref().unwrap().field.text(), "-3");
        s.ime_replace(None, "k", false);
        assert_eq!(s.head_fader.entry.as_ref().unwrap().field.text(), "-3k");
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

    mod headless {
        use gpui::{
            Action, ActivityGuard, AnyWindowHandle, BackgroundExecutor, Bounds, Capslock,
            ClipboardItem, CursorStyle, DispatchEventResult, DisplayId, DummyKeyboardMapper,
            ForegroundExecutor, GpuSpecs, HeadlessAtlas, Keymap, Modifiers, NoopTextSystem,
            PathPromptOptions, Pixels, Platform, PlatformAtlas, PlatformDispatcher,
            PlatformDisplay, PlatformInput, PlatformInputHandler, PlatformKeyboardLayout,
            PlatformKeyboardMapper, PlatformTextSystem, PlatformWindow, Point, Priority,
            PromptButton, PromptLevel, RequestFrameOptions, RunnableVariant, Scene, Size, Task,
            ThermalState, WindowAppearance, WindowBackgroundAppearance, WindowBounds,
            WindowControlArea, WindowParams, WindowVisibility, point, px, size,
        };
        use raw_window_handle::{
            DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
        };
        use std::{
            cell::{Cell, RefCell},
            ffi::OsString,
            path::{Path, PathBuf},
            rc::Rc,
            sync::Arc,
            time::Duration,
        };

        struct InlineDispatcher;

        impl PlatformDispatcher for InlineDispatcher {
            fn is_main_thread(&self) -> bool {
                true
            }
            fn dispatch(&self, runnable: RunnableVariant, _: Priority) {
                runnable.run();
            }
            fn dispatch_on_main_thread(&self, runnable: RunnableVariant, _: Priority) {
                runnable.run();
            }
            fn dispatch_after(&self, _: Duration, _: RunnableVariant) {}
            fn spawn_realtime(&self, f: Box<dyn FnOnce() + Send>) {
                f();
            }
        }

        #[derive(Debug)]
        struct HeadlessDisplay;

        impl PlatformDisplay for HeadlessDisplay {
            fn id(&self) -> DisplayId {
                DisplayId::new(1)
            }
            fn uuid(&self) -> gpui::Result<uuid::Uuid> {
                Ok(uuid::Uuid::nil())
            }
            fn bounds(&self) -> Bounds<Pixels> {
                Bounds::new(point(px(0.0), px(0.0)), size(px(1920.0), px(1080.0)))
            }
        }

        struct DummyLayout;

        impl PlatformKeyboardLayout for DummyLayout {
            fn id(&self) -> &str {
                "us"
            }
            fn name(&self) -> &str {
                "US"
            }
        }

        type FrameRequest = Box<dyn FnMut(RequestFrameOptions)>;
        type InputCallback = Box<dyn FnMut(PlatformInput) -> DispatchEventResult>;

        pub(super) struct WindowState {
            bounds: Cell<Bounds<Pixels>>,
            display: Rc<dyn PlatformDisplay>,
            atlas: Arc<dyn PlatformAtlas>,
            request_frame: RefCell<Option<FrameRequest>>,
            input: RefCell<Option<InputCallback>>,
            input_handler: RefCell<Option<PlatformInputHandler>>,
        }

        #[derive(Clone)]
        pub(super) struct HeadlessWindow(pub(super) Rc<WindowState>);

        impl HasWindowHandle for HeadlessWindow {
            fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
                Err(HandleError::NotSupported)
            }
        }

        impl HasDisplayHandle for HeadlessWindow {
            fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
                Err(HandleError::NotSupported)
            }
        }

        impl HeadlessWindow {
            pub(super) fn draw(&self) {
                let cb = self.0.request_frame.borrow_mut().take();
                if let Some(mut cb) = cb {
                    cb(RequestFrameOptions {
                        force_render: true,
                        ..Default::default()
                    });
                    *self.0.request_frame.borrow_mut() = Some(cb);
                }
            }

            pub(super) fn dispatch(&self, ev: PlatformInput) {
                let cb = self.0.input.borrow_mut().take();
                if let Some(mut cb) = cb {
                    cb(ev);
                    *self.0.input.borrow_mut() = Some(cb);
                }
            }
        }

        impl PlatformWindow for HeadlessWindow {
            fn bounds(&self) -> Bounds<Pixels> {
                self.0.bounds.get()
            }
            fn is_maximized(&self) -> bool {
                false
            }
            fn window_bounds(&self) -> WindowBounds {
                WindowBounds::Windowed(self.0.bounds.get())
            }
            fn content_size(&self) -> Size<Pixels> {
                self.0.bounds.get().size
            }
            fn resize(&mut self, size: Size<Pixels>) {
                let mut b = self.0.bounds.get();
                b.size = size;
                self.0.bounds.set(b);
            }
            fn scale_factor(&self) -> f32 {
                1.0
            }
            fn appearance(&self) -> WindowAppearance {
                WindowAppearance::Dark
            }
            fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
                Some(self.0.display.clone())
            }
            fn mouse_position(&self) -> Point<Pixels> {
                Point::default()
            }
            fn modifiers(&self) -> Modifiers {
                Modifiers::default()
            }
            fn capslock(&self) -> Capslock {
                Capslock::default()
            }
            fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
                *self.0.input_handler.borrow_mut() = Some(input_handler);
            }
            fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
                self.0.input_handler.borrow_mut().take()
            }
            fn prompt(
                &self,
                _: PromptLevel,
                _: &str,
                _: Option<&str>,
                _: &[PromptButton],
            ) -> Option<futures_channel::oneshot::Receiver<usize>> {
                None
            }
            fn activate(&self) {}
            fn is_active(&self) -> bool {
                true
            }
            fn visibility(&self) -> WindowVisibility {
                WindowVisibility::Visible
            }
            fn is_hovered(&self) -> bool {
                true
            }
            fn background_appearance(&self) -> WindowBackgroundAppearance {
                WindowBackgroundAppearance::Opaque
            }
            fn set_title(&mut self, _: &str) {}
            fn set_background_appearance(&self, _: WindowBackgroundAppearance) {}
            fn minimize(&self) {}
            fn zoom(&self) {}
            fn toggle_fullscreen(&self) {}
            fn is_fullscreen(&self) -> bool {
                false
            }
            fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
                *self.0.request_frame.borrow_mut() = Some(callback);
            }
            fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
                *self.0.input.borrow_mut() = Some(callback);
            }
            fn on_active_status_change(&self, _: Box<dyn FnMut(bool)>) {}
            fn on_visibility_change(&self, _: Box<dyn FnMut(WindowVisibility)>) {}
            fn on_hover_status_change(&self, _: Box<dyn FnMut(bool)>) {}
            fn on_resize(&self, _: Box<dyn FnMut(Size<Pixels>, f32)>) {}
            fn on_moved(&self, _: Box<dyn FnMut()>) {}
            fn on_should_close(&self, _: Box<dyn FnMut() -> bool>) {}
            fn on_hit_test_window_control(&self, _: Box<dyn FnMut() -> Option<WindowControlArea>>) {
            }
            fn on_close(&self, _: Box<dyn FnOnce()>) {}
            fn on_appearance_changed(&self, _: Box<dyn FnMut()>) {}
            fn draw(&self, _: &Scene) {}
            fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
                self.0.atlas.clone()
            }
            fn is_subpixel_rendering_supported(&self) -> bool {
                false
            }
            #[cfg(target_os = "windows")]
            fn get_raw_handle(&self) -> windows::Win32::Foundation::HWND {
                windows::Win32::Foundation::HWND(std::ptr::null_mut())
            }
            fn gpu_specs(&self) -> Option<GpuSpecs> {
                None
            }
            fn update_ime_position(&self, _: Bounds<Pixels>) {}
        }

        pub(super) struct HeadlessPlatform {
            background: BackgroundExecutor,
            foreground: ForegroundExecutor,
            text_system: Arc<dyn PlatformTextSystem>,
            display: Rc<dyn PlatformDisplay>,
            pub(super) windows: RefCell<Vec<HeadlessWindow>>,
        }

        impl HeadlessPlatform {
            pub(super) fn new() -> Rc<Self> {
                let dispatcher: Arc<dyn PlatformDispatcher> = Arc::new(InlineDispatcher);
                Rc::new(Self {
                    background: BackgroundExecutor::new(dispatcher.clone()),
                    foreground: ForegroundExecutor::new(dispatcher),
                    text_system: Arc::new(NoopTextSystem::new()),
                    display: Rc::new(HeadlessDisplay),
                    windows: RefCell::new(Vec::new()),
                })
            }
        }

        impl Platform for HeadlessPlatform {
            fn background_executor(&self) -> BackgroundExecutor {
                self.background.clone()
            }
            fn foreground_executor(&self) -> ForegroundExecutor {
                self.foreground.clone()
            }
            fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
                self.text_system.clone()
            }
            fn run(&self, on_finish_launching: Box<dyn 'static + FnOnce()>) {
                on_finish_launching();
            }
            fn quit(&self) {}
            fn restart(&self, _: Option<PathBuf>, _: Vec<OsString>) {}
            fn activate(&self, _: bool) {}
            fn hide(&self) {}
            fn hide_other_apps(&self) {}
            fn unhide_other_apps(&self) {}
            fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
                vec![self.display.clone()]
            }
            fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
                Some(self.display.clone())
            }
            fn active_window(&self) -> Option<AnyWindowHandle> {
                None
            }
            fn open_window(
                &self,
                _: AnyWindowHandle,
                options: WindowParams,
            ) -> gpui::Result<Box<dyn PlatformWindow>> {
                let win = HeadlessWindow(Rc::new(WindowState {
                    bounds: Cell::new(options.bounds),
                    display: self.display.clone(),
                    atlas: Arc::new(HeadlessAtlas::default()),
                    request_frame: RefCell::new(None),
                    input: RefCell::new(None),
                    input_handler: RefCell::new(None),
                }));
                self.windows.borrow_mut().push(win.clone());
                Ok(Box::new(win))
            }
            fn window_appearance(&self) -> WindowAppearance {
                WindowAppearance::Dark
            }
            fn open_url(&self, _: &str) {}
            fn on_open_urls(&self, _: Box<dyn FnMut(Vec<String>)>) {}
            fn register_url_scheme(&self, _: &str) -> Task<gpui::Result<()>> {
                Task::ready(Ok(()))
            }
            fn prompt_for_paths(
                &self,
                _: PathPromptOptions,
            ) -> futures_channel::oneshot::Receiver<gpui::Result<Option<Vec<PathBuf>>>>
            {
                unimplemented!()
            }
            fn prompt_for_new_path(
                &self,
                _: &Path,
                _: Option<&str>,
            ) -> futures_channel::oneshot::Receiver<gpui::Result<Option<PathBuf>>> {
                unimplemented!()
            }
            fn can_select_mixed_files_and_dirs(&self) -> bool {
                false
            }
            fn reveal_path(&self, _: &Path) {}
            fn open_with_system(&self, _: &Path) {}
            fn on_quit(&self, _: Box<dyn FnMut() -> bool>) {}
            fn on_reopen(&self, _: Box<dyn FnMut()>) {}
            fn on_system_sleep(&self, _: Box<dyn FnMut()>) {}
            fn on_system_wake(&self, _: Box<dyn FnMut()>) {}
            fn set_menus(&self, _: Vec<gpui::Menu>, _: &Keymap) {}
            fn set_dock_menu(&self, _: Vec<gpui::MenuItem>, _: &Keymap) {}
            fn on_app_menu_action(&self, _: Box<dyn FnMut(&dyn Action)>) {}
            fn on_will_open_app_menu(&self, _: Box<dyn FnMut()>) {}
            fn on_validate_app_menu_command(&self, _: Box<dyn FnMut(&dyn Action) -> bool>) {}
            fn thermal_state(&self) -> ThermalState {
                ThermalState::Nominal
            }
            fn on_thermal_state_change(&self, _: Box<dyn FnMut()>) {}
            fn prevent_idle_sleep(&self, _: &str) -> Task<gpui::Result<ActivityGuard>> {
                Task::ready(Ok(ActivityGuard::noop()))
            }
            fn app_path(&self) -> gpui::Result<PathBuf> {
                Ok(PathBuf::new())
            }
            fn path_for_auxiliary_executable(&self, _: &str) -> gpui::Result<PathBuf> {
                Ok(PathBuf::new())
            }
            fn set_cursor_style(&self, _: CursorStyle) {}
            fn hide_cursor_until_mouse_moves(&self) {}
            fn is_cursor_visible(&self) -> bool {
                true
            }
            fn should_auto_hide_scrollbars(&self) -> bool {
                false
            }
            fn read_from_clipboard(&self) -> Option<ClipboardItem> {
                None
            }
            fn write_to_clipboard(&self, _: ClipboardItem) {}
            #[cfg(any(target_os = "linux", target_os = "freebsd"))]
            fn read_from_primary(&self) -> Option<ClipboardItem> {
                None
            }
            #[cfg(any(target_os = "linux", target_os = "freebsd"))]
            fn write_to_primary(&self, _: ClipboardItem) {}
            #[cfg(target_os = "macos")]
            fn read_from_find_pasteboard(&self) -> Option<ClipboardItem> {
                None
            }
            #[cfg(target_os = "macos")]
            fn write_to_find_pasteboard(&self, _: ClipboardItem) {}
            fn write_credentials(&self, _: &str, _: &str, _: &[u8]) -> Task<gpui::Result<()>> {
                Task::ready(Ok(()))
            }
            fn read_credentials(&self, _: &str) -> Task<gpui::Result<Option<(String, Vec<u8>)>>> {
                Task::ready(Ok(None))
            }
            fn delete_credentials(&self, _: &str) -> Task<gpui::Result<()>> {
                Task::ready(Ok(()))
            }
            fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
                Box::new(DummyLayout)
            }
            fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
                Rc::new(DummyKeyboardMapper)
            }
            fn on_keyboard_layout_change(&self, _: Box<dyn FnMut()>) {}
        }

        pub(super) struct BlankRoot;
        impl gpui::Render for BlankRoot {
            fn render(
                &mut self,
                _: &mut gpui::Window,
                _: &mut gpui::Context<Self>,
            ) -> impl gpui::IntoElement {
                gpui::div()
            }
        }

        /// The main window the `Windows::sync` call under test stands on. `independent::open`
        /// looks the OS's window of a title up to parent and place, which a headless window
        /// has none of: this is the same `cx.open_window` without that look-up.
        pub(super) fn open_test<V: gpui::Render + 'static>(
            cx: &mut gpui::App,
            bounds: Bounds<Pixels>,
            build: impl FnOnce(&mut gpui::Window, &mut gpui::App) -> gpui::Entity<V> + 'static,
        ) -> gpui::Result<gpui::WindowHandle<V>> {
            cx.open_window(
                gpui::WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                build,
            )
        }
    }

    fn write_test_wav(path: &std::path::Path) {
        let tone: Vec<f32> = (0..441).map(|i| (i as f32 * 0.1).sin() * 0.4).collect();
        xirius_session::export::write_wav(path, &tone, &tone, 44_100, 16).unwrap();
    }

    #[test]
    fn independent_device_window_accepts_external_file_drops_and_clears_preview() {
        use crate::{
            device::sampler::DropOn,
            instrument::{Pick, current, window::Part},
        };
        use gpui::{
            Application, Capslock, ExternalPaths, FileDropEvent, Modifiers, ModifiersChangedEvent,
            PlatformInput, point, px, size,
        };
        use headless::{BlankRoot, HeadlessPlatform};

        let dir = tempfile::tempdir().unwrap();
        let kick = dir.path().join("Kick.wav");
        let snare = dir.path().join("Snare.wav");
        write_test_wav(&kick);
        write_test_wav(&snare);

        let mut s = Studio::sample();
        s.geometry(1280.0, 800.0);
        let track = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Keys")
            .unwrap()
            .id;
        assert!(s.set_instrument(track, &Pick::built_in("Drum Kit")));
        let id = current(&s.project().tracks[&track]).unwrap().id;
        s.show_instrument(track, id);

        let studio = Rc::new(RefCell::new(s));
        let windows = Rc::new(RefCell::new(Windows::default()));
        let platform = HeadlessPlatform::new();

        let _app = Application::with_platform(platform.clone()).run_embedded({
            let studio = studio.clone();
            let windows = windows.clone();
            move |cx| {
                let main = headless::open_test(
                    cx,
                    Bounds::new(point(px(0.0), px(0.0)), size(px(1280.0), px(800.0))),
                    |_, cx| cx.new(|_| BlankRoot),
                )
                .unwrap();
                let _ = main.update(cx, |_, main_win, cx| {
                    windows.borrow_mut().sync(&studio, main_win, cx);
                });
            }
        });

        let dev_win = platform.windows.borrow()[1].clone();
        dev_win.draw();

        let (pad_pt, plane_pt) = {
            let mut s = studio.borrow_mut();
            let body = s.instruments.open[0].body;
            let old = scope(&mut s, Some(id));
            let g = surface_geometry(&s, body.0, body.1 + crate::title_bar::BAR_H);
            let pad = s.instrument_center(&g, id, Part::Pad(36)).unwrap();
            let plane = s.instrument_center(&g, id, Part::Body).unwrap();
            restore(&mut s, old);
            (point(px(pad.0), px(pad.1)), point(px(plane.0), px(plane.1)))
        };

        let mut paths = ExternalPaths::default();
        paths.0.extend([kick.clone(), snare.clone()]);

        // 1. Enter over empty pad 0 -> preview and DropOn::Plane (while kit has no samples) are set.
        dev_win.dispatch(PlatformInput::FileDrop(FileDropEvent::Entered {
            position: pad_pt,
            paths: paths.clone(),
        }));
        {
            let s = studio.borrow();
            assert_eq!(
                s.instruments.drop_on.map(|(p, on, _)| (p, on)),
                Some((id, DropOn::Plane))
            );
            let preview = s
                .instruments
                .preview
                .as_ref()
                .expect("FileDrop::Entered on DeviceWindow should populate preview");
            assert_eq!(preview.plugin, id);
            assert_eq!(preview.pads.len(), 2);
            assert!(!s.instruments.drop_layers);
        }

        // 2. Move over Drop plane -> DropOn::Plane is set.
        dev_win.dispatch(PlatformInput::FileDrop(FileDropEvent::Pending {
            position: plane_pt,
        }));
        assert_eq!(
            studio
                .borrow()
                .instruments
                .drop_on
                .map(|(p, on, _)| (p, on)),
            Some((id, DropOn::Plane))
        );

        // 3. Move back to pad 0 and toggle Shift -> drop_layers updates and stacks onto 1 pad.
        dev_win.dispatch(PlatformInput::FileDrop(FileDropEvent::Pending {
            position: pad_pt,
        }));
        dev_win.dispatch(PlatformInput::ModifiersChanged(ModifiersChangedEvent {
            modifiers: Modifiers {
                shift: true,
                ..Default::default()
            },
            capslock: Capslock::default(),
        }));
        {
            let s = studio.borrow();
            assert!(s.instruments.drop_layers);
            let preview = s.instruments.preview.as_ref().unwrap();
            assert_eq!(preview.pads.len(), 1);
            assert_eq!(preview.pads[0].2, "Vel 2");
        }
        dev_win.dispatch(PlatformInput::ModifiersChanged(ModifiersChangedEvent {
            modifiers: Modifiers::default(),
            capslock: Capslock::default(),
        }));
        assert!(!studio.borrow().instruments.drop_layers);

        // 4. Drag exits window -> next frame clears preview and drop_on.
        dev_win.dispatch(PlatformInput::FileDrop(FileDropEvent::Exited));
        dev_win.draw();
        {
            let s = studio.borrow();
            assert_eq!(s.instruments.drop_on, None);
            assert!(s.instruments.preview.is_none());
        }

        // 5. While input_blocked() is true, Entered and Submit are ignored.
        studio.borrow_mut().transport.finishing_take = true;
        dev_win.dispatch(PlatformInput::FileDrop(FileDropEvent::Entered {
            position: pad_pt,
            paths: paths.clone(),
        }));
        dev_win.dispatch(PlatformInput::FileDrop(FileDropEvent::Submit {
            position: pad_pt,
        }));
        {
            let mut s = studio.borrow_mut();
            s.transport.finishing_take = false;
            assert_eq!(s.instruments.drop_on, None);
            assert!(s.instruments.preview.is_none());
            assert!(
                current(&s.project().tracks[&track])
                    .unwrap()
                    .samples
                    .is_empty()
            );
        }

        // 6. Real drop (Entered + Submit) loads both WAV files into the Drum Kit pads and supports single-step undo.
        dev_win.dispatch(PlatformInput::FileDrop(FileDropEvent::Entered {
            position: pad_pt,
            paths,
        }));
        dev_win.dispatch(PlatformInput::FileDrop(FileDropEvent::Submit {
            position: pad_pt,
        }));
        {
            let mut s = studio.borrow_mut();
            assert_eq!(s.instruments.drop_on, None);
            assert!(s.instruments.preview.is_none());
            let loaded = current(&s.project().tracks[&track])
                .unwrap()
                .samples
                .clone();
            assert_eq!(loaded.len(), 2);
            assert!(loaded[0].ends_with("Kick.wav"));
            assert!(loaded[1].ends_with("Snare.wav"));
            let g = s.geometry(1280.0, 800.0);
            assert!(s.undo(&g));
            assert!(
                current(&s.project().tracks[&track])
                    .unwrap()
                    .samples
                    .is_empty()
            );
        }
    }

    #[test]
    fn every_sample_bearing_builtin_device_window_accepts_external_file_drops() {
        use crate::instrument::{Pick, current, window::Part};
        use gpui::{Application, ExternalPaths, FileDropEvent, PlatformInput, point, px, size};
        use headless::{BlankRoot, HeadlessPlatform};

        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("Tone.wav");
        write_test_wav(&wav);

        let sample_defs: Vec<&xirius_params::BuiltinDef> = xirius_params::BUILTINS
            .iter()
            .copied()
            .filter(|def| crate::device::sampler::has_files(def))
            .collect();
        assert!(
            sample_defs.len() >= 2,
            "expected at least Sampler and Drum Kit to accept sample files"
        );

        for def in sample_defs {
            let mut s = Studio::sample();
            s.geometry(1280.0, 800.0);
            let track = s
                .project()
                .tracks
                .values()
                .find(|t| t.name == "Keys")
                .unwrap()
                .id;
            assert!(s.set_instrument(track, &Pick::built_in(def.name)));
            let id = current(&s.project().tracks[&track]).unwrap().id;
            s.show_instrument(track, id);

            let studio = Rc::new(RefCell::new(s));
            let windows = Rc::new(RefCell::new(Windows::default()));
            let platform = HeadlessPlatform::new();

            let _app = Application::with_platform(platform.clone()).run_embedded({
                let studio = studio.clone();
                let windows = windows.clone();
                move |cx| {
                    let main = headless::open_test(
                        cx,
                        Bounds::new(point(px(0.0), px(0.0)), size(px(1280.0), px(800.0))),
                        |_, cx| cx.new(|_| BlankRoot),
                    )
                    .unwrap();
                    let _ = main.update(cx, |_, main_win, cx| {
                        windows.borrow_mut().sync(&studio, main_win, cx);
                    });
                }
            });

            let dev_win = platform.windows.borrow()[1].clone();
            dev_win.draw();

            let drop_pt = {
                let mut s = studio.borrow_mut();
                let body = s.instruments.open[0].body;
                let old = scope(&mut s, Some(id));
                let g = surface_geometry(&s, body.0, body.1 + crate::title_bar::BAR_H);
                let pt = s.instrument_center(&g, id, Part::Body).unwrap();
                restore(&mut s, old);
                point(px(pt.0), px(pt.1))
            };

            let mut paths = ExternalPaths::default();
            paths.0.push(wav.clone());
            dev_win.dispatch(PlatformInput::FileDrop(FileDropEvent::Entered {
                position: drop_pt,
                paths,
            }));
            assert!(
                studio.borrow().instruments.drop_on.is_some(),
                "built-in device `{}` ({}) did not highlight on FileDrop::Entered in DeviceWindow",
                def.uid,
                def.name
            );

            dev_win.dispatch(PlatformInput::FileDrop(FileDropEvent::Submit {
                position: drop_pt,
            }));
            let loaded = current(&studio.borrow().project().tracks[&track])
                .unwrap()
                .samples
                .clone();
            assert_eq!(
                loaded.len(),
                1,
                "built-in device `{}` ({}) did not load dropped sample in DeviceWindow",
                def.uid,
                def.name
            );
        }
    }
}
