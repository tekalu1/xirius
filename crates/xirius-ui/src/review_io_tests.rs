//! PRJ-031 review: exercise dispatchers and their continuations, not just helpers.
use crate::{
    browser_io_tests::Release,
    song::Song,
    studio::{Rect, Studio},
};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use xirius_core::{
    CmdError,
    session::{RunOptions, RunResult},
};
use xirius_mcp::Host;

fn setup() -> (tempfile::TempDir, Studio) {
    let root = tempfile::tempdir().unwrap();
    let mut s = Studio::new(Song::empty());
    s.song.save_as(&root.path().join("song")).unwrap();
    let file = s.song.places_file.as_ref().unwrap();
    let mut prefs = xirius_session::places::Prefs::first_run(None);
    prefs.places.push(xirius_session::places::Place {
        name: "Existing".into(),
        path: root.path().into(),
    });
    xirius_session::places::save(file, &prefs).unwrap();
    s.song.load_settings(&root.path().join("settings"));
    (root, s)
}

#[test]
fn external_reads_preserve_note_move_and_resize_until_mouse_up_and_undo() {
    use crate::tools::Mods;
    for resize in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut s = Studio::sample();
        s.song.save_as(&root.path().join("song")).unwrap();
        let g = s.geometry(1440.0, 820.0);
        s.roll.center(&s.roll_ctx(&g).unwrap());
        let ctx = s.roll_ctx(&g).unwrap();
        let n = ctx.notes()[0];
        let original = (n.id, n.start, n.length, n.pitch);
        let v = s.roll.view(&ctx);
        let from = (
            v.x(n.start)
                + if resize {
                    v.px(n.length) - 1.0
                } else {
                    v.px(n.length) / 2.0
                },
            v.y(n.pitch) + v.row_h / 2.0,
        );
        let to = (from.0 + v.px(i64::from(ctx.project.ppq)), from.1);
        s.bind_window_io();
        let out = s.roll.down(&ctx, from.0, from.1, Mods::NONE, 1);
        s.apply_roll(&g, out);
        let out = s.roll.moved(&ctx, to.0, to.1, Mods::NONE);
        s.apply_roll(&g, out);
        assert!(s.roll.drag.is_some());
        let preview = s.roll.preview.changed[&original.0];
        for name in ["track.list", "project.save", "project.new"] {
            // Even when the platform flag has not been updated, the actual drag protects it.
            let call = request(&mut s, name, json!({}), RunOptions::human());
            assert!(call.join().unwrap().is_err());
            assert!(s.saving.is_none());
            assert!(s.roll.drag.is_some());
            let current = s.roll.preview.changed[&original.0];
            assert_eq!(
                (current.start, current.length, current.pitch),
                (preview.start, preview.length, preview.pitch)
            );
        }
        let ctx = s.roll_ctx(&g).unwrap();
        let out = s.roll.up(&ctx, to.0, to.1, Mods::NONE);
        s.apply_roll(&g, out);
        let ctx = s.roll_ctx(&g).unwrap();
        let edited = ctx.notes().iter().find(|n| n.id == original.0).unwrap();
        assert_eq!(
            (edited.start, edited.length, edited.pitch),
            (preview.start, preview.length, preview.pitch)
        );
        assert_ne!((edited.start, edited.length), (original.1, original.2));
        s.run_command("history.undo", json!({}));
        finish(&mut s);
        let ctx = s.roll_ctx(&g).unwrap();
        let restored = ctx.notes().iter().find(|n| n.id == original.0).unwrap();
        assert_eq!(
            (restored.start, restored.length, restored.pitch),
            (original.1, original.2, original.3)
        );
        let retry = request(&mut s, "track.list", json!({}), RunOptions::human());
        finish(&mut s);
        retry.join().unwrap().unwrap();
    }
}

#[test]
fn external_reads_and_automatic_adoption_preserve_held_pc_keys() {
    let (_root, mut s) = setup();
    let track = *s.project().tracks.keys().next().unwrap();
    s.pc_keys.down.push(("a".into(), track, 60, 60));
    s.bind_window_io();
    assert!(s.other_busy());
    let call = request(&mut s, "track.list", json!({}), RunOptions::human());
    assert!(call.join().unwrap().is_err());
    assert_eq!(s.pc_keys.down.len(), 1);
    assert!(s.saving.is_none());
}

