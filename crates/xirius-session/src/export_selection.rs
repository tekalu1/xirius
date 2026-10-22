//! Clip bounds determine the export range before the render snapshot is taken.
use xirius_core::{Args, CmdError, CmdResult, id::ClipId, model::Project, time::Tick};

pub fn span(project: &Project, clips: &[ClipId]) -> CmdResult<(Tick, Tick)> {
    if clips.is_empty() {
        return Err(CmdError::invalid("クリップが選択されていません"));
    }
    let mut start = Tick::MAX;
    let mut end = 0;
    for id in clips {
        let (_, clip) = project.clip(*id)?;
        start = start.min(clip.start);
        end = end.max(clip.end());
    }
    Ok((start, end))
}

pub(crate) fn ranged(project: &Project, args: &Args) -> CmdResult<Args> {
    let (start, end) = span(project, &args.ids::<ClipId>("ids")?)?;
    let mut args = args.0.clone();
    args.remove("ids");
    args.insert("range".into(), "custom".into());
    args.insert("from".into(), start.into());
    args.insert("to".into(), end.into());
    Ok(Args(args))
}

/// Older export records can contain bar positions instead of resolved ticks.
pub fn saved_span(project: &Project, args: &serde_json::Value) -> CmdResult<(Tick, Tick)> {
    let tick = |key: &str| {
        if let Some(tick) = args[key].as_i64() {
            return Ok(tick);
        }
        let text = args[key]
            .as_str()
            .ok_or_else(|| CmdError::invalid("書き出しの範囲が不正です"))?;
        project.time_map().parse(text)
    };
    let (start, end) = (tick("from")?, tick("to")?);
    if start < 0 || end <= start {
        return Err(CmdError::range("書き出しの範囲が不正です"));
    }
    Ok((start, end))
}
