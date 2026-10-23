use std::path::PathBuf;
use std::sync::Arc;

use xirius_plugins::Entry;

use super::*;
use crate::mixer::MixerHit;
use crate::popup::{Action, Look, PopupKind};
use crate::studio::{DrawerTab, Geometry, Hit};

/// The sample song with the mixer open. Its instrument tracks have no instrument yet.
fn studio() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    let g = s.geometry(1600.0, 1000.0);
    (s, g)
}

fn track(s: &Studio, name: &str) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == name)
        .map(|t| t.id)
        .unwrap_or_else(|| panic!("no track {name}"))
}

fn press(s: &mut Studio, g: &Geometry, hit: MixerHit) -> bool {
    let (x, y) = s.mixer_center(g, hit).expect("on screen");
    assert_eq!(s.hit(g, x, y), Hit::Mixer(hit));
    s.press(g, Hit::Mixer(hit))
}

fn instrument(s: &Studio, t: TrackId) -> Option<Plugin> {
    current(&s.project().tracks[&t]).cloned()
}

fn a_synth() -> Catalog {
    Catalog {
        entries: vec![Entry {
            uid: "FEDCBA9876543210FEDCBA9876543210".into(),
            name: "Pad Synth".into(),
            vendor: "Test".into(),
            kind: PluginKind::Instrument,
            format: "vst3".into(),
            category: "Instrument|Synth".into(),
            version: "1.0".into(),
            path: PathBuf::from("C:/nowhere/Pad Synth.vst3"),
            ara: false,
        }],
        failures: Vec::new(),
    }
}

#[test]
fn an_instrument_track_without_an_instrument_shows_the_pick_row() {
    let (s, g) = studio();
    let t = track(&s, "Keys");
    assert!(instrument(&s, t).is_none());
    assert_eq!(gap(&s.project().tracks[&t]), 1);
    let (x, y) = s
        .mixer_center(&g, MixerHit::AddInstrument(t))
        .expect("on screen");
    assert_eq!(s.hit(&g, x, y), Hit::Mixer(MixerHit::AddInstrument(t)));
    // An audio track has no such row.
    let vocal = track(&s, "Vocal");
    assert!(s.mixer_center(&g, MixerHit::AddInstrument(vocal)).is_none());
}

#[test]
fn the_pick_row_lists_the_built_in_instruments_then_the_scan() {
    let (mut s, g) = studio();
    s.mixer.catalog = Some(Arc::new(a_synth()));
    let t = track(&s, "Keys");
    assert!(press(&mut s, &g, MixerHit::AddInstrument(t)));
    let popup = s.popup.as_ref().expect("the instrument list");
    assert_eq!(popup.kind, PopupKind::Instrument(t));
    let cs = popup.choices();
    let labels: Vec<&str> = cs.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Sampler",
            "Drum Kit",
            "Pad Synth — Test",
            "プラグインを再スキャン"
        ]
    );
    assert_eq!(
        popup.choices().last().map(|c| c.to.as_str()),
        Some(crate::mixer::scan::RESCAN)
    );
    // Effects are not in it.
    assert!(!labels.iter().any(|l| l.starts_with("Comp")));
}

#[test]
fn picking_puts_the_instrument_in_and_replacing_keeps_its_place() {
    let (mut s, g) = studio();
    s.mixer.catalog = Some(Arc::new(a_synth()));
    let t = track(&s, "Keys");
    let before = s.song().steps();
    assert!(press(&mut s, &g, MixerHit::AddInstrument(t)));
    s.popup_action(&g, Action::Pick("builtin:Sampler".into()));
    assert!(s.popup.is_none());
    assert_eq!(s.song().steps(), before + 1);
    let first = instrument(&s, t).expect("an instrument");
    assert_eq!(
        (first.name.as_str(), first.format.as_str()),
        ("Sampler", "builtin")
    );
    // The ＋ row is gone; the instrument row is where it was.
    assert!(s.mixer_center(&g, MixerHit::AddInstrument(t)).is_none());
    assert!(s.mixer_center(&g, MixerHit::Open(t, first.id)).is_some());

    // Replacing: the scanned synth takes its place, in one step.
    assert!(s.pick_instrument(t, "FEDCBA9876543210FEDCBA9876543210"));
    let second = instrument(&s, t).expect("an instrument");
    assert_eq!(
        (second.name.as_str(), second.format.as_str()),
        ("Pad Synth", "vst3")
    );
    assert_eq!(s.project().tracks[&t].inserts.len(), 1);
    assert!(s.undo(&g));
    assert_eq!(instrument(&s, t).unwrap().name, "Sampler");
}

