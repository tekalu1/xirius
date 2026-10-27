//! The cleanup of the song that was closed (REC-016), on a thread of its own once the next song
//! is open (REC-026). On a slow drive its reads took minutes on the screen's thread, before the
//! next song could open; it needs only the closed song's folder and the song as it closed.
//!
//! What it did is said when it ends, as before ([`crate::notice::cleaned`], or the error). The
//! song is opened again at once: its cleanup is told to stop first, and puts back the file it
//! had put aside (it holds the files only to look at them in memory, so no command waits for it
//! to move one). When the window closes, the cleanups still running are waited for a while
//! ([`CLOSE_WAIT`]); those not over then are stopped, and run again at the next start.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use xirius_session::cleanup::{self, Closed};

use crate::studio::Studio;

/// How many recordings went to the recycle bin, or why none could.
pub type Cleaned = Result<usize, String>;

/// How long the window, once closed, waits for the cleanups still running.
pub const CLOSE_WAIT: Duration = Duration::from_secs(10);

/// A cleanup running on its thread.
pub struct Tidy {
    dir: PathBuf,
    /// Where it was taken from, for one a window stopped when it closed ([`Studio::tidy_later`]):
    /// it goes back there if the song is open elsewhere now.
    later: Option<PathBuf>,
    stop: Arc<AtomicBool>,
    rx: Receiver<Cleaned>,
    thread: Option<JoinHandle<()>>,
    #[cfg(test)]
    finished: Option<Cleaned>,
}

impl Tidy {
    pub fn start(closed: Closed) -> Tidy {
        let dir = closed.dir().to_path_buf();
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let halt = stop.clone();
        let thread = std::thread::Builder::new()
            .name("xirius-cleanup".into())
            .spawn(move || {
                // The recycle bin runs the shell's extensions, as on the screen's thread before.
                let _apartment = xirius_session::os::apartment();
                let got = closed
                    .run(&halt)
                    .map(|out| out.data["moved"].as_array().map_or(0, Vec::len))
                    .map_err(|e| e.to_string());
                let _ = tx.send(got);
            })
            .ok();
        Tidy {
            dir,
            later: None,
            stop,
            rx,
            thread,
            #[cfg(test)]
            finished: None,
        }
    }

    /// What it did, once it is over.
    fn ended(&mut self) -> Option<Cleaned> {
        #[cfg(test)]
        if let Some(got) = self.finished.take() {
            return Some(got);
        }
        if self.thread.as_ref().is_some_and(|t| !t.is_finished()) {
            return None;
        }
        match self.rx.try_recv() {
            Ok(got) => Some(got),
            Err(TryRecvError::Empty) => self.thread.is_none().then_some(Ok(0)),
            Err(TryRecvError::Disconnected) => Some(Ok(0)),
        }
    }

    /// What it did, once it is over or `until` has come: None then, and it is still running.
    fn ended_by(&self, until: Instant) -> Option<Cleaned> {
        if self.thread.is_none() {
            return Some(Ok(0));
        }
        match self
            .rx
            // design-lint: allow(ui-blocking-io) PRJ-031: worker-only helper; UI callers submit/poll and never call this boundary directly.
            .recv_timeout(until.saturating_duration_since(Instant::now()))
        {
            Ok(got) => Some(got),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => Some(Ok(0)),
        }
    }

    /// It moves nothing more, and ends soon.
    fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }

    /// Waits for the thread, which has said what it did or soon will.
    fn join(&mut self) {
        if let Some(t) = self.thread.take() {
            // design-lint: allow(ui-blocking-io) PRJ-031: normal poll checks is_finished before this join; shutdown waits only on its worker.
            let _ = t.join();
        }
    }
}

/// What a cleanup left because the song in `dir` is open elsewhere says.
fn left_open(dir: &Path) -> String {
    let name = dir.file_name().unwrap_or(dir.as_os_str()).to_string_lossy();
    format!("「{name}」はほかで開かれているので、録音を片付けませんでした")
}

/// Whether `a` and `b` are one folder, however they are written, without the disk (it may be
/// slow): when that cannot tell (a path not absolute), they are taken for one.
fn same_folder(a: &Path, b: &Path) -> bool {
    if !a.is_absolute() || !b.is_absolute() {
        return true;
    }
    xirius_core::in_use::key(None, a) == xirius_core::in_use::key(None, b)
}

