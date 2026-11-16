//! The one instrument track whose notes a receiving effect takes (MIX-051).
use super::*;
use crate::id::{IdKind, PluginId, TrackId};
use crate::model::{NoteSource, Plugin, TrackKind};

pub(super) fn defs() -> Vec<CommandDef> {
    let plugin = || ArgDef::id("id", IdKind::Plugin, "ノートを受けるプラグイン").req();
    vec![
        edit(
            "plugin.pitch_key.toggle",
            "パラメーターを設定",
            vec![
                plugin(),
                ArgDef::int("relative", "Key").range(0.0, 11.0).req(),
            ],
            |p, _, a| {
                let id: PluginId = a.id("id")?;
                let relative =
                    u8::try_from(a.i64("relative")?).map_err(|_| CmdError::invalid("Key"))?;
                let x = receiving_plugin(p, id)?;
                if x.builtin().is_none_or(|def| def.uid != "pitch-correction") {
                    return Err(CmdError::conflict("Pitch Correction"));
                }
                let scale = x.params.get("scale").copied().unwrap_or(0.0);
                let next = xirius_params::pitch_correction::toggle_relative(scale, relative);
                let mask = xirius_params::pitch_correction::scale_mask(next);
                let next = match mask {
                    xirius_params::pitch_correction::CHROMATIC => 0.0,
                    xirius_params::pitch_correction::MAJOR => 1.0 / 3.0,
                    xirius_params::pitch_correction::MINOR => 2.0 / 3.0,
                    _ => next,
                };
                x.params.insert("scale".to_string(), next);
                Ok(Output::msg("Key"))
            },
        ),
        edit(
            "plugin.note_source.set",
            "プラグインのノートの送り元を設定します",
            vec![
                plugin(),
                ArgDef::id("from", IdKind::Track, "送り元のトラック").req(),
            ],
            |p, _, a| {
                let id: PluginId = a.id("id")?;
                let from: TrackId = a.id("from")?;
                let name = p.track(from)?.name.clone();
                if p.track(from)?.kind != TrackKind::Instrument {
                    return Err(CmdError::conflict(format!(
                        "{name} はインストゥルメントのトラックではありません"
                    )));
                }
                let x = receiving_plugin(p, id)?;
                if !x.takes_notes() {
                    return Err(CmdError::conflict(format!(
                        "{} はノートを受けられません",
                        x.name
                    )));
                }
                x.note_source = Some(NoteSource {
                    on: true,
                    track: from,
                });
                Ok(Output::msg(format!(
                    "{id} のノートの送り元を {name} に設定しました"
                )))
            },
        ),
        destroy(
            "plugin.note_source.remove",
            "プラグインのノートの送り元を外します",
            vec![plugin()],
            |p, _, a| {
                let id: PluginId = a.id("id")?;
                let x = receiving_plugin(p, id)?;
                if x.note_source.take().is_none() {
                    return Err(CmdError::not_found(format!(
                        "プラグイン {id} にノートの送り元がありません"
                    )));
                }
                Ok(Output::msg(format!("{id} のノートの送り元を外しました")))
            },
        ),
        edit(
            "plugin.note_source.enable",
            "ノートの送り元の入・切を切り替えます",
            vec![
                plugin(),
                ArgDef::choice("state", &["on", "off"], "入・切").req(),
            ],
            |p, _, a| {
                let id: PluginId = a.id("id")?;
                let on = a.str("state")? == "on";
                let x = receiving_plugin(p, id)?;
                let Some(source) = &mut x.note_source else {
                    return Err(CmdError::not_found(format!(
                        "プラグイン {id} にノートの送り元がありません"
                    )));
                };
                source.on = on;
                Ok(Output::msg(format!(
                    "{id} のノートの送り元を{}",
                    onoff_word(on)
                )))
            },
        ),
        read(
            "plugin.note_source.list",
            "ノートの送り元を表示します",
            vec![plugin()],
            |p, _, _, a| {
                let id: PluginId = a.id("id")?;
                let owner = super::track::plugin_owner(p, id)?;
                let x = p
                    .track(owner)?
                    .inserts
                    .iter()
                    .find(|x| x.id == id)
                    .expect("found");
                let source = x.note_source;
                let name = source
                    .and_then(|s| p.tracks.get(&s.track))
                    .map(|t| t.name.as_str());
                Ok(Output::with(
                    format!("{id} のノートの送り元"),
                    json!({
                        "id": id.to_string(), "on": source.is_some_and(|s| s.on),
                        "from": source.map(|s| s.track.to_string()), "name": name,
                    }),
                ))
            },
        ),
    ]
}

fn receiving_plugin(p: &mut Project, id: PluginId) -> CmdResult<&mut Plugin> {
    let owner = super::track::plugin_owner(p, id)?;
    Ok(p.track_mut(owner)?
        .inserts
        .iter_mut()
        .find(|x| x.id == id)
        .expect("found"))
}
