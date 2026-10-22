//! The keys the roll answers to while its tab is open (EDT-054 … EDT-057).
//! The number keys that pick a tool are handled one level up, with the top
//! bar's buttons.

use serde_json::json;
use xirius_core::id::NoteId;

use super::{Ctx, Edit, Out, Roll, Then, ids_json};
use crate::tools::Mods;

impl Roll {
    /// `key` is the key's name as the window reports it ("up", "delete",
    /// "q", "escape").
    pub fn key(&mut self, ctx: &Ctx, key: &str, mods: Mods) -> Out {
        if key == "escape" {
            return self.cancel();
        }
        let chord = xirius_prefs::Chord::from_event(key, crate::input_dispatch::bits(mods));
        let binding = chord.and_then(|k| xirius_prefs::Keymap::default().lookup("roll", &k));
        let Some((_, binding)) = binding else {
            return Out::quiet();
        };
        let Ok((command, args)) = xirius_prefs::cmdline::parse(&binding, None) else {
            return Out::quiet();
        };
        self.command(ctx, &command, &args)
    }

    pub fn command(
        &mut self,
        ctx: &Ctx,
        command: &str,
        args: &serde_json::Map<String, serde_json::Value>,
    ) -> Out {
        self.sync(ctx);
        if self.drag.is_some() {
            return Out::used();
        }
        match command {
            "note.delete" => return self.delete_key(ctx),
            "select.all" => {
                self.points.clear();
                self.selected = ctx.notes().iter().map(|n| n.id).collect();
                return Out::used();
            }
            "edit.copy" | "edit.cut" => {
                if !self.points.is_empty() {
                    return self.points_to_board(ctx, command == "edit.cut");
                }
                let notes = self.selected_notes(ctx);
                self.clipboard = notes.iter().map(|n| super::NoteView::from(*n)).collect();
                // The session's clipboard keeps them too, so a copy here and a paste
                // elsewhere (or by the CLI) read one clipboard.
                let ids: Vec<NoteId> = notes.iter().map(|n| n.id).collect();
                let mut out = Out::used();
                if !ids.is_empty() {
                    out = out.push(Edit::new(
                        "edit.copy",
                        json!({ "ids": ids_json(&ids) }),
                        Then::Keep,
                    ));
                }
                if command == "edit.cut" {
                    out.edits.extend(self.delete_key(ctx).edits);
                }
                return out;
            }
            "edit.paste" if !self.clipboard.is_empty() => {
                self.points.clear();
                let first = self.clipboard.iter().map(|n| n.start).min().unwrap_or(0);
                let last = self
                    .clipboard
                    .iter()
                    .map(|n| n.start)
                    .max()
                    .unwrap_or(first);
                let tick = self.pointer.map_or(0, |(x, _)| self.view(ctx).tick(x));
                let start = ctx
                    .snapped(self.beat_px, false, tick)
                    .clamp(0, (ctx.clip_len() - 1 - (last - first)).max(0));
                let notes: Vec<_> = self
                    .clipboard
                    .iter()
                    .map(|n| super::NoteView {
                        start: (n.start - first + start).min(ctx.clip_len() - 1),
                        ..*n
                    })
                    .collect();
                return Out::edit(self.put_edit(ctx, &notes));
            }
            _ => {}
        }
        match command {
            "note.quantize" if args.get("restore") == Some(&json!(true)) => {
                self.restore_timing(ctx)
            }
            "note.quantize" if args.get("freeze") == Some(&json!(true)) => Out::edit(Edit::new(
                "note.quantize",
                json!({"ids":ids_json(&self.target_ids(ctx, true)),"freeze":true}),
                Then::Keep,
            )),
            "note.join" => {
                // Notes join inside one part.
                let ids: Vec<NoteId> = self.selected_notes(ctx).iter().map(|n| n.id).collect();
                if ids.len() < 2 {
                    return Out::used();
                }
                Out::edit(Edit::new(
                    "note.join",
                    json!({"ids":ids_json(&ids)}),
                    Then::Select,
                ))
            }
            "note.split.grid" => {
                let ids = self.target_ids(ctx, false);
                if ids.is_empty() {
                    return Out::used();
                }
                Out::edit(Edit::new(
                    "note.split.grid",
                    json!({"ids":ids_json(&ids),"grid":ctx.tools.snap.grid.to_string()}),
                    Then::Keep,
                ))
            }
            "note.transpose" => self.transpose(
                ctx,
                args.get("by")
                    .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
                    .unwrap_or(1) as i32,
            ),
            "edit.nudge.right" | "edit.nudge.left" => {
                let dir = if command == "edit.nudge.right" { 1 } else { -1 };
                let by = args
                    .get("grid")
                    .and_then(|v| match v {
                        serde_json::Value::String(s) => {
                            xirius_core::time::parse_grid(s, ctx.ppq() as u32).ok()
                        }
                        v => v.as_i64(),
                    })
                    .filter(|&t| t > 0);
                self.nudge(ctx, dir, by)
            }
            "note.quantize" => self.quantize(
                ctx,
                args.get("strength")
                    .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
                    .unwrap_or(100) as u32,
            ),
            "note.duplicate" => self.duplicate(ctx),
            "note.mute" => self.mute(ctx),
            _ => Out::quiet(),
        }
    }

