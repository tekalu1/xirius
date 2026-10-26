//! UI-061: how waiting looks. Work that goes on without stopping the person shows a thin line;
//! work that stops them shows a veil (with the job and Esc to cancel) only once it has lasted
//! 0.2 s, so a quick one never flashes; a change waiting to land is drawn at once as hatching,
//! and says 「反映中…」 only after 0.3 s.
//!
//! The jobs live in one registry per thread ([`begin`], [`end`]); the view is a pure function of
//! the jobs and the clock, so tests drive it with their own instants.

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use gpui::{PathBuilder, point, px};
use xirius_design::{FontSize, Layout, Radius, Space, Weight};

use crate::draw::{Align, Pen};
use crate::studio::{Geometry, Rect, Studio};

/// A job that stops the person shows its veil after this long.
pub const OVERLAY_AFTER: Duration = Duration::from_millis(200);
/// A change waiting to land says so after this long.
pub const REFLECTING_AFTER: Duration = Duration::from_millis(300);
/// What the veil's card and the waiting say.
pub const REFLECTING: &str = "反映中…";
pub const CANCEL: &str = "中止";

const LINE_H: f32 = 2.0;
const CARD_W: f32 = 280.0;
const CARD_H: f32 = 84.0;
const HATCH_STEP: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The person keeps working: a thin line.
    Line,
    /// The person waits: a veil after [`OVERLAY_AFTER`], Esc cancels.
    Block,
    /// A change drawn at once and waiting to land: hatching, and 「反映中…」 after
    /// [`REFLECTING_AFTER`]. The thin line runs too.
    Reflect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Id(u64);

#[derive(Clone, Debug)]
struct Job {
    id: Id,
    kind: Kind,
    label: String,
    since: Instant,
    cancelled: bool,
}

#[derive(Debug, Default)]
pub struct Waits {
    jobs: Vec<Job>,
    next: u64,
}

/// What is to be seen at one moment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    /// The thin line under the top bar.
    pub line: bool,
    /// The veil's job name, once a blocking job has lasted [`OVERLAY_AFTER`].
    pub veil: Option<String>,
    /// A change is waiting to land (hatch what it will change).
    pub hatch: bool,
    /// 「反映中…」 is said.
    pub reflecting: bool,
}

impl Waits {
    pub fn begin(&mut self, kind: Kind, label: &str, now: Instant) -> Id {
        self.next += 1;
        let id = Id(self.next);
        self.jobs.push(Job {
            id,
            kind,
            label: label.to_string(),
            since: now,
            cancelled: false,
        });
        id
    }

    pub fn end(&mut self, id: Id) {
        self.jobs.retain(|j| j.id != id);
    }

    pub fn any(&self) -> bool {
        !self.jobs.is_empty()
    }

    /// The jobs began before `now` and not ended, by kind.
    pub fn count(&self, kind: Kind) -> usize {
        self.jobs.iter().filter(|j| j.kind == kind).count()
    }

    pub fn view(&self, now: Instant) -> View {
        let age = |j: &Job| now.saturating_duration_since(j.since);
        View {
            line: self
                .jobs
                .iter()
                .any(|j| matches!(j.kind, Kind::Line | Kind::Reflect)),
            veil: self
                .jobs
                .iter()
                .find(|j| j.kind == Kind::Block && !j.cancelled && age(j) >= OVERLAY_AFTER)
                .map(|j| j.label.clone()),
            hatch: self.jobs.iter().any(|j| j.kind == Kind::Reflect),
            reflecting: self
                .jobs
                .iter()
                .any(|j| j.kind == Kind::Reflect && age(j) >= REFLECTING_AFTER),
        }
    }

    /// Esc: cancels the blocking jobs whose veil is up. False when there is no veil (the key
    /// then does what it does everywhere else).
    pub fn cancel(&mut self, now: Instant) -> bool {
        let mut any = false;
        for j in &mut self.jobs {
            if j.kind == Kind::Block
                && !j.cancelled
                && now.saturating_duration_since(j.since) >= OVERLAY_AFTER
            {
                j.cancelled = true;
                any = true;
            }
        }
        any
    }

    /// Whether the person cancelled the job (its owner stops, then [`Waits::end`]s it).
    pub fn cancelled(&self, id: Id) -> bool {
        self.jobs.iter().any(|j| j.id == id && j.cancelled)
    }

