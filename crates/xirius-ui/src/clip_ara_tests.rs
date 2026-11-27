//! ARA on the clips, pressed as the window presses them: the rows of 「処理」, the list that
//! picks the plugin, the badge after the name and what a press on it does.

use std::sync::Arc;

use xirius_core::model::PluginKind;
use xirius_plugins::{Catalog, Entry};

use super::*;
use crate::arrange::probe::{clip_mid, track};
use crate::context::layout::Category;
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

#[test]
fn context_catalog_attached_and_crashed_audio_in_mixed_selection() {
    let (mut s, g) = studio();
    let (t, c) = vocal(&s);
    attach(&mut s, c);
    let mt = track(&s, "Keys");
    let mc = s.project().tracks[&mt].clips[0].id;
    for crashed in [false, true] {
        let mut state = report(c, Phase::Ready, 1.0);
        if crashed {
            state.crashed = Some("stopped".into());
        }
        s.ara.fake = Some(vec![state]);
        s.poll_ara();
        s.arrange.clips = [(t, c)].into_iter().collect();
        assert!(s.clip_context(&g, t, c, 600.0, 300.0));
        s.arrange.clips = [(t, c), (mt, mc)].into_iter().collect();
        assert!(s.clip_context(&g, mt, mc, 600.0, 300.0));
    }
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
    crate::context::tests::all_rows(s)
        .into_iter()
        .find(|r| r.label == label)
        .unwrap_or_else(|| panic!("no row {label}: {:?}", labels(s)))
}

/// A right press on the Vocal clip, then the category for its ARA actions.
fn processing(s: &mut Studio, g: &Geometry) {
    s.popup = None;
    let (x, y) = clip_mid(s, g, "Vocal", 0);
    assert!(s.scene_context(g, x, y));
    let attached = s
        .selected_clip
        .is_some_and(|(_, c)| s.project().ara.clips.contains_key(&c));
    choose(
        s,
        g,
        if attached {
            Category::Clip
        } else {
            Category::Audio
        }
        .label(),
    );
}

/// Presses the cell of the open menu's row named `label`.
fn choose(s: &mut Studio, g: &Geometry, label: &str) -> bool {
    crate::context::tests::choose(s, g, label)
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
    assert_eq!(
        add.key,
        if cfg!(target_os = "macos") {
            "⌘M"
        } else {
            "Ctrl+M"
        }
    );
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
            .field
            .value
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
    s.ara.fake = Some(vec![report(c, Phase::Rendering, 1.0)]);
    s.poll_ara();
    assert!(s.ara.opened.is_empty());
    s.ara.fake = Some(vec![report(c, Phase::Ready, 1.0)]);
    s.poll_ara();
    s.poll_ara();
    assert_eq!(s.ara.opened, [(UID.to_string(), vec![c])]);
}

/// The ARA row of 「処理」 for what is selected, and what Ctrl+M says of it.
fn why_not(s: &mut Studio, g: &Geometry, x: f32, y: f32) -> (crate::context::Row, Option<String>) {
    s.popup = None;
    s.error = None;
    assert!(s.scene_context(g, x, y));
    let add = row(s, ADD);
    // A dim row does nothing.
    choose(s, g, ADD);
    assert!(!picking(s));
    s.popup = None;
    s.mapped_key(g, "m", None, Mods::ctrl());
    assert!(!picking(s));
    (add, s.error.take())
}

