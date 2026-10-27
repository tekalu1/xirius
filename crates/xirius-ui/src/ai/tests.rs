use std::sync::Arc;

use serde_json::{Value, json};
use xirius_core::AiPermission;
use xirius_core::model::Note;
use xirius_core::session::RunOptions;
use xirius_mcp::{Gateway, Jobs, Scope, bridge};

use super::*;
use crate::popup::{Action, Look, PopupKind};
use crate::studio::{Geometry, Hit};
use crate::topbar::BarButton;

/// The sample song, its roll on the Keys clip, and a fixed clock.
fn studio() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.ai.clock = Some(1_000_000);
    let g = s.geometry(1600.0, 1000.0);
    (s, g)
}

fn notes(s: &Studio) -> Vec<Note> {
    s.roll_target()
        .expect("the Keys clip in the roll")
        .notes
        .to_vec()
}

fn clip_id(s: &Studio) -> String {
    s.roll_target().unwrap().clip.id.to_string()
}

/// The pitches, and whether the AI changed each last.
fn pitches(s: &Studio) -> Vec<(u8, bool)> {
    notes(s).iter().map(|n| (n.pitch, n.ai)).collect()
}

/// One run as the AI, as a call over MCP would make it.
fn ai(s: &mut Studio, command: &str, args: Value) -> Result<RunResult, CmdError> {
    let raw = args.as_object().cloned().unwrap_or_default();
    s.ai_run(command, &raw, &RunOptions::ai("AI"), true)
}

/// Opens the pop-over from the top bar and presses the cell with `action`.
fn press_in_popover(s: &mut Studio, g: &Geometry, action: Action) {
    if s.popup.as_ref().map(|p| p.kind) != Some(PopupKind::Ai) {
        assert!(s.press(g, Hit::Bar(BarButton::Ai)));
    }
    let layout = s.popup.as_ref().expect("the AI's pop-over").layout(g.size);
    let i = layout
        .cells
        .iter()
        .position(|c| c.action.as_ref() == Some(&action))
        .unwrap_or_else(|| panic!("{action:?} in the pop-over"));
    assert!(s.press(g, Hit::Item(i)));
}

fn set_level(s: &mut Studio, g: &Geometry, p: AiPermission) {
    press_in_popover(s, g, Action::Ai(Act::Level(p)));
    assert_eq!(s.song.session().ai_permission(), p);
    // The person finishes the permission choice before the next external request.
    assert!(s.key(g, "escape", false, false));
}

/// The AI and the window, as the app wires them: the call waits on its own
/// thread while the studio answers each run that comes in.
struct Wire {
    gateway: Arc<Gateway>,
    jobs: Jobs,
}

impl Wire {
    fn new() -> Wire {
        let (host, jobs) = bridge(xirius_session::commands::shared_registry());
        Wire {
            gateway: Arc::new(Gateway::hosted(host, "AI", Scope::Edit)),
            jobs,
        }
    }

