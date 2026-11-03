//! ARA on the clips, pressed as the window presses them: the rows of 「処理」, the list that
//! picks the plugin, the badge after the name and what a press on it does.

use std::sync::Arc;

use xirius_core::model::PluginKind;
use xirius_plugins::{Catalog, Entry};

use super::*;
use crate::arrange::probe::{clip_mid, track};
use crate::studio::Hit;
use crate::tools::Mods;

/// The test plugin: registered with the session, and in the screen's catalog under its maker.
const UID: &str = "ABCDEF0123456789ABCDEF0123456789";
const NAME: &str = "Test ARA";

fn never() -> xirius_session::ara::Loader {
    Box::new(|| unreachable!("the screen's tests open no document"))
}

fn studio() -> (Studio, Geometry) {
    xirius_session::ara::register(UID, NAME, Arc::new(never));
    let mut s = Studio::sample();
    s.drawer_open = false;
    let g = s.geometry(1440.0, 1000.0);
    s.resized(&g);
    s.mixer.catalog = Some(Arc::new(Catalog {
        entries: vec![Entry {
            uid: UID.into(),
            name: NAME.into(),
            vendor: "Test".into(),
            kind: PluginKind::Effect,
            format: "vst3".into(),
            category: "Fx".into(),
            version: "1.0".into(),
            path: "test.vst3".into(),
            ara: true,
            requires_process_main: false,
        }],
        failures: Vec::new(),
    }));
    (s, g)
}

fn vocal(s: &Studio) -> (TrackId, ClipId) {
    let t = track(s, "Vocal");
    (t, s.project().tracks[&t].clips[0].id)
}

fn attach(s: &mut Studio, clip: ClipId) {
    s.song
        .run(
            "clip.ara.attach.apply",
            json!({ "ids": [clip.to_string()], "plugin": UID, "name": NAME }),
        )
        .unwrap();
}

fn report(clip: ClipId, phase: Phase, progress: f32) -> Report {
    Report {
        plugin: UID.into(),
        name: NAME.into(),
        loaded: true,
        crashed: None,
        failed: None,
        clips: 1,
        working: usize::from(phase != Phase::Ready),
        progress,
        phases: vec![(clip.0, phase)],
        editing: false,
    }
}

fn labels(s: &Studio) -> Vec<String> {
    s.context_rows()
        .into_iter()
        .filter(|r| !r.gap)
        .map(|r| r.label)
        .collect()
}

fn row(s: &Studio, label: &str) -> crate::context::Row {
    s.context_rows()
        .into_iter()
        .find(|r| r.label == label)
        .unwrap_or_else(|| panic!("no row {label}: {:?}", labels(s)))
}

/// A right press on the Vocal clip, then 「処理」.
fn processing(s: &mut Studio, g: &Geometry) {
    s.popup = None;
    let (x, y) = clip_mid(s, g, "Vocal", 0);
    assert!(s.scene_context(g, x, y));
    choose(s, g, "処理");
}

/// Presses the cell of the open menu's row named `label`.
fn choose(s: &mut Studio, g: &Geometry, label: &str) -> bool {
    let layout = s.popup.as_ref().expect("a menu is open").layout(g.size);
    let cell = layout
        .cells
        .iter()
        .find(|c| c.text == label)
        .unwrap_or_else(|| panic!("no cell {label}"));
    let hit = s.hit(
        g,
        cell.rect.x + cell.rect.w / 2.0,
        cell.rect.y + cell.rect.h / 2.0,
    );
    s.press(g, hit)
}

fn picking(s: &Studio) -> bool {
    s.popup
        .as_ref()
        .is_some_and(|p| p.kind == PopupKind::AraPlugin)
}

