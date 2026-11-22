//! A window at the editor's place while its host is opening it (UI-363).

use crate::draw::Pen;
use crate::independent::{Kind, Spec};
use crate::mixer::editors::Where;
use crate::paint::Colors;
use crate::studio::{Rect, Studio};
use crate::title_bar;
use gpui::{
    App, Bounds, Context, IntoElement, MouseButton, Render, Window, WindowHandle, canvas, div,
    point, prelude::*, px, size,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};
use xirius_core::id::PluginId;

const SPIN_AFTER: Duration = Duration::from_millis(150);
// The first view size follows measured editors and is fitted to the visible display.
const FIRST_SIZE: (f32, f32) = (1400.0, 900.0);

fn opening_size(remembered: &HashMap<String, (i32, i32)>, uid: &str, scale: f32) -> (f32, f32) {
    let (width, view_height) = remembered
        .get(uid)
        .map_or((FIRST_SIZE.0 / scale, FIRST_SIZE.1 / scale), |(w, h)| {
            (*w as f32 / scale, *h as f32 / scale)
        });
    (width, view_height + title_bar::BAR_H)
}

fn spinner_visible(since: Instant, now: Instant) -> bool {
    now.duration_since(since) >= SPIN_AFTER
}

#[derive(Clone)]
struct Pending {
    id: PluginId,
    title: String,
    track: String,
    device: String,
    preset: String,
    instrument: bool,
    powered: bool,
    pinned: bool,
    size: (f32, f32),
    position: Option<(f32, f32)>,
}

#[derive(Default)]
pub(crate) struct Windows {
    open: HashMap<PluginId, WindowHandle<LoadingWindow>>,
}

impl Windows {
    pub(crate) fn has_pending(&self) -> bool {
        !self.open.is_empty()
    }

    pub(crate) fn close(&mut self, cx: &mut App) {
        for (_, handle) in self.open.drain() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }

    /// An owned loading window can outlive main-window frames. Retire it and
    /// release the host's foreground request on the UI thread between frames too.
    pub(crate) fn sync_after_poll(
        &mut self,
        studio: &Rc<RefCell<Studio>>,
        main: &Window,
        cx: &mut App,
    ) {
        if self.has_pending()
            || studio
                .borrow()
                .mixer
                .editors
                .iter()
                .any(|editor| editor.awaits_placement())
        {
            self.sync(studio, main, cx);
        }
    }

    pub(crate) fn sync(&mut self, studio: &Rc<RefCell<Studio>>, main: &Window, cx: &mut App) {
        let wanted = {
            let s = studio.borrow();
            let mut ids: Vec<_> = s
                .mixer
                .editors
                .iter()
                .filter(|e| matches!(e.at, Where::Host { opened: false }))
                .map(|e| e.id)
                .collect();
            ids.extend(s.mixer.open_later.iter().map(|w| w.id));
            let project = s.project();
            ids.into_iter()
                .filter_map(|id| {
                    let (track, plugin) = project
                        .tracks
                        .values()
                        .find_map(|t| t.inserts.iter().find(|p| p.id == id).map(|p| (t, p)))?;
                    Some(Pending {
                        id,
                        title: format!("Xirius · {} · {id}", plugin.name),
                        track: track.shown_name().to_string(),
                        device: plugin.name.clone(),
                        preset: plugin.preset.clone().unwrap_or_else(|| "—".into()),
                        instrument: plugin.kind == xirius_core::model::PluginKind::Instrument,
                        powered: plugin.runs(),
                        pinned: s.mixer.editor_pins.contains(&id),
                        size: opening_size(&s.mixer.editor_sizes, &plugin.uid, main.scale_factor()),
                        position: s.mixer.editor_positions.get(&id).map(|(x, y)| {
                            (
                                *x as f32 / main.scale_factor(),
                                *y as f32 / main.scale_factor(),
                            )
                        }),
                    })
                })
                .collect::<Vec<_>>()
        };
        let mut retired = Vec::new();
        self.open.retain(|id, handle| {
            if wanted.iter().any(|p| p.id == *id) {
                let handle = *handle;
                cx.defer(move |cx| {
                    let _ = handle.update(cx, |_, _, cx| cx.notify());
                });
                return true;
            }
            let hwnd = handle
                .update(cx, |_, window, _| {
                    #[cfg(windows)]
                    let hwnd = crate::device_window::hwnd_of(window) as u64;
                    #[cfg(not(windows))]
                    let hwnd = 0;
                    window.remove_window();
                    hwnd
                })
                .unwrap_or(0);
            retired.push((*id, hwnd));
            false
        });
        let mut focus_ready = false;
        for editor in &mut studio.borrow_mut().mixer.editors {
            if editor.awaits_placement() {
                editor.retired_loading = Some(
                    retired
                        .iter()
                        .find(|(id, _)| *id == editor.id)
                        .map_or(0, |(_, hwnd)| *hwnd),
                );
                focus_ready = true;
            }
        }
        if !retired.is_empty() || focus_ready {
            // Retiring an owned loading window can move the studio's window group above
            // the newly opened editor. Place the editors after the removal is processed.
            let studio = studio.clone();
            let retired = !retired.is_empty();
            cx.defer(move |_| {
                let mut s = studio.borrow_mut();
                if !s.finish_editor_open() && retired {
                    s.raise_editors_over(0);
                }
            });
        }
        for pending in wanted {
            if self.open.contains_key(&pending.id) {
                continue;
            }
            let id = pending.id;
            let dimensions = pending.size;
            let bounds = main.bounds();
            let origin = pending.position.map_or_else(
                || {
                    point(
                        bounds.origin.x + (bounds.size.width - px(dimensions.0)) / 2.0,
                        bounds.origin.y + (bounds.size.height - px(dimensions.1)) / 2.0,
                    )
                },
                |(x, y)| point(px(x), px(y)),
            );
            let initial = Bounds::new(origin, size(px(dimensions.0), px(dimensions.1)));
            let (initial, display) = {
                let s = studio.borrow();
                let source = s
                    .mixer
                    .editors
                    .iter()
                    .find(|e| e.id == id)
                    .map(|e| e.foreground_from)
                    .or_else(|| {
                        s.mixer
                            .open_later
                            .iter()
                            .find(|w| w.id == id)
                            .map(|w| w.foreground_from)
                    })
                    .unwrap_or(0);
                crate::independent::device_bounds(&s, main, cx, id, initial, source)
            };
            let shared = studio.clone();
            let spec = Spec {
                kind: Kind::Device(id),
                title: pending.title.clone(),
                initial,
                display,
                min_size: None,
                resizable: false,
                positions: None,
                restore: false,
            };
            if let Ok(handle) = crate::independent::open(cx, main, spec, move |window, cx| {
                let closing = shared.clone();
                window.on_window_should_close(cx, move |_, _| {
                    cancel(&closing, id);
                    true
                });
                cx.new(|_| LoadingWindow {
                    studio: shared,
                    pending,
                    since: Instant::now(),
                })
            }) {
                self.open.insert(id, handle);
            }
        }
    }
}

