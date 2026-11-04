use super::menu::Menu;
use super::window::{Part, preset_of, preset_values, presets, text, values};
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

/// A sample in the Sampler, so that its window shows its knobs (UI-333).
fn fill(s: &mut Studio, id: PluginId) {
    assert!(s.run_command(
        "plugin.samples.add",
        serde_json::json!({ "id": id.to_string(), "file": "Piano C3.wav" }),
    ));
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
    let (mut s, _, t, id) = studio();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    let g = s.geometry(1600.0, 1000.0);
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
    fill(&mut s, id);
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
    assert_eq!(preset_of(&plugin(&s, t)), None);
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
    fill(&mut s, id);
    s.show_instrument(t, id);
    // The transposition is in whole semitones: a drag lands on one of them.
    let (sound, spec) = plugin(&s, t).builtin().unwrap().param("transpose").unwrap();
    let (x, y) = at(&s, &g, id, Part::Knob(sound));
    assert!(s.instrument_down(&g, x, y));
    assert!(s.instrument_moved(&g, x, y - 40.0, false));
    assert!(s.instrument_up());
    let now = values(&plugin(&s, t))[sound];
    let plain = spec.to_plain(now);
    assert_eq!(plain, plain.round());
    assert!(plain >= 1.0, "{plain}");
    assert_eq!(text(spec, now), spec.text(plain));
}

#[test]
fn a_preset_sets_every_knob_in_one_step() {
    let (mut s, g, t, id) = studio();
    fill(&mut s, id);
    s.show_instrument(t, id);
    assert_eq!(preset_of(&plugin(&s, t)), Some("Default"));
    nudge(&mut s, &g, id, "cutoff");
    nudge(&mut s, &g, id, "attack");
    assert_eq!(preset_of(&plugin(&s, t)), None);
    let (x, y) = at(&s, &g, id, Part::Preset);
    assert!(s.instrument_down(&g, x, y));
    let popup = s.popup.as_ref().expect("the presets");
    assert!(matches!(popup.kind, PopupKind::InstMenu(_)));
    let cs = popup.choices();
    let labels: Vec<&str> = cs.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Default",
            "上書き保存",
            "名前を付けて保存…",
            "ブラウザーに表示"
        ]
    );
    // The ✓ stays on the preset the knobs moved off.
    assert_eq!(popup.current(), format!("{}Default", super::menu::LOAD));
    let before = s.song().steps();
    s.popup_action(&g, Action::Pick(format!("{}Default", super::menu::LOAD)));
    assert!(s.popup.is_none());
    assert_eq!(s.song().steps(), before + 1);
    assert_eq!(preset_of(&plugin(&s, t)), Some("Default"));
    assert_eq!(
        values(&plugin(&s, t)),
        preset_values(plugin(&s, t).builtin().unwrap(), "Default").unwrap()
    );
    assert!(s.undo(&g));
    assert_eq!(preset_of(&plugin(&s, t)), None);
}

