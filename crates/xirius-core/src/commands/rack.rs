//! `rack.show`: the track's insert wiring as a tree (PRJ-026).
use super::*;
use crate::id::{IdKind, TrackId};

pub(super) fn defs() -> Vec<CommandDef> {
    vec![read(
        "rack.show",
        "トラックのラック（音源と FX の木）を JSON で返します。ラックの無いトラックは直列の形を implicit: true で返します",
        vec![ArgDef::id("track", IdKind::Track, "トラック").req()],
        |p, _, _, a| {
            let tid: TrackId = a.id("track")?;
            let t = p.track(tid)?;
            let v = t.rack_json();
            let has = t.rack.is_some();
            Ok(Output::with(
                if has {
                    format!("{} のラックを表示しました", t.name)
                } else {
                    format!("{} のラック（直列）を表示しました", t.name)
                },
                v,
            ))
        },
    )]
}
