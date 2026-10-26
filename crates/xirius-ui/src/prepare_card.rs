//! ENG-064, approved 2026-10-26: a blocked background preparation, its failures,
//! explicit retry/continue, and cancellation through cleanup to the start screen.
//! The older staged transport/card supports non-window fixtures only; every
//! transport constructed on the GPUI event-loop thread uses the background path.

use gpui::{accesskit, div, prelude::*, px};
use std::time::{Duration, Instant};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use xirius_design::{FontSize, Radius, Space, Weight};
use xirius_session::output::OutputMode;
use xirius_session::starting::{Progress, UnitState};

use crate::browser::text_width;
use crate::draw::{Align, Pen};
use crate::studio::{Geometry, Rect, Studio};
use crate::transport::StartStep;

pub const TITLE: &str = "プラグインを準備中";
pub const LATER: &str = "後で読む";

const W: f32 = 300.0;
/// The card keeps this far from the body's sides when the body is narrow.
const EDGE: f32 = 12.0;
const PAD: f32 = 12.0;
const GAP: f32 = 8.0;
const HEAD_H: f32 = 20.0;
const BAR_H: f32 = 4.0;
const ROW_H: f32 = 20.0;
const ROW_GAP: f32 = 2.0;
const COL_GAP: f32 = 10.0;
const ICON: f32 = 12.0;
const BUTTON_H: f32 = 28.0;
/// At most this many tracks are listed: the ones around the one under way.
const LISTED: usize = 16;
/// How long the bar takes to move to a new count.
const BAR_MOVE: Duration = Duration::from_millis(200);

#[derive(Clone, Debug, PartialEq)]
struct DetailLines {
    text: String,
    width: f32,
    lines: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub(crate) background: bool,
    pub(crate) cancelling: bool,
    pub(crate) failure: Option<String>,
    pub(crate) failures: Vec<xirius_session::build::PreparationFailure>,
    failure_scroll: usize,
    detail: Option<usize>,
    detail_scroll: usize,
    detail_max: Cell<usize>,
    detail_lines: RefCell<Option<DetailLines>>,
    pub progress: Progress,
    /// The pointer is over 後で読む.
    pub hover: bool,
    /// The bar moves from `from` to `to` (parts of the whole) from `at` on.
    from: f32,
    to: f32,
    at: Instant,
}

/// Where the parts of the card are.
#[derive(Clone, Debug, PartialEq)]
pub struct CardLayout {
    pub rect: Rect,
    pub head: Rect,
    pub bar: Rect,
    /// Each track listed, with its index in [`Progress::units`].
    pub rows: Vec<(usize, Rect)>,
    pub button: Rect,
    pub retry: Option<Rect>,
    pub proceed: Option<Rect>,
}

impl Card {
    #[cfg(feature = "loading-probe")]
    pub(crate) fn detail_progress(&self) -> Option<(usize, usize)> {
        self.detail
            .map(|_| (self.detail_scroll, self.detail_max.get()))
    }

    #[cfg(feature = "loading-probe")]
    pub(crate) fn detail_content(&self) -> Option<String> {
        self.detail_lines
            .borrow()
            .as_ref()
            .map(|d| d.lines.concat())
    }

    pub fn new(progress: Progress) -> Card {
        let to = fraction(&progress);
        Card {
            background: false,
            cancelling: false,
            failure: None,
            failures: Vec::new(),
            failure_scroll: 0,
            detail: None,
            detail_scroll: 0,
            detail_max: Cell::new(0),
            detail_lines: RefCell::new(None),
            progress,
            hover: false,
            from: to,
            to,
            at: Instant::now(),
        }
    }

    /// Takes in how far the start is now; the bar moves to it.
    pub fn set(&mut self, progress: Progress, now: Instant) {
        let to = fraction(&progress);
        if to != self.to {
            self.from = self.bar(now);
            self.to = to;
            self.at = now;
        }
        self.progress = progress;
    }

