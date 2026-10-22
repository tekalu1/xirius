//! What a clip shows of its processing (EDT-007, EDT-015, EDT-016, EXP-004): marks at the right
//! of its name (frozen, following the song's tempo, speed, transposition, reversed), and at its
//! lower left how far the sound it plays is (a ring and the percent while it is stretched or
//! bounced, ⚠ when it could not be stretched). Marks that do not fit go from the right; the
//! name ends before them.
//!
//! A bounce shows a square at the clip's lower right that stops it. A stretch that fails also
//! says so once in a toast, with 「再試行」.

use std::collections::HashSet;

use gpui::{Hsla, PathBuilder, point, px};
use xirius_core::id::ClipId;
use xirius_core::model::{Clip, ClipBody, Track};
use xirius_design::{FontSize, Radius, Weight};
use xirius_session::processed::State;

use crate::draw::{Align, Pen};
use crate::studio::{Rect, Studio};
use crate::toast::Answer;

const MARK_H: f32 = 13.0;
const MARK_TOP: f32 = 3.0;
const MARK_ICON: f32 = 12.0;
const ICON_MARK_W: f32 = 15.0;
const MARK_PAD: f32 = 4.0;
const MARK_GAP: f32 = 3.0;
/// From the clip's right edge.
const MARK_RIGHT: f32 = 5.0;
/// The name keeps at least this much before the marks.
const NAME_KEEPS: f32 = 40.0;
const CHIP_H: f32 = 17.0;
const STOP_W: f32 = 20.0;
pub(crate) const STOP_SQUARE: f32 = 8.0;
const CHIP_INSET: f32 = 3.0;
const RING: f32 = 5.0;
/// The chip's room before its label, and after it.
const CHIP_LEAD: f32 = 18.0;
const CHIP_TAIL: f32 = 8.0;
/// How far the ring's arc goes round, and how fast it turns.
const ARC: f32 = 1.4 * std::f32::consts::PI;
const TURN_MS: f32 = 140.0;
/// What a failed stretch says on the clip.
pub const CANNOT: &str = "伸縮できません";
/// What a clip says while the bounce or freeze that reads it waits for the ones before it.
pub const QUEUED: &str = "待機中";

/// A mark at the right of a clip's name.
#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    Icon(&'static str),
    Text(String),
}

/// How far a clip's sound is, when it is not ready to play.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Status {
    /// Being rendered: what renders it (none for a stretch) and the percent; a bounce can be
    /// stopped from the clip.
    Working {
        label: Option<&'static str>,
        percent: u32,
        stop: bool,
    },
    /// Waiting for the bounces and freezes before it (ENG-033).
    Queued,
    Failed,
}

/// What the clip's processing settings show, in order from the left.
pub fn marks(track: &Track, clip: &Clip) -> Vec<Mark> {
    let mut out = Vec::new();
    if track.frozen.is_some() {
        out.push(Mark::Icon("freeze"));
    }
    if let ClipBody::Midi { transpose, .. } = &clip.body
        && *transpose != 0
    {
        out.push(Mark::Text(signed(i32::from(*transpose))));
    }
    if let ClipBody::Audio { process: p, .. } = &clip.body {
        if p.follow {
            out.push(Mark::Icon("stretch"));
        }
        if (p.speed - 100.0).abs() > 1e-6 {
            out.push(Mark::Text(format!("{}%", p.speed.round())));
        }
        if p.semis != 0 {
            out.push(Mark::Text(signed(p.semis)));
        }
        if p.cents != 0 {
            out.push(Mark::Text(format!("{} ct", signed(p.cents))));
        }
        if p.reverse {
            out.push(Mark::Icon("reverse"));
        }
    }
    out
}

fn signed(v: i32) -> String {
    match v {
        0 => "0".to_string(),
        v if v > 0 => format!("+{v}"),
        v => format!("−{}", -v),
    }
}

