//! One turn at a time, across the whole process, for the calls plugins are not safe to make
//! from several threads at once: loading a module (`LoadLibrary`, `InitDll`), the factory,
//! and creating, initialising, terminating and
//! releasing instances. kHs Gain created from 8 threads at once corrupts the heap; one at a
//! time it does not. `process` and the other calls on a live instance do not take a turn.
//!
//! The turn is reentrant, so a teardown inside a failed setup does not wait for itself. A
//! thread with windows that waits for a turn keeps answering messages sent to it, so a plugin
//! that sends to its own window on that thread while setting up elsewhere does not deadlock.
//!
//! The holder and every waiter are written down with what they are doing and since when, so
//! a turn that is not handed on says where it stopped: a setup gives up after
//! [`SETUP_LIMIT`] with that in its error, a teardown (which cannot give up) prints it every
//! [`REPORT_EVERY`], and [`turn_report`] gives it to anyone who asks.

use std::fmt::Write as _;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IsGUIThread, MSG, PM_NOREMOVE, PM_QS_SENDMESSAGE, PeekMessageW,
};

use crate::{Error, error};

/// How often a thread with windows looks at its sent messages while it waits.
const PUMP_EVERY: Duration = Duration::from_millis(2);
/// How often a thread without windows looks again on its own, even when no one woke it.
const LOOK_EVERY: Duration = Duration::from_millis(50);
/// How long a setup (loading a module, creating an instance) waits for the turn.
pub(crate) const SETUP_LIMIT: Duration = Duration::from_secs(60);
/// How often a teardown that is still waiting says so on stderr.
const REPORT_EVERY: Duration = Duration::from_secs(30);

/// A thread holding or waiting for the turn.
struct Job {
    /// By Win32 id (also valid while thread-locals are torn down, unlike
    /// `std::thread::current`).
    thread: u32,
    what: &'static str,
    bundle: PathBuf,
    since: Instant,
}

impl Job {
    fn describe(&self, now: Instant) -> String {
        format!(
            "thread {} ({} {}, {:.1} s)",
            self.thread,
            self.what,
            self.bundle.display(),
            now.duration_since(self.since).as_secs_f64()
        )
    }
}

struct State {
    /// The holder and how many turns it holds.
    holder: Option<(Job, usize)>,
    waiting: Vec<Job>,
}

static STATE: Mutex<State> = Mutex::new(State {
    holder: None,
    waiting: Vec::new(),
});
static FREED: Condvar = Condvar::new();

fn lock() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Held while the calls above run. Dropping it hands the turn on.
pub(crate) struct Turn {
    _same_thread: PhantomData<*const ()>,
}

/// Waits for the turn, however long it takes. Returns at once when this thread already holds
/// it. `what` and `bundle` say what the turn is for, in reports.
pub(crate) fn turn(what: &'static str, bundle: &Path) -> Turn {
    match wait(what, bundle, None) {
        Ok(t) => t,
        Err(_) => unreachable!("a wait without a limit does not give up"),
    }
}

/// As [`turn`], but gives up after `limit`, with an error that says who holds the turn.
pub(crate) fn turn_within(
    what: &'static str,
    bundle: &Path,
    limit: Duration,
) -> Result<Turn, Error> {
    wait(what, bundle, Some(limit))
}

fn wait(what: &'static str, bundle: &Path, limit: Option<Duration>) -> Result<Turn, Error> {
    // SAFETY: a plain query about the calling thread.
    let me = unsafe { GetCurrentThreadId() };
    let job = || Job {
        thread: me,
        what,
        bundle: bundle.to_path_buf(),
        since: Instant::now(),
    };
    let mut state = lock();
    match &mut state.holder {
        None => {
            state.holder = Some((job(), 1));
            return Ok(Turn {
                _same_thread: PhantomData,
            });
        }
        Some((holder, depth)) if holder.thread == me => {
            *depth += 1;
            return Ok(Turn {
                _same_thread: PhantomData,
            });
        }
        Some(_) => {}
    }
    let start = Instant::now();
    state.waiting.push(job());
    let mut report_at = start + REPORT_EVERY;
    let outcome = loop {
        if state.holder.is_none() {
            break Ok(());
        }
        let now = Instant::now();
        if limit.is_some_and(|l| now.duration_since(start) >= l) {
            break Err(error(format!(
                "gave up waiting for the plugin turn to {what} {}: {}",
                bundle.display(),
                report(&state, now)
            )));
        }
        if limit.is_none() && now >= report_at {
            eprintln!(
                "xirius-vst3: thread {me} is still waiting for the plugin turn to {what} {}: {}",
                bundle.display(),
                report(&state, now)
            );
            report_at += REPORT_EVERY;
        }
        // Looked at every time: a plugin may give this thread its first window while it waits.
        // SAFETY: a plain query about the calling thread.
        let gui = unsafe { IsGUIThread(0) } != 0;
        let every = if gui { PUMP_EVERY } else { LOOK_EVERY };
        state = FREED
            .wait_timeout(state, every)
            .unwrap_or_else(PoisonError::into_inner)
            .0;
        if gui {
            drop(state);
            answer_sent_messages();
            state = lock();
        }
    };
    state.waiting.retain(|j| j.thread != me);
    outcome.map(|()| {
        state.holder = Some((job(), 1));
        Turn {
            _same_thread: PhantomData,
        }
    })
}

