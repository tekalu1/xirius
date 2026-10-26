//! The adoption boundary for a blocked song preparation (PRJ-030).
//!
//! This state belongs to the UI. Workers only return generation-tagged events. In
//! particular, requesting cancellation does not remove the block: retirement must
//! acknowledge it first. A retry retains successful tracks and invalidates every
//! response from the preceding attempt.

use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Generation(u64);

fn next_generation() -> Option<Generation> {
    let mut value = NEXT_GENERATION.load(Ordering::Relaxed);
    loop {
        let next = value.checked_add(1)?;
        match NEXT_GENERATION.compare_exchange_weak(
            value,
            next,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return Some(Generation(value)),
            Err(current) => value = current,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrackState {
    Waiting,
    Working,
    Ready,
    Failed(String),
    Silent(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    pub name: String,
    pub state: TrackState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Preparing,
    Finishing,
    Failed,
    Cancelling,
    Closed,
    Ready,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Working(usize),
    Track(usize, Result<(), String>),
    /// All shared work, graph construction and output adoption have finished.
    Finished(Result<(), String>),
    /// Output and all reclaimable preparation resources have been retired.
    Retired,
}

#[derive(Clone, Debug)]
pub struct Preparation {
    generation: Generation,
    pub tracks: Vec<Track>,
    pub phase: Phase,
    pub global_failure: Option<String>,
}

impl Preparation {
    pub fn new(names: impl IntoIterator<Item = String>) -> Self {
        Self {
            generation: next_generation().expect("preparation generation exhausted"),
            tracks: names
                .into_iter()
                .map(|name| Track {
                    name,
                    state: TrackState::Waiting,
                })
                .collect(),
            phase: Phase::Preparing,
            global_failure: None,
        }
    }

    pub fn generation(&self) -> Generation {
        self.generation
    }

    pub fn blocked(&self) -> bool {
        !matches!(self.phase, Phase::Ready | Phase::Closed)
    }

    pub fn done(&self) -> usize {
        self.tracks
            .iter()
            .filter(|t| t.state == TrackState::Ready)
            .count()
    }

    pub fn current(&self) -> Option<&str> {
        self.tracks
            .iter()
            .find(|t| t.state == TrackState::Working)
            .map(|t| t.name.as_str())
    }

    /// The UI acknowledges immediately. Even an already queued Finished event is
    /// ignored from here on. The worker still owes the current generation Retired.
    pub fn cancel(&mut self) -> bool {
        if !self.blocked() || self.phase == Phase::Cancelling {
            return false;
        }
        self.phase = Phase::Cancelling;
        true
    }

    pub fn accept(&mut self, generation: Generation, event: Event) -> bool {
        if generation != self.generation || !self.blocked() {
            return false;
        }
        if self.phase == Phase::Cancelling {
            if event == Event::Retired {
                self.phase = Phase::Closed;
                return true;
            }
            return false;
        }
        if !matches!(self.phase, Phase::Preparing | Phase::Finishing) {
            return false;
        }
        match event {
            Event::Working(i) => {
                let Some(track) = self.tracks.get_mut(i) else {
                    return false;
                };
                if track.state != TrackState::Waiting {
                    return false;
                }
                track.state = TrackState::Working;
            }
            Event::Track(i, result) => {
                let Some(track) = self.tracks.get_mut(i) else {
                    return false;
                };
                if !matches!(track.state, TrackState::Waiting | TrackState::Working) {
                    return false;
                }
                track.state = match result {
                    Ok(()) => TrackState::Ready,
                    Err(reason) => TrackState::Failed(reason),
                };
                if self
                    .tracks
                    .iter()
                    .all(|t| !matches!(t.state, TrackState::Waiting | TrackState::Working))
                {
                    self.phase = Phase::Finishing;
                }
            }
            Event::Finished(result) => {
                // A success cannot skip tracks still in flight. A graph/device
                // failure can arrive before any track has started.
                if result.is_ok()
                    && self
                        .tracks
                        .iter()
                        .any(|t| matches!(t.state, TrackState::Waiting | TrackState::Working))
                {
                    return false;
                }
                self.global_failure = result.err();
                self.phase = if self.global_failure.is_some()
                    || self
                        .tracks
                        .iter()
                        .any(|t| matches!(t.state, TrackState::Failed(_)))
                {
                    Phase::Failed
                } else {
                    Phase::Ready
                };
            }
            Event::Retired => return false,
        }
        true
    }

    pub fn can_continue(&self) -> bool {
        self.phase == Phase::Failed
            && self.global_failure.is_none()
            && self
                .tracks
                .iter()
                .any(|t| matches!(t.state, TrackState::Failed(_)))
    }

    /// The prepared graph must already omit failed tracks, including their sends.
    /// This is explicit permission to adopt that graph, never an effects bypass.
    pub fn continue_silently(&mut self) -> bool {
        if !self.can_continue() {
            return false;
        }
        for t in &mut self.tracks {
            if let TrackState::Failed(reason) = &t.state {
                t.state = TrackState::Silent(reason.clone());
            }
        }
        self.phase = Phase::Ready;
        true
    }

    pub fn retry(&mut self) -> Option<Generation> {
        if self.phase != Phase::Failed {
            return None;
        }
        self.generation = next_generation()?;
        for t in &mut self.tracks {
            if matches!(t.state, TrackState::Failed(_)) {
                t.state = TrackState::Waiting;
            }
        }
        self.global_failure = None;
        self.phase = Phase::Preparing;
        Some(self.generation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_song_never_accepts_the_previous_songs_generation() {
        let old = Preparation::new(["same track".into()]);
        let mut new = Preparation::new(["same track".into()]);
        assert_ne!(old.generation(), new.generation());
        assert!(!new.accept(old.generation(), Event::Track(0, Ok(()))));
        assert_eq!(new.done(), 0);
    }

    #[test]
    fn cancellation_wins_over_queued_completion_and_keeps_the_block_until_retired() {
        let mut p = Preparation::new(["track".into()]);
        let g = p.generation();
        assert!(p.accept(g, Event::Working(0)));
        assert!(p.cancel());
        assert!(!p.cancel());
        assert!(!p.accept(g, Event::Track(0, Ok(()))));
        assert!(!p.accept(g, Event::Finished(Ok(()))));
        assert_eq!(p.current(), Some("track"));
        assert!(p.blocked());
        assert!(p.accept(g, Event::Retired));
        assert_eq!(p.phase, Phase::Closed);
        assert!(!p.blocked());
    }

    #[test]
    fn retry_retains_success_and_rejects_old_results_including_retirement() {
        let mut p = Preparation::new(["a".into(), "b".into()]);
        let g = p.generation();
        p.accept(g, Event::Track(0, Ok(())));
        p.accept(g, Event::Track(1, Err("decode".into())));
        assert!(p.blocked());
        p.accept(g, Event::Finished(Ok(())));
        let new = p.retry().unwrap();
        assert_ne!(g, new);
        assert_eq!(p.done(), 1);
        assert!(!p.accept(g, Event::Track(1, Ok(()))));
        assert!(!p.accept(g, Event::Retired));
        assert!(!p.accept(new, Event::Finished(Ok(()))));
        p.accept(new, Event::Track(1, Ok(())));
        p.accept(new, Event::Finished(Ok(())));
        assert_eq!(p.phase, Phase::Ready);
        assert!(!p.cancel());
    }

    #[test]
    fn all_tracks_can_fail_but_shared_failure_never_allows_continuation() {
        let mut p = Preparation::new(["a".into(), "b".into()]);
        let g = p.generation();
        for i in 0..2 {
            p.accept(g, Event::Track(i, Err("unreadable".into())));
        }
        p.accept(g, Event::Finished(Err("master".into())));
        assert!(!p.can_continue());
        assert!(!p.continue_silently());
        let g = p.retry().unwrap();
        for i in 0..2 {
            p.accept(g, Event::Track(i, Err("unreadable".into())));
        }
        p.accept(g, Event::Finished(Ok(())));
        assert_eq!(p.done(), 0);
        assert!(p.continue_silently());
        assert!(
            p.tracks
                .iter()
                .all(|t| matches!(t.state, TrackState::Silent(_)))
        );
    }

    #[test]
    fn zero_tracks_still_waits_for_graph_and_output_and_names_remain_complete() {
        let mut empty = Preparation::new([]);
        assert!(empty.blocked());
        empty.accept(empty.generation(), Event::Finished(Ok(())));
        assert!(!empty.blocked());
        let name = "長いトラック名🎹".repeat(1024);
        let mut p = Preparation::new([name.clone()]);
        p.accept(p.generation(), Event::Working(0));
        assert_eq!(p.current(), Some(name.as_str()));
        assert!(!p.accept(p.generation(), Event::Track(1, Ok(()))));
    }
}