    /// How much of the bar is filled at `now`.
    pub fn bar(&self, now: Instant) -> f32 {
        let t = (now.saturating_duration_since(self.at).as_secs_f32() / BAR_MOVE.as_secs_f32())
            .min(1.0);
        let eased = 1.0 - (1.0 - t) * (1.0 - t);
        self.from + (self.to - self.from) * eased
    }

    /// 「3/8」.
    pub fn count(&self) -> String {
        format!("{}/{}", self.progress.done, self.progress.units.len())
    }

    /// The tracks listed: all of them, or [`LISTED`] around the first that is not done.
    fn listed(&self) -> std::ops::Range<usize> {
        let n = self.progress.units.len();
        if n <= LISTED {
            return 0..n;
        }
        let first = self
            .progress
            .units
            .iter()
            .position(|(_, s)| *s != UnitState::Done)
            .unwrap_or(n);
        let start = (first / 2 * 2)
            .saturating_sub(4)
            .min((n - LISTED).div_ceil(2) * 2);
        start..(start + LISTED).min(n)
    }

    pub fn layout(&self, size: (f32, f32)) -> CardLayout {
        if self.background {
            return self.background_layout(size);
        }
        let w = W.min(size.0 - EDGE * 2.0).max(0.0);
        let listed = self.listed();
        let lines = listed.len().div_ceil(2) as f32;
        let list_h = (lines * ROW_H + (lines - 1.0).max(0.0) * ROW_GAP).max(0.0);
        let h = PAD + HEAD_H + GAP + BAR_H + GAP + list_h + GAP + BUTTON_H + PAD;
        let x = ((size.0 - w) / 2.0).max(0.0);
        let y = ((size.1 - h) / 2.0).max(0.0);
        let inner = w - PAD * 2.0;
        let head = Rect::new(x + PAD, y + PAD, inner, HEAD_H);
        let bar = Rect::new(x + PAD, head.bottom() + GAP, inner, BAR_H);
        let col = (inner - COL_GAP) / 2.0;
        let top = bar.bottom() + GAP;
        let rows = listed
            .clone()
            .enumerate()
            .map(|(i, at)| {
                let (line, side) = ((i / 2) as f32, (i % 2) as f32);
                (
                    at,
                    Rect::new(
                        x + PAD + side * (col + COL_GAP),
                        top + line * (ROW_H + ROW_GAP),
                        col,
                        ROW_H,
                    ),
                )
            })
            .collect();
        let bw = text_width(LATER, FontSize::Small) + Space::X3.px() * 2.0;
        let button = Rect::new(x + w - PAD - bw, y + h - PAD - BUTTON_H, bw, BUTTON_H);
        CardLayout {
            rect: Rect::new(x, y, w, h),
            head,
            bar,
            rows,
            button,
            retry: None,
            proceed: None,
        }
    }
}

fn fraction(p: &Progress) -> f32 {
    if p.units.is_empty() {
        1.0
    } else {
        p.done as f32 / p.units.len() as f32
    }
}

impl Studio {
    pub(crate) fn sound_preparing(&self) -> bool {
        self.preparing.is_some() || self.transport.background_starting()
    }

    fn show_background_preparation(&mut self) {
        if self.preparing.is_none() && self.transport.background_starting() {
            let mut card = Card::new(self.transport.start_progress().unwrap_or_default());
            card.background = true;
            self.preparing = Some(card);
        }
    }
    /// Gets the instruments and effects of the song just opened ready (ENG-064): the player
    /// starts a little at a time behind the card, and the sound device opens when it is done.
    /// A window without the device (`--silent`, the tests) starts its player when a note or a
    /// play first wants it, as before; a test calls [`Studio::prepare_song`] itself.
    pub(crate) fn prepare_on_open(&mut self) {
        self.preparing = None;
        if std::mem::take(&mut self.skip_preparation_once) {
            return;
        }
        if self.transport.mode == OutputMode::Device || self.transport.is_windowed() {
            self.prepare_song();
        }
    }