#[test]
fn removing_the_instrument_brings_the_pick_row_back() {
    let (mut s, g) = studio();
    let t = track(&s, "Keys");
    assert!(s.set_instrument(t, &Pick::built_in("Drum Kit")));
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": "EQ" })
    ));
    assert!(s.remove_instrument(t));
    let track_now = &s.project().tracks[&t];
    assert_eq!(track_now.inserts.len(), 1, "the effect stays");
    assert_eq!(gap(track_now), 1);
    assert!(s.mixer_center(&g, MixerHit::AddInstrument(t)).is_some());
    // The effect sits under the ＋ row.
    let eq = track_now.inserts[0].id;
    let (_, plus) = s.mixer_center(&g, MixerHit::AddInstrument(t)).unwrap();
    let (_, fx) = s.mixer_center(&g, MixerHit::Open(t, eq)).unwrap();
    assert!(fx > plus);
    // Undo puts it back.
    assert!(s.undo(&g));
    assert_eq!(instrument(&s, t).unwrap().name, "Drum Kit");
    // Nothing to take out: nothing happens.
    let vocal = track(&s, "Vocal");
    assert!(!s.remove_instrument(vocal));
}

#[test]
fn a_name_from_the_browser_resolves_to_its_plugin() {
    let catalog = a_synth();
    assert_eq!(
        resolve(&catalog, "Sampler"),
        Some(Pick::built_in("Sampler"))
    );
    assert_eq!(resolve(&catalog, "Pad Synth").unwrap().format, "vst3");
    assert_eq!(resolve(&catalog, "No Such Synth"), None);
    let (mut s, _) = studio();
    let t = track(&s, "Keys");
    assert!(!s.pick_instrument(t, "builtin-gone"));
    assert!(s.error.is_some());
}

/// `n` instruments of a few makers, some with long names.
fn many_synths(n: usize) -> Catalog {
    let makers = ["Native Instruments", "Arturia", "Spitfire Audio", "u-he"];
    Catalog {
        entries: (0..n)
            .map(|i| Entry {
                uid: format!("{i:032X}"),
                name: if i % 3 == 0 {
                    format!("Orchestral Strings Ensemble Long Name {i}")
                } else {
                    format!("Synth {i}")
                },
                vendor: makers[i % makers.len()].into(),
                kind: PluginKind::Instrument,
                format: "vst3".into(),
                category: "Instrument|Synth".into(),
                version: "1.0".into(),
                path: PathBuf::from(format!("C:/nowhere/{i}.vst3")),
                ara: false,
            })
            .collect(),
        failures: Vec::new(),
    }
}

#[test]
fn ui_297_the_instrument_list_keeps_its_width_and_says_a_cut_name_in_full() {
    let (mut s, g) = studio();
    s.mixer.catalog = Some(Arc::new(many_synths(400)));
    let t = track(&s, "Keys");
    assert!(press(&mut s, &g, MixerHit::AddInstrument(t)));
    let layout = s.popup.as_ref().unwrap().layout(g.size);
    assert_eq!(layout.rect.w, 260.0);
    // The search box on top, as in the effects' list, then twelve rows.
    let input = &layout.cells[0];
    assert_eq!(
        (input.look.clone(), input.icon),
        (Look::Input, Some("search"))
    );
    let rows: Vec<_> = layout
        .cells
        .iter()
        .filter(|c| c.look == Look::Row && !c.foot)
        .collect();
    assert_eq!(
        rows.len(),
        s.popup
            .as_ref()
            .unwrap()
            .pick
            .as_ref()
            .unwrap()
            .capacity(g.size)
    );
    assert!(rows.iter().all(|c| !c.foot));
    assert!(rows.iter().all(|c| c.icon == Some("keys")));
    assert!(rows.iter().all(|c| c.rect.right() <= layout.rect.right()));
    // A long name is cut in the row (its text stays whole) and said in full in the balloon;
    // a short one has no balloon.
    let long = rows
        .iter()
        .find(|c| c.text.starts_with("Orchestral"))
        .expect("a long name");
    assert_eq!(long.tip, long.text);
    let short = rows.iter().find(|c| c.text == "Sampler").unwrap();
    assert!(short.tip.is_empty());
    let (x, y) = (long.rect.x + 40.0, long.rect.y + long.rect.h / 2.0);
    assert!(
        layout
            .item_at(x, y)
            .is_some_and(|i| layout.cells[i].tip == long.text)
    );
}

