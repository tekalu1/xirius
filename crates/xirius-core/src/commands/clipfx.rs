//! Fades, crossfades and the gain of audio clips (EDT-002, EDT-017).

use serde_json::json;

use super::clip::trim_clip;
use super::*;
use crate::clipfx::{
    CROSS_CURVE, CURVE_MAX, CURVE_MIN, ClipLoop, Cross, Fade, GainPoint, envelope_db_at,
};
use crate::command::EditCtx;
use crate::error::ErrorCode;
use crate::id::{ClipId, IdKind};
use crate::model::{MAX_DB, MIN_DB};

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        edit(
            "clip.fade.set",
            "オーディオクリップのフェードイン・フェードアウトを設定します（長さは tick、カーブは -1〜1。0 が直線、0.5 が等パワー）",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ"),
                ArgDef::int("in", "フェードインの長さ（tick。0 で外す）"),
                ArgDef::int("out", "フェードアウトの長さ（tick。0 で外す）"),
                ArgDef::float("in_curve", "フェードインのカーブ（-1〜1）"),
                ArgDef::float("out_curve", "フェードアウトのカーブ（-1〜1）"),
                ArgDef::float("curve", "両方のカーブ（-1〜1）"),
                ArgDef::flag(
                    "auto",
                    "クリックノイズを避ける短いフェードを両端に付ける（クロスフェード中の端は除く）",
                ),
                ArgDef::flag("clear", "両方のフェードを外す"),
            ],
            fade_set,
        ),
        edit(
            "clip.fade.cross",
            "重なる（または接する）2 つのオーディオクリップにクロスフェードを作ります。1 つだけ選んだときは右隣、なければ左隣と組みます",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ"),
                ArgDef::int(
                    "length",
                    "重ねる長さ（tick。省略時は今の重なり、接していれば短い既定の長さ）",
                ),
                ArgDef::float(
                    "curve",
                    "カーブ（-1〜1。省略時は今の値、新規なら 0.5 の等パワー）",
                ),
                ArgDef::int(
                    "move",
                    "クロスフェードの中心を動かす量（tick。正で後ろへ。両方のクリップの境が一緒に動く）",
                ),
                ArgDef::flag("remove", "クロスフェードを外す（クリップの位置は変えない）"),
            ],
            fade_cross,
        ),
        edit(
            "clip.gain.set",
            "オーディオクリップのゲイン（dB）を設定します。--db は絶対値、--by は今の値からの変化量。MIN〜MAX の範囲に収めます",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ"),
                ArgDef::float("db", "ゲイン（dB）"),
                ArgDef::float("by", "変化量（dB）"),
                ArgDef::flag("reset", "0 dB に戻し、ゲインの線の点も消す"),
            ],
            gain_set,
        ),
        edit(
            "clip.gain.point.add",
            "ゲインの線に点を足します（既にその位置に点があれば値を置き換えます）",
            vec![
                ArgDef::id("id", IdKind::Clip, "クリップ").req(),
                ArgDef::int("at", "クリップの先頭からの位置（tick）").req(),
                ArgDef::float("db", "その位置のゲイン（dB。省略時は線のその位置の値）"),
            ],
            point_add,
        ),
        edit(
            "clip.gain.point.move",
            "ゲインの線の点を動かします（位置は隣の点を越えない範囲に収めます）",
            vec![
                ArgDef::id("id", IdKind::Clip, "クリップ").req(),
                ArgDef::int("index", "点の番号（0 から。ゲインの線の左から数える）").req(),
                ArgDef::int("at", "新しい位置（クリップの先頭から tick）"),
                ArgDef::float("db", "新しいゲイン（dB）"),
            ],
            point_move,
        ),
        destroy(
            "clip.gain.point.delete",
            "ゲインの線の点を消します",
            vec![
                ArgDef::id("id", IdKind::Clip, "クリップ").req(),
                ArgDef::int("index", "点の番号（0 から）").req(),
            ],
            point_delete,
        ),
        edit(
            "clip.loop.set",
            "クリップの中身のループを設定します。クリップはループの終わりまで鳴らしたあと、クリップの終わりまでループを繰り返します（オーディオと MIDI）。--start だけなら終わりを保ち、--length だけなら頭を保ちます",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ"),
                ArgDef::int("start", "ループの頭（クリップの頭からの tick）"),
                ArgDef::int("length", "ループの長さ（tick）"),
                ArgDef::flag("clear", "ループを外す"),
            ],
            loop_set,
        ),
    ]
}

