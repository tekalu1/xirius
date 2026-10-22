//! Bounces and freezes run on a thread of their own (EDT-015, EXP-003, EXP-004): the clips (or
//! the track) they read show how far it is, the square stops it, and the rest of the song stays
//! in reach. What they wrote goes into the song as one step of the history when they are done;
//! a failure says why in a toast, and a stop says nothing.
//!
//! The commands are the session's own (`clip.bounce`, `range.bounce`, `track.bounce`,
//! `track.freeze`), so the CLI and the AI run the same work in the foreground.
//!
//! One runs at a time, in the order they were asked for (ENG-033): one asked for while another
//! runs waits in a queue, its clips saying so. Ctrl+Z takes back the newest one not yet in the
//! song when nothing was done after it: a waiting one leaves the queue, the running one stops.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use serde_json::Value;
use xirius_core::id::{ClipId, TrackId};
use xirius_core::{Args, CmdResult};
use xirius_session::bounce::Planned;
use xirius_session::export::Progress;
use xirius_session::import::Plan;

use crate::studio::Studio;

pub use xirius_session::bounce::NAMES;

/// A bounce or a freeze being rendered.
pub struct Job {
    /// The command it answers.
    pub name: String,
    /// Its id for `job.cancel` (EXP-047).
    pub id: String,
    pub progress: Arc<Progress>,
    rx: Receiver<CmdResult<Plan>>,
    /// The clips it reads, which show how far it is.
    pub clips: Vec<ClipId>,
    /// The tracks being frozen, whose headers say so.
    pub freezing: Vec<TrackId>,
    /// The tracks it makes, shown below where they go while it runs (EXP-047).
    pub planned: Vec<Planned>,
    /// The clips whose sound goes to a new track: how far it is shows there, not on them.
    pub moved: Vec<ClipId>,
    wait: crate::wait::Id,
    /// Where the history was when it was asked for (ENG-033).
    mark: u64,
}

/// A bounce or a freeze waiting for the ones before it (ENG-033).
pub struct Queued {
    pub name: String,
    args: Value,
    /// The clips it will read.
    pub clips: Vec<ClipId>,
    /// Where the history was when it was asked for: a Ctrl+Z from there takes it back.
    mark: u64,
}

/// The bounces and freezes waiting, oldest first.
pub type Queue = VecDeque<Queued>;

impl Job {
    /// 0..=1.
    pub fn fraction(&self) -> f32 {
        self.progress.fraction() as f32
    }
}

/// What a failed job says.
pub fn failed(name: &str, why: &str) -> String {
    let what = if name == "track.freeze" {
        "フリーズできません"
    } else {
        "バウンスできません"
    };
    format!("{what}（{why}）")
}

impl Studio {
    /// Starts the bounce or freeze `name` on a thread, or puts it in the queue while another
    /// runs. What makes it fail at once (nothing to bounce, a frozen track) shows as any failed
    /// command does. False when it did not start nor wait.
    pub(crate) fn job_start(&mut self, name: &str, args: Value) -> bool {
        let mark = self.song.session().cursor();
        if self.job.is_some() {
            let Some((_, clips, _)) = self.job_check(name, args.clone()) else {
                return false;
            };
            self.job_queue.push_back(Queued {
                name: name.to_string(),
                args,
                clips,
                mark,
            });
            self.error = None;
            return true;
        }
        let Some((args, clips, freezing)) = self.job_check(name, args) else {
            return false;
        };
        self.job_launch(name, args, clips, freezing, mark);
        true
    }

