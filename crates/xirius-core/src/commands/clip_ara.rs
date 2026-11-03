//! Putting audio clips through an ARA plugin and taking them out (EDT-023). The one thing a
//! clip can have plugged into it is an ARA plugin; there is no effect of a clip's own.
//!
//! `clip.ara.attach` needs the plugin list (which plugin, whether it speaks ARA), so the session
//! answers it and leaves one `clip.ara.attach.apply` for the history.

use super::*;
use crate::ara::AraClip;
use crate::id::IdKind;
use crate::model::AudioProcess;

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
            "オーディオクリップを ARA のプラグインに付けます。clip.ara.attach と窓が使う道で、プラグインの id が入っているか・ARA のプラグインかは確かめません（無ければ「見つからない」の印になります）",
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
                let n = ids.len();
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
                // Said here too: the window plugs a clip in through this edit (L3).
                Ok(Output::msg(format!(
                    "クリップ {n} 件を ARA のプラグイン {name} に付けました"
                )))
            },
        ),
        edit(
            "clip.ara.detach",
            "オーディオクリップを ARA のプラグインから外します。クリップは元の音で鳴ります",
            vec![ArgDef::ids(IdKind::Clip, "クリップ")],
            |p, _, a| {
                let ids = a.require_ids::<ClipId>("クリップ")?;
                for id in &ids {
                    unfrozen(p, *id)?;
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

/// Why the clips cannot take an ARA plugin, said the same in the menu, by Ctrl+M and here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unfit {
    /// MIDI, or audio and MIDI together.
    NotAudio,
    Frozen,
    Tempo,
    Speed,
    Pitch,
    Reverse,
    /// Some other process of the clip's own, which the reasons above do not name yet.
    Processed,
}

impl Unfit {
    pub fn said(self) -> &'static str {
        match self {
            Unfit::NotAudio => "音声クリップを選択してください",
            Unfit::Frozen => "フリーズ中のトラックは編集できません",
            Unfit::Tempo => "テンポが変更されています",
            Unfit::Speed => "速さが変更されています",
            Unfit::Pitch => "ピッチが変更されています",
            Unfit::Reverse => "逆再生が有効です",
            Unfit::Processed => "クリップに処理が掛かっています",
        }
    }

    fn error(self, id: ClipId) -> CmdError {
        let text = format!("クリップ {id}: {}", self.said());
        match self {
            Unfit::NotAudio => CmdError::invalid(text),
            _ => CmdError::conflict(text),
        }
    }
}

/// The first reason one clip cannot take an ARA plugin. ARA reads the file, so only an audio
/// clip that plays its file as it is can: tempo, speed, pitch and direction of the clip's own
/// are not for it. A frozen track's clips do not change.
pub fn unfit(p: &Project, id: ClipId) -> CmdResult<Option<Unfit>> {
    let (t, c) = p.clip(id)?;
    let ClipBody::Audio { process, .. } = &c.body else {
        return Ok(Some(Unfit::NotAudio));
    };
    Ok(if t.frozen.is_some() {
        Some(Unfit::Frozen)
    } else if process.tempo.is_some() || process.follow {
        Some(Unfit::Tempo)
    } else if process.speed != AudioProcess::default().speed {
        Some(Unfit::Speed)
    } else if process.semis != 0 || process.cents != 0 {
        Some(Unfit::Pitch)
    } else if process.reverse {
        Some(Unfit::Reverse)
    } else if !process.is_plain() {
        // A field added to the process and not named above: still refused, in a release too.
        debug_assert!(false, "a process field unfit does not name");
        Some(Unfit::Processed)
    } else {
        None
    })
}

/// The reason for a selection: MIDI in it first, then the order of [`Unfit`].
pub fn unfit_all(p: &Project, ids: &[ClipId]) -> Option<Unfit> {
    ids.iter()
        .filter_map(|id| unfit(p, *id).ok().flatten())
        .min_by_key(|u| *u as u8)
}

pub fn attachable(p: &Project, id: ClipId) -> CmdResult<()> {
    match unfit(p, id)? {
        Some(u) => Err(u.error(id)),
        None => Ok(()),
    }
}

/// A frozen track's clips stay as they are, plugged in or not.
fn unfrozen(p: &Project, id: ClipId) -> CmdResult<()> {
    let (t, _) = p.clip(id)?;
    if t.frozen.is_some() {
        return Err(Unfit::Frozen.error(id));
    }
    Ok(())
}
