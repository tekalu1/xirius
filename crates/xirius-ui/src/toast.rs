//! A toast at the foot of the arrangement, for what the audio processing has to say (EDT-007,
//! EDT-015, EXP-004): a bounce, a freeze or a stretch that could not be done (with ⚠), an edit
//! a frozen track refused (with 「フリーズを解除」), and an imported sound that could follow the
//! song's tempo (with 「曲のテンポに合わせる」). A song written to or read from DAWproject
//! (EXP-006, EXP-007) shows how far it is with a turning mark and no ×, then what was done with
//! a check, and what the file could not carry behind a second button.
//!
//! It sits at the left, over the lanes, and stays until it is closed or answered; one without
//! a button or ⚠ goes after [`SHOWN`]. A new one takes the place of the one before.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use xirius_core::id::{ClipId, TrackId};
use xirius_design::{FontSize, Radius, Weight};

use crate::draw::{Align, Pen};
use crate::mixer::MixerHit;
use crate::studio::{Geometry, Rect, Studio};

/// How long a toast without a button or ⚠ stays.
pub const SHOWN: Duration = Duration::from_secs(4);
const H: f32 = 36.0;
const PAD: f32 = 6.0;
const LEFT: f32 = 10.0;
const GAP: f32 = 8.0;
const ICON: f32 = 16.0;
const BUTTON_H: f32 = 24.0;
const BUTTON_PAD: f32 = 10.0;
const CLOSE: f32 = 24.0;
/// From the edges of the lanes.
const MARGIN: f32 = 8.0;

/// What the toast's button does.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    SaveAs,
    SaveAgain,
    /// Renders again the clips whose sound could not be made.
    Retry(Vec<ClipId>),
    /// Lets the tracks go of their frozen sound.
    Unfreeze(Vec<TrackId>),
    /// Makes the clips follow the song's tempo.
    Follow(Vec<ClipId>),
    /// Opens the list of what a DAWproject file could not carry, over the toast; the toast stays.
    Skipped(crate::dawproject_ui::List),
    /// Shows the file in its folder.
    Reveal(PathBuf),
    /// Takes in what another app saved to the song (UI-289).
    Reload,
    /// Takes back what came in from another app, as Ctrl+Z (UI-289).
    UndoReload,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub text: String,
    /// Something could not be done: ⚠, and it stays.
    pub alert: bool,
    pub button: Option<(String, Answer)>,
    /// A second button, after the first.
    pub also: Option<(String, Answer)>,
    /// Done: a check in front.
    pub done: bool,
    /// Still going: a turning mark in front, no ×, and it stays.
    pub busy: bool,
    /// Something to know (UI-289): ⓘ in front, in the text's colour.
    pub info: bool,
    /// Goes after [`SHOWN`] even with a button, and has no × (UI-289).
    pub brief: bool,
    pub since: Instant,
}

impl Toast {
    /// A toast that says `text` and nothing more.
    pub fn plain(text: String) -> Toast {
        Toast {
            text,
            alert: false,
            button: None,
            also: None,
            done: false,
            busy: false,
            info: false,
            brief: false,
            since: Instant::now(),
        }
    }

    pub(crate) fn stays(&self) -> bool {
        !self.brief && (self.alert || self.busy || self.button.is_some())
    }

    /// The mark in front: ⚠, a check, ⓘ or the turning mark.
    fn marked(&self) -> bool {
        self.alert || self.done || self.busy || self.info
    }

    /// Whether it has a ×.
    fn closes(&self) -> bool {
        !self.busy && !self.brief
    }
}

/// What of the toast is under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastPart {
    Body,
    Button,
    /// The second button.
    Also,
    Close,
}

/// Where the toast's parts are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parts {
    pub rect: Rect,
    pub icon: Option<Rect>,
    pub text: Rect,
    pub button: Option<Rect>,
    pub also: Option<Rect>,
    /// None while it is still going.
    pub close: Option<Rect>,
}

/// The frozen tracks an edit was refused for says this.
pub const FROZEN: &str = "フリーズ中のトラックは編集できません";
const FROZEN_MARK: &str = "はフリーズ中のため編集できません";

impl Studio {
    /// Says `text` with ⚠ until it is closed.
    pub fn warn(&mut self, text: String, button: Option<(String, Answer)>) {
        self.toast = Some(Toast {
            alert: true,
            button,
            ..Toast::plain(text)
        });
    }

