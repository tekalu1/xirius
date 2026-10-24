//! Painting of the mixer (mock v4's strips). It is painted over the cached
//! body every frame, so the meters can move at the screen's rate.

use std::f32::consts::PI;
use std::time::Instant;

use gpui::{Hsla, PathBuilder, point, px};
use xirius_core::model::{Project, Track, TrackKind};
use xirius_design::{FontSize, Radius, Weight};

use super::meters::{FLOOR_DB, Meter, db_to_t};
use super::{List, MixerHit, StripLayout, list_row, section_icon, send_parts};
use crate::draw::{Align, Pen, output_label, track_icon};
use crate::paint::METER_RAMP;
use crate::studio::{Geometry, Hit, Rect, Studio};
use crate::tree::RowKind;

/// A knob's sweep: from the lower left round to the lower right.
const SWEEP_FROM: f32 = 0.75 * PI;
const SWEEP: f32 = 1.5 * PI;

pub fn paint(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    let started = Instant::now();
    let body = g.drawer_body;
    let p = studio.project();
    let c = pen.c;
    pen.fill(body, c.surface_0);
    let hover = match studio.hover {
        Hit::Mixer(h) => Some(h),
        _ => None,
    };
    let view = studio.mixer_view(g);
    for s in studio.mixer_strips(g) {
        let Some(track) = p.tracks.get(&s.track) else {
            continue;
        };
        // A channel is cut at the view's edge, where the master takes over.
        let shown = if s.track.is_master() {
            s.rect
        } else {
            let right = s.rect.right().min(view.right());
            let left = s.rect.x.max(view.x);
            Rect::new(left, s.rect.y, (right - left).max(0.0), s.rect.h)
        };
        pen.mask(shown, |pen| paint_strip(pen, studio, &p, track, &s, hover));
    }
    if let Some((_, thumb)) = studio.mixer_bar(g) {
        // The scroll thumb's tones (the grid's two stronger tones are the scroll thumb's).
        let tone = if studio.mixer.bar_grab.is_some() {
            c.grid[0]
        } else {
            c.grid[1]
        };
        pen.round(thumb, tone, Radius::Pill);
    }
    studio
        .mixer
        .paint_us
        .set(started.elapsed().as_micros() as u64);
}

