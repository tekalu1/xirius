use super::window::{CUSTOM, Part, part_rect, preset_of, preset_values, text, values};
use super::*;
use crate::mixer::MixerHit;
use crate::popup::{Action, PopupKind};
use crate::studio::{DrawerTab, Geometry, Hit, Rect};
use xirius_core::id::PluginId;

/// The sample song with Keys playing the built-in sampler.
fn studio() -> (Studio, Geometry, TrackId, PluginId) {
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    let t = keys(&s);
    assert!(s.set_instrument(t, &Pick::built_in(DEFAULT)));
    let id = current(&s.project().tracks[&t]).unwrap().id;
    (s, g, t, id)
}

fn keys(s: &Studio) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .map(|t| t.id)
        .unwrap()
}

fn plugin(s: &Studio, t: TrackId) -> Plugin {
    current(&s.project().tracks[&t]).cloned().unwrap()
}

/// Presses the keys button at the head of the track, as a click does.
fn press_keys(s: &mut Studio, g: &Geometry, t: TrackId) -> bool {
    let (x, y) = s.center_of(g, Hit::Keys(t)).expect("on screen");
    assert_eq!(s.hit(g, x, y), Hit::Keys(t));
    s.press(g, Hit::Keys(t))
}

fn at(s: &Studio, g: &Geometry, id: PluginId, part: Part) -> (f32, f32) {
    s.instrument_center(g, id, part).expect("in sight")
}

#[test]
fn the_keys_button_opens_the_instruments_window() {
    let (mut s, g, t, id) = studio();
    assert!(press_keys(&mut s, &g, t));
    assert!(s.instruments.is_open(id));
    // The window takes the clicks over it: nothing under it is hit.
    let (x, y) = at(&s, &g, id, Part::Body);
    assert_eq!(s.hit(&g, x, y), Hit::None);
    // Pressing again keeps the one window.
    assert!(press_keys(&mut s, &g, t));
    assert_eq!(s.instruments.open.len(), 1);
    // Its close button shuts it.
    let (x, y) = at(&s, &g, id, Part::Close);
    assert!(s.instrument_down(&g, x, y));
    assert!(!s.instruments.is_open(id));
}

#[test]
fn a_track_without_an_instrument_lists_them_instead() {
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    let t = keys(&s);
    assert!(press_keys(&mut s, &g, t));
    assert_eq!(
        s.popup.as_ref().map(|p| p.kind),
        Some(PopupKind::Instrument(t))
    );
    assert!(s.instruments.open.is_empty());
    // An audio track's head has no keys button to press.
    let vocal = s
        .project()
        .tracks
        .values()
        .find(|x| x.name == "Vocal")
        .unwrap()
        .id;
    if let Some((x, y)) = s.center_of(&g, Hit::Keys(vocal)) {
        assert_ne!(s.hit(&g, x, y), Hit::Keys(vocal));
    }
}

#[test]
fn the_mixer_row_of_a_built_in_instrument_opens_the_same_window() {
    let (mut s, g, t, id) = studio();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    let (x, y) = s
        .mixer_center(&g, MixerHit::Open(t, id))
        .expect("on screen");
    let hit = s.hit(&g, x, y);
    assert_eq!(hit, Hit::Mixer(MixerHit::Open(t, id)));
    s.press(&g, hit);
    assert!(s.instruments.is_open(id));
    assert_eq!(s.mixer.open_editors(), 0, "no window of a plug-in");
    assert!(s.error.is_none());
    // The row's second press shuts it.
    assert!(s.open_plugin(t, id));
    assert!(!s.instruments.is_open(id));
}