fn report(state: &State, now: Instant) -> String {
    let mut out = match &state.holder {
        Some((h, _)) => format!("held by {}", h.describe(now)),
        None => "free".to_string(),
    };
    if !state.waiting.is_empty() {
        out.push_str("; waiting:");
        for (i, w) in state.waiting.iter().enumerate() {
            let sep = if i == 0 { " " } else { ", " };
            let _ = write!(out, "{sep}{}", w.describe(now));
        }
    }
    out
}

/// Who holds the plugin turn, for what and since when, and who waits for it. `None` when no
/// thread holds it.
pub fn turn_report() -> Option<String> {
    let state = lock();
    state.holder.as_ref()?;
    Some(report(&state, Instant::now()))
}

/// Runs the window procedures for messages other threads sent to this thread's windows,
/// leaving posted messages in the queue.
fn answer_sent_messages() {
    // SAFETY: `msg` is an out-struct owned here; PeekMessage delivers sent messages itself.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        PeekMessageW(
            &mut msg,
            std::ptr::null_mut(),
            0,
            0,
            PM_NOREMOVE | PM_QS_SENDMESSAGE,
        );
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        let mut state = lock();
        if let Some((_, depth)) = &mut state.holder {
            *depth -= 1;
            if *depth == 0 {
                state.holder = None;
                // Every waiter looks; the first to lock takes the turn.
                FREED.notify_all();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier, mpsc};

    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, HWND_MESSAGE, SMTO_NORMAL, SendMessageTimeoutW, WM_NULL,
    };

    fn here() -> &'static Path {
        Path::new("test.vst3")
    }

    #[test]
    fn one_thread_at_a_time_and_reentrant() {
        let inside = Arc::new(AtomicUsize::new(0));
        let most = Arc::new(AtomicUsize::new(0));
        let start = Arc::new(Barrier::new(8));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let (inside, most, start) = (inside.clone(), most.clone(), start.clone());
                std::thread::spawn(move || {
                    start.wait();
                    for _ in 0..200 {
                        let _a = turn("load", here());
                        let _b = turn("load", here());
                        let n = inside.fetch_add(1, Ordering::SeqCst) + 1;
                        most.fetch_max(n, Ordering::SeqCst);
                        std::thread::yield_now();
                        inside.fetch_sub(1, Ordering::SeqCst);
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(most.load(Ordering::SeqCst), 1);
    }

    /// The holder sends to a window of a thread that waits for the turn. Without answering
    /// sent messages while waiting, both would wait for each other.
    #[test]
    fn a_waiting_thread_with_a_window_answers_messages_sent_to_it() {
        let held = turn("load", here());
        let (tx, rx) = mpsc::channel();
        let waiter = std::thread::spawn(move || {
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            // SAFETY: a message-only window of a system class: never shown, never focused.
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    std::ptr::null(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    HWND_MESSAGE,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                )
            };
            assert!(!hwnd.is_null());
            tx.send(hwnd as usize).unwrap();
            let _t = turn("load", here());
            // SAFETY: the window was made on this thread.
            unsafe { DestroyWindow(hwnd) };
        });
        let hwnd = rx.recv().unwrap() as windows_sys::Win32::Foundation::HWND;
        // Give the other thread time to start waiting.
        std::thread::sleep(Duration::from_millis(50));
        let mut result = 0usize;
        // SAFETY: the window lives until the other thread gets the turn, which is after this.
        let answered =
            unsafe { SendMessageTimeoutW(hwnd, WM_NULL, 0, 0, SMTO_NORMAL, 5000, &mut result) };
        assert_ne!(answered, 0, "the waiting thread did not answer");
        drop(held);
        waiter.join().unwrap();
    }

    /// A setup that cannot get the turn gives up and says who holds it and who else waits.
    #[test]
    fn a_setup_gives_up_and_names_the_holder() {
        let (held_tx, held_rx) = mpsc::channel();
        let (end_tx, end_rx) = mpsc::channel::<()>();
        let holder = std::thread::spawn(move || {
            let _t = turn("unload", Path::new("stuck.vst3"));
            held_tx.send(()).unwrap();
            end_rx.recv().unwrap();
        });
        held_rx.recv().unwrap();
        let waiter = std::thread::spawn(|| {
            let _t = turn("release an instance of", Path::new("other.vst3"));
        });
        while !turn_report().is_some_and(|r| r.contains("other.vst3")) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let start = Instant::now();
        let err = match turn_within("load", Path::new("new.vst3"), Duration::from_millis(200)) {
            Ok(_) => panic!("got the turn while another thread holds it"),
            Err(e) => e.to_string(),
        };
        assert!(start.elapsed() >= Duration::from_millis(200));
        assert!(err.contains("load new.vst3"), "{err}");
        assert!(err.contains("held by thread"), "{err}");
        assert!(err.contains("unload stuck.vst3"), "{err}");
        assert!(err.contains("release an instance of other.vst3"), "{err}");
        end_tx.send(()).unwrap();
        holder.join().unwrap();
        waiter.join().unwrap();
        // The waiter that gave up is no longer listed, and the turn is handed on.
        let _t = turn_within("load", here(), Duration::from_secs(5)).expect("the turn");
        let now = turn_report().expect("held here");
        assert!(!now.contains("new.vst3"), "{now}");
    }
}
