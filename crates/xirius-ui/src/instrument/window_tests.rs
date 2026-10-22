use super::window::{CUSTOM, Part, preset_of, preset_values, text, values};
use super::*;
use crate::mixer::MixerHit;
use crate::popup::{Action, PopupKind};
use crate::studio::{DrawerTab, Geometry, Hit};
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
    let labels: Vec<&str> = popup.choices.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["初期", "やさしく", "強め", "声を前に"]);
    assert_eq!(popup.current, "初期");
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