/// The clip's state: a bounce or a freeze reading it, else its stretched sound.
pub fn status(studio: &Studio, clip: &Clip) -> Option<Status> {
    if let Some(job) = studio.job_on_clip(clip.id) {
        let bounce = job.name != "track.freeze";
        return Some(Status::Working {
            label: bounce.then_some("バウンス中"),
            percent: percent(job.fraction()),
            stop: bounce,
        });
    }
    if studio.job_queued_on_clip(clip.id) {
        return Some(Status::Queued);
    }
    match studio.transport.host()?.clip_state(clip)? {
        State::Ready(_) => None,
        State::Working(f) => Some(Status::Working {
            label: None,
            percent: percent(f),
            stop: false,
        }),
        State::Failed(_) => Some(Status::Failed),
    }
}

fn percent(f: f32) -> u32 {
    (f.clamp(0.0, 1.0) * 100.0).floor() as u32
}

/// How much the waveform is dimmed while the clip's sound is not ready.
pub fn dim(status: Option<Status>) -> f32 {
    match status {
        Some(Status::Working { .. }) => 0.45,
        Some(Status::Failed) => 0.3,
        Some(Status::Queued) | None => 1.0,
    }
}

/// Draws the marks that fit at the right of the head of the clip at `r`, whose name is
/// `name_w` wide; returns the x the name ends before.
pub fn paint_marks(pen: &mut Pen, marks: &[Mark], r: Rect, name_w: f32) -> f32 {
    let c = pen.c;
    let widths: Vec<f32> = marks
        .iter()
        .map(|m| match m {
            Mark::Icon(_) => ICON_MARK_W,
            Mark::Text(t) => pen.measure(t, FontSize::Micro, Weight::Semibold) + MARK_PAD * 2.0,
        })
        .collect();
    let room = r.w - 12.0 - name_w.min(NAME_KEEPS);
    let (mut keep, mut used) = (0, 0.0);
    for w in &widths {
        if used + w + MARK_GAP > room {
            break;
        }
        keep += 1;
        used += w + MARK_GAP;
    }
    let mut x = r.right() - MARK_RIGHT;
    for (m, w) in marks[..keep].iter().zip(&widths).rev() {
        x -= w;
        let at = Rect::new(x, r.y + MARK_TOP, *w, MARK_H);
        pen.round(at, c.surface_3, Radius::Xs);
        match m {
            Mark::Icon(name) => {
                let color = if *name == "freeze" { c.ring } else { c.ink };
                let i = Rect::new(
                    x + (w - MARK_ICON) / 2.0,
                    at.y + (MARK_H - MARK_ICON) / 2.0,
                    MARK_ICON,
                    MARK_ICON,
                );
                pen.icon(name, i, color);
            }
            Mark::Text(t) => {
                pen.text(
                    t,
                    FontSize::Micro,
                    Weight::Semibold,
                    c.ink,
                    at,
                    Align::Center,
                );
            }
        }
        x -= MARK_GAP;
    }
    x
}

/// Where the state's chip goes in the clip at `r`, for a label `label_w` wide.
pub fn chip_rect(r: Rect, label_w: f32) -> Rect {
    Rect::new(
        r.x + CHIP_INSET,
        r.bottom() - CHIP_H - 2.0,
        (label_w + CHIP_LEAD + CHIP_TAIL).min((r.w - CHIP_INSET * 2.0).max(0.0)),
        CHIP_H,
    )
}

/// The square that stops a bounce, at the lower right of the clip at `r`; none when the clip
/// is too narrow for it beside the chip.
pub fn stop_rect(r: Rect) -> Option<Rect> {
    let stop = Rect::new(
        r.right() - CHIP_INSET - STOP_W,
        r.bottom() - CHIP_H - 2.0,
        STOP_W,
        CHIP_H,
    );
    (stop.x >= r.x + CHIP_INSET + CHIP_LEAD + 24.0 + CHIP_TAIL).then_some(stop)
}

