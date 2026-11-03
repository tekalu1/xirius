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

const fn pre_stage(n: u8) -> Act {
    Act::Stage(Play::Laid(play_presets), n)
}

const fn brw_stage(n: u8) -> Act {
    Act::Stage(Play::Laid(play_browser), n)
}

const fn size_stage(n: u8) -> Act {
    Act::Stage(Play::Laid(play_size), n)
}

pub const SHOTS: &[Shot] = &[
    // The browser's effects with the Reverb's presets open under it, one chosen and a
    // favourite (BRW-017).
    Shot {
        name: "brw01-presets-wide",
        acts: &[Act::Reset, brw_stage(0)],
    },
    // The same in the narrowest browser.
    Shot {
        name: "brw02-presets-narrow",
        acts: &[Act::Reset, brw_stage(0), brw_stage(1)],
    },
    // A preset held over a track: the drop's label.
    Shot {
        name: "brw03-preset-drag",
        acts: &[Act::Reset, brw_stage(0), brw_stage(2)],
    },
    // A search finds a preset under its device.
    Shot {
        name: "brw04-preset-search",
        acts: &[Act::Reset, brw_stage(0), brw_stage(3)],
    },
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
    // The Compressor at a saved preset, a knob moved off it: the tag's mark (UI-332).
    Shot {
        name: "pre01-tag-marked",
        acts: &[Act::Reset, pre_stage(0)],
    },
    // Its list: Default first, then by name, the ✓ on the preset it moved off, the saves
    // under them.
    Shot {
        name: "pre02-list",
        acts: &[Act::Reset, pre_stage(0), pre_stage(1)],
    },
    // The same list from the narrow window.
    Shot {
        name: "pre03-list-narrow",
        acts: &[Act::Reset, pre_stage(0), pre_stage(2), pre_stage(1)],
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
    // The EQ's window made bigger by its corner: the curve grows, the
    // knobs keep their size, the grip shows under the pointer.
    Shot {
        name: "size01-eq-grown",
        acts: &[Act::Reset, size_stage(0)],
    },
    // The Compressor's: its history and meters grow, the curve stays square.
    Shot {
        name: "size02-compressor-grown",
        acts: &[Act::Reset, size_stage(1)],
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

/// Drags the corner of `id`'s window by `(dx, dy)` and leaves the pointer
/// on it.
fn grow(s: &mut Studio, g: &Geometry, id: PluginId, dx: f32, dy: f32) {
    if let Some((x, y)) = s.instrument_center(g, id, Part::Resize) {
        s.instrument_press(g, x, y, 1, Mods::NONE);
        s.instrument_moved(g, x + dx, y + dy, false);
        s.instrument_up();
        s.instrument_hover(g, x + dx, y + dy);
    }
}

fn play_size(s: &mut Studio, g: &Geometry, n: u8) {
    let g = &s.geometry(g.size.0, g.size.1);
    match n {
        0 => {
            if let Some(id) = open_device(s, "EQ") {
                grow(s, g, id, 240.0, 160.0);
            }
        }
        1 => {
            if let Some(id) = open_device(s, "Compressor") {
                hear_a_beat(s, id);
                grow(s, g, id, 200.0, 140.0);
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

/// Waits for a command the window gave to a worker (the preset files') to finish.
fn settle(s: &mut Studio) {
    for _ in 0..3000 {
        if s.saving.is_none() {
            return;
        }
        s.poll_saving();
        s.poll_saving_transport();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// The shots' own folder of presets has the factory set, and 「いつもの」 saved from the
/// Compressor before its threshold moved.
fn play_presets(s: &mut Studio, g: &Geometry, n: u8) {
    let g = &s.geometry(g.size.0, g.size.1);
    match n {
        0 => {
            s.run_command("preset.factory.restore", json!({}));
            settle(s);
            let Some(id) = open_device(s, "Compressor") else {
                return;
            };
            s.save_preset(id, "いつもの", true);
            settle(s);
            if let Some(k) = param(s, id, "threshold") {
                let p = s.project();
                let x = p
                    .tracks
                    .values()
                    .flat_map(|t| &t.inserts)
                    .find(|x| x.id == id);
                if let Some((x, spec)) = x.and_then(|x| Some((x, x.builtin()?.params.get(k)?))) {
                    let v = x
                        .params
                        .get(spec.id)
                        .copied()
                        .unwrap_or(spec.default_norm());
                    s.run_command(
                        "plugin.param.set",
                        json!({ "id": id.to_string(), "param": spec.id, "value": (v - 0.12).max(0.0) }),
                    );
                }
            }
            s.refresh_presets();
        }
        1 => {
            if let Some(id) = s.instruments.open.last().map(|w| w.plugin) {
                s.popup = None;
                if let Some((x, y)) = s.instrument_center(g, id, Part::Preset) {
                    s.instrument_down(g, x, y);
                }
            }
        }
        2 => {
            if let Some(id) = s.instruments.open.last().map(|w| w.plugin) {
                width(s, id, compressor::MIN_W + 40.0);
            }
        }
        _ => {}
    }
}

/// The browser on the effects, the factory presets read, the Reverb's open with ホール
/// chosen and a favourite, Keys the chosen track.
fn play_browser(s: &mut Studio, g: &Geometry, n: u8) {
    use crate::browser::{BRow, BrowserHit, RailTab, preset_key};
    let g = &s.geometry(g.size.0, g.size.1);
    let reverb = xirius_params::find("Reverb").map_or("", |d| d.uid);
    match n {
        0 => {
            s.run_command("preset.factory.restore", json!({}));
            settle(s);
            // The window's read is a worker's: wait for its answer.
            for _ in 0..300 {
                s.refresh_presets();
                if s.browser.presets.contains_key(reverb) {
                    break;
                }
                settle(s);
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let t = keys(s);
            s.select_row(Some(crate::tree::RowKind::Track(t)));
            if !(s.browser.open && s.browser.tab == RailTab::Effects) {
                s.browser.press_rail(RailTab::Effects);
            }
            s.browser.toggle_device("Reverb");
            let key = preset_key(reverb, "ホール");
            if !s.browser.favourites.contains(&key) {
                s.browser.toggle_favourite(&key);
            }
            s.browser.selected_item = None;
            s.browser.selected_preset = Some((reverb, "ホール".to_string()));
            s.mixer.browser_fx_focus = true;
        }
        1 => {
            s.side_size.browser = Some(crate::side_edge::BROWSER_MIN_W);
        }
        2 => {
            let l = s.browser.layout(g.panel);
            let Some(i) = l
                .rows
                .iter()
                .position(|p| matches!(&p.row, BRow::Preset { name, .. } if name == "小さい部屋"))
            else {
                return;
            };
            let hit = BrowserHit::Preset(i);
            let Some((x, y)) = s.browser.center_of(&l, hit) else {
                return;
            };
            let bass = s
                .project()
                .tracks
                .values()
                .find(|t| t.name == "Bass")
                .map(|t| t.id);
            let Some(row) = bass.and_then(|t| s.tree().row_of(crate::tree::RowKind::Track(t)))
            else {
                return;
            };
            let to = (
                g.lanes.x + g.lanes.w * 0.4,
                g.column.y + row.y - s.scroll.1 + row.h / 2.0,
            );
            let hit = crate::studio::Hit::Browser(hit);
            s.press(g, hit);
            s.lib_grab(g, hit, x, y, 1);
            s.lib_move(to.0, to.1);
        }
        3 => {
            s.browser.search = "ホール".into();
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
    fn the_preset_stages_play_without_a_window() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 1000.0);
        for n in [0, 2, 1] {
            assert!(Play::Laid(play_presets).without_window(&mut s, &g, n));
        }
        let id = s.instruments.open[0].plugin;
        let x = s
            .project()
            .tracks
            .values()
            .flat_map(|t| &t.inserts)
            .find(|x| x.id == id)
            .cloned()
            .unwrap();
        assert_eq!(s.instruments.tag_of(&x), ("いつもの".to_string(), true));
        assert!(s.popup.is_some(), "the list is open");
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
    fn the_size_stages_play_without_a_window() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 1000.0);
        assert!(Play::Laid(play_size).without_window(&mut s, &g, 0));
        let natural = layout::size(xirius_params::find("eq").unwrap(), eq::NATURAL_W);
        let body = s.instruments.open[0].body;
        assert_eq!(body, (natural.0 + 240.0, natural.1 + 160.0));
        assert!(s.instruments.sizing(), "the grip shows");
    }

    #[test]
    fn the_browser_stages_play_without_a_window() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 1000.0);
        for n in 0..3 {
            assert!(Play::Laid(play_browser).without_window(&mut s, &g, n));
        }
        assert!(s.browser.open_devices.contains("Reverb"));
        assert!(
            s.browser
                .drag
                .as_ref()
                .is_some_and(|d| d.moved && d.preset.is_some()),
            "a preset is held"
        );
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
