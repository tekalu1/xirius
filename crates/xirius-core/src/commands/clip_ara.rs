//! Putting audio clips through an ARA plugin and taking them out (EDT-023). The one thing a
//! clip can have plugged into it is an ARA plugin; there is no effect of a clip's own.
//!
//! `clip.ara.attach` needs the plugin list (which plugin, whether it speaks ARA), so the session
//! answers it and leaves one `clip.ara.attach.apply` for the history.

use super::*;
use crate::ara::AraClip;
use crate::id::IdKind;

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        session(
            "clip.ara.attach",
            "オーディオクリップを ARA のプラグインで編集できるようにします。--plugin を省くと、曲で使った ARA のプラグインか、ただ 1 つ入っている ARA のプラグインを使います",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ"),
                ArgDef::text("plugin", "プラグインの id（plugin.list の id）"),
            ],
            Risk::Write,
            false,
        ),
        edit(
            "clip.ara.attach.apply",
            "オーディオクリップを ARA のプラグインに付けます（clip.ara.attach が使います）",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ"),
                ArgDef::text("plugin", "プラグインの id").req(),
                ArgDef::text("name", "プラグインの名前"),
            ],
            |p, _, a| {
                let ids = a.require_ids::<ClipId>("クリップ")?;
                let plugin = a.str("plugin")?.to_string();
                let name = a.opt_str("name").unwrap_or(&plugin).to_string();
                for id in &ids {
                    attachable(p, *id)?;
                }
                for id in ids {
                    let keep = p.ara.clips.get(&id).filter(|r| r.plugin == plugin);
                    let clone_of = keep.and_then(|r| r.clone_of);
                    p.ara.clips.insert(
                        id,
                        AraClip {
                            plugin: plugin.clone(),
                            name: name.clone(),
                            clone_of,
                        },
                    );
                }
                // clip.ara.attach has said what it did.
                Ok(Output::msg(""))
            },
        ),
        edit(
            "clip.ara.detach",
            "オーディオクリップを ARA のプラグインから外します。クリップは元の音で鳴ります",
            vec![ArgDef::ids(IdKind::Clip, "クリップ")],
            |p, _, a| {
                let ids = a.require_ids::<ClipId>("クリップ")?;
                for id in &ids {
                    p.clip(*id)?;
                }
                let n = ids
                    .iter()
                    .filter(|id| p.ara.clips.remove(id).is_some())
                    .count();
                if n == 0 {
                    return Err(CmdError::conflict(
                        "ARA のプラグインに付いたクリップがありません",
                    ));
                }
                Ok(Output::msg(format!(
                    "クリップ {n} 件を ARA のプラグインから外しました"
                )))
            },
        ),
    ]
}

/// An audio clip that plays its file as it is: ARA reads the file, so tempo, speed, pitch and
/// direction of the clip's own are not for it.
pub fn attachable(p: &Project, id: ClipId) -> CmdResult<()> {
    let (_, c) = p.clip(id)?;
    match &c.body {
        ClipBody::Audio { process, .. } if process.is_plain() => Ok(()),
        ClipBody::Audio { .. } => Err(CmdError::conflict(format!(
            "クリップ {id} はテンポ・速さ・ピッチ・逆再生を変えてあります。戻してから付けてください"
        ))),
        ClipBody::Midi { .. } => Err(CmdError::invalid(format!(
            "クリップ {id} は MIDI のクリップです。ARA のプラグインに付けられるのはオーディオのクリップだけです"
        ))),
    }
}
