use crate::{
    shot::{Act, Play, Shot, Target},
    studio::Studio,
    topbar::BarButton,
};
use serde_json::json;

/// Locate the fixture used by device-free scenarios; Mac tests construct a real bundle.
pub(crate) fn fixture_path() -> std::path::PathBuf {
    let base = std::env::current_exe().unwrap();
    let base = base.parent().unwrap();
    #[cfg(all(test, target_os = "macos"))]
    {
        xirius_test_vst3::fixture_path(base)
    }
    #[cfg(not(all(test, target_os = "macos")))]
    {
        let name = if cfg!(target_os = "macos") {
            "xirius_test_vst3.vst3"
        } else {
            "xirius_test_vst3.dll"
        };
        [base.join(name), base.join("deps").join(name)]
            .into_iter()
            .find(|p| p.exists())
            .expect("build xirius-test-vst3 (a bundle on macOS) first")
    }
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "fixture01-shared-exit",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Studio(prepare), 0),
            Act::Press(Target::Top(BarButton::Play)),
            Act::Stage(Play::Studio(crate::plugin_crash_shots::crash), 0),
        ],
    },
    Shot {
        name: "fixture02-reload",
        acts: &[
            Act::Press(Target::Top(BarButton::Palette)),
            Act::Key("r"),
            Act::Key("e"),
            Act::Key("l"),
            Act::Key("o"),
            Act::Key("a"),
            Act::Key("d"),
        ],
    },
    Shot {
        name: "fixture03-restored",
        acts: &[
            Act::Key("enter"),
            Act::Stage(Play::Studio(crate::plugin_reload_shots::restored), 0),
            Act::Press(Target::Top(BarButton::Stop)),
        ],
    },
];

fn prepare(s: &mut Studio, _: u8) {
    crate::plugin_reload_shots::prepare(s, 0);
    assert!(s.run_command(
        "plugin.isolation.set",
        json!({
            "ids":[crate::plugin_reload_shots::id(s).to_string()], "mode":"shared"
        })
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use xirius_core::session::RunOptions;

    #[test]
    fn the_fixture_exits_from_its_processor_and_reload_restores_shared_and_dedicated_instances() {
        for shared in [true, false] {
            let mut s = Studio::sample();
            if shared {
                prepare(&mut s, 0);
            } else {
                crate::plugin_reload_shots::prepare(&mut s, 0);
            }
            assert!(s.toggle_play());
            let project = s.project();
            let steps = s.song().steps();
            crate::plugin_crash_shots::crash(&mut s, 0);
            let result = xirius_session::commands::run(
                s.song.session_mut(),
                s.transport.host_mut(),
                "plugin.reload",
                &Default::default(),
                &RunOptions::human(),
            )
            .unwrap();
            assert_eq!(result.command, "plugin.reload");
            assert!(result.step.is_none());
            crate::plugin_reload_shots::restored(&mut s, 0);
            assert_eq!(*s.project(), *project);
            assert_eq!(s.song().steps(), steps);
            s.transport.pause();
        }
    }
}