#[test]
fn ui_297_the_instrument_list_shows_a_thumb_when_more_rows_than_show() {
    let (mut s, g) = studio();
    s.mixer.catalog = Some(Arc::new(many_synths(400)));
    let t = track(&s, "Keys");
    assert!(press(&mut s, &g, MixerHit::AddInstrument(t)));
    // Where the thumb is in the pop-over.
    let thumb = |s: &Studio| {
        let layout = s.popup.as_ref().unwrap().layout(g.size);
        let o = layout.rect;
        layout
            .cells
            .into_iter()
            .find(|c| c.look == Look::Thumb)
            .map(|c| Rect::new(c.rect.x - o.x, c.rect.y - o.y, c.rect.w, c.rect.h))
    };
    let pad = xirius_design::Layout::POPUP_PAD;
    let row_h = xirius_design::Layout::MENU_ROW_H;
    // The rows' part keeps its height at any scroll (UI-338).
    let h = s
        .popup
        .as_ref()
        .unwrap()
        .pick
        .as_ref()
        .unwrap()
        .body_h(g.size);
    let n = s
        .popup
        .as_ref()
        .unwrap()
        .pick
        .as_ref()
        .unwrap()
        .visible()
        .len();
    // At the top: 4 px wide, 1 px from the rows' right edge, as tall as the share that shows.
    let top = thumb(&s).expect("a thumb");
    assert_eq!((top.w, top.right()), (4.0, 260.0 - pad - 1.0));
    assert_eq!(top.y, pad + row_h);
    assert_eq!(
        top.h,
        (h * s
            .popup
            .as_ref()
            .unwrap()
            .pick
            .as_ref()
            .unwrap()
            .capacity(g.size) as f32
            / n as f32)
            .max(16.0)
    );
    // Sent to the end, it reaches the bottom of the rows.
    let end = s.popup.as_ref().unwrap().max_offset(g.size);
    s.popup.as_mut().unwrap().pick.as_mut().unwrap().offset = end;
    let bottom = thumb(&s).unwrap();
    assert!((bottom.bottom() - (pad + row_h + h)).abs() < 0.01);
    // Narrowed to few enough to show at once (Synth 391, 392, 394, …): no thumb.
    for ch in "synth 39".chars() {
        let ch = ch.to_string();
        assert!(s.fx_typed(&ch, Some(&ch)));
    }
    let p = s.popup.as_ref().unwrap();
    assert_eq!(
        (
            p.pick.as_ref().unwrap().offset,
            crate::pick_list::found(p).count()
        ),
        (0, 6)
    );
    assert_eq!(thumb(&s), None);
}

#[test]
fn ui_297_typing_narrows_by_name_and_maker_and_the_keys_pick() {
    let (mut s, g) = studio();
    s.mixer.catalog = Some(Arc::new(many_synths(40)));
    let t = track(&s, "Keys");
    assert!(press(&mut s, &g, MixerHit::AddInstrument(t)));
    // By maker: 「u-he」 makes every fourth.
    for ch in ["u", "-", "H", "e"] {
        assert!(s.fx_typed(ch, Some(ch)));
    }
    let p = s.popup.as_ref().unwrap();
    let found: Vec<String> = crate::pick_list::found(p)
        .map(|c| c.label.clone())
        .collect();
    assert_eq!(found.len(), 10, "{found:?}");
    assert!(found.iter().all(|l| l.ends_with("— u-he")));
    assert!(
        p.layout(g.size).cells.iter().all(|c| c.look != Look::Thumb),
        "ten show at once"
    );
    // By name: 「synth 1」 is Synth 1 and Synth 10 to 19 (some of which are long).
    for _ in 0..4 {
        assert!(s.fx_typed("backspace", None));
    }
    for ch in "synth 1".chars() {
        let ch = ch.to_string();
        assert!(s.fx_typed(&ch, Some(&ch)));
    }
    let p = s.popup.as_ref().unwrap();
    let found: Vec<String> = crate::pick_list::found(p)
        .map(|c| c.label.clone())
        .collect();
    assert!(!found.is_empty());
    assert!(found.iter().all(|l| l.to_lowercase().contains("synth 1")));
    // ↓ chooses the next, ↑ goes back, Enter puts the chosen one in.
    assert!(s.fx_typed("down", None));
    assert!(s.fx_typed("down", None));
    assert!(s.fx_typed("up", None));
    let chosen = found[1].split(" — ").next().unwrap().to_string();
    let before = s.song().steps();
    assert!(s.fx_typed("enter", None));
    assert!(s.popup.is_none());
    assert_eq!(s.song().steps(), before + 1);
    assert_eq!(instrument(&s, t).expect("an instrument").name, chosen);
}

