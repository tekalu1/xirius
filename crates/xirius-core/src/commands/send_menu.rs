//! Send rows: mute retains the fader and its automation, and retargeting is atomic.
use super::*;
use crate::auto::AutoTarget;
use crate::id::{IdKind, SendId, TrackId};
use crate::model::{MIN_DB, Track, TrackKind};

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        edit(
            "send.mute",
            "センドのミュートを切り替えます",
            [
                vec![ArgDef::id("id", IdKind::Send, "センド").req()],
                on_off().to_vec(),
            ]
            .concat(),
            |p, _, a| {
                let id: SendId = a.id("id")?;
                let tid = owner(p, id)?;
                let t = p.track_mut(tid)?;
                let s = t.sends.iter_mut().find(|s| s.id == id).expect("send owner");
                let on = a.toggle()?.unwrap_or(s.muted_level.is_none());
                if on && s.muted_level.is_none() {
                    s.muted_level = Some(s.level_db);
                    s.level_db = MIN_DB;
                }
                if !on && let Some(level) = s.muted_level.take() {
                    s.level_db = level;
                }
                for l in &mut t.automation {
                    if l.target == AutoTarget::Send(id) {
                        l.suspended = on;
                    }
                }
                Ok(Output::msg("センドのミュートを変更しました"))
            },
        ),
        edit(
            "send.target.set",
            "センドの送り先を変更します",
            vec![
                ArgDef::id("id", IdKind::Send, "センド").req(),
                ArgDef::text("to", "送り先のバスの ID または new").req(),
            ],
            |p, _, a| {
                let id: SendId = a.id("id")?;
                let tid = owner(p, id)?;
                let to = if a.str("to")? == "new" {
                    let to = TrackId(p.alloc());
                    let name = super::track::unique_name(p, "Bus");
                    let mut bus = Track::new(to, TrackKind::Bus, name);
                    bus.color = Some(p.next_track_color());
                    p.tracks.insert(to, Arc::new(bus));
                    p.root.push(crate::id::Node::Track(to));
                    to
                } else {
                    a.id::<TrackId>("to")?
                };
                p.bus(to)?;
                if p.track(tid)?.sends.iter().any(|s| s.id != id && s.to == to) {
                    return Err(CmdError::conflict("このバスにはすでに送信しています"));
                }
                p.track_mut(tid)?
                    .sends
                    .iter_mut()
                    .find(|s| s.id == id)
                    .expect("send owner")
                    .to = to;
                Ok(Output::msg("送り先を変更しました"))
            },
        ),
        edit(
            "send.pre",
            "センドのプリフェーダーを切り替えます",
            [
                vec![ArgDef::id("id", IdKind::Send, "センド").req()],
                on_off().to_vec(),
            ]
            .concat(),
            |p, _, a| {
                let id: SendId = a.id("id")?;
                let tid = owner(p, id)?;
                let s = p
                    .track_mut(tid)?
                    .sends
                    .iter_mut()
                    .find(|s| s.id == id)
                    .expect("send owner");
                s.pre = a.toggle()?.unwrap_or(!s.pre);
                Ok(Output::msg("センドの位置を変更しました"))
            },
        ),
    ]
}

fn owner(p: &Project, id: SendId) -> CmdResult<TrackId> {
    p.tracks
        .values()
        .find(|t| t.sends.iter().any(|s| s.id == id))
        .map(|t| t.id)
        .ok_or_else(|| CmdError::not_found(format!("センド {id} が見つかりません")))
}
