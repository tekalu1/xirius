//! `--script update` (UI-282): 「Xirius について」 with a newer version, while it is
//! downloaded, once it is checked, and when the download failed. The states are laid on the
//! studio, so no network and no installer is touched.

use std::path::PathBuf;

use xirius_session::update;

use crate::shot::{Act, Play, Shot, Target};
use crate::studio::Studio;
use crate::topbar::BarButton;

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "update-01-notice",
        acts: &[
            Act::Stage(Play::Studio(stage), 0),
            Act::Press(Target::Top(BarButton::Logo)),
        ],
    },
    Shot {
        name: "update-02-downloading",
        acts: &[Act::Stage(Play::Studio(stage), 1)],
    },
    Shot {
        name: "update-03-ready",
        acts: &[Act::Stage(Play::Studio(stage), 2)],
    },
    Shot {
        name: "update-04-failed",
        acts: &[Act::Stage(Play::Studio(stage), 3)],
    },
];

/// The release the pictures show.
pub fn newer() -> update::Release {
    update::Release {
        version: "1.1.0".into(),
        url: "https://github.com/tekalu1/xirius/releases/tag/v1.1.0".into(),
        notes: vec![
            "録音の開始が速くなりました".into(),
            "MIDI の入力の不具合を修正しました".into(),
        ],
        prerelease: false,
        assets: Vec::new(),
    }
}

/// A folder that is both where the app runs from and where the installer puts it.
fn folder() -> PathBuf {
    std::env::temp_dir().join("xirius-update-shots")
}

/// `0` the notice, `1` downloading, `2` checked, `3` failed.
pub fn stage(s: &mut Studio, stage: u8) {
    let dir = folder();
    s.update.newer = Some(newer());
    s.update.key = "1".repeat(64);
    s.update.location = Some(update::Location {
        exe_dir: dir.clone(),
        install_dir: dir.clone(),
    });
    s.update.ready = None;
    s.update.set_progress(update::Progress::default());
    match stage {
        1 => s.update.set_progress(update::Progress {
            phase: update::Phase::Downloading,
            got: 42,
            total: 100,
        }),
        2 => {
            let manifest = update::Manifest {
                version: "1.1.0".into(),
                file: update::setup_name("1.1.0"),
                sha256: "00".repeat(32),
                channel: update::STABLE.into(),
                min_version: "1.0.0".into(),
            };
            let manifest_bytes = serde_json::to_vec(&manifest).expect("the manifest is written");
            s.update.ready = Some(update::Installer {
                path: dir.join(update::setup_name("1.1.0")),
                version: "1.1.0".into(),
                manifest,
                manifest_bytes,
                signature: String::new(),
                sums: String::new(),
            });
            s.update.set_progress(update::Progress {
                phase: update::Phase::Ready,
                got: 100,
                total: 100,
            });
        }
        3 => s.update.set_progress(update::Progress {
            phase: update::Phase::Failed(update::BAD_SIGNATURE.into()),
            got: 100,
            total: 100,
        }),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::about::{AboutLook, Act as AboutAct};
    use crate::popup::{Action, Look};
    use crate::studio::Hit;

    fn cells(s: &Studio, g: &crate::studio::Geometry) -> Vec<crate::popup::Cell> {
        s.popup.as_ref().unwrap().layout(g.size).cells
    }

    fn acts(cells: &[crate::popup::Cell]) -> Vec<AboutAct> {
        cells
            .iter()
            .filter_map(|c| match c.action {
                Some(Action::About(act)) => Some(act),
                _ => None,
            })
            .collect()
    }

    /// UI-282: the four pictures of the update, each with the button of that moment.
    #[test]
    fn the_four_states_show_the_download_and_what_to_do_with_it() {
        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 900.0);
        stage(&mut s, 0);
        s.press(&g, Hit::Bar(BarButton::Logo));
        let first = cells(&s, &g);
        assert!(acts(&first).contains(&AboutAct::Download));
        assert_eq!(
            first
                .iter()
                .find(|c| c.look == Look::Primary)
                .map(|c| c.text.as_str()),
            Some("ダウンロード")
        );

        for (stage_no, text, act) in [
            (1, "ダウンロード 42%", None),
            (2, "更新して再起動", Some(AboutAct::UpdateNow)),
            (3, update::BAD_SIGNATURE, Some(AboutAct::Page)),
        ] {
            stage(&mut s, stage_no);
            s.poll_update();
            let shown = cells(&s, &g);
            assert!(
                shown.iter().any(|c| c.text == text),
                "{stage_no}: {text} is not shown"
            );
            let primary = shown
                .iter()
                .find(|c| {
                    c.look == Look::Primary || matches!(c.look, Look::About(AboutLook::Busy(_)))
                })
                .unwrap();
            match act {
                Some(act) => assert!(acts(&shown).contains(&act), "{stage_no}: {act:?}"),
                None => assert_eq!(primary.action, None, "nothing to press while it goes"),
            }
            // The progress is said once, in the button (UI-282).
            assert!(
                shown
                    .iter()
                    .filter(|c| c.text.starts_with("ダウンロード "))
                    .count()
                    <= 1,
                "{stage_no}: the progress is said twice"
            );
            // Nothing ran: the pictures only set the state.
            assert!(s.update.pending.is_none());
        }
        // The failure's line is the alert's colour.
        assert!(
            cells(&s, &g)
                .iter()
                .any(|c| c.look == Look::About(AboutLook::Alert) && c.text == update::BAD_SIGNATURE)
        );
    }
}
