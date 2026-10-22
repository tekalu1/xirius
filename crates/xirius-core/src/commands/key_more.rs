//! Commands the rest of the Studio Pro key table asks for (KEY-012): clearing every solo or
//! mute, taking the clips of tracks out of the selection, selecting up to the song's start or
//! end, and playing the next or the previous take.
use std::collections::{HashMap, HashSet};

use super::*;
use crate::id::{IdKind, TrackId};
use crate::model::Track;

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        edit(
            "track.solo.clear",
            "すべてのトラックのソロを解除します",
            vec![],
            |p, _, _| clear(p, "ソロ", |t| t.solo, |t| t.solo = false),
        ),
        edit(
            "track.mute.clear",
            "すべてのトラックのミュートを解除します",
            vec![],
            |p, _, _| clear(p, "ミュート", |t| t.mute, |t| t.mute = false),
        ),
        read(
            "select.off-tracks",
            "トラックのクリップを選択から外します（ほかの選択は残ります）",
            vec![ArgDef::ids(
                IdKind::Track,
                "トラック（省略時は選択中のトラック）",
            )],
            |p, s, _, a| {
                let mut tracks: HashSet<TrackId> = a.ids::<TrackId>("ids")?.into_iter().collect();
                if tracks.is_empty() {
                    tracks = s
                        .selection
                        .iter()
                        .filter_map(|i| match i {
                            AnyId::Track(t) => Some(*t),
                            _ => None,
                        })
                        .collect();
                }
                if tracks.is_empty() {
                    return Err(CmdError::invalid("トラックを選んでから実行してください"));
                }
                for t in &tracks {
                    p.track(*t)?;
                }
                let before = s.selection.len();
                s.selection.retain(|i| match i {
                    AnyId::Clip(c) => p.find_clip(*c).is_none_or(|(t, _)| !tracks.contains(&t)),
                    _ => true,
                });
                Ok(Output::msg(format!(
                    "{} 件を選択から外しました",
                    before - s.selection.len()
                )))
            },
        ),
        read(
            "select.span",
            "選択中のクリップから曲の先頭（--way start）か最後（--way end）まで、同じトラックのクリップを選択します",
            vec![ArgDef::choice("way", &["start", "end"], "広げる向き").req()],
            |p, s, _, a| {
                let to_start = a.str("way")? == "start";
                // Per track, the latest start (toward the start) or the earliest (toward the end).
                let mut edge: HashMap<TrackId, Tick> = HashMap::new();
                for i in &s.selection {
                    let AnyId::Clip(c) = i else { continue };
                    let Ok((t, clip)) = p.clip(*c) else { continue };
                    let (t, start) = (t.id, clip.start);
                    edge.entry(t)
                        .and_modify(|e| {
                            *e = if to_start {
                                (*e).max(start)
                            } else {
                                (*e).min(start)
                            }
                        })
                        .or_insert(start);
                }
                if edge.is_empty() {
                    return Err(CmdError::invalid("クリップを選んでから実行してください"));
                }
                let mut out: Vec<AnyId> = s
                    .selection
                    .iter()
                    .filter(|i| !matches!(i, AnyId::Clip(_)))
                    .copied()
                    .collect();
                for (tid, t) in &p.tracks {
                    let Some(e) = edge.get(tid) else { continue };
                    out.extend(
                        t.clips
                            .iter()
                            .filter(|c| {
                                if to_start {
                                    c.start <= *e
                                } else {
                                    c.start >= *e
                                }
                            })
                            .map(|c| AnyId::Clip(c.id)),
                    );
                }
                s.selection = out;
                Ok(Output::msg(format!(
                    "{} 件を選択しました",
                    s.selection.len()
                )))
            },
        ),
        edit(
            "take.next",
            "重なっているテイクのうち、次のテイクを再生します（最後の次は最初）",
            vec![ArgDef::id("id", IdKind::Clip, "テイク（重なっているもののいずれか）").req()],
            |p, _, a| cycle(p, a, true),
        ),
        edit(
            "take.prev",
            "重なっているテイクのうち、前のテイクを再生します（最初の前は最後）",
            vec![ArgDef::id("id", IdKind::Clip, "テイク（重なっているもののいずれか）").req()],
            |p, _, a| cycle(p, a, false),
        ),
    ]
}

fn clear(
    p: &mut Project,
    what: &str,
    on: fn(&Track) -> bool,
    off: fn(&mut Track),
) -> CmdResult<Output> {
    let ids: Vec<TrackId> = p.tracks.values().filter(|t| on(t)).map(|t| t.id).collect();
    for id in &ids {
        off(p.track_mut(*id)?);
    }
    Ok(Output::msg(format!(
        "{} トラックの{what}を解除しました",
        ids.len()
    )))
}

/// Plays the row after (or before) the one that plays now, and only it.
fn cycle(p: &mut Project, a: &Args, forward: bool) -> CmdResult<Output> {
    let id: ClipId = a.id("id")?;
    let (t, _) = p.clip(id)?;
    let stack = crate::takes::stack_of(t, id)
        .ok_or_else(|| CmdError::invalid("テイクが重なっていません"))?;
    let given = stack
        .slots
        .iter()
        .find(|s| s.clip == id)
        .expect("stack contains take");
    // The row that plays: the given take's when it plays, else the first that plays.
    let now = if given.plays {
        given.row
    } else {
        stack
            .slots
            .iter()
            .find(|s| s.plays)
            .map_or(given.row, |s| s.row)
    };
    let row = if forward {
        (now + 1) % stack.rows
    } else {
        (now + stack.rows - 1) % stack.rows
    };
    for slot in &stack.slots {
        p.clip_mut(slot.clip)?.1.muted = slot.row != row;
    }
    let clip = stack.row(row).next().map(|s| s.clip.to_string());
    Ok(Output::with(
        format!("テイク {} を再生します", row + 1),
        json!({ "clip": clip, "row": row }),
    ))
}