fn cancel(studio: &Rc<RefCell<Studio>>, id: PluginId) {
    let mut s = studio.borrow_mut();
    s.mixer.open_later.retain(|w| w.id != id);
    s.close_editor(id);
}

struct LoadingWindow {
    studio: Rc<RefCell<Studio>>,
    pending: Pending,
    since: Instant,
}

impl Render for LoadingWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let id = self.pending.id;
        let entity = cx.entity();
        let surface = canvas(
            |_, _, _| (),
            move |_, _, window, cx| {
                entity.update(cx, |view, cx| {
                    let s = view.studio.borrow();
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
                    let colors = Colors::new(theme);
                    let extent = window.viewport_size();
                    let width = f32::from(extent.width);
                    let height = f32::from(extent.height);
                    let mut pen = Pen {
                        window,
                        cx,
                        c: &colors,
                        origin: (0.0, 0.0),
                    };
                    pen.fill(
                        Rect::new(0.0, title_bar::BAR_H, width, height - title_bar::BAR_H),
                        colors.surface_0,
                    );
                    title_bar::paint(
                        &mut pen,
                        Rect::new(0.0, 0.0, width, title_bar::BAR_H),
                        &title_bar::View {
                            track: &view.pending.track,
                            device: &view.pending.device,
                            icon: if view.pending.instrument {
                                "piano"
                            } else {
                                "ins"
                            },
                            powered: view.pending.powered,
                            preset: &view.pending.preset,
                            modified: false,
                            steps: (true, true),
                            pinned: view.pending.pinned,
                            keyed: false,
                            seats: title_bar::Seats {
                                power: true,
                                channel: true,
                                ..Default::default()
                            },
                        },
                    );
                    if spinner_visible(view.since, Instant::now()) {
                        pen.icon_turned(
                            "spinner",
                            Rect::new(
                                (width - 24.0) / 2.0,
                                (height - 24.0 + title_bar::BAR_H) / 2.0,
                                24.0,
                                24.0,
                            ),
                            colors.ink,
                            s.spin(),
                        );
                    }
                });
            },
        )
        .size_full();
        div()
            .size_full()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &gpui::MouseDownEvent, window, cx| {
                    let width = f32::from(window.viewport_size().width);
                    let bar = title_bar::layout(
                        Rect::new(0.0, 0.0, width, title_bar::BAR_H),
                        &this.pending.track,
                        &this.pending.device,
                        title_bar::Seats {
                            power: true,
                            channel: true,
                            ..Default::default()
                        },
                    );
                    let x = f32::from(e.position.x);
                    let y = f32::from(e.position.y);
                    if bar.close.contains(x, y) {
                        cancel(&this.studio, id);
                        window.remove_window();
                    } else if y < title_bar::BAR_H {
                        window.start_window_move();
                    }
                    cx.notify();
                }),
            )
            .child(surface)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fast_editor_never_shows_the_spinner() {
        let start = Instant::now();
        assert!(!spinner_visible(start, start));
        assert!(!spinner_visible(start, start + Duration::from_millis(149)));
        assert!(spinner_visible(start, start + Duration::from_millis(150)));
    }

    #[test]
    fn previous_editor_size_belongs_to_its_plugin_type() {
        let mut sizes = HashMap::new();
        sizes.insert("first".to_string(), (1600, 1000));
        assert_eq!(
            opening_size(&sizes, "first", 2.0),
            (800.0, 500.0 + title_bar::BAR_H)
        );
        assert_eq!(
            opening_size(&sizes, "second", 2.0),
            (FIRST_SIZE.0 / 2.0, FIRST_SIZE.1 / 2.0 + title_bar::BAR_H)
        );
    }

    #[test]
    fn closing_the_placeholder_cancels_a_waiting_request() {
        let studio = Rc::new(RefCell::new(Studio::sample()));
        studio
            .borrow_mut()
            .mixer
            .open_later
            .push(crate::loading::OpenLater {
                track: xirius_core::id::TrackId(1),
                id: PluginId(2),
                foreground_from: 0,
            });
        cancel(&studio, PluginId(2));
        assert!(studio.borrow().mixer.open_later.is_empty());
    }
}