#[test]
fn the_bar_drags_the_window_and_it_stays_in_reach() {
    let (mut s, g, t, id) = studio();
    s.show_instrument(t, id);
    let (x, y) = at(&s, &g, id, Part::Head);
    let before = s.instruments.frame(0, &g);
    assert!(s.instrument_down(&g, x, y));
    assert!(s.instrument_moved(&g, x + 40.0, y + 25.0, false));
    assert!(s.instrument_up());
    let after = s.instruments.rect(0, g.size);
    // To within float rounding (the press was at the bar's centre).
    assert!(
        (after.x - before.x - 40.0).abs() < 1e-3,
        "{after:?} {before:?}"
    );
    assert!(
        (after.y - before.y - 25.0).abs() < 1e-3,
        "{after:?} {before:?}"
    );
    // Dragged far off, a strip of it stays on the screen.
    let (x, y) = at(&s, &g, id, Part::Head);
    assert!(s.instrument_down(&g, x, y));
    assert!(s.instrument_moved(&g, x + 5000.0, y + 5000.0, false));
    assert!(s.instrument_up());
    let r = s.instruments.frame(0, &g);
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

/// L-ui-2: until it is forgotten, a window whose device went takes no hover or wheel.
#[test]
fn a_window_whose_device_went_takes_no_pointer() {
    let (mut s, g, t, id) = studio();
    s.show_instrument(t, id);
    let r = s.instruments.rect(0, g.size);
    let (x, y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    assert!(s.instruments.at(&s.project(), &g, x, y).is_some());
    let w = s.instruments.open[0];
    assert!(s.remove_instrument(t));
    // As between a change from elsewhere and the next look at the windows.
    if s.instruments.open.is_empty() {
        s.instruments.open.push(w);
    }
    assert_eq!(s.instruments.at(&s.project(), &g, x, y), None);
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
    assert_eq!(preset_values(def, "Default").unwrap(), defaults);
    assert_eq!(preset_of(&x), Some("Default"));
    // The Sampler has only its Default (PLG-064); an effect's are its own.
    assert_eq!(presets(def), ["Default"]);
    assert_eq!(preset_values(def, "ドラムをまとめる"), None);
    assert_eq!(preset_values(def, "no such"), None);
    let comp = xirius_params::find("compressor").unwrap();
    assert_eq!(
        presets(comp),
        [
            "Default",
            "ドラムをまとめる",
            "ボーカルを整える",
            "並列で太く"
        ]
    );
    assert_eq!(text(&def.params[5], 1.0), "20.0 kHz");
}

#[test]
fn a_built_in_effect_opens_from_the_mixer_and_its_knob_is_undone() {
    let (mut s, _, t, _) = studio();
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
    s.drawer_size.mixer = Some(600.0);
    let g = s.geometry(1600.0, 1000.0);
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
    // is wider than the bar's narrow width: ‹ › show.
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
        // ‹ ›, and no side-chain seat on an instrument.
        assert!(
            s.instrument_center(&g, id, Part::PrevPreset).is_some(),
            "at {w}"
        );
        assert!(
            s.instrument_center(&g, id, Part::NextPreset).is_some(),
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
    assert_eq!(preset_of(&fx_of(&s)), Some("Default"));
    let (x, y) = at(&s, &g, eq, Part::NextPreset);
    assert!(s.instrument_down(&g, x, y));
    assert_eq!(preset_of(&fx_of(&s)), Some("ボーカルを明るく"));
    let (x, y) = at(&s, &g, eq, Part::NextPreset);
    s.instrument_down(&g, x, y);
    assert_eq!(preset_of(&fx_of(&s)), Some("低域を締める"));
    // ‹ steps back and wraps at the ends.
    for want in ["ボーカルを明るく", "Default", "電話の声"] {
        let (x, y) = at(&s, &g, eq, Part::PrevPreset);
        s.instrument_down(&g, x, y);
        assert_eq!(preset_of(&fx_of(&s)), Some(want));
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
    // Pinned, so the Compressor opens beside it (UI-323).
    s.instruments.open[0].pinned = true;
    s.show_instrument(t, comp);
    let p = s.project();
    for (plugin_id, takes) in [(comp, true), (id, false)] {
        let i = s
            .instruments
            .open
            .iter()
            .position(|w| w.plugin == plugin_id)
            .unwrap();
        let r = s.instruments.frame(i, &g);
        let track = &p.tracks[&t];
        let plugin = track.inserts.iter().find(|x| x.id == plugin_id).unwrap();
        let seat = s.instruments.part_rect(
            r,
            &p,
            track,
            plugin,
            plugin.builtin().unwrap(),
            Part::Sidechain,
        );
        if takes {
            // The on button, between the empty stretch and the channel
            // editor's button (UI-336).
            assert!(seat.w > 0.0);
            assert!(
                seat.x > r.x && seat.right() < r.right(),
                "{seat:?} in {r:?}"
            );
        } else {
            assert_eq!(seat, Rect::default());
        }
    }
}

#[test]
fn the_tag_marks_the_preset_the_knobs_moved_off() {
    let (mut s, g, t, _) = studio();
    let id = compressor(&mut s, t);
    // At the defaults the tag names the preset with no mark.
    assert_eq!(
        s.instruments.tag_of(&insert(&s, id)),
        ("Default".to_string(), false)
    );
    // Load another: it names that one; a knob off it marks it.
    assert!(s.load_preset(id, "ドラムをまとめる"));
    assert_eq!(
        s.instruments.tag_of(&insert(&s, id)),
        ("ドラムをまとめる".to_string(), false)
    );
    nudge(&mut s, &g, id, "threshold");
    assert_eq!(
        s.instruments.tag_of(&insert(&s, id)),
        ("ドラムをまとめる".to_string(), true)
    );
    // Back on a preset's values the mark is gone.
    assert!(s.load_preset(id, "Default"));
    assert_eq!(
        s.instruments.tag_of(&insert(&s, id)),
        ("Default".to_string(), false)
    );
}

/// A Compressor on the track, its window open.
fn compressor(s: &mut Studio, t: TrackId) -> PluginId {
    assert!(s.run_command(
        "plugin.add",
        serde_json::json!({ "track": t.to_string(), "name": "Compressor" }),
    ));
    let comp = added(s, t, "Compressor");
    s.show_instrument(t, comp);
    comp
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
    assert!(s.load_preset(comp, "ドラムをまとめる"));
    let (x, y) = at(&s, &g, comp, Part::Preset);
    assert!(s.instrument_down(&g, x, y));
    let popup = s.popup.as_ref().expect("the presets");
    assert_eq!(
        popup.current(),
        format!("{}ドラムをまとめる", super::menu::LOAD),
        "not the instrument's Default"
    );
    assert_eq!(
        s.instruments.tag_of(&insert(&s, comp)),
        ("ドラムをまとめる".to_string(), false)
    );
}

#[test]
fn the_tag_keeps_its_preset_in_a_song_opened_again() {
    let (mut s, _, t, _) = studio();
    let id = compressor(&mut s, t);
    assert!(s.load_preset(id, "ドラムをまとめる"));
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let dir = root.join(format!("target/inst-tag-reopen-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    s.command(|song| song.save_as(&dir));
    assert!(s.error.is_none(), "{:?}", s.error);
    // A fresh start opens the song: the device keeps its preset's name in it.
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    s.finish_file_request_sync(crate::studio::FileRequest::Open, &dir);
    assert!(s.error.is_none(), "{:?}", s.error);
    let t2 = keys(&s);
    assert_eq!(t2, t);
    s.show_instrument(t, id);
    assert_eq!(
        s.instruments.tag_of(&insert(&s, id)),
        ("ドラムをまとめる".to_string(), false)
    );
    nudge(&mut s, &g, id, "threshold");
    assert_eq!(
        s.instruments.tag_of(&insert(&s, id)),
        ("ドラムをまとめる".to_string(), true)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_tag_keeps_the_preset_an_undo_went_back_to() {
    let (mut s, g, t, _) = studio();
    let id = compressor(&mut s, t);
    assert!(s.load_preset(id, "ドラムをまとめる"));
    assert!(s.load_preset(id, "ボーカルを整える"));
    assert!(s.undo(&g));
    assert_eq!(preset_of(&insert(&s, id)), Some("ドラムをまとめる"));
    nudge(&mut s, &g, id, "threshold");
    assert_eq!(
        s.instruments.tag_of(&insert(&s, id)),
        ("ドラムをまとめる".to_string(), true)
    );
}

#[test]
fn the_tag_keeps_a_preset_loaded_from_the_cli() {
    let (mut s, g, t, _) = studio();
    let id = compressor(&mut s, t);
    // The CLI and the MCP send the command itself, not through the tag.
    let def = insert(&s, id).builtin().unwrap();
    let values: Vec<_> = def
        .params
        .iter()
        .zip(preset_values(def, "ドラムをまとめる").unwrap())
        .filter(|(p, _)| p.preset)
        .map(|(p, v)| serde_json::json!({ "param": p.id, "value": v }))
        .collect();
    assert!(s.run_command(
        "plugin.preset.load",
        serde_json::json!({ "id": id.to_string(), "preset": "ドラムをまとめる", "values": values }),
    ));
    nudge(&mut s, &g, id, "threshold");
    assert_eq!(
        s.instruments.tag_of(&insert(&s, id)),
        ("ドラムをまとめる".to_string(), true)
    );
}

/// A Compressor in a song from before the sidechain (PLG-071) keeps only its seven knobs. At
/// 「ドラムをまとめる」 it opens again as that, its knobs reading as they did.
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
    for (i, id) in old.iter().enumerate() {
        assert_eq!(def.param(id).unwrap().0, i, "{id} kept its place");
    }
    let strong = preset_values(def, "ドラムをまとめる").unwrap()[..old.len()].to_vec();
    let values_of: Vec<_> = old
        .iter()
        .zip(&strong)
        .map(|(id, v)| serde_json::json!({ "param": id, "value": v }))
        .collect();
    assert!(s.run_command(
        "plugin.preset.load",
        serde_json::json!({ "id": comp.to_string(), "preset": "ドラムをまとめる", "values": values_of }),
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
    assert_eq!(preset_of(&x), Some("ドラムをまとめる"));
    assert_eq!(
        s.instruments.tag_of(&x),
        ("ドラムをまとめる".to_string(), false)
    );
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
    for preset in ["ドラムをまとめる", "ボーカルを整える", "Default"] {
        assert!(s.load_preset(comp, preset));
        assert_eq!(kept(&s), before, "{preset}");
        assert_eq!(preset_of(&insert(&s, comp)), Some(preset));
        assert_eq!(
            s.instruments.tag_of(&insert(&s, comp)),
            (preset.to_string(), false)
        );
    }
}

/// The rows of the open preset list: (label, enabled), and the row with the ✓.
fn preset_rows(s: &mut Studio, g: &Geometry, id: PluginId) -> (Vec<(String, bool)>, String) {
    s.popup = None;
    let (x, y) = at(s, g, id, Part::Preset);
    assert!(s.instrument_down(g, x, y));
    let popup = s.popup.as_ref().expect("the presets");
    let PopupKind::InstMenu(menu) = popup.kind else {
        panic!("{:?}", popup.kind);
    };
    let (rows, now) = s.inst_menu_view(menu);
    let rows = rows
        .into_iter()
        .map(|c| (c.label, !c.to.starts_with("off:")))
        .collect();
    let now = now
        .strip_prefix(super::menu::LOAD)
        .unwrap_or(&now)
        .to_string();
    (rows, now)
}

fn enabled(rows: &[(String, bool)], label: &str) -> bool {
    rows.iter().find(|(l, _)| l == label).expect(label).1
}

/// 上書き保存 is there once the knobs moved off a preset's file, never over Default; the
/// device then keeps the name it was saved as, the mark gone, in one step.
#[test]
fn saving_a_preset_names_the_device_after_it() {
    let (mut s, g, t, _) = studio();
    let id = compressor(&mut s, t);
    let (rows, now) = preset_rows(&mut s, &g, id);
    assert_eq!(now, "Default");
    assert!(!enabled(&rows, "上書き保存"), "at Default");
    assert!(enabled(&rows, "名前を付けて保存…"));
    assert!(enabled(&rows, "ブラウザーに表示"), "BRW-017");
    nudge(&mut s, &g, id, "threshold");
    let (rows, _) = preset_rows(&mut s, &g, id);
    assert!(!enabled(&rows, "上書き保存"), "never over Default");
    // Save it as a new preset: the list has it and the tag names it.
    let before = s.song().steps();
    assert!(s.save_preset(id, "締める", false));
    assert_eq!(s.song().steps(), before + 1);
    assert_eq!(
        s.instruments.tag_of(&insert(&s, id)),
        ("締める".to_string(), false)
    );
    let (rows, now) = preset_rows(&mut s, &g, id);
    assert_eq!(now, "締める");
    assert!(rows.iter().any(|(l, _)| l == "締める"));
    assert!(!enabled(&rows, "上書き保存"), "the knobs are at it");
    // Off it: 上書き保存 writes over its file and the mark goes.
    nudge(&mut s, &g, id, "ratio");
    assert_eq!(
        s.instruments.tag_of(&insert(&s, id)),
        ("締める".to_string(), true)
    );
    let (rows, now) = preset_rows(&mut s, &g, id);
    assert_eq!(now, "締める", "the ✓ stays on it");
    assert!(enabled(&rows, "上書き保存"));
    s.inst_menu_pick(Menu::Preset(t, id), super::menu::SAVE);
    assert_eq!(
        s.instruments.tag_of(&insert(&s, id)),
        ("締める".to_string(), false)
    );
    // Loading another and back reads the file's knobs.
    assert!(s.load_preset(id, "Default"));
    assert!(s.load_preset(id, "締める"));
    assert_eq!(
        s.instruments.tag_of(&insert(&s, id)),
        ("締める".to_string(), false)
    );
}

/// L-pre-2: the song's «hall» is the file «Hall», as the session finds it: the tag names the
/// file, so the list's ✓ and ‹ › find its row, and its knobs tell whether the device is off.
#[test]
fn the_tag_finds_a_preset_whatever_its_letter_case() {
    let (mut s, g, t, _) = studio();
    let id = compressor(&mut s, t);
    nudge(&mut s, &g, id, "threshold");
    assert!(s.save_preset(id, "Hall", false));
    let mut x = insert(&s, id);
    x.preset = Some("hall".into());
    let presets = &s.instruments.presets;
    let def = x.builtin().unwrap();
    assert_eq!(
        presets.values_of(def, "hall"),
        presets.values_of(def, "Hall")
    );
    assert!(presets.values_of(def, "hall").is_some());
    assert_eq!(presets.tag_of(&x), ("Hall".to_string(), false));
    assert_eq!(presets.origin(&x).as_deref(), Some("Hall"));
}

/// 名前を付けて保存… asks for the name in the list's place, starting with a name no preset
/// has: the preset's with a number.
#[test]
fn save_as_asks_for_the_name() {
    let (mut s, g, t, _) = studio();
    let id = compressor(&mut s, t);
    assert!(s.load_preset(id, "ドラムをまとめる"));
    nudge(&mut s, &g, id, "threshold");
    preset_rows(&mut s, &g, id);
    s.inst_menu_pick(Menu::Preset(t, id), super::menu::SAVE_AS);
    assert!(s.context_open());
    assert_eq!(
        s.ask_field().map(|f| f.text()),
        Some("ドラムをまとめる 2".to_string())
    );
    for _ in 0..40 {
        s.ask_key("backspace");
    }
    s.ask_text(None, "強く締める", false);
    assert_eq!(s.ask_key("enter"), Some(true));
    assert!(!s.context_open());
    assert_eq!(
        s.instruments.tag_of(&insert(&s, id)),
        ("強く締める".to_string(), false)
    );
    // The factory preset it came from is as it was.
    assert!(s.load_preset(id, "ドラムをまとめる"));
    assert_eq!(
        s.instruments.tag_of(&insert(&s, id)),
        ("ドラムをまとめる".to_string(), false)
    );
}

/// Low 15 of the third review: the next name after `X 4294967295` does not overflow, nor the
/// one after `X 4294967294` (low 11 of the fourth); `Bass 9` goes on to `Bass 10`.
#[test]
fn save_as_names_past_the_last_number_do_not_overflow() {
    let (mut s, g, t, _) = studio();
    let id = compressor(&mut s, t);
    for (name, next) in [
        ("X 4294967294", "X 4294967295"),
        ("X 4294967295", "X 4294967295 2"),
        ("Bass 9", "Bass 10"),
        ("TR-808", "TR-808 2"),
    ] {
        nudge(&mut s, &g, id, "threshold");
        assert!(s.save_preset(id, name, false), "{:?}", s.error);
        preset_rows(&mut s, &g, id);
        s.inst_menu_pick(Menu::Preset(t, id), super::menu::SAVE_AS);
        assert_eq!(s.ask_field().map(|f| f.text()), Some(next.to_string()));
        s.ask_key("escape");
        assert!(!s.context_open());
    }
}

/// 名前を付けて保存… with a name the device has asks once more (上書きして保存…); Enter on
/// that writes over the file, Escape writes nothing.
#[test]
fn save_as_over_a_name_asks_first() {
    let (mut s, g, t, _) = studio();
    let id = compressor(&mut s, t);
    nudge(&mut s, &g, id, "threshold");
    assert!(s.save_preset(id, "締める", false));
    let kept = values(&insert(&s, id));
    nudge(&mut s, &g, id, "ratio");
    let moved = values(&insert(&s, id));
    assert_ne!(moved, kept);
    let name_it = |s: &mut Studio, name: &str| {
        preset_rows(s, &g, id);
        s.inst_menu_pick(Menu::Preset(t, id), super::menu::SAVE_AS);
        assert!(s.context_open());
        for _ in 0..40 {
            s.ask_key("backspace");
        }
        s.ask_text(None, name, false);
        s.ask_key("enter");
    };
    // The name in other letter case is the same file: it asks, and Escape leaves it.
    let before = s.song().steps();
    name_it(&mut s, "締める");
    assert!(s.context_open(), "上書きして保存…");
    assert_eq!(s.ask_field().map(|f| f.text()), Some("締める".to_string()));
    s.ask_key("escape");
    assert!(!s.context_open());
    assert_eq!(s.song().steps(), before, "nothing written");
    assert!(s.load_preset(id, "締める"));
    assert_near(&values(&insert(&s, id)), &kept);
    // Again, and Enter on the second box writes over it.
    nudge(&mut s, &g, id, "ratio");
    let moved = values(&insert(&s, id));
    name_it(&mut s, "締める");
    assert!(s.context_open());
    assert_eq!(s.ask_key("enter"), Some(true));
    assert!(!s.context_open());
    assert_eq!(
        s.instruments.tag_of(&insert(&s, id)),
        ("締める".to_string(), false)
    );
    assert!(s.load_preset(id, "Default"));
    assert!(s.load_preset(id, "締める"));
    assert_near(&values(&insert(&s, id)), &moved);
}

/// The knobs read back from a file: as kept, to the file's digits.
fn assert_near(got: &[f64], want: &[f64]) {
    assert_eq!(got.len(), want.len());
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() < 1e-9, "{got:?} {want:?}");
    }
}

/// A preset's row is told from the list's own rows whatever its name: `-`, `@save` and
/// `off:x` load as presets (M-pre-1).
#[test]
fn a_preset_named_like_a_row_loads() {
    let (mut s, g, t, _) = studio();
    let id = compressor(&mut s, t);
    let names = ["-", "@save", "off:x"];
    let mut kept = Vec::new();
    for name in names {
        nudge(&mut s, &g, id, "threshold");
        assert!(s.save_preset(id, name, false), "{name}");
        kept.push(values(&insert(&s, id)));
    }
    for (name, v) in names.iter().zip(&kept) {
        assert!(s.load_preset(id, "Default"));
        let (rows, _) = preset_rows(&mut s, &g, id);
        assert!(enabled(&rows, name), "{name}");
        let before = s.song().steps();
        s.inst_menu_pick(Menu::Preset(t, id), &format!("{}{name}", super::menu::LOAD));
        assert_eq!(s.song().steps(), before + 1, "{name}");
        assert_near(&values(&insert(&s, id)), v);
        assert_eq!(insert(&s, id).preset.as_deref(), Some(*name));
        let (_, now) = preset_rows(&mut s, &g, id);
        assert_eq!(now, *name);
    }
}

/// A preset whose file went (removed from the CLI) keeps its name, marked, and 上書き保存
/// writes it again (M-pre-3).
#[test]
fn a_gone_presets_name_is_marked_and_saves_again() {
    let (mut s, g, t, _) = studio();
    let id = compressor(&mut s, t);
    nudge(&mut s, &g, id, "threshold");
    assert!(s.save_preset(id, "締める", false));
    assert!(s.run_command(
        "preset.remove",
        serde_json::json!({ "preset": "締める", "device": "Compressor" }),
    ));
    s.refresh_presets();
    assert_eq!(
        s.instruments.tag_of(&insert(&s, id)),
        ("締める".to_string(), true)
    );
    let (rows, now) = preset_rows(&mut s, &g, id);
    assert!(!rows.iter().any(|(l, _)| l == "締める"), "not listed");
    assert_eq!(now, "締める");
    assert!(enabled(&rows, "上書き保存"));
    s.inst_menu_pick(Menu::Preset(t, id), super::menu::SAVE);
    s.refresh_presets();
    assert_eq!(
        s.instruments.tag_of(&insert(&s, id)),
        ("締める".to_string(), false)
    );
    let (rows, _) = preset_rows(&mut s, &g, id);
    assert!(rows.iter().any(|(l, _)| l == "締める"));
}

/// A device that keeps no preset (a song from before) and is at none shows 「—」 marked.
#[test]
fn a_device_at_no_known_preset_shows_a_dash() {
    let (mut s, g, t, _) = studio();
    let id = compressor(&mut s, t);
    nudge(&mut s, &g, id, "threshold");
    let mut x = insert(&s, id);
    x.preset = None;
    assert_eq!(
        s.instruments.tag_of(&x),
        (super::presets::UNKNOWN.to_string(), true)
    );
    assert_eq!(s.instruments.presets.origin(&x), None);
    // At a preset's knobs it takes that preset's name.
    let def = x.builtin().unwrap();
    let v = s.instruments.presets.values_of(def, "Default").unwrap();
    x.params.clear();
    for (p, v) in def.params.iter().zip(v) {
        x.params.insert(p.id.to_string(), v);
    }
    assert_eq!(s.instruments.tag_of(&x), ("Default".to_string(), false));
}

/// The search box filters the presets, not the saves under them.
#[test]
fn the_saves_stay_under_a_filtered_list() {
    let (mut s, _, t, _) = studio();
    let id = compressor(&mut s, t);
    let (choices, now) = s.inst_menu_view(Menu::Preset(t, id));
    let mut list = super::menu::pick(Menu::Preset(t, id), choices, &now);
    list.query = "ドラム".into();
    let rows: Vec<String> = list.visible().iter().map(|r| r.label.clone()).collect();
    assert_eq!(rows, ["ドラムをまとめる"]);
    let foot: Vec<String> = list.footer.iter().map(|r| r.label.clone()).collect();
    assert_eq!(
        foot,
        ["上書き保存", "名前を付けて保存…", "ブラウザーに表示"]
    );
}

/// L2-pre-1: 名前を付けて保存… from a preset whose name ends in a number starts at the next
/// one, and Default is refused at once, nothing asked or written. L2-pre-2: at 「—」 (no
/// preset known) 上書き保存 is off.
#[test]
fn save_as_counts_on_and_refuses_default_and_the_dash_writes_nothing() {
    let (mut s, g, t, _) = studio();
    let id = compressor(&mut s, t);
    nudge(&mut s, &g, id, "threshold");
    assert!(s.save_preset(id, "締める 2", false));
    nudge(&mut s, &g, id, "ratio");
    preset_rows(&mut s, &g, id);
    s.inst_menu_pick(Menu::Preset(t, id), super::menu::SAVE_AS);
    assert_eq!(
        s.ask_field().map(|f| f.text()),
        Some("締める 3".to_string())
    );
    for _ in 0..40 {
        s.ask_key("backspace");
    }
    let before = s.song().steps();
    s.ask_text(None, "default", false);
    s.ask_key("enter");
    assert!(!s.context_open(), "nothing asked");
    assert!(s.error.is_some());
    assert_eq!(s.song().steps(), before, "nothing written");

    let mut p = (*s.project()).clone();
    for x in p
        .tracks
        .values_mut()
        .flat_map(|t| &mut std::sync::Arc::make_mut(t).inserts)
    {
        if x.id == id {
            x.preset = None;
        }
    }
    let (rows, _) = super::menu::view(&p, &s.instruments.presets, Menu::Preset(t, id));
    let save = rows.iter().find(|c| c.label == "上書き保存").unwrap();
    assert!(save.to.starts_with("off:"), "{}", save.to);
}

/// L-ui-3: a narrow device's window that takes a wide one in takes its
/// width, not the narrow one's.
#[test]
fn a_window_that_takes_a_wider_device_in_takes_its_width() {
    let (mut s, _g, t, id) = studio();
    assert!(s.run_command(
        "plugin.add",
        serde_json::json!({ "track": t.to_string(), "name": "Limiter" }),
    ));
    let lim = added(&s, t, "Limiter");
    s.show_instrument(t, lim);
    assert_eq!(s.instruments.open.len(), 1);
    let narrow = s.instruments.open[0].body.0;
    s.show_instrument(t, id);
    assert_eq!(
        s.instruments.open.len(),
        1,
        "the unpinned window took it in"
    );
    let w = s.instruments.open[0];
    assert_eq!(w.plugin, id);
    let def = plugin(&s, t).builtin().unwrap();
    assert!(
        w.body.0 >= crate::device::layout::min_w(def),
        "{}",
        w.body.0
    );
    assert!(w.body.0 > narrow, "{} > {narrow}", w.body.0);
}

/// Low 13 of the fourth review: a body that does not fit even at its least width is laid at
/// the lower of its least and its full width; one that fits narrower is as wide as fits, and
/// one that grows taller as it narrows keeps its width.
#[test]
fn a_body_that_cannot_fit_spills_the_least() {
    use super::window::fit_w;
    // Lower as it narrows, never low enough: the least width.
    assert_eq!(fit_w(800.0, 400.0, 100.0, |w| w / 2.0), 400.0);
    // Taller as it narrows: the full width.
    assert_eq!(fit_w(800.0, 400.0, 100.0, |w| 1000.0 - w), 800.0);
    // Fits from 600 down: as wide as fits.
    let w = fit_w(800.0, 400.0, 300.0, |w| w / 2.0);
    assert!((599.0..=600.0).contains(&w), "{w}");
    // Fits as it is.
    assert_eq!(fit_w(800.0, 400.0, 400.0, |w| w / 2.0), 800.0);
    // Half a pixel over at the least width fits, as at the full one (the fifth review's low
    // 12): as wide as fits within that, not the least.
    let w = fit_w(800.0, 400.0, 100.0, |w| (w - 400.0) / 40.0 + 100.3);
    assert!((407.0..=408.0).contains(&w), "{w}");
}
