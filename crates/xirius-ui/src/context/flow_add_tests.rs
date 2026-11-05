//! Layers, splitters and effects added to a track (MIX-044) and the insert heading's menu
//! (MIX-045), pressed as the window presses them.

use serde_json::json;
use xirius_core::clipboard::Clipboard;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::rack::RackNode;

use super::tests::{choose, enabled, key, labels};
use crate::channel::flow::{CardKind, ColKind};
use crate::mixer::MixerHit;
use crate::mixer::flow::{Area, Kind};
use crate::popup::PopupKind;
use crate::studio::{DrawerTab, Geometry, Studio};
use crate::tree::RowKind;

fn mix() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    s.drawer_size.mixer = Some(600.0);
    let g = s.geometry(1600.0, 1000.0);
    s.resized(&g);
    (s, g)
}

fn track(s: &Studio, name: &str) -> TrackId {
    crate::arrange::probe::track(s, name)
}

fn add_effect(s: &mut Studio, t: TrackId, name: &str) -> PluginId {
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": name, "kind": "effect", "format": "builtin", "uid": name }),
    ));
    s.project().tracks[&t].inserts.last().unwrap().id
}

fn add_instrument(s: &mut Studio, t: TrackId) {
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": "Sampler", "kind": "instrument", "format": "builtin", "uid": "sampler" }),
    ));
}

/// Right-presses at the middle of `hit`.
fn press(s: &mut Studio, g: &Geometry, hit: MixerHit) -> bool {
    let r = s.mixer_rect(g, hit).expect("on screen");
    // a heading is pressed left of its ＋
    let (x, y) = match hit {
        MixerHit::Heading(..) => (r.x + 12.0, r.y + r.h / 2.0),
        _ => (r.x + r.w / 2.0, r.y + r.h / 2.0),
    };
    assert_eq!(s.mixer_hit(g, x, y), Some(hit));
    s.scene_context(g, x, y)
}

fn press_name(s: &mut Studio, g: &Geometry, t: TrackId) -> bool {
    let r = s
        .mixer_strips(g)
        .into_iter()
        .find(|x| x.track == t)
        .expect("a strip")
        .name;
    s.scene_context(g, r.x + r.w / 2.0, r.y + r.h / 2.0)
}

fn close(s: &mut Studio) {
    s.popup = None;
    s.ctx = None;
}

/// What the strip's flow shows, without the headings.
fn rows(s: &Studio, g: &Geometry, t: TrackId) -> Vec<Kind> {
    let strip = s
        .mixer_strips(g)
        .into_iter()
        .find(|x| x.track == t)
        .unwrap();
    s.strip_flow(&strip, &s.project().tracks[&t])
        .rows
        .iter()
        .map(|(l, _)| l.kind)
        .filter(|k| !matches!(k, Kind::Head { .. }))
        .collect()
}

/// The channel editor's picture of the track: its splitter cards and its columns.
fn editor(s: &Studio, t: TrackId) -> (usize, Vec<ColKind>) {
    let p = s.project();
    let f = crate::channel::flow::layout(&p, &p.tracks[&t], (0.0, 0.0));
    let splits = f
        .cards
        .iter()
        .filter(|c| matches!(c.kind, CardKind::Split { .. }))
        .count();
    (splits, f.columns.iter().map(|c| c.kind).collect())
}

const ADDS: [&str; 4] = [
    "レイヤーを追加…",
    "エフェクトを追加…",
    "スプリッターを追加",
    "チャンネルエディターを開く",
];

#[test]
fn the_insert_heading_menu_has_its_rows_in_order() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    assert!(press(&mut s, &g, MixerHit::Heading(vocal, Area::Insert)));
    assert_eq!(
        labels(&s),
        [
            "インサートをコピー",
            "インサートを貼り付け",
            "インサートを後ろに貼り付け",
            "エフェクトを追加…",
            "スプリッターを追加",
            "チャンネルの設定をコピー",
            "チャンネルの設定を貼り付け",
        ]
    );
    assert_eq!(key(&s, "インサートをコピー"), "Ctrl+C");
    assert_eq!(key(&s, "インサートを貼り付け"), "Ctrl+V");
    assert!(enabled(&s, "インサートをコピー"));
    assert!(!enabled(&s, "インサートを貼り付け"));
    assert!(!enabled(&s, "インサートを後ろに貼り付け"));
}

