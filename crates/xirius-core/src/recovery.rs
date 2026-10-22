//! What a crash leaves and what coming back from it says (PRJ-012).
//!
//! The history is written one step at a time (`Session::push_step`), so a song
//! opened after a crash already stands at its last step. This module only
//! describes that: how many steps were not saved, and the last one.

use std::fs;
use std::path::Path;

use crate::session::LOCK;
use crate::session::Step;

/// The steps a crashed song came back with, past the last time it was saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recovery {
    /// How many operations the song holds that `project.json` does not.
    pub steps: usize,
    /// The command name of the last one (`track.add`), if there is any.
    pub last_command: Option<String>,
    /// The last one as the history panel names it.
    pub last_label: Option<String>,
    /// When the last one was made (ms since the epoch).
    pub last_at_ms: u64,
    /// The version that kept the state `project.json` held, when it differs
    /// from where the history ended (the "before the crash" version).
    pub version: Option<String>,
}

impl Recovery {
    pub(crate) fn from_history(
        steps: &[Step],
        cursor: u64,
        saved_step: u64,
        version: Option<String>,
    ) -> Recovery {
        let past: Vec<&Step> = steps
            .iter()
            .filter(|s| s.n > saved_step && s.n <= cursor)
            .collect();
        let last = past.last();
        Recovery {
            steps: past.len(),
            last_command: last.map(|s| s.command.clone()),
            last_label: last.map(|s| s.author.row_label(&s.command)),
            last_at_ms: last.map_or(0, |s| s.at_ms),
            version,
        }
    }
}

impl Recovery {
    /// "記録から N 件の操作を戻しました。最後は「…」", with the last
    /// operation named by `name` (a person's word for the command).
    pub fn line(&self, name: impl Fn(&str) -> String) -> String {
        match &self.last_command {
            Some(c) if self.steps > 0 => format!(
                "記録から {} 件の操作を戻しました。最後は「{}」です",
                self.steps,
                name(c)
            ),
            _ => "保存した状態から続けます。戻す操作はありませんでした".to_string(),
        }
    }
}

/// Is a process with this id running?
pub fn pid_alive(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const NO_WINDOW: u32 = 0x0800_0000;
        let out = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .creation_flags(NO_WINDOW)
            .output();
        out.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&format!("\"{pid}\"")))
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .is_ok_and(|o| o.status.success())
    }
}

/// Did the app that last had this song folder open go down without closing it?
/// True when the folder still holds a lock whose process is gone.
pub fn crashed(dir: &Path) -> bool {
    let Ok(text) = fs::read_to_string(dir.join(LOCK)) else {
        return false;
    };
    match text.trim().parse::<u32>() {
        Ok(pid) => !pid_alive(pid),
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Author;
    use serde_json::json;

    fn step(n: u64, command: &str) -> Step {
        Step {
            n,
            command: command.into(),
            args: json!({}),
            author: Author::Human,
            at_ms: n * 1000,
            summary: String::new(),
            group: 0,
        }
    }

    #[test]
    fn counts_the_steps_after_the_save_up_to_where_the_history_ended() {
        let steps = vec![
            step(1, "track.add"),
            step(2, "clip.add"),
            step(3, "note.add"),
        ];
        let r = Recovery::from_history(&steps, 3, 1, None);
        assert_eq!(r.steps, 2);
        assert_eq!(r.last_command.as_deref(), Some("note.add"));
        assert_eq!(r.last_at_ms, 3000);
        // An undone step is not in the song that comes back.
        let r = Recovery::from_history(&steps, 2, 1, Some("v1".into()));
        assert_eq!(r.steps, 1);
        assert_eq!(r.last_command.as_deref(), Some("clip.add"));
        assert_eq!(r.version.as_deref(), Some("v1"));
    }

    #[test]
    fn nothing_after_the_save_means_no_last_operation() {
        let steps = vec![step(1, "track.add")];
        let r = Recovery::from_history(&steps, 1, 1, None);
        assert_eq!((r.steps, r.last_command), (0, None));
    }

    #[test]
    fn the_line_names_the_count_and_the_last_operation() {
        let steps = vec![step(1, "track.add"), step(2, "clip.add")];
        let r = Recovery::from_history(&steps, 2, 0, None);
        assert_eq!(
            r.line(|c| c.to_string()),
            "記録から 2 件の操作を戻しました。最後は「clip.add」です"
        );
        let none = Recovery::from_history(&steps, 2, 2, None);
        assert!(
            none.line(|c| c.to_string())
                .contains("戻す操作はありません")
        );
    }

    #[test]
    fn this_process_is_alive() {
        assert!(pid_alive(std::process::id()));
    }
}
