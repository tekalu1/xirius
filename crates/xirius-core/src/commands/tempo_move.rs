use super::*;
use crate::id::{IdKind, TempoId};

pub(super) fn defs() -> Vec<CommandDef> {
    vec![edit(
        "tempo.move",
        "テンポポイントの位置とBPMを変更します",
        vec![
            ArgDef::id("id", IdKind::Tempo, "テンポポイント").req(),
            ArgDef::pos("at", "位置").req(),
            ArgDef::float("bpm", "BPM").range(1.0, 999.0),
        ],
        |p, _, a| {
            let id: TempoId = a.id("id")?;
            let at = opt_tick(p, a, "at")?.unwrap();
            let index = p
                .tempo
                .iter()
                .position(|t| t.id == id)
                .ok_or_else(|| CmdError::not_found("テンポポイントがありません"))?;
            if at < 0 {
                return Err(CmdError::range("位置は0以上にしてください"));
            }
            if p.tempo[index].tick == 0 && at != 0 {
                return Err(CmdError::range("曲頭のテンポポイントは移動できません"));
            }
            if p.tempo.iter().any(|t| t.id != id && t.tick == at) {
                return Err(CmdError::conflict("その位置にはテンポポイントがあります"));
            }
            p.tempo[index].tick = at;
            if let Some(bpm) = a.opt_f64("bpm") {
                p.tempo[index].bpm = bpm;
            }
            p.tempo.sort_by_key(|t| t.tick);
            Ok(Output::with(
                "テンポポイントを移動しました",
                json!({"id":id,"at":at}),
            ))
        },
    )]
}