#[test]
fn the_heading_menu_copies_and_pastes_the_inserts() {
    let (mut s, g) = mix();
    let (vocal, guitar) = (track(&s, "Vocal"), track(&s, "Guitar"));
    add_effect(&mut s, vocal, "Compressor");
    add_effect(&mut s, guitar, "Delay");
    assert!(press(&mut s, &g, MixerHit::Heading(vocal, Area::Insert)));
    choose(&mut s, &g, "インサートをコピー");
    assert!(matches!(
        s.song.session().state.clipboard,
        Some(Clipboard::Inserts(ref c)) if c.source == vocal
    ));
    // on the source itself there is nothing to paste
    assert!(press(&mut s, &g, MixerHit::Heading(vocal, Area::Insert)));
    assert!(!enabled(&s, "インサートを貼り付け"));
    close(&mut s);
    assert!(press(&mut s, &g, MixerHit::Heading(guitar, Area::Insert)));
    assert!(enabled(&s, "インサートを後ろに貼り付け"));
    assert!(choose(&mut s, &g, "インサートを後ろに貼り付け"));
    let names: Vec<_> = s.project().tracks[&guitar]
        .inserts
        .iter()
        .map(|p| p.name.clone())
        .collect();
    assert_eq!(names, ["Delay", "Compressor"]);
    assert_eq!(s.notice_text(), Some("インサートを後ろに貼り付けました"));
}

#[test]
fn the_head_the_name_and_the_rows_add_with_the_same_words() {
    let (mut s, g) = mix();
    let keys = track(&s, "Keys");
    let vocal = track(&s, "Vocal");
    let comp = add_effect(&mut s, vocal, "Compressor");
    // the strip's name
    assert!(press_name(&mut s, &g, keys));
    let got = labels(&s);
    let at = got.iter().position(|l| l == ADDS[0]).expect("the adds");
    assert_eq!(got[at..at + 4], ADDS);
    assert!(enabled(&s, "レイヤーを追加…"));
    close(&mut s);
    // an audio track has no layers
    assert!(press_name(&mut s, &g, vocal));
    let got = labels(&s);
    let at = got.iter().position(|l| l == ADDS[1]).expect("the adds");
    assert_eq!(got[at..at + 3], ADDS[1..]);
    assert!(!got.iter().any(|l| l == ADDS[0]));
    close(&mut s);
    // the instrument heading
    assert!(press(&mut s, &g, MixerHit::Heading(keys, Area::Instrument)));
    assert_eq!(labels(&s), ADDS);
    close(&mut s);
    // an effect's row adds after itself, then the same rows (an audio track has no layers)
    assert!(press(&mut s, &g, MixerHit::Open(vocal, comp)));
    let got = labels(&s);
    let at = got
        .iter()
        .position(|l| l == "エフェクトを後ろに追加…")
        .expect("after");
    assert_eq!(got[at + 1], "スプリッターを後ろに追加");
    assert_eq!(got[at + 2..at + 5], ADDS[1..]);
    assert!(!got.iter().any(|l| l.contains("足す")));
    close(&mut s);
    // the track's head in the arrangement, scrolled to the top of the column
    s.scroll.1 = s.tree().row_of(RowKind::Track(keys)).unwrap().y;
    let (x, y) = crate::arrange::probe::row_mid(&s, &g, RowKind::Track(keys));
    assert!(s.scene_context(&g, x, y));
    let got = labels(&s);
    let at = got.iter().position(|l| l == ADDS[0]).expect("the adds");
    assert_eq!(got[at..at + 4], ADDS);
}

#[test]
fn the_plus_of_both_headings_shows_without_the_pointer() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    let keys = track(&s, "Keys");
    s.hover = crate::studio::Hit::None;
    assert!(s.mixer_rect(&g, MixerHit::AddInsert(vocal)).is_some());
    assert!(
        s.mixer_rect(&g, MixerHit::AddInstrument(keys)).is_some()
            || s.mixer_rect(&g, MixerHit::AddLayer(keys)).is_some()
    );
    // and the screen reader names them
    let marks = crate::a11y::marks(&s, &g);
    let name = |hit: MixerHit| {
        marks
            .iter()
            .find(|m| m.key == format!("flow-{vocal}-{hit:?}"))
            .and_then(|m| m.node.label().map(str::to_string))
    };
    assert_eq!(
        name(MixerHit::AddInsert(vocal)).as_deref(),
        Some("エフェクトを追加")
    );
    assert_eq!(
        name(MixerHit::Heading(vocal, Area::Insert)).as_deref(),
        Some("インサート")
    );
    add_instrument(&mut s, keys);
    let marks = crate::a11y::marks(&s, &g);
    let layer = marks
        .iter()
        .find(|m| m.key == format!("flow-{keys}-{:?}", MixerHit::AddLayer(keys)))
        .expect("the layer ＋");
    assert_eq!(layer.node.label(), Some("レイヤーを追加"));
}

