//! A short line the studio says once and lets go of: how many recordings closing a song moved
//! to the recycle bin (REC-016), what a command did and how to take it back (MIX-007, MIX-030).
//!
//! It floats at the foot of the arrangement for [`SHOWN`]. Most say a thing and have nothing to
//! answer; one that has a way on carries one button at its right end (元に戻す, 詳しく), and
//! stays [`SHOWN_WITH_BUTTON`] so there is time to reach it.
//! Opening another song says it at once. Closing the window leaves the count in a file next to
//! the plugin cache, and the next window says it and removes the file.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use xirius_design::{FontSize, Radius, Space, Weight};

use crate::draw::{Align, Pen};
use crate::studio::{Geometry, Rect, Studio};

/// How long a notice stays.
pub const SHOWN: Duration = Duration::from_secs(5);
/// How long a notice with a button stays.
pub const SHOWN_WITH_BUTTON: Duration = Duration::from_secs(8);
/// Height of the line, as the line that says why a command failed.
const H: f32 = 30.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub text: String,
    pub since: Instant,
    pub button: Option<Button>,
}

/// The button at the end of a notice, and what a press on it does.
#[derive(Clone, Debug, PartialEq)]
pub struct Button {
    pub label: String,
    pub press: Press,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Press {
    /// 元に戻す: the last step.
    Undo,
    /// 詳しく: the list of what a preset applied adjusted.
    Report(crate::adjust_report::View),
    /// A command of the table, with its arguments (フリーズを解除).
    Command(String, serde_json::Value),
}

impl Notice {
    fn shown(&self) -> Duration {
        if self.button.is_some() {
            SHOWN_WITH_BUTTON
        } else {
            SHOWN
        }
    }
}

/// What a close that moved `n` recordings says; nothing when none moved.
pub fn cleaned(n: usize) -> Option<String> {
    (n > 0).then(|| format!("使われていない録音 {n} 個をごみ箱に移動しました"))
}

/// Where a closed window leaves its count for the next one.
pub fn left_path() -> PathBuf {
    xirius_plugins::default_cache_path().with_file_name("cleaned.txt")
}

/// The window closes: `n` more recordings moved, kept for the next window.
pub fn leave(path: &Path, n: usize) -> std::io::Result<()> {
    if n == 0 {
        return Ok(());
    }
    let before = read(path);
    if let Some(dir) = path.parent() {
        // design-lint: allow(ui-blocking-io) PRJ-031: worker-only helper; UI callers submit/poll and never call this boundary directly.
        std::fs::create_dir_all(dir)?;
    }
    // design-lint: allow(ui-blocking-io) PRJ-031: worker-only helper; UI callers submit/poll and never call this boundary directly.
    std::fs::write(path, (before + n).to_string())
}

/// Where a closed window keeps the song folders whose cleanup it stopped, for the next one.
pub fn later_path() -> PathBuf {
    xirius_plugins::default_cache_path().with_file_name("tidy-later.txt")
}

/// Keeps `dirs` in `path`, one a line, with those kept before.
pub fn leave_later(path: &Path, dirs: &[PathBuf]) -> std::io::Result<()> {
    if dirs.is_empty() {
        return Ok(());
    }
    let mut all = read_later(path);
    for d in dirs {
        if !all.contains(d) {
            all.push(d.clone());
        }
    }
    if let Some(dir) = path.parent() {
        // design-lint: allow(ui-blocking-io) PRJ-031: worker-only helper; UI callers submit/poll and never call this boundary directly.
        std::fs::create_dir_all(dir)?;
    }
    let text: String = all.iter().map(|d| format!("{}\n", d.display())).collect();
    // design-lint: allow(ui-blocking-io) PRJ-031: worker-only helper; UI callers submit/poll and never call this boundary directly.
    std::fs::write(path, text)
}

/// The folders kept in `path`, once: the file goes (none when it cannot).
pub fn take_later(path: &Path) -> Vec<PathBuf> {
    let all = read_later(path);
    // design-lint: allow(ui-blocking-io) PRJ-031: worker-only helper; UI callers submit/poll and never call this boundary directly.
    if path.exists() && std::fs::remove_file(path).is_err() {
        return Vec::new();
    }
    all
}

fn read_later(path: &Path) -> Vec<PathBuf> {
    // design-lint: allow(ui-blocking-io) PRJ-031: worker-only helper; UI callers submit/poll and never call this boundary directly.
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn acknowledge_later(path: &Path, adopted: &[PathBuf]) -> std::io::Result<()> {
    let remaining: Vec<_> = read_later(path)
        .into_iter()
        .filter(|p| !adopted.contains(p))
        .collect();
    if remaining.is_empty() {
        // design-lint: allow(ui-blocking-io) PRJ-031: worker-only helper; UI callers submit/poll and never call this boundary directly.
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    } else {
        let text: String = remaining
            .iter()
            .map(|p| format!("{}\n", p.display()))
            .collect();
        // design-lint: allow(ui-blocking-io) PRJ-031: worker-only helper; UI callers submit/poll and never call this boundary directly.
        std::fs::write(path, text)
    }
}

fn read(path: &Path) -> usize {
    // design-lint: allow(ui-blocking-io) PRJ-031: worker-only helper; UI callers submit/poll and never call this boundary directly.
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

impl Studio {
    /// Says `text` for [`SHOWN`], in place of what was said before.
    pub fn tell(&mut self, text: String) {
        self.notice = Some(Notice {
            text,
            since: Instant::now(),
            button: None,
        });
    }

    /// Says `text` with a button at its end, for [`SHOWN_WITH_BUTTON`].
    pub fn tell_with(&mut self, text: String, label: &str, press: Press) {
        self.notice = Some(Notice {
            text,
            since: Instant::now(),
            button: Some(Button {
                label: label.to_string(),
                press,
            }),
        });
    }

    /// What is being said, while it still is.
    pub fn notice_text(&self) -> Option<&str> {
        self.shown_notice().map(|n| n.text.as_str())
    }

    fn shown_notice(&self) -> Option<&Notice> {
        self.notice
            .as_ref()
            .filter(|n| n.since.elapsed() < n.shown())
    }

    /// The button label of what is being said, if it has one.
    pub fn notice_button(&self) -> Option<&str> {
        self.shown_notice()
            .and_then(|n| n.button.as_ref())
            .map(|b| b.label.as_str())
    }

    /// A press at (`x`, `y`) of the body: on the notice's button it does what the button
    /// says, and the notice goes. True when the press was the button's.
    pub fn notice_press(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(n) = self.shown_notice() else {
            return false;
        };
        let Some(button) = &n.button else {
            return false;
        };
        let r = place(self, g, &n.text, Some(&button.label));
        if !button_rect(r, &button.label).contains(x, y) {
            return false;
        }
        let press = button.press.clone();
        self.notice = None;
        match press {
            Press::Undo => {
                self.undo(g);
            }
            Press::Report(view) => self.open_report(g, view),
            Press::Command(command, args) => {
                self.arrange_run(&command, args);
                self.rebuild(g);
            }
        }
        true
    }

    /// Lets go of a notice said long enough. True when one went.
    pub fn expire_notice(&mut self) -> bool {
        if self.notice.is_some() && self.notice_text().is_none() {
            self.notice = None;
            return true;
        }
        false
    }

    /// Says what a closed window left at `path`, once: the file goes.
    pub fn take_left(&mut self, path: &Path) {
        if self.transport.is_windowed() {
            let path = path.to_owned();
            match crate::ancillary_io::submit(move || {
                #[cfg(any(test, feature = "loading-probe"))]
                xirius_core::io_test::before("notice_read", &path).map_err(|e| e.to_string())?;
                Ok(take_count(&path))
            }) {
                Ok(job) => self.view_read.left = Some(job),
                Err(e) => self.error = Some(e),
            }
            return;
        }
        if let Some(text) = cleaned(take_count(path)) {
            self.tell(text);
        }
    }

    pub(crate) fn read_later_notices(&mut self, path: PathBuf) {
        let source = path.clone();
        match crate::ancillary_io::submit(move || {
            #[cfg(any(test, feature = "loading-probe"))]
            xirius_core::io_test::before("notice_read", &path).map_err(|e| e.to_string())?;
            // Leave the durable retry list until the UI actually adopts it.
            Ok(read_later(&path))
        }) {
            Ok(job) => self.view_read.later = Some((source, job)),
            Err(e) => self.error = Some(e),
        }
    }

    pub(crate) fn poll_left_notices(&mut self) -> bool {
        let mut changed = false;
        if let Some(result) = self
            .view_read
            .left
            .as_ref()
            .and_then(crate::ancillary_io::Ticket::poll)
        {
            self.view_read.left = None;
            match result {
                Ok(n) => {
                    if let Some(text) = cleaned(n) {
                        self.tell(text);
                    }
                }
                Err(e) => self.error = Some(e),
            }
            changed = true;
        }
        if let Some((source, result)) = self
            .view_read
            .later
            .as_ref()
            .and_then(|(p, j)| j.poll().map(|r| (p.clone(), r)))
        {
            self.view_read.later = None;
            match result {
                Ok(dirs) => {
                    self.adopt_later(dirs.clone(), &source);
                    if let Err(e) = crate::ancillary_io::dispatch(move || {
                        acknowledge_later(&source, &dirs).map_err(|e| e.to_string())
                    }) {
                        self.error = Some(e);
                    }
                }
                Err(e) => self.error = Some(e),
            }
            changed = true;
        }
        changed
    }
}

fn take_count(path: &Path) -> usize {
    let n = read(path);
    // design-lint: allow(ui-blocking-io) PRJ-031: worker-only helper; UI callers submit/poll and never call this boundary directly.
    if path.exists() && std::fs::remove_file(path).is_err() {
        return 0;
    }
    n
}

/// The notice, centred at the foot of the arrangement; above the failed command's line when
/// that shows too.
pub fn paint(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    let Some(text) = studio.notice_text() else {
        return;
    };
    let label = studio.notice_button();
    let r = place(studio, g, text, label);
    pen.round(r, pen.c.surface_3, Radius::S);
    let Some(label) = label else {
        pen.mask(r, |pen| {
            pen.text(
                text,
                FontSize::Small,
                Weight::Medium,
                pen.c.ink_strong,
                r,
                Align::Center,
            );
        });
        return;
    };
    let b = button_rect(r, label);
    pen.round(b, pen.c.surface_2, Radius::S);
    pen.text(
        label,
        FontSize::Small,
        Weight::Medium,
        pen.c.ink_strong,
        b,
        Align::Center,
    );
    let room = Rect::new(
        r.x + Space::X4.px(),
        r.y,
        b.x - Space::X2.px() - r.x - Space::X4.px(),
        r.h,
    );
    pen.mask(room, |pen| {
        pen.text(
            text,
            FontSize::Small,
            Weight::Medium,
            pen.c.ink_strong,
            room,
            Align::Left,
        );
    });
}

/// The button inside the notice `r`, at its right end.
pub fn button_rect(r: Rect, label: &str) -> Rect {
    let w = crate::browser::text_width(label, FontSize::Small) + Space::X3.px() * 2.0;
    let inset = Space::X1.px();
    Rect::new(r.right() - inset - w, r.y + inset, w, r.h - inset * 2.0)
}

/// Where the notice goes; `button` is the label of its button when it has one.
pub fn place(studio: &Studio, g: &Geometry, text: &str, button: Option<&str>) -> Rect {
    let extra = button.map_or(0.0, |b| {
        Space::X2.px() + crate::browser::text_width(b, FontSize::Small) + Space::X3.px() * 2.0
    });
    let w = (crate::browser::text_width(text, FontSize::Small) + Space::X4.px() * 2.0 + extra)
        .min(g.lanes.w);
    let lift = if studio.error.is_some() {
        H + Space::X1.px()
    } else {
        0.0
    };
    Rect::new(
        g.lanes.x + (g.lanes.w - w) / 2.0,
        g.drawer.y - 36.0 - lift,
        w,
        H,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::FileRequest;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let p = std::env::temp_dir().join(format!(
                "xirius-notice-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Scratch(p)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A saved song with a recording nothing names, whose cleanup moves into `bin`.
    fn song_with_a_stray(song: &Path, bin: &Path) -> Studio {
        let to = bin.to_path_buf();
        let trash: xirius_session::cleanup::Trash = std::sync::Arc::new(move |p| {
            std::fs::rename(p, to.join(p.file_name().unwrap())).map_err(|e| e.to_string())
        });
        let mut s = Studio::new(crate::song::Song::sample_with_trash(trash));
        s.song.save_as(song).unwrap();
        let stray = song.join("Recordings").join("stray.wav");
        std::fs::create_dir_all(stray.parent().unwrap()).unwrap();
        std::fs::write(&stray, b"RIFF").unwrap();
        s
    }

    /// Waits for the cleanups of the songs closed, off the screen's thread (REC-026).
    fn tidied(s: &mut Studio) {
        let t = Instant::now();
        while s.is_tidying() {
            assert!(t.elapsed() < Duration::from_secs(60));
            s.tick();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn opening_another_song_says_once_how_many_recordings_moved() {
        let (song, bin, other) = (
            Scratch::new("song"),
            Scratch::new("bin"),
            Scratch::new("other"),
        );
        let mut s = song_with_a_stray(&song.0, &bin.0);
        crate::song::Song::sample().save_as(&other.0).unwrap();
        assert_eq!(s.notice_text(), None);
        s.finish_file_request_sync(FileRequest::Open, &other.0);
        assert!(s.error.is_none(), "{:?}", s.error);
        tidied(&mut s);
        assert!(s.error.is_none(), "{:?}", s.error);
        assert_eq!(
            s.notice_text(),
            Some("使われていない録音 1 個をごみ箱に移動しました")
        );
        // Gone after a while, and said no more.
        s.notice.as_mut().unwrap().since -= SHOWN;
        assert_eq!(s.notice_text(), None);
        assert!(s.expire_notice());
        assert!(s.notice.is_none());
        assert!(!s.expire_notice());
        // The idle poll lets it go too, so a still window draws once more without it.
        s.tell("x".into());
        s.notice.as_mut().unwrap().since -= SHOWN;
        assert!(s.idle().changed);
    }

    #[test]
    fn nothing_moved_says_nothing() {
        let (song, other) = (Scratch::new("quiet"), Scratch::new("quiet-other"));
        let mut s = Studio::sample();
        s.song.save_as(&song.0).unwrap();
        crate::song::Song::sample().save_as(&other.0).unwrap();
        s.finish_file_request_sync(FileRequest::Open, &other.0);
        tidied(&mut s);
        assert!(s.error.is_none(), "{:?}", s.error);
        assert_eq!(s.notice_text(), None);
        assert_eq!(cleaned(0), None);
    }

    #[test]
    fn a_closed_window_leaves_the_count_for_the_next_one_which_says_it_once() {
        let dir = Scratch::new("left");
        let path = dir.0.join("Xirius").join("cleaned.txt");
        leave(&path, 0).unwrap();
        assert!(!path.exists(), "nothing to say, nothing written");
        leave(&path, 2).unwrap();
        leave(&path, 1).unwrap();
        let mut s = Studio::sample();
        s.take_left(&path);
        assert_eq!(
            s.notice_text(),
            Some("使われていない録音 3 個をごみ箱に移動しました")
        );
        assert!(!path.exists());
        let mut next = Studio::sample();
        next.take_left(&path);
        assert_eq!(next.notice_text(), None);
    }

    #[test]
    fn the_notice_sits_at_the_foot_of_the_lanes_and_above_an_error() {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 860.0);
        let text = cleaned(3).unwrap();
        let r = place(&s, &g, &text, None);
        assert!(r.x > g.lanes.x && r.right() < g.lanes.right());
        assert!((r.x + r.w / 2.0 - (g.lanes.x + g.lanes.w / 2.0)).abs() < 0.5);
        assert!(r.bottom() <= g.drawer.y);
        s.error = Some("だめでした".into());
        let lifted = place(&s, &g, &text, None);
        assert!(lifted.bottom() <= g.drawer.y - 36.0, "{lifted:?}");
    }
}