#[test]
fn ui_297_enter_on_a_line_with_nothing_to_pick_does_nothing() {
    let (mut s, g) = studio();
    s.mixer.catalog = Some(Arc::new(a_synth()));
    let t = track(&s, "Keys");
    assert!(press(&mut s, &g, MixerHit::AddInstrument(t)));
    s.popup
        .as_mut()
        .unwrap()
        .pick
        .as_mut()
        .unwrap()
        .groups
        .insert(
            0,
            crate::pick_list::PickGroup::new(
                "",
                vec![crate::pick_list::PickRow::new("スキャン中").note()],
            ),
        );
    let notes = s.popup.as_ref().unwrap().layout(g.size).cells;
    assert_eq!(notes[1].look, Look::Note);
    let before = s.song().steps();
    assert!(s.fx_typed("enter", None));
    assert_eq!(s.song().steps(), before);
    assert_eq!(s.error, None);
}

#[test]
fn ui_305_the_strip_row_shows_the_rails_icon_alone_in_the_middle() {
    use gpui::AssetSource;
    let (s, g) = studio();
    let keys = track(&s, "Keys");
    let row = s
        .mixer_rect(&g, MixerHit::AddInstrument(keys))
        .expect("on screen");
    // The word does not fit the strip, which is why it is not painted.
    let room = row.w - 2.0 * 6.0;
    assert!(
        crate::browser::text_width(PICK_LABEL, xirius_design::FontSize::Caption) > room,
        "the word is wider than the strip"
    );
    let (name, icon) = empty_icon(row);
    assert_eq!(name, crate::browser::RailTab::Instruments.icon());
    assert!(
        crate::assets::Assets
            .load(&format!("icons/{name}.svg"))
            .unwrap()
            .is_some()
    );
    assert_eq!(
        (icon.x + icon.w / 2.0, icon.y + icon.h / 2.0),
        (row.x + row.w / 2.0, row.y + row.h / 2.0)
    );
    assert!(icon.x >= row.x && icon.right() <= row.right());
    assert!(icon.y >= row.y && icon.bottom() <= row.bottom());
    // Nothing is written on the row.
    let source = include_str!("mod.rs");
    let body = source
        .split("pub fn paint_empty")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("paint_empty");
    assert!(!body.contains(".text("), "the row paints no text");
}

#[test]
fn ui_305_the_balloon_and_the_screen_reader_say_pick_an_instrument() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    assert_eq!(PICK_LABEL, "インストゥルメントを選ぶ");
    assert_eq!(
        crate::hover_tip::text(&s, Hit::Mixer(MixerHit::AddInstrument(keys))).as_deref(),
        Some("インストゥルメントを選ぶ")
    );
    let marks = crate::a11y::marks(&s, &g);
    let m = marks
        .iter()
        .find(|m| m.pick == Some(keys))
        .expect("a button over the row");
    assert_eq!(m.node.role(), gpui::accesskit::Role::Button);
    assert_eq!(m.node.label(), Some("インストゥルメントを選ぶ"));
    // It lies over the row the pointer presses.
    let b = m.node.bounds().expect("bounds");
    let row = s
        .mixer_rect(&g, MixerHit::AddInstrument(keys))
        .expect("on screen");
    assert_eq!(
        (b.x0 as f32, b.width() as f32, b.height() as f32),
        (row.x, row.w, row.h)
    );
    // An audio track has none; a track that has its instrument has none.
    let vocal = track(&s, "Vocal");
    assert!(marks.iter().all(|m| m.pick != Some(vocal)));
    // Its press does what the pointer's press does: the instrument list opens.
    assert!(crate::a11y::pick(&mut s, &g, keys));
    assert_eq!(
        s.popup.as_ref().map(|p| p.kind),
        Some(PopupKind::Instrument(keys))
    );
    s.popup = None;
    assert!(s.set_instrument(keys, &Pick::built_in("Sampler")));
    assert!(
        crate::a11y::marks(&s, &g)
            .iter()
            .all(|m| m.pick != Some(keys))
    );
}
