//! `--script mp3`: MP3 in the export panel (EXP-005) and 「Xirius について」 under
//! the logo (LIC-015). The song is saved to a scratch folder first, so it can export;
//! the MP3 encoder is the one beside the binary (`scripts/build-lame.ps1`,
//! `scripts/build-lame.sh`).

use xirius_session::mp3;

use crate::export::Act as Xp;
use crate::shot::{Act, Play, Shot, Target};
use crate::studio::Studio;
use crate::topbar::BarButton;

/// Stage `n` of this script, played by [`stage`].
const fn staged(n: u8) -> Act {
    Act::Stage(Play::Studio(stage), n)
}

pub const MP3: &[Shot] = &[
    // MP3 chosen: the bitrates under the format, and why 96 kHz cannot be.
    Shot {
        name: "x01-mp3-chosen",
        acts: &[
            staged(0),
            Act::Press(Target::Top(BarButton::Export)),
            staged(1),
            Act::Hover(Target::PopupTip(mp3::OVER_48K)),
        ],
    },
    // A mixdown written: its loudness and true peak from the MP3, its title in a balloon.
    Shot {
        name: "x02-mp3-done",
        acts: &[staged(2), Act::Hover(Target::PopupTip("タイトル"))],
    },
    // Stems: each file titled by its track, the song as the album.
    Shot {
        name: "x03-mp3-stems",
        acts: &[staged(3), Act::Hover(Target::PopupTip("タイトル"))],
    },
    // The encoder cannot be loaded: said in red left of the button, which cannot be pressed.
    Shot {
        name: "x04-mp3-no-encoder",
        acts: &[staged(4), Act::Hover(Target::Top(BarButton::Export))],
    },
    // 「Xirius について」 from the logo, LAME's row open.
    Shot {
        name: "x05-about",
        acts: &[staged(5), Act::Press(Target::Top(BarButton::Logo))],
    },
];

/// Where [`MP3`] keeps its song.
pub fn dir() -> std::path::PathBuf {
    std::env::temp_dir().join("xirius-mp3-shots")
}

/// Plays stage `stage` of [`MP3`].
pub fn stage(s: &mut Studio, stage: u8) {
    let exported = |s: &Studio| !s.export.running();
    match stage {
        0 => {
            let dir = dir();
            let _ = std::fs::remove_dir_all(&dir);
            s.command(|song| song.save_as(&dir));
        }
        1 => {
            s.export_action(Xp::Format(xirius_session::export::Format::Mp3));
            s.export_action(Xp::Rate(48_000));
            s.export_action(Xp::Kbps(320));
        }
        2 => {
            s.export_action(Xp::Go);
            crate::shot::run_for(s, 30.0, exported);
        }
        3 => {
            s.export_action(Xp::Mode(true));
            s.export_action(Xp::Kbps(192));
            s.export_action(Xp::Go);
            crate::shot::run_for(s, 60.0, exported);
        }
        4 => {
            s.export_action(Xp::Mode(false));
            s.export.mp3_error = Some(mp3::CANNOT_LOAD.to_string());
            s.refresh_popup();
        }
        5 => s.popup = None,
        _ => {}
    }
}