#[test]
fn picker_answers_keep_their_own_reservation_and_cannot_cross_songs() {
    let (_root, mut s) = setup();
    s.bind_window_io();
    let old = s.begin_picker();
    let current = s.begin_picker();
    assert!(!s.accept_picker(old));
    assert!(s.other_busy());
    let call = request(&mut s, "track.list", json!({}), RunOptions::human());
    assert!(call.join().unwrap().is_err());
    assert!(s.accept_picker(current));
    assert!(!s.human_interaction_active());
    let old_song = s.begin_picker();
    s.song = Song::empty();
    assert!(!s.accept_picker(old_song));
    assert!(!s.human_interaction_active());
}

#[test]
fn late_groove_results_cannot_replace_later_choices_or_editor_intent() {
    use crate::quantize_panel::QAct;
    for choice in 0..7 {
        let root = tempfile::tempdir().unwrap();
        let mut s = Studio::sample();
        s.song.save_as(&root.path().join("song")).unwrap();
        s.song.load_settings(&root.path().join("settings"));
        let clip = s.selected_clip.unwrap().1;
        s.drawer_open = true;
        s.tab = crate::studio::DrawerTab::Roll;
        s.focus_arrange(false);
        let file = root.path().join("groove.mid");
        let mut midi = b"MThd".to_vec();
        midi.extend([0, 0, 0, 6, 0, 0, 0, 1, 0, 96]);
        midi.extend(b"MTrk");
        midi.extend([
            0, 0, 0, 12, 0, 0x90, 60, 80, 96, 0x80, 60, 0, 0, 0xff, 0x2f, 0,
        ]);
        std::fs::write(
            &file,
            if choice == 6 {
                b"invalid midi".to_vec()
            } else {
                midi
            },
        )
        .unwrap();
        s.quant.groove_on = true;
        s.bind_window_io();
        let (release, entered, _observe) = gate("groove_read", root.path());
        let _ui = xirius_core::io_test::forbid_ui_io();
        s.import_groove(&file).unwrap();
        entered.recv_timeout(Duration::from_secs(30)).unwrap();
        let g = s.geometry(880.0, 600.0);
        match choice {
            0 => {
                s.quant_action(&g, QAct::Groove(false));
            }
            1 => {
                s.quant_action(&g, QAct::Clip(clip));
            }
            2 => {
                s.quant_action(&g, QAct::Preset(0));
            }
            3 => {
                s.quant_action(&g, QAct::External);
            }
            4 => {
                s.quant_action(&g, QAct::Groove(false));
                s.quant_action(&g, QAct::Groove(true)); // Same value again still has newer intent.
            }
            5 => {
                s.focus_arrange(true);
                s.focus_arrange(false);
            }
            _ => {
                s.quant_action(&g, QAct::Groove(false));
            }
        }
        let latest = s.quant;
        let saved = s.song.session().state.view.roll.quantize.clone();
        let external = s.song.session().state.view.roll.external_groove.clone();
        let error = s.error.clone();
        release.release();
        let end = Instant::now() + Duration::from_secs(30);
        while s.view_read.groove.is_some() {
            s.poll_groove_import();
            assert!(Instant::now() < end);
            std::thread::yield_now();
        }
        assert_eq!(s.quant, latest);
        assert_eq!(s.song.session().state.view.roll.quantize, saved);
        assert_eq!(s.song.session().state.view.roll.external_groove, external);
        assert_eq!(
            s.error, error,
            "stale failures do not replace newer feedback"
        );
    }
}

fn gate(
    operation: &'static str,
    source: &Path,
) -> (
    Release,
    mpsc::Receiver<()>,
    xirius_core::io_test::Observation,
) {
    let release = Release::new();
    let held = release.0.clone();
    let source = source.to_owned();
    let ui = std::thread::current().id();
    let (tx, entered) = mpsc::channel();
    let observe = xirius_core::io_test::observe(Arc::new(move |op, path| {
        if op == operation && path.starts_with(&source) {
            assert_ne!(std::thread::current().id(), ui);
            let _ = tx.send(());
            drop(
                held.1
                    .wait_while(held.0.lock().unwrap(), |go| !*go)
                    .unwrap(),
            );
        }
        Ok(())
    }));
    (release, entered, observe)
}

