//! The AI on the screen (AI-003, AI-022, AI-025..AI-028, AI-030, AI-033,
//! AI-034, AI-036, AI-037): what it may do, how to reach it, and how its work
//! shows.
//!
//! The app holds no model (AI-019). An AI is an MCP client that reaches the
//! song over the loopback server the screen starts ([`Connection`]); each of
//! its runs comes to the screen's thread as a [`xirius_mcp::Job`] and is
//! answered there by [`Studio::ai_job`], so the song has one owner.
//!
//! * The top bar's stars open a pop-over with the four permission levels
//!   (`ai.permission.set`), where to connect, and what the AI did last.
//! * What the AI changed keeps a thin purple edge until a person touches it
//!   (the notes' `ai` flag and the session's marks).
//! * While the AI works, its notes flicker, a band runs over them and the
//!   stars twinkle; moved notes glide to their new place; its newest history
//!   row glows once. Each motion is about a second, and only fill, opacity,
//!   position and size change. With the OS's reduced motion, nothing moves
//!   and the edges stay.

mod paint;
pub mod serve;
pub mod shots;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

use serde_json::json;
use xirius_core::AiPermission;
use xirius_core::id::{ClipId, NoteId};
use xirius_core::session::RunResult;
use xirius_core::{CmdError, ErrorCode, Risk};
use xirius_design::{AI_LOOP_MS, Easing, Layout, Motion};

use crate::popup::{Action, Cell, Look};
use crate::roll::NoteView;
use crate::song::AI_ROW;
use crate::studio::{Rect, Studio};

pub(crate) type Before = Option<(ClipId, HashMap<NoteId, NoteView>)>;

pub(crate) use paint::clip_marked;
pub use paint::{overlay, paint_bar_button, paint_cell, paint_fresh};

/// The pop-over's width.
pub const W: f32 = 300.0;
/// How long a note the AI moved takes to reach its place.
pub const GLIDE_MS: u64 = 600;
const PAD: f32 = Layout::POPUP_PAD;
const SEG: f32 = 36.0;
const SEG_H: f32 = 28.0;
const INSET: f32 = 2.0;

/// One permission level: its icon, its name and what it lets the AI do.
pub struct Level {
    pub permission: AiPermission,
    pub icon: &'static str,
    pub name: &'static str,
    pub about: &'static str,
}

/// The four levels, from none to all (AI-012, AI-022).
pub const LEVELS: [Level; 4] = [
    Level {
        permission: AiPermission::Off,
        icon: "power",
        name: "オフ",
        about: "AI からの操作をすべて拒否します",
    },
    Level {
        permission: AiPermission::Read,
        icon: "eye",
        name: "読み取りのみ",
        about: "変更は実行せず、ドライランの結果を返します",
    },
    Level {
        permission: AiPermission::Edit,
        icon: "pencil",
        name: "編集",
        about: "保存と書き出しはユーザーが行います",
    },
    Level {
        permission: AiPermission::Save,
        icon: "unlock",
        name: "編集と保存・書き出し",
        about: "AI も保存と書き出しを行えます",
    },
];

pub fn level(p: AiPermission) -> &'static Level {
    LEVELS
        .iter()
        .find(|l| l.permission == p)
        .unwrap_or(&LEVELS[2])
}

/// A press in the pop-over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Level(AiPermission),
    /// Puts how to connect on the clipboard.
    Copy,
}

/// How each part of the pop-over is painted.
#[derive(Clone, Debug, PartialEq)]
pub enum AiLook {
    /// The stars and the title.
    Head,
    /// The well the levels sit in.
    Tray,
    /// One level: its icon, lit when on.
    Seg,
    /// The level's name (`text`) and what it lets the AI do (`hint`).
    About,
    /// Where to connect, or why it cannot be reached.
    Url { down: bool },
    /// What the AI did last (`text`) and what came of it (`hint`).
    Last { refused: bool },
}

/// Where an MCP client reaches the song.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connection {
    pub url: String,
    pub token: String,
}