fn paint_strip(
    pen: &mut Pen,
    studio: &Studio,
    p: &Project,
    track: &Track,
    s: &StripLayout,
    hover: Option<MixerHit>,
) {
    let c = pen.c;
    let t = track.id;
    // The strips of the selected tracks share the heads' face (UI-340).
    let selected = !t.is_master() && studio.is_picked(RowKind::Track(t));
    pen.fill(s.rect, if selected { c.track_sel } else { c.surface_1 });
    let lit = studio.mixer.flash_level(t, xirius_core::store::now_ms());
    if lit > 0.0 {
        pen.fill(s.rect, c.strip_flash.opacity(lit));
    }

    // The name, with the track's icon in its colour.
    let ink = if selected { c.ink_strong } else { c.ink };
    let icon_ink = match track.kind {
        TrackKind::Audio | TrackKind::Instrument => crate::draw::color_of(c, track),
        _ => c.ink,
    };
    let icon_r = Rect::new(s.name.x, s.name.y + (s.name.h - 12.0) / 2.0, 12.0, 12.0);
    pen.icon(track_icon(track), icon_r, icon_ink);
    // A frozen track's name is followed by the snowflake (EXP-004).
    let flake = if track.frozen.is_some() {
        12.0 + 4.0
    } else {
        0.0
    };
    let name_r = Rect::new(
        icon_r.right() + 5.0,
        s.name.y,
        (s.name.right() - icon_r.right() - 5.0 - flake).max(0.0),
        s.name.h,
    );
    if flake > 0.0 {
        let w = crate::browser::text_width(&track.name, FontSize::Small).min(name_r.w);
        pen.icon(
            "freeze",
            Rect::new(name_r.x + w + 4.0, icon_r.y, 12.0, 12.0),
            c.ring,
        );
    }
    pen.mask(name_r, |pen| {
        pen.text(
            track.shown_name(),
            FontSize::Small,
            Weight::Medium,
            ink,
            name_r,
            Align::Left,
        );
    });

    if let Some(input) = s.input {
        pen.icon("input", section_icon(input), c.ink);
        // A strip is narrow: the default names its channels (an instrument,
        // every MIDI port), and the list marks it as the default.
        crate::dropdown::field(
            pen,
            input,
            crate::input_row::label(track),
            crate::dropdown::Face::Well,
            hover == Some(MixerHit::Input(t)),
            true,
        );
    }

    // Inserts, then the ＋ row.
    pen.icon("effects", section_icon(list_row(s.inserts, 0)), c.ink);
    crate::fx_chain::paint(pen, studio, track, s.inserts, s.rows.inserts);

    // Pan, M, S, arm.
    pen.icon("volume", section_icon(s.level_row()), c.ink);
    if let Some(r) = s.pan {
        let v = studio.mixer_value(MixerHit::Pan(t)).unwrap_or(track.pan) as f32;
        knob(pen, r, 0.5 + v / 2.0, 0.5, 3.0, true);
        crate::auto_mark::paint(pen, track, xirius_core::auto::AutoTarget::Pan, r);
    }
    for (r, label, on, lit) in [
        (s.mute, "M", track.mute, c.mute_on),
        (s.solo, "S", track.solo, c.solo_on),
        (s.arm, "arm", track.armed, c.arm_on),
    ] {
        let Some(r) = r else {
            continue;
        };
        let (face, ink) = if on {
            lit
        } else {
            (c.surface_3, c.ink_on_toggle)
        };
        let face = pen.transition(
            &studio.motion,
            studio.ai.motion,
            r,
            &format!("mixer-{label}-{t}"),
            on,
            face,
            lit.0,
        );
        pen.round(r, face, Radius::Clip);
        if label == "arm" {
            let i = Rect::new(
                r.x + (r.w - 14.0) / 2.0,
                r.y + (r.h - 14.0) / 2.0,
                14.0,
                14.0,
            );
            pen.icon("record", i, ink);
        } else {
            pen.text(
                label,
                FontSize::Caption,
                Weight::Semibold,
                ink,
                r,
                Align::Center,
            );
        }
    }

    // The fader, its meters and its level.
    let db = studio
        .mixer_value(MixerHit::Fader(t))
        .unwrap_or(track.volume_db);
    fader(pen, s, db, hover == Some(MixerHit::Fader(t)));
    crate::auto_mark::paint(pen, track, xirius_core::auto::AutoTarget::Volume, s.fader);
    crate::midi_assign::paint_strip(pen, studio, t, s.pan, s.fader);
    let m = studio.mixer.meters.get(s.strip());
    for (ch, r) in s.meter.iter().enumerate() {
        meter(pen, *r, &m, ch);
    }
    let text = if db as f32 <= FLOOR_DB {
        "-∞".to_string()
    } else {
        format!("{db:+.1}")
    };
    pen.digits(
        &text,
        FontSize::Caption,
        Weight::Medium,
        c.ink_strong,
        s.db,
        Align::Center,
    );

    // Sends, then the ＋ row.
    if let Some(list) = s.sends {
        pen.icon("send", section_icon(list_row(list, 0)), c.ink);
        let off = studio.mixer.offset(t, List::Sends);
        for i in 0..s.rows.sends {
            let k = off + i;
            let row = list_row(list, i);
            if let Some(send) = track.sends.get(k) {
                pen.round(row, c.surface_2, Radius::S);
                let (nm, kr, pp) = send_parts(row);
                let to = p.tracks.get(&send.to).map_or("?", |b| b.shown_name());
                let nm = Rect::new(nm.x + 6.0, nm.y, nm.w - 6.0, nm.h);
                pen.mask(nm, |pen| {
                    pen.text(
                        to,
                        FontSize::Caption,
                        Weight::Regular,
                        c.ink_strong,
                        nm,
                        Align::Left,
                    );
                });
                let level = studio
                    .mixer_value(MixerHit::SendLevel(t, send.id))
                    .unwrap_or_else(|| super::send_level_db(send));
                knob(pen, kr, db_to_t(level as f32), 0.0, 3.0, false);
                crate::auto_mark::paint(
                    pen,
                    track,
                    xirius_core::auto::AutoTarget::Send(send.id),
                    kr,
                );
                if hover == Some(MixerHit::SendPre(t, send.id)) {
                    pen.round(pp, c.surface_3, Radius::Xs);
                }
                pre_glyph(pen, pp, if send.pre { c.ink_blue } else { c.ink });
            } else if k == track.sends.len() {
                add_row(pen, row, hover == Some(MixerHit::AddSend(t)));
            }
        }
    }

    // Where the strip plays to.
    if let Some(out) = s.output {
        pen.icon("output", section_icon(out), c.ink);
        // A strip is narrow: the default names where it goes, and the list
        // marks it as the default.
        let label = if track.kind == TrackKind::Master {
            let pair = studio
                .song
                .io
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .view
                .output;
            format!("Out {}+{}", pair[0] + 1, pair[1] + 1)
        } else {
            output_label(p, track)
        };
        crate::dropdown::field(
            pen,
            out,
            &label,
            crate::dropdown::Face::Well,
            hover == Some(MixerHit::Output(t)),
            true,
        );
    }
}