/// Sets, moves or clears the loop of clips (EDT-041). With no loop yet, the
/// loop is the whole clip.
fn loop_set(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let ids = a.require_ids::<ClipId>("クリップ")?;
    let (start, length, clear) = (a.opt_i64("start"), a.opt_i64("length"), a.flag("clear"));
    if start.is_none() && length.is_none() && !clear {
        return Err(CmdError::invalid(
            "--start・--length・--clear のいずれかを指定してください",
        ));
    }
    if start.is_some_and(|x| x < 0) {
        return Err(CmdError::range("ループの頭は 0 以上にしてください"));
    }
    if length.is_some_and(|x| x <= 0) {
        return Err(CmdError::range("ループの長さは 1 以上にしてください"));
    }
    for id in &ids {
        let len = p.clip(*id)?.1.length;
        let was = p.clip_loop(*id).unwrap_or(ClipLoop {
            start: 0,
            length: len,
        });
        let looped = match (start, length) {
            _ if clear => None,
            (Some(s), Some(l)) => Some(ClipLoop {
                start: s,
                length: l,
            }),
            (Some(s), None) => {
                if s >= was.end() {
                    return Err(CmdError::range(format!(
                        "クリップ {id} のループの頭は終わり（{}）より前にしてください",
                        was.end()
                    )));
                }
                Some(ClipLoop {
                    start: s,
                    length: was.end() - s,
                })
            }
            (None, Some(l)) => Some(ClipLoop { length: l, ..was }),
            (None, None) => None,
        };
        p.fx_mut(*id).looped = looped;
        p.tidy_fx(*id);
    }
    Ok(Output::msg(if clear {
        format!("クリップ {} 件のループを外しました", ids.len())
    } else {
        format!("クリップ {} 件のループを設定しました", ids.len())
    }))
}

/// The length of an audio clip, or why it has no fades.
fn audio_length(p: &Project, id: ClipId) -> CmdResult<Tick> {
    let (_, c) = p.clip(id)?;
    if c.is_midi() {
        return Err(CmdError::invalid(format!(
            "クリップ {id} は MIDI です。フェードとゲインの線はオーディオのクリップだけに使えます"
        )));
    }
    Ok(c.length)
}

fn curve_arg(a: &Args, name: &str) -> CmdResult<Option<f64>> {
    match a.opt_f64(name) {
        Some(c) if !(CURVE_MIN..=CURVE_MAX).contains(&c) => Err(CmdError::range(format!(
            "--{name} は {CURVE_MIN} から {CURVE_MAX} の間で指定してください"
        ))),
        other => Ok(other),
    }
}

fn norm(f: Fade) -> Fade {
    if f.length == 0 { Fade::default() } else { f }
}