    /// Begins starting the player for the song, behind the card while plugins are to open.
    pub fn prepare_song(&mut self) {
        let p = self.project();
        if !self.transport.begin_start(&p, self.song.dir()) {
            return;
        }
        match self.transport.start_progress() {
            Some(progress)
                if !progress.units.is_empty() || self.transport.background_starting() =>
            {
                let mut card = Card::new(progress);
                card.background = self.transport.background_starting();
                self.preparing = Some(card);
            }
            // Nothing to open: the player and the device start now.
            _ => {
                self.step_preparing();
            }
        }
    }

    /// Once a frame: opens a little more while the card is up, and closes it once the player
    /// started. True while the card is up.
    pub(crate) fn poll_preparing(&mut self) -> bool {
        self.show_background_preparation();
        if self.preparing.is_none() {
            return false;
        }
        self.step_preparing();
        true
    }

    /// Once a frame: does a little more of the mix a change of the song began, and says what the
    /// host processes said about plugins that were not in yet (ENG-065). True while there is
    /// more to do, so the frames keep coming.
    pub(crate) fn poll_swap(&mut self) -> bool {
        let (busy, said) = self.transport.step_swap();
        if !said.is_empty() {
            self.error = Some(said.join("。"));
        }
        busy
    }

    fn step_preparing(&mut self) {
        if self
            .preparing
            .as_ref()
            .is_some_and(|c| c.background && c.failure.is_some() && !c.cancelling)
        {
            return;
        }
        if self
            .preparing
            .as_ref()
            .is_some_and(|c| c.background && c.cancelling)
            && !self.transport.preparing()
        {
            self.finish_preparation_cancel();
            return;
        }
        let p = self.project();
        match self.transport.step_start(&p, self.song.dir()) {
            Some(StartStep::Going(progress)) => {
                if let Some(card) = &mut self.preparing
                    && !card.cancelling
                {
                    card.set(progress, Instant::now());
                }
            }
            Some(StartStep::Failed(failures)) => {
                if let Some(card) = &mut self.preparing {
                    card.progress = self.transport.start_progress().unwrap_or_default();
                    card.failures = failures;
                }
            }
            Some(StartStep::Done(started)) => {
                if let Err(error) = &started
                    && let Some(card) = &mut self.preparing
                    && card.background
                {
                    card.failure = Some(error.clone());
                    card.failure_scroll = 0;
                    return;
                }
                self.preparing = None;
                if let Err(e) = started {
                    self.error = Some(e);
                }
            }
            Some(StartStep::Cancelled) => self.finish_preparation_cancel(),
            None => self.preparing = None,
        }
    }

    /// 後で読む: the card closes and the song can be edited; what is left opens when it is
    /// first needed.
    pub fn prepare_later(&mut self) {
        self.show_background_preparation();
        if let Some(card) = &mut self.preparing
            && card.background
        {
            if !card.cancelling {
                let now = Instant::now();
                let value = card.bar(now);
                card.from = value;
                card.to = value;
                card.at = now;
                card.cancelling = true;
                card.failure = None;
                self.transport.cancel_preparation();
            }
            return;
        }
        self.preparing = None;
    }

    fn finish_preparation_cancel(&mut self) {
        if self.opening.is_some() {
            return;
        }
        let card = self.preparing.take();
        self.skip_preparation_once = true;
        if self.request_open(crate::opening::Request::New) {
            // Keep the same cleanup veil until the old Session is closed, too.
            self.preparing = card;
        } else {
            self.skip_preparation_once = false;
            self.preparing = card;
        }
    }

    /// A press while the card is up. Every press belongs to the card.
    pub fn prepare_press(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        self.show_background_preparation();
        let Some(card) = &self.preparing else {
            return false;
        };
        let layout = card.layout(g.size);
        if layout.retry.is_some_and(|r| r.contains(x, y)) {
            self.prepare_retry();
        } else if layout.proceed.is_some_and(|r| r.contains(x, y)) {
            self.prepare_continue();
        } else if layout.button.contains(x, y) {
            self.prepare_later();
        }
        true
    }