    /// When the view next changes by itself, to draw it again then.
    pub fn next_change(&self, now: Instant) -> Option<Duration> {
        self.jobs
            .iter()
            .flat_map(|j| {
                let after = match j.kind {
                    Kind::Line => return None,
                    Kind::Block if j.cancelled => return None,
                    Kind::Block => OVERLAY_AFTER,
                    Kind::Reflect => REFLECTING_AFTER,
                };
                let at = j.since + after;
                (at > now).then(|| at - now)
            })
            .min()
    }
}

thread_local! {
    static WAITS: RefCell<Waits> = RefCell::new(Waits::default());
    static SCRIPT_TIME: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// The window script drives the same registry and painting with a controlled clock.
/// Ordinary windows leave this unset and use the monotonic clock.
pub(crate) fn script_time(now: Option<Instant>) {
    SCRIPT_TIME.set(now);
}

pub(crate) fn now() -> Instant {
    SCRIPT_TIME.get().unwrap_or_else(Instant::now)
}

/// A job begins now.
pub fn begin(kind: Kind, label: &str) -> Id {
    WAITS.with(|w| w.borrow_mut().begin(kind, label, now()))
}

pub fn end(id: Id) {
    WAITS.with(|w| w.borrow_mut().end(id));
}

pub fn cancelled(id: Id) -> bool {
    WAITS.with(|w| w.borrow().cancelled(id))
}

/// Esc was pressed: true when it cancelled a job behind a veil.
pub fn cancel() -> bool {
    WAITS.with(|w| w.borrow_mut().cancel(now()))
}

pub fn view() -> View {
    WAITS.with(|w| w.borrow().view(now()))
}

/// True while the view will change by itself (the next frame is asked for).
pub fn pending() -> bool {
    WAITS.with(|w| w.borrow().next_change(now()).is_some())
}

/// Hatching over `r` (a change that has not landed yet).
pub fn hatch(pen: &mut Pen, r: Rect, color: gpui::Hsla) {
    if r.w < 1.0 || r.h < 1.0 {
        return;
    }
    pen.mask(r, |pen| {
        let (ox, oy) = pen.origin;
        let mut path = PathBuilder::stroke(px(1.0));
        let mut x = r.x - r.h;
        while x < r.x + r.w {
            path.move_to(point(px(ox + x), px(oy + r.y + r.h)));
            path.line_to(point(px(ox + x + r.h), px(oy + r.y)));
            x += HATCH_STEP;
        }
        if let Ok(path) = path.build() {
            pen.window.paint_path(path, color);
        }
    });
}

/// The thin line, the veil and 「反映中…」 over the body.
pub fn paint(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    if crate::unsaved::paint(pen, studio, g) {
        return;
    }
    if crate::opening_view::paint(pen, studio, g) {
        return;
    }
    let mut v = view();
    let saving = studio.saving.as_ref();
    if let Some(save) = saving {
        v.line = save.cancellable();
        v.veil = save.veil().then(|| save.label().to_string());
    }
    let c = pen.c;
    if v.line {
        pen.fill(Rect::new(0.0, 0.0, g.size.0, LINE_H), c.fill);
    }
    if v.reflecting {
        let w = pen.measure(REFLECTING, FontSize::Small, Weight::Regular) + Space::X4.px();
        let r = Rect::new(
            g.size.0 - w - Space::X3.px(),
            LINE_H + Space::X2.px(),
            w,
            Layout::MENU_ROW_H,
        );
        pen.round(r, c.surface_3, Radius::S);
        pen.text(
            REFLECTING,
            FontSize::Small,
            Weight::Regular,
            c.ink,
            r,
            Align::Center,
        );
    }
    if let Some(label) = v.veil {
        pen.fill(Rect::new(0.0, 0.0, g.size.0, g.size.1), c.veil);
        let card = saving.map_or_else(
            || {
                Rect::new(
                    (g.size.0 - CARD_W) / 2.0,
                    (g.size.1 - CARD_H) / 2.0,
                    CARD_W,
                    CARD_H,
                )
            },
            |save| crate::saving::card(g, save.cancellable()),
        );
        pen.floating(card, c.surface_2, Radius::M);
        let pad = Space::X4.px();
        pen.text(
            &label,
            FontSize::Lead,
            Weight::Medium,
            c.ink_strong,
            Rect::new(card.x + pad, card.y + pad, card.w - pad * 2.0, 28.0),
            Align::Left,
        );
        if let Some(save) = saving.filter(|save| save.cancellable()) {
            let track = Rect::new(
                card.x + pad,
                card.y + pad + 28.0 + Space::X2.px(),
                card.w - pad * 2.0,
                Space::X1.px(),
            );
            pen.round(track, c.surface_3, Radius::S);
            pen.round(
                Rect::new(
                    track.x,
                    track.y,
                    track.w * save.progress.fraction(),
                    track.h,
                ),
                c.fill,
                Radius::S,
            );
        }
        if saving.is_none_or(|save| save.cancellable()) {
            pen.text(
                &format!("{CANCEL}  Esc"),
                FontSize::Small,
                Weight::Regular,
                c.ink,
                Rect::new(
                    card.x + pad,
                    card.y + card.h - pad - 20.0,
                    card.w - pad * 2.0,
                    20.0,
                ),
                Align::Right,
            );
        }
    }
}

pub(crate) fn save_cancel_rect(g: &Geometry) -> Rect {
    let card = crate::saving::card(g, true);
    Rect::new(
        card.x + Space::X4.px(),
        card.y + card.h - Space::X4.px() - 20.0,
        card.w - Space::X4.px() * 2.0,
        20.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn a_job_under_200_ms_never_shows_a_veil_and_one_over_does() {
        let t0 = Instant::now();
        let mut w = Waits::default();
        let id = w.begin(Kind::Block, "取り込み", t0);
        assert_eq!(w.view(t0).veil, None);
        assert_eq!(w.view(t0 + ms(199)).veil, None);
        assert_eq!(w.view(t0 + ms(200)).veil.as_deref(), Some("取り込み"));
        // A job that finished in time leaves nothing.
        w.end(id);
        assert_eq!(w.view(t0 + ms(250)), View::default());
        let quick = w.begin(Kind::Block, "速い", t0 + ms(300));
        assert_eq!(w.view(t0 + ms(450)).veil, None);
        w.end(quick);
        assert!(!w.any());
    }

    #[test]
    fn esc_cancels_only_behind_a_veil() {
        let t0 = Instant::now();
        let mut w = Waits::default();
        let id = w.begin(Kind::Block, "取り込み", t0);
        assert!(!w.cancel(t0 + ms(100)), "no veil yet, Esc is not taken");
        assert!(!w.cancelled(id));
        assert!(w.cancel(t0 + ms(250)));
        assert!(w.cancelled(id));
        assert_eq!(
            w.view(t0 + ms(260)).veil,
            None,
            "a cancelled job's veil goes"
        );
        assert!(!w.cancel(t0 + ms(300)), "nothing left to cancel");
        w.end(id);
        assert!(!w.cancelled(id));
    }

    #[test]
    fn work_that_goes_on_shows_a_line_and_never_a_veil() {
        let t0 = Instant::now();
        let mut w = Waits::default();
        w.begin(Kind::Line, "波形", t0);
        let v = w.view(t0 + ms(5000));
        assert!(v.line && v.veil.is_none() && !v.hatch && !v.reflecting);
        assert!(!w.cancel(t0 + ms(5000)));
    }

    #[test]
    fn a_change_waiting_to_land_is_hatched_at_once_and_says_so_after_300_ms() {
        let t0 = Instant::now();
        let mut w = Waits::default();
        w.begin(Kind::Reflect, "反映", t0);
        let at = |n| w.view(t0 + ms(n));
        assert!(at(0).hatch && !at(0).reflecting);
        assert!(at(299).hatch && !at(299).reflecting);
        assert!(at(300).hatch && at(300).reflecting);
        assert!(at(300).line);
        assert_eq!(w.next_change(t0 + ms(100)), Some(ms(200)));
        assert_eq!(w.next_change(t0 + ms(300)), None);
    }

    #[test]
    fn the_next_frame_is_asked_for_only_while_a_threshold_is_ahead() {
        let t0 = Instant::now();
        let mut w = Waits::default();
        assert_eq!(w.next_change(t0), None);
        w.begin(Kind::Line, "波形", t0);
        assert_eq!(w.next_change(t0), None);
        let id = w.begin(Kind::Block, "取り込み", t0);
        assert_eq!(w.next_change(t0 + ms(50)), Some(ms(150)));
        assert_eq!(w.next_change(t0 + ms(200)), None);
        w.end(id);
        assert_eq!(w.count(Kind::Block), 0);
    }
}
