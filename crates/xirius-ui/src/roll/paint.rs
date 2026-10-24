//! The note grid, keyboard and controller lanes, including drag previews.
use super::lanes::y_of;
use super::{Ctx, LaneKind, NoteView, Roll};
use crate::draw::{Align, Pen};
use crate::studio::{Rect, Studio};
use gpui::Hsla;
use xirius_core::model::CtrlShape;
use xirius_core::model::Note;
use xirius_design::{FontSize, Radius, Weight};

/// The spinner in a lit key whose instrument is not ready yet (ENG-064).
const WAIT_SPINNER: f32 = 12.0;

pub fn paint(pen: &mut Pen, studio: &Studio, body: Rect) {
    let Some(target) = studio.roll_target() else {
        return;
    };
    let (mousemap, prefs) = {
        let settings = studio
            .song
            .settings()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        (settings.mousemap.clone(), settings.prefs.clone())
    };
    let ctx = Ctx {
        mousemap,
        prefs,
        maps: studio.meters.of(&studio.project(), studio.roll.beat_px),
        project: studio.project(),
        clip: target.clip,
        tools: studio.roll_tools(),
        shape: studio.song.session().state.view.draw_shape,
        body,
        drum: studio.roll_drum(),
        drum_rows: studio.roll_drum_rows(),
        others: studio.roll_parts(),
    };
    let roll = &studio.roll;
    let f = roll.frame(body);
    let v = roll.view(&ctx);
    // The notes' grid and each lane's share the lines.
    let lines = grid_lines(roll, &ctx);
    super::ruler::paint_ruler(pen, studio, roll, &ctx, &f);
    let all_selected = roll.selected.len() == ctx.notes().len() && ctx.others.is_empty();
    let selected = |id| all_selected || roll.selected.contains(id);
    let color = pen.c.tracks[crate::draw::track_color(&target.track)];
    pen.mask(f.grid, |pen| {
        for pitch in v.rows.pitches() {
            let y = v.y(pitch);
            if y + v.row_h < f.grid.y || y >= f.grid.bottom() {
                continue;
            }
            let black = if v.drums() {
                v.rows.rank(pitch) % 2 == 1
            } else {
                matches!(pitch % 12, 1 | 3 | 6 | 8 | 10)
            };
            pen.fill(
                Rect::new(f.grid.x, y, f.grid.w, v.row_h),
                if black { pen.c.surface_0 } else { pen.c.paper },
            );
        }
        let notes_grid = super::parts::edged(lines.clone(), &ctx);
        crate::grid_lines::paint(pen, &notes_grid, f.grid.y, f.grid.h, |t| v.x(t));
        super::ruler::paint_veil(pen, studio, roll, &ctx, &f);
        super::ruler::paint_shade(pen, studio, roll, &ctx, &f);
        super::parts::paint(pen, roll, &ctx, &v, &f);
        let started = std::time::Instant::now();
        let mut ends = [f32::NEG_INFINITY; 128];
        let ai = super::ai::Notes::new(studio, ctx.clip.id, ctx.ppq());
        // What can be on screen; a glide shows notes away from where the song has them.
        let shown: Vec<&Note> = if ai.moving() {
            ctx.notes().iter().collect()
        } else {
            roll.visible_notes(&ctx, &v)
        };
        let disjoint = roll.preview.ghosts.is_empty()
            && !ai.moving()
            && shown.iter().all(|n| {
                let n = roll.shown(n);
                let end = &mut ends[usize::from(n.pitch)];
                let r = super::notes::note_rect(&v, &n);
                let clear = r.x >= *end;
                *end = r.right();
                clear
            });
        let mute_target = roll.mute_target(&ctx);
        let paint_notes = |pen: &mut Pen| {
            for n in shown.iter().copied() {
                if roll.preview.erasing.contains(&n.id) {
                    continue;
                }
                let mut view = roll.shown(n);
                if mute_target == Some(n.id) {
                    view.muted = !view.muted;
                }
                ai.paint(pen, &v, n, view, note_face(selected(&n.id), color, pen.c));
            }
            for n in &roll.preview.ghosts {
                note(pen, &v, *n, pen.c.ring);
            }
        };
        if disjoint {
            pen.layer(f.grid, paint_notes);
        } else {
            paint_notes(pen);
        }
        roll.disjoint_notes.set(disjoint);
        if let Some((id, rel)) = roll.cut_line(&ctx)
            && let Some(n) = ctx.notes().iter().find(|n| n.id == id)
        {
            let r = super::notes::note_rect(&v, &roll.shown(n));
            let x = v.x(rel).round();
            let ink = pen.c.ink_strong;
            pen.mask(f.grid, |pen| {
                pen.fill(Rect::new(x, r.y - 3.0, 1.0, r.h + 6.0), ink);
            });
        }
        if let Some((_, band)) = roll.zone_band(&ctx) {
            let ring = pen.c.ring;
            pen.mask(f.grid, |pen| pen.round(band, ring, Radius::Pill));
        }
        roll.note_paint_us.set(started.elapsed().as_micros() as u64);
        if let Some((a, b)) = roll.preview.marquee {
            let a = v.screen_point(a.0, a.1);
            let b = v.screen_point(b.0, b.1);
            let mut tint = pen.c.ring;
            tint.a = 0.2;
            pen.fill(
                Rect::new(
                    a.0.min(b.0),
                    a.1.min(b.1),
                    (a.0 - b.0).abs(),
                    (a.1 - b.1).abs(),
                ),
                tint,
            );
        }
    });
    super::ruler::paint_playhead(pen, studio, roll, &ctx, &f);
    // The lit key's instrument is opening again (ENG-064).
    let waiting = studio.roll_key_waiting();
    let spin = studio.spin();
    let spinner = |pen: &mut Pen, face: Rect, color| {
        let d = WAIT_SPINNER.min(face.h);
        let r = Rect::new(
            face.x + (face.w - d) / 2.0,
            face.y + (face.h - d) / 2.0,
            d,
            d,
        );
        pen.icon_turned("spinner", r, color, spin);
    };
    pen.mask(f.keys, |pen| {
        pen.fill(f.keys, pen.c.surface_0);
        for pitch in v.rows.pitches() {
            let y = v.y(pitch);
            if y + v.row_h < f.keys.y || y >= f.keys.bottom() {
                continue;
            }
            if v.drums() {
                let hover = roll.pointer.is_some_and(|(x, py)| {
                    f.keys.contains(x, py) && v.pitch_at(py) == i32::from(pitch)
                });
                let lit = roll.audition == Some(pitch) || hover;
                let row = Rect::new(f.keys.x, y + 0.5, f.keys.w - 2.0, v.row_h - 1.0);
                pen.round(
                    row,
                    if lit { pen.c.sel } else { pen.c.surface_2 },
                    Radius::Xs,
                );
                let ink = if lit { pen.c.ink_strong } else { pen.c.ink };
                pen.mask(row, |pen| {
                    pen.text(
                        &ctx.drum_name(pitch),
                        FontSize::Micro,
                        Weight::Medium,
                        ink,
                        Rect::new(row.x + 6.0, row.y, row.w - 8.0, row.h),
                        Align::Left,
                    );
                });
                if lit && waiting == Some(pitch) {
                    let end = Rect::new(row.right() - row.h - 2.0, row.y, row.h, row.h);
                    spinner(pen, end, ink);
                }
                continue;
            }
            let black = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
            let hover = roll.pointer.is_some_and(|(x, py)| {
                f.keys.contains(x, py) && v.pitch_at(py) == i32::from(pitch)
            });
            let lit = roll.audition == Some(pitch) || hover;
            let face = if lit {
                pen.c.ring
            } else if black {
                pen.c.key_black
            } else {
                pen.c.key_white
            };
            let key = Rect::new(
                f.keys.x,
                y + 0.5,
                f.keys.w * if black { 0.62 } else { 0.97 },
                v.row_h - 1.0,
            );
            pen.round(key, face, Radius::Xs);
            if lit && waiting == Some(pitch) {
                spinner(pen, key, pen.c.on_fill);
            }
            if roll.in_scale(pitch) {
                pen.round(
                    Rect::new(f.keys.x + f.keys.w - 9.0, y + v.row_h / 2.0 - 2.5, 5.0, 5.0),
                    pen.c.ring,
                    Radius::Xs,
                );
            }
            if pitch % 12 == 0 {
                pen.text(
                    &super::pitch_name(pitch),
                    FontSize::Micro,
                    Weight::Medium,
                    if lit {
                        pen.c.key_label_lit
                    } else {
                        pen.c.key_label
                    },
                    Rect::new(f.keys.x, y, f.keys.w - 5.0, v.row_h),
                    Align::Right,
                );
            }
        }
    });
    pen.fill(f.strip, pen.c.surface_1);
    if !roll.lanes_folded {
        pen.icon("plus", f.plus, pen.c.ink);
    }
    pen.icon(
        if roll.lanes_folded {
            "chevron-up"
        } else {
            "chevron-down"
        },
        f.fold,
        pen.c.ink,
    );
    crate::draw::toggle_button(pen, studio, f.follow, "follow", studio.follow.on, false);
    super::zoom::paint(pen, roll, &f);
    if !roll.lanes_folded {
        for (i, r) in f.tabs.iter().enumerate() {
            pen.mask(*r, |pen| {
                let ink = super::lanemark::tab(pen, *r, i == roll.active_lane);
                pen.text(
                    &roll.lanes[i].kind.name(),
                    FontSize::Caption,
                    Weight::Medium,
                    ink,
                    *r,
                    Align::Center,
                );
            });
        }
    }
    for lb in &f.lanes {
        let kind = roll.lanes[lb.index].kind;
        super::lanemark::head(pen, lb, roll.active_lane == lb.index);
        pen.fill(lb.plot, pen.c.surface_0);
        pen.icon(
            if lb.folded {
                "chevron-right"
            } else {
                "chevron-down"
            },
            lb.fold,
            pen.c.ink,
        );
        label(
            pen,
            &kind.name(),
            Rect::new(lb.head.x + 17.0, lb.head.y, lb.head.w - 17.0, lb.head.h),
        );
        if lb.folded {
            continue;
        }
        pen.mask(lb.plot, |pen| {
            crate::grid_lines::paint(pen, &lines, lb.plot.y, lb.plot.h, |t| v.x(t));
            match kind {
                LaneKind::Velocity => {
                    let mut painted = std::collections::HashSet::new();
                    for n in roll.visible_notes(&ctx, &v) {
                        let nview = roll.shown(n);
                        let x = v.x(nview.start);
                        if x < lb.plot.x || x > lb.plot.right() {
                            continue;
                        }
                        let y = y_of(kind, &lb.plot, i32::from(nview.velocity));
                        if !painted.insert((x.to_bits(), y.to_bits(), selected(&n.id))) {
                            continue;
                        }
                        let col = note_face(selected(&n.id), color, pen.c);
                        // design-lint: allow(one-sided-edge) ベロシティの棒は、ノートの値を表す線（面の端の帯ではない）
                        pen.fill(Rect::new(x, y, 2.0, lb.plot.bottom() - y - 3.0), col);
                        pen.round(Rect::new(x - 2.0, y - 2.0, 6.0, 4.0), col, Radius::Xs);
                    }
                    if let Some(h) = roll.velocity_handles(&ctx, &v, &lb.plot) {
                        let mut line = pen.c.ring;
                        line.a = 0.5;
                        let f = h.frame;
                        pen.fill(Rect::new(f.x, f.y, f.w, 1.0), line);
                        pen.fill(Rect::new(f.x, f.y, 1.0, f.h), line);
                        pen.fill(Rect::new(f.right() - 1.0, f.y, 1.0, f.h), line);
                        for handle in std::iter::once(h.scale).chain(h.tilt) {
                            pen.round(handle, pen.c.ring, Radius::Xs);
                        }
                    }
                }
                LaneKind::Ctrl(_) => {
                    let pts = roll.shown_points(&ctx, lb.index);
                    super::lanemark::under(pen, roll, &ctx, &v, lb, &pts, color);
                    for (i, p) in pts.iter().enumerate() {
                        let x = v.x(p.tick);
                        let y = y_of(kind, &lb.plot, p.value);
                        if let Some(next) = pts.get(i + 1) {
                            let to = (v.x(next.tick), y_of(kind, &lb.plot, next.value));
                            if super::lanemark::is_step(kind) || p.shape == CtrlShape::Step {
                                super::lanemark::step(pen, &lb.plot, (x, y), to, color);
                            } else {
                                segment(pen, (x, y), to, color, lb.plot);
                            }
                        }
                        if x >= lb.plot.x && x <= lb.plot.right() {
                            pen.round(
                                Rect::new(x - 2.0, y - 2.0, 4.0, 4.0),
                                note_face(roll.points.contains(&p.tick), color, pen.c),
                                Radius::Xs,
                            );
                        }
                    }
                }
            }
        });
    }
    if let Some((a, b)) = roll.preview.band {
        let mut tint = pen.c.ring;
        tint.a = 0.2;
        pen.fill(
            Rect::new(a.0, a.1, (b.0 - a.0).abs(), (b.1 - a.1).abs()),
            tint,
        );
    }
    if roll.menu.is_some() {
        let entries = roll.menu_entries();
        let r = roll.menu_rect(&f, entries.len());
        pen.floating(r, pen.c.surface_3, Radius::S);
        for (i, (name, _)) in entries.iter().enumerate() {
            label(pen, name, roll.menu_item_rect(r, i));
        }
    }
    if let Some(tip) = studio.roll_balloon() {
        let w = (tip.text.chars().count() as f32 * 9.0 + 16.0).min(body.w);
        let r = Rect::new(
            (tip.at.0 as f32).clamp(body.x, (body.right() - w).max(body.x)),
            (tip.at.1 as f32).max(body.y),
            w,
            24.0,
        );
        pen.floating(r, pen.c.surface_3, Radius::S);
        label(pen, &tip.text, Rect::new(r.x + 8.0, r.y, r.w - 16.0, r.h));
    }
}