    pub(crate) fn prepare_retry(&mut self) {
        if !self
            .preparing
            .as_ref()
            .is_some_and(|c| !c.cancelling && (c.failure.is_some() || !c.failures.is_empty()))
        {
            return;
        }
        let project = self.project();
        if self.transport.retry_preparation(&project, self.song.dir()) {
            let mut card = Card::new(self.transport.start_progress().unwrap_or_default());
            card.background = true;
            self.preparing = Some(card);
        }
    }

    pub(crate) fn prepare_continue(&mut self) {
        if self
            .preparing
            .as_ref()
            .is_some_and(|c| !c.cancelling && c.failure.is_none() && !c.failures.is_empty())
            && self.transport.continue_preparation()
            && let Some(card) = &mut self.preparing
        {
            card.failures.clear();
        }
    }

    pub(crate) fn prepare_scroll(&mut self, amount: f32) -> bool {
        let Some(card) = &mut self.preparing else {
            return false;
        };
        if card.detail.is_some() && card.detail_max.get() > 0 {
            let before = card.detail_scroll;
            if amount > 0.0 {
                card.detail_scroll = card.detail_scroll.saturating_sub(1);
            } else if amount < 0.0 {
                card.detail_scroll = (card.detail_scroll + 1).min(card.detail_max.get());
            }
            return before != card.detail_scroll;
        }
        let before = card.failure_scroll;
        if amount > 0.0 {
            card.failure_scroll = card.failure_scroll.saturating_sub(1);
        } else if amount < 0.0 {
            card.failure_scroll =
                (card.failure_scroll + 1).min(card.failures.len().saturating_sub(1));
        }
        before != card.failure_scroll
    }

    pub fn prepare_hover(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        self.show_background_preparation();
        let Some(card) = &mut self.preparing else {
            return false;
        };
        let layout = card.layout(g.size);
        let detail = layout
            .rows
            .iter()
            .find(|(_, r)| r.contains(x, y))
            .map(|(i, _)| *i);
        let hover = layout.button.contains(x, y);
        let changed = hover != card.hover || detail != card.detail;
        if detail != card.detail {
            card.detail_scroll = 0;
            card.detail_max.set(0);
        }
        card.detail = detail;
        card.hover = hover;
        changed
    }

    /// Esc cancels preparation. Arrow/page keys scroll the failure detail/list.
    pub fn prepare_key(&mut self, key: &str) -> bool {
        self.show_background_preparation();
        if self.preparing.is_none() {
            return false;
        }
        if key == "escape" {
            self.prepare_later();
        } else if key == "down" || key == "pagedown" {
            self.prepare_scroll(-1.0);
        } else if key == "up" || key == "pageup" {
            self.prepare_scroll(1.0);
        }
        true
    }
}