impl Connection {
    /// What an MCP client's settings want: the server, with its token.
    pub fn settings(&self) -> String {
        let v = json!({"mcpServers": {"xirius": {
            "type": "http",
            "url": self.url,
            "headers": {"Authorization": format!("Bearer {}", self.token)},
        }}});
        serde_json::to_string_pretty(&v).unwrap_or_default()
    }
}

/// The pop-over's line for a change the level turned into a proposal.
const READ_ONLY: &str = "「読み取りのみ」のため、ドライランの結果を返しました";
const READ_ONLY_PLAN: &str = "ドライラン: ";

/// What the AI did last, as the pop-over says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Last {
    /// "AI · 操作名".
    pub label: String,
    pub text: String,
    /// Not run: the permission, or a person's answer, stopped it.
    pub refused: bool,
}

/// What one picture of the pop-over shows.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub level: AiPermission,
    pub connection: Option<Result<String, String>>,
    pub copied: bool,
    pub last: Option<Last>,
}

/// What the top bar's stars look like now (AI-036): each star's size and
/// strength.
#[derive(Clone, Debug, PartialEq)]
pub struct BarAi {
    pub level: AiPermission,
    pub star: (f32, f32),
    pub spark: (f32, f32),
    /// The AI is at work.
    pub working: bool,
}

impl Default for BarAi {
    fn default() -> Self {
        BarAi {
            level: AiPermission::Edit,
            star: (1.0, 1.0),
            spark: (1.0, 1.0),
            working: false,
        }
    }
}

/// Notes the AI moved, where they were, and when.
#[derive(Clone, Debug)]
struct Glide {
    clip: ClipId,
    from: HashMap<NoteId, NoteView>,
    at_ms: u64,
}

/// The AI's part of the screen.
#[derive(Debug, Default)]
pub struct Ai {
    pub motion: Motion,
    /// A fixed time (tests and pictures); the wall clock when `None`.
    pub clock: Option<u64>,
    /// Where an MCP client reaches the song, or why it cannot.
    pub connection: Option<Result<Connection, String>>,
    pub last: Option<Last>,
    /// Text for the screen to put on the clipboard.
    pub copy: Option<String>,
    copied: bool,
    /// The AI's newest history step, and when it was written.
    fresh: Option<(u64, u64)>,
    glide: Option<Glide>,
}

impl Ai {
    pub fn now(&self) -> u64 {
        self.clock.unwrap_or_else(xirius_core::store::now_ms)
    }

    /// How far (0..1) into a loop of `ms` the time is, `delay` ms late.
    fn phase(&self, ms: u32, delay: u64) -> f32 {
        let ms = u64::from(ms.max(1));
        ((self.now() + ms - delay % ms) % ms) as f32 / ms as f32
    }

    /// The fresh history step, and how far (0..1) its glow is; `None` when
    /// it has finished. Under reduced motion the glow stays lit.
    pub fn fresh(&self) -> Option<(u64, f32)> {
        let (n, at) = self.fresh?;
        if self.motion.reduced {
            return Some((n, 0.0));
        }
        let t = self.now().saturating_sub(at) as f32 / AI_LOOP_MS as f32;
        (t < 1.0).then_some((n, t))
    }

    /// Where a note the AI moved is drawn now: from where it was toward
    /// `to`, or `to` itself once there.
    pub fn glide(&self, clip: ClipId, id: NoteId, to: NoteView) -> GlideAt {
        let Some(g) = self.glide.as_ref().filter(|g| g.clip == clip) else {
            return GlideAt::Still;
        };
        let Some(from) = g.from.get(&id).filter(|f| **f != to) else {
            return GlideAt::Still;
        };
        let t = self.now().saturating_sub(g.at_ms) as f32 / GLIDE_MS as f32;
        if self.motion.reduced || t >= 1.0 {
            return GlideAt::Still;
        }
        GlideAt::Moving(*from, ease(Easing::OUT, t))
    }

    fn gliding(&self) -> bool {
        self.glide
            .as_ref()
            .is_some_and(|g| !self.motion.reduced && self.now().saturating_sub(g.at_ms) < GLIDE_MS)
    }