/// The label of a working state: with what renders it while that fits the clip.
fn working_label(pen: &mut Pen, label: Option<&str>, percent: u32, r: Rect) -> String {
    let short = format!("{percent}%");
    match label {
        Some(l) => {
            let long = format!("{l} {short}");
            let w = pen.measure(&long, FontSize::Caption, Weight::Semibold);
            if w + CHIP_LEAD + CHIP_TAIL > r.w - 34.0 {
                short
            } else {
                long
            }
        }
        None => short,
    }
}

/// Draws the state at the lower left of the clip at `r`.
pub fn paint_status(pen: &mut Pen, status: Status, r: Rect) {
    let c = pen.c;
    match status {
        Status::Working {
            label,
            percent,
            stop,
        } => {
            let text = working_label(pen, label, percent, r);
            let w = pen.measure(&text, FontSize::Caption, Weight::Semibold);
            let chip = chip_rect(r, w);
            pen.round(chip, c.surface_1, Radius::Xs);
            let centre = (chip.x + 8.0, chip.y + CHIP_H / 2.0);
            let turn = since_start_ms() / TURN_MS;
            ring(pen, centre, 0.0, std::f32::consts::TAU, c.surface_3);
            ring(pen, centre, turn, turn + ARC, c.ring);
            let label = Rect::new(chip.x + CHIP_LEAD, chip.y, w, CHIP_H);
            pen.text(
                &text,
                FontSize::Caption,
                Weight::Semibold,
                c.ink,
                label,
                Align::Left,
            );
            if let Some(at) = stop_rect(r).filter(|at| stop && at.x > chip.right()) {
                pen.round(at, c.surface_3, Radius::Xs);
                let s = STOP_SQUARE;
                let square = Rect::new(at.x + (at.w - s) / 2.0, at.y + (at.h - s) / 2.0, s, s);
                pen.round(square, c.ink, Radius::Xs);
            }
        }
        Status::Queued => {
            let w = pen.measure(QUEUED, FontSize::Caption, Weight::Semibold);
            let chip = chip_rect(r, w);
            pen.round(chip, c.surface_1, Radius::Xs);
            let centre = (chip.x + 8.0, chip.y + CHIP_H / 2.0);
            ring(pen, centre, 0.0, std::f32::consts::TAU, c.surface_3);
            let label = Rect::new(chip.x + CHIP_LEAD, chip.y, w, CHIP_H);
            pen.text(
                QUEUED,
                FontSize::Caption,
                Weight::Semibold,
                c.ink,
                label,
                Align::Left,
            );
        }
        Status::Failed => {
            let w = pen.measure(CANNOT, FontSize::Caption, Weight::Semibold);
            let chip = chip_rect(r, w);
            pen.round(chip, c.surface_1, Radius::Xs);
            let icon = Rect::new(chip.x + 3.0, chip.y + 2.0, 13.0, 13.0);
            pen.icon("alert", icon, c.ink_alert);
            let label = Rect::new(chip.x + CHIP_LEAD + 1.0, chip.y, w, CHIP_H);
            pen.text(
                CANNOT,
                FontSize::Caption,
                Weight::Semibold,
                c.ink_alert,
                label,
                Align::Left,
            );
        }
    }
}

/// Milliseconds since the first ring was drawn, which turns the rings.
fn since_start_ms() -> f32 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs_f32()
        * 1000.0
}

/// An arc of the progress ring, from angle `a` to `b` (radians, clockwise from the right).
fn ring(pen: &mut Pen, (cx, cy): (f32, f32), a: f32, b: f32, color: Hsla) {
    let (ox, oy) = pen.origin;
    let steps = ((b - a).abs() / 0.2).ceil().max(2.0) as usize;
    let at = |t: f32| point(px(ox + cx + RING * t.cos()), px(oy + cy + RING * t.sin()));
    let mut path = PathBuilder::stroke(px(2.0));
    path.move_to(at(a));
    for i in 1..=steps {
        path.line_to(at(a + (b - a) * i as f32 / steps as f32));
    }
    if let Ok(path) = path.build() {
        pen.window.paint_path(path, color);
    }
}

