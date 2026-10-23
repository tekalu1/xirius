//! Stages 15–24: the clips split, looped and faded, the mixer's fader, pan, insert and send,
//! automation drawn, a marker, a bounce and a freeze.

use super::make::{shut_drawer, tool};
use super::{
    Done, Studio1, cell, cell_where, check, click, clip_rect, drag, mid, press, read, right_click,
    row_of, spot, track_of, tracks,
};
use crate::{
    mixer::MixerHit,
    popup::Action,
    studio::{DrawerTab, Hit, Studio},
    tools::Tool,
    tree::RowKind,
};
use gpui::{App, Window};
use xirius_core::{auto::AutoTarget, id::TrackId};

pub(super) fn play(n: u8, s: &Studio1, w: &mut Window, cx: &mut App) -> Done {
    match n {
        15 => {
            let t = track_of(|m| m.audio, "audio")?;
            shut_drawer(s, w, cx)?;
            tool(s, Tool::Cut, w, cx)?;
            let cut = s.borrow().project().time_map().bar_start(3);
            let p = read(s, w, |s, g| {
                let r = clip_rect(s, g, t, 0)?;
                Ok::<_, String>((s.tick_x(g, cut), r.y + r.h / 2.0))
            })?;
            click(p, w, cx);
            let starts = starts(s, t);
            check(
                starts.len() == 2 && starts[1] == cut,
                format!("the clips start at {starts:?}, cut at {cut}"),
            )?;
            tool(s, Tool::Select, w, cx)?;
            Ok(format!("clips start at {starts:?}"))
        }
        16 => {
            let t = track_of(|m| m.synth, "instrument")?;
            let (corner, id, length) = read(s, w, |s, g| {
                let r = clip_rect(s, g, t, 0)?;
                let c = first(s, t).ok_or("no part")?;
                Ok::<_, String>(((r.right() - 2.0, r.bottom() - 2.0), c.id, c.length))
            })?;
            let half = read(s, w, |s, _| s.ticks_to_px(length / 2));
            drag(corner, (corner.0 - half, corner.1), w, cx);
            let p = s.borrow().project();
            let l = p.clip_loop(id);
            let now = first(&s.borrow(), t).map(|c| c.length);
            check(l.is_some(), "no loop")?;
            check(
                now == Some(length),
                format!("the part's length went to {now:?}"),
            )?;
            Ok(format!("loop={l:?} length={length}"))
        }
        17 => {
            let t = track_of(|m| m.audio, "audio")?;
            let linked = s.borrow().song.session().state.view.link;
            s.borrow_mut()
                .arrange_run("view.link", serde_json::json!({ "on": true }));
            let mut lengths = Vec::new();
            let step = read(s, w, |s, g| clip_rect(s, g, t, 0).map(|r| r.w / 4.0))?;
            for dx in [step, step, -step] {
                let (flag, id, before) = read(s, w, |s, g| {
                    let p = s.project();
                    let track = &p.tracks[&t];
                    let c = first(s, t).ok_or("no clip")?;
                    let r = clip_rect(s, g, t, 0)?;
                    let fx = crate::arrange::clipfx::FxView::of(&p, track, &c).ok_or("no fades")?;
                    let f = fx.handles(s, r).flag_in.ok_or("no fade-in flag")?;
                    Ok::<_, String>((mid(f), c.id, fx.fade_in.length))
                })?;
                drag(flag, (flag.0 + dx, flag.1), w, cx);
                let p = s.borrow().project();
                let fade = p.clip_fx.get(&id).map_or(0, |f| f.fade_in.length);
                check(
                    if dx > 0.0 {
                        fade > before
                    } else {
                        fade < before
                    },
                    format!("fade {before} -> {fade}, dx={dx}"),
                )?;
                lengths.push(fade);
            }
            s.borrow_mut().arrange_run(
                "view.link",
                serde_json::json!({ "on": linked, "off": !linked }),
            );
            Ok(format!("fade-in ticks={lengths:?}"))
        }
        18 => {
            let t = track_of(|m| m.synth, "instrument")?;
            mixer(s, w, cx)?;
            let was = volume_pan(s, t);
            let f = spot(s, w, Hit::Mixer(MixerHit::Fader(t)))?;
            drag(f, (f.0, f.1 + 30.0), w, cx);
            let p = spot(s, w, Hit::Mixer(MixerHit::Pan(t)))?;
            drag(p, (p.0, p.1 - 30.0), w, cx);
            let now = volume_pan(s, t);
            check(
                now.0 < was.0,
                format!("the volume went {} -> {}", was.0, now.0),
            )?;
            check(
                now.1 > was.1,
                format!("the pan went {} -> {}", was.1, now.1),
            )?;
            Ok(format!("volume {:.1} dB pan {:.2}", now.0, now.1))
        }
        19 => {
            let t = track_of(|m| m.synth, "instrument")?;
            let before = s.borrow().project().tracks[&t].inserts.len();
            press(s, Hit::Mixer(MixerHit::AddInsert(t)), w, cx)?;
            let to = s
                .borrow()
                .popup
                .as_ref()
                .and_then(|p| {
                    p.pick.as_ref().and_then(|l| {
                        l.groups
                            .iter()
                            .flat_map(|g| g.rows.iter())
                            .find(|r| r.label.starts_with("Reverb"))
                            .and_then(|r| r.act.clone())
                    })
                })
                .and_then(|a| match a {
                    Action::Pick(to) => Some(to),
                    _ => None,
                })
                .ok_or("no Reverb in the list")?;
            cell_where(
                s,
                "Reverb",
                |c| c.action.as_ref() == Some(&Action::Pick(to.clone())),
                w,
                cx,
            )?;
            let p = s.borrow().project();
            let inserts: Vec<_> = p.tracks[&t]
                .inserts
                .iter()
                .map(|i| i.name.clone())
                .collect();
            check(
                inserts.len() == before + 1 && inserts.iter().any(|n| n.contains("Reverb")),
                format!("the inserts are {inserts:?}"),
            )?;
            // Its window opened over the screen; the next press goes to the strips.
            s.borrow_mut().instruments.open.clear();
            Ok(format!("inserts={inserts:?}"))
        }
        20 => {
            let t = track_of(|m| m.synth, "instrument")?;
            let before = tracks(s);
            press(s, Hit::Mixer(MixerHit::AddSend(t)), w, cx)?;
            cell_where(
                s,
                "new bus",
                |c| c.action.as_ref() == Some(&Action::Pick(crate::mixer::NEW_BUS.into())),
                w,
                cx,
            )?;
            let bus = super::made(s, &before)?;
            let p = s.borrow().project();
            let sends = &p.tracks[&t].sends;
            check(
                sends.iter().any(|x| x.to == bus),
                format!("{} sends, none to the new bus", sends.len()),
            )?;
            Ok(format!("send to {}", p.tracks[&bus].name))
        }
        21 => automation(s, w, cx),
        22 => {
            let before = s.borrow().project().markers.len();
            let p = read(s, w, |s, g| {
                let tick = s.project().time_map().bar_start(2);
                (s.tick_x(g, tick) + 2.0, g.ruler.bottom() - 4.0)
            });
            right_click(p, w, cx);
            check(s.borrow().context_open(), "no menu")?;
            cell(s, "マーカーを追加", w, cx)?;
            let p = s.borrow().project();
            check(
                p.markers.len() == before + 1,
                format!("{} markers", p.markers.len()),
            )?;
            let m = p.markers.last().map(|m| (m.tick, m.name.clone()));
            Ok(format!("marker={m:?}"))
        }
        23 => {
            let t = track_of(|m| m.audio, "audio")?;
            let before = tracks(s);
            let id = first(&s.borrow(), t).ok_or("no clip")?.id;
            let p = spot(s, w, Hit::Clip(t, id))?;
            right_click(p, w, cx);
            cell(s, "バウンス", w, cx)?;
            cell(s, "新しいトラックにバウンス", w, cx)?;
            check(s.borrow().job.is_some(), "the bounce did not start")?;
            wait_job(s, w)?;
            let made = super::made(s, &before)?;
            let p = s.borrow().project();
            let clips = p.tracks[&made].clips.len();
            check(clips == 1, format!("{clips} clips on the bounced track"))?;
            Ok(format!("bounced to {}", p.tracks[&made].name))
        }
        24 => {
            let t = track_of(|m| m.synth, "instrument")?;
            let p = read(s, w, |s, g| {
                s.center_of(g, Hit::Row(RowKind::Track(t)))
                    .ok_or("the row is not on the screen")
            })?;
            right_click(p, w, cx);
            cell(s, "トラックをフリーズ", w, cx)?;
            check(s.borrow().job.is_some(), "the freeze did not start")?;
            wait_job(s, w)?;
            let p = s.borrow().project();
            let frozen = p.tracks[&t].frozen.is_some();
            check(frozen, "the track is not frozen")?;
            Ok("frozen".into())
        }
        _ => Err(format!("no stage {n}")),
    }
}