#[test]
fn a_knob_turns_while_dragged_and_is_one_step_when_let_go() {
    let (mut s, g, t, id) = studio();
    s.show_instrument(t, id);
    let before = s.song().steps();
    let (cutoff, spec) = plugin(&s, t).builtin().unwrap().param("cutoff").unwrap();
    // It starts open (20 kHz), at the top of its sweep: down closes it.
    assert_eq!(values(&plugin(&s, t))[cutoff], 1.0);
    let (x, y) = at(&s, &g, id, Part::Knob(cutoff));
    assert!(s.instrument_down(&g, x, y));
    assert!(s.instrument_moved(&g, x, y + 30.0, false));
    assert!(s.instrument_moved(&g, x, y + 60.0, false));
    // Nothing goes into the song until it is let go.
    assert_eq!(s.song().steps(), before);
    assert!(s.instrument_up());
    assert_eq!(s.song().steps(), before + 1);
    let now = values(&plugin(&s, t))[cutoff];
    assert!((now - 0.6).abs() < 1e-6, "{now}");
    assert_eq!(plugin(&s, t).params.get("cutoff"), Some(&now), "by its id");
    assert_eq!(text(spec, now), spec.text(spec.to_plain(0.6)));
    assert_eq!(preset_of(&plugin(&s, t)), CUSTOM);
    // A press that does not turn it puts nothing in.
    let (x, y) = at(&s, &g, id, Part::Knob(cutoff));
    assert!(s.instrument_down(&g, x, y));
    assert!(s.instrument_up());
    assert_eq!(s.song().steps(), before + 1);
    // Undo takes the turn back.
    assert!(s.undo(&g));
    assert_eq!(values(&plugin(&s, t))[cutoff], spec.default_norm());
}

#[test]
fn a_stepped_knob_lands_on_its_steps() {
    let (mut s, g, t, id) = studio();
    s.show_instrument(t, id);
    // 音色 is a choice of six: a drag lands on one of them.
    let (sound, spec) = plugin(&s, t).builtin().unwrap().param("sound").unwrap();
    let (x, y) = at(&s, &g, id, Part::Knob(sound));
    assert!(s.instrument_down(&g, x, y));
    assert!(s.instrument_moved(&g, x, y - 40.0, false));
    assert!(s.instrument_up());
    let now = values(&plugin(&s, t))[sound];
    let plain = spec.to_plain(now);
    assert_eq!(plain, plain.round());
    assert!(plain >= 1.0, "{plain}");
    assert_eq!(text(spec, now), spec.choices()[plain as usize]);
}

#[test]
fn a_preset_sets_every_knob_in_one_step() {
    let (mut s, g, t, id) = studio();
    s.show_instrument(t, id);
    assert_eq!(preset_of(&plugin(&s, t)), "初期");
    let (x, y) = at(&s, &g, id, Part::Preset);
    assert!(s.instrument_down(&g, x, y));
    let popup = s.popup.as_ref().expect("the presets");
    assert!(matches!(popup.kind, PopupKind::InstMenu(_)));
    let cs = popup.choices();
    let labels: Vec<&str> = cs.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["初期", "やさしく", "強め", "声を前に"]);
    assert_eq!(popup.current(), "初期");
    let before = s.song().steps();
    s.popup_action(&g, Action::Pick("強め".into()));
    assert!(s.popup.is_none());
    assert_eq!(s.song().steps(), before + 1);
    assert_eq!(preset_of(&plugin(&s, t)), "強め");
    assert_eq!(
        values(&plugin(&s, t)),
        preset_values(plugin(&s, t).builtin().unwrap(), "強め").unwrap()
    );
    assert!(s.undo(&g));
    assert_eq!(preset_of(&plugin(&s, t)), "初期");
}