impl Studio {
    /// A press on the square of a clip a bounce reads, or of one it is making on a new track,
    /// stops the bounce. True when it did.
    pub fn job_stop_press(&mut self, g: &crate::studio::Geometry, x: f32, y: f32) -> bool {
        let Some(job) = &self.job else {
            return false;
        };
        if job.name == "track.freeze" || !g.lanes.contains(x, y) {
            return false;
        }
        if self.job_row_stop_press(g, x, y) {
            return true;
        }
        let p = self.project();
        let tree = self.tree();
        let on = p.tracks.values().any(|t| {
            let Some(row) = tree.row_of(crate::tree::RowKind::Track(t.id)) else {
                return false;
            };
            t.clips
                .iter()
                .filter(|c| self.job_on_clip(c.id).is_some())
                .any(|c| stop_rect(self.clip_rect(g, row, c)).is_some_and(|r| r.contains(x, y)))
        });
        on && self.job_cancel()
    }

    /// Between frames: says once in a toast which clip could not be stretched. True while a
    /// clip's sound is being made, so its ring turns.
    pub fn poll_processed(&mut self) -> bool {
        let Some(host) = self.transport.host() else {
            return false;
        };
        let p = self.project();
        let mut working = false;
        let mut failed: Vec<(ClipId, String, String)> = Vec::new();
        let mut now_failed = HashSet::new();
        for t in p.tracks.values() {
            for c in &t.clips {
                match host.clip_state(c) {
                    Some(State::Working(_)) => working = true,
                    Some(State::Failed(why)) => {
                        now_failed.insert(c.id);
                        if !self.stretch_told.contains(&c.id) {
                            failed.push((c.id, c.name.clone(), self.reason(c, &why)));
                        }
                    }
                    _ => {}
                }
            }
        }
        self.stretch_told = now_failed;
        if let Some((_, name, why)) = failed.first() {
            let ids: Vec<ClipId> = failed.iter().map(|(id, ..)| *id).collect();
            self.stretch_told.extend(ids.iter().copied());
            self.warn(
                format!("「{name}」を{CANNOT}（{why}）"),
                Some(("再試行".into(), Answer::Retry(ids))),
            );
            return true;
        }
        working
    }

