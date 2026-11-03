//! `--script devices`: the windows of the built-in devices (UI-326 to UI-334), each wide and
//! narrow, with its menus, in the order of their names. Run once dark and once with `--light`. Silent; no OS input.

use serde_json::json;
use xirius_core::id::{PluginId, TrackId};

use crate::device::{compressor, delay, dynamics, eq, layout, limiter, reverb};
use crate::instrument::window::Part;
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;

const fn eq_stage(n: u8) -> Act {
    Act::Stage(Play::Laid(play_eq), n)
}

const fn cmp_stage(n: u8) -> Act {
    Act::Stage(Play::Laid(play_compressor), n)
}

const fn del_stage(n: u8) -> Act {
    Act::Stage(Play::Laid(play_delay), n)
}

const fn lim_stage(n: u8) -> Act {
    Act::Stage(Play::Laid(play_limiter), n)
}

const fn rev_stage(n: u8) -> Act {
    Act::Stage(Play::Laid(play_reverb), n)
}

const fn pin_stage(n: u8) -> Act {
    Act::Stage(Play::Laid(play_pin), n)
}

pub const SHOTS: &[Shot] = &[
    // The Compressor as it opens, four seconds of a beat in its history.
    Shot {
        name: "cmp01-wide",
        acts: &[Act::Reset, cmp_stage(0)],
    },
    // Narrow: the history under the curve, the knobs in two rows.
    Shot {
        name: "cmp02-narrow",
        acts: &[Act::Reset, cmp_stage(0), cmp_stage(1)],
    },
    // The Delay as it opens: synced at 1/8 dotted, its repeats and the Color band.
    Shot {
        name: "del01-wide",
        acts: &[Act::Reset, del_stage(0)],
    },
    // Narrow: the knobs under the band.
    Shot {
        name: "del02-narrow",
        acts: &[Act::Reset, del_stage(0), del_stage(1)],
    },
    // The Beats box's menu.
    Shot {
        name: "del03-beats-menu",
        acts: &[Act::Reset, del_stage(0), del_stage(2)],
    },
    // The EQ as it opens: all six bands, the LMF chosen.
    Shot {
        name: "eq01-wide",
        acts: &[Act::Reset, eq_stage(0)],
    },
    // Narrow: one band at a time under the tabs, the HF chosen.
    Shot {
        name: "eq02-narrow",
        acts: &[Act::Reset, eq_stage(0), eq_stage(1)],
    },
    // The analyser's menu.
    Shot {
        name: "eq03-spectrum-menu",
        acts: &[Act::Reset, eq_stage(0), eq_stage(2)],
    },
    // A band's type menu, the HMF switched off.
    Shot {
        name: "eq04-type-menu",
        acts: &[Act::Reset, eq_stage(0), eq_stage(3)],
    },
    // The Limiter with its gain at +6 dB, five seconds of a mix in its history.
    Shot {
        name: "lim01-wide",
        acts: &[Act::Reset, lim_stage(0)],
    },
    // Narrow: the meters under the history.
    Shot {
        name: "lim02-narrow",
        acts: &[Act::Reset, lim_stage(0), lim_stage(1)],
    },
    // The ceiling's menu.
    Shot {
        name: "lim03-ceiling-menu",
        acts: &[Act::Reset, lim_stage(0), lim_stage(2)],
    },
    // The EQ pinned, the Compressor in a new window a step on (UI-323).
    Shot {
        name: "pin01-pinned-and-next",
        acts: &[Act::Reset, pin_stage(0), pin_stage(1)],
    },
    // The Reverb as it opens: how its sound dies away, the Color band and the six knobs.
    Shot {
        name: "rev01-wide",
        acts: &[Act::Reset, rev_stage(0)],
    },
    // Narrow: a lower plane, the knobs in two rows.
    Shot {
        name: "rev02-narrow",
        acts: &[Act::Reset, rev_stage(0), rev_stage(1)],
    },
    // The tail's menu of decays.
    Shot {
        name: "rev03-decay-menu",
        acts: &[Act::Reset, rev_stage(0), rev_stage(2)],
    },
];

/// The track the devices are put on.
fn keys(s: &Studio) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .map(|t| t.id)
        .expect("the sample song has Keys")
}

/// Puts the built-in effect `name` on Keys and opens its window at the top left of the arrange.
pub(crate) fn open_device(s: &mut Studio, name: &str) -> Option<PluginId> {
    s.ctx = None;
    s.popup = None;
    let t = keys(s);
    s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": name }),
    );
    let id = s.project().tracks[&t]
        .inserts
        .iter()
        .rfind(|x| x.name == name)?
        .id;
    s.show_instrument(t, id);
    // The shots show the window settled, its frame no longer lit (UI-323).
    s.instruments.flash = None;
    Some(id)
}

fn width(s: &mut Studio, id: PluginId, w: f32) {
    let p = s.project();
    let Some(def) = p
        .tracks
        .values()
        .flat_map(|t| &t.inserts)
        .find(|x| x.id == id)
        .and_then(|x| x.builtin())
    else {
        return;
    };
    if let Some(win) = s.instruments.open.iter_mut().find(|w| w.plugin == id) {
        win.body = layout::size(def, w);
        win.placed_w = w;
    }
}

