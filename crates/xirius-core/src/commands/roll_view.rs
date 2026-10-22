//! How the piano roll shows a part (EDT-006, EDT-058): as a keyboard, or as the rows of a drum
//! map. The choice is remembered for the part, and the map for the part's track, outside the
//! history. The map's rows can be hidden, moved and renamed, also for the track.

use super::*;
use crate::drum;
use crate::id::IdKind;
use crate::session::SessionState;

pub(super) fn defs() -> Vec<CommandDef> {
    let mut v = vec![read(
        "roll.view",
        "パートをピアノロールで鍵盤で見るか、ドラムの行（ドラムマップ）で見るかを選びます。パートごと、マップはトラックごとに覚えます",
        vec![
            ArgDef::id("clip", IdKind::Clip, "クリップ").req(),
            ArgDef::choice("mode", &["keys", "drums"], "見方"),
            ArgDef::choice("map", &["gm", "kit"], "ドラムマップ"),
        ],
        |p, s, _, a| {
            let clip: ClipId = a.id("clip")?;
            let (tid, _) = p
                .find_clip(clip)
                .ok_or_else(|| CmdError::not_found(format!("クリップ {clip} が見つかりません")))?;
            let mode = a.opt_str("mode");
            let map = a.opt_str("map");
            if mode.is_none() && map.is_none() {
                return Err(CmdError::invalid("--mode か --map が必要です"));
            }
            if let Some(mode) = mode {
                let on = match mode {
                    "keys" => false,
                    "drums" => true,
                    other => {
                        return Err(CmdError::invalid(format!(
                            "見方 `{other}` を読み取れません"
                        )));
                    }
                };
                s.view.roll.drum_clips.insert(clip.to_string(), on);
            }
            if let Some(map) = map {
                if !drum::is_map(map) {
                    return Err(CmdError::invalid(format!(
                        "ドラムマップ `{map}` を読み取れません"
                    )));
                }
                s.view
                    .roll
                    .drum_maps
                    .insert(tid.to_string(), map.to_string());
            }
            let shown = drum::shown(p, &s.view.roll, clip);
            let text = match &shown {
                Some(map) => format!("ドラム（{}）で見ます", drum::map_label(map)),
                None => "鍵盤で見ます".to_string(),
            };
            Ok(Output::with(
                text,
                json!({ "mode": if shown.is_some() { "drums" } else { "keys" }, "map": shown }),
            ))
        },
    )];
    v.extend(row_defs());
    v
}

/// The part's track, the map it shows and the pitches its notes use.
fn part(p: &Project, s: &SessionState, a: &Args) -> CmdResult<(String, String, Vec<u8>)> {
    let clip: ClipId = a.id("clip")?;
    let (track, c) = p.clip(clip)?;
    let tid = track.id;
    let map = s
        .view
        .roll
        .drum_maps
        .get(&tid.to_string())
        .filter(|m| drum::is_map(m))
        .cloned()
        .unwrap_or_else(|| drum::DEFAULT_MAP.to_string());
    let used = c.notes().iter().map(|n| n.pitch).collect();
    Ok((tid.to_string(), map, used))
}

fn pitch(a: &Args) -> CmdResult<u8> {
    let p = a.i64("pitch")?;
    u8::try_from(p)
        .ok()
        .filter(|p| *p <= 127)
        .ok_or_else(|| CmdError::invalid(format!("ノート番号 {p} は 0〜127 で指定してください")))
}

/// What the part's rows are after a change, as the output carries them.
fn rows_out(text: String, map: &str, e: &drum::RowEdits, used: &[u8]) -> Output {
    let rows: Vec<Value> = drum::shown_rows(map, e, used.iter().copied())
        .into_iter()
        .map(|p| json!({ "pitch": p, "name": e.name(p) }))
        .collect();
    Output::with(text, json!({ "rows": rows, "hidden": e.hidden }))
}

fn row_defs() -> Vec<CommandDef> {
    let clip = || ArgDef::id("clip", IdKind::Clip, "クリップ").req();
    let pitch_arg = || {
        ArgDef::int("pitch", "行のノート番号")
            .req()
            .range(0.0, 127.0)
    };
    vec![
        read(
            "roll.drum.hide",
            "ドラム表示の行を隠します（--show で戻します）。ノートのある行は隠しても出たままです。トラックごとに覚えます",
            vec![clip(), pitch_arg(), ArgDef::flag("show", "隠した行を戻す")],
            |p, s, _, a| {
                let (tid, map, used) = part(p, s, a)?;
                let pitch = pitch(a)?;
                let show = a.flag("show");
                let e = s.view.roll.drum_rows.entry(tid).or_default();
                e.hidden.retain(|q| *q != pitch);
                if !show {
                    e.hidden.push(pitch);
                    e.hidden.sort_unstable();
                }
                let e = e.clone();
                let verb = if show {
                    "表示します"
                } else {
                    "隠します"
                };
                Ok(rows_out(
                    format!("{} の行を{verb}", e.name(pitch)),
                    &map,
                    &e,
                    &used,
                ))
            },
        ),
        read(
            "roll.drum.move",
            "ドラム表示の行を上下へ移します（--by -1 で 1 つ上）。トラックごとに覚えます",
            vec![
                clip(),
                pitch_arg(),
                ArgDef::int("by", "移す行の数（負で上へ）").req(),
            ],
            |p, s, _, a| {
                let (tid, map, used) = part(p, s, a)?;
                let pitch = pitch(a)?;
                let by = a.i64("by")?;
                let e = s.view.roll.drum_rows.entry(tid).or_default();
                let mut rows = drum::shown_rows(&map, e, used.iter().copied());
                let Some(at) = rows.iter().position(|q| *q == pitch) else {
                    return Err(CmdError::not_found(format!(
                        "ノート {pitch} の行は表示されていません"
                    )));
                };
                let to = (at as i64 + by).clamp(0, rows.len() as i64 - 1) as usize;
                let row = rows.remove(at);
                rows.insert(to, row);
                e.order = rows;
                let e = e.clone();
                Ok(rows_out(
                    format!("{} の行を移しました", e.name(pitch)),
                    &map,
                    &e,
                    &used,
                ))
            },
        ),
        read(
            "roll.drum.rename",
            "ドラム表示の行の名前を変えます（空で元の名前に戻します）。トラックごとに覚えます",
            vec![clip(), pitch_arg(), ArgDef::text("name", "名前").req()],
            |p, s, _, a| {
                let (tid, map, used) = part(p, s, a)?;
                let pitch = pitch(a)?;
                let name = a.str("name")?.trim().to_string();
                let e = s.view.roll.drum_rows.entry(tid).or_default();
                if name.is_empty() {
                    e.names.remove(&pitch);
                } else {
                    e.names.insert(pitch, name);
                }
                let e = e.clone();
                Ok(rows_out(
                    format!("ノート {pitch} の行を「{}」にします", e.name(pitch)),
                    &map,
                    &e,
                    &used,
                ))
            },
        ),
        read(
            "roll.drum.reset",
            "ドラム表示の行の並び・隠した行・名前を、マップのとおりに戻します",
            vec![clip()],
            |p, s, _, a| {
                let (tid, map, used) = part(p, s, a)?;
                s.view.roll.drum_rows.remove(&tid);
                Ok(rows_out(
                    "ドラムの行をマップのとおりに戻します".to_string(),
                    &map,
                    &drum::RowEdits::default(),
                    &used,
                ))
            },
        ),
    ]
}