impl Studio {
    /// Wait for the worker's result, then exercise the normal notification path. Tests must
    /// not infer completion from frames or elapsed time; the timeout only diagnoses a stall.
    #[cfg(test)]
    pub(crate) fn await_tidy_for_test(&mut self) {
        let until = Instant::now() + Duration::from_secs(60);
        assert!(!self.tidying.is_empty(), "cleanup was not started");
        for t in &mut self.tidying {
            t.finished =
                Some(t.ended_by(until).unwrap_or_else(|| {
                    panic!("waiting for cleanup worker result for {:?}", t.dir)
                }));
        }
        assert!(self.poll_tidy());
        assert!(!self.is_tidying());
    }

    /// The closed song's cleanup starts, unless the song now open is that one again: its
    /// recordings are looked at when it closes.
    pub(crate) fn start_tidy(&mut self, closed: Closed) {
        self.start_tidy_from(closed, None);
    }

    /// [`Studio::start_tidy`], for a cleanup taken from `later`.
    fn start_tidy_from(&mut self, closed: Closed, later: Option<&Path>) {
        if self
            .song
            .dir()
            .is_some_and(|now| same_folder(&now, closed.dir()))
        {
            return;
        }
        let mut t = Tidy::start(closed);
        t.later = later.map(Path::to_path_buf);
        self.tidying.push(t);
    }

    /// Once a frame and between frames: says what each cleanup that ended did. True when one
    /// ended.
    pub(crate) fn poll_tidy(&mut self) -> bool {
        let mut ended = Vec::new();
        let mut i = 0;
        while i < self.tidying.len() {
            match self.tidying[i].ended() {
                Some(got) => {
                    let mut t = self.tidying.remove(i);
                    // design-lint: allow(ui-blocking-io) PRJ-031: normal poll checks is_finished before this join; shutdown waits only on its worker.
                    t.join();
                    ended.push((t.dir, t.later, got));
                }
                None => i += 1,
            }
        }
        let any = !ended.is_empty();
        for (dir, later, got) in ended {
            match (later, got) {
                // Left for the next start again, unsaid: the song closes elsewhere later, and
                // is tidied then.
                (Some(later), Err(e)) if e.contains(cleanup::OTHERS_OPEN) => {
                    if self.transport.is_windowed() {
                        if let Err(e) = crate::ancillary_io::dispatch(move || {
                            crate::notice::leave_later(&later, &[dir]).map_err(|e| e.to_string())
                        }) {
                            self.error = Some(e);
                        }
                        continue;
                    }
                    if let Err(e) = crate::notice::leave_later(&later, &[dir]) {
                        self.error = Some(format!("{} に書けませんでした: {e}", later.display()));
                    }
                }
                (_, got) => self.say_cleaned(&dir, got),
            }
        }
        any
    }

    /// The song in `path` is about to be opened (or written): a cleanup of it still running is
    /// told to stop first, so it never moves a file the song opened again records or names. It
    /// looks at the stop each time it holds the files, before it moves one and after it put one
    /// aside (and then puts it back), so the song opens at once.
    pub(crate) fn stop_tidy(&mut self, path: &Path) {
        for t in &self.tidying {
            if same_folder(&t.dir, path) {
                t.stop();
            }
        }
    }

    /// The window closes: the open song's cleanup joins those of the songs closed before it,
    /// and the song is let go (its folder is free at once, for the next window). An error when
    /// it cannot be tidied (an AI's work copy may still need its files).
    pub fn close_window_song(&mut self) -> Result<(), String> {
        if self.open_closing.is_some() {
            return Ok(());
        }
        let closing = self.closing();
        self.release_song();
        if let Some(closed) = closing? {
            self.tidying.push(Tidy::start(closed));
        }
        Ok(())
    }

    /// Waits for every cleanup still running, until `within` has passed: how many files they
    /// moved. The errors they met are said with `say`. Those still running then are told to
    /// stop and not waited for (a file may take long to move), and their folders kept in
    /// `later` for the next start ([`Studio::tidy_later`]): a recording they had not looked at
    /// is left, not lost.
    pub fn wait_tidy(
        &mut self,
        within: Duration,
        later: &Path,
        say: &mut dyn FnMut(String),
    ) -> usize {
        wait_all(std::mem::take(&mut self.tidying), within, later, say)
    }

    /// The cleanups a window stopped are read away from the window thread.
    pub fn tidy_later(&mut self, later: &Path) {
        if self.transport.is_windowed() {
            self.read_later_notices(later.to_owned());
            return;
        }
        self.adopt_later(crate::notice::take_later(later), later);
    }

    pub(crate) fn adopt_later(&mut self, dirs: Vec<PathBuf>, later: &Path) {
        for dir in dirs {
            let closed = Closed::saved(dir, self.song.trash());
            self.start_tidy_from(closed, Some(later));
        }
    }

    /// Whether a cleanup is still running.
    pub fn is_tidying(&self) -> bool {
        !self.tidying.is_empty()
    }