fn add_row(pen: &mut Pen, r: Rect, hot: bool) {
    let c = pen.c;
    pen.round(r, if hot { c.surface_3 } else { c.surface_1 }, Radius::S);
    let i = Rect::new(
        r.x + (r.w - 12.0) / 2.0,
        r.y + (r.h - 12.0) / 2.0,
        12.0,
        12.0,
    );
    pen.icon("plus", i, c.ink);
}

fn fader(pen: &mut Pen, s: &StripLayout, db: f64, hot: bool) {
    let c = pen.c;
    let f = s.fader;
    let half = super::CAP_H / 2.0;
    pen.round(
        Rect::new(
            f.x + (f.w - 4.0) / 2.0,
            f.y + half,
            4.0,
            (f.h - super::CAP_H).max(0.0),
        ),
        c.well,
        Radius::Xs,
    );
    let unity = s.cap_y(0.0);
    pen.fill(Rect::new(f.x + 2.0, unity, f.w - 4.0, 1.0), c.line[0]);
    let y = s.cap_y(db);
    let cap = Rect::new(f.x, y - half, f.w, super::CAP_H);
    // Under the pointer the cap wears the primary hover face and its grip the ink on it (UI-309).
    let (face, grip) = if hot {
        c.primary_hover
    } else {
        (c.thumb, c.ink_strong)
    };
    pen.round(cap, face, Radius::Clip);
    pen.round(
        Rect::new(cap.x + 4.0, cap.y + 5.0, cap.w - 8.0, 2.0),
        grip,
        Radius::Xs,
    );
}