/// The veil over the body and the card on it.
pub fn paint(pen: &mut Pen, card: &Card, g: &Geometry, turn: f32) {
    if card.background {
        paint_background(pen, card, g, turn);
        return;
    }
    let c = pen.c;
    pen.fill(Rect::new(0.0, 0.0, g.size.0, g.size.1), c.veil);
    let l = card.layout(g.size);
    pen.floating(l.rect, c.surface_1, Radius::M);
    let icon = |r: Rect| Rect::new(r.x, r.y + (r.h - ICON) / 2.0, ICON, ICON);
    pen.icon_turned("spinner", icon(l.head), c.ink_strong, turn);
    let text = Rect::new(
        l.head.x + ICON + Space::X2.px(),
        l.head.y,
        l.head.w - ICON - Space::X2.px(),
        l.head.h,
    );
    pen.text(
        TITLE,
        FontSize::Body,
        Weight::Medium,
        c.ink_strong,
        text,
        Align::Left,
    );
    pen.text(
        &card.count(),
        FontSize::Body,
        Weight::Regular,
        c.ink,
        text,
        Align::Right,
    );
    pen.round(l.bar, c.surface_3, Radius::Xs);
    let filled = l.bar.w * card.bar(Instant::now()).clamp(0.0, 1.0);
    if filled > 0.0 {
        pen.round(
            Rect::new(l.bar.x, l.bar.y, filled, l.bar.h),
            c.fill,
            Radius::Xs,
        );
    }
    for (i, r) in &l.rows {
        let (name, state) = &card.progress.units[*i];
        let mark = icon(*r);
        let ink = match state {
            UnitState::Done => {
                pen.icon("check", mark, c.ink_strong);
                c.ink_strong
            }
            UnitState::Working => {
                pen.icon_turned("spinner", mark, c.ink, turn);
                c.ink
            }
            UnitState::Waiting => {
                pen.icon("dot", mark, c.ink);
                c.ink
            }
        };
        let label = Rect::new(r.x + ICON + 6.0, r.y, r.w - ICON - 6.0, r.h);
        pen.mask(label, |pen| {
            pen.text(
                name,
                FontSize::Small,
                Weight::Regular,
                ink,
                label,
                Align::Left,
            );
        });
    }
    pen.round(
        l.button,
        if card.hover { c.thumb } else { c.surface_3 },
        Radius::S,
    );
    pen.text(
        LATER,
        FontSize::Small,
        Weight::Medium,
        c.ink_strong,
        l.button,
        Align::Center,
    );
}

