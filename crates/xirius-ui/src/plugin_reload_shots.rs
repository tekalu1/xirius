use crate::{
    shot::{Act, Play, Shot, Target},
    studio::Studio,
    topbar::BarButton,
};
use serde_json::json;
use xirius_core::id::PluginId;

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "reload01-crashed",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Studio(prepare), 0),
            Act::Press(Target::Top(BarButton::Play)),
            Act::Stage(Play::Studio(crash), 0),
        ],
    },
    Shot {
        name: "reload02-palette",
        acts: &[
            Act::Press(Target::Top(BarButton::Palette)),
            Act::Key("r"),
            Act::Key("e"),
            Act::Key("l"),
            Act::Key("o"),
            Act::Key("a"),
            Act::Key("d"),
            Act::Stage(Play::Studio(selected), 0),
        ],
    },
    Shot {
        name: "reload03-restored",
        acts: &[
            Act::Key("enter"),
            Act::Stage(Play::Studio(restored), 0),
            Act::Press(Target::Top(BarButton::Stop)),
        ],
    },
];

pub(crate) fn id(s: &Studio) -> PluginId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == "Vocal")
        .unwrap()
        .inserts[0]
        .id
}

/// Tests use the host prepared with the other executables by scripts/ci_tests.py.
#[cfg(test)]
pub(crate) fn require_plugin_host() {
    let host = xirius_engine::server::find_host()
        .expect("prepare test executables with python scripts/ci_tests.py --prepare");
    let exe = std::env::current_exe().unwrap();
    let profile = exe.parent().unwrap().parent().unwrap();
    let expected = profile.join(format!(
        "xirius-plugin-host{}",
        std::env::consts::EXE_SUFFIX
    ));
    assert_eq!(
        host.canonicalize().unwrap(),
        expected.canonicalize().unwrap(),
        "the host must belong to this test build"
    );
    let mut command = std::process::Command::new(host);
    command.arg("--version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().expect("read prepared host version");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        format!(
            "xirius-plugin-host {} protocol {}",
            env!("CARGO_PKG_VERSION"),
            xirius_engine::wire::PROTOCOL
        )
    );
}

pub(crate) fn prepare(s: &mut Studio, _: u8) {
    #[cfg(test)]
    require_plugin_host();
    let dll = crate::plugin_fixture_shots::fixture_path();
    let entries = xirius_plugins::probe(&dll).unwrap();
    let entry = &entries[0];
    let track = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Vocal")
        .unwrap()
        .id;
    let prior = s.project().tracks[&track]
        .inserts
        .iter()
        .map(|p| p.id.to_string())
        .collect::<Vec<_>>();
    if !prior.is_empty() {
        assert!(s.run_command("plugin.remove", json!({"ids":prior})));
    }
    assert!(s.run_command(
        "plugin.add",
        json!({
            "track":track.to_string(), "name":entry.name, "kind":"effect",
            "format":"vst3", "uid":entry.uid
        })
    ));
    let id = id(s);
    assert!(s.run_command(
        "plugin.isolation.set",
        json!({"ids":[id.to_string()], "mode":"dedicated"})
    ));
    assert!(s.run_command(
        "plugin.param.set",
        json!({"id":id.to_string(), "param":"1", "value":0.75})
    ));
    s.use_plugins(xirius_plugins::Catalog {
        entries,
        failures: Vec::new(),
    });
    s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
    s.drawer_open = true;
    s.tab = crate::studio::DrawerTab::Mixer;
}

pub(crate) fn ready(s: &mut Studio) {
    assert!(
        xirius_engine::server::find_host().is_some(),
        "xirius-plugin-host is not built: run `cargo build -p xirius-engine --bin xirius-plugin-host`"
    );
    let id = id(s);
    crate::shot::run_for(s, 15.0, |s| {
        !s.sound_preparing()
            && s.error.is_none()
            && s.transport.host().is_some_and(|h| {
                h.plugins().reports().iter().any(|r| {
                    r.id == id
                        && r.status.latency == xirius_engine::server::DEFAULT_LATENCY + 192
                        && !r.status.crashed
                })
            })
    });
    assert!(
        s.error.is_none(),
        "error={:?}, preparation={:?}",
        s.error,
        s.preparing
    );
    let reports = s.transport.host().unwrap().plugins().reports();
    assert!(
        reports.iter().any(|r| r.id == id
            && r.status.latency == xirius_engine::server::DEFAULT_LATENCY + 192
            && !r.status.crashed),
        "{reports:?}"
    );
}

