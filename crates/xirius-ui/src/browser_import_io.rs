//! Browser imports prepare file reads/copies on the bounded I/O pool. Only the
//! resulting edits enter the live session, after checking its project and folder.
use crate::{drop_audio::Placed, studio::Studio};
use serde_json::{Value, json};
use std::{collections::VecDeque, path::PathBuf, sync::Arc};
use xirius_core::{
    Args, ExtContext,
    id::TrackId,
    model::{Parent, Project},
};
use xirius_session::{import::Plan, read_jobs::ReadJobs};
struct Import {
    path: PathBuf,
    track: Option<TrackId>,
    at: i64,
    midi: bool,
    /// Where a new track goes: the folder (or the root) and the place in it (UI-381).
    into: Option<(Parent, usize)>,
    /// Goes on the track the last import put its clip on, right after that clip (Alt while
    /// dropping several files); `track`, `at` and `into` stand when that import failed.
    chain: bool,
    /// What the arrangement knows this import by while its ghost waits for it (UI-386).
    ticket: u64,
    reservation: Arc<xirius_core::in_use::Uses>,
}
struct Pending {
    id: u64,
    key: u64,
    ticket: u64,
    project: Arc<Project>,
    dir: Option<PathBuf>,
    midi: bool,
    stem: String,
    name: String,
    _reservation: Arc<xirius_core::in_use::Uses>,
}
#[derive(Default)]
pub(crate) struct ImportIo {
    jobs: ReadJobs<u64, Result<Plan, String>>,
    queue: VecDeque<Import>,
    pending: Option<Pending>,
    next: u64,
    placed: Vec<Placed>,
    pub many: Option<String>,
    session: Option<u64>,
    /// The track and the end of the clip the last import placed, for one chained after it.
    last: Option<(TrackId, i64)>,
    tickets: u64,
}
impl ImportIo {
    /// Whether the import known by `ticket` still waits: queued or being read.
    pub(crate) fn holds(&self, ticket: u64) -> bool {
        self.pending.as_ref().is_some_and(|p| p.ticket == ticket)
            || self.queue.iter().any(|i| i.ticket == ticket)
    }
    fn follow_session(&mut self, session: u64) -> bool {
        let changed = self.session.is_some_and(|old| old != session);
        if changed {
            self.jobs.retain(|_| false);
            self.pending = None;
            self.queue.clear();
            self.placed.clear();
            self.many = None;
            self.last = None;
        }
        self.session = Some(session);
        changed
    }
}
impl Studio {
    pub(crate) fn queue_browser_import(
        &mut self,
        path: PathBuf,
        track: Option<TrackId>,
        at: i64,
        midi: bool,
    ) -> Result<(), String> {
        self.queue_browser_import_with(path, track, at, midi, None, false)
            .map(|_| ())
    }