fn press(s: &mut Studio, g: &Geometry, id: PluginId, part: Part) {
    if let Some((x, y)) = s.instrument_center(g, id, part) {
        s.instrument_press(g, x, y, 1, Mods::NONE);
        s.instrument_up();
    }
}

fn param(s: &Studio, id: PluginId, name: &str) -> Option<usize> {
    let p = s.project();
    let x = p
        .tracks
        .values()
        .flat_map(|t| &t.inserts)
        .find(|x| x.id == id)?;
    Some(x.builtin()?.param(name)?.0)
}

fn play_eq(s: &mut Studio, g: &Geometry, n: u8) {
    let g = &s.geometry(g.size.0, g.size.1);
    if n == 0 {
        open_device(s, "EQ");
        return;
    }
    let Some(id) = s.instruments.open.last().map(|w| w.plugin) else {
        return;
    };
    match n {
        1 => {
            if let Some(k) = param(s, id, "high_freq") {
                press(s, g, id, Part::Knob(k));
            }
            width(s, id, eq::MIN_W + 60.0);
        }
        2 => press(s, g, id, Part::Spectrum),
        3 => {
            if let Some(k) = param(s, id, "b4_on") {
                press(s, g, id, Part::Knob(k));
            }
            if let Some(k) = param(s, id, "b3_type") {
                press(s, g, id, Part::Knob(k));
            }
        }
        _ => {}
    }
}

/// Four seconds of a beat through the Compressor `id` as it is set: the
/// shots are silent, so its history is laid in by hand.
fn hear_a_beat(s: &mut Studio, id: PluginId) {
    let p = s.project();
    let Some(x) = p
        .tracks
        .values()
        .flat_map(|t| &t.inserts)
        .find(|x| x.id == id)
    else {
        return;
    };
    let Some(def) = x.builtin() else {
        return;
    };
    let knobs = compressor::shape(def, &crate::instrument::window::values(x));
    let t0 = std::time::Instant::now();
    let d = s.instruments.dynamics.entry(id).or_default();
    let steps = (compressor::SPAN.as_millis() / dynamics::STEP.as_millis()) as u32;
    for i in 0..=steps {
        // A hit each half second, falling away, over a bed at -30 dB.
        let beat = (i % 20) as f32;
        let level = (-6.0 - beat * 1.6).max(-30.0) as f64;
        let gr = (level - compressor::out_db(level, knobs)) as f32;
        let lin = |db: f64| 10f32.powf(db as f32 / 20.0);
        let out = lin(level - f64::from(gr));
        d.feed(
            ([lin(level); 2], [out; 2]),
            gr,
            t0 + dynamics::STEP * i,
            compressor::SPAN,
        );
    }
}

fn play_compressor(s: &mut Studio, _: &Geometry, n: u8) {
    if n == 0 {
        if let Some(id) = open_device(s, "Compressor") {
            hear_a_beat(s, id);
        }
        return;
    }
    let Some(id) = s.instruments.open.last().map(|w| w.plugin) else {
        return;
    };
    if n == 1 {
        width(s, id, compressor::MIN_W + 40.0);
    }
}

/// Five seconds of a mix through the Limiter `id` as it is set: a hit each
/// half second over a moving bed, the gain added, what is over the ceiling
/// taken off at once and given back over the release.
fn hear_a_mix(s: &mut Studio, id: PluginId) {
    let p = s.project();
    let Some(x) = p
        .tracks
        .values()
        .flat_map(|t| &t.inserts)
        .find(|x| x.id == id)
    else {
        return;
    };
    let Some(def) = x.builtin() else {
        return;
    };
    let values = crate::instrument::window::values(x);
    let at = |name: &str| {
        def.param(name)
            .map_or(0.0, |(k, spec)| spec.to_plain(values[k]) as f32)
    };
    let (gain, ceiling, release) = (at("input"), at("ceiling"), at("release"));
    let t0 = std::time::Instant::now();
    let d = s.instruments.dynamics.entry(id).or_default();
    let steps = (limiter::SPAN.as_millis() / dynamics::STEP.as_millis()) as u32;
    let fall = (-(dynamics::STEP.as_secs_f32() * 1000.0) / release).exp();
    let mut gr = 0.0f32;
    for i in 0..=steps {
        let beat = (i % 20) as f32;
        let bed = -12.0 + 2.0 * ((i as f32) * 0.7).sin() + ((i as f32) * 2.3).sin();
        let hit = if beat < 1.0 {
            -2.0 - (i % 3) as f32
        } else {
            -40.0
        };
        let level = bed.max(hit) + gain;
        let need = (level - ceiling).max(0.0);
        gr = if need > gr {
            need
        } else {
            need + (gr - need) * fall
        };
        let lin = |db: f32| 10f32.powf(db / 20.0);
        d.feed(
            ([lin(level); 2], [lin(level - gr); 2]),
            gr,
            t0 + dynamics::STEP * i,
            limiter::SPAN,
        );
    }
}