/// The grid's lines across the roll, found in song time (they count from the
/// start of each bar) and given from the clip's head.
pub fn grid_lines(roll: &Roll, ctx: &Ctx) -> Vec<crate::grid_lines::Line> {
    let v = roll.view(ctx);
    let start = ctx.clip.start;
    crate::grid_lines::lines(
        &ctx.tools.snap,
        &ctx.meter(roll.beat_px),
        v.tick(v.grid.x) + start,
        v.tick(v.grid.right()) + start,
    )
    .into_iter()
    .map(|l| crate::grid_lines::Line {
        tick: l.tick - start,
        ..l
    })
    .collect()
}

fn label(pen: &mut Pen, text: &str, r: Rect) {
    pen.mask(r, |pen| {
        pen.text(
            text,
            FontSize::Caption,
            Weight::Medium,
            pen.c.ink,
            r,
            Align::Left,
        );
    });
}

/// What a note, its velocity stem or a controller point is painted with: the track's colour,
/// or the strong ink (white in the dark theme, dark navy in the light one) while it is selected,
/// so a selection reads whatever colour the track has.
pub(crate) fn note_face(selected: bool, track: Hsla, c: &crate::paint::Colors) -> Hsla {
    if selected { c.ink_strong } else { track }
}

/// A note's body: a rounded bar, or a diamond in the drum view.
pub(super) fn blob(pen: &mut Pen, v: &super::frame::View, r: Rect, color: Hsla) {
    if v.drums() {
        pen.diamond(r, color);
    } else {
        pen.round(r, color, Radius::Xs);
    }
}

pub(super) fn note(pen: &mut Pen, v: &super::frame::View, n: NoteView, color: Hsla) {
    let r = super::notes::note_rect(v, &n);
    if r.right() < v.grid.x
        || r.x >= v.grid.right()
        || r.bottom() < v.grid.y
        || r.y >= v.grid.bottom()
    {
        return;
    }
    blob(
        pen,
        v,
        Rect::new(r.x, r.y + 1.0, r.w, r.h - 2.0),
        if n.muted { pen.c.surface_3 } else { color },
    );
}

fn segment(pen: &mut Pen, a: (f32, f32), b: (f32, f32), color: Hsla, clip: Rect) {
    let left = a.0.max(clip.x);
    let right = b.0.min(clip.right());
    for x in (left as i32..=right as i32).step_by(2) {
        let t = (x as f32 - a.0) / (b.0 - a.0).max(1.0);
        pen.fill(Rect::new(x as f32, a.1 + (b.1 - a.1) * t, 2.0, 1.5), color);
    }
}