    /// Like [`Studio::queue_browser_import`], with the place of a new track and whether the
    /// clip goes right after the last import's (UI-381). Returns the import's ticket.
    pub(crate) fn queue_browser_import_with(
        &mut self,
        path: PathBuf,
        track: Option<TrackId>,
        at: i64,
        midi: bool,
        into: Option<(Parent, usize)>,
        chain: bool,
    ) -> Result<u64, String> {
        self.browser_import_io
            .follow_session(self.song.session().uses_id());
        if self.browser_import_io.queue.len() >= 64 {
            return Err("ファイルの読み込みを待っています".into());
        }
        let reservation = Arc::new(xirius_core::in_use::Uses::default());
        reservation.reserve(self.song.dir().as_deref(), &path);
        self.browser_import_io.tickets += 1;
        let ticket = self.browser_import_io.tickets;
        self.browser_import_io.queue.push_back(Import {
            path,
            track,
            at,
            midi,
            into,
            chain,
            ticket,
            reservation,
        });
        Ok(ticket)
    }
    pub(crate) fn poll_browser_import(&mut self) -> bool {
        let mut changed = self
            .browser_import_io
            .follow_session(self.song.session().uses_id());
        if let Some(p) = &self.browser_import_io.pending {
            let Some(result) = self
                .browser_import_io
                .jobs
                .take(&p.key, p.id)
                .map(|r| r.unwrap_or_else(|_| Err("素材の読み込みが止まりました".into())))
            else {
                return false;
            };
            let p = self.browser_import_io.pending.take().unwrap();
            changed = true;
            self.browser_import_io.last = None;
            if !Arc::ptr_eq(&p.project, &self.project()) || p.dir != self.song.dir() {
                self.error = Some("読み込み中に曲が変わったため、素材の配置を中止しました".into());
            } else {
                match result {
                    Err(e) => self.error = Some(format!("「{}」: {e}", p.name)),
                    Ok(plan) => {
                        let before = self.project();
                        let had: Vec<_> = before
                            .tracks
                            .values()
                            .flat_map(|t| t.clips.iter().map(|c| c.id))
                            .collect();
                        let mut ok = true;
                        for (name, args) in plan.edits {
                            // design-lint: allow(ui-blocking-io) PRJ-031 review: Song::ensure_inline refuses unreviewed Session handlers; model edits enqueue history, and reviewed status/list calls use window caches.
                            if let Err(e) = self.song.run(&name, Value::Object(args)) {
                                self.error = Some(e);
                                ok = false;
                                break;
                            }
                        }
                        if ok {
                            self.clear_error();
                            self.after_edit();
                            let project = self.project();
                            let made = project.tracks.values().find_map(|t| {
                                t.clips
                                    .iter()
                                    .find(|c| !had.contains(&c.id))
                                    .map(|c| (t.id, c.id))
                            });
                            // The ghost gives way to the clip in this frame (UI-386).
                            self.clip_drop_landed(p.ticket, made.map(|(_, c)| c));
                            if let Some((t, c)) = made {
                                self.select_clip_here(t, c);
                                let end = project.tracks.get(&t).and_then(|track| {
                                    track
                                        .clips
                                        .iter()
                                        .find(|x| x.id == c)
                                        .map(|x| x.start + x.length)
                                });
                                self.browser_import_io.last = end.map(|e| (t, e));
                            }
                            let mut data = plan.output.data;
                            data["song_bpm"] = json!(
                                before
                                    .tempo
                                    .iter()
                                    .find(|t| t.tick == 0)
                                    .map_or(120.0, |t| t.bpm)
                            );
                            let follows = made.map(|(_, c)| c).filter(|_| {
                                data["bpm"].is_null()
                                    && data["file_bpm"].is_number()
                                    && data["file_bpm"] != data["song_bpm"]
                            });
                            self.browser_import_io.placed.push(Placed {
                                line: if p.midi {
                                    format!("{} をインポートしました", p.stem)
                                } else {
                                    crate::drop_audio::said(&p.stem, &data)
                                },
                                follows,
                            });
                        }
                    }
                }
            }
        }
        if let Some(mut import) = self.browser_import_io.queue.pop_front() {
            match self.browser_import_io.last {
                Some((t, end)) if import.chain => {
                    import.track = Some(t);
                    import.at = end;
                    import.into = None;
                }
                _ => {}
            }
            let project = self.project();
            let work = project.clone();
            let dir = self.song.dir();
            let folder = dir.clone();
            let registry = Arc::new(self.song.registry().clone());
            let midi = import.midi;
            let name = import
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let stem = import
                .path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let stem = if midi { name.clone() } else { stem };
            self.browser_import_io.next += 1;
            let key = self.browser_import_io.next;
            let path = import.path.clone();
            let reservation = import.reservation.clone();
            let id = self.browser_import_io.jobs.request(key, move || {
                let _reservation = reservation;
                #[cfg(test)]
                xirius_core::io_test::before(
                    if midi {
                        "browser_midi_import"
                    } else {
                        "browser_audio_import"
                    },
                    &path,
                )
                .map_err(|e| e.to_string())?;
                let author = xirius_core::command::Author::Human;
                let cx = ExtContext {
                    project: &work,
                    dir: folder.as_deref(),
                    author: &author,
                    registry: &registry,
                    work_copy: false,
                    lane: None,
                    uses: 0,
                };
                let name = if midi { "midi.import" } else { "clip.import" };
                let mut args = json!({"file":path,"at":import.at});
                if midi {
                    args["instrument"] = json!(crate::instrument::DEFAULT);
                }
                if let Some(track) = import.track {
                    args["track"] = json!(track.to_string());
                } else if let Some((parent, index)) = import.into {
                    args["into"] = match parent {
                        Parent::Root => json!("root"),
                        Parent::Folder(f) => json!(f.to_string()),
                    };
                    let len = match parent {
                        Parent::Root => work.root.len(),
                        Parent::Folder(f) => {
                            work.folders.get(&f).map_or(index, |x| x.children.len())
                        }
                    };
                    args["index"] = json!(index.min(len));
                }
                let def = registry
                    .get(name)
                    .ok_or_else(|| "インポートのコマンドがありません".to_string())?;
                let args: Args = registry
                    .normalize(def, args.as_object().unwrap())
                    .map_err(|e| e.to_string())?;
                if midi {
                    xirius_session::midi_import::run(&cx, &args, false)
                } else {
                    xirius_session::import::run(&cx, &args, false)
                }
                .map_err(|e| e.to_string())
                .inspect(|_| {
                    #[cfg(test)]
                    let _ = xirius_core::io_test::before("browser_import_prepared", &path);
                })
            });
            if let Some(id) = id {
                self.browser_import_io.pending = Some(Pending {
                    id,
                    key,
                    ticket: import.ticket,
                    project,
                    dir,
                    midi,
                    stem,
                    name,
                    _reservation: import.reservation.clone(),
                });
            } else {
                self.browser_import_io.queue.push_front(import);
            }
        } else if !self.browser_import_io.placed.is_empty() {
            let placed = std::mem::take(&mut self.browser_import_io.placed);
            let n = placed.len();
            let many = self
                .browser_import_io
                .many
                .take()
                .unwrap_or_else(|| format!("ファイル {n} 個を置きました"));
            self.tell_placed(placed, many);
        }
        changed
    }
    /// A browser import is still being read or waits its turn.
    pub(crate) fn browser_import_busy(&self) -> bool {
        self.browser_import_io.pending.is_some() || !self.browser_import_io.queue.is_empty()
    }
    #[cfg(test)]
    pub(crate) fn settle_browser_import(&mut self) {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while self.browser_import_io.pending.is_some() || !self.browser_import_io.queue.is_empty() {
            self.poll_browser_import();
            assert!(
                std::time::Instant::now() < end,
                "browser import never returned"
            );
            std::thread::yield_now();
        }
    }
}