fn crash(s: &mut Studio, _: u8) {
    ready(s);
    let id = id(s);
    let host = s.transport.host().unwrap();
    let link = host.plugins().link(id).unwrap();
    host.plugins()
        .server_of(id)
        .unwrap()
        .fault(&link, xirius_engine::wire::Fault::Crash, 1);
    crate::shot::run_for(s, 15.0, |s| s.health.crashed(id));
    s.poll_plugins_now();
    assert!(s.health.fake.is_none());
    assert!(s.health.crashed(id));
}

fn selected(s: &mut Studio, _: u8) {
    let row = s.palette.as_ref().unwrap().current().unwrap();
    assert_eq!(row.name, "plugin.reload");
    assert_eq!(row.label, "プラグインを再読み込み");
    assert!(row.enabled);
}

pub(crate) fn restored(s: &mut Studio, _: u8) {
    ready(s);
    let id = id(s);
    let host = s.transport.host().unwrap();
    assert_eq!(
        host.plugins().link(id).unwrap().opened().unwrap().latency,
        192
    );
    let before = host
        .plugins()
        .reports()
        .into_iter()
        .find(|r| r.id == id)
        .unwrap()
        .status
        .wet_blocks;
    crate::shot::run_for(s, 5.0, |s| {
        s.transport
            .host()
            .unwrap()
            .plugins()
            .reports()
            .iter()
            .any(|r| r.id == id && r.status.wet_blocks > before)
    });
    assert!(
        s.transport
            .host()
            .unwrap()
            .plugins()
            .reports()
            .iter()
            .any(|r| r.id == id && r.status.wet_blocks > before)
    );
    s.poll_plugins_now();
    assert!(!s.health.crashed(id));
    assert_eq!(
        s.project()
            .tracks
            .values()
            .find(|t| t.name == "Vocal")
            .unwrap()
            .inserts[0]
            .params["1"],
        0.75
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use xirius_core::session::RunOptions;

    #[test]
    fn reload_restarts_the_running_host_without_reopening_the_song_or_editing_history() {
        xirius_test_vst3::fixture_marker();
        let mut s = Studio::sample();
        prepare(&mut s, 0);
        assert!(s.toggle_play());
        crash(&mut s, 0);
        let project = s.project();
        let steps = s.song().steps();
        let dry = RunOptions {
            dry_run: true,
            ..RunOptions::human()
        };
        let r = xirius_session::commands::run(
            s.song.session_mut(),
            s.transport.host_mut(),
            "plugin.reload",
            &Default::default(),
            &dry,
        )
        .unwrap();
        assert!(r.dry_run);
        assert!(
            s.transport
                .host()
                .unwrap()
                .plugins()
                .reports()
                .iter()
                .any(|r| r.id == id(&s) && r.status.crashed)
        );
        s.poll_plugins_now();
        assert!(s.health.crashed(id(&s)));
        let r = xirius_session::commands::run(
            s.song.session_mut(),
            s.transport.host_mut(),
            "plugin.reload",
            &Default::default(),
            &RunOptions::human(),
        )
        .unwrap();
        assert_eq!(r.command, "plugin.reload");
        assert!(r.step.is_none());
        restored(&mut s, 0);
        assert_eq!(*s.project(), *project);
        assert_eq!(s.song().steps(), steps);
        s.transport.pause();
    }

    #[test]
    fn the_palette_reload_row_is_enabled_only_for_a_crash_and_uses_the_live_host() {
        xirius_test_vst3::fixture_marker();
        let mut s = Studio::sample();
        prepare(&mut s, 0);
        let g = s.geometry(1600.0, 900.0);
        assert!(
            !s.palette_rows(&g)
                .iter()
                .find(|r| r.name == "plugin.reload")
                .unwrap()
                .enabled
        );
        assert!(s.toggle_play());
        crash(&mut s, 0);
        let steps = s.song().steps();
        let row = s
            .palette_rows(&g)
            .into_iter()
            .find(|r| r.name == "plugin.reload")
            .unwrap();
        assert!(s.run_row(&g, &row));
        restored(&mut s, 0);
        assert_eq!(s.health.restarts, 1);
        assert_eq!(s.song().steps(), steps);
        s.transport.pause();
    }
}