pub(super) fn fade_set(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let ids = a.require_ids::<ClipId>("クリップ")?;
    let (fin, fout) = (a.opt_i64("in"), a.opt_i64("out"));
    let curve = curve_arg(a, "curve")?;
    let (cin, cout) = (
        curve_arg(a, "in_curve")?.or(curve),
        curve_arg(a, "out_curve")?.or(curve),
    );
    let (auto, clear) = (a.flag("auto"), a.flag("clear"));
    if fin.is_none() && fout.is_none() && cin.is_none() && cout.is_none() && !auto && !clear {
        return Err(CmdError::invalid(
            "--in・--out・--curve・--auto・--clear のいずれかを指定してください",
        ));
    }
    if fin.is_some_and(|x| x < 0) || fout.is_some_and(|x| x < 0) {
        return Err(CmdError::range("フェードの長さは 0 以上にしてください"));
    }
    let short = (i64::from(p.ppq) / 32).max(1);
    for id in &ids {
        let len = audio_length(p, *id)?;
        let (xin, xout, own) = {
            let (t, c) = p.clip(*id)?;
            (
                p.cross_in(t, c).is_some(),
                p.cross_out(t, c),
                p.fx(*id).cloned().unwrap_or_default(),
            )
        };
        let (mut fi, mut fo) = (own.fade_in, own.fade_out);
        if clear {
            (fi, fo) = (Fade::default(), Fade::default());
        }
        if auto {
            if !xin && fi.is_none() {
                fi.length = short.min(len / 2);
            }
            if xout.is_none() && fo.is_none() {
                fo.length = short.min(len / 2);
            }
        }
        let touches_in = clear || fin.is_some() || cin.is_some();
        let touches_out = clear || fout.is_some() || cout.is_some();
        if let Some(l) = fin {
            fi.length = l;
        }
        if let Some(l) = fout {
            fo.length = l;
        }
        if let Some(c) = cin {
            fi.curve = c;
        }
        if let Some(c) = cout {
            fo.curve = c;
        }
        let (fi, fo) = (norm(fi), norm(fo));
        if fi.length + fo.length > len {
            return Err(CmdError::range(format!(
                "クリップ {id} のフェードの長さの合計がクリップより長くなります"
            )));
        }
        // A fade set by hand takes the place of the crossfade on that side.
        if touches_in && xin {
            p.fx_mut(*id).cross = None;
        }
        if touches_out && let Some((right, _, _)) = xout {
            p.fx_mut(right).cross = None;
            p.tidy_fx(right);
        }
        let fx = p.fx_mut(*id);
        fx.fade_in = fi;
        fx.fade_out = fo;
        p.tidy_fx(*id);
    }
    Ok(Output::msg(format!(
        "クリップ {} 件のフェードを設定しました",
        ids.len()
    )))
}

// ---- crossfade -------------------------------------------------------------------------------

/// The pairs (left, right) the selection means: consecutive selected clips of
/// a track that touch or overlap; with one clip, it and its right neighbour,
/// or else its left one.
fn cross_pairs(p: &Project, ids: &[ClipId]) -> CmdResult<Vec<(ClipId, ClipId)>> {
    let mut by_track: Vec<(crate::id::TrackId, Vec<ClipId>)> = Vec::new();
    for id in ids {
        audio_length(p, *id)?;
        let (t, _) = p.clip(*id)?;
        match by_track.iter_mut().find(|(tid, _)| *tid == t.id) {
            Some((_, v)) => v.push(*id),
            None => by_track.push((t.id, vec![*id])),
        }
    }
    let mut out = Vec::new();
    for (tid, group) in by_track {
        let t = p.track(tid)?;
        let start_of = |id: &ClipId| t.clips.iter().find(|c| c.id == *id).map_or(0, |c| c.start);
        let mut group = group;
        group.sort_by_key(start_of);
        let meets = |l: &Clip, r: &Clip| l.start < r.start && l.end() >= r.start;
        let audio = |c: &&Clip| !c.is_midi();
        if group.len() == 1 {
            let c = t.clips.iter().find(|c| c.id == group[0]).expect("found");
            let right = t
                .clips
                .iter()
                .filter(audio)
                .filter(|x| meets(c, x))
                .min_by_key(|x| x.start);
            let left = t
                .clips
                .iter()
                .filter(audio)
                .filter(|x| meets(x, c))
                .max_by_key(|x| x.start);
            match (right, left) {
                (Some(r), _) => out.push((c.id, r.id)),
                (None, Some(l)) => out.push((l.id, c.id)),
                _ => {}
            }
        } else {
            for w in group.windows(2) {
                let l = t.clips.iter().find(|c| c.id == w[0]).expect("found");
                let r = t.clips.iter().find(|c| c.id == w[1]).expect("found");
                if meets(l, r) {
                    out.push((l.id, r.id));
                }
            }
        }
    }
    Ok(out)
}