impl Card {
    fn background_layout(&self, size: (f32, f32)) -> CardLayout {
        let failed = !self.cancelling && (self.failure.is_some() || !self.failures.is_empty());
        let count = if self.failure.is_some() {
            1
        } else {
            self.failures.len()
        };
        let shown = count.min(((size.1 - 160.0) / 64.0).clamp(1.0, 5.0) as usize);
        let h = if failed {
            124.0 + shown as f32 * 64.0
        } else {
            148.0
        };
        let w = W.min((size.0 - EDGE * 2.0).max(0.0));
        let r = Rect::new(
            ((size.0 - w) / 2.0).max(0.0),
            ((size.1 - h) / 2.0).max(0.0),
            w,
            h,
        );
        CardLayout {
            rect: r,
            head: Rect::new(r.x + PAD, r.y + PAD, w - PAD * 2.0, 20.0),
            bar: Rect::new(r.x + PAD, r.y + 96.0, w - PAD * 2.0, BAR_H),
            rows: if failed {
                (self.failure_scroll..(self.failure_scroll + shown).min(count))
                    .enumerate()
                    .map(|(row, index)| {
                        (
                            index,
                            Rect::new(
                                r.x + PAD,
                                r.y + 64.0 + row as f32 * 64.0,
                                w - PAD * 2.0,
                                60.0,
                            ),
                        )
                    })
                    .collect()
            } else {
                Vec::new()
            },
            retry: failed
                .then(|| Rect::new(r.x + PAD, r.bottom() - PAD - BUTTON_H, 64.0, BUTTON_H)),
            proceed: (failed && self.failure.is_none() && !self.failures.is_empty()).then(|| {
                Rect::new(
                    r.x + PAD + 68.0,
                    r.bottom() - PAD - BUTTON_H,
                    108.0,
                    BUTTON_H,
                )
            }),
            button: Rect::new(
                r.right() - PAD - 60.0,
                r.bottom() - PAD - BUTTON_H,
                60.0,
                BUTTON_H,
            ),
        }
    }
}
fn paint_background(pen: &mut Pen, card: &Card, g: &Geometry, turn: f32) {
    let l = card.layout(g.size);
    let c = pen.c;
    pen.fill(Rect::new(0.0, 0.0, g.size.0, g.size.1), c.veil);
    pen.floating(l.rect, c.surface_1, Radius::M);
    if !card.cancelling && (card.failure.is_some() || !card.failures.is_empty()) {
        paint_failures(pen, card, &l);
        return;
    }
    pen.icon_turned(
        "spinner",
        Rect::new(l.head.x, l.head.y + 4.0, ICON, ICON),
        c.ink_strong,
        turn,
    );
    let title = if card.cancelling {
        "中止しています"
    } else if card.failure.is_some() {
        "準備できません"
    } else {
        "音を準備しています"
    };
    pen.text(
        title,
        FontSize::Body,
        Weight::Medium,
        c.ink_strong,
        Rect::new(l.head.x + 20.0, l.head.y, l.head.w - 20.0, 20.0),
        Align::Left,
    );
    let count = if card.cancelling {
        "片付け中".into()
    } else {
        format!(
            "{} トラック中 {} トラック準備済み",
            card.progress.units.len(),
            card.progress.done
        )
    };
    pen.text(
        &count,
        FontSize::Small,
        Weight::Regular,
        c.ink,
        Rect::new(l.head.x, l.head.bottom() + 8.0, l.head.w, 20.0),
        Align::Left,
    );
    let current = if card.cancelling {
        String::new()
    } else if let Some(error) = &card.failure {
        error.clone()
    } else if let Some((name, _)) = card
        .progress
        .units
        .iter()
        .find(|(_, s)| *s == UnitState::Working)
    {
        format!("準備中：{name}")
    } else {
        "音の準備を仕上げています".into()
    };
    let row = Rect::new(l.head.x, l.head.bottom() + 36.0, l.head.w, 20.0);
    let current = crate::opening_view::shorten(&current, row.w, |s| {
        pen.measure(s, FontSize::Small, Weight::Regular)
    });
    pen.mask(row, |pen| {
        pen.text(
            &current,
            FontSize::Small,
            Weight::Regular,
            c.ink,
            row,
            Align::Left,
        );
    });
    pen.round(l.bar, c.surface_3, Radius::Xs);
    let filled = l.bar.w * card.bar(Instant::now()).clamp(0.0, 1.0);
    if filled > 0.0 {
        pen.round(
            Rect::new(l.bar.x, l.bar.y, filled, l.bar.h),
            c.fill,
            Radius::Xs,
        );
    }
    if !card.cancelling {
        pen.round(
            l.button,
            if card.hover { c.thumb } else { c.surface_3 },
            Radius::S,
        );
        pen.text(
            "中止",
            FontSize::Small,
            Weight::Medium,
            c.ink_strong,
            l.button,
            Align::Center,
        );
    }
}

