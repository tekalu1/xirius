//! 音源を準備中 (ENG-064): what the window shows while a song it just opened gets its
//! instruments and effects ready, so that a key pressed afterwards sounds at once.
//!
//! The mock (rollfeel § 02) is a card on a veil: a spinner, 「音源を準備中」 and how many
//! tracks are done, a bar, each track with plugins as done (a check), under way (a spinner) or
//! waiting (a dot), and one button. The song cannot be edited while it is up. 後で読む (and
//! Esc) closes it: what is left opens when it is first needed, as before (UI-285).

use std::time::{Duration, Instant};

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
pub struct Card {
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
}

impl Card {
    pub fn new(progress: Progress) -> Card {
        let to = fraction(&progress);
        Card {
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
    /// Gets the instruments and effects of the song just opened ready (ENG-064): the player
    /// starts a little at a time behind the card, and the sound device opens when it is done.
    /// A window without the device (`--silent`, the tests) starts its player when a note or a
    /// play first wants it, as before; a test calls [`Studio::prepare_song`] itself.
    pub(crate) fn prepare_on_open(&mut self) {
        self.preparing = None;
        if self.transport.mode == OutputMode::Device {
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
            Some(progress) if !progress.units.is_empty() => {
                self.preparing = Some(Card::new(progress));
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
        let p = self.project();
        match self.transport.step_start(&p, self.song.dir()) {
            Some(StartStep::Going(progress)) => {
                if let Some(card) = &mut self.preparing {
                    card.set(progress, Instant::now());
                }
            }
            Some(StartStep::Done(started)) => {
                self.preparing = None;
                if let Err(e) = started {
                    self.error = Some(e);
                }
            }
            None => self.preparing = None,
        }
    }

    /// 後で読む: the card closes and the song can be edited; what is left opens when it is
    /// first needed.
    pub fn prepare_later(&mut self) {
        self.preparing = None;
    }

    /// A press while the card is up. Every press belongs to the card.
    pub fn prepare_press(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(card) = &self.preparing else {
            return false;
        };
        if card.layout(g.size).button.contains(x, y) {
            self.prepare_later();
        }
        true
    }

    pub fn prepare_hover(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(card) = &mut self.preparing else {
            return false;
        };
        let hover = card.layout(g.size).button.contains(x, y);
        let changed = hover != card.hover;
        card.hover = hover;
        changed
    }

    /// Esc is 後で読む; every other key waits.
    pub fn prepare_key(&mut self, key: &str) -> bool {
        if self.preparing.is_none() {
            return false;
        }
        if key == "escape" {
            self.prepare_later();
        }
        true
    }
}

/// The veil over the body and the card on it.
pub fn paint(pen: &mut Pen, card: &Card, g: &Geometry, turn: f32) {
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