fn play_delay(s: &mut Studio, g: &Geometry, n: u8) {
    let g = &s.geometry(g.size.0, g.size.1);
    if n == 0 {
        open_device(s, "Delay");
        return;
    }
    let Some(id) = s.instruments.open.last().map(|w| w.plugin) else {
        return;
    };
    match n {
        1 => width(s, id, delay::MIN_W + 60.0),
        2 => {
            let k = xirius_params::DELAY.param("division").map(|(k, _)| k);
            if let Some((x, y)) = k.and_then(|k| s.instrument_center(g, id, Part::Knob(k))) {
                s.instrument_press(g, x, y, 1, Mods::NONE);
                s.instrument_up();
            }
        }
        _ => {}
    }
}

fn play_limiter(s: &mut Studio, g: &Geometry, n: u8) {
    let g = &s.geometry(g.size.0, g.size.1);
    if n == 0 {
        if let Some(id) = open_device(s, "Limiter") {
            let six = xirius_params::LIMITER
                .param("input")
                .map(|(k, spec)| (k, spec.to_norm(6.0)));
            if let Some((k, v)) = six {
                s.device_set(id, k, v);
            }
            hear_a_mix(s, id);
        }
        return;
    }
    let Some(id) = s.instruments.open.last().map(|w| w.plugin) else {
        return;
    };
    match n {
        1 => width(s, id, limiter::MIN_W + 60.0),
        2 => {
            if let Some((x, y)) = s.instrument_center(g, id, Part::Point(0, 1)) {
                s.instrument_right(g, x, y);
            }
        }
        _ => {}
    }
}

fn play_pin(s: &mut Studio, g: &Geometry, n: u8) {
    let g = &s.geometry(g.size.0, g.size.1);
    match n {
        0 => {
            if let Some(id) = open_device(s, "EQ") {
                press(s, g, id, Part::Pin);
            }
        }
        1 => {
            open_device(s, "Compressor");
        }
        _ => {}
    }
}

fn play_reverb(s: &mut Studio, g: &Geometry, n: u8) {
    let g = &s.geometry(g.size.0, g.size.1);
    if n == 0 {
        open_device(s, "Reverb");
        return;
    }
    let Some(id) = s.instruments.open.last().map(|w| w.plugin) else {
        return;
    };
    match n {
        1 => width(s, id, reverb::MIN_W + 60.0),
        2 => {
            if let Some((x, y)) = s.instrument_center(g, id, Part::Point(0, 1)) {
                s.instrument_right(g, x, y);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_eq_stages_play_without_a_window() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 1000.0);
        for n in 0..4 {
            assert!(Play::Laid(play_eq).without_window(&mut s, &g, n));
        }
        assert_eq!(s.instruments.open.len(), 1);
        assert!(!s.context_rows().is_empty(), "the type menu is open");
    }

    #[test]
    fn the_compressor_stages_play_without_a_window() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 1000.0);
        for n in 0..2 {
            assert!(Play::Laid(play_compressor).without_window(&mut s, &g, n));
        }
        let id = s.instruments.open[0].plugin;
        assert_eq!(s.instruments.dynamics[&id].history.len(), 160);
    }

    #[test]
    fn the_delay_stages_play_without_a_window() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 1000.0);
        for n in 0..3 {
            assert!(Play::Laid(play_delay).without_window(&mut s, &g, n));
        }
        assert_eq!(s.instruments.open.len(), 1);
        let rows = s.context_rows();
        assert!(rows.iter().any(|r| r.label == "1/8 D"), "the Beats menu");
    }

    #[test]
    fn the_limiter_stages_play_without_a_window() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 1000.0);
        for n in 0..3 {
            assert!(Play::Laid(play_limiter).without_window(&mut s, &g, n));
        }
        let id = s.instruments.open[0].plugin;
        assert_eq!(s.instruments.dynamics[&id].history.len(), 200);
        let p = s.project();
        let x = p
            .tracks
            .values()
            .flat_map(|t| &t.inserts)
            .find(|x| x.id == id)
            .unwrap();
        let (k, spec) = x.builtin().unwrap().param("input").unwrap();
        let v = crate::instrument::window::values(x)[k];
        assert!((spec.to_plain(v) - 6.0).abs() < 1e-6);
        assert!(!s.context_rows().is_empty(), "the ceiling's menu is open");
    }

    #[test]
    fn the_pin_stages_play_without_a_window() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 1000.0);
        for n in 0..2 {
            assert!(Play::Laid(play_pin).without_window(&mut s, &g, n));
        }
        assert_eq!(s.instruments.open.len(), 2);
        assert!(s.instruments.open[0].pinned && !s.instruments.open[1].pinned);
    }

    #[test]
    fn the_reverb_stages_play_without_a_window() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 1000.0);
        for n in 0..3 {
            assert!(Play::Laid(play_reverb).without_window(&mut s, &g, n));
        }
        let rows = s.context_rows();
        assert!(
            rows.iter().any(|r| r.label == "Decay を 4 s に"),
            "the decays"
        );
    }
}
