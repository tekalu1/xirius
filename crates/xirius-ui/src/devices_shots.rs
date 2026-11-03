//! `--script devices`: the windows of the built-in devices (UI-326 to UI-334), each wide and
//! narrow, with its menus. Run once dark and once with `--light`. Silent; no OS input.

use serde_json::json;
use xirius_core::id::{PluginId, TrackId};

use crate::device::{eq, layout};
use crate::instrument::window::Part;
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;

const fn eq_stage(n: u8) -> Act {
    Act::Stage(Play::Laid(play_eq), n)
}

pub const SHOTS: &[Shot] = &[
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
}