#[test]
fn the_bar_drags_the_window_and_it_stays_in_reach() {
    let (mut s, g, t, id) = studio();
    s.show_instrument(t, id);
    let (x, y) = at(&s, &g, id, Part::Head);
    let before = s.instruments.rect(0, g.size);
    assert!(s.instrument_down(&g, x, y));
    assert!(s.instrument_moved(&g, x + 40.0, y + 25.0, false));
    assert!(s.instrument_up());
    let after = s.instruments.rect(0, g.size);
    assert_eq!((after.x, after.y), (before.x + 40.0, before.y + 25.0));
    // Dragged far off, a strip of it stays on the screen.
    let (x, y) = at(&s, &g, id, Part::Head);
    assert!(s.instrument_down(&g, x, y));
    assert!(s.instrument_moved(&g, x + 5000.0, y + 5000.0, false));
    assert!(s.instrument_up());
    let r = s.instruments.rect(0, g.size);
    assert!(r.x <= g.size.0 - 80.0 && r.y <= g.size.1 - 30.0);
    // A second window opens a little lower and to the right, on top.
    let drums = s
        .project()
        .tracks
        .values()
        .find(|x| x.kind == TrackKind::Instrument && x.id != t)
        .map(|x| x.id);
    if let Some(d) = drums {
        assert!(s.set_instrument(d, &Pick::built_in("Drum Kit")));
        let did = current(&s.project().tracks[&d]).unwrap().id;
        s.show_instrument(d, did);
        assert_eq!(s.instruments.open.last().unwrap().plugin, did);
    }
}

#[test]
fn the_window_closes_when_its_instrument_goes() {
    let (mut s, g, t, id) = studio();
    s.show_instrument(t, id);
    assert!(s.remove_instrument(t));
    let (x, y) = (g.size.0 / 2.0, 100.0);
    s.instrument_down(&g, x, y);
    assert!(s.instruments.open.is_empty());
}

#[test]
fn the_knobs_are_the_built_in_parameters_in_their_order() {
    let (s, _, t, _) = studio();
    let x = plugin(&s, t);
    let def = x.builtin().expect("the Sampler");
    assert_eq!(def.uid, "sampler");
    // Every parameter has its knob, at its default.
    let defaults: Vec<f64> = def.params.iter().map(|p| p.default_norm()).collect();
    assert_eq!(values(&x), defaults);
    assert_eq!(preset_values(def, "初期").unwrap(), defaults);
    assert_eq!(preset_of(&x), "初期");
    // 強め moves the sweeping ones and leaves 音色 (a choice) where it is.
    let strong = preset_values(def, "強め").unwrap();
    assert_eq!(strong[0], defaults[0]);
    assert_ne!(strong[1], defaults[1]);
    assert!(strong.iter().all(|v| (0.0..=1.0).contains(v)));
    assert_eq!(preset_values(def, "no such"), None);
    assert_eq!(text(&def.params[5], 1.0), "20.0 kHz");
}

#[test]
fn a_built_in_effect_opens_from_the_mixer_and_its_knob_is_undone() {
    let (mut s, g, t, _) = studio();
    assert!(s.run_command(
        "plugin.add",
        serde_json::json!({ "track": t.to_string(), "name": "Compressor" }),
    ));
    let fx = s.project().tracks[&t]
        .inserts
        .iter()
        .find(|x| x.name == "Compressor")
        .unwrap()
        .clone();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    let (x, y) = s
        .mixer_center(&g, MixerHit::Open(t, fx.id))
        .expect("on screen");
    let hit = s.hit(&g, x, y);
    assert_eq!(hit, Hit::Mixer(MixerHit::Open(t, fx.id)));
    s.press(&g, hit);
    assert!(s.instruments.is_open(fx.id), "{:?}", s.error);
    assert!(s.error.is_none());
    assert_eq!(s.mixer.open_editors(), 0, "no window of a plug-in");
    // A knob for each of its parameters.
    let def = fx.builtin().unwrap();
    assert_eq!(s.instruments.open.last().unwrap().knobs, def.params.len());
    let (k, spec) = def.param("threshold").unwrap();
    let fx_of = |s: &Studio| {
        s.project().tracks[&t]
            .inserts
            .iter()
            .find(|x| x.id == fx.id)
            .cloned()
            .unwrap()
    };
    let start = values(&fx_of(&s))[k];
    assert_eq!(start, spec.default_norm());
    let before = s.song().steps();
    let (x, y) = at(&s, &g, fx.id, Part::Knob(k));
    assert!(s.instrument_down(&g, x, y));
    assert!(s.instrument_moved(&g, x, y + 30.0, false));
    assert!(s.instrument_up());
    assert_eq!(s.song().steps(), before + 1);
    let now = values(&fx_of(&s))[k];
    assert!((now - (start - 0.2)).abs() < 1e-6, "{now}");
    assert!(s.undo(&g));
    assert_eq!(values(&fx_of(&s))[k], start);
}

