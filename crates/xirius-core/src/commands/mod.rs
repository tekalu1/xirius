//! The commands the app ships. Each file lists its commands in `defs()`;
//! `all()` is the one table that the registry, the CLI and MCP are built from.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::command::{ArgDef, Args, CommandDef, EditFn, Handler, Output, ReadFn, Risk};
use crate::error::{CmdError, CmdResult};
use crate::id::{AnyId, ClipId, Node, NoteId};
use crate::model::{Clip, ClipBody, Note, Parent, Project};
use crate::time::{Tick, parse_grid};

mod arrange;
mod auto;
mod auto_nudge;
mod bounce;
mod browser;
mod bus_group;
mod cc;
mod channel_settings;
mod clip;
mod clip_process;
mod clipfx;
mod comp;
mod context_scope;
mod context_view;
pub mod docs;
mod edit;
mod edit_clip_join;
mod edit_groove;
mod edit_humanize;
mod edit_join;
mod edit_range;
mod edit_replace;
mod edit_scale;
mod edit_velocity;
mod freeze;
mod group;
mod import;
mod input;
mod insert;
mod instrument;
pub mod jobs;
mod key_more;
mod key_rows;
pub mod keys;
pub mod labels;
mod marker;
mod menu;
mod midi_edit;
mod misc;
mod note;
mod note_value;
pub(crate) mod overlap;
mod prefs;
pub use prefs::{ALT_TOOLS, ROLL_ALT_TOOLS};
mod draw_shape;
mod preset;
mod rack;
mod range_lane;
pub use range_lane::{RangeMove, bpm_at, moved_lane, moved_tempo, stretch};
mod receipts;
mod record;
mod ripple;
mod roll_view;
pub mod screen_ops;
mod select;
mod send_menu;
mod sidechain;
mod take_menu;
mod tempo_move;
mod time_shift;
mod track;
mod track_clipboard;
mod track_preset;
pub mod visual;

/// Older names of commands, `(old, current)`. The old name still runs the command (a script, a
/// keymap or an AI tool that has it keeps working); the palette, the docs and `commands` list
/// only the current one.
pub const ALIASES: &[(&str, &str)] = &[("send.level.set", "send.set")];

pub fn all() -> Vec<CommandDef> {
    let mut v = Vec::new();
    v.extend(track::defs());
    v.extend(send_menu::defs());
    v.extend(sidechain::defs());
    v.extend(insert::defs());
    v.extend(group::defs());
    v.extend(preset::defs());
    v.extend(clip::defs());
    v.extend(clipfx::defs());
    v.extend(note::defs());
    v.extend(note_value::defs());
    v.extend(cc::defs());
    v.extend(edit::defs());
    v.extend(edit_velocity::defs());
    v.extend(edit_join::defs());
    v.extend(edit_humanize::defs());
    v.extend(midi_edit::defs());
    v.extend(edit_clip_join::defs());
    v.extend(edit_range::defs());
    v.extend(edit_groove::defs());
    v.extend(edit_scale::defs());
    v.extend(edit_replace::defs());
    v.extend(clip_process::defs());
    v.extend(bounce::defs());
    v.extend(freeze::defs());
    v.extend(select::defs());
    v.extend(misc::defs());
    v.extend(tempo_move::defs());
    v.extend(arrange::defs());
    v.extend(time_shift::defs());
    v.extend(auto::defs());
    v.extend(auto_nudge::defs());
    v.extend(record::defs());
    v.extend(comp::defs());
    v.extend(take_menu::defs());
    v.extend(prefs::defs());
    v.extend(instrument::defs());
    v.extend(input::defs());
    v.extend(import::defs());
    v.extend(browser::defs());
    v.extend(menu::defs());
    v.extend(marker::defs());
    v.extend(context_view::defs());
    v.extend(context_scope::defs());
    v.extend(receipts::defs());
    v.extend(key_rows::defs());
    v.extend(screen_ops::defs());
    v.extend(draw_shape::defs());
    v.extend(range_lane::defs());
    v.extend(roll_view::defs());
    v.extend(channel_settings::defs());
    v.extend(track_preset::defs());
    v.extend(rack::defs());
    v.extend(jobs::defs());
    v.extend(key_more::defs());
    v
}

