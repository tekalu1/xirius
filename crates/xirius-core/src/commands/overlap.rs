//! EDT-137 / EDT-138: no overlap when editing. The session calls [`apply`] after every edit,
//! so the screen, the CLI and MCP get the same result and the trimming is in the same history
//! step as the edit.
//!
//! * Notes (`--overlap trim`, filled from the setting `edit.note.overlap`): where a note the
//!   edit moved, made longer, changed in pitch or added lies over a note of the same pitch in
//!   the same clip, the earlier one ends where the later one starts. A note wholly covered by
//!   another is removed, as Studio One's "No overlap" does. Two notes that start together keep
//!   the edited one (of two edited ones, the longer). Overlaps the edit did not touch stay.
//! * Clips (`--no-overlap`, filled from `edit.clip.no-overlap`): the clips the edit moved,
//!   resized or made lie on top; what they cover of the other clips of the same track is cut
//!   away: a clip wholly under one is removed, one under its head or tail is trimmed (an audio
//!   clip keeps playing the same part of its file), one under its middle is split in two. A
//!   pair that crossfades keeps its overlap, and so do two clips the edit both touched and
//!   (EDT-139) two takes of one stack ([`crate::takes::stacks`] before the edit); a take and a
//!   clip outside its stack are cut as any two clips. With `--ripple`, only the clips the edit
//!   names cut: the ones it pushed or pulled along keep what overlaps them.
//!
//! Recording (`record.take.add`) has neither argument, so the takes it lays down are never cut
//! and never cut anything, whatever the settings say.

use std::collections::{BTreeSet, HashMap, HashSet};

use super::*;
use crate::id::TrackId;

/// The argument of the note edits: how notes of the same pitch may overlap.
pub(super) fn note_arg() -> ArgDef {
    ArgDef::choice(
        "overlap",
        &["allow", "trim"],
        "同じピッチのノートの重なり: trim で前のノートの終わりを後のノートの頭で切り詰め、完全に覆われたノートを消す（省略時は設定の edit.note.overlap）",
    )
    .named()
}

/// The argument of the clip edits: whether the clips under the edited ones are cut away.
pub(super) fn clip_arg() -> ArgDef {
    ArgDef::flag(
        "no-overlap",
        "クリップの重なりを防ぐ: 同じトラックで下になるクリップの重なった部分を切り詰める（真ん中なら 2 つに分ける。省略時は設定の edit.clip.no-overlap）",
    )
    .named()
}

/// Trims what the edit from `before` to `after` made overlap, as its arguments ask.
pub fn apply(before: &Project, after: &mut Project, args: &Args) -> CmdResult<()> {
    if args.opt_str("overlap") == Some("trim") {
        trim_notes(before, after);
    }
    if args.flag("no-overlap") {
        trim_clips(before, after, args)?;
    }
    Ok(())
}

// ---- notes -----------------------------------------------------------------------

fn trim_notes(before: &Project, after: &mut Project) {
    let old: HashMap<ClipId, HashMap<NoteId, (Tick, Tick, u8)>> = before
        .tracks
        .values()
        .flat_map(|t| &t.clips)
        .filter(|c| c.is_midi())
        .map(|c| {
            let notes = c
                .notes()
                .iter()
                .map(|n| (n.id, (n.start, n.length, n.pitch)))
                .collect();
            (c.id, notes)
        })
        .collect();
    let mut changed: Vec<(ClipId, Vec<Note>)> = Vec::new();
    for t in after.tracks.values() {
        for c in &t.clips {
            // The notes of a clip the edit made (a pasted clip) came as they were.
            let Some(was) = old.get(&c.id) else {
                continue;
            };
            let touched: HashSet<NoteId> = c
                .notes()
                .iter()
                .filter(|n| was.get(&n.id) != Some(&(n.start, n.length, n.pitch)))
                .map(|n| n.id)
                .collect();
            if touched.is_empty() {
                continue;
            }
            let notes = resolve_notes(c.notes(), &touched);
            if notes.as_slice() != c.notes() {
                changed.push((c.id, notes));
            }
        }
    }
    for (cid, notes) in changed {
        let kept: HashSet<NoteId> = notes.iter().map(|n| n.id).collect();
        let Ok((_, clip)) = after.clip_mut(cid) else {
            continue;
        };
        let ClipBody::Midi {
            notes: old_notes, ..
        } = &mut clip.body
        else {
            continue;
        };
        let gone: Vec<NoteId> = old_notes
            .iter()
            .map(|n| n.id)
            .filter(|id| !kept.contains(id))
            .collect();
        *old_notes = Arc::new(notes);
        for id in gone {
            after.original_timing.remove(&id);
        }
    }
}