fn callbacks(s: &mut Studio) {
    let _ui = xirius_core::io_test::forbid_ui_io();
    for _ in 0..4 {
        s.tick();
        s.idle();
        let g = s.geometry(880.0, 600.0);
        s.key(&g, "escape", false, false);
    }
}

pub(crate) fn request(
    s: &mut Studio,
    name: &str,
    raw: Value,
    opts: RunOptions,
) -> std::thread::JoinHandle<Result<RunResult, CmdError>> {
    request_with_save(s, name, raw, opts, false)
}

pub(crate) fn request_with_save(
    s: &mut Studio,
    name: &str,
    raw: Value,
    opts: RunOptions,
    save: bool,
) -> std::thread::JoinHandle<Result<RunResult, CmdError>> {
    let (bridge, mut jobs) = xirius_mcp::bridge(xirius_session::commands::shared_registry());
    let name = name.to_owned();
    let call = std::thread::spawn(move || bridge.run(&name, raw.as_object().unwrap(), &opts, save));
    let end = Instant::now() + Duration::from_secs(30);
    let job = loop {
        if let Ok(job) = jobs.try_recv() {
            break job;
        }
        assert!(Instant::now() < end, "MCP receipt watchdog");
        std::thread::yield_now();
    };
    let _ui = xirius_core::io_test::forbid_ui_io();
    s.ai_job(job);
    call
}

pub(crate) fn finish(s: &mut Studio) {
    let end = Instant::now() + Duration::from_secs(30);
    while s.saving.is_some() || s.macro_run.is_some() {
        s.tick();
        s.idle();
        assert!(Instant::now() < end, "completion watchdog: {:?}", s.error);
        std::thread::yield_now();
    }
}

#[test]
fn places_commands_from_mcp_return_callbacks_while_reads_and_writes_are_held() {
    for name in xirius_session::places::NAMES {
        for operation in ["prefs_read", "prefs_write"] {
            if name == "location.list" && operation == "prefs_write" {
                continue;
            }
            let (root, mut s) = setup();
            let file = s.song.places_file.clone().unwrap();
            let raw = match name {
                "location.add" => json!({"path":root.path().join("song"),"name":"Added"}),
                "location.rename" => json!({"location":"Existing","name":"Renamed"}),
                "location.remove" => json!({"location":"Existing"}),
                "browser.favorite" | "browser.hide" => json!({"item":"Fixture","on":true}),
                "view.browser.hidden" => json!({"on":true}),
                _ => json!({}),
            };
            s.song
                .run(
                    "ai.permission.set",
                    json!({"level":if name=="location.list" {"read"} else {"save"}}),
                )
                .unwrap();
            s.bind_window_io();
            let (release, entered, _observe) = gate(operation, &file);
            let call = request(&mut s, name, raw, RunOptions::ai("fixture"));
            entered.recv_timeout(Duration::from_secs(30)).unwrap();
            callbacks(&mut s);
            assert!(
                !call.is_finished(),
                "no persistence receipt before I/O completes"
            );
            release.release();
            finish(&mut s);
            assert!(call.join().unwrap().is_ok(), "{name}");
        }
    }
}

#[test]
fn command_and_browser_writer_rebase_under_one_transaction_in_both_orders() {
    for command_first in [false, true] {
        let (_root, mut s) = setup();
        let file = s.song.places_file.clone().unwrap();
        let mut base = xirius_session::places::load(&file).unwrap();
        base.favourites.insert("Shared".into());
        xirius_session::places::save(&file, &base).unwrap();
        let mut next = base.clone();
        next.hidden.insert("Browser edit".into());
        next.favourites.remove("Shared");
        s.bind_window_io();
        let (release, entered, _observe) = gate("prefs_write", &file);
        let mut call = None;
        let mut ticket = None;
        if command_first {
            call = Some(request(
                &mut s,
                "browser.favorite",
                json!({"item":"Shared","on":true}),
                RunOptions::human(),
            ));
        } else {
            ticket = Some(crate::browser_prefs_save::submit(
                &file,
                base.clone(),
                next.clone(),
            ));
        }
        entered.recv_timeout(Duration::from_secs(30)).unwrap();
        if command_first {
            ticket = Some(crate::browser_prefs_save::submit(&file, base, next));
        } else {
            call = Some(request(
                &mut s,
                "browser.favorite",
                json!({"item":"Shared","on":true}),
                RunOptions::human(),
            ));
        }
        callbacks(&mut s);
        release.release();
        finish(&mut s);
        call.unwrap().join().unwrap().unwrap();
        let end = Instant::now() + Duration::from_secs(30);
        while ticket.as_ref().unwrap().result().is_none() {
            assert!(Instant::now() < end);
            std::thread::yield_now();
        }
        assert!(ticket.unwrap().result().unwrap().is_ok());
        let disk = xirius_session::places::load(&file).unwrap();
        assert!(disk.hidden.contains("Browser edit"));
        assert_eq!(disk.favourites.contains("Shared"), !command_first);
    }
}

