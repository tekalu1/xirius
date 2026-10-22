//! Bouncing (EDT-015, EXP-003). `clip.bounce`, `range.bounce` and `track.bounce` need the
//! sound, so the session answers them: it renders, writes the files into the song's folder and
//! leaves one `bounce.apply` for the history, which puts the new clips and tracks in and mutes
//! what they stand in for.

use std::collections::HashMap;

use super::*;
use crate::auto::AutoTarget;
use crate::command::EditCtx;
use crate::id::{IdKind, TrackId};
use crate::model::{AudioProcess, Track, TrackKind};

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        session(
            "clip.bounce",
            "クリップを、伸縮・速さ・トランスポーズ・逆再生を焼いた 1 つの音に置き換えます（ゲインとフェードはそのまま）。--new-track は、インサートも通した音を下の新しいトラックに置き、元のクリップをミュートします。MIDI のクリップはいつも新しいトラックに置きます",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ"),
                ArgDef::flag("new-track", "新しいトラックに置く"),
            ],
            Risk::Write,
            false,
        ),
        session(
            "range.bounce",
            "範囲のトラックの音を、インサートとボリューム・パンを通してミックスし、下の新しいトラックに置きます（センドとマスターは通しません）。範囲に収まる元のクリップはミュートします",
            vec![
                ArgDef::pos("from", "範囲の開始位置").req(),
                ArgDef::pos("to", "範囲の終了位置").req(),
                ArgDef::id_list("tracks", IdKind::Track, "トラック").req(),
                ArgDef::flag(
                    "per-track",
                    "トラックごとに新しいトラックを作る（ボリューム・パンは焼かない）",
                ),
                ArgDef::float("tail", "残響の尾を含める最大の秒数")
                    .range(0.0, 10.0)
                    .default(json!(10.0)),
            ],
            Risk::Write,
            false,
        ),
        session(
            "track.bounce",
            "トラックの音を、インサートを通して（ボリューム・パンの手前で）下の新しいトラックに置き、元のトラックをミュートします。新しいトラックのボリューム・パン・センド・出力は元と同じです",
            vec![ArgDef::ids(IdKind::Track, "トラック")],
            Risk::Write,
            false,
        ),
        edit(
            "bounce.apply",
            "バウンスした音をクリップとトラックに置きます（clip.bounce・range.bounce・track.bounce が使います）",
            vec![
                ArgDef::objects("replace", "置き換えるクリップ [{id, file, name?}]"),
                ArgDef::objects(
                    "tracks",
                    "新しいトラック [{after, like?, name, clips: [{name, file, start, length}]}]",
                ),
                ArgDef::id_list("mute_clips", IdKind::Clip, "ミュートするクリップ"),
                ArgDef::id_list("mute_tracks", IdKind::Track, "ミュートするトラック"),
            ],
            apply,
        ),
    ]
}

fn str_of<'a>(x: &'a Value, key: &str) -> CmdResult<&'a str> {
    x[key]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| CmdError::invalid(format!("{key} がありません")))
}

fn int_of(x: &Value, key: &str) -> CmdResult<i64> {
    x[key]
        .as_i64()
        .ok_or_else(|| CmdError::invalid(format!("{key} がありません")))
}

fn list<'a>(a: &'a Args, key: &str) -> &'a [Value] {
    a.0.get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn apply(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let replace = list(a, "replace");
    let tracks = list(a, "tracks");
    let mute_clips = a.ids::<ClipId>("mute_clips")?;
    let mute_tracks = a.ids::<TrackId>("mute_tracks")?;
    if replace.is_empty() && tracks.is_empty() {
        return Err(CmdError::invalid(
            "--replace か --tracks のどちらかを指定してください",
        ));
    }
    for x in replace {
        let id: ClipId = str_of(x, "id")?.parse().map_err(CmdError::invalid)?;
        let file = str_of(x, "file")?.to_string();
        let name = x["name"].as_str().filter(|s| !s.trim().is_empty());
        let clip = p.clip_mut(id)?.1;
        let ClipBody::Audio {
            file: f,
            offset,
            process,
            ..
        } = &mut clip.body
        else {
            return Err(CmdError::invalid(format!(
                "クリップ {id} は MIDI です。置き換えられるのはオーディオのクリップだけです"
            )));
        };
        *f = file;
        *offset = 0;
        *process = AudioProcess::default();
        if let Some(name) = name {
            clip.name = name.to_string();
        }
    }
    let mut made = Vec::new();
    let mut clips = Vec::new();
    let mut placed: HashMap<TrackId, usize> = HashMap::new();
    for x in tracks {
        let after: TrackId = str_of(x, "after")?.parse().map_err(CmdError::invalid)?;
        let name = str_of(x, "name")?.to_string();
        let like = match x["like"].as_str() {
            Some(s) => Some(s.parse::<TrackId>().map_err(CmdError::invalid)?),
            None => None,
        };
        let (parent, i) = p
            .find_node(Node::Track(after))
            .ok_or_else(|| CmdError::not_found(format!("トラック {after} が見つかりません")))?;
        let mut t = match like {
            Some(src) => {
                let mut src = p.track(src)?.clone();
                src.clips.clear();
                let mut t = super::track::clone_track(p, &src, false);
                // The inserts are in the sound; the fader, the pan and the sends still work.
                t.inserts.clear();
                t.automation
                    .retain(|l| !matches!(l.target, AutoTarget::Param(..)));
                t.kind = TrackKind::Audio;
                t.input = None;
                t.input_gain_db = 0.0;
                t.armed = false;
                t.monitor = false;
                t.mute = false;
                t.solo = false;
                t.midi_out = None;
                t.frozen = None;
                t
            }
            None => Track::new(TrackId(p.alloc()), TrackKind::Audio, ""),
        };
        t.name = name;
        for c in x["clips"].as_array().map_or(&[][..], Vec::as_slice) {
            let id = ClipId(p.alloc());
            t.clips.push(Clip {
                id,
                name: str_of(c, "name")?.to_string(),
                color: None,
                take: None,
                controllers: Vec::new(),
                start: int_of(c, "start")?,
                length: int_of(c, "length")?.max(1),
                muted: false,
                body: ClipBody::Audio {
                    file: str_of(c, "file")?.to_string(),
                    offset: 0,
                    gain_db: 0.0,
                    process: AudioProcess::default(),
                },
            });
            clips.push(id.to_string());
        }
        let id = t.id;
        p.tracks.insert(id, Arc::new(t));
        let k = placed.entry(after).or_default();
        p.children_mut(parent).insert(i + 1 + *k, Node::Track(id));
        *k += 1;
        made.push(id.to_string());
    }
    for id in &mute_clips {
        p.clip_mut(*id)?.1.muted = true;
    }
    for id in &mute_tracks {
        p.track_mut(*id)?.mute = true;
    }
    // The bouncing command has said what was done; this adds only the new ids.
    Ok(Output::with("", json!({"tracks": made, "clips": clips})))
}