    /// One star's size and strength as it twinkles (`delay` ms late).
    fn twinkle(&self, delay: u64) -> (f32, f32) {
        let k = ease(Easing::ORGANIC, peak(self.phase(AI_LOOP_MS, delay), 0.5));
        (1.0 + 0.25 * k, 1.0 - 0.6 * k)
    }

    /// A note's flicker while the AI works on it: its strength and its
    /// height, `delay` ms late.
    pub fn flicker(&self, delay: u64) -> (f32, f32) {
        if self.motion.reduced {
            return (1.0, 1.0);
        }
        let k = ease(Easing::ORGANIC, peak(self.phase(AI_LOOP_MS, delay), 0.4));
        (1.0 - 0.6 * k, 1.0 + 0.35 * k)
    }

    /// Where the band that runs over the AI's work is: its left edge, from
    /// -1 (out to the left) to `1 / width` (out to the right), as a part of
    /// the width it runs over.
    pub fn scan(&self, width: f32) -> f32 {
        let p = ease(Easing::ORGANIC, self.phase(AI_LOOP_MS, 0));
        -width + p * (1.0 + width)
    }
}

/// Where a note is drawn while the AI moves it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GlideAt {
    Still,
    /// From here, this far (0..1) along.
    Moving(NoteView, f32),
}

/// 0 → 1 at `top` → 0 over one loop.
fn peak(p: f32, top: f32) -> f32 {
    if p < top {
        p / top
    } else {
        (1.0 - p) / (1.0 - top)
    }
}

/// A CSS `cubic-bezier` at `x` (0..1).
pub fn ease(e: Easing, x: f32) -> f32 {
    let [x1, y1, x2, y2] = e.0;
    let x = x.clamp(0.0, 1.0);
    let bez = |a: f32, b: f32, t: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if bez(x1, x2, mid) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez(y1, y2, (lo + hi) / 2.0)
}

// ------------------------------------------------------------------- the view

fn ai(rect: Rect, look: AiLook) -> Cell {
    Cell::new(rect, Look::Ai(look))
}

/// The pop-over's cells from (0, 0) = its top-left corner.
pub fn cells(v: &View) -> Vec<Cell> {
    let cw = W - PAD * 2.0;
    let mut cells = vec![ai(Rect::new(PAD, PAD, cw, 20.0), AiLook::Head).text("AI の権限")];
    let mut y = PAD + 20.0 + 10.0;

    let tray_w = SEG * LEVELS.len() as f32 + INSET * 2.0;
    cells.push(ai(
        Rect::new(PAD, y, tray_w, SEG_H + INSET * 2.0),
        AiLook::Tray,
    ));
    for (i, l) in LEVELS.iter().enumerate() {
        cells.push(
            ai(
                Rect::new(PAD + INSET + SEG * i as f32, y + INSET, SEG, SEG_H),
                AiLook::Seg,
            )
            .icon(l.icon)
            .text(l.name)
            .tip(l.about)
            .on(l.permission == v.level)
            .act(Action::Ai(Act::Level(l.permission))),
        );
    }
    y += SEG_H + INSET * 2.0 + 10.0;
    cells.push(Cell::new(Rect::new(PAD, y, cw, 16.0), Look::Label).text("接続先"));
    y += 16.0 + 4.0;
    let (url, down) = match &v.connection {
        Some(Ok(url)) => (url.clone(), false),
        Some(Err(why)) => (why.clone(), true),
        None => ("未接続".to_string(), true),
    };
    cells.push(ai(Rect::new(PAD, y, cw, 20.0), AiLook::Url { down }).text(url));
    y += 20.0 + 8.0;
    let copy = Cell::new(Rect::new(PAD, y, cw, 28.0), Look::Button).text(if v.copied {
        "コピーしました"
    } else {
        "接続情報をコピー"
    });
    cells.push(if down {
        copy.off()
    } else {
        copy.act(Action::Ai(Act::Copy))
    });
    y += 28.0;

    if let Some(last) = &v.last {
        y += 14.0;
        cells.push(Cell::new(Rect::new(PAD, y, cw, 16.0), Look::Label).text("最後の操作"));
        y += 16.0 + 4.0;
        cells.push(
            ai(
                Rect::new(PAD, y, cw, 40.0),
                AiLook::Last {
                    refused: last.refused,
                },
            )
            .text(&last.label)
            .tip(&last.text),
        );
    }
    cells
}

