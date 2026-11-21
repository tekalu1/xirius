//! Explicit desktop fixture setup. Input is supplied by scripts/test_plugin_presets.py.
use crate::{
    shot::{Act, Play, Shot},
    studio::{Geometry, Studio},
};
use gpui::{App, Window};
use serde_json::json;
use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Instant};

pub const SHOTS: &[Shot] = &[Shot {
    name: "preset-input",
    acts: &[Act::Stage(Play::Window(prepare), 0)],
}];

pub const COLD: &[Shot] = &[Shot {
    name: "preset-input-cold",
    acts: &[Act::Stage(Play::Window(prepare), 1)],
}];

thread_local! {
    static WAIT: RefCell<Option<(PathBuf, Instant)>> = const { RefCell::new(None) };
}

fn prepare(stage: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, _: &mut App) {
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
    if stage == 1 {
        prepare_cold(&mut s, window, dir);
        return;
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
    let drag_name = if std::env::var_os("XIRIUS_PRESET_INPUT_BUILTIN").is_some() {
        "Sampler"
    } else {
        &source.name
    };
    assert!(s.run_command("browser.preset.list", json!({})));
    crate::shot::run_for(&mut s, 10.0, |s| {
        s.browser.track_library.as_ref().is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry.device == source.uid
                    && entry.category == xirius_core::track_browser::Category::Instrument
            })
        })
    });
    let g = browser_geometry(&mut s, window, drag_name);
    let screen = |at| screen_point(main, window, at);
    let track_point = |track| {
        s.center_of(
            &g,
            crate::studio::Hit::Row(crate::tree::RowKind::Track(track)),
        )
        .map(screen)
        .expect("instrument track must be visible")
    };
    let [drag_from, drag_to] = drag_points(&s, &g, drag_name);
    let edge = s
        .tree()
        .row_of(crate::tree::RowKind::Track(second_track))
        .unwrap()
        .y;
    let seam = (g.column.x + 50.0, g.column.y + edge - s.scroll.1 + 2.0);
    input_ready(
        dir,
        &json!({
            "main":main, "frames":frames, "builtin":track_point(keys),
            "external_track":track_point(second_track),
            "drag_from":screen(drag_from), "drag_to":screen(drag_to), "seam":screen(seam),
            "root":s.project().root,
            "before_index":s.project().root.iter().position(|node| {
                *node == xirius_core::id::Node::Track(second_track)
            }).unwrap(),
            "builtin_title":format!("Xirius · {}", s.project().tracks[&keys].inserts[0].id)
        }),
    );
}

fn prepare_cold(s: &mut Studio, window: &Window, dir: PathBuf) {
    let catalog = xirius_plugins::Catalog::load(s.song().plugin_cache());
    let name = catalog
        .entries
        .first()
        .expect("scanned fixture")
        .name
        .clone();
    s.use_plugins(catalog);
    crate::shot::run_for(s, 10.0, |s| !s.input_blocked());
    assert!(!s.input_blocked());
    let main = crate::park::own_handle(crate::screen::WINDOW_TITLE).unwrap();
    let g = browser_geometry(s, window, &name);
    let [from, to] = drag_points(s, &g, &name);
    input_ready(
        dir,
        &json!({
            "main":main,
            "drag_from":screen_point(main, window, from),
            "drag_to":screen_point(main, window, to)
        }),
    );
}

fn browser_geometry(s: &mut Studio, window: &Window, name: &str) -> Geometry {
    s.drawer_open = false;
    s.arrange_visible = true;
    s.browser.open = true;
    s.browser.tab = crate::browser::RailTab::Instruments;
    s.browser.search.value = name.into();
    s.scroll = (0.0, 0.0);
    let viewport = window.viewport_size();
    let width = viewport.width.into();
    let height = (f32::from(viewport.height)
        - xirius_design::Layout::TOP_BAR_H
        - xirius_design::Layout::BOTTOM_BAR_H)
        .max(0.0);
    let g = s.settled_geometry(width, height);
    s.fold_all(&g, true);
    s.settled_geometry(width, height)
}

fn screen_point(main: isize, window: &Window, (x, y): (f32, f32)) -> [i32; 2] {
    let scale = window.scale_factor();
    let mut point = windows_sys::Win32::Foundation::POINT {
        x: (x * scale) as i32,
        y: ((y + xirius_design::Layout::TOP_BAR_H) * scale) as i32,
    };
    // SAFETY: query the fixture's own main window.
    unsafe { windows_sys::Win32::Graphics::Gdi::ClientToScreen(main as _, &mut point) };
    [point.x, point.y]
}

fn drag_points(s: &Studio, g: &Geometry, name: &str) -> [(f32, f32); 2] {
    let browser = s.browser.layout(g.panel);
    let item = browser.rows.iter().position(|placed| {
        matches!(&placed.row, crate::browser::BRow::Item { item, .. } if item.name == name)
    }).expect("fixture instrument in browser");
    let from = s
        .browser
        .center_of(&browser, crate::browser::BrowserHit::Item(item))
        .expect("fixture instrument must be visible");
    let to = (g.lanes.right() - 55.0, g.lanes.bottom() - 30.0);
    assert_eq!(s.lib_drop_at(g, to.0, to.1), crate::browser_inst::Drop::New);
    [from, to]
}

fn input_ready(dir: PathBuf, data: &serde_json::Value) {
    std::fs::write(
        dir.join("input-ready.json"),
        serde_json::to_vec_pretty(data).unwrap(),
    )
    .unwrap();
    WAIT.with_borrow_mut(|wait| *wait = Some((dir, Instant::now())));
}

pub(crate) fn ready(studio: &Rc<RefCell<Studio>>) -> bool {
    WAIT.with_borrow_mut(|wait| {
        let Some((dir, started)) = wait else {
            return true;
        };
        assert!(
            started.elapsed().as_secs() < 180,
            "desktop input driver timed out"
        );
        let request = dir.join("input-state-request");
        if request.exists()
            && let Ok(s) = studio.try_borrow()
        {
            let p = s.project();
            let editor = s
                .selected_row
                .and_then(|row| match row {
                    crate::tree::RowKind::Track(t) => crate::instrument::current(p.tracks.get(&t)?),
                    _ => None,
                })
                .map(|plugin| format!("Xirius · {}", plugin.id));
            let temp = dir.join("input-state.tmp");
            std::fs::write(
                &temp,
                serde_json::to_vec(&json!({"root":p.root,"editor":editor})).unwrap(),
            )
            .unwrap();
            std::fs::rename(temp, dir.join("input-state.json")).unwrap();
            std::fs::remove_file(request).unwrap();
        }
        if dir.join("input-done").exists() {
            *wait = None;
            true
        } else {
            false
        }
    })
}