#[test]
fn batch_and_palette_reads_use_the_owner_and_failed_batches_revert() {
    for palette in [false, true] {
        let (root, mut s) = setup();
        s.bind_window_io();
        let (release, entered, _observe) = gate("pool_read_dir", root.path());
        let call = if palette {
            let g = s.geometry(880.0, 600.0);
            let _ui = xirius_core::io_test::forbid_ui_io();
            assert!(s.palette_query(&g, "file.list", json!({})));
            None
        } else {
            Some(request(
                &mut s,
                "ai.batch",
                json!({"commands":[
                    {"command":"file.list","args":{}},
                    {"command":"track.add.audio","args":{"name":"Must revert"}},
                    {"command":"no.such.command","args":{}}
                ]}),
                RunOptions::human(),
            ))
        };
        entered.recv_timeout(Duration::from_secs(30)).unwrap();
        callbacks(&mut s);
        release.release();
        finish(&mut s);
        if let Some(call) = call {
            assert!(call.join().unwrap().is_err());
        } else {
            assert!(
                s.palette
                    .as_ref()
                    .and_then(|p| p.form.as_ref())
                    .is_some_and(|f| f.output)
            );
        }
        assert!(!s.project().tracks.values().any(|t| t.name == "Must revert"));
    }
}

#[test]
fn macros_resume_on_the_real_session_and_close_the_group_after_async_steps() {
    for before in [false, true] {
        let (root, mut s) = setup();
        let mut steps = vec![
            "version.list".into(),
            "track.add.audio --name MacroTrack".into(),
        ];
        if before {
            steps.insert(0, "track.add.audio --name BeforeAsync".into());
        }
        s.song.settings().lock().unwrap().macros.set("Async", steps);
        s.bind_window_io();
        let (release, entered, _observe) = gate("store_version_list", root.path());
        let g = s.geometry(880.0, 600.0);
        {
            let _ui = xirius_core::io_test::forbid_ui_io();
            assert!(s.run_macro(&g, "Async"));
        }
        entered.recv_timeout(Duration::from_secs(30)).unwrap();
        callbacks(&mut s);
        assert!(!s.project().tracks.values().any(|t| t.name == "MacroTrack"));
        {
            let _ui = xirius_core::io_test::forbid_ui_io();
            assert!(
                s.song
                    .run("track.add.audio", json!({"name":"Forbidden preview"}))
                    .is_err()
            );
        }
        release.release();
        finish(&mut s);
        assert!(
            s.project().tracks.values().any(|t| t.name == "MacroTrack"),
            "{:?}",
            s.error
        );
        s.run_command("track.add.audio", json!({"name":"After"}));
        s.run_command("history.undo", json!({}));
        finish(&mut s);
        assert!(!s.project().tracks.values().any(|t| t.name == "After"));
        assert!(s.project().tracks.values().any(|t| t.name == "MacroTrack"));
        s.run_command("history.undo", json!({}));
        finish(&mut s);
        assert!(!s.project().tracks.values().any(|t| t.name == "MacroTrack"));
        assert!(!s.project().tracks.values().any(|t| t.name == "BeforeAsync"));
    }
}