    /// What the AI hears back: the result's JSON and whether it is an error.
    fn call(&mut self, s: &mut Studio, tool: &str, args: Value) -> (Value, bool) {
        let gateway = self.gateway.clone();
        let tool = tool.to_string();
        let raw = args.as_object().cloned().unwrap_or_default();
        let call = std::thread::spawn(move || gateway.call(&tool, &raw));
        loop {
            if let Ok(job) = self.jobs.try_recv() {
                s.ai_job(job);
            } else if call.is_finished() {
                break;
            } else {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        call.join().unwrap().expect("a tool of the table")
    }
}

fn why(v: &Value) -> &str {
    v["error"]["message"].as_str().unwrap_or_default()
}

// ---- the four levels (AI-023, AI-025..AI-028) ----------------------------------------

#[test]
fn each_level_answers_the_ai_over_mcp_and_says_why() {
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-ui-ai-levels-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("ui-ai-levels");
    let _ = std::fs::remove_dir_all(&dir);
    let (mut s, g) = studio();
    s.song.session_mut().save_as(&dir).unwrap();
    let mut wire = Wire::new();
    let clip = clip_id(&s);
    let transpose = json!({"ids": [clip], "by": 2});
    let before = pitches(&s);
    assert!(
        before.len() >= 2 && before.iter().all(|(_, ai)| !ai),
        "{before:?}"
    );
    let up: Vec<(u8, bool)> = before.iter().map(|(p, _)| (p + 2, true)).collect();

    // The default is 編集.
    assert_eq!(s.song.session().ai_permission(), AiPermission::Edit);
    assert_eq!(s.bar_view().ai.level, AiPermission::Edit);

    // オフ: nothing runs, not even a look, and the result says why.
    set_level(&mut s, &g, AiPermission::Off);
    for (tool, args) in [
        ("note_transpose", transpose.clone()),
        ("track_list", json!({})),
    ] {
        let (v, error) = wire.call(&mut s, tool, args);
        assert!(error, "{v}");
        assert_eq!(v["error"]["code"], "denied", "{v}");
        assert!(why(&v).contains("AI の権限がオフです"), "{v}");
    }
    assert_eq!(pitches(&s), before);
    assert!(s.ai.last.as_ref().unwrap().refused);

    // 読み取りのみ: a look runs; an edit comes back as the dry-run proposal only.
    set_level(&mut s, &g, AiPermission::Read);
    let (v, error) = wire.call(&mut s, "track_list", json!({}));
    assert!(!error, "{v}");
    let steps = s.song.steps();
    let (v, error) = wire.call(&mut s, "note_transpose", transpose.clone());
    assert!(!error, "{v}");
    assert_eq!(v["dry_run"], true, "{v}");
    assert!(v["message"].as_str().unwrap().contains("ドライラン"), "{v}");
    assert_eq!(pitches(&s), before, "a proposal changes nothing");
    assert_eq!(s.song.steps(), steps);
    let last = s.ai.last.clone().unwrap();
    assert!(last.refused && last.label.starts_with(AI_ROW), "{last:?}");
    assert!(last.text.starts_with("ドライラン: "), "{last:?}");
    assert!(
        last.text.contains("ノート 12 件"),
        "the proposal says how many notes: {last:?}"
    );

    // 編集: the edit lands as the AI's; saving is refused with the reason.
    set_level(&mut s, &g, AiPermission::Edit);
    let (v, error) = wire.call(&mut s, "note_transpose", transpose.clone());
    assert!(!error, "{v}");
    assert_eq!(pitches(&s), up);
    let (v, error) = wire.call(&mut s, "project_save", json!({}));
    assert!(error, "{v}");
    assert_eq!(v["error"]["code"], "denied", "{v}");
    assert!(
        why(&v).contains("保存には「編集と保存・書き出し」の権限が必要です"),
        "{v}"
    );
    assert!(s.song.session().is_dirty(), "編集 does not save");

    // 編集と保存・書き出し: saving runs.
    set_level(&mut s, &g, AiPermission::Save);
    let (v, error) = wire.call(&mut s, "project_save", json!({}));
    assert!(!error, "{v}");
    assert!(!s.song.session().is_dirty());

    // The pop-over tells the level and what happened last.
    let view = s.ai_view();
    assert_eq!(view.level, AiPermission::Save);
    assert!(!view.last.unwrap().refused);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- marks, history, undo (AI-005, AI-006, AI-030, EDT-005, EDT-010) ------------------

#[test]
fn the_ais_edit_is_marked_named_and_undone_in_one_step() {
    let (mut s, g) = studio();
    let before = pitches(&s);
    let clip = clip_id(&s);
    let r = ai(&mut s, "note.transpose", json!({"ids": [clip], "by": 3})).unwrap();
    assert!(r.step.is_some());

    // The notes and their clip carry the AI's edge.
    assert!(notes(&s).iter().all(|n| n.ai));
    let project = s.project();
    let held = project
        .tracks
        .values()
        .flat_map(|t| t.clips.iter())
        .find(|c| c.id.to_string() == clip)
        .unwrap();
    assert!(clip_marked(&s, held));

    // The history row says the AI did it, and it glows once.
    assert!(s.press(&g, Hit::Bar(BarButton::History)));
    let history = s.popup.as_ref().unwrap().history.clone();
    let row = history.last().unwrap();
    assert!(row.ai && row.current, "{row:?}");
    assert!(row.label.starts_with("AI · "), "{row:?}");
    assert!(!row.label.contains('.'), "a person's words: {row:?}");
    assert_eq!(s.ai.fresh().map(|(n, _)| n), Some(row.n));

    // One undo puts the song back, edges and all.
    assert!(s.song.undo());
    s.after_edit();
    assert_eq!(pitches(&s), before);
    assert!(!clip_marked(
        &s,
        s.project()
            .tracks
            .values()
            .flat_map(|t| t.clips.iter())
            .find(|c| c.id.to_string() == clip)
            .unwrap()
    ));
}

#[test]
fn a_note_a_person_touches_loses_the_ais_edge() {
    let (mut s, _) = studio();
    let clip = clip_id(&s);
    ai(&mut s, "note.transpose", json!({"ids": [clip], "by": 1})).unwrap();
    let all = notes(&s);
    let touched = all[0].id.to_string();
    assert!(s.ai_marked(&touched));
    assert!(s.run_command("note.transpose", json!({"ids": [touched], "by": 1})));
    let after = notes(&s);
    let n = after.iter().find(|n| n.id.to_string() == touched).unwrap();
    assert!(!n.ai && !s.ai_marked(&touched), "{n:?}");
    assert!(
        after
            .iter()
            .filter(|n| n.id.to_string() != touched)
            .all(|n| n.ai),
        "the others keep theirs"
    );
}

// ---- the pop-over (AI-003, AI-022) -------------------------------------------------

#[test]
fn the_popover_shows_the_levels_and_copies_where_to_connect() {
    let (mut s, g) = studio();
    assert!(s.press(&g, Hit::Bar(BarButton::Ai)));
    let cells = s.popup.as_ref().unwrap().layout(g.size).cells;
    let levels: Vec<(&str, bool)> = cells
        .iter()
        .filter(|c| matches!(c.look, Look::Ai(AiLook::Seg)))
        .map(|c| (c.text.as_str(), c.on))
        .collect();
    assert_eq!(
        levels,
        [
            ("オフ", false),
            ("読み取りのみ", false),
            ("編集", true),
            ("編集と保存・書き出し", false),
        ]
    );
    // No server: nothing to copy.
    let copy = cells.iter().find(|c| c.text == "接続情報をコピー").unwrap();
    assert!(copy.action.is_none());
    assert!(cells.iter().any(|c| c.text == "未接続"));
    assert!(
        !cells
            .iter()
            .any(|c| matches!(c.look, Look::Ai(AiLook::About)))
    );
    let mut view = s.ai_view();
    view.last = Some(Last {
        label: "AI · ノートを追加".into(),
        text: "編集を許可していません".into(),
        refused: true,
    });
    let last = super::cells(&view)
        .into_iter()
        .find(|c| matches!(c.look, Look::Ai(AiLook::Last { .. })))
        .unwrap();
    assert!(last.hint.is_empty());
    assert_eq!(last.tip, "編集を許可していません");

    s.ai.connection = Some(Ok(Connection {
        url: "http://127.0.0.1:4100/mcp".into(),
        token: "t0ken".into(),
    }));
    s.refresh_popup();
    press_in_popover(&mut s, &g, Action::Ai(Act::Copy));
    let text = s.ai.copy.take().expect("text for the clipboard");
    let v: Value = serde_json::from_str(&text).unwrap();
    let server = &v["mcpServers"]["xirius"];
    assert_eq!(server["url"], "http://127.0.0.1:4100/mcp");
    assert_eq!(server["headers"]["Authorization"], "Bearer t0ken");
    let cells = s.popup.as_ref().unwrap().layout(g.size).cells;
    assert!(cells.iter().any(|c| c.text == "コピーしました"));
}

/// The levels say what the AI may do as a product says it: the one who saves is
/// the user, never "a person" (AI-012).
#[test]
fn the_levels_name_the_user_not_a_person() {
    let about: Vec<&str> = LEVELS.iter().map(|l| l.about).collect();
    assert_eq!(
        about,
        [
            "AI からの操作をすべて拒否します",
            "変更は実行せず、ドライランの結果を返します",
            "保存と書き出しはユーザーが行います",
            "AI も保存と書き出しを行えます",
        ]
    );
    for line in about.iter().chain([&READ_ONLY, &READ_ONLY_PLAN]) {
        assert!(!line.contains('人') && !line.contains('案'), "{line}");
        assert!(
            line.ends_with("ます") || line.ends_with("ました") || line.ends_with(": "),
            "{line}"
        );
    }
}

// ---- motion (AI-036, AI-037) ---------------------------------------------------------

#[test]
fn the_ais_motion_runs_about_a_second_and_stops_for_reduced_motion() {
    let (mut s, _) = studio();
    let clip = clip_id(&s);
    ai(&mut s, "note.transpose", json!({"ids": [clip], "by": 1})).unwrap();
    let at = s.song.session().ai_activity().updated_ms;
    s.ai.clock = Some(at);
    s.ai.fresh = s.ai.fresh.map(|(n, _)| (n, at));
    assert!(s.ai_working() && s.ai_moving());
    assert!(s.ai.fresh().is_some());
    assert!(s.bar_view().ai.working);

    // The stars twinkle within a loop and come back to rest.
    let star = |s: &mut Studio, ms: u64| {
        s.ai.clock = Some(ms);
        s.ai_bar().star
    };
    let rest = star(&mut s, at - at % u64::from(AI_LOOP_MS));
    let top = star(
        &mut s,
        at - at % u64::from(AI_LOOP_MS) + u64::from(AI_LOOP_MS) / 2,
    );
    assert!(
        (rest.0 - 1.0).abs() < 1e-3 && top.0 > 1.2 && top.1 < 0.5,
        "{rest:?} {top:?}"
    );

    // The history glow ends after a loop; the glow on the work after LIVE_MS.
    s.ai.clock = Some(at + u64::from(AI_LOOP_MS));
    assert!(s.ai.fresh().is_none());
    s.ai.clock = Some(at + xirius_core::session::LIVE_MS);
    assert!(!s.ai_working() && !s.ai_moving());
    assert!(s.ai_marked(&notes(&s)[0].id.to_string()), "the edge stays");

    // Reduced motion: nothing moves, the history badge stays lit.
    s.ai.clock = Some(at);
    s.ai.motion.reduced = true;
    assert!(!s.ai_moving());
    assert_eq!(s.ai.flicker(0), (1.0, 1.0));
    assert_eq!(s.ai_bar().star, (1.0, 1.0));
    s.ai.clock = Some(at + 60_000);
    assert_eq!(s.ai.fresh().map(|(_, t)| t), Some(0.0));
}

#[test]
fn the_curves_start_at_zero_and_end_at_one() {
    for e in [Easing::OUT, Easing::ORGANIC] {
        assert!(ease(e, 0.0).abs() < 1e-3);
        assert!((ease(e, 1.0) - 1.0).abs() < 1e-3);
        assert!(ease(e, 0.25) < ease(e, 0.75));
    }
}

// ---- job.cancel (EXP-047) --------------------------------------------------------------

#[test]
fn the_ai_lists_and_cancels_a_bounce_the_window_runs_and_the_song_stays() {
    let (mut s, _) = studio();
    let mut wire = Wire::new();
    let vocal = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Vocal")
        .map(|t| t.id)
        .unwrap();
    let steps = s.song.steps();
    let tracks = s.project().tracks.len();
    // Long enough to be stopped while it renders.
    let to = 3840 * 400;
    assert!(s.job_start(
        "range.bounce",
        json!({"from": 0, "to": to, "tracks": [vocal.to_string()], "tail": 0}),
    ));
    let id = s.job.as_ref().unwrap().id.clone();

    let (v, error) = wire.call(&mut s, "job_list", json!({}));
    assert!(!error, "{v}");
    let job = v["data"]["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["id"] == id.as_str())
        .cloned()
        .unwrap_or_else(|| panic!("{id} in {v}"));
    assert_eq!(job["command"], "range.bounce");

    let (v, error) = wire.call(&mut s, "job_cancel", json!({"id": id}));
    assert!(!error, "{v}");
    assert_eq!(v["message"], "処理 1 件をキャンセルしました");
    s.job_wait();
    assert!(s.job.is_none());
    assert_eq!(s.song.steps(), steps);
    assert_eq!(s.project().tracks.len(), tracks);
    assert!(s.toast_shown().is_none(), "a cancel is not a failure");

    // Nothing runs now.
    let (v, error) = wire.call(&mut s, "job_cancel", json!({"id": id}));
    assert!(error, "{v}");
}

#[test]
fn the_mcp_server_setting_turned_off_starts_no_server() {
    let (mut s, _) = studio();
    s.rec_set(
        "prefs.set",
        json!({"key": "ai.mcp.enabled", "value": "false"}),
    );
    assert!(s.error.is_none(), "{:?}", s.error);
    assert!(serve::start(&mut s).is_none());
    // The pop-over says why and has nothing to copy.
    let cells = super::cells(&s.ai_view());
    assert!(cells.iter().any(|c| c.text == serve::OFF));
    let copy = cells.iter().find(|c| c.text == "接続情報をコピー").unwrap();
    assert!(copy.action.is_none());
}