    /// What a cleanup did, said the way closing a song says it (one that was stopped, nothing).
    pub(crate) fn say_cleaned(&mut self, dir: &Path, got: Cleaned) {
        match got {
            Err(e) if e.contains(cleanup::STOPPED) => {}
            Err(e) if e.contains(cleanup::OTHERS_OPEN) => self.tell(left_open(dir)),
            Err(e) => self.error = Some(e),
            Ok(n) => {
                if let Some(text) = crate::notice::cleaned(n) {
                    self.tell(text);
                }
            }
        }
    }
}

pub(crate) fn wait_all(
    tidies: Vec<Tidy>,
    within: Duration,
    later: &Path,
    say: &mut dyn FnMut(String),
) -> usize {
    let until = Instant::now() + within;
    let mut moved = 0;
    let mut left = Vec::new();
    for mut t in tidies {
        let Some(got) = t.ended_by(until) else {
            t.stop();
            left.push(t.dir.clone());
            continue;
        };
        // design-lint: allow(ui-blocking-io) PRJ-031: worker-only helper; UI callers submit/poll and never call this boundary directly.
        t.join();
        match got {
            Ok(n) => moved += n,
            Err(e) if e.contains(cleanup::STOPPED) => {}
            Err(e) => say(e),
        }
    }
    if let Err(e) = crate::notice::leave_later(later, &left) {
        say(format!("{} に書けませんでした: {e}", later.display()));
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_compare_as_written_without_the_disk_and_as_one_when_it_cannot_tell() {
        let base = std::env::temp_dir().join("xirius-no-such-folder");
        let a = base.join("Demo");
        assert!(!a.exists());
        assert!(same_folder(
            &a,
            &base.join("x").join("..").join(".").join("Demo")
        ));
        assert!(!same_folder(&a, &base.join("Other")));
        assert!(
            same_folder(&a, Path::new("Demo")),
            "a relative path cannot be told"
        );
        if cfg!(windows) {
            let upper = PathBuf::from(a.to_string_lossy().to_uppercase());
            assert!(same_folder(&a, &upper));
            let verbatim = PathBuf::from(format!(r"\\?\{}", a.display()));
            assert!(same_folder(&a, &verbatim));
        }
    }

    #[test]
    fn a_song_left_because_it_is_open_elsewhere_is_news_with_its_name_and_no_error() {
        let mut s = Studio::sample();
        let dir = std::env::temp_dir().join("Demo");
        s.say_cleaned(&dir, Err(cleanup::OTHERS_OPEN.to_string()));
        assert!(s.error.is_none(), "{:?}", s.error);
        assert_eq!(s.notice_text(), Some(left_open(&dir).as_str()));
        assert!(left_open(&dir).contains("「Demo」"));
        s.say_cleaned(&dir, Err("読み込めません".into()));
        assert_eq!(s.error.as_deref(), Some("読み込めません"));
    }

    /// The recycle bin's shell extensions need a single-threaded apartment, as the screen's
    /// thread had; a thread with none would have them made in another, or failing.
    #[cfg(windows)]
    #[test]
    fn the_cleanup_moves_from_an_apartment_of_its_own_as_the_screens_thread_did() {
        use std::sync::Mutex;
        use windows::Win32::System::Com::{
            APTTYPE, APTTYPE_MAINSTA, APTTYPE_STA, APTTYPEQUALIFIER, CoGetApartmentType,
        };
        let dir = std::env::temp_dir().join(format!("xirius-ui-tidy-sta-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        crate::song::Song::sample().save_as(&dir).unwrap();
        let recordings = dir.join("Media/Recordings");
        std::fs::create_dir_all(&recordings).unwrap();
        std::fs::write(recordings.join("stray.wav"), b"RIFF").unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let trash: cleanup::Trash = {
            let seen = seen.clone();
            Arc::new(move |p| {
                let (mut kind, mut how) = (APTTYPE::default(), APTTYPEQUALIFIER::default());
                // SAFETY: two out-pointers to locals.
                let got = unsafe { CoGetApartmentType(&mut kind, &mut how) };
                seen.lock().unwrap().push(got.map(|()| kind));
                std::fs::remove_file(p).map_err(|e| e.to_string())
            })
        };
        let mut t = Tidy::start(Closed::saved(dir.clone(), trash));
        let got = t.ended_by(Instant::now() + Duration::from_secs(60));
        t.join();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(got, Some(Ok(1)));
        // The first in the process is its main one.
        let seen = seen.lock().unwrap();
        assert!(
            matches!(seen[..], [Ok(APTTYPE_STA | APTTYPE_MAINSTA)]),
            "{seen:?}"
        );
    }
}