#[test]
fn cancelled_cleanup_receives_its_cache_without_replacing_a_later_popup() {
    let (root, mut s) = setup();
    for dir in xirius_session::record::take_folders(&s.project().media, &s.song.dir().unwrap()) {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("unused.wav"), b"fixture").unwrap();
    }
    s.bind_window_io();
    let (release, entered, _observe) = gate("cleanup_list", root.path());
    let _ui = xirius_core::io_test::forbid_ui_io();
    assert!(s.open_cleanup());
    entered.recv_timeout(Duration::from_secs(30)).unwrap();
    let g = s.geometry(880.0, 600.0);
    s.key(&g, "escape", false, false);
    s.open_popup(crate::popup::PopupKind::Grid, Rect::default());
    let id = s.popup.as_ref().unwrap().intent_id;
    release.release();
    let end = Instant::now() + Duration::from_secs(30);
    while s.view_read.cleanup.job.is_some() {
        s.tick();
        s.idle();
        assert!(Instant::now() < end);
        std::thread::yield_now();
    }
    assert_eq!(s.popup.as_ref().unwrap().intent_id, id);
}

#[test]
fn accepted_and_refused_tokens_keep_permission_and_one_use_rules_on_the_worker() {
    for yes in [false, true] {
        let (root, mut s) = setup();
        let source = root.path().join("song/spare.wav");
        std::fs::write(&source, b"fixture").unwrap();
        s.song
            .run("ai.permission.set", json!({"level":"edit"}))
            .unwrap();
        s.song
            .session_mut()
            .set_confirm(xirius_core::session::Confirm {
                save: true,
                ..Default::default()
            });
        let held = s
            .ai_run(
                "file.rename",
                json!({"path":"spare.wav","name":"renamed.wav"})
                    .as_object()
                    .unwrap(),
                &RunOptions::ai("fixture"),
                false,
            )
            .unwrap();
        let token = held.decision.expect("file changes require approval").token;
        s.bind_window_io();
        let (release, entered, _observe) = gate("file_rename", root.path());
        let raw = json!({"token":token,"answer":if yes {"yes"}else{"no"}});
        let call = request(&mut s, "ai.decide", raw.clone(), RunOptions::human());
        if yes {
            entered.recv_timeout(Duration::from_secs(30)).unwrap();
            callbacks(&mut s);
        }
        release.release();
        finish(&mut s);
        call.join().unwrap().unwrap();
        assert_eq!(source.exists(), !yes);
        assert_eq!(root.path().join("song/renamed.wav").exists(), yes);
        let repeated = request(&mut s, "ai.decide", raw, RunOptions::human());
        finish(&mut s);
        assert!(repeated.join().unwrap().is_err());
    }
}

#[test]
fn a_failed_macro_step_stops_the_tail_and_closes_the_real_group() {
    let (root, mut s) = setup();
    s.song.settings().lock().unwrap().macros.set(
        "Failure",
        vec![
            "track.add.audio --name BeforeFailure".into(),
            "version.list".into(),
            "track.add.audio --name MustNotRun".into(),
        ],
    );
    s.bind_window_io();
    let (release, entered, _gate) = gate("store_version_list", root.path());
    let source = root.path().to_owned();
    let _fail = xirius_core::io_test::observe(Arc::new(move |op, path| {
        if op == "store_version_list" && path.starts_with(&source) {
            return Err(std::io::Error::other("fixture read refused"));
        }
        Ok(())
    }));
    let g = s.geometry(880.0, 600.0);
    s.run_macro(&g, "Failure");
    entered.recv_timeout(Duration::from_secs(30)).unwrap();
    callbacks(&mut s);
    release.release();
    finish(&mut s);
    assert!(s.error.is_some());
    assert!(
        s.project()
            .tracks
            .values()
            .any(|t| t.name == "BeforeFailure")
    );
    assert!(!s.project().tracks.values().any(|t| t.name == "MustNotRun"));
    s.run_command("track.add.audio", json!({"name":"AfterFailure"}));
    s.run_command("history.undo", json!({}));
    finish(&mut s);
    assert!(
        s.project()
            .tracks
            .values()
            .any(|t| t.name == "BeforeFailure")
    );
    assert!(
        !s.project()
            .tracks
            .values()
            .any(|t| t.name == "AfterFailure")
    );
}

#[test]
fn inline_helpers_and_session_dry_runs_cannot_reintroduce_file_io() {
    let (_root, mut s) = setup();
    s.bind_window_io();
    let _ui = xirius_core::io_test::forbid_ui_io();
    for name in ["location.list", "file.list", "ai.batch", "version.list"] {
        assert!(s.song.run(name, json!({})).is_err(), "{name}");
    }
    assert!(s.song.check("location.list", json!({})).unwrap());
    assert!(s.song.check("file.list", json!({})).unwrap());
}