fn paint_failures(pen: &mut Pen, card: &Card, l: &CardLayout) {
    let c = pen.c;
    pen.text(
        "準備できませんでした",
        FontSize::Body,
        Weight::Medium,
        c.ink_strong,
        l.head,
        Align::Left,
    );
    let count = format!(
        "{} トラック中 {} トラック準備済み",
        card.progress.units.len(),
        card.progress.done
    );
    pen.text(
        &count,
        FontSize::Small,
        Weight::Regular,
        c.ink,
        Rect::new(l.head.x, l.head.bottom() + 4.0, l.head.w, 20.0),
        Align::Left,
    );
    for (index, row) in &l.rows {
        let (name, reason) = if let Some(reason) = &card.failure {
            ("全体の準備", reason.as_str())
        } else {
            let failure = &card.failures[*index];
            (failure.name.as_str(), failure.reason.as_str())
        };
        for (text, offset, weight) in [(name, 0.0, Weight::Medium), (reason, 20.0, Weight::Regular)]
        {
            let text = crate::opening_view::shorten(text, row.w, |s| {
                pen.measure(s, FontSize::Small, weight)
            });
            pen.text(
                &text,
                FontSize::Small,
                weight,
                c.ink,
                Rect::new(row.x, row.y + offset, row.w, 20.0),
                Align::Left,
            );
        }
    }
    for (rect, label) in [
        (Some(l.button), "中止"),
        (l.retry, "再試行"),
        (l.proceed, "このまま開く"),
    ] {
        if let Some(rect) = rect {
            pen.round(rect, c.surface_3, Radius::S);
            pen.text(
                label,
                FontSize::Small,
                Weight::Medium,
                c.ink_strong,
                rect,
                Align::Center,
            );
        }
    }
    if let Some(index) = card.detail {
        let full = if let Some(reason) = &card.failure {
            reason.clone()
        } else if let Some(failure) = card.failures.get(index) {
            format!("{}\n{}", failure.reason, failure.name)
        } else {
            return;
        };
        let width = (f32::from(pen.window.viewport_size().width) - 24.0).clamp(0.0, 600.0);
        let mut cached = card.detail_lines.borrow_mut();
        if cached
            .as_ref()
            .is_none_or(|d| d.text != full || d.width != width)
        {
            *cached = Some(DetailLines {
                lines: pen.wrap_text(&full, width - 16.0, FontSize::Small, Weight::Regular),
                text: full,
                width,
            });
        }
        let lines = &cached.as_ref().expect("detail layout cached").lines;
        // Keep the actions visible and make every line reachable, even when a
        // plugin returns a long reason or several tracks share very long names.
        let capacity = ((l.button.y - GAP - l.rect.y - 32.0) / 20.0)
            .floor()
            .max(1.0) as usize;
        let visible = lines.len().min(capacity);
        let max = lines.len().saturating_sub(visible);
        card.detail_max.set(max);
        let offset = card.detail_scroll.min(max);
        let h = visible as f32 * 20.0 + if max > 0 { 32.0 } else { 16.0 };
        let tip = Rect::new(
            (l.rect.x + (l.rect.w - width) / 2.0).max(0.0),
            l.rect.y,
            width,
            h,
        );
        pen.floating(tip, c.surface_3, Radius::S);
        pen.mask(tip, |pen| {
            for (i, line) in lines.iter().skip(offset).take(visible).enumerate() {
                pen.text(
                    line,
                    FontSize::Small,
                    Weight::Regular,
                    c.ink,
                    Rect::new(
                        tip.x + 8.0,
                        tip.y + 8.0 + i as f32 * 20.0,
                        tip.w - 16.0,
                        20.0,
                    ),
                    Align::Left,
                );
            }
        });
        if max > 0 {
            pen.text(
                &format!("スクロールで全文を確認  {}/{}", offset + 1, max + 1),
                FontSize::Small,
                Weight::Regular,
                c.ink,
                Rect::new(tip.x + 8.0, tip.bottom() - 16.0, tip.w - 16.0, 16.0),
                Align::Left,
            );
        }
    }
}

