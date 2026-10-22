//! Selecting by rule: all, none, invert, the clips on the selected tracks, the clips inside the
//! loop (CTX-028, CTX-030). They change only the session's selection, never the song.
//!
//! "All" and "invert" work on notes when the selection holds notes (or `--clip` names a clip),
//! and on clips otherwise (`--track` narrows to one track).

use std::collections::HashSet;

use super::*;
use crate::id::{IdKind, TrackId};
use crate::session::SessionState;

pub(super) fn defs() -> Vec<CommandDef> {
    let scope = || {
        vec![
            ArgDef::id("clip", IdKind::Clip, "このクリップのノートを対象にします"),
            ArgDef::id(
                "track",
                IdKind::Track,
                "このトラックのクリップを対象にします",
            ),
        ]
    };
    vec![
        read(
            "select.all",
            "すべてを選択します（選択中にノートがあれば、そのクリップのノート。なければ全クリップ。--clip・--track で絞れます）",
            scope(),
            |p, s, _, a| {
                let all = universe(p, s, a)?;
                s.selection = all;
                Ok(Output::msg(format!(
                    "{} 件を選択しました",
                    s.selection.len()
                )))
            },
        ),
        read(
            "select.invert",
            "選択を反転します（選択中にノートがあれば、そのクリップのノートの中で。なければクリップの中で）",
            scope(),
            |p, s, _, a| {
                let all = universe(p, s, a)?;
                let now: HashSet<AnyId> = s.selection.iter().copied().collect();
                s.selection = all.into_iter().filter(|i| !now.contains(i)).collect();
                Ok(Output::msg(format!(
                    "{} 件を選択しました",
                    s.selection.len()
                )))
            },
        ),
        read(
            "select.on-tracks",
            "選択中のトラック（選択中のクリップがあるトラックも）の、すべてのクリップを選択します",
            vec![ArgDef::ids(
                IdKind::Any,
                "トラックかクリップ（省略時は選択中のもの）",
            )],
            |p, s, _, a| {
                let mut from = a.ids::<AnyId>("ids")?;
                if from.is_empty() {
                    from = s.selection.clone();
                }
                let mut tracks: HashSet<TrackId> = HashSet::new();
                for id in from {
                    match id {
                        AnyId::Track(t) => {
                            tracks.insert(t);
                        }
                        AnyId::Clip(c) => {
                            if let Some((t, _)) = p.find_clip(c) {
                                tracks.insert(t);
                            }
                        }
                        _ => {}
                    }
                }
                if tracks.is_empty() {
                    return Err(CmdError::invalid(
                        "トラックかクリップを選んでから実行してください",
                    ));
                }
                s.selection = p
                    .tracks
                    .iter()
                    .filter(|(t, _)| tracks.contains(t))
                    .flat_map(|(_, t)| t.clips.iter().map(|c| AnyId::Clip(c.id)))
                    .collect();
                Ok(Output::msg(format!(
                    "{} 件を選択しました",
                    s.selection.len()
                )))
            },
        ),
        read(
            "select.in-loop",
            "ループ範囲にかかっているクリップを選択します（--clip を指定すると、そのクリップのノート）",
            scope(),
            |p, s, _, a| {
                let lr = p
                    .loop_range
                    .ok_or_else(|| CmdError::conflict("ループ範囲が設定されていません"))?;
                let overlaps = |start: Tick, end: Tick| start < lr.end && end > lr.start;
                let mut out = Vec::new();
                if let Some(cid) = a.opt_id::<ClipId>("clip")? {
                    let (_, c) = p.clip(cid)?;
                    for n in c.notes() {
                        if overlaps(c.start + n.start, c.start + n.start + n.length) {
                            out.push(AnyId::Note(n.id));
                        }
                    }
                } else {
                    let only: Option<TrackId> = a.opt_id("track")?;
                    for (tid, t) in &p.tracks {
                        if only.is_some_and(|o| o != *tid) {
                            continue;
                        }
                        for c in &t.clips {
                            if overlaps(c.start, c.end()) {
                                out.push(AnyId::Clip(c.id));
                            }
                        }
                    }
                }
                s.selection = out;
                Ok(Output::msg(format!(
                    "{} 件を選択しました",
                    s.selection.len()
                )))
            },
        ),
    ]
}

/// Everything "all" means for the arguments and the current selection.
fn universe(p: &Project, s: &SessionState, a: &Args) -> CmdResult<Vec<AnyId>> {
    let mut clips: Vec<ClipId> = a.opt_id::<ClipId>("clip")?.into_iter().collect();
    if clips.is_empty() && a.opt_id::<TrackId>("track")?.is_none() {
        // Notes in the selection: the notes of the clips that hold them.
        let held: HashSet<NoteId> = s
            .selection
            .iter()
            .filter_map(|i| match i {
                AnyId::Note(n) => Some(*n),
                _ => None,
            })
            .collect();
        if !held.is_empty() {
            clips = p
                .tracks
                .values()
                .flat_map(|t| t.clips.iter())
                .filter(|c| c.notes().iter().any(|n| held.contains(&n.id)))
                .map(|c| c.id)
                .collect();
        }
    }
    let mut out = Vec::new();
    if !clips.is_empty() {
        for cid in clips {
            let (_, c) = p.clip(cid)?;
            out.extend(c.notes().iter().map(|n| AnyId::Note(n.id)));
        }
        return Ok(out);
    }
    let only: Option<TrackId> = a.opt_id("track")?;
    if let Some(t) = only {
        p.track(t)?;
    }
    for (tid, t) in &p.tracks {
        if only.is_none_or(|o| o == *tid) {
            out.extend(t.clips.iter().map(|c| AnyId::Clip(c.id)));
        }
    }
    Ok(out)
}
