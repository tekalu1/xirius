//! EDT-135: ripple editing. With `--ripple`, taking a clip or a range out closes up what comes
//! after it on the same track, and making a clip longer or shorter pushes or pulls what comes
//! after it, as Studio One's Ripple Edit does. Only clips of the edited tracks move: markers,
//! the tempo, the time signatures, the sections and the other tracks stay where they are. The
//! automation under a clip that moves goes with it unless `--automation stay`.

use std::collections::BTreeMap;

use super::*;
use crate::id::TrackId;

/// The arguments of an edit that can ripple. The session fills `ripple` from the setting
/// `edit.ripple` and `automation` from `edit.automation.follows-clips` when they are left out.
pub(super) fn args() -> [ArgDef; 2] {
    [
        ArgDef::flag(
            "ripple",
            "リップル編集: 同じトラックの後ろのクリップを詰める・押す（省略時は設定の edit.ripple）",
        ),
        ArgDef::choice(
            "automation",
            &["follow", "stay"],
            "リップル編集で動くクリップの範囲のオートメーション: follow で一緒に動かす（既定）、stay でその場に残す",
        ),
    ]
}

/// Whether the edit ripples, and whether the automation goes with the clips that move.
pub(super) fn wanted(a: &Args) -> Option<bool> {
    a.flag("ripple")
        .then(|| a.opt_str("automation") != Some("stay"))
}

/// Closes up the stretches `gone` (track, start, end) took out of their tracks: every clip of
/// the track that starts after a stretch moves earlier by as much of it as lies before its
/// start. Gives how many clips moved.
pub(super) fn close_up(
    p: &mut Project,
    gone: &[(TrackId, Tick, Tick)],
    follow: bool,
) -> CmdResult<usize> {
    let mut by_track: BTreeMap<TrackId, Vec<(Tick, Tick)>> = BTreeMap::new();
    for &(t, s, e) in gone {
        if e > s {
            by_track.entry(t).or_default().push((s, e));
        }
    }
    let mut moves = Vec::new();
    for (tid, mut spans) in by_track {
        spans.sort();
        let mut merged: Vec<(Tick, Tick)> = Vec::new();
        for (s, e) in spans {
            match merged.last_mut() {
                Some(last) if s <= last.1 => last.1 = last.1.max(e),
                _ => merged.push((s, e)),
            }
        }
        for c in &p.track(tid)?.clips {
            let removed: Tick = merged
                .iter()
                .map(|&(s, e)| (c.start.min(e) - s).max(0))
                .sum();
            if removed > 0 {
                moves.push((c.id, -removed));
            }
        }
    }
    shift(p, &moves, follow)
}

/// Moves each clip of `resized` (clip, its end before, how much longer it got) pushes or pulls
/// the clips of its track that start at or after that end by the same amount. Gives how many
/// clips moved.
pub(super) fn follow_ends(
    p: &mut Project,
    resized: &[(ClipId, Tick, Tick)],
    follow: bool,
) -> CmdResult<usize> {
    let mut by_track: BTreeMap<TrackId, Vec<(ClipId, Tick, Tick)>> = BTreeMap::new();
    for &(id, old_end, delta) in resized {
        if delta != 0 {
            let (t, _) = p.clip(id)?;
            by_track.entry(t.id).or_default().push((id, old_end, delta));
        }
    }
    let mut moves = Vec::new();
    for (tid, ends) in by_track {
        for c in &p.track(tid)?.clips {
            let by: Tick = ends
                .iter()
                .filter(|&&(id, old_end, _)| id != c.id && c.start >= old_end)
                .map(|&(_, _, d)| d)
                .sum();
            if by != 0 {
                moves.push((c.id, by));
            }
        }
    }
    shift(p, &moves, follow)
}

/// Moves each clip by its amount; with `follow`, the automation under it goes too. The clips
/// are taken from left to right, so what one brings is not picked up by the next.
fn shift(p: &mut Project, moves: &[(ClipId, Tick)], follow: bool) -> CmdResult<usize> {
    let mut placed = Vec::new();
    for &(id, by) in moves {
        let (t, c) = p.clip(id)?;
        placed.push((c.start, id, t.id, c.length, by));
    }
    placed.sort_by_key(|m| (m.0, m.1));
    if follow {
        // Pulled clips are carried from the left, pushed ones from the right.
        let pulled = placed.iter().filter(|m| m.4 < 0);
        let pushed = placed.iter().rev().filter(|m| m.4 > 0);
        for &(start, _, tid, length, by) in pulled.chain(pushed) {
            super::clip::carry_automation(p, &[(tid, start, start + length, tid)], by)?;
        }
    }
    for &(_, id, _, _, by) in &placed {
        let c = p.clip_mut(id)?.1;
        c.start = (c.start + by).max(0);
    }
    Ok(placed.len())
}

/// The words a message gets when clips were moved to close up or make room.
pub(super) fn told(moved: usize) -> String {
    if moved == 0 {
        String::new()
    } else {
        format!("（後ろのクリップ {moved} 件を動かしました）")
    }
}