    /// Delete: the selected points go, and the selected notes with them (CTX-007).
    fn delete_key(&mut self, ctx: &Ctx) -> Out {
        let points = if self.points.is_empty() {
            None
        } else {
            self.delete_points(ctx)
        };
        let ids = self.target_ids(ctx, false);
        let mut out = match points {
            Some(e) => Out::edit(e),
            None if ids.is_empty() => return Out::quiet(),
            None => Out::used(),
        };
        if !ids.is_empty() {
            self.selected.clear();
            out = out.push(Edit::new(
                "note.delete",
                json!({ "ids": ids_json(&ids) }),
                Then::Keep,
            ));
        }
        out
    }

    /// Alt+↑↓: by semitones, held inside the keyboard.
    fn transpose(&mut self, ctx: &Ctx, by: i32) -> Out {
        let mut notes = self.selected_notes(ctx);
        notes.extend(self.selected_elsewhere(ctx).into_iter().map(|(_, n)| n));
        if notes.is_empty() {
            return Out::quiet();
        }
        let lo = notes.iter().map(|n| i32::from(n.pitch)).min().unwrap_or(0);
        let hi = notes.iter().map(|n| i32::from(n.pitch)).max().unwrap_or(0);
        let by = by.clamp(-lo, 127 - hi);
        if by == 0 {
            return Out::used();
        }
        let ids: Vec<NoteId> = notes.iter().map(|n| n.id).collect();
        Out::edit(Edit::new(
            "note.transpose",
            json!({ "ids": ids_json(&ids), "by": by }),
            Then::Keep,
        ))
    }

    /// Alt+←→: one grid step; ←→ alone, `step` (a sixteenth, UI-088).
    fn nudge(&mut self, ctx: &Ctx, dir: i64, step: Option<i64>) -> Out {
        // Every selected note, of every part, stays inside its own part.
        let Some((back, ahead)) = self.room(ctx) else {
            return Out::quiet();
        };
        let by = (dir * step.unwrap_or_else(|| ctx.step(self.beat_px))).clamp(-back, ahead);
        if by == 0 {
            return Out::used();
        }
        let ids = self.target_ids(ctx, false);
        Out::edit(Edit::new(
            "note.move",
            json!({ "ids": ids_json(&ids), "by": by }),
            Then::Keep,
        ))
    }

    /// Q: the selection, or every note when nothing is selected.
    fn quantize(&mut self, ctx: &Ctx, strength: u32) -> Out {
        let ids = self.target_ids(ctx, true);
        self.quantize_ids(ctx, &ids, strength)
    }

    /// AQ (EDT-075): the selected notes, and only those, to the current grid.
    pub fn auto_quantize(&mut self, ctx: &Ctx) -> Out {
        self.sync(ctx);
        let ids = self.target_ids(ctx, false);
        self.quantize_ids(ctx, &ids, 100)
    }

    fn quantize_ids(&self, ctx: &Ctx, ids: &[NoteId], strength: u32) -> Out {
        if ids.is_empty() {
            return Out::quiet();
        }
        Out::edit(Edit::new(
            "note.quantize",
            json!({
                "ids": ids_json(ids),
                "grid": ctx.tools.snap.grid.to_string(),
                "strength": strength,
            }),
            Then::Keep,
        ))
    }

    /// D: copies placed right after the selection.
    fn duplicate(&mut self, ctx: &Ctx) -> Out {
        let notes = self.selected_notes(ctx);
        if notes.is_empty() {
            return Out::quiet();
        }
        let first = notes.iter().map(|n| n.start).min().unwrap_or(0);
        let last = notes.iter().map(|n| n.start + n.length).max().unwrap_or(0);
        let by = (last - first).max(1);
        let ids: Vec<NoteId> = notes.iter().map(|n| n.id).collect();
        if notes.iter().any(|n| n.start + by >= ctx.clip_len()) {
            return Out::used();
        }
        Out::edit(Edit::new(
            "note.duplicate",
            json!({ "ids": ids_json(&ids), "by": by }),
            Then::Select,
        ))
    }

    /// Shift+M: mute, or unmute when everything selected is already muted.
    fn mute(&mut self, ctx: &Ctx) -> Out {
        let mut notes = self.selected_notes(ctx);
        notes.extend(self.selected_elsewhere(ctx).into_iter().map(|(_, n)| n));
        if notes.is_empty() {
            return Out::quiet();
        }
        let all_muted = notes.iter().all(|n| n.muted);
        let ids: Vec<NoteId> = notes.iter().map(|n| n.id).collect();
        let args = if all_muted {
            json!({ "ids": ids_json(&ids), "off": true })
        } else {
            json!({ "ids": ids_json(&ids), "on": true })
        };
        Out::edit(Edit::new("note.mute", args, Then::Keep))
    }
}