// ------------------------------------------------------------------- the studio

impl Studio {
    pub(crate) fn ai_view(&self) -> View {
        View {
            level: self.song.session().ai_permission(),
            connection: self.ai.connection.as_ref().map(|c| match c {
                Ok(c) => Ok(c.url.clone()),
                Err(e) => Err(e.clone()),
            }),
            copied: self.ai.copied,
            last: self.ai.last.clone(),
        }
    }

    /// What the top bar's stars look like now.
    pub(crate) fn ai_bar(&self) -> BarAi {
        let level = self.song.session().ai_permission();
        let working = self.ai_working();
        let (star, spark) = if working && !self.ai.motion.reduced {
            (self.ai.twinkle(0), self.ai.twinkle(400))
        } else {
            ((1.0, 1.0), (1.0, 1.0))
        };
        BarAi {
            level,
            star,
            spark,
            working,
        }
    }

    /// The AI's latest edits are still running (their glow is on).
    pub(crate) fn ai_working(&self) -> bool {
        self.song.session().ai_activity().is_live(self.ai.now())
    }

    /// Is the AI marked on this id: changed by it and not touched since?
    pub(crate) fn ai_marked(&self, id: &str) -> bool {
        let a = self.song.session().ai_activity();
        a.marks.iter().any(|t| t.id == id)
    }

    /// Is the AI at work on this id now?
    pub(crate) fn ai_editing(&self, id: &str) -> bool {
        self.ai_working()
            && self
                .song
                .session()
                .ai_activity()
                .editing
                .iter()
                .any(|t| t.id == id)
    }

    /// Something of the AI's moves: the screen wants the next frame.
    pub(crate) fn ai_moving(&self) -> bool {
        !self.ai.motion.reduced
            && (self.ai_working() || self.ai.gliding() || self.ai.fresh().is_some())
    }

    /// A press in the pop-over. Returns whether the screen changed.
    pub(crate) fn ai_act(&mut self, act: Act) -> bool {
        match act {
            Act::Level(p) => {
                if let Err(e) = self
                    .song
                    // design-lint: allow(ui-blocking-io) PRJ-031 review: Song::ensure_inline refuses unreviewed Session handlers; model edits enqueue history, and reviewed status/list calls use window caches.
                    .run("ai.permission.set", json!({"level": p.as_str()}))
                {
                    self.error = Some(e);
                }
                self.ai.copied = false;
            }
            Act::Copy => {
                if let Some(Ok(c)) = &self.ai.connection {
                    self.ai.copy = Some(c.settings());
                    self.ai.copied = true;
                }
            }
        }
        self.refresh_popup();
        true
    }

    /// Runs one of the AI's commands on the song and answers it (AI-023):
    /// the session checks the permission and says why it refuses; the
    /// screen keeps what came of it and starts the motion.
    pub fn ai_job(&mut self, job: xirius_mcp::Job) {
        if self.human_interaction_active() {
            job.answer(Err(CmdError::conflict(
                // design-lint: allow(guidance) MCP busy response explains retry because an active human gesture must not be cancelled.
                "操作中です。操作が終わってから再試行してください",
            )));
            return;
        }
        if self.input_blocked() || self.macro_run.is_some() {
            job.answer(Err(CmdError::conflict(if self.opening.is_some() {
                crate::opening::BUSY
            } else if self.sound_preparing() {
                crate::prepare_card::TITLE
            } else {
                "保存中です"
            })));
            return;
        }
        if matches!(
            job.command.as_str(),
            "project.open" | "version.open" | "project.new"
        ) || (job.command == "ai.decide" && self.song.session().is_open_decision(&job.raw))
        {
            self.open_mcp(job);
            return;
        }
        if self.transport.is_windowed()
            && !job.opts.dry_run
            && self
                .song
                .session()
                .save_request(&job.command, &job.raw)
                .is_some()
        {
            self.save_for_mcp(job, None);
            return;
        }
        let deferred = self.transport.is_windowed();
        // Every connected Session command, including unknown future extensions,
        // executes on its owner. Open/save protocols above retain their guards.
        if deferred {
            self.io_command_for_mcp(job);
            return;
        }
        // design-lint: allow(ui-blocking-io) PRJ-031 review: synchronous offline adapter; ai_run refuses window mode and connected jobs use the Session worker.
        let r = self.ai_run(&job.command, &job.raw, &job.opts, job.save);
        job.answer(r);
    }