#[test]
fn the_insert_list_starts_with_the_splitter_and_its_keys_work() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    assert!(s.mixer_press(&g, MixerHit::AddInsert(vocal)));
    assert!(matches!(
        s.popup.as_ref().map(|p| p.kind),
        Some(PopupKind::Insert(t)) if t == vocal
    ));
    let first = s.popup.as_ref().unwrap().choices()[0].clone();
    assert_eq!(first.to, crate::mixer::scan::SPLITTER);
    assert_eq!(first.label, "Splitter");
    // words that match nothing
    assert!(s.fx_typed("q", Some("qqqqqq")));
    let list = s.popup.as_ref().unwrap().pick.as_ref().unwrap();
    assert!(list.visible().is_empty());
    assert_eq!(list.empty, "一致する項目はありません");
    // Esc: the words go, then the list
    assert!(s.fx_typed("escape", None));
    assert!(s.popup.is_some());
    assert!(
        s.popup
            .as_ref()
            .unwrap()
            .pick
            .as_ref()
            .unwrap()
            .query
            .is_empty()
    );
    assert!(s.fx_typed("escape", None));
    assert!(s.popup.is_none());
    // ↓ then ↑ comes back to the splitter; Enter adds it
    assert!(s.mixer_press(&g, MixerHit::AddInsert(vocal)));
    assert!(s.fx_typed("down", None));
    assert!(s.fx_typed("up", None));
    assert!(s.fx_typed("enter", None));
    assert!(
        rows(&s, &g, vocal)
            .iter()
            .any(|k| matches!(k, Kind::Split { .. }))
    );
}

#[test]
fn a_splitter_and_a_branch_added_grow_the_strip_and_the_editor_alike() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    add_effect(&mut s, vocal, "EQ");
    assert_eq!(editor(&s, vocal).0, 0);
    assert!(press(&mut s, &g, MixerHit::Heading(vocal, Area::Insert)));
    assert!(choose(&mut s, &g, "スプリッターを追加"));
    let id = match s.project().tracks[&vocal].rack.as_ref().unwrap().fx.last() {
        Some(RackNode::Split(sp)) => sp.id,
        other => panic!("{other:?}"),
    };
    let branches = |s: &Studio| {
        editor(s, vocal)
            .1
            .iter()
            .filter(|c| matches!(c, ColKind::Branch { .. }))
            .count()
    };
    assert_eq!(editor(&s, vocal).0, 1);
    assert_eq!(branches(&s), 2);
    assert!(
        rows(&s, &g, vocal)
            .iter()
            .any(|k| matches!(k, Kind::Split { id: x, .. } if *x == id))
    );
    // the splitter's row adds a branch
    assert!(press(&mut s, &g, MixerHit::Fold(vocal, id)));
    let got = labels(&s);
    assert_eq!(got[0], "分岐を追加");
    assert_eq!(
        got[1..3],
        ["エフェクトを後ろに追加…", "スプリッターを後ろに追加"]
    );
    assert!(choose(&mut s, &g, "分岐を追加"));
    assert_eq!(branches(&s), 3);
    // the strip's splitter row says three as well
    assert!(
        rows(&s, &g, vocal)
            .iter()
            .any(|k| matches!(k, Kind::Split { id: x, branches: 3, .. } if *x == id))
    );
}

#[test]
fn a_layer_added_from_the_menu_grows_the_strip_and_the_editor() {
    let (mut s, g) = mix();
    let keys = track(&s, "Keys");
    add_instrument(&mut s, keys);
    let layers = |s: &Studio| {
        editor(s, keys)
            .1
            .iter()
            .filter(|c| matches!(c, ColKind::Layer { .. }))
            .count()
    };
    let before = layers(&s);
    assert!(press(&mut s, &g, MixerHit::Heading(keys, Area::Instrument)));
    assert!(choose(&mut s, &g, "レイヤーを追加…"));
    assert!(matches!(
        s.popup.as_ref().map(|p| p.kind),
        Some(PopupKind::Layer(t)) if t == keys
    ));
    let to = s
        .popup
        .as_ref()
        .unwrap()
        .choices()
        .into_iter()
        .map(|c| c.to)
        .find(|to| !to.is_empty() && to != crate::mixer::scan::RESCAN)
        .expect("an instrument to add");
    s.popup = None;
    s.mixer_pick(PopupKind::Layer(keys), &to);
    assert!(layers(&s) > before.max(1), "{} → {}", before, layers(&s));
    let strip_layers = crate::mixer::flow::lines(&s.project().tracks[&keys], &s.mixer.folds, false)
        .iter()
        .filter(|l| l.target.is_some_and(|x| x.layer))
        .count();
    assert_eq!(strip_layers, layers(&s));
}

#[test]
fn the_channel_editor_opens_from_the_menu() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    assert!(press_name(&mut s, &g, vocal));
    assert!(choose(&mut s, &g, "チャンネルエディターを開く"));
    assert!(s.channel.as_ref().is_some_and(|w| w.track == vocal));
}