// ---- building definitions ------------------------------------------------------

/// A command that edits the song and goes into the history.
pub(crate) fn edit(
    name: &'static str,
    summary: &'static str,
    args: Vec<ArgDef>,
    f: EditFn,
) -> CommandDef {
    CommandDef {
        name,
        summary,
        args,
        history: true,
        risk: Risk::Write,
        handler: Handler::Edit(f),
        screen_only: false,
    }
}

/// A command that edits the song by removing something: as `edit`, but the
/// AI's use of it can be held for a person's answer.
pub(crate) fn destroy(
    name: &'static str,
    summary: &'static str,
    args: Vec<ArgDef>,
    f: EditFn,
) -> CommandDef {
    CommandDef {
        risk: Risk::Destructive,
        ..edit(name, summary, args, f)
    }
}

/// A command that only looks (or changes selection / view): never in the history.
pub(crate) fn read(
    name: &'static str,
    summary: &'static str,
    args: Vec<ArgDef>,
    f: ReadFn,
) -> CommandDef {
    CommandDef {
        name,
        summary,
        args,
        history: false,
        risk: Risk::Read,
        handler: Handler::Read(f),
        screen_only: false,
    }
}

/// A command the session runs itself (history, files, versions, AI permission).
pub(crate) fn session(
    name: &'static str,
    summary: &'static str,
    args: Vec<ArgDef>,
    risk: Risk,
    history: bool,
) -> CommandDef {
    CommandDef {
        name,
        summary,
        args,
        history,
        risk,
        handler: Handler::Session,
        screen_only: false,
    }
}

/// `--on` / `--off` flags for a toggle command.
pub(crate) fn on_off() -> [ArgDef; 2] {
    [
        ArgDef::flag("on", "オン（省略時は現在の状態を切り替え）"),
        ArgDef::flag("off", "オフ"),
    ]
}

// ---- reading arguments -------------------------------------------------------------

pub(crate) fn bad_tick(name: &str, v: &Value) -> CmdError {
    CmdError::invalid(format!("--{name} の位置 `{v}` を読み取れません"))
}

/// A position given as `bar.beat.tick` or a tick count.
pub(crate) fn opt_tick(p: &Project, args: &Args, name: &str) -> CmdResult<Option<Tick>> {
    match args.0.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n
            .as_i64()
            .map(Some)
            .ok_or_else(|| bad_tick(name, &Value::Number(n.clone()))),
        Some(Value::String(s)) => p.time_map().parse(s).map(Some),
        Some(v) => Err(bad_tick(name, v)),
    }
}

pub(crate) fn tick(p: &Project, args: &Args, name: &str) -> CmdResult<Tick> {
    opt_tick(p, args, name)?.ok_or_else(|| CmdError::invalid(format!("--{name} が必要です")))
}

pub(crate) fn grid(p: &Project, args: &Args, name: &str) -> CmdResult<Tick> {
    match args.0.get(name) {
        Some(Value::Number(n)) => parse_grid(&n.to_string(), p.ppq),
        Some(Value::String(s)) => parse_grid(s, p.ppq),
        _ => Err(CmdError::invalid(format!("--{name} が必要です"))),
    }
}

/// `--length` in ticks, or `--bars` (default 1) counted from `at`.
pub(crate) fn length_arg(p: &Project, args: &Args, at: Tick) -> CmdResult<Tick> {
    if let Some(l) = args.opt_i64("length") {
        if l < 1 {
            return Err(CmdError::range("長さは 1 tick 以上にしてください"));
        }
        return Ok(l);
    }
    let bars = args.opt_f64("bars").unwrap_or(1.0);
    if bars <= 0.0 {
        return Err(CmdError::range(
            "--bars には 0 より大きい値を指定してください",
        ));
    }
    let tm = p.time_map();
    let (bar, _, _) = tm.to_bbt(at);
    let whole = bars.floor() as u32;
    let mut end = tm.bar_start(bar + whole);
    let frac = bars - bars.floor();
    if frac > 0.0 {
        let bar_len = tm.bar_start(bar + whole + 1) - tm.bar_start(bar + whole);
        end += (bar_len as f64 * frac).round() as Tick;
    }
    let len = end - tm.bar_start(bar);
    Ok(len.max(1))
}

