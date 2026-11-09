//! KEY-012: the next and the previous channel and preset, pressed as the window presses its keys
//! (`]` and `[` in the mixer, Alt+PageDown and Alt+PageUp over a plugin window), and the same
//! names typed as commands.

use xirius_core::id::{PluginId, TrackId};

use crate::instrument::window::{Part, preset_of, presets};
use crate::studio::{DrawerTab, Geometry, Studio};
use crate::tools::Mods;
use crate::tree::RowKind;

const H: f32 = 1000.0;

fn alt() -> Mods {
    Mods {
        alt: true,
        ..Mods::NONE
    }
}

fn key(s: &mut Studio, g: &Geometry, k: &str, mods: Mods) -> bool {
    let character = (k.chars().count() == 1).then_some(k);
    s.mapped_key(g, k, character, mods)
}

fn selected(s: &Studio) -> Option<TrackId> {
    match s.selected_row {
        Some(RowKind::Track(t)) => Some(t),
        _ => None,
    }
}

/// The channels in the mixer's order, without the master.
fn channels(s: &Studio) -> Vec<TrackId> {
    crate::mixer::strip_order(&s.project())
        .into_iter()
        .filter(|t| !t.is_master())
        .collect()
}

#[test]
fn the_bracket_keys_step_through_the_mixer_and_stop_at_the_ends() {
    let mut s = Studio::sample();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    // Narrow, so the last strips are out of sight at first.
    let g = s.geometry(700.0, H);
    s.selected_row = None;
    assert_eq!(s.input_context(&g), "mixer");
    let order = channels(&s);
    assert!(order.len() >= 3, "{order:?}");
    let steps = s.song().steps();
    // With none selected, ] takes the first and [ the last.
    assert!(key(&mut s, &g, "]", Mods::NONE));
    assert_eq!(selected(&s), Some(order[0]));
    assert_eq!(s.arrange.tracks, vec![RowKind::Track(order[0])]);
    assert!(key(&mut s, &g, "]", Mods::NONE));
    assert_eq!(selected(&s), Some(order[1]));
    assert!(key(&mut s, &g, "[", Mods::NONE));
    assert!(
        !key(&mut s, &g, "[", Mods::NONE),
        "the first has none before it"
    );
    assert_eq!(selected(&s), Some(order[0]));
    s.selected_row = None;
    assert!(key(&mut s, &g, "[", Mods::NONE));
    let last = *order.last().unwrap();
    assert_eq!(selected(&s), Some(last));
    assert!(
        !key(&mut s, &g, "]", Mods::NONE),
        "the last has none after it"
    );
    // The mixer scrolled to the last strip, and it is in sight.
    assert!(s.mixer.scroll > 0.0);
    let view = s.mixer_view(&g);
    let strip = s
        .mixer_strips(&g)
        .into_iter()
        .find(|x| x.track == last)
        .expect("a strip")
        .name;
    assert!(strip.x >= view.x - 0.5 && strip.x + strip.w <= view.x + view.w + 0.5);
    // Nothing went into the song.
    assert_eq!(s.song().steps(), steps);
}

#[test]
fn the_channel_commands_run_by_name_without_the_mixer_but_its_keys_wait_for_it() {
    let mut s = Studio::sample();
    s.drawer_open = false;
    let g = s.geometry(1600.0, H);
    let order = channels(&s);
    s.selected_row = Some(RowKind::Track(order[0]));
    // The keys are the mixer's: in the arrange they do nothing.
    assert_ne!(s.input_context(&g), "mixer");
    assert!(!key(&mut s, &g, "]", Mods::NONE));
    assert_eq!(selected(&s), Some(order[0]));
    // The command by its name selects the next track in the column, mixer or not.
    assert!(s.run_binding(&g, "mixer.channel.next"));
    assert_eq!(selected(&s), Some(order[1]));
    assert!(!s.drawer_open, "it does not open the mixer");
    assert!(s.run_binding(&g, "mixer.channel.prev"));
    assert_eq!(selected(&s), Some(order[0]));
}

