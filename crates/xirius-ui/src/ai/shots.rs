//! The AI's pictures (`--script ai`): the pop-over at each level, its work
//! as it runs and once it rests, and a person's touch. The clock is fixed,
//! so each picture is the same moment every run; no server is started.

use serde_json::json;
use xirius_core::AiPermission;
use xirius_core::session::{LIVE_MS, RunOptions};

use super::{Act as AiAct, Connection};
use crate::shot::{Act, Play, Shot, Target};
use crate::studio::Studio;
use crate::topbar::BarButton;

const OPEN: Act = Act::Press(Target::Top(BarButton::Ai));
const HISTORY: Act = Act::Press(Target::Top(BarButton::History));

/// Stage `n` of this script, played by [`stage`].
const fn staged(n: u8) -> Act {
    Act::Stage(Play::Studio(stage), n)
}

pub const AI: &[Shot] = &[
    Shot {
        name: "00-popover-edit",
        acts: &[Act::Reset, staged(0), OPEN],
    },
    Shot {
        name: "01-popover-read-refused",
        acts: &[staged(1)],
    },
    Shot {
        name: "02-working",
        acts: &[staged(2)],
    },
    Shot {
        name: "03-marked",
        acts: &[staged(3)],
    },
    Shot {
        name: "04-history-fresh",
        acts: &[staged(4), HISTORY],
    },
    Shot {
        name: "05-touched",
        acts: &[staged(5)],
    },
    Shot {
        name: "06-off",
        acts: &[staged(6), OPEN],
    },
];

/// Into the second half of the history row's glow, and the notes' glide.
const MID_GLOW_MS: u64 = 300;

fn transpose(s: &mut Studio, by: i64, opts: &RunOptions) {
    let Some(clip) = s.roll_target().map(|t| t.clip.id.to_string()) else {
        eprintln!("no clip in the roll");
        return;
    };
    let raw = json!({"ids": [clip], "by": by});
    let raw = raw.as_object().cloned().unwrap_or_default();
    if let Err(e) = s.ai_run("note.transpose", &raw, opts, false) {
        eprintln!("note.transpose: {e}");
    }
}

/// The time of the AI's newest edit, plus `ms`.
fn after_edit(s: &mut Studio, ms: u64) {
    s.ai.clock = Some(s.song.session().ai_activity().updated_ms + ms);
    s.ai.fresh = s.ai.fresh.map(|(n, _)| (n, s.ai.clock.unwrap_or(0) - ms));
    if let Some(g) = s.ai.glide.as_mut() {
        g.at_ms = s.ai.clock.unwrap_or(0) - ms;
    }
}

/// The stages of [`AI`] that are not a press.
pub fn stage(s: &mut Studio, n: u8) {
    let ai = RunOptions::ai("AI");
    match n {
        0 => {
            s.ai.clock = Some(0);
            s.ai.connection = Some(Ok(Connection {
                url: "http://127.0.0.1:52417/mcp".into(),
                token: String::new(),
            }));
        }
        1 => {
            s.ai_act(AiAct::Level(AiPermission::Read));
            transpose(s, 2, &ai);
        }
        2 => {
            s.ai_act(AiAct::Level(AiPermission::Edit));
            s.popup = None;
            transpose(s, 2, &ai);
            after_edit(s, MID_GLOW_MS);
        }
        3 => after_edit(s, LIVE_MS),
        4 => after_edit(s, MID_GLOW_MS),
        5 => {
            s.popup = None;
            let first = s
                .roll_target()
                .and_then(|t| t.notes.first().map(|n| n.id.to_string()));
            if let Some(id) = first {
                s.run_command("note.transpose", json!({"ids": [id], "by": 1}));
            }
            after_edit(s, LIVE_MS);
        }
        6 => {
            s.ai_act(AiAct::Level(AiPermission::Off));
            s.popup = None;
        }
        _ => {}
    }
}