#[test]
fn a_clip_that_cannot_take_it_has_the_row_dim_with_why() {
    let cases: [(&str, Value, &str); 5] = [
        (
            "clip.tempo.set",
            json!({ "bpm": 90 }),
            "テンポが変更されています",
        ),
        (
            "clip.speed.set",
            json!({ "percent": 150 }),
            "速さが変更されています",
        ),
        (
            "clip.transpose",
            json!({ "semis": 3 }),
            "ピッチが変更されています",
        ),
        (
            "clip.tune",
            json!({ "cents": 20 }),
            "ピッチが変更されています",
        ),
        (
            "clip.reverse.apply",
            json!({ "reverse": true, "offset": 0 }),
            "逆再生が有効です",
        ),
    ];
    for (name, mut args, said) in cases {
        let (mut s, g) = studio();
        let (t, c) = vocal(&s);
        if name == "clip.reverse.apply" {
            args["id"] = json!(c.to_string());
            args = json!({ "clips": [args] });
        } else {
            args["ids"] = json!([c.to_string()]);
        }
        s.song.run(name, args).unwrap();
        s.select_clip_here(t, c);
        let (x, y) = clip_mid(&s, &g, "Vocal", 0);
        let (add, error) = why_not(&mut s, &g, x, y);
        assert!(!add.enabled, "{name}");
        assert_eq!(add.reason, said, "{name}");
        // The key says the menu's words.
        assert_eq!(error.as_deref(), Some(said), "{name}");
        // So does the core, for a command sent from elsewhere.
        let e = s
            .song
            .run(
                "clip.ara.attach.apply",
                json!({ "ids": [c.to_string()], "plugin": UID }),
            )
            .unwrap_err();
        assert!(e.to_string().contains(said), "{name}: {e}");
    }
    // Frozen: the same row, with the freeze's reason, and the core turns it away too; a clip
    // plugged in before stays plugged in.
    let (mut s, g) = studio();
    let (t, c) = vocal(&s);
    s.song
        .run(
            "track.freeze.apply",
            json!({ "tracks": [{ "id": t.to_string(), "file": "frozen.wav", "start": 0 }] }),
        )
        .unwrap();
    // 「処理」 is dim with the freeze's reason, as everything in it is.
    s.popup = None;
    let (x, y) = clip_mid(&s, &g, "Vocal", 0);
    assert!(s.scene_context(&g, x, y));
    let sub = row(&s, ADD);
    assert!(!sub.enabled);
    assert_eq!(sub.reason, crate::toast::FROZEN);
    // The key still reaches it, and says why.
    s.popup = None;
    s.select_clip_here(t, c);
    s.mapped_key(&g, "m", None, Mods::ctrl());
    assert!(!picking(&s));
    assert_eq!(s.error.as_deref(), Some(crate::toast::FROZEN));
    let e = s
        .song
        .run(
            "clip.ara.attach.apply",
            json!({ "ids": [c.to_string()], "plugin": UID }),
        )
        .unwrap_err();
    assert!(e.to_string().contains(crate::toast::FROZEN), "{e}");
    // Plugged in before the freeze: it stays plugged in.
    let (mut s, _) = studio();
    let (t, c) = vocal(&s);
    attach(&mut s, c);
    s.song
        .run(
            "track.freeze.apply",
            json!({ "tracks": [{ "id": t.to_string(), "file": "frozen.wav", "start": 0 }] }),
        )
        .unwrap();
    let e = s
        .song
        .run("clip.ara.detach", json!({ "ids": [c.to_string()] }))
        .unwrap_err();
    assert!(e.to_string().contains(crate::toast::FROZEN), "{e}");
    assert!(s.project().ara.clips.contains_key(&c));
}

#[test]
fn midi_and_a_mix_have_the_row_dim_and_ask_for_audio() {
    const AUDIO_ONLY: &str = "音声クリップを選択してください";
    // A MIDI clip: 「処理」 is there, with only the dim ARA row.
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    let k = s.project().tracks[&keys].clips[0].id;
    s.select_clip_here(keys, k);
    let (x, y) = clip_mid(&s, &g, "Keys", 0);
    let (add, error) = why_not(&mut s, &g, x, y);
    assert!(!add.enabled);
    assert_eq!(add.reason, AUDIO_ONLY);
    assert_eq!(error.as_deref(), Some(AUDIO_ONLY));
    // Audio and MIDI together.
    let (mut s, g) = studio();
    let (t, c) = vocal(&s);
    s.arrange.clips = [(t, c), (keys, k)].into_iter().collect();
    let (x, y) = clip_mid(&s, &g, "Vocal", 0);
    let (add, error) = why_not(&mut s, &g, x, y);
    assert_eq!(s.arrange.clips.len(), 2, "the press kept the selection");
    assert!(!add.enabled);
    assert_eq!(add.reason, AUDIO_ONLY);
    assert_eq!(error.as_deref(), Some(AUDIO_ONLY));
}