    /// Says `text` with a button that does `answer`, until it is closed.
    pub fn offer(&mut self, text: String, label: &str, answer: Answer) {
        self.toast = Some(Toast {
            button: Some((label.to_string(), answer)),
            ..Toast::plain(text)
        });
    }

    /// The toast, while it shows.
    pub fn toast_shown(&self) -> Option<&Toast> {
        self.toast
            .as_ref()
            .filter(|t| t.stays() || t.since.elapsed() < SHOWN)
    }

    /// Lets go of a toast shown long enough. True when one went.
    pub fn expire_toast(&mut self) -> bool {
        if self.toast.is_some() && self.toast_shown().is_none() {
            self.toast = None;
            return true;
        }
        false
    }

    /// A command failed with `e`: an edit a frozen track refused offers to unfreeze it; the
    /// rest shows on the line under the lanes, as before.
    pub(crate) fn fail(&mut self, e: String) {
        if e.contains(FROZEN_MARK) {
            let p = self.project();
            let tracks: Vec<TrackId> = p
                .tracks
                .values()
                .filter(|t| t.frozen.is_some() && e.contains(&format!("「{}」", t.name)))
                .map(|t| t.id)
                .collect();
            if !tracks.is_empty() {
                self.error = None;
                self.offer(FROZEN.into(), "フリーズを解除", Answer::Unfreeze(tracks));
                return;
            }
        }
        self.error = Some(e);
    }

    /// A press on the instrument or an insert of a frozen track (EXP-004): they are locked, so
    /// the press offers to unfreeze it instead. True when it was refused.
    pub(crate) fn refuse_frozen(&mut self, hit: MixerHit) -> bool {
        let t = match hit {
            MixerHit::Open(t, _)
            | MixerHit::Bypass(t, _)
            | MixerHit::AddInsert(t)
            | MixerHit::AddInstrument(t) => t,
            _ => return false,
        };
        if !self
            .project()
            .tracks
            .get(&t)
            .is_some_and(|t| t.frozen.is_some())
        {
            return false;
        }
        self.offer(FROZEN.into(), "フリーズを解除", Answer::Unfreeze(vec![t]));
        true
    }

    /// The toast's button: does what it offers and closes it.
    pub fn toast_answer(&mut self) {
        self.toast_button(None, false);
    }

    /// The toast's first button or its second (`second`): does what it offers and closes it.
    /// The list of what a file could not carry opens over the toast, which stays.
    pub(crate) fn toast_button(&mut self, g: Option<&Geometry>, second: bool) {
        let answer = self.toast.as_ref().and_then(|t| {
            if second {
                t.also.clone()
            } else {
                t.button.clone()
            }
        });
        let Some((_, answer)) = answer else {
            return;
        };
        if let Answer::Skipped(list) = answer {
            if let Some(parts) = g.and_then(|g| layout(self, g)) {
                self.open_skipped(parts.rect, list);
            }
            return;
        }
        if !matches!(answer, Answer::SaveAs | Answer::SaveAgain) {
            self.toast = None;
        }
        let ids = |xs: &[ClipId]| xs.iter().map(ToString::to_string).collect::<Vec<_>>();
        match answer {
            Answer::SaveAs => self.file_request = Some(crate::studio::FileRequest::SaveAs),
            Answer::SaveAgain => {
                self.start_save();
            }
            Answer::Retry(clips) => self.retry_processed(&clips),
            Answer::Unfreeze(tracks) => {
                let ids: Vec<String> = tracks.iter().map(ToString::to_string).collect();
                self.arrange_run("track.unfreeze", serde_json::json!({ "ids": ids }));
            }
            Answer::Follow(clips) => {
                self.arrange_run(
                    "clip.stretch",
                    serde_json::json!({ "ids": ids(&clips), "follow": "on" }),
                );
            }
            Answer::Skipped(_) => {}
            Answer::Reveal(path) => self.reveal_file(path),
            Answer::Reload => self.other_load(g),
            Answer::UndoReload => self.other_undo(g),
        }
    }