fn first(s: &Studio, t: TrackId) -> Option<xirius_core::model::Clip> {
    let p = s.project();
    p.tracks
        .get(&t)?
        .clips
        .iter()
        .min_by_key(|c| c.start)
        .cloned()
}

fn starts(s: &Studio1, t: TrackId) -> Vec<i64> {
    let p = s.borrow().project();
    let mut v: Vec<_> = p.tracks[&t].clips.iter().map(|c| c.start).collect();
    v.sort_unstable();
    v
}

fn volume_pan(s: &Studio1, t: TrackId) -> (f64, f64) {
    let p = s.borrow().project();
    (p.tracks[&t].volume_db, p.tracks[&t].pan)
}

/// The mixer shown with its tab.
fn mixer(s: &Studio1, w: &mut Window, cx: &mut App) -> Result<(), String> {
    if !s.borrow().mixer_shown() {
        press(s, Hit::Tab(DrawerTab::Mixer), w, cx)?;
    }
    check(s.borrow().mixer_shown(), "the mixer is not shown")
}

/// The work of a bounce or freeze waited for, and the screen built again with what it made.
fn wait_job(s: &Studio1, w: &Window) -> Result<(), String> {
    s.borrow_mut().job_wait();
    check(s.borrow().job.is_none(), "the work did not end in 60 s")?;
    let g = read(s, w, |_, g| *g);
    s.borrow_mut().rebuild(&g);
    Ok(())
}