#[test]
fn processing_ends_with_the_attach_row_and_its_key_opens_the_list() {
    let (mut s, g) = studio();
    processing(&mut s, &g);
    let all = labels(&s);
    assert_eq!(all.last().map(String::as_str), Some(ADD), "{all:?}");
    let add = row(&s, ADD);
    assert!(add.enabled);
    assert_eq!(add.key, "Ctrl+M");
    // The row opens the list of the ARA plugins, under their makers.
    choose(&mut s, &g, ADD);
    assert!(picking(&s), "{:?}", s.error);
    let pick = s.popup.as_ref().unwrap().pick.clone().expect("a list menu");
    let headings: Vec<&str> = pick.groups.iter().map(|g| g.heading.as_str()).collect();
    assert_eq!(headings, ["Test"]);
    // Registered and in the catalog: one row.
    let rows: Vec<&str> = pick.groups[0]
        .rows
        .iter()
        .map(|r| r.label.as_str())
        .collect();
    assert_eq!(rows, [NAME]);
    // Esc clears the words first, then shuts the list.
    s.typed(&g, "x", Some("x"));
    assert!(
        s.popup
            .as_ref()
            .unwrap()
            .pick
            .as_ref()
            .unwrap()
            .visible()
            .is_empty()
    );
    s.typed(&g, "escape", None);
    assert!(picking(&s));
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
    s.typed(&g, "escape", None);
    assert!(s.popup.is_none());
    // Ctrl+M on the selected clip opens it too; one plugin is still picked, with Enter. As in
    // the app's window, where the session's own commands are the song owner's and refused.
    s.song.bind_window_resources();
    let (t, c) = vocal(&s);
    s.select_clip_here(t, c);
    s.mapped_key(&g, "m", None, Mods::ctrl());
    assert!(picking(&s), "{:?}", s.error);
    let n = s.song.steps();
    assert!(s.project().ara.clips.is_empty());
    s.typed(&g, "enter", None);
    assert!(s.popup.is_none());
    assert_eq!(s.song.steps(), n + 1);
    assert_eq!(s.project().ara.clips[&c].plugin, UID);
}

#[test]
fn a_clip_that_cannot_take_it_has_the_row_dim_with_why() {
    let (mut s, g) = studio();
    let (t, c) = vocal(&s);
    s.song
        .run(
            "clip.speed.set",
            json!({ "ids": [c.to_string()], "percent": 150 }),
        )
        .unwrap();
    processing(&mut s, &g);
    let add = row(&s, ADD);
    assert!(!add.enabled);
    assert_eq!(add.reason, CANNOT_ATTACH);
    // A dim row does nothing.
    choose(&mut s, &g, ADD);
    assert!(!picking(&s));
    // Frozen: the same row, with the freeze's reason.
    let (mut s, g) = studio();
    s.song
        .run(
            "track.freeze.apply",
            json!({ "tracks": [{ "id": t.to_string(), "file": "frozen.wav", "start": 0 }] }),
        )
        .unwrap();
    s.popup = None;
    let (x, y) = clip_mid(&s, &g, "Vocal", 0);
    assert!(s.scene_context(&g, x, y));
    let rows = s.context_rows();
    let sub = rows.iter().find(|r| r.label == "処理").unwrap();
    assert!(!sub.enabled);
    // The key still reaches it, and says why.
    s.popup = None;
    s.select_clip_here(t, c);
    s.mapped_key(&g, "m", None, Mods::ctrl());
    assert!(!picking(&s));
    assert_eq!(s.error.as_deref(), Some(crate::toast::FROZEN));
}