/// Makes or changes the crossfade of `l` and `r`. `len` is the overlap wanted
/// (kept from the pair's present overlap when `None`); `shift` slides the
/// centre. The pair's edges move so that the right clip starts where the
/// overlap starts and the left ends where it ends; the content of both stays
/// where it is in the song.
fn make_cross(
    p: &mut Project,
    l: ClipId,
    r: ClipId,
    len: Option<Tick>,
    curve: Option<f64>,
    shift: Tick,
) -> CmdResult<()> {
    let (lc, rc) = (p.clip(l)?.1.clone(), p.clip(r)?.1.clone());
    if p.clip(l)?.0.id != p.clip(r)?.0.id {
        return Err(CmdError::conflict("2 つのクリップが別のトラックにあります"));
    }
    if lc.start >= rc.start || lc.end() < rc.start {
        return Err(CmdError::conflict(format!(
            "クリップ {l} と {r} は重なっても接してもいません"
        )));
    }
    let current = crate::clipfx::overlap(&lc, &rc);
    if current > 0 && lc.end() >= rc.end() {
        return Err(CmdError::conflict(format!(
            "クリップ {r} は {l} の中に収まっているため、クロスフェードにできません"
        )));
    }
    let existing = p.fx(r).and_then(|f| f.cross).filter(|c| c.with == l);
    let default = (i64::from(p.ppq) / 8)
        .max(2)
        .min(lc.length / 2)
        .min(rc.length / 2);
    let want = len.unwrap_or(if current > 0 { current } else { default });
    if want < 1 {
        return Err(CmdError::range(
            "クロスフェードの長さは 1 tick 以上にしてください",
        ));
    }
    let new_start = if current > 0 {
        // keep the centre
        (rc.start + lc.end() - want).div_euclid(2) + shift
    } else {
        // a fresh crossfade over a join: centred on it, as far as the right
        // clip has material before its start
        let head = match &rc.body {
            ClipBody::Audio { offset, .. } => *offset,
            ClipBody::Midi { .. } => 0,
        };
        rc.start - (want / 2).min(head) + shift
    };
    let new_end = new_start + want;
    if new_start <= lc.start || new_end > rc.end() {
        return Err(CmdError::range(
            "クロスフェードがクリップの範囲に収まりません",
        ));
    }
    let d = new_start - rc.start;
    if d != 0 {
        trim_clip(p.clip_mut(r)?.1, r, d)?;
        p.fx_trimmed(r, d);
    }
    let grow = new_end - lc.end();
    if grow != 0 {
        let c = p.clip_mut(l)?.1;
        if c.length + grow < 1 {
            return Err(CmdError::range(format!(
                "クリップ {l} の長さが 1 tick 未満になります"
            )));
        }
        c.length += grow;
    }
    let curve = curve.or(existing.map(|c| c.curve)).unwrap_or(CROSS_CURVE);
    p.fx_mut(r).cross = Some(Cross { with: l, curve });
    Ok(())
}

fn fade_cross(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let ids = a.require_ids::<ClipId>("クリップ")?;
    let (len, shift) = (a.opt_i64("length"), a.opt_i64("move").unwrap_or(0));
    let curve = curve_arg(a, "curve")?;
    if a.flag("remove") {
        let mut n = 0;
        for id in &ids {
            audio_length(p, *id)?;
            // the crossfades this clip is in, on either side
            let (t, c) = p.clip(*id)?;
            let mut rights: Vec<ClipId> = t
                .clips
                .iter()
                .filter(|r| {
                    p.fx(r.id)
                        .and_then(|f| f.cross)
                        .is_some_and(|x| x.with == *id)
                })
                .map(|r| r.id)
                .collect();
            if p.cross_in(t, c).is_some() {
                rights.push(*id);
            }
            for r in rights {
                p.fx_mut(r).cross = None;
                p.tidy_fx(r);
                n += 1;
            }
        }
        if n == 0 {
            return Err(CmdError::new(
                ErrorCode::NothingToDo,
                "外すクロスフェードがありません",
            ));
        }
        return Ok(Output::msg(format!("クロスフェード {n} 件を外しました")));
    }
    let pairs = cross_pairs(p, &ids)?;
    if pairs.is_empty() {
        return Err(CmdError::new(
            ErrorCode::NothingToDo,
            "重なる、または接するオーディオクリップの組がありません",
        ));
    }
    for (l, r) in &pairs {
        make_cross(p, *l, *r, len, curve, shift)?;
    }
    Ok(Output::with(
        format!("クロスフェード {} 件を作りました", pairs.len()),
        json!({ "ids": pairs.iter().map(|(_, r)| r.to_string()).collect::<Vec<_>>() }),
    ))
}