pub(crate) fn color_arg(args: &Args) -> CmdResult<Option<String>> {
    let c = args.str("color")?;
    if c.is_empty() || c == "none" {
        return Ok(None);
    }
    let ok = c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|x| x.is_ascii_hexdigit());
    if !ok {
        return Err(CmdError::invalid(
            "--color には #rrggbb か none を指定してください",
        ));
    }
    Ok(Some(c.to_lowercase()))
}

/// The new on/off of a toggle: the flag given, else the opposite of `current`.
pub(crate) fn toggled(args: &Args, current: bool) -> CmdResult<bool> {
    Ok(args.toggle()?.unwrap_or(!current))
}

pub(crate) fn onoff_word(on: bool) -> &'static str {
    if on {
        "オンにしました"
    } else {
        "オフにしました"
    }
}

pub(crate) fn created(what: &str, id: impl std::fmt::Display) -> Output {
    Output::with(
        format!("{what} {id} を作成しました"),
        json!({ "id": id.to_string() }),
    )
}

// ---- the track column -------------------------------------------------------------------

pub(crate) fn parent_arg(p: &Project, args: &Args, name: &str) -> CmdResult<Parent> {
    match args.opt_str(name) {
        None | Some("root") | Some("") => Ok(Parent::Root),
        Some(s) => {
            let f = s.parse().map_err(CmdError::invalid)?;
            p.folder(f)?;
            Ok(Parent::Folder(f))
        }
    }
}

/// Puts `node` into `parent` at `index` (the end when `None`).
pub(crate) fn insert_at(
    p: &mut Project,
    parent: Parent,
    index: Option<i64>,
    node: Node,
) -> CmdResult<()> {
    let len = p.children(parent).len();
    let i = match index {
        None => len,
        Some(i) if i < 0 => return Err(CmdError::range("--index には 0 以上を指定してください")),
        Some(i) => (i as usize).min(len),
    };
    p.children_mut(parent).insert(i, node);
    Ok(())
}