    /// Why the clip's sound could not be made, said shortly: the decoder's words carry the
    /// whole path, which the toast has no room for.
    fn reason(&self, clip: &Clip, why: &str) -> String {
        let ClipBody::Audio { file, .. } = &clip.body else {
            return why.to_string();
        };
        let path = std::path::Path::new(file);
        let path = match self.song().dir() {
            Some(dir) if !path.is_absolute() => dir.join(path),
            _ => path.to_path_buf(),
        };
        if !path.exists() {
            "ファイルが見つかりません".into()
        } else if why.contains("を読めません") {
            "ファイルを読めません".into()
        } else {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
            why.replace(&path.display().to_string(), &name.unwrap_or_default())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xirius_core::model::AudioProcess;

    #[test]
    fn the_marks_follow_the_settings_in_order() {
        let s = Studio::sample();
        let p = s.project();
        let (track, clip) = p
            .tracks
            .values()
            .find_map(|t| {
                t.clips
                    .iter()
                    .find(|c| matches!(c.body, ClipBody::Audio { .. }))
                    .map(|c| ((**t).clone(), c.clone()))
            })
            .expect("the sample has an audio clip");
        assert_eq!(marks(&track, &clip), Vec::new());
        let mut clip = clip;
        let mut track = track;
        if let ClipBody::Audio { process, .. } = &mut clip.body {
            *process = AudioProcess {
                follow: true,
                speed: 150.0,
                semis: 3,
                cents: -5,
                reverse: true,
                ..AudioProcess::default()
            };
        }
        track.frozen = Some(xirius_core::model::Frozen {
            file: "f.wav".into(),
            start: 0,
        });
        assert_eq!(
            marks(&track, &clip),
            vec![
                Mark::Icon("freeze"),
                Mark::Icon("stretch"),
                Mark::Text("150%".into()),
                Mark::Text("+3".into()),
                Mark::Text("−5 ct".into()),
                Mark::Icon("reverse"),
            ]
        );
    }

    #[test]
    fn a_midi_clip_shows_its_transpose_at_the_right_of_its_name() {
        let s = Studio::sample();
        let p = s.project();
        let (track, clip) = p
            .tracks
            .values()
            .find_map(|t| {
                t.clips
                    .iter()
                    .find(|c| c.is_midi())
                    .map(|c| ((**t).clone(), c.clone()))
            })
            .expect("the sample has a MIDI clip");
        assert_eq!(marks(&track, &clip), Vec::new());
        let mut clip = clip;
        assert!(clip.set_transpose(2));
        assert_eq!(marks(&track, &clip), vec![Mark::Text("+2".into())]);
        clip.set_transpose(-12);
        assert_eq!(marks(&track, &clip), vec![Mark::Text("−12".into())]);
        clip.set_transpose(0);
        assert_eq!(marks(&track, &clip), Vec::new());
    }

    #[test]
    fn a_job_reading_a_clip_shows_how_far_it_is() {
        let s = Studio::sample();
        let p = s.project();
        let clip = p.tracks.values().flat_map(|t| &t.clips).next().unwrap();
        assert_eq!(status(&s, clip), None);
        assert_eq!(dim(None), 1.0);
        let working = Status::Working {
            label: None,
            percent: 0,
            stop: false,
        };
        assert!(dim(Some(Status::Failed)) < dim(Some(working)));
        assert_eq!(percent(0.468), 46);
        assert_eq!(percent(2.0), 100);
    }

    #[test]
    fn the_toast_says_why_without_the_files_path() {
        let s = Studio::sample();
        let dir = std::env::temp_dir().join(format!("xirius-reason-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("loop.wav");
        std::fs::write(&file, b"not a sound").unwrap();
        let mut clip = s
            .project()
            .tracks
            .values()
            .flat_map(|t| &t.clips)
            .find(|c| matches!(c.body, ClipBody::Audio { .. }))
            .unwrap()
            .clone();
        let ClipBody::Audio { file: f, .. } = &mut clip.body else {
            unreachable!()
        };
        *f = file.to_string_lossy().into_owned();
        let read = format!("{} を読めません: end of stream", file.display());
        assert_eq!(s.reason(&clip, &read), "ファイルを読めません");
        let stretch = format!("{} を伸縮できません", file.display());
        assert_eq!(s.reason(&clip, &stretch), "loop.wav を伸縮できません");
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(s.reason(&clip, &read), "ファイルが見つかりません");
    }

    #[test]
    fn the_chip_sits_at_the_lower_left_inside_the_clip() {
        let r = Rect::new(100.0, 50.0, 200.0, 60.0);
        let chip = chip_rect(r, 40.0);
        assert_eq!(chip.x, r.x + CHIP_INSET);
        assert!(chip.bottom() < r.bottom() && chip.y > r.y + 16.0);
        // A narrow clip keeps the chip inside.
        let narrow = Rect::new(0.0, 0.0, 30.0, 60.0);
        assert!(chip_rect(narrow, 80.0).right() <= narrow.right());
        // The bounce's square sits at the lower right, past the chip, when there is room.
        let stop = stop_rect(r).unwrap();
        assert!(stop.right() <= r.right() && stop.x > chip.right());
        assert_eq!(stop_rect(narrow), None);
    }
}