/// The sample song with an EQ on Keys, its window open. The EQ has its own presets
/// (PLG-064); the Sampler has only its Default.
fn keys_window() -> (Studio, Geometry, TrackId, PluginId) {
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, H);
    let t = crate::arrange::probe::track(&s, "Keys");
    assert!(s.run_command(
        "plugin.add",
        serde_json::json!({ "track": t.to_string(), "name": "EQ" }),
    ));
    let id = eq(&s, t).id;
    s.show_instrument(t, id);
    s.instruments.detached_target = Some(id);
    (s, g, t, id)
}

fn eq(s: &Studio, t: TrackId) -> xirius_core::model::Plugin {
    s.project().tracks[&t]
        .inserts
        .iter()
        .find(|x| x.name == "EQ")
        .cloned()
        .unwrap()
}

fn preset(s: &Studio, t: TrackId) -> Option<&'static str> {
    preset_of(&eq(s, t))
}

fn list() -> Vec<&'static str> {
    presets(xirius_params::find("eq").unwrap())
}

#[test]
fn alt_page_keys_load_the_next_and_the_previous_preset_round_the_list() {
    let (mut s, g, t, _) = keys_window();
    let list = list();
    let last = list.last().copied();
    assert_eq!(preset(&s, t), Some(list[0]));
    let before = s.song().steps();
    assert!(key(&mut s, &g, "pagedown", alt()));
    assert_eq!(preset(&s, t), Some(list[1]));
    assert_eq!(s.song().steps(), before + 1, "one step a preset");
    assert!(key(&mut s, &g, "pageup", alt()));
    assert_eq!(preset(&s, t), Some(list[0]));
    // Before the first is the last, after the last the first.
    assert!(key(&mut s, &g, "pageup", alt()));
    assert_eq!(preset(&s, t), last);
    assert!(s.run_binding(&g, "plugin.preset.next"));
    assert_eq!(preset(&s, t), Some(list[0]));
    assert!(s.run_binding(&g, "plugin.preset.prev"));
    assert_eq!(preset(&s, t), last);
}

#[test]
fn from_a_turned_knob_the_preset_keys_step_from_its_preset_and_without_a_window_do_nothing() {
    let (mut s, g, t, id) = keys_window();
    let def = eq(&s, t).builtin().unwrap();
    let (gain, _) = def.param("output").unwrap();
    let (x, y) = s
        .instrument_center(&g, id, Part::Knob(gain))
        .expect("in sight");
    assert!(s.instrument_down(&g, x, y));
    assert!(s.instrument_moved(&g, x, y + 30.0, false));
    assert!(s.instrument_up());
    assert_eq!(preset(&s, t), None);
    // The tag still names the first; the one before it is the last (UI-332).
    assert!(key(&mut s, &g, "pageup", alt()));
    assert_eq!(preset(&s, t), list().last().copied());
    // Off the last, the next is the first again.
    assert!(s.instrument_down(&g, x, y));
    assert!(s.instrument_moved(&g, x, y + 30.0, false));
    assert!(s.instrument_up());
    assert!(key(&mut s, &g, "pagedown", alt()));
    assert_eq!(preset(&s, t), Some(list()[0]));
    // The window shut, the keys have nothing to load into.
    s.instruments.open.clear();
    let steps = s.song().steps();
    assert!(!key(&mut s, &g, "pagedown", alt()));
    assert!(!s.run_binding(&g, "plugin.preset.next"));
    assert_eq!(s.song().steps(), steps);
}

#[test]
fn the_four_commands_are_in_the_table_with_no_arguments_and_a_name() {
    let s = Studio::sample();
    for (name, label) in [
        ("mixer.channel.next", "次のチャンネル"),
        ("mixer.channel.prev", "前のチャンネル"),
        ("plugin.preset.next", "次のプリセット"),
        ("plugin.preset.prev", "前のプリセット"),
    ] {
        let def = s.song.registry().get(name).expect(name);
        assert!(def.args.is_empty(), "{name}");
        assert_eq!(xirius_core::commands::labels::label(name), Some(label));
    }
}