/// The notes of one clip with the overlaps of the `touched` ones taken out.
fn resolve_notes(notes: &[Note], touched: &HashSet<NoteId>) -> Vec<Note> {
    let mut out = notes.to_vec();
    let mut gone: HashSet<usize> = HashSet::new();
    let pitches: BTreeSet<u8> = out
        .iter()
        .filter(|n| touched.contains(&n.id))
        .map(|n| n.pitch)
        .collect();
    let end = |n: &Note| n.start + n.length;
    for pitch in pitches {
        let mut order: Vec<usize> = (0..out.len()).filter(|&i| out[i].pitch == pitch).collect();
        // Earlier first; at the same start the edited one first, then the longer.
        order.sort_by_key(|&i| {
            let n = &out[i];
            (
                n.start,
                !touched.contains(&n.id),
                std::cmp::Reverse(end(n)),
                n.id.0,
            )
        });
        let mut kept: Vec<usize> = Vec::new();
        for b in order {
            let tb = touched.contains(&out[b].id);
            let mut stays = true;
            for &a in &kept {
                let ta = touched.contains(&out[a].id);
                if !(ta || tb) || end(&out[a]) <= out[b].start {
                    continue;
                }
                if out[a].start == out[b].start || (end(&out[b]) <= end(&out[a]) && !(tb && !ta)) {
                    // `b` starts with `a`, which goes first, or lies wholly in it.
                    stays = false;
                    break;
                }
                // The earlier one ends where the later one starts.
                out[a].length = out[b].start - out[a].start;
            }
            if stays {
                kept.push(b);
            } else {
                gone.insert(b);
            }
        }
    }
    out.into_iter()
        .enumerate()
        .filter(|(i, _)| !gone.contains(i))
        .map(|(_, n)| n)
        .collect()
}

// ---- clips -----------------------------------------------------------------------

fn trim_clips(before: &Project, after: &mut Project, args: &Args) -> CmdResult<()> {
    let old: HashMap<ClipId, (TrackId, Tick, Tick)> = before
        .tracks
        .values()
        .flat_map(|t| {
            t.clips
                .iter()
                .map(move |c| (c.id, (t.id, c.start, c.length)))
        })
        .collect();
    let mut top: Vec<(TrackId, Tick, ClipId)> = after
        .tracks
        .values()
        .flat_map(|t| {
            t.clips
                .iter()
                .filter(|c| old.get(&c.id) != Some(&(t.id, c.start, c.length)))
                .map(move |c| (t.id, c.start, c.id))
        })
        .collect();
    if top.is_empty() {
        return Ok(());
    }
    top.sort();
    let on_top: HashSet<ClipId> = top.iter().map(|x| x.2).collect();
    // EDT-139: the take stack each clip was in before the edit, by its track and place.
    let tracks: BTreeSet<TrackId> = top.iter().map(|x| x.0).collect();
    let stack_of: HashMap<ClipId, (TrackId, usize)> = tracks
        .iter()
        .filter_map(|tid| before.tracks.get(tid))
        .flat_map(|t| {
            crate::takes::stacks(t)
                .into_iter()
                .enumerate()
                .flat_map(move |(i, s)| s.slots.into_iter().map(move |x| (x.clip, (t.id, i))))
        })
        .collect();
    let same_stack = |x: ClipId, y: ClipId| {
        stack_of
            .get(&x)
            .is_some_and(|a| stack_of.get(&y) == Some(a))
    };
    // A ripple moves the clips after the edited ones out of the way: they keep what overlaps
    // them and cut nothing.
    let edited: Option<HashSet<ClipId>> = args
        .flag("ripple")
        .then(|| args.ids::<ClipId>("ids").ok())
        .flatten()
        .filter(|ids| !ids.is_empty())
        .map(|ids| ids.into_iter().collect());
    top.retain(|x| edited.as_ref().is_none_or(|e| e.contains(&x.2)));
    for (tid, _, id) in top {
        let Some(t) = after.tracks.get(&tid) else {
            continue;
        };
        let Some(c) = t.clips.iter().find(|c| c.id == id) else {
            continue;
        };
        let (start, end) = (c.start, c.end());
        let crossed = |x: ClipId, y: ClipId| {
            after
                .fx(x)
                .and_then(|f| f.cross)
                .is_some_and(|cr| cr.with == y)
        };
        let under: Vec<(ClipId, Tick, Tick)> = t
            .clips
            .iter()
            .filter(|u| !on_top.contains(&u.id) && u.start < end && u.end() > start)
            .filter(|u| !crossed(u.id, id) && !crossed(id, u.id) && !same_stack(u.id, id))
            .map(|u| (u.id, u.start, u.end()))
            .collect();
        for (uid, u_start, u_end) in under {
            match (u_start < start, u_end > end) {
                // Wholly under it.
                (false, false) => {
                    let t = after.track_mut(tid)?;
                    t.clips.retain(|x| x.id != uid);
                    after.fx_forget(uid);
                }
                // Its tail is under the head.
                (true, false) => after.clip_mut(uid)?.1.length = start - u_start,
                // Its head is under the tail.
                (false, true) => {
                    let by = end - u_start;
                    super::clip::trim_clip(after.clip_mut(uid)?.1, uid, by)?;
                    after.fx_trimmed(uid, by);
                }
                // Its middle is under it: two pieces are left.
                (true, true) => {
                    super::clip::split_one(after, uid, end)?;
                    after.clip_mut(uid)?.1.length = start - u_start;
                }
            }
        }
    }
    Ok(())
}