// ---- gain ----------------------------------------------------------------------------------------

pub(super) fn gain_set(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let ids = a.require_ids::<ClipId>("クリップ")?;
    let (db, by, reset) = (a.opt_f64("db"), a.opt_f64("by"), a.flag("reset"));
    if usize::from(db.is_some()) + usize::from(by.is_some()) + usize::from(reset) != 1 {
        return Err(CmdError::invalid(
            "--db・--by・--reset のいずれか 1 つだけを指定してください",
        ));
    }
    for id in &ids {
        audio_length(p, *id)?;
        let c = p.clip_mut(*id)?.1;
        let ClipBody::Audio { gain_db, .. } = &mut c.body else {
            unreachable!("checked above");
        };
        *gain_db = match (db, by) {
            (Some(v), _) => v,
            (_, Some(d)) => *gain_db + d,
            _ => 0.0,
        }
        .clamp(MIN_DB, MAX_DB);
        if reset && let Some(fx) = p.clip_fx.get_mut(id) {
            fx.points.clear();
            p.tidy_fx(*id);
        }
    }
    Ok(Output::msg(format!(
        "クリップ {} 件のゲインを設定しました",
        ids.len()
    )))
}

fn point_index(p: &Project, id: ClipId, a: &Args) -> CmdResult<usize> {
    let i = a.i64("index")?;
    let n = p.fx(id).map_or(0, |f| f.points.len());
    usize::try_from(i).ok().filter(|i| *i < n).ok_or_else(|| {
        CmdError::not_found(format!("ゲインの線に {i} 番の点がありません（{n} 個）"))
    })
}

fn point_add(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let id: ClipId = a.id("id")?;
    let len = audio_length(p, id)?;
    let at = a.i64("at")?;
    if !(0..=len).contains(&at) {
        return Err(CmdError::range(format!(
            "--at は 0 から {len}（クリップの長さ）の間で指定してください"
        )));
    }
    let fx = p.fx_mut(id);
    let db = a
        .opt_f64("db")
        .unwrap_or_else(|| envelope_db_at(&fx.points, at))
        .clamp(MIN_DB, MAX_DB);
    let i = match fx.points.binary_search_by_key(&at, |x| x.tick) {
        Ok(i) => {
            fx.points[i].db = db;
            i
        }
        Err(i) => {
            fx.points.insert(i, GainPoint { tick: at, db });
            i
        }
    };
    Ok(Output::with(
        format!("ゲインの線に点を足しました（{i} 番）"),
        json!({ "index": i }),
    ))
}

fn point_move(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let id: ClipId = a.id("id")?;
    let len = audio_length(p, id)?;
    let i = point_index(p, id, a)?;
    if a.opt_i64("at").is_none() && a.opt_f64("db").is_none() {
        return Err(CmdError::invalid("--at か --db を指定してください"));
    }
    let fx = p.fx_mut(id);
    let mut pt = fx.points[i];
    if let Some(at) = a.opt_i64("at") {
        let lo = if i == 0 { 0 } else { fx.points[i - 1].tick + 1 };
        let hi = fx.points.get(i + 1).map_or(len, |x| x.tick - 1);
        pt.tick = at.clamp(lo, hi.max(lo));
    }
    if let Some(db) = a.opt_f64("db") {
        pt.db = db.clamp(MIN_DB, MAX_DB);
    }
    fx.points[i] = pt;
    Ok(Output::with(
        format!("{i} 番の点を動かしました"),
        json!({ "index": i, "at": pt.tick, "db": pt.db }),
    ))
}

fn point_delete(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let id: ClipId = a.id("id")?;
    audio_length(p, id)?;
    let i = point_index(p, id, a)?;
    p.fx_mut(id).points.remove(i);
    p.tidy_fx(id);
    Ok(Output::msg(format!("{i} 番の点を消しました")))
}
