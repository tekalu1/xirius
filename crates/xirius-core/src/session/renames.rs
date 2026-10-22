//! A rename on the disk that a step of the history carries (BRW-013): `file.rename` points the
//! song at the new name with a `clip.relink` whose `disk` names the rename, so moving across
//! that step (undo, redo, a jump in the history, discarding what was not saved) renames the
//! file or the folder back or again. When the disk cannot follow (the name to go back to is
//! taken, the file is gone), the history does not move either: the song never names a file
//! the disk does not have because of an undo.
//!
//! A path inside the song folder is kept relative to it, so a copy of the song made by
//! "save as" renames its own files.

use super::*;

/// One rename the disk is to make, and how to say it.
struct Move {
    from: PathBuf,
    to: PathBuf,
}

/// The renames a step made, as `[{from, to}]` (empty for any other step).
fn of_step(step: &Step) -> Vec<(String, String)> {
    if step.command != "clip.relink" {
        return Vec::new();
    }
    step.args
        .get("disk")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            Some((
                m.get("from")?.as_str()?.to_string(),
                m.get("to")?.as_str()?.to_string(),
            ))
        })
        .collect()
}

/// `file` on the disk: a relative one is in the song folder.
fn on_disk(file: &str, dir: Option<&Path>) -> PathBuf {
    let p = Path::new(file);
    match dir {
        Some(d) if p.is_relative() => std::path::absolute(d)
            .unwrap_or_else(|_| d.to_path_buf())
            .join(p),
        _ => p.to_path_buf(),
    }
}

fn name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

/// Whether two paths are one entry that a rename may change the case of (Windows and the Mac
/// ignore it).
fn case_only(a: &Path, b: &Path) -> bool {
    cfg!(any(windows, target_os = "macos"))
        && a != b
        && a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

impl Session {
    /// Whether moving the history to `abs` crosses a step that renamed something on the disk.
    pub(super) fn moves_disk(&self, abs: u64) -> bool {
        !self.disk_moves(abs).is_empty()
    }

    /// The renames moving the history from where it is to `abs` makes, in order.
    fn disk_moves(&self, abs: u64) -> Vec<Move> {
        let h = &self.hist;
        let dir = self.store.as_ref().map(|s| s.dir.as_path());
        let mut out = Vec::new();
        if abs < h.cursor {
            for n in (abs.max(h.first) + 1..=h.cursor).rev() {
                for (from, to) in of_step(&h.steps[h.rel(n) - 1]).into_iter().rev() {
                    out.push(Move {
                        from: on_disk(&to, dir),
                        to: on_disk(&from, dir),
                    });
                }
            }
        } else {
            for n in h.cursor + 1..=abs.min(h.last()) {
                for (from, to) in of_step(&h.steps[h.rel(n) - 1]) {
                    out.push(Move {
                        from: on_disk(&from, dir),
                        to: on_disk(&to, dir),
                    });
                }
            }
        }
        out
    }

    /// What moving the history to `abs` renames on the disk, as `[{from, to}]` with full paths
    /// (empty when nothing): the screen follows it in its file lists.
    pub(super) fn disk_renames(&self, abs: u64) -> Vec<Value> {
        self.disk_moves(abs)
            .iter()
            .map(|m| json!({ "from": m.from.to_string_lossy(), "to": m.to.to_string_lossy() }))
            .collect()
    }

    /// Renames what the steps between here and `abs` renamed, back (undo) or again (redo). On
    /// an error the disk is left as it was and the history does not move.
    pub(super) fn move_disk(&self, abs: u64) -> CmdResult<()> {
        let back = abs < self.hist.cursor;
        let moves = self.disk_moves(abs);
        for (i, m) in moves.iter().enumerate() {
            if let Err(e) = rename(m, back) {
                for done in moves[..i].iter().rev() {
                    let _ = fs::rename(&done.to, &done.from);
                }
                return Err(e);
            }
        }
        Ok(())
    }
}

fn rename(m: &Move, back: bool) -> CmdResult<()> {
    let (old, new) = (name_of(&m.from), name_of(&m.to));
    let (cannot, not_done) = if back {
        (format!("名前を {new} に戻せません"), "取り消していません")
    } else {
        (
            format!("名前を {new} に変更できません"),
            "やり直していません",
        )
    };
    if fs::symlink_metadata(&m.from).is_err() {
        return Err(CmdError::not_found(format!(
            "{old} が見つからないため、{cannot}（{not_done}）"
        )));
    }
    if fs::symlink_metadata(&m.to).is_ok() && !case_only(&m.from, &m.to) {
        return Err(CmdError::conflict(format!(
            "{new} が既にあるため、{old} の{cannot}（{not_done}）"
        )));
    }
    fs::rename(&m.from, &m.to)
        .map_err(|e| CmdError::io(format!("{old} の{cannot}: {e}（{not_done}）")))
}