/// A built-in effect of the track by name: its plugin's id.
fn added(s: &Studio, t: TrackId, name: &str) -> PluginId {
    assert!(
        s.project().tracks[&t]
            .inserts
            .iter()
            .any(|x| x.name == name)
    );
    s.project().tracks[&t]
        .inserts
        .iter()
        .find(|x| x.name == name)
        .unwrap()
        .id
}

#[test]
fn the_bars_parts_are_in_sight_at_1280_and_360() {
    // UI-322: every seat of the shared bar sits inside the window at both
    // screen sizes, a press on it landing on its part. The Sampler's window
    // is narrower than the bar's narrow width: ‹ › hide and the name cuts.
    let (mut s, _, t, id) = studio();
    s.show_instrument(t, id);
    for w in [1280.0, 360.0] {
        let g = s.geometry(w, 700.0);
        for part in [
            Part::Power,
            Part::Head,
            Part::Preset,
            Part::Channel,
            Part::Pin,
            Part::Close,
        ] {
            assert!(
                s.instrument_center(&g, id, part).is_some(),
                "{part:?} at {w}"
            );
        }
        // No ‹ › and no side-chain seat on a window this narrow.
        assert!(
            s.instrument_center(&g, id, Part::PrevPreset).is_none(),
            "at {w}"
        );
        assert!(
            s.instrument_center(&g, id, Part::NextPreset).is_none(),
            "at {w}"
        );
        assert!(
            s.instrument_center(&g, id, Part::Sidechain).is_none(),
            "at {w}"
        );
    }
}

#[test]
fn the_steps_hide_on_a_narrow_bar_and_walk_the_presets_on_a_wide_one() {
    let (mut s, g, t, _) = studio();
    assert!(s.run_command(
        "plugin.add",
        serde_json::json!({ "track": t.to_string(), "name": "EQ" }),
    ));
    let eq = added(&s, t, "EQ");
    s.show_instrument(t, eq);
    let fx_of = |s: &Studio| {
        s.project().tracks[&t]
            .inserts
            .iter()
            .find(|x| x.id == eq)
            .cloned()
            .unwrap()
    };
    // EQ's window is wider than the bar's narrow width: the ‹ › show.
    assert_eq!(preset_of(&fx_of(&s)), "初期");
    let (x, y) = at(&s, &g, eq, Part::NextPreset);
    assert!(s.instrument_down(&g, x, y));
    assert_eq!(preset_of(&fx_of(&s)), "やさしく");
    let (x, y) = at(&s, &g, eq, Part::NextPreset);
    s.instrument_down(&g, x, y);
    assert_eq!(preset_of(&fx_of(&s)), "強め");
    // ‹ steps back and wraps at the ends.
    for want in ["やさしく", "初期", "声を前に"] {
        let (x, y) = at(&s, &g, eq, Part::PrevPreset);
        s.instrument_down(&g, x, y);
        assert_eq!(preset_of(&fx_of(&s)), want);
    }
}

#[test]
fn the_power_toggles_the_plugins_bypass() {
    let (mut s, g, t, id) = studio();
    s.show_instrument(t, id);
    assert!(plugin(&s, t).runs());
    for want in [true, false] {
        let (x, y) = at(&s, &g, id, Part::Power);
        assert!(s.instrument_down(&g, x, y));
        assert_eq!(plugin(&s, t).bypass, want);
    }
    // The press is one undo step, as the mixer's mark is.
    assert!(s.undo(&g));
    assert!(plugin(&s, t).bypass);
}