#[test]
fn an_attached_clip_edits_and_detaches_by_the_plugins_name() {
    let (mut s, g) = studio();
    let (t, c) = vocal(&s);
    attach(&mut s, c);
    processing(&mut s, &g);
    let edit = format!("{NAME}で編集");
    let detach = format!("{NAME}を外す");
    let all = labels(&s);
    assert!(!all.iter().any(|l| l == ADD), "{all:?}");
    assert_eq!(row(&s, &edit).key, "Ctrl+M");
    assert_eq!(row(&s, &detach).key, "Ctrl+Alt+M");
    // 編集 opens the plugin's window, for that clip.
    choose(&mut s, &g, &edit);
    assert_eq!(s.ara.opened, [(UID.to_string(), vec![c])]);
    // Ctrl+M on an attached clip is its window, not the list.
    s.popup = None;
    s.select_clip_here(t, c);
    s.mapped_key(&g, "m", None, Mods::ctrl());
    assert!(!picking(&s));
    assert_eq!(s.ara.opened.len(), 2);
    // A double click on the clip opens it too.
    let (x, y) = clip_mid(&s, &g, "Vocal", 0);
    assert!(s.arrange_open(&g, x, y, Mods::NONE));
    assert_eq!(s.ara.opened.len(), 3);
    // 外す is one step, and the clip has no badge after it.
    processing(&mut s, &g);
    let n = s.song.steps();
    choose(&mut s, &g, &detach);
    assert_eq!(s.song.steps(), n + 1);
    assert!(s.project().ara.clips.is_empty());
    assert_eq!(s.ara_badge(c), None);
    // Ctrl+Alt+M detaches the selected clip.
    attach(&mut s, c);
    s.popup = None;
    s.select_clip_here(t, c);
    s.mapped_key(
        &g,
        "m",
        None,
        Mods {
            ctrl: true,
            shift: false,
            alt: true,
        },
    );
    assert!(s.project().ara.clips.is_empty(), "{:?}", s.error);
}

#[test]
fn the_badge_follows_the_plugin_and_says_it() {
    let (mut s, g) = studio();
    let (t, c) = vocal(&s);
    assert_eq!(s.ara_badge(c), None);
    attach(&mut s, c);
    s.ara.fake = Some(Vec::new());
    // Not read yet: analysing from 0.
    assert_eq!(s.ara_badge(c), Some(Badge::Busy(0)));
    s.ara.fake = Some(vec![report(c, Phase::Analysing, 0.42)]);
    assert_eq!(s.ara_badge(c), Some(Badge::Busy(42)));
    assert!(s.poll_ara(), "a turning ring keeps the frames coming");
    assert_eq!(Badge::Busy(42).said(), "解析中 42%");
    s.ara.fake = Some(vec![report(c, Phase::Ready, 1.0)]);
    assert_eq!(s.ara_badge(c), Some(Badge::Ready));
    assert!(!s.poll_ara());
    assert_eq!(Badge::Ready.said(), "済み");
    let mut down = report(c, Phase::Ready, 1.0);
    down.crashed = Some("stopped".into());
    s.ara.fake = Some(vec![down]);
    assert_eq!(s.ara_badge(c), Some(Badge::Crashed));
    assert_eq!(Badge::Crashed.said(), "停止");
    let mut failed = report(c, Phase::Waiting, 0.0);
    failed.failed = Some("読み込めません".into());
    s.ara.fake = Some(vec![failed]);
    assert_eq!(s.ara_badge(c), Some(Badge::Failed("読み込めません".into())));
    // A screen reader hears it as a button, with the reason.
    let marks = crate::a11y::marks(&s, &g);
    let mark = marks
        .iter()
        .find(|m| m.key == format!("ara-{c}"))
        .expect("the badge's mark");
    assert_eq!(mark.node.role(), gpui::accesskit::Role::Button);
    assert_eq!(
        mark.node.label(),
        Some(format!("{NAME}: 開けません").as_str())
    );
    assert_eq!(mark.node.description(), Some("読み込めません"));
    assert_eq!(mark.press, Some(Hit::Ara(t, c)));
    // Not in the catalog any more: it cannot be opened, and says so.
    s.ara.fake = None;
    s.mixer.catalog = Some(Arc::new(Catalog::default()));
    xirius_session::ara::register(UID, NAME, Arc::new(never));
    let other = "00000000000000000000000000000001";
    s.song
        .run(
            "clip.ara.attach.apply",
            json!({ "ids": [c.to_string()], "plugin": other, "name": "Gone" }),
        )
        .unwrap();
    assert_eq!(s.ara_badge(c), Some(Badge::Failed(MISSING.into())));
}