    /// The arguments of `name` checked against the song as it is now, with the clips it reads
    /// and the tracks it freezes; None (and the error shown) when it cannot run.
    fn job_check(&mut self, name: &str, args: Value) -> Option<(Args, Vec<ClipId>, Vec<TrackId>)> {
        let map = match args {
            Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        let registry = self.song.registry();
        let args = match registry.get(name).map(|def| registry.normalize(def, &map)) {
            Some(Ok(args)) => args,
            Some(Err(e)) => {
                self.fail(e.to_string());
                return None;
            }
            None => Args(map),
        };
        let project = self.project();
        let dir = self.song.dir();
        // Checked first, so what cannot be done is said before anything is written.
        let checked = xirius_session::bounce::prepare(
            &project,
            dir.as_deref(),
            None,
            name,
            &args,
            true,
            None,
        )
        .map(|_| ())
        .and_then(|()| {
            let (clips, freezing) = reads(&project, name, &args);
            // A bounce in place changes the clips it reads; a frozen track keeps its own.
            if matches!(name, "clip.bounce" | "range.bounce") {
                for t in project.tracks.values() {
                    if t.frozen.is_some() && t.clips.iter().any(|c| clips.contains(&c.id)) {
                        return Err(xirius_core::CmdError::conflict(format!(
                            "「{}」はフリーズ中のため編集できません",
                            t.name
                        )));
                    }
                }
            }
            Ok((clips, freezing))
        });
        match checked {
            Ok((clips, freezing)) => Some((args, clips, freezing)),
            Err(e) => {
                self.fail(e.to_string());
                None
            }
        }
    }

    fn job_launch(
        &mut self,
        name: &str,
        args: Args,
        clips: Vec<ClipId>,
        freezing: Vec<TrackId>,
        mark: u64,
    ) {
        let project = self.project();
        let dir = self.song.dir();
        let planned = xirius_session::bounce::planned(&project, name, &args).unwrap_or_default();
        let moved = moved(&project, name, &args, &clips);
        let catalog = self.plugin_catalog();
        let progress = Arc::new(Progress::default());
        let (tx, rx) = std::sync::mpsc::channel();
        let work = progress.clone();
        let command = name.to_string();
        let ticket = xirius_core::running::start(name, dir.as_deref());
        let id = ticket.id().to_string();
        std::thread::spawn(move || {
            let done = xirius_session::bounce::prepare(
                &project,
                dir.as_deref(),
                Some(catalog),
                &command,
                &args,
                false,
                Some((&work, ticket)),
            );
            let _ = tx.send(done);
        });
        let label = self.song.friendly(name);
        self.job = Some(Job {
            name: name.to_string(),
            id,
            progress,
            rx,
            clips,
            freezing,
            planned,
            moved,
            wait: crate::wait::begin(crate::wait::Kind::Line, &label),
            mark,
        });
        self.error = None;
        // The tracks it makes take their place below.
        self.reshape();
    }

    /// Starts the oldest waiting job, checked again against the song as it is now; one that
    /// cannot run any more says why and the next one starts.
    fn job_next(&mut self) {
        while self.job.is_none() {
            let Some(q) = self.job_queue.pop_front() else {
                return;
            };
            if let Some((args, clips, freezing)) = self.job_check(&q.name, q.args) {
                self.job_launch(&q.name, args, clips, freezing, q.mark);
            }
        }
    }

    /// Whether Ctrl+Z takes back a job: the newest one asked for, when nothing was done after.
    pub fn job_undoable(&self) -> bool {
        let at = self.song.session().cursor();
        match self.job_queue.back() {
            Some(q) => q.mark >= at,
            None => self
                .job
                .as_ref()
                .is_some_and(|j| j.mark >= at && !j.progress.cancelled()),
        }
    }

    /// Ctrl+Z: the newest job asked for leaves the queue, or stops when it is the one running,
    /// when nothing was done after it. False when Ctrl+Z goes to the history.
    pub(crate) fn job_undo(&mut self) -> bool {
        if !self.job_undoable() {
            return false;
        }
        if self.job_queue.pop_back().is_none() {
            self.job_cancel();
        }
        true
    }

    /// Is the clip `id` read by a job waiting in the queue?
    pub fn job_queued_on_clip(&self, id: ClipId) -> bool {
        self.job_queue.iter().any(|q| q.clips.contains(&id))
    }

    /// Stops the job at its next block; what it wrote goes.
    pub fn job_cancel(&mut self) -> bool {
        match &self.job {
            Some(j) => {
                j.progress.cancel();
                true
            }
            None => false,
        }
    }

    /// Once a frame and between frames: puts a finished job's sound into the song. True while
    /// a job runs (its progress moves) and once when it ends.
    pub fn poll_job(&mut self) -> bool {
        let Some(job) = &self.job else {
            return false;
        };
        let done = match job.rx.try_recv() {
            Ok(done) => done,
            Err(TryRecvError::Empty) => return true,
            Err(TryRecvError::Disconnected) => Err(xirius_core::CmdError::io("処理が止まりました")),
        };
        let job = self.job.take().expect("checked above");
        crate::wait::end(job.wait);
        // The places of the tracks it would have made close.
        self.reshape();
        if !job.progress.cancelled() {
            self.job_apply(&job, done);
        }
        self.job_next();
        true
    }

    /// Puts what `job` wrote into the song. The jobs waiting since before it went in still come
    /// after it in the history, so Ctrl+Z still takes them back first.
    fn job_apply(&mut self, job: &Job, done: CmdResult<Plan>) {
        let before = self.song.session().cursor();
        match done {
            Ok(plan) => {
                let mut broke = None;
                for (name, edit) in plan.edits {
                    if let Err(e) = self.song.run(&name, Value::Object(edit)) {
                        broke = Some(e);
                        break;
                    }
                }
                self.after_edit();
                match broke {
                    Some(e) => self.fail(e),
                    None => {
                        self.error = None;
                        self.arrange.touched();
                    }
                }
            }
            Err(e) => self.warn(failed(&job.name, &e.message), None),
        }
        let after = self.song.session().cursor();
        for q in &mut self.job_queue {
            if q.mark >= before {
                q.mark = after;
            }
        }
    }

    /// Waits for the job to end (tests and scripts).
    pub fn job_wait(&mut self) {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while self.job.is_some() && std::time::Instant::now() < until {
            self.poll_job();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// Renders again the sound of `clips` that could not be made (EDT-007).
    pub(crate) fn retry_processed(&mut self, clips: &[ClipId]) {
        let Some(rate) = self.transport.host().map(|h| h.rate()) else {
            return;
        };
        let p = self.project();
        let tmap = xirius_session::tempo::TempoMap::from_project(&p);
        let dir = self.song.dir();
        for id in clips {
            if let Ok((_, c)) = p.clip(*id)
                && let Some(key) =
                    xirius_session::processed::clip_key(c, &tmap, rate, dir.as_deref())
            {
                xirius_session::processed::retry(&key);
            }
        }
    }

    /// `range.bounce` of the range the range tool chose, on the tracks of the clips in it:
    /// mixed into one new track, or one for each track (`per_track`).
    pub(crate) fn range_bounce_args(&self, per_track: bool) -> Option<Value> {
        let (from, to) = self.arrange.range?;
        let mut tracks: Vec<String> = Vec::new();
        for (t, _) in &self.arrange.clips {
            let t = t.to_string();
            if !tracks.contains(&t) {
                tracks.push(t);
            }
        }
        if tracks.is_empty() {
            return None;
        }
        let mut args = serde_json::json!({ "from": from, "to": to, "tracks": tracks });
        if per_track {
            args["per-track"] = Value::Bool(true);
        }
        Some(args)
    }

    /// What the key of `clip.bounce` bounces when it is not the selected clips: the range
    /// the range tool chose, or the selected track when no clip is selected.
    pub(crate) fn bounce_instead(&self, new_track: bool) -> Option<(&'static str, Value)> {
        if let Some(args) = self.range_bounce_args(new_track) {
            return Some(("range.bounce", args));
        }
        match self.selected_row {
            Some(crate::tree::RowKind::Track(t)) if self.arrange.clips.is_empty() => Some((
                "track.bounce",
                serde_json::json!({ "ids": [t.to_string()] }),
            )),
            _ => None,
        }
    }

    /// The track has a snowflake: an audio or instrument track.
    pub fn freezable(&self, t: TrackId) -> bool {
        use xirius_core::model::TrackKind;
        self.project()
            .tracks
            .get(&t)
            .is_some_and(|x| matches!(x.kind, TrackKind::Audio | TrackKind::Instrument))
    }

    /// How far the job is for the clip `id`, while one reads it. A clip whose sound goes to a
    /// new track shows it there, unless the place of that track is out of sight.
    pub fn job_on_clip(&self, id: ClipId) -> Option<&Job> {
        self.job
            .as_ref()
            .filter(|j| j.clips.contains(&id) && !(j.moved.contains(&id) && self.job_rows_shown(j)))
    }

    /// The places of the tracks the job makes are in the tree.
    fn job_rows_shown(&self, job: &Job) -> bool {
        !job.planned.is_empty()
            && job.planned.iter().all(|t| {
                self.tree()
                    .row_of(crate::tree::RowKind::Track(t.after))
                    .is_some_and(|r| r.job_h > 0.0)
            })
    }

    /// How far the freeze of `track` is, while it runs.
    pub fn job_freezing(&self, track: TrackId) -> Option<&Job> {
        self.job.as_ref().filter(|j| j.freezing.contains(&track))
    }
}

/// The clips of `reads` whose sound `name` puts on a new track.
fn moved(
    p: &xirius_core::model::Project,
    name: &str,
    args: &Args,
    reads: &[ClipId],
) -> Vec<ClipId> {
    match name {
        "clip.bounce" if !args.flag("new-track") => reads
            .iter()
            .copied()
            .filter(|id| {
                p.clip(*id).is_ok_and(|(_, c)| {
                    !matches!(c.body, xirius_core::model::ClipBody::Audio { .. })
                })
            })
            .collect(),
        "clip.bounce" | "range.bounce" | "track.bounce" => reads.to_vec(),
        _ => Vec::new(),
    }
}

/// The clips `name` reads, and the tracks it freezes.
fn reads(p: &xirius_core::model::Project, name: &str, args: &Args) -> (Vec<ClipId>, Vec<TrackId>) {
    let key = if name == "range.bounce" {
        "tracks"
    } else {
        "ids"
    };
    let tracks: Vec<TrackId> = args.ids(key).unwrap_or_default();
    let of_tracks = |from: i64, to: i64| -> Vec<ClipId> {
        tracks
            .iter()
            .filter_map(|t| p.tracks.get(t))
            .flat_map(|t| t.clips.iter())
            .filter(|c| c.start < to && c.start + c.length > from)
            .map(|c| c.id)
            .collect()
    };
    match name {
        "clip.bounce" => (args.ids("ids").unwrap_or_default(), Vec::new()),
        "range.bounce" => {
            let at = |k: &str| args.0.get(k).and_then(Value::as_i64).unwrap_or(0);
            (of_tracks(at("from"), at("to")), Vec::new())
        }
        "track.freeze" => (of_tracks(i64::MIN, i64::MAX), tracks),
        _ => (of_tracks(i64::MIN, i64::MAX), Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use xirius_core::id::TrackId;

    use crate::clip_marks::{Status, status};
    use crate::studio::{Geometry, Studio};
    use crate::tools::Mods;

    fn track(s: &Studio, name: &str) -> TrackId {
        s.project()
            .tracks
            .values()
            .find(|t| t.name == name)
            .map(|t| t.id)
            .unwrap_or_else(|| panic!("no track {name}"))
    }

    /// A bounce of `bars` bars of the track `name` into a new track.
    fn bounce(s: &mut Studio, name: &str, bars: i64) -> bool {
        let t = track(s, name).to_string();
        let to = i64::from(s.project().ppq) * 4 * bars;
        s.job_start(
            "range.bounce",
            json!({ "from": 0, "to": to, "tracks": [t], "tail": 0 }),
        )
    }

    /// Ctrl+Z, as the window takes it.
    fn ctrl_z(s: &mut Studio, g: &Geometry) -> bool {
        s.edit_key(g, "z", Mods::ctrl())
    }

    /// Long enough to be still running when the test presses Ctrl+Z.
    const LONG: i64 = 400;

    fn queued_on(s: &Studio, name: &str) -> bool {
        let p = s.project();
        p.tracks[&track(s, name)]
            .clips
            .iter()
            .any(|c| matches!(status(s, c), Some(Status::Queued)))
    }

    fn setup() -> (Studio, Geometry) {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 1000.0);
        s.rebuild(&g);
        (s, g)
    }

    #[test]
    fn ctrl_z_while_a_slow_bounce_runs_takes_it_out_of_the_queue() {
        let (mut s, g) = setup();
        let (steps, tracks) = (s.song.steps(), s.project().tracks.len());
        assert!(bounce(&mut s, "Vocal", LONG), "{:?}", s.error);
        // Asked for while the first renders: it waits, and its clips say so.
        assert!(bounce(&mut s, "Bass", LONG), "{:?}", s.error);
        assert_eq!(s.job_queue.len(), 1);
        assert!(queued_on(&s, "Bass"));
        assert!(!queued_on(&s, "Vocal"));
        assert!(s.key_enabled(&g, "history.undo", crate::palette::Scope::Global));

        // Ctrl+Z takes back the newest one: the waiting bounce leaves the queue.
        assert!(ctrl_z(&mut s, &g));
        assert!(s.job_queue.is_empty());
        assert!(!queued_on(&s, "Bass"));
        let job = s.job.as_ref().expect("the first still runs");
        assert!(!job.progress.cancelled());

        // Again: the running one stops, and nothing went into the song.
        assert!(ctrl_z(&mut s, &g));
        s.job_wait();
        assert!(s.job.is_none());
        assert_eq!(s.song.steps(), steps);
        assert_eq!(s.project().tracks.len(), tracks);
        assert!(s.toast_shown().is_none(), "taking back is not a failure");
        assert!(!s.job_undoable());
    }

    #[test]
    fn jobs_asked_for_together_go_into_the_song_in_order() {
        let (mut s, g) = setup();
        let ids = |s: &Studio| -> Vec<TrackId> { s.project().tracks.keys().copied().collect() };
        let at_first = ids(&s);
        assert!(bounce(&mut s, "Vocal", 2), "{:?}", s.error);
        assert!(bounce(&mut s, "Bass", 2), "{:?}", s.error);
        // The Vocal's bounce goes in first, and only then does the Bass's start.
        let until = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !s.job_queue.is_empty() && std::time::Instant::now() < until {
            assert_eq!(s.project().tracks.len(), at_first.len());
            s.poll_job();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let first: Vec<TrackId> = ids(&s)
            .into_iter()
            .filter(|t| !at_first.contains(t))
            .collect();
        assert_eq!(first.len(), 1, "{:?}", s.toast_shown());
        s.job_wait();
        assert!(s.job.is_none() && s.job_queue.is_empty());
        assert_eq!(s.project().tracks.len(), at_first.len() + 2);

        // Ctrl+Z now goes to the history: the newest step, the Bass's bounce, is taken back.
        assert!(!s.job_undoable());
        assert!(ctrl_z(&mut s, &g));
        let left: Vec<TrackId> = ids(&s)
            .into_iter()
            .filter(|t| !at_first.contains(t))
            .collect();
        assert_eq!(left, first);
    }

    #[test]
    fn an_edit_made_after_it_is_taken_back_before_the_waiting_job() {
        let (mut s, g) = setup();
        let vocal = track(&s, "Vocal").to_string();
        let tracks = s.project().tracks.len();
        assert!(bounce(&mut s, "Bass", LONG), "{:?}", s.error);
        assert!(s.run_command("auto.mode", json!({ "ids": [vocal], "mode": "touch" })));
        let mode = |s: &Studio| s.project().tracks[&track(s, "Vocal")].auto_mode;
        let touched = mode(&s);

        // The edit is the newest: Ctrl+Z takes it back and the bounce goes on.
        assert!(ctrl_z(&mut s, &g));
        assert_ne!(mode(&s), touched);
        assert!(s.job.as_ref().is_some_and(|j| !j.progress.cancelled()));
        // Then the bounce.
        assert!(ctrl_z(&mut s, &g));
        s.job_wait();
        assert_eq!(s.project().tracks.len(), tracks);
    }

    #[test]
    fn a_job_waiting_since_before_one_went_into_the_song_is_still_taken_back_first() {
        let (mut s, g) = setup();
        let tracks = s.project().tracks.len();
        assert!(bounce(&mut s, "Vocal", 2), "{:?}", s.error);
        assert!(bounce(&mut s, "Bass", LONG), "{:?}", s.error);
        // The short one goes in, and the long one starts.
        let until = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !s.job_queue.is_empty() && std::time::Instant::now() < until {
            s.poll_job();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(s.job_queue.is_empty());
        assert_eq!(s.project().tracks.len(), tracks + 1);
        // Ctrl+Z stops the long one, which was asked for last, and leaves the short one's track.
        assert!(ctrl_z(&mut s, &g));
        s.job_wait();
        assert_eq!(s.project().tracks.len(), tracks + 1);
    }
}