/// The volume lane opened with the track's automation button, and two points drawn in it.
fn automation(s: &Studio1, w: &mut Window, cx: &mut App) -> Done {
    let t = track_of(|m| m.synth, "instrument")?;
    shut_drawer(s, w, cx)?;
    let toggle = read(s, w, |s, g| {
        let row = row_of(s, t)?;
        let a = crate::studio::RowParts::of(&row)
            .auto
            .ok_or("no automation button")?;
        Ok::<_, String>((
            g.column.x + a.x + a.w / 2.0,
            s.row_y(g, &row) + a.y + a.h / 2.0,
        ))
    })?;
    click(toggle, w, cx);
    let lanes = |s: &Studio1| {
        let p = s.borrow().project();
        p.tracks[&t]
            .automation
            .iter()
            .filter(|l| !l.hidden)
            .map(|l| (l.target.clone(), l.points.len()))
            .collect::<Vec<_>>()
    };
    let shown = lanes(s);
    check(
        shown.first().map(|l| &l.0) == Some(&AutoTarget::Volume),
        format!("the lanes are {shown:?}"),
    )?;
    tool(s, Tool::Draw, w, cx)?;
    let spots = read(s, w, |s, g| {
        let row = row_of(s, t)?;
        let top = s.lane_top(g, &row, 0);
        let tm = s.project().time_map();
        Ok::<_, String>([(2, -12.0), (3, -3.0)].map(|(bar, db)| {
            (
                s.tick_x(g, tm.bar_start(bar)) + 1.0,
                top + crate::arrange::lane_value_y(&AutoTarget::Volume, db),
            )
        }))
    })?;
    for p in spots {
        click(p, w, cx);
    }
    tool(s, Tool::Select, w, cx)?;
    let now = lanes(s);
    let points = now.first().map_or(0, |l| l.1);
    check(points >= 2, format!("the lanes are {now:?}"))?;
    Ok(format!("volume points={points}"))
}
