//! `--script devices`: the windows of the built-in devices (UI-326 to UI-334), each wide and
//! narrow, with its menus, in the order of their names. Run once dark and once with `--light`. Silent; no OS input.

use serde_json::json;
use xirius_core::id::{PluginId, TrackId};

use crate::device::{compressor, dynamics, eq, layout};
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
}