#[test]
fn the_pin_marks_the_window() {
    let (mut s, g, t, id) = studio();
    s.show_instrument(t, id);
    assert!(!s.instruments.open[0].pinned);
    let (x, y) = at(&s, &g, id, Part::Pin);
    assert!(s.instrument_down(&g, x, y));
    assert!(s.instruments.open[0].pinned);
    let (x, y) = at(&s, &g, id, Part::Pin);
    assert!(s.instrument_down(&g, x, y));
    assert!(!s.instruments.open[0].pinned);
}

#[test]
fn the_sidechain_seat_is_only_on_the_devices_that_take_a_key() {
    let (mut s, g, t, id) = studio();
    assert!(s.run_command(
        "plugin.add",
        serde_json::json!({ "track": t.to_string(), "name": "Compressor" }),
    ));
    let comp = added(&s, t, "Compressor");
    s.show_instrument(t, id);
    s.show_instrument(t, comp);
    let p = s.project();
    for (plugin_id, takes) in [(comp, true), (id, false)] {
        let i = s
            .instruments
            .open
            .iter()
            .position(|w| w.plugin == plugin_id)
            .unwrap();
        let r = s.instruments.rect(i, g.size);
        let track = &p.tracks[&t];
        let plugin = track.inserts.iter().find(|x| x.id == plugin_id).unwrap();
        let seat = part_rect(r, track, plugin, plugin.builtin().unwrap(), Part::Sidechain);
        if takes {
            // Width 0 until UI-336 fills it, between the empty stretch and
            // the channel editor's button.
            assert_eq!(seat.w, 0.0);
            assert!(seat.x > r.x && seat.x < r.right(), "{seat:?} in {r:?}");
        } else {
            assert_eq!(seat, Rect::default());
        }
    }
}

#[test]
fn the_tag_marks_the_preset_the_knobs_moved_off() {
    let (mut s, g, t, id) = studio();
    s.show_instrument(t, id);
    // At the defaults the tag names the preset with no mark.
    assert_eq!(s.instruments.tag_of(&plugin(&s, t)), ("初期", false));
    // Load another: it names that one; a knob off it marks it.
    assert!(s.load_preset(id, "強め"));
    assert_eq!(s.instruments.tag_of(&plugin(&s, t)), ("強め", false));
    let (cutoff, _) = plugin(&s, t).builtin().unwrap().param("cutoff").unwrap();
    let (x, y) = at(&s, &g, id, Part::Knob(cutoff));
    assert!(s.instrument_down(&g, x, y));
    assert!(s.instrument_moved(&g, x, y + 30.0, false));
    assert!(s.instrument_up());
    assert_eq!(s.instruments.tag_of(&plugin(&s, t)), ("強め", true));
    // Back on a preset's values the mark is gone.
    assert!(s.load_preset(id, "初期"));
    assert_eq!(s.instruments.tag_of(&plugin(&s, t)), ("初期", false));
}

/// Moves the knob `param` of the window of `id` off where it is, as a drag
/// does.
fn nudge(s: &mut Studio, g: &Geometry, id: PluginId, param: &str) {
    let (k, _) = insert(s, id).builtin().unwrap().param(param).unwrap();
    let (x, y) = at(s, g, id, Part::Knob(k));
    assert!(s.instrument_down(g, x, y));
    assert!(s.instrument_moved(g, x, y + 30.0, false));
    assert!(s.instrument_up());
}

/// The insert `id` wherever it is.
fn insert(s: &Studio, id: PluginId) -> Plugin {
    s.project()
        .tracks
        .values()
        .flat_map(|t| &t.inserts)
        .find(|x| x.id == id)
        .cloned()
        .unwrap()
}