pub(crate) fn accessible(s: Rc<RefCell<Studio>>, g: &Geometry) -> impl IntoElement {
    let mut root = div().id("preparing-accessible").absolute();
    let borrowed = s.borrow();
    let Some(card) = &borrowed.preparing else {
        return root;
    };
    let layout = card.layout(g.size);
    let r = layout.rect;
    let mut announce = format!(
        "音を準備しています。{} トラック中 {} トラック準備済み",
        card.progress.units.len(),
        card.progress.done
    );
    if card.cancelling {
        announce = "中止しています。片付け中".into();
    }
    if let Some(error) = &card.failure {
        announce.push_str(&format!("。{error}"));
    }
    for failure in &card.failures {
        announce.push_str(&format!("。{}: {}", failure.name, failure.reason));
    }
    root = root
        .left(px(r.x))
        .top(px(r.y + xirius_design::Layout::TOP_BAR_H))
        .w(px(r.w))
        .h(px(r.h))
        .role(gpui::Role::Dialog)
        .aria_label("音の準備")
        .a11y_synthetic_children(move |tree| {
            let mut status = accesskit::Node::new(accesskit::Role::Status);
            status.set_label(announce.clone());
            status.set_live(accesskit::Live::Polite);
            tree.push_child(tree.synthetic_node_id("prepare-status"), status);
        });
    let buttons = if card.cancelling {
        Vec::new()
    } else {
        vec![
            (Some(layout.button), "中止", 0),
            (layout.retry, "再試行", 1),
            (layout.proceed, "このまま開く", 2),
        ]
    };
    drop(borrowed);
    for (button, label, action) in buttons {
        let Some(button) = button else { continue };
        let studio = s.clone();
        root = root.child(
            div()
                .id(label)
                .absolute()
                .left(px(button.x - r.x))
                .top(px(button.y - r.y))
                .w(px(button.w))
                .h(px(button.h))
                .role(gpui::Role::Button)
                .aria_label(label)
                .on_a11y_action(accesskit::Action::Click, move |_, window, _| {
                    let mut studio = studio.borrow_mut();
                    match action {
                        1 => studio.prepare_retry(),
                        2 => studio.prepare_continue(),
                        _ => studio.prepare_later(),
                    }
                    window.refresh();
                }),
        );
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(states: &[UnitState]) -> Progress {
        Progress {
            units: states
                .iter()
                .enumerate()
                .map(|(i, s)| (format!("T{i}"), *s))
                .collect(),
            done: states.iter().filter(|s| **s == UnitState::Done).count(),
        }
    }

    #[test]
    fn the_card_counts_and_lays_out_two_columns_with_the_button_at_the_right() {
        use UnitState::*;
        let card = Card::new(progress(&[
            Done, Done, Done, Working, Waiting, Waiting, Waiting, Waiting,
        ]));
        assert_eq!(card.count(), "3/8");
        let l = card.layout((1200.0, 800.0));
        assert_eq!(l.rect.w, W);
        assert_eq!(l.rows.len(), 8);
        assert_eq!(l.rows[0].1.y, l.rows[1].1.y, "two to a line");
        assert!(l.rows[1].1.x > l.rows[0].1.x);
        assert_eq!(l.rows[2].1.y, l.rows[0].1.y + ROW_H + ROW_GAP);
        assert_eq!(l.button.right(), l.rect.right() - PAD);
        assert_eq!(l.button.h, BUTTON_H);
        // Centred in the body.
        assert!((l.rect.x * 2.0 + l.rect.w - 1200.0).abs() < 0.01);
        // A narrow body keeps the card inside.
        assert_eq!(card.layout((200.0, 800.0)).rect.w, 200.0 - EDGE * 2.0);
    }

    #[test]
    fn a_long_song_lists_the_tracks_around_the_one_under_way() {
        let mut states = vec![UnitState::Done; 30];
        states[20] = UnitState::Working;
        for s in &mut states[21..] {
            *s = UnitState::Waiting;
        }
        let card = Card::new(progress(&states));
        let listed: Vec<usize> = card
            .layout((1200.0, 800.0))
            .rows
            .iter()
            .map(|r| r.0)
            .collect();
        assert_eq!(listed.len(), LISTED);
        assert!(listed.contains(&20), "{listed:?}");
        assert_eq!(listed[0] % 2, 0, "the columns stay as they were");
        assert_eq!(*listed.last().unwrap(), listed[0] + LISTED - 1);
    }

    #[test]
    fn the_bar_moves_to_a_new_count_in_a_fifth_of_a_second() {
        use UnitState::*;
        let at = Instant::now();
        let mut card = Card::new(progress(&[Waiting, Waiting]));
        assert_eq!(card.bar(at), 0.0);
        card.set(progress(&[Done, Working]), at);
        assert_eq!(card.bar(at), 0.0);
        let half = card.bar(at + BAR_MOVE / 2);
        assert!(half > 0.25 && half < 0.5, "{half}");
        assert_eq!(card.bar(at + BAR_MOVE), 0.5);
    }
}
