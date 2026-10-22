//! Freezing tracks (EXP-004). `track.freeze` needs the sound, so the session answers it: it
//! renders each track up to its fader into `Freeze/` and leaves one `track.freeze.apply` for the
//! history. `track.unfreeze` only forgets the file, so undo can bring the frozen sound back.

use super::*;
use crate::error::ErrorCode;
use crate::id::{IdKind, TrackId};
use crate::model::Frozen;

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        session(
            "track.freeze",
            "トラックの音を、インストゥルメントとインサートを通して（ボリューム・パンの手前で）ファイルに書き出し、以後はその音を鳴らします。フリーズ中のトラックのクリップ・インサートは編集できません。ボリューム・パン・センドは使えます",
            vec![ArgDef::ids(IdKind::Track, "トラック")],
            Risk::Write,
            false,
        ),
        edit(
            "track.freeze.apply",
            "フリーズした音をトラックに置きます（track.freeze が使います）",
            vec![ArgDef::objects("tracks", "トラック [{id, file, start}]").req()],
            |p, _, a| {
                let list = a.0.get("tracks").and_then(Value::as_array);
                let list = list.map_or(&[][..], Vec::as_slice);
                if list.is_empty() {
                    return Err(CmdError::invalid("--tracks を指定してください"));
                }
                for x in list {
                    let id: TrackId = x["id"]
                        .as_str()
                        .ok_or_else(|| CmdError::invalid("id がありません"))?
                        .parse()
                        .map_err(CmdError::invalid)?;
                    let file = x["file"]
                        .as_str()
                        .filter(|s| !s.trim().is_empty())
                        .ok_or_else(|| CmdError::invalid("file がありません"))?;
                    let t = p.track_mut(id)?;
                    if !t.has_clips() {
                        return Err(CmdError::invalid(format!(
                            "「{}」はフリーズできません",
                            t.name
                        )));
                    }
                    t.frozen = Some(Frozen {
                        file: file.to_string(),
                        start: x["start"].as_i64().unwrap_or(0),
                    });
                }
                // track.freeze has said what was done.
                Ok(Output::msg(""))
            },
        ),
        edit(
            "track.unfreeze",
            "フリーズを解除し、トラックのクリップ・インストゥルメント・インサートをまた鳴らします",
            vec![ArgDef::ids(IdKind::Track, "トラック")],
            |p, _, a| {
                let mut n = 0;
                for id in a.require_ids::<TrackId>("トラック")? {
                    let t = p.track_mut(id)?;
                    if t.frozen.take().is_some() {
                        n += 1;
                    }
                }
                match n {
                    0 => Err(CmdError::new(
                        ErrorCode::NothingToDo,
                        "フリーズしているトラックがありません",
                    )),
                    1 => Ok(Output::msg("フリーズを解除しました")),
                    n => Ok(Output::msg(format!("{n} トラックのフリーズを解除しました"))),
                }
            },
        ),
    ]
}
