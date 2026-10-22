//! Keeps the unchanged song overview out of the roll's redraw work.
use crate::{
    browser::Browser,
    draw::{Pen, paint_body},
    paint::Colors,
    studio::{DrawerTab, Geometry, Hit, Studio},
    tree::RowKind,
};
use gpui::{Context, Render, Window, canvas, prelude::*};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{cell::RefCell, rc::Rc, sync::Arc};
use xirius_core::{
    id::{ClipId, TrackId},
    model::Project,
};

/// Times the screen asked the overview whether it changed (once a frame), and times the
/// overview was actually painted: the frame measurement of UI-039 reads both.
static SYNCS: AtomicU64 = AtomicU64::new(0);
static PAINTS: AtomicU64 = AtomicU64::new(0);

/// `(frames the overview was asked about, times it was painted)` so far.
pub fn counts() -> (u64, u64) {
    (
        SYNCS.load(Ordering::Relaxed),
        PAINTS.load(Ordering::Relaxed),
    )
}

#[derive(PartialEq)]
struct Key {
    geometry: Geometry,
    scroll: (f32, f32),
    beat_w: f32,
    hover: Hit,
    row: Option<RowKind>,
    clip: Option<(TrackId, ClipId)>,
    tab: DrawerTab,
    drawer_open: bool,
    arrange_visible: bool,
    tempo_visible: bool,
    browser: Browser,
    /// Each row, whether it is a folded folder, and whether it is widened.
    rows: Vec<(RowKind, bool, bool)>,
    /// Anything the arrangement's own state shows (selection, lanes, zoom).
    arrange: u64,
    /// The loop band and its shade look different while the range repeats.
    looping: bool,
}

impl Key {
    /// What the overview shows of `s` laid out by `g`.
    fn of(s: &Studio, g: Geometry) -> Key {
        // When the song's files were last read is not shown: it changes every second.
        let mut browser = s.browser.clone();
        browser.pool_read = None;
        Key {
            geometry: g,
            scroll: s.scroll,
            beat_w: s.beat_w,
            // The mixer is painted over the cached body, as is the
            // playhead, so pointing at it does not repaint it.
            hover: match s.hover {
                Hit::Mixer(_) => Hit::None,
                h => h,
            },
            row: s.selected_row,
            clip: s.selected_clip,
            tab: s.tab,
            drawer_open: s.drawer_open,
            arrange_visible: s.arrange_visible,
            tempo_visible: s.tempo_visible,
            browser,
            rows: s
                .tree()
                .rows
                .iter()
                .map(|r| {
                    (
                        r.kind,
                        match r.kind {
                            RowKind::Folder(id) => s.is_collapsed(id),
                            RowKind::Track(_) => false,
                        },
                        r.tall,
                    )
                })
                .collect(),
            arrange: s.arrange.generation,
            looping: s.transport.looping,
        }
    }
}

pub struct Background {
    studio: Rc<RefCell<Studio>>,
    colors: Rc<Colors>,
    project: Option<Arc<Project>>,
    key: Option<Key>,
}

impl Background {
    pub fn new(studio: Rc<RefCell<Studio>>, colors: Rc<Colors>) -> Self {
        Self {
            studio,
            colors,
            project: None,
            key: None,
        }
    }
    /// Paints in `colors` from now on (the theme changed, SET-002).
    pub fn set_colors(&mut self, colors: Rc<Colors>, cx: &mut Context<Self>) {
        self.colors = colors;
        self.key = None;
        cx.notify();
    }

    pub fn sync(&mut self, g: Geometry, cx: &mut Context<Self>) {
        SYNCS.fetch_add(1, Ordering::Relaxed);
        let s = self.studio.borrow();
        let project = s.project();
        let key = Key::of(&s, g);
        if self.key.as_ref() != Some(&key)
            || !self
                .project
                .as_ref()
                .is_some_and(|p| Arc::ptr_eq(p, &project))
        {
            self.project = Some(project);
            self.key = Some(key);
            cx.notify();
        }
    }
}

impl Render for Background {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let studio = self.studio.clone();
        let colors = self.colors.clone();
        canvas(
            |_, _, _| (),
            move |b, (), window, cx| {
                let s = studio.borrow();
                PAINTS.fetch_add(1, Ordering::Relaxed);
                let started = std::time::Instant::now();
                let mut pen = Pen {
                    window,
                    cx,
                    c: &colors,
                    origin: (f32::from(b.origin.x), f32::from(b.origin.y)),
                };
                paint_body(
                    &mut pen,
                    &s,
                    f32::from(b.size.width),
                    f32::from(b.size.height),
                );
                s.roll.paint_us.set(started.elapsed().as_micros() as u64);
            },
        )
        .size_full()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UI-039: the song's files are read again every second while nothing changes; the overview
    /// is not painted again for it.
    #[test]
    fn reading_the_song_files_again_does_not_change_what_the_overview_shows() {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 860.0);
        s.follow_pool();
        let before = Key::of(&s, g);
        let read = s.browser.pool_read.expect("read");
        s.browser.pool_read = Some((read.0, read.1 - std::time::Duration::from_secs(2)));
        assert!(!s.follow_pool(), "the files did not change");
        assert!(
            s.browser.pool_read.is_some_and(|r| r.1 > read.1),
            "read again"
        );
        assert!(Key::of(&s, g) == before);
    }
}