#[test]
fn the_badge_sits_after_the_name_and_stays_when_the_name_is_cut() {
    let (mut s, g) = studio();
    let (t, c) = vocal(&s);
    attach(&mut s, c);
    s.ara.fake = Some(vec![report(c, Phase::Ready, 1.0)]);
    let at = s.ara_badge_at(&g, t, c).expect("on screen");
    let p = s.project();
    let tr = &p.tracks[&t];
    let clip = tr.clips.iter().find(|k| k.id == c).unwrap();
    let row = *s.tree().row_of(crate::tree::RowKind::Track(t)).unwrap();
    let r = s.clip_rect(&g, &row, clip);
    let name_w = crate::fonts::measure(&clip.name, FontSize::Caption, Weight::Medium);
    assert!(
        (at.x - (r.x + Space::X1.px() + name_w + GAP)).abs() < 0.5,
        "{at:?} {r:?}"
    );
    // The point on it is the badge's, not the clip's.
    let (x, y) = s.center_of(&g, Hit::Ara(t, c)).expect("the badge's middle");
    assert_eq!(s.hit(&g, x, y), Hit::Ara(t, c));
    // A clip too narrow for its name: the badge stays inside it, at the name's start.
    let narrow = Rect::new(r.x, r.y, BOX_W + 8.0, r.h);
    assert!(name_w + GAP + BOX_W > narrow.w);
    let at = s.ara_badge_rect(tr, clip, narrow, &Badge::Ready);
    assert!(
        at.x >= narrow.x && at.right() <= narrow.right(),
        "{at:?} {narrow:?}"
    );
}

#[test]
fn a_press_on_the_badge_opens_explains_or_offers_the_restart() {
    let (mut s, g) = studio();
    let (t, c) = vocal(&s);
    attach(&mut s, c);
    // Done: its window.
    s.ara.fake = Some(vec![report(c, Phase::Ready, 1.0)]);
    let (x, y) = s.center_of(&g, Hit::Ara(t, c)).unwrap();
    let hit = s.hit(&g, x, y);
    s.press(&g, hit);
    assert_eq!(s.ara.opened, [(UID.to_string(), vec![c])]);
    assert!(s.arrange.clips.contains(&(t, c)));
    // Cannot be opened: why, in a brief note, and no window.
    let mut failed = report(c, Phase::Waiting, 0.0);
    failed.failed = Some("読み込めません".into());
    s.ara.fake = Some(vec![failed]);
    let hit = s.hit(&g, x, y);
    s.press(&g, hit);
    assert_eq!(s.ara.opened.len(), 1);
    assert_eq!(
        s.toast_shown().map(|t| t.text.as_str()),
        Some("読み込めません")
    );
    // Went down: only the colour says so; the press opens the clip's menu with the restart
    // first, and the restart brings the plugin back.
    let mut down = report(c, Phase::Ready, 1.0);
    down.crashed = Some("stopped".into());
    s.ara.fake = Some(vec![down]);
    s.toast = None;
    s.poll_ara();
    assert!(s.toast_shown().is_none() && s.popup.is_none());
    let hit = s.hit(&g, x, y);
    s.press(&g, hit);
    assert_eq!(labels(&s).first().map(String::as_str), Some(RESTART));
    assert_eq!(s.ara.opened.len(), 1);
    let before = s.health.restarts;
    choose(&mut s, &g, RESTART);
    assert_eq!(s.health.restarts, before + 1);
    assert_eq!(s.ara_badge(c), Some(Badge::Ready));
    // The menu of a clip whose plugin runs has no restart.
    processing(&mut s, &g);
    s.popup = None;
    let (x, y) = clip_mid(&s, &g, "Vocal", 0);
    assert!(s.scene_context(&g, x, y));
    assert!(!labels(&s).iter().any(|l| l == RESTART));
}

#[test]
fn the_badges_balloon_names_the_plugin() {
    let (mut s, _) = studio();
    let (_, c) = vocal(&s);
    attach(&mut s, c);
    s.ara.fake = Some(vec![report(c, Phase::Analysing, 0.5)]);
    assert_eq!(s.ara_tip(c).as_deref(), Some("Test ARA: 解析中 50%"));
    s.ara.fake = Some(vec![report(c, Phase::Ready, 1.0)]);
    assert_eq!(s.ara_tip(c).as_deref(), Some("Test ARAで編集"));
    assert_eq!(
        clip_ids(
            &json!({ "ids": [c.to_string()] })
                .as_object()
                .unwrap()
                .clone()
        ),
        [c]
    );
}