#[test]
fn an_effects_preset_list_checks_the_preset_it_is_at() {
    let (mut s, g, t, _) = studio();
    assert!(s.run_command(
        "plugin.add",
        serde_json::json!({ "track": t.to_string(), "name": "Compressor" }),
    ));
    let comp = added(&s, t, "Compressor");
    s.show_instrument(t, comp);
    assert!(s.load_preset(comp, "強め"));
    let (x, y) = at(&s, &g, comp, Part::Preset);
    assert!(s.instrument_down(&g, x, y));
    let popup = s.popup.as_ref().expect("the presets");
    assert_eq!(popup.current(), "強め", "not the instrument's 初期");
    assert_eq!(s.instruments.tag_of(&insert(&s, comp)), ("強め", false));
}

#[test]
fn the_tag_keeps_its_preset_in_a_song_opened_again() {
    let (mut s, _, t, id) = studio();
    assert!(s.load_preset(id, "強め"));
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let dir = root.join(format!("target/inst-tag-reopen-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    s.command(|song| song.save_as(&dir));
    assert!(s.error.is_none(), "{:?}", s.error);
    // A fresh start opens the song: the tag learns its preset from the knobs.
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    s.finish_file_request_sync(crate::studio::FileRequest::Open, &dir);
    assert!(s.error.is_none(), "{:?}", s.error);
    let t2 = keys(&s);
    assert_eq!(t2, t);
    s.show_instrument(t, id);
    assert_eq!(s.instruments.tag_of(&plugin(&s, t)), ("強め", false));
    nudge(&mut s, &g, id, "cutoff");
    assert_eq!(s.instruments.tag_of(&plugin(&s, t)), ("強め", true));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_tag_keeps_the_preset_an_undo_went_back_to() {
    let (mut s, g, t, id) = studio();
    s.show_instrument(t, id);
    assert!(s.load_preset(id, "強め"));
    assert!(s.load_preset(id, "やさしく"));
    assert!(s.undo(&g));
    assert_eq!(preset_of(&plugin(&s, t)), "強め");
    nudge(&mut s, &g, id, "cutoff");
    assert_eq!(s.instruments.tag_of(&plugin(&s, t)), ("強め", true));
}

#[test]
fn the_tag_keeps_a_preset_loaded_from_the_cli() {
    let (mut s, g, t, id) = studio();
    s.show_instrument(t, id);
    // The CLI and the MCP send the command itself, not through the tag.
    let def = plugin(&s, t).builtin().unwrap();
    let values: Vec<_> = def
        .params
        .iter()
        .zip(preset_values(def, "強め").unwrap())
        .map(|(p, v)| serde_json::json!({ "param": p.id, "value": v }))
        .collect();
    assert!(s.run_command(
        "plugin.preset.load",
        serde_json::json!({ "id": id.to_string(), "preset": "強め", "values": values }),
    ));
    nudge(&mut s, &g, id, "cutoff");
    assert_eq!(s.instruments.tag_of(&plugin(&s, t)), ("強め", true));
}

/// A Compressor in a song from before the sidechain (PLG-071) keeps only its seven knobs. At
/// 「強め」 it opens again as 「強め」, its knobs reading as they did.
#[test]
fn an_old_songs_compressor_opens_at_its_preset() {
    let (mut s, _, t, _) = studio();
    assert!(s.run_command(
        "plugin.add",
        serde_json::json!({ "track": t.to_string(), "name": "Compressor" }),
    ));
    let comp = added(&s, t, "Compressor");
    let def = insert(&s, comp).builtin().unwrap();
    let old = [
        "threshold",
        "ratio",
        "attack",
        "release",
        "knee",
        "makeup",
        "mix",
    ];
    // 「強め」 as it was: each sweeping knob 0.22 of its sweep off its default, every
    // other one the other way.
    let strong: Vec<f64> = old
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let (k, p) = def.param(id).unwrap();
            assert_eq!(k, i, "{id} kept its place");
            let d = p.default_norm();
            let f = if i % 2 == 1 { -0.22 } else { 0.22 };
            if p.steps().is_some() {
                d
            } else {
                (d + f).clamp(0.0, 1.0)
            }
        })
        .collect();
    assert_eq!(preset_values(def, "強め").unwrap()[..old.len()], strong);
    let values_of: Vec<_> = old
        .iter()
        .zip(&strong)
        .map(|(id, v)| serde_json::json!({ "param": id, "value": v }))
        .collect();
    assert!(s.run_command(
        "plugin.preset.load",
        serde_json::json!({ "id": comp.to_string(), "preset": "強め", "values": values_of }),
    ));
    let texts = |x: &Plugin| -> Vec<String> {
        def.params
            .iter()
            .zip(values(x))
            .map(|(p, v)| text(p, v))
            .collect()
    };
    let before = texts(&insert(&s, comp));
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let dir = root.join(format!("target/comp-old-preset-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    s.command(|song| song.save_as(&dir));
    assert!(s.error.is_none(), "{:?}", s.error);
    // The song as the version before wrote it: none of the new knobs.
    let file = dir.join("project.json");
    let mut song: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    fn strip(v: &mut serde_json::Value, id: &str, old: &[&str]) -> bool {
        match v {
            serde_json::Value::Object(m) if m.get("id").and_then(|x| x.as_str()) == Some(id) => {
                let params = m.get_mut("params").and_then(|p| p.as_object_mut()).unwrap();
                params.retain(|k, _| old.contains(&k.as_str()));
                true
            }
            serde_json::Value::Object(m) => m.values_mut().any(|x| strip(x, id, old)),
            serde_json::Value::Array(a) => a.iter_mut().any(|x| strip(x, id, old)),
            _ => false,
        }
    }
    assert!(strip(&mut song, &comp.to_string(), &old));
    std::fs::write(&file, serde_json::to_string(&song).unwrap()).unwrap();
    // Its history and versions are from before too; here they would bring the new knobs back.
    for d in ["history", "versions"] {
        std::fs::remove_dir_all(dir.join(d)).unwrap();
    }
    let mut s = Studio::sample();
    s.finish_file_request_sync(crate::studio::FileRequest::Open, &dir);
    assert!(s.error.is_none(), "{:?}", s.error);
    let _ = std::fs::remove_dir_all(&dir);
    s.show_instrument(t, comp);
    let x = insert(&s, comp);
    assert_eq!(x.params.len(), old.len(), "the old knobs only");
    assert_eq!(texts(&x), before);
    assert_eq!(preset_of(&x), "強め");
    assert_eq!(s.instruments.tag_of(&x), ("強め", false));
}

/// A preset is the compressor's own knobs (PLG-071): the Gain and the key's filter and
/// Listen stay where they were set, and the preset is still read with them so.
#[test]
fn a_preset_leaves_the_sidechain_knobs_where_they_are() {
    let (mut s, _, t, _) = studio();
    assert!(s.run_command(
        "plugin.add",
        serde_json::json!({ "track": t.to_string(), "name": "Compressor" }),
    ));
    let comp = added(&s, t, "Compressor");
    s.show_instrument(t, comp);
    let set = [
        ("gain", "6"),
        ("filter", "on"),
        ("low_cut", "200"),
        ("high_cut", "5000"),
        ("listen", "on"),
    ];
    for (param, v) in set {
        assert!(s.run_command(
            "plugin.param.set",
            serde_json::json!({ "id": comp.to_string(), "param": param, "plain": v }),
        ));
    }
    let kept = |s: &Studio| -> Vec<String> {
        let x = insert(s, comp);
        let def = x.builtin().unwrap();
        let now = values(&x);
        set.iter()
            .map(|(id, _)| {
                let (k, p) = def.param(id).unwrap();
                text(p, now[k])
            })
            .collect()
    };
    let before = kept(&s);
    for preset in ["強め", "やさしく", "初期"] {
        assert!(s.load_preset(comp, preset));
        assert_eq!(kept(&s), before, "{preset}");
        assert_eq!(preset_of(&insert(&s, comp)), preset);
        assert_eq!(s.instruments.tag_of(&insert(&s, comp)), (preset, false));
    }
}
