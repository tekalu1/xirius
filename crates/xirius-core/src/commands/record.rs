//! Recording (REC-001, REC-005, REC-014, REC-016). The session's host captures the input while
//! the transport plays; when it stops, the takes come here as one `record.take.add`, so one
//! recording is one step of the history. `record.start` / `record.stop` / `record.status` are
//! answered by the session's host (an extension), not by the song.

use serde_json::json;

use super::*;
use crate::command::EditCtx;
use crate::id::TrackId;
use crate::model::TrackKind;

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        edit(
            "record.take.add",
            "録音したテイクをクリップとして配置します（1 回で取り消せます。既存のクリップは変更しません。同じトラックのテイクは最後のものだけが再生されます）",
            vec![
                ArgDef::objects(
                    "takes",
                    "[{track, start, length, file?, offset?, notes?: [{start, length, pitch, velocity?}], name?, muted?}] の配列。start・length は tick、file は曲のフォルダーからの相対パス（オーディオトラック）、offset はファイルの中で使い始める位置（tick）、notes の start はクリップの先頭から（インストゥルメントトラック）",
                )
                .req(),
            ],
            add_takes,
        ),
        session(
            "record.start",
            "録音準備のトラックの入力を録音します（停止中なら再生も開始します）",
            vec![
                ArgDef::pos("at", "録音を開始する位置（省略時は現在の位置）"),
                ArgDef::float(
                    "latency",
                    "入力のレイテンシー（ms。出力から入力に戻るまでの時間。省略時はデバイスの値）",
                ),
            ],
            Risk::Write,
            false,
        ),
        session(
            "record.stop",
            "録音を停止し、録音したテイクをクリップにします（再生も停止します。1 回の録音は 1 回で取り消せます）",
            vec![],
            Risk::Write,
            false,
        ),
        session(
            "record.status",
            "録音の状態（録音中のトラック・長さ・欠落したフレーム数、パンチ・プリロール・ポストロール・プリカウント・入力クオンタイズの設定）を表示します",
            vec![],
            Risk::Read,
            false,
        ),
        session(
            "record.punch",
            "オートパンチを切り替えます（ループの範囲の中だけを録音します。省略時は現在の状態を切り替え）",
            on_off().to_vec(),
            Risk::Write,
            false,
        ),
        session(
            "record.preroll",
            "プリロールを切り替えます（パンチ範囲の手前から再生を始めます。省略時は現在の状態を切り替え）",
            [
                on_off().to_vec(),
                vec![ArgDef::int("bars", "プリロールの小節数（1〜8）").range(1.0, 8.0)],
            ]
            .concat(),
            Risk::Write,
            false,
        ),
        session(
            "record.postroll",
            "ポストロールを切り替えます（パンチ範囲の後も再生を続けて止めます。オートパンチが入のときだけ使えます）",
            [
                on_off().to_vec(),
                vec![ArgDef::int("bars", "ポストロールの小節数（1〜8）").range(1.0, 8.0)],
            ]
            .concat(),
            Risk::Write,
            false,
        ),
        session(
            "record.count-in",
            "プリカウントを切り替えます（録音の前にクリックを数えます。audio.count-in と同じ値です）",
            [
                on_off().to_vec(),
                vec![ArgDef::int("bars", "プリカウントの小節数（1〜8）").range(1.0, 8.0)],
            ]
            .concat(),
            Risk::Write,
            false,
        ),
        session(
            "record.input-quantize",
            "入力クオンタイズを切り替えます（録音するノートの頭を値にそろえます。長さは変えません）",
            [
                on_off().to_vec(),
                vec![ArgDef::choice(
                    "value",
                    &["1/4", "1/8", "1/16", "1/8T"],
                    "そろえる値",
                )],
            ]
            .concat(),
            Risk::Write,
            false,
        ),
    ]
}

struct Planned {
    track: TrackId,
    start: Tick,
    length: Tick,
    name: Option<String>,
    muted: Option<bool>,
    body: ClipBody,
}

