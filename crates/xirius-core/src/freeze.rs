//! What a frozen track keeps (EXP-004). Its sound is a file rendered from its clips, instrument
//! and inserts, so an edit that would change any of those is turned away until the track is
//! unfrozen. The fader, the pan, the sends, the name and the color still change.

use crate::auto::AutoTarget;
use crate::error::{CmdError, CmdResult};
use crate::model::{Project, Track};

/// Turns away an edit that changed what a track frozen before and after it is rendered from.
/// Freezing and unfreezing themselves change `frozen`, so they pass.
pub fn check(before: &Project, after: &Project) -> CmdResult<()> {
    for (id, old) in &before.tracks {
        let Some(old_frozen) = &old.frozen else {
            continue;
        };
        let Some(new) = after.tracks.get(id) else {
            continue;
        };
        if new.frozen.as_ref() != Some(old_frozen) {
            continue;
        }
        if !same_source(before, old, after, new) {
            return Err(CmdError::conflict(format!(
                "「{}」はフリーズ中のため編集できません",
                new.name
            )));
        }
    }
    Ok(())
}

fn same_source(before: &Project, old: &Track, after: &Project, new: &Track) -> bool {
    let params = |t: &Track| {
        t.automation
            .iter()
            .filter(|l| matches!(l.target, AutoTarget::Param(..)))
            .cloned()
            .collect::<Vec<_>>()
    };
    let fx = |p: &Project, t: &Track| {
        t.clips
            .iter()
            .map(|c| p.clip_fx.get(&c.id).cloned())
            .collect::<Vec<_>>()
    };
    old.clips == new.clips
        && old.inserts == new.inserts
        && old.kind == new.kind
        && old.midi_out == new.midi_out
        && params(old) == params(new)
        && fx(before, old) == fx(after, new)
}