/// One channel of a strip's meter: the level in the meter's colours, the
/// held peak as a line, the clip light as a ring.
pub(crate) fn meter(pen: &mut Pen, r: Rect, m: &Meter, ch: usize) {
    let c = pen.c;
    pen.round(r, c.well, Radius::Clip);
    if r.h <= 0.0 {
        return;
    }
    pen.mask(r, |pen| {
        let level = m.level[ch].clamp(0.0, 1.0);
        // The colour at each height is the meter's colour at that point of
        // the scale, so a quiet level stays green and a loud one reaches red.
        let step = r.h / METER_RAMP as f32;
        let top = r.bottom() - level * r.h;
        for i in 0..METER_RAMP {
            let y1 = r.bottom() - i as f32 * step;
            let y0 = (y1 - step).max(top);
            if y0 >= y1 {
                break;
            }
            pen.fill(Rect::new(r.x, y0, r.w, y1 - y0), c.meter_ramp[i]);
        }
        let rms = m.rms[ch].clamp(0.0, 1.0);
        if rms > 0.0 {
            pen.fill(
                Rect::new(r.x, r.bottom() - rms * r.h, r.w, 1.0),
                c.ink_strong,
            );
        }
        let hold = m.hold[ch].clamp(0.0, 1.0);
        if hold > 0.0 {
            let i = ((hold * (METER_RAMP - 1) as f32).round() as usize).min(METER_RAMP - 1);
            pen.fill(
                Rect::new(r.x, r.bottom() - hold * r.h, r.w, 2.0),
                c.meter_ramp[i],
            );
        }
        if m.clipped(ch) {
            pen.fill(Rect::new(r.x, r.y, r.w, 3.0), c.over);
        }
    });
}

/// A knob: the track of its sweep, the value's arc from `from` (0..1 of the
/// sweep) to `v`, and a pointer.
pub(crate) fn knob(pen: &mut Pen, r: Rect, v: f32, from: f32, width: f32, disc: bool) {
    let c = pen.c;
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    let rad = r.w.min(r.h) / 2.0 - width / 2.0 - 0.5;
    let v = v.clamp(0.0, 1.0);
    arc(pen, (cx, cy), rad, 0.0, 1.0, width, c.well);
    if (v - from).abs() > 0.002 {
        arc(
            pen,
            (cx, cy),
            rad,
            from.min(v),
            from.max(v),
            width,
            c.ink_blue,
        );
    }
    if disc {
        let d = rad - width;
        pen.round(
            Rect::new(cx - d, cy - d, d * 2.0, d * 2.0),
            c.surface_3,
            Radius::Pill,
        );
    }
    let a = SWEEP_FROM + v * SWEEP;
    let (inner, outer) = if disc {
        (rad * 0.25, rad - width)
    } else {
        (0.0, rad)
    };
    line(
        pen,
        (cx + inner * a.cos(), cy + inner * a.sin()),
        (cx + outer * a.cos(), cy + outer * a.sin()),
        if disc { 1.6 } else { 2.0 },
        c.ink_strong,
    );
}

/// Part of a knob's sweep, from `t0` to `t1` (0..1).
fn arc(pen: &mut Pen, center: (f32, f32), rad: f32, t0: f32, t1: f32, width: f32, color: Hsla) {
    let (ox, oy) = pen.origin;
    let n = ((t1 - t0) * 40.0).ceil().max(1.0) as usize;
    let mut path = PathBuilder::stroke(px(width));
    for i in 0..=n {
        let a = SWEEP_FROM + (t0 + (t1 - t0) * i as f32 / n as f32) * SWEEP;
        let at = point(
            px(ox + center.0 + rad * a.cos()),
            px(oy + center.1 + rad * a.sin()),
        );
        if i == 0 {
            path.move_to(at);
        } else {
            path.line_to(at);
        }
    }
    if let Ok(path) = path.build() {
        pen.window.paint_path(path, color);
    }
}

fn line(pen: &mut Pen, a: (f32, f32), b: (f32, f32), width: f32, color: Hsla) {
    let (ox, oy) = pen.origin;
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(point(px(ox + a.0), px(oy + a.1)));
    path.line_to(point(px(ox + b.0), px(oy + b.1)));
    if let Ok(path) = path.build() {
        pen.window.paint_path(path, color);
    }
}

/// The send's pre/post mark: a dot before a bar (pre) in the mock's glyph.
fn pre_glyph(pen: &mut Pen, r: Rect, color: Hsla) {
    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    pen.round(Rect::new(cx - 5.0, cy - 2.0, 4.0, 4.0), color, Radius::Pill);
    pen.fill(Rect::new(cx + 2.0, cy - 5.0, 1.5, 10.0), color);
}