    /// A press at (`x`, `y`) on the toast. True when it took the press.
    pub fn toast_press(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(parts) = layout(self, g) else {
            return false;
        };
        if !parts.rect.contains(x, y) {
            return false;
        }
        if parts.close.is_some_and(|c| c.contains(x, y)) {
            self.other_refused();
            self.toast = None;
        } else if parts.button.is_some_and(|b| b.contains(x, y)) {
            self.toast_button(Some(g), false);
        } else if parts.also.is_some_and(|b| b.contains(x, y)) {
            self.toast_button(Some(g), true);
        }
        true
    }
}

/// Where the toast and its parts go: at the left foot of the lanes, above the failed command's
/// line when that shows too.
pub fn layout(studio: &Studio, g: &Geometry) -> Option<Parts> {
    let t = studio.toast_shown()?;
    let width = |s: &str| crate::browser::text_width(s, FontSize::Small);
    let icon_w = if t.marked() { ICON + GAP } else { 0.0 };
    let button_w = |b: &Option<(String, Answer)>| {
        b.as_ref()
            .map_or(0.0, |(label, _)| width(label) + BUTTON_PAD * 2.0 + GAP)
    };
    let (first_w, also_w) = (button_w(&t.button), button_w(&t.also));
    // Without the ×, the text ends as far from the edge as it starts.
    let end = if t.closes() { GAP + CLOSE + PAD } else { LEFT };
    let room = (g.lanes.w - MARGIN * 2.0).max(0.0);
    let fixed = LEFT + icon_w + first_w + also_w + end;
    let text_w = width(&t.text).min((room - fixed).max(0.0));
    let w = (fixed + text_w).min(room);
    let lift = if studio.error.is_some() {
        30.0 + 4.0
    } else {
        0.0
    };
    let r = Rect::new(g.lanes.x + MARGIN, g.drawer.y - MARGIN - H - lift, w, H);
    let mid = |h: f32| r.y + (H - h) / 2.0;
    let mut x = r.x + LEFT;
    let icon = t.marked().then(|| {
        let i = Rect::new(x, mid(ICON), ICON, ICON);
        x += ICON + GAP;
        i
    });
    let text = Rect::new(x, r.y, text_w, H);
    x += text_w + GAP;
    let mut place = |b: &Option<(String, Answer)>, w: f32| {
        b.as_ref().map(|_| {
            let b = Rect::new(x, mid(BUTTON_H), w - GAP, BUTTON_H);
            x = b.right() + GAP;
            b
        })
    };
    let button = place(&t.button, first_w);
    let also = place(&t.also, also_w);
    let close = t
        .closes()
        .then(|| Rect::new(r.right() - PAD - CLOSE, mid(CLOSE), CLOSE, CLOSE));
    Some(Parts {
        rect: r,
        icon,
        text,
        button,
        also,
        close,
    })
}

/// The part of the toast at (`x`, `y`).
pub fn part_at(studio: &Studio, g: &Geometry, x: f32, y: f32) -> Option<ToastPart> {
    let parts = layout(studio, g)?;
    if !parts.rect.contains(x, y) {
        return None;
    }
    let on = |r: Option<Rect>| r.is_some_and(|r| r.contains(x, y));
    Some(if on(parts.close) {
        ToastPart::Close
    } else if on(parts.button) {
        ToastPart::Button
    } else if on(parts.also) {
        ToastPart::Also
    } else {
        ToastPart::Body
    })
}

