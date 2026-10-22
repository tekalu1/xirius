//! Audio clips that do not follow the song's tempo keep their length in seconds when the
//! tempo changes (EDT-007): the start stays on its bar, the end moves.

use std::collections::HashMap;
use std::sync::Arc;

use crate::id::ClipId;
use crate::model::{Clip, ClipBody, Project};
use crate::tempo::TempoMap;

/// Plays its source at a fixed speed, whatever the song's tempo.
pub fn keeps_seconds(c: &Clip) -> bool {
    match &c.body {
        ClipBody::Audio { process, .. } => !(process.follow && process.tempo.is_some()),
        ClipBody::Midi { .. } => false,
    }
}

/// After an edit, moves the ends (and offsets) of the audio clips the edit did not touch, if
/// the edit changed the tempo. Clips the edit moved or changed are left as it made them.
pub fn retime(before: &Project, after: &mut Project) {
    if before.tempo == after.tempo || before.ppq != after.ppq {
        return;
    }
    let old: HashMap<ClipId, &Clip> = before
        .tracks
        .values()
        .flat_map(|t| &t.clips)
        .filter(|c| keeps_seconds(c))
        .map(|c| (c.id, c))
        .collect();
    let (from, to) = (
        TempoMap::from_project(before),
        TempoMap::from_project(after),
    );
    let untouched = |c: &Clip| old.get(&c.id).is_some_and(|o| **o == *c);
    for t in after.tracks.values_mut() {
        if !t.clips.iter().any(untouched) {
            continue;
        }
        for c in &mut Arc::make_mut(t).clips {
            if !untouched(c) {
                continue;
            }
            let base = from.seconds_at(c.start);
            let at = to.seconds_at(c.start);
            let len = from.seconds_at(c.end()) - base;
            c.length = (to.tick_at(at + len) - c.start).max(1);
            if let ClipBody::Audio { offset, .. } = &mut c.body {
                let off = from.seconds_at(c.start + *offset) - base;
                *offset = (to.tick_at(at + off) - c.start).max(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::{TempoId, TrackId};
    use crate::model::{AudioProcess, TempoPoint, Track, TrackKind};

    fn song(process: AudioProcess) -> (Project, ClipId) {
        let mut p = Project::new("t");
        let ppq = p.ppq as i64;
        let id = ClipId(p.alloc());
        let mut t = Track::new(TrackId(p.alloc()), TrackKind::Audio, "a");
        t.clips.push(Clip {
            id,
            name: "a".into(),
            color: None,
            start: ppq * 4,
            length: ppq * 4,
            muted: false,
            take: None,
            controllers: Vec::new(),
            body: ClipBody::Audio {
                file: "a.wav".into(),
                offset: ppq,
                gain_db: 0.0,
                process,
            },
        });
        p.tracks.insert(t.id, Arc::new(t));
        (p, id)
    }

    #[test]
    fn a_plain_clip_keeps_its_seconds_and_its_start() {
        let (before, id) = song(AudioProcess::default());
        let mut after = before.clone();
        after.tempo[0].bpm = 60.0;
        retime(&before, &mut after);
        let ppq = after.ppq as i64;
        let c = after.clip(id).unwrap().1;
        assert_eq!(c.start, ppq * 4);
        // Four beats at 120 are two seconds; at 60 they are two beats.
        assert_eq!(c.length, ppq * 2);
        let ClipBody::Audio { offset, .. } = c.body else {
            unreachable!()
        };
        assert_eq!(offset, ppq / 2);
    }

    #[test]
    fn a_following_clip_keeps_its_beats() {
        let (before, id) = song(AudioProcess {
            tempo: Some(120.0),
            follow: true,
            ..Default::default()
        });
        let mut after = before.clone();
        after.tempo[0].bpm = 60.0;
        retime(&before, &mut after);
        assert_eq!(after.clip(id).unwrap().1, before.clip(id).unwrap().1);
    }

    #[test]
    fn a_clip_the_edit_changed_is_left_alone() {
        let (before, id) = song(AudioProcess::default());
        let mut after = before.clone();
        let tempo = TempoId(after.alloc());
        after.tempo.push(TempoPoint {
            id: tempo,
            tick: 1,
            bpm: 90.0,
            ramp: false,
        });
        after.clip_mut(id).unwrap().1.start += 10;
        let want = after.clip(id).unwrap().1.clone();
        retime(&before, &mut after);
        assert_eq!(*after.clip(id).unwrap().1, want);
    }
}