#[test]
fn ctrl_m_on_clips_some_of_which_have_it_says_the_rest_are_left() {
    let (mut s, g) = studio();
    let (t, c) = vocal(&s);
    attach(&mut s, c);
    s.ara.fake = Some(vec![report(c, Phase::Ready, 1.0)]);
    let drums = track(&s, "Drums");
    let d = s.project().tracks[&drums].clips[0].id;
    s.arrange.clips = [(t, c), (drums, d)].into_iter().collect();
    s.mapped_key(&g, "m", None, Mods::ctrl());
    assert_eq!(s.ara.opened, [(UID.to_string(), vec![c])]);
    let said = s.toast.as_ref().map(|t| t.text.clone());
    assert_eq!(
        said.as_deref(),
        Some("ほかのクリップ 1 件は窓に出していません")
    );
    assert!(!s.project().ara.clips.contains_key(&d));
}

#[test]
fn an_attached_clip_edits_and_detaches_by_the_plugins_name() {
    let (mut s, g) = studio();
    let (t, c) = vocal(&s);
    attach(&mut s, c);
    s.ara.fake = Some(vec![report(c, Phase::Ready, 1.0)]);
    processing(&mut s, &g);
    let edit = format!("{NAME}で編集");
    let detach = format!("{NAME}を外す");
    let all = labels(&s);
    assert!(!all.iter().any(|l| l == ADD), "{all:?}");
    assert_eq!(
        row(&s, &edit).key,
        if cfg!(target_os = "macos") {
            "⌘M"
        } else {
            "Ctrl+M"
        }
    );
    assert_eq!(
        row(&s, &detach).key,
        if cfg!(target_os = "macos") {
            "⌥⌘M"
        } else {
            "Ctrl+Alt+M"
        }
    );
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
    // Not read yet: being opened, the ring without a percent.
    assert_eq!(s.ara_badge(c), Some(Badge::Busy(None)));
    assert_eq!(Badge::Busy(None).said(), "解析中");
    s.ara.fake = Some(vec![report(c, Phase::Analysing, 0.42)]);
    assert_eq!(s.ara_badge(c), Some(Badge::Busy(Some(42))));
    assert!(s.poll_ara(), "a turning ring keeps the frames coming");
    assert_eq!(Badge::Busy(Some(42)).said(), "解析中 42%");
    // Analysed and being rendered: the ring, not a 100% that stays.
    s.ara.fake = Some(vec![report(c, Phase::Rendering, 1.0)]);
    assert_eq!(s.ara_badge(c), Some(Badge::Busy(None)));
    assert!(s.poll_ara());
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
    s.ara.fake = Some(vec![failed.clone()]);
    assert_eq!(s.ara_badge(c), Some(Badge::Failed("読み込めません".into())));
    // Down and cannot be opened: down, whose menu restarts it.
    let mut both = failed.clone();
    both.crashed = Some("stopped".into());
    s.ara.fake = Some(vec![both]);
    assert_eq!(s.ara_badge(c), Some(Badge::Crashed));
    s.ara.fake = Some(vec![failed]);
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
fn without_the_host_the_badge_never_says_done() {
    let (mut s, _) = studio();
    let (_, c) = vocal(&s);
    attach(&mut s, c);
    assert!(s.transport.host().is_none() && s.ara.fake.is_none());
    // No report: nothing opens the document, and the clip plays its own sound.
    assert_eq!(s.ara_badge(c), Some(Badge::Failed(NO_HOST.into())));
    assert_eq!(s.ara_tip(c), Some(format!("{NAME}: {NO_HOST}")));
    assert!(!s.poll_ara());
}

#[test]
fn while_the_song_is_prepared_the_badge_waits_and_does_not_say_it_cannot_open() {
    let (mut s, _) = studio();
    let (_, c) = vocal(&s);
    attach(&mut s, c);
    s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
    // Before a play wants the player (a window without the device): not known yet.
    assert_eq!(s.ara_badge(c), Some(Badge::Busy(None)));
    // The player did not start, or the device cannot be had: no player will open it.
    s.transport.start_failed = true;
    assert_eq!(s.ara_badge(c), Some(Badge::Failed(NO_HOST.into())));
    s.transport.start_failed = false;
    s.transport.go_down_for_test("busy");
    assert_eq!(s.ara_badge(c), Some(Badge::Failed(NO_HOST.into())));
    s.transport.down = None;
    // Right after the song opened: the player is being started behind the card.
    let p = s.project();
    assert!(s.transport.begin_start(&p, s.song.dir()));
    assert!(s.transport.preparing() && s.transport.host().is_none());
    assert_eq!(s.ara_badge(c), Some(Badge::Busy(None)));
    assert_eq!(s.ara_tip(c), Some(format!("{NAME}: 解析中")));
}

#[test]
fn a_player_that_cannot_begin_starting_makes_the_badge_say_it_cannot_open() {
    let (mut s, _) = studio();
    let (_, c) = vocal(&s);
    attach(&mut s, c);
    s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
    // The device would not be let go of for the preparation (an exclusive device): nothing is
    // being prepared and no player comes, so the badge does not wait for one.
    s.transport.bind_window();
    s.transport.fail_start = Some("busy".into());
    let p = s.project();
    assert!(!s.transport.begin_start(&p, s.song.dir()));
    assert!(!s.transport.preparing() && s.transport.host().is_none());
    assert_eq!(s.ara_badge(c), Some(Badge::Failed(NO_HOST.into())));
    // The next start clears it.
    assert!(s.transport.begin_start(&p, s.song.dir()));
    assert_eq!(s.ara_badge(c), Some(Badge::Busy(None)));
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
    let at = s.ara_badge_rect(tr, clip, narrow, &Badge::Ready, &[]);
    assert!(
        at.x >= narrow.x && at.right() <= narrow.right(),
        "{at:?} {narrow:?}"
    );
    // Analysing in a wide clip: the ring and its percent.
    let busy = Badge::Busy(Some(100));
    assert!(busy.width() > BOX_W);
    let at = s.ara_badge_rect(tr, clip, r, &busy, &[]);
    assert_eq!(at.w, busy.width());
    // In a narrow one the percent goes and the ring stays, inside the clip.
    let at = s.ara_badge_rect(tr, clip, narrow, &busy, &[]);
    assert_eq!(at.w, BOX_W);
    assert!(
        at.x >= narrow.x && at.right() <= narrow.right(),
        "{at:?} {narrow:?}"
    );
    // Narrower than the ring: the box is cut at the clip's end, never past it.
    let thin = Rect::new(r.x, r.y, BOX_W / 2.0, r.h);
    for badge in [busy, Badge::Ready, Badge::Crashed] {
        let at = s.ara_badge_rect(tr, clip, thin, &badge, &[]);
        assert!(
            at.x >= thin.x && at.right() <= thin.right() + 0.01,
            "{badge:?} {at:?} {thin:?}"
        );
    }
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
    // Went down: only the colour says so; the press opens the plugin's small menu, not the
    // clip's, and the restart brings the plugin back.
    let mut down = report(c, Phase::Ready, 1.0);
    down.crashed = Some("stopped".into());
    s.ara.fake = Some(vec![down.clone()]);
    s.toast = None;
    s.poll_ara();
    assert!(s.toast_shown().is_none() && s.popup.is_none());
    let hit = s.hit(&g, x, y);
    s.press(&g, hit);
    assert_eq!(labels(&s), ["Test ARA", "編集", "再起動", "外す"]);
    assert!(
        !s.context_rows()[0].enabled,
        "the title is not an operation"
    );
    let edit = row(&s, "編集");
    assert!(!edit.enabled);
    assert_eq!(edit.reason, DOWN);
    assert!(row(&s, "再起動").enabled && row(&s, "外す").enabled);
    // Disabled editing does nothing; the old decorative title is not an operation.
    choose(&mut s, &g, "編集");
    // The title stays a dim, unpressable row.
    assert!(!s.context_rows()[0].enabled);
    assert_eq!(s.ara.opened.len(), 1);
    // Nor do the clip's 「〈名前〉で編集」 or Ctrl+M (the double click is the same opening).
    processing(&mut s, &g);
    let edit = row(&s, &format!("{NAME}で編集"));
    assert!(!edit.enabled);
    assert_eq!(edit.reason, DOWN);
    s.popup = None;
    s.mapped_key(&g, "m", None, Mods::ctrl());
    assert_eq!(s.error.take().as_deref(), Some(DOWN));
    assert_eq!(s.ara.opened.len(), 1);
    assert!(!s.open_ara_editor_for(&[c]));
    // 「再起動」 brings it back.
    let hit = s.hit(&g, x, y);
    s.press(&g, hit);
    let before = s.health.restarts;
    choose(&mut s, &g, "再起動");
    assert_eq!(s.health.restarts, before + 1);
    assert_eq!(s.ara_badge(c), Some(Badge::Ready));
    // 「外す」 takes the plugin off the clip.
    s.ara.fake = Some(vec![down]);
    let hit = s.hit(&g, x, y);
    s.press(&g, hit);
    choose(&mut s, &g, "外す");
    assert_eq!(s.ara_badge(c), None, "{:?}", s.error);
    // The menu of a clip whose plugin runs has no restart.
    processing(&mut s, &g);
    s.popup = None;
    let (x, y) = clip_mid(&s, &g, "Vocal", 0);
    assert!(s.scene_context(&g, x, y));
    assert!(!labels(&s).iter().any(|l| l == RESTART));
}

/// A second ARA plugin, kept with the clip (not in the screen's catalog).
const OTHER: &str = "00000000000000000000000000000002";
const OTHER_NAME: &str = "Other ARA";

/// UI-380: the ARA window's title bar is the clip's track and the plugin, in the ARA form. It is
/// given as the window is asked for, and again only when it changes; the pin on it turns on and
/// off; the VST3 bar's other presses do nothing; a closed window has none.
#[test]
fn the_ara_windows_bar_names_the_track_and_the_plugin_and_pins() {
    let (mut s, _) = studio();
    let (t, c) = vocal(&s);
    attach(&mut s, c);
    s.ara.fake = Some(vec![report(c, Phase::Ready, 1.0)]);
    assert!(s.open_ara_editor_for(&[c]));
    let key = UID.to_ascii_lowercase();
    let track_name = s.project().tracks[&t].shown_name().to_string();
    let bars = std::mem::take(&mut s.ara.sent_bars);
    assert_eq!(bars.len(), 1, "{bars:?}");
    let (to, bar) = &bars[0];
    assert_eq!(to, &key);
    assert_eq!(bar.track, track_name);
    assert_eq!(bar.device, NAME);
    assert!(bar.ara && !bar.pinned);
    assert!(bar.preset.is_empty() && bar.presets.is_empty() && bar.menu.is_empty());
    // The next frame gives nothing: the bar did not change.
    assert!(!s.poll_ara_editor_bar());
    assert!(s.ara.sent_bars.is_empty());
    // The pin on the bar: on, then off, each given back to the window.
    for pinned in [true, false] {
        s.ara.fake_actions.push((UID.into(), EditorAction::Pin));
        assert!(s.poll_ara_editor_bar());
        let bars = std::mem::take(&mut s.ara.sent_bars);
        assert_eq!(bars.len(), 1, "{bars:?}");
        assert_eq!(bars[0].1.pinned, pinned);
        assert!(bars[0].1.ara);
    }
    // The VST3 bar's other presses are not on it.
    s.ara.fake_actions.push((UID.into(), EditorAction::Power));
    s.ara
        .fake_actions
        .push((UID.into(), EditorAction::PresetNext));
    assert!(!s.poll_ara_editor_bar());
    assert!(s.ara.sent_bars.is_empty());
    // Closed: its bar is forgotten, and nothing more is given.
    s.close_ara_editor(&key);
    assert_eq!(s.ara.closed, std::slice::from_ref(&key));
    assert!(s.ara.editor_bars.is_empty() && s.ara.editor_clips.is_empty());
    s.ara.fake_actions.push((UID.into(), EditorAction::Pin));
    assert!(!s.poll_ara_editor_bar());
    assert!(s.ara.sent_bars.is_empty());
}

/// UI-380, as a VST3 window: opening another plugin's ARA window closes the unpinned ones; a
/// pinned window stays; a pinned window opening closes nothing; unpinned, it takes the place of
/// the other unpinned ones.
#[test]
fn an_unpinned_ara_window_gives_way_to_another_plugins_and_a_pinned_one_stays() {
    let (mut s, _) = studio();
    let (vocal_track, c) = vocal(&s);
    attach(&mut s, c);
    // The other plugin on an audio clip of another track.
    let others: Vec<(TrackId, ClipId)> = s
        .project()
        .tracks
        .iter()
        .filter(|(t, _)| **t != vocal_track)
        .flat_map(|(t, track)| track.clips.iter().map(|clip| (*t, clip.id)))
        .collect();
    let (other_track, d) = others
        .into_iter()
        .find(|(_, d)| {
            s.song
                .run(
                    "clip.ara.attach.apply",
                    json!({ "ids": [d.to_string()], "plugin": OTHER, "name": OTHER_NAME }),
                )
                .is_ok()
        })
        .expect("an audio clip on another track");
    let mut other = report(d, Phase::Ready, 1.0);
    other.plugin = OTHER.into();
    other.name = OTHER_NAME.into();
    s.ara.fake = Some(vec![report(c, Phase::Ready, 1.0), other]);
    let (mine, theirs) = (UID.to_ascii_lowercase(), OTHER.to_ascii_lowercase());
    let pin = |s: &mut Studio, plugin: &str| {
        s.ara.fake_actions.push((plugin.into(), EditorAction::Pin));
        assert!(s.poll_ara_editor_bar());
    };

    assert!(s.open_ara_editor_for(&[c]));
    assert!(s.open_ara_editor_for(&[d]));
    assert_eq!(
        s.ara.closed,
        std::slice::from_ref(&mine),
        "the unpinned window gave way"
    );
    assert!(s.ara.editor_clips.contains_key(&theirs));
    let bar = &s.ara.editor_bars[&theirs];
    assert_eq!(bar.device, OTHER_NAME);
    assert_eq!(
        bar.track,
        s.project().tracks[&other_track].shown_name().to_string()
    );

    // Pinned, it stays when another plugin's window opens.
    pin(&mut s, OTHER);
    assert!(s.open_ara_editor_for(&[c]));
    assert_eq!(s.ara.closed.len(), 1);
    assert!(s.ara.editor_clips.contains_key(&mine) && s.ara.editor_clips.contains_key(&theirs));

    // Both pinned, then the other unpinned: the pinned one stays.
    pin(&mut s, UID);
    pin(&mut s, OTHER);
    assert_eq!(s.ara.closed.len(), 1);
    // A pinned window opening closes nothing, not even an unpinned one.
    assert!(s.open_ara_editor_for(&[c]));
    assert_eq!(s.ara.closed.len(), 1);
    assert!(s.ara.editor_clips.contains_key(&theirs));
    // Unpinned, it takes the place of the other unpinned window.
    pin(&mut s, UID);
    assert_eq!(s.ara.closed, [mine.clone(), theirs.clone()]);
    assert!(s.ara.editor_clips.contains_key(&mine));
    assert!(!s.ara.editor_clips.contains_key(&theirs));
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