fn add_takes(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let Some(Value::Array(list)) = a.0.get("takes") else {
        return Err(CmdError::invalid(
            "--takes には [{track, start, length, ...}] の配列を指定してください",
        ));
    };
    let mut planned = Vec::new();
    for (i, t) in list.iter().enumerate() {
        let n = i + 1;
        let int = |k: &str| {
            t.get(k).and_then(Value::as_i64).ok_or_else(|| {
                CmdError::invalid(format!("{n} 番目の {k} には数を指定してください"))
            })
        };
        let track: TrackId = t
            .get("track")
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| CmdError::invalid(format!("{n} 番目の track を読み取れません")))?;
        let (start, length) = (int("start")?, int("length")?);
        if start < 0 || length < 1 {
            return Err(CmdError::range(format!(
                "{n} 番目の位置 {start}・長さ {length} は使用できません"
            )));
        }
        let kind = p.track(track)?.kind;
        let body = match (t.get("file").and_then(Value::as_str), kind) {
            (Some(file), TrackKind::Audio) => ClipBody::Audio {
                file: file.to_string(),
                offset: t.get("offset").and_then(Value::as_i64).unwrap_or(0).max(0),
                gain_db: 0.0,
                process: Default::default(),
            },
            (None, TrackKind::Instrument) => {
                let mut notes = Vec::new();
                for x in t
                    .get("notes")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let get = |k: &str| x.get(k).and_then(Value::as_i64);
                    let (Some(at), Some(len), Some(pitch)) =
                        (get("start"), get("length"), get("pitch"))
                    else {
                        return Err(CmdError::invalid(format!(
                            "{n} 番目のノートは {{start, length, pitch}} の形で指定してください"
                        )));
                    };
                    if !(0..=127).contains(&pitch) || at < 0 || len < 1 {
                        return Err(CmdError::range(format!(
                            "{n} 番目のノート（{at}・{len}・{pitch}）は使用できません"
                        )));
                    }
                    notes.push(Note {
                        id: NoteId(p.alloc()),
                        start: at,
                        length: len,
                        pitch: pitch as u8,
                        velocity: get("velocity").unwrap_or(100).clamp(1, 127) as u8,
                        muted: false,
                        ai: false,
                    });
                }
                ClipBody::Midi {
                    notes: Arc::new(notes),
                    transpose: 0,
                }
            }
            (Some(_), _) => {
                return Err(CmdError::conflict(format!(
                    "{n} 番目: オーディオのテイクはオーディオトラックにのみ配置できます（{track}）"
                )));
            }
            (None, _) => {
                return Err(CmdError::conflict(format!(
                    "{n} 番目: MIDI のテイクはインストゥルメントトラックにのみ配置できます（{track}）"
                )));
            }
        };
        planned.push(Planned {
            track,
            start,
            length,
            name: t
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            muted: t.get("muted").and_then(Value::as_bool),
            body,
        });
    }
    if planned.is_empty() {
        return Err(CmdError::new(
            crate::error::ErrorCode::NothingToDo,
            "配置するテイクがありません",
        ));
    }
    // REC-005 / REC-014: every take stays; on each track only the last one plays.
    let mut ids = Vec::new();
    for (i, t) in planned.iter().enumerate() {
        let later = planned[i + 1..].iter().any(|u| u.track == t.track);
        let id = ClipId(p.alloc());
        // A take that was not given a name is called after its track; the rows under the track
        // tell the takes apart (T1, T2, …), and `take` the pieces of each (UI-296).
        let track_name = p.track(t.track)?.name.clone();
        let clip = Clip {
            id,
            name: t.name.clone().unwrap_or(track_name),
            color: None,
            start: t.start,
            length: t.length,
            muted: t.muted.unwrap_or(later),
            take: Some(id),
            controllers: Vec::new(),
            body: t.body.clone(),
        };
        p.track_mut(t.track)?.clips.push(clip);
        ids.push(id.to_string());
    }
    Ok(Output::with(
        format!("テイク {} 本をクリップにしました", ids.len()),
        json!({ "clips": ids }),
    ))
}