pub fn paint(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    let (Some(t), Some(parts)) = (studio.toast_shown(), layout(studio, g)) else {
        return;
    };
    let c = pen.c;
    pen.floating(parts.rect, c.surface_1, Radius::S);
    if let Some(i) = parts.icon {
        if t.busy {
            let turn = t.since.elapsed().as_secs_f32().fract();
            pen.icon_turned("spinner", i, c.ink_strong, turn);
        } else if t.done {
            pen.icon("saved", i, c.ink);
        } else if t.info {
            pen.icon("info", i, c.ink);
        } else {
            pen.icon("alert", i, c.ink_alert);
        }
    }
    pen.mask(parts.text, |pen| {
        pen.text(
            &t.text,
            FontSize::Small,
            Weight::Regular,
            c.ink,
            parts.text,
            Align::Left,
        );
    });
    let over = |part| studio.hover == crate::studio::Hit::Toast(part);
    let buttons = [
        (parts.button, &t.button, ToastPart::Button),
        (parts.also, &t.also, ToastPart::Also),
    ];
    for (b, button, part) in buttons {
        if let (Some(b), Some((label, _))) = (b, button) {
            // The scroll thumb's hover tone, as the mock's button.
            let face = if over(part) { c.grid[0] } else { c.surface_3 };
            pen.round(b, face, Radius::Xs);
            pen.text(
                label,
                FontSize::Small,
                Weight::Medium,
                c.ink_strong,
                b,
                Align::Center,
            );
        }
    }
    if let Some(close) = parts.close {
        // The toast lies on `surface_1`, the hover tone itself, so the close lights as a
        // button's face.
        if over(ToastPart::Close) {
            pen.round(close, c.surface_3, Radius::Xs);
        }
        let ic = Rect::new(
            close.x + (CLOSE - ICON) / 2.0,
            close.y + (CLOSE - ICON) / 2.0,
            ICON,
            ICON,
        );
        pen.icon("close", ic, c.ink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_toast_goes_and_one_with_a_button_or_alert_stays() {
        let mut s = Studio::sample();
        s.toast = Some(Toast {
            since: Instant::now() - SHOWN,
            ..Toast::plain("x".into())
        });
        assert!(s.toast_shown().is_none());
        assert!(s.expire_toast());
        s.warn("バウンスできません（x）".into(), None);
        s.toast.as_mut().unwrap().since -= SHOWN * 10;
        assert!(s.toast_shown().is_some());
        assert!(!s.expire_toast());
    }

    #[test]
    fn the_toast_sits_at_the_left_foot_of_the_lanes_and_its_parts_inside_it() {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 860.0);
        s.warn(
            "「drumloop」を伸縮できません（ファイルが見つかりません）".into(),
            Some(("再試行".into(), Answer::Retry(Vec::new()))),
        );
        let p = layout(&s, &g).unwrap();
        assert_eq!(p.rect.x, g.lanes.x + MARGIN);
        assert!(p.rect.bottom() <= g.drawer.y - MARGIN + 0.01);
        let icon = p.icon.unwrap();
        let button = p.button.unwrap();
        assert!(icon.right() <= p.text.x && p.text.right() <= button.x);
        let close = p.close.unwrap();
        assert!(button.right() <= close.x && close.right() <= p.rect.right());
        // Closing takes the press and the toast.
        let (x, y) = (close.x + 4.0, close.y + 4.0);
        assert!(s.toast_press(&g, x, y));
        assert!(s.toast.is_none());
        assert!(!s.toast_press(&g, x, y));
    }

    #[test]
    fn the_pointer_over_the_button_or_the_close_lights_it() {
        use crate::studio::Hit;
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 860.0);
        s.offer(
            FROZEN.into(),
            "フリーズを解除",
            Answer::Unfreeze(Vec::new()),
        );
        let p = layout(&s, &g).unwrap();
        let mid = |r: Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let (bx, by) = mid(p.button.unwrap());
        let (cx, cy) = mid(p.close.unwrap());
        let (tx, ty) = mid(p.text);
        assert_eq!(s.hit(&g, bx, by), Hit::Toast(ToastPart::Button));
        assert_eq!(s.hit(&g, cx, cy), Hit::Toast(ToastPart::Close));
        assert_eq!(s.hit(&g, tx, ty), Hit::Toast(ToastPart::Body));
        assert_ne!(s.hit(&g, p.rect.x - 4.0, ty), Hit::Toast(ToastPart::Body));
        s.toast = None;
        assert!(!matches!(s.hit(&g, bx, by), Hit::Toast(_)));

        // The second button is a part of its own; a toast still going has no close.
        s.toast = Some(Toast {
            button: Some(("A".into(), Answer::Retry(Vec::new()))),
            also: Some(("B".into(), Answer::Retry(Vec::new()))),
            busy: true,
            ..Toast::plain("x".into())
        });
        let p = layout(&s, &g).unwrap();
        let (ax, ay) = mid(p.also.unwrap());
        assert_eq!(s.hit(&g, ax, ay), Hit::Toast(ToastPart::Also));
        assert_eq!(s.center_of(&g, Hit::Toast(ToastPart::Also)), Some((ax, ay)));
        assert_eq!(s.center_of(&g, Hit::Toast(ToastPart::Close)), None);
    }
}