    pub(crate) fn ai_run(
        &mut self,
        command: &str,
        raw: &serde_json::Map<String, serde_json::Value>,
        opts: &xirius_core::session::RunOptions,
        save: bool,
    ) -> Result<RunResult, CmdError> {
        if self.transport.is_windowed() {
            return Err(CmdError::conflict("窓のコマンドは所有者の処理が必要です"));
        }
        if self.sound_preparing() {
            return Err(CmdError::conflict(crate::prepare_card::TITLE));
        }
        let before = self.ai_before();
        let r =
            // design-lint: allow(ui-blocking-io) PRJ-031 review: synchronous offline adapter; ai_run refuses window mode and connected jobs use the Session worker.
            xirius_mcp::persist::run_and_persist(self.song.session_mut(), command, raw, opts, save);
        self.ai_complete(command, &r, before);
        r
    }

    pub(crate) fn ai_before(&self) -> Before {
        self.roll_target().map(|t| {
            let from = t.notes.iter().map(|n| (n.id, NoteView::from(n))).collect();
            (t.clip.id, from)
        })
    }

    pub(crate) fn ai_complete(
        &mut self,
        command: &str,
        r: &Result<RunResult, CmdError>,
        before: Before,
    ) {
        let label = format!("{AI_ROW}{}", self.song.friendly(command));
        match r {
            Ok(run) if run.step.is_some() => {
                let now = self.ai.now();
                self.ai.fresh = run.step.map(|n| (n, now));
                if let Some((clip, from)) = before {
                    self.ai.glide = Some(Glide {
                        clip,
                        from,
                        at_ms: now,
                    });
                }
                self.ai.last = Some(Last {
                    label,
                    text: run.message.clone(),
                    refused: false,
                });
                self.after_edit();
            }
            Ok(run) if run.dry_run || run.decision.is_some() => {
                // The core's reason is long for one line; say it as the
                // pop-over names the level, with what would change (AI-027).
                let text = if run.forced_dry_run && !run.changes.is_empty() {
                    format!("{READ_ONLY_PLAN}{}が変更されます", run.changes.summary())
                } else if run.forced_dry_run {
                    READ_ONLY.into()
                } else {
                    run.message.clone()
                };
                self.ai.last = Some(Last {
                    label,
                    text,
                    refused: run.forced_dry_run || run.decision.is_some(),
                });
                self.refresh_popup();
            }
            Ok(run) => {
                // A look is not worth telling; a save, an undo or a redo is
                // (they have no step of their own).
                let looks = self
                    .song
                    .session()
                    .registry()
                    .get(command)
                    .is_some_and(|d| d.risk == Risk::Read);
                if !looks {
                    self.ai.last = Some(Last {
                        label,
                        text: run.message.clone(),
                        refused: false,
                    });
                    self.refresh_popup();
                }
                if run_changed(r) {
                    self.after_edit();
                }
            }
            Err(e) => {
                self.ai.last = Some(Last {
                    label,
                    text: e.message.clone(),
                    refused: e.code == ErrorCode::Denied,
                });
                self.refresh_popup();
            }
        }
    }
}

fn run_changed(r: &Result<RunResult, CmdError>) -> bool {
    r.as_ref().is_ok_and(|r| !r.changes.items.is_empty())
}
