use super::*;
use crate::id::IdKind;

pub(super) fn defs() -> Vec<CommandDef> {
    vec![edit(
        "clip.replace",
        "クリップの素材を差し替えます",
        vec![
            ArgDef::id("id", IdKind::Clip, "クリップ").req(),
            ArgDef::text("file", "素材のファイル").req(),
        ],
        |p, _, a| {
            let id: ClipId = a.id("id")?;
            let file = a.str("file")?.to_string();
            if file.trim().is_empty() {
                return Err(CmdError::invalid("素材のファイルを指定してください"));
            }
            let c = p.clip_mut(id)?.1;
            let ClipBody::Audio {
                file: source,
                offset,
                ..
            } = &mut c.body
            else {
                return Err(CmdError::conflict(
                    "オーディオクリップだけを差し替えできます",
                ));
            };
            *source = file;
            *offset = 0;
            Ok(Output::with(
                "素材を差し替えました",
                json!({"ids":[id.to_string()]}),
            ))
        },
    )]
}
