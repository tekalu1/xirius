use super::*;
use crate::id::IdKind;

pub(super) fn defs() -> Vec<CommandDef> {
    vec![edit(
        "clip.join",
        "同じトラックのクリップを結合します",
        vec![ArgDef::ids(IdKind::Clip, "クリップ")],
        join_clips,
    )]
}

fn join_clips(p: &mut Project, _: &crate::command::EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let ids = a.require_ids::<ClipId>("クリップ")?;
    if ids.len() < 2 {
        return Err(CmdError::invalid("結合には 2 つ以上のクリップが必要です"));
    }
    let mut clips = ids
        .iter()
        .map(|id| p.clip(*id).map(|(t, c)| (t.id, c.clone())))
        .collect::<CmdResult<Vec<_>>>()?;
    clips.sort_by_key(|(_, c)| c.start);
    let (track, first) = clips[0].clone();
    if clips.iter().any(|(_, c)| {
        c.muted != first.muted
            || p.clip_fx
                .get(&c.id)
                .is_some_and(|fx| *fx != crate::clipfx::ClipFx::default())
    }) {
        return Err(CmdError::conflict(
            "ミュート状態が異なるクリップや、エンベロープのあるクリップは結合できません",
        ));
    }
    if clips
        .iter()
        .any(|(t, c)| *t != track || c.is_midi() != first.is_midi())
    {
        return Err(CmdError::conflict(
            "同じトラックの同じ種類のクリップを選択してください",
        ));
    }
    let end = clips.iter().map(|(_, c)| c.start + c.length).max().unwrap();
    let mut joined = first.clone();
    joined.length = end - first.start;
    match &mut joined.body {
        ClipBody::Midi { notes, .. } => {
            let mut all = Vec::new();
            joined.controllers.clear();
            for (_, c) in &clips {
                let delta = c.start - first.start;
                for n in c.notes() {
                    if let Some(o) = p.original_timing.get_mut(&n.id) {
                        o.start += delta;
                    }
                }
                // Each clip's transpose goes into its notes, so the joined clip plays what the
                // pieces played; the joined clip starts at 0 (UI-311).
                let by = i32::from(c.transpose());
                for n in c.notes() {
                    if n.start >= c.length || n.start + n.length <= 0 {
                        continue;
                    }
                    let pitch = i32::from(n.pitch) + by;
                    if !(0..=127).contains(&pitch) {
                        return Err(CmdError::range(format!(
                            "ノート {} を {by} 半音移動すると範囲外になります（{} → {pitch}）",
                            n.id, n.pitch
                        )));
                    }
                    all.push(Note {
                        start: n.start + delta,
                        length: n.length.min(c.length - n.start),
                        pitch: pitch as u8,
                        ..*n
                    });
                }
                for lane in &c.controllers {
                    if let Some(existing) =
                        joined.controllers.iter_mut().find(|l| l.kind == lane.kind)
                    {
                        Arc::make_mut(&mut existing.points).extend(
                            lane.points
                                .iter()
                                .filter(|pt| pt.tick < c.length)
                                .map(|pt| crate::model::CtrlPoint {
                                    tick: pt.tick + delta,
                                    ..*pt
                                }),
                        );
                    } else {
                        let mut lane = lane.clone();
                        lane.points = Arc::new(
                            lane.points
                                .iter()
                                .filter(|pt| pt.tick < c.length)
                                .map(|pt| crate::model::CtrlPoint {
                                    tick: pt.tick + delta,
                                    ..*pt
                                })
                                .collect(),
                        );
                        joined.controllers.push(lane);
                    }
                }
            }
            all.sort_by_key(|n| (n.start, n.pitch, n.id));
            *notes = Arc::new(all);
            for lane in &mut joined.controllers {
                Arc::make_mut(&mut lane.points).sort_by_key(|pt| pt.tick);
            }
        }
        ClipBody::Audio {
            file,
            offset,
            gain_db,
            process,
        } => {
            let mut covered = first.start + first.length;
            for (_, c) in clips.iter().skip(1) {
                let ClipBody::Audio {
                    file: other,
                    offset: source,
                    gain_db: other_gain,
                    process: other_process,
                } = &c.body
                else {
                    unreachable!()
                };
                if other != file
                    || *other_gain != *gain_db
                    || other_process != process
                    || *source != *offset + c.start - first.start
                    || c.start != covered
                {
                    return Err(CmdError::conflict(
                        "連続する同じ素材のクリップだけを結合できます",
                    ));
                }
                covered = covered.max(c.start + c.length);
            }
        }
    }
    // Only MIDI clips carry it; the pieces' values went into their notes above.
    joined.set_transpose(0);
    let t = p.track_mut(track)?;
    t.clips.retain(|c| !ids.contains(&c.id));
    t.clips.push(joined);
    for id in ids.iter().filter(|id| **id != first.id) {
        p.clip_fx.remove(id);
    }
    Ok(Output::with(
        "クリップを結合しました",
        json!({"ids":[first.id.to_string()]}),
    ))
}