/// Every node of the column, top to bottom, folders before their children.
pub(crate) fn column_nodes(p: &Project) -> Vec<Node> {
    fn walk(p: &Project, kids: &[Node], out: &mut Vec<Node>) {
        for n in kids {
            out.push(*n);
            if let Node::Folder(f) = n {
                walk(p, &p.folders[f].children, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(p, &p.root, &mut out);
    out
}

// ---- clips and notes -----------------------------------------------------------------------------

/// A copy of a clip with a new id (and new note ids).
pub(crate) fn clone_clip(p: &mut Project, c: &Clip) -> Clip {
    let mut copy = c.clone();
    copy.id = ClipId(p.alloc());
    if let ClipBody::Midi { notes, .. } = &c.body {
        let fresh: Vec<Note> = notes
            .iter()
            .map(|n| Note {
                id: NoteId(p.alloc()),
                ..*n
            })
            .collect();
        copy.body = ClipBody::Midi {
            notes: Arc::new(fresh),
            transpose: c.transpose(),
        };
    }
    p.copy_fx(c.id, copy.id);
    copy
}

/// The notes a command was pointed at: note ids and clip ids (all notes of it).
pub(crate) struct Scope {
    notes: HashSet<NoteId>,
    clips: HashSet<ClipId>,
}

impl Scope {
    pub fn from_args(args: &Args) -> CmdResult<Scope> {
        let ids = args.ids::<AnyId>("ids")?;
        let mut s = Scope {
            notes: HashSet::new(),
            clips: HashSet::new(),
        };
        for id in ids {
            match id {
                AnyId::Note(n) => {
                    s.notes.insert(n);
                }
                AnyId::Clip(c) => {
                    s.clips.insert(c);
                }
                _ => {}
            }
        }
        if s.notes.is_empty() && s.clips.is_empty() {
            return Err(CmdError::invalid(
                "ノートかクリップを --ids で指定してください（選択中のものもありません）",
            ));
        }
        Ok(s)
    }

    fn hits(&self, clip: ClipId, n: &Note) -> bool {
        self.clips.contains(&clip) || self.notes.contains(&n.id)
    }

    fn missing(&self, p: &Project, found_notes: usize, found_clips: usize) -> CmdResult<()> {
        if found_notes == self.notes.len() && found_clips == self.clips.len() {
            return Ok(());
        }
        for c in &self.clips {
            if p.find_clip(*c).is_none() {
                return Err(CmdError::not_found(format!(
                    "クリップ {c} が見つかりません"
                )));
            }
        }
        let have: HashSet<NoteId> = p
            .tracks
            .values()
            .flat_map(|t| t.clips.iter().flat_map(|c| c.notes().iter().map(|n| n.id)))
            .collect();
        let n = self.notes.iter().find(|n| !have.contains(n)).copied();
        Err(CmdError::not_found(format!(
            "ノート {} が見つかりません",
            n.map_or("?".to_string(), |n| n.to_string())
        )))
    }
}

fn touches(t: &crate::model::Track, scope: &Scope) -> bool {
    t.clips.iter().any(|c| {
        scope.clips.contains(&c.id)
            || (c.is_midi()
                && !scope.notes.is_empty()
                && c.notes().iter().any(|n| scope.notes.contains(&n.id)))
    })
}

/// Runs `f` on every note in `scope`, copying only the clips it touches.
/// Returns how many notes `f` was called on.
pub(crate) fn edit_notes(
    p: &mut Project,
    scope: &Scope,
    mark_ai: bool,
    mut f: impl FnMut(&mut Note, ClipId) -> CmdResult<()>,
) -> CmdResult<usize> {
    let tids: Vec<_> = p
        .tracks
        .iter()
        .filter(|(_, t)| touches(t, scope))
        .map(|(id, _)| *id)
        .collect();
    let (mut count, mut found_notes, mut found_clips) = (0, 0, 0);
    for tid in tids {
        let t = Arc::make_mut(p.tracks.get_mut(&tid).expect("listed above"));
        for c in &mut t.clips {
            let cid = c.id;
            let whole = scope.clips.contains(&c.id);
            if whole {
                found_clips += 1;
            }
            let ClipBody::Midi { notes, .. } = &mut c.body else {
                continue;
            };
            if !whole && !notes.iter().any(|n| scope.notes.contains(&n.id)) {
                continue;
            }
            for n in Arc::make_mut(notes).iter_mut() {
                if scope.hits(cid, n) {
                    if scope.notes.contains(&n.id) {
                        found_notes += 1;
                    }
                    let before = *n;
                    f(n, cid)?;
                    if *n != before {
                        n.ai = mark_ai;
                    }
                    count += 1;
                }
            }
        }
    }
    scope.missing(p, found_notes, found_clips)?;
    Ok(count)
}

/// Removes the notes named in `scope` (clips in the scope are not removed).
pub(crate) fn remove_notes(p: &mut Project, scope: &Scope) -> CmdResult<usize> {
    let tids: Vec<_> = p
        .tracks
        .iter()
        .filter(|(_, t)| touches(t, scope))
        .map(|(id, _)| *id)
        .collect();
    let (mut count, mut found_notes, mut found_clips) = (0, 0, 0);
    for tid in tids {
        let t = Arc::make_mut(p.tracks.get_mut(&tid).expect("listed above"));
        for c in &mut t.clips {
            let whole = scope.clips.contains(&c.id);
            if whole {
                found_clips += 1;
            }
            let ClipBody::Midi { notes, .. } = &mut c.body else {
                continue;
            };
            if whole {
                found_notes += notes.iter().filter(|n| scope.notes.contains(&n.id)).count();
                count += notes.len();
                Arc::make_mut(notes).clear();
                continue;
            }
            if !notes.iter().any(|n| scope.notes.contains(&n.id)) {
                continue;
            }
            let v = Arc::make_mut(notes);
            let before = v.len();
            v.retain(|n| !scope.notes.contains(&n.id));
            found_notes += before - v.len();
            count += before - v.len();
        }
    }
    scope.missing(p, found_notes, found_clips)?;
    Ok(count)
}
