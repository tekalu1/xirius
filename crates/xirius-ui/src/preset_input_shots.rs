//! Explicit desktop fixture setup. Input is supplied by scripts/test_plugin_presets.py.
use crate::{
    shot::{Act, Play, Shot},
    studio::Studio,
};
use gpui::{App, Window};
use serde_json::json;
use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Instant};

pub const SHOTS: &[Shot] = &[Shot {
    name: "preset-input",
    acts: &[Act::Stage(Play::Window(prepare), 0)],
}];

thread_local! {
    static WAIT: RefCell<Option<(PathBuf, Instant)>> = const { RefCell::new(None) };
}

fn prepare(_: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, _: &mut App) {
    assert_eq!(std::env::var("XIRIUS_DESKTOP_INPUT").as_deref(), Ok("1"));
    let args: Vec<_> = std::env::args().collect();
    let dir = PathBuf::from(&args.windows(2).find(|a| a[0] == "--shot").unwrap()[1]);
    let mut s = studio.borrow_mut();
    s.os_files = true;
    // The sample song references demonstration audio absent from this isolated run.
    let clips: Vec<_> = s
        .project()
        .tracks
        .values()
        .flat_map(|t| t.clips.iter().map(|c| c.id))
        .collect();
    if !clips.is_empty() {
        assert!(s.run_command("clip.delete", json!({"ids":clips})));
    }
    crate::plugin_reload_shots::prepare(&mut s, 0);
    crate::shot::run_for(&mut s, 15.0, |s| !s.input_blocked());
    let first = crate::plugin_reload_shots::id(&s);
    let project = s.project();
    let track = project
        .tracks
        .values()
        .find(|t| t.name == "Vocal")
        .unwrap()
        .id;
    let keys = project
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .unwrap()
        .id;
    let source = project.tracks[&track].inserts[0].clone();
    let catalog = xirius_plugins::Catalog::load(s.song().plugin_cache());
    s.use_plugins(catalog);
    assert!(s.run_command(
        "track.add.with-instrument",
        json!({"instrument":source.name, "format":"vst3", "uid":source.uid})
    ));
    let second_track = *s.project().tracks.keys().max().unwrap();
    let second = s.project().tracks[&second_track].inserts[0].id;
    assert!(s.run_command(
        "plugin.isolation.set",
        json!({"ids":[second], "mode":"dedicated"})
    ));
    assert!(s.set_instrument(
        keys,
        &crate::instrument::Pick::built_in(crate::instrument::DEFAULT)
    ));
    s.prepare_song();
    crate::shot::run_for(&mut s, 20.0, |s| {
        !s.sound_preparing() && !s.input_blocked() && s.has_loaded(first) && s.has_loaded(second)
    });
    assert!(
        !s.input_blocked(),
        "fixture held: preparation={:?}, opening={}, saving={}, inserts={}, unsaved={}, error={:?}",
        s.preparing,
        s.opening.is_some(),
        s.saving.is_some(),
        s.inserts_wait.is_some(),
        s.unsaved.is_some(),
        s.error
    );
    for name in ["Clean", "Bright"] {
        crate::shot::run_for(&mut s, 20.0, |s| !s.input_blocked());
        assert!(
            s.run_command(
                "preset.save",
                json!({"track":track,"of":first,"kind":"fx","name":name})
            ),
            "save {name}: error={:?}, blocked={}, macro={}",
            s.error,
            s.input_blocked(),
            s.macro_run.is_some()
        );
        crate::shot::run_for(&mut s, 10.0, |s| !s.input_blocked());
        assert!(s.error.is_none(), "{:?}", s.error);
    }
    assert!(s.open_plugin(track, first));
    crate::shot::run_for(&mut s, 15.0, |s| s.editor_shown(first));
    s.mixer.editor_pins.insert(first);
    assert!(s.open_plugin(second_track, second));
    crate::shot::run_for(&mut s, 15.0, |s| s.editor_shown(second));
    s.mixer.editor_pins.insert(second);
    crate::shot::run_for(&mut s, 10.0, |s| {
        [first, second]
            .iter()
            .all(|&id| s.mixer.editor_hwnd(id).is_some())
    });
    let frames = [first, second].map(|id| s.mixer.editor_hwnd(id).unwrap());
    let main = crate::park::own_handle(crate::screen::WINDOW_TITLE).unwrap();
    s.drawer_open = false;
    s.arrange_visible = true;
    s.browser.open = true;
    s.browser.tab = crate::browser::RailTab::Instruments;
    s.browser.search.value.clone_from(&source.name);
    assert!(s.run_command("browser.preset.list", json!({})));
    crate::shot::run_for(&mut s, 10.0, |s| {
        s.browser.track_library.as_ref().is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry.device == source.uid
                    && entry.category == xirius_core::track_browser::Category::Instrument
            })
        })
    });
    s.scroll = (0.0, 0.0);
    let viewport = window.viewport_size();
    let width = viewport.width.into();
    let height = (f32::from(viewport.height)
        - xirius_design::Layout::TOP_BAR_H
        - xirius_design::Layout::BOTTOM_BAR_H)
        .max(0.0);
    let g = s.settled_geometry(width, height);
    s.fold_all(&g, true);
    let g = s.settled_geometry(width, height);
    let scale = window.scale_factor();
    let screen = |(x, y): (f32, f32)| {
        let mut point = windows_sys::Win32::Foundation::POINT {
            x: (x * scale) as i32,
            y: ((y + xirius_design::Layout::TOP_BAR_H) * scale) as i32,
        };
        // SAFETY: query the fixture's own main window.
        unsafe { windows_sys::Win32::Graphics::Gdi::ClientToScreen(main as _, &mut point) };
        [point.x, point.y]
    };
    let track_point = |track| {
        s.center_of(
            &g,
            crate::studio::Hit::Row(crate::tree::RowKind::Track(track)),
        )
        .map(screen)
        .expect("instrument track must be visible")
    };
    let browser = s.browser.layout(g.panel);
    let item = browser.rows.iter().position(|placed| {
        matches!(&placed.row, crate::browser::BRow::Item { item, .. } if item.name == source.name)
    }).expect("fixture instrument in browser");
    let drag_from = screen(
        s.browser
            .center_of(&browser, crate::browser::BrowserHit::Item(item))
            .expect("fixture instrument must be visible"),
    );
    let drag_to = (g.lanes.right() - 55.0, g.lanes.bottom() - 30.0);
    assert_eq!(
        s.lib_drop_at(&g, drag_to.0, drag_to.1),
        crate::browser_inst::Drop::New
    );
    std::fs::write(
        dir.join("input-ready.json"),
        serde_json::to_vec_pretty(&json!({
            "main":main, "frames":frames, "builtin":track_point(keys),
            "external_track":track_point(second_track),
            "drag_from":drag_from, "drag_to":screen(drag_to),
            "builtin_title":format!("Xirius · {}", s.project().tracks[&keys].inserts[0].id)
        }))
        .unwrap(),
    )
    .unwrap();
    WAIT.with_borrow_mut(|wait| *wait = Some((dir, Instant::now())));
}

pub(crate) fn ready() -> bool {
    WAIT.with_borrow_mut(|wait| {
        let Some((dir, started)) = wait else {
            return true;
        };
        assert!(
            started.elapsed().as_secs() < 180,
            "desktop input driver timed out"
        );
        if dir.join("input-done").exists() {
            *wait = None;
            true
        } else {
            false
        }
    })
}
