//! `job.list` and `job.cancel` (EXP-047): the bounces and freezes running now, in this
//! process or another one ([`crate::running`]). They touch no song, so the MCP gateway answers
//! them without waiting for the song a bounce holds ([`answer`]).

use super::*;
use crate::error::ErrorCode;
use crate::running::{self, Running};

/// The commands [`answer`] runs.
pub const NAMES: [&str; 2] = ["job.list", "job.cancel"];

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        read(
            "job.list",
            "実行中のバウンスとフリーズを一覧します",
            vec![],
            |_, _, _, a| answer("job.list", a),
        ),
        read(
            "job.cancel",
            "実行中のバウンスとフリーズをキャンセルします（--id を省略するとすべて）。曲は元のままです",
            vec![ArgDef::text("id", "処理の ID（job.list の id）")],
            |_, _, _, a| answer("job.cancel", a),
        ),
    ]
}

fn jobs(v: &[Running]) -> Value {
    json!({ "jobs": v })
}

/// Runs `job.list` or `job.cancel`.
pub fn answer(name: &str, a: &Args) -> CmdResult<Output> {
    match name {
        "job.list" => {
            let v = running::list();
            let msg = match v.len() {
                0 => "実行中の処理はありません".to_string(),
                n => format!("実行中の処理は {n} 件です"),
            };
            Ok(Output::with(msg, jobs(&v)))
        }
        "job.cancel" => {
            let id = a.opt_str("id").filter(|s| !s.trim().is_empty());
            let v = running::cancel(id);
            match (v.len(), id) {
                (0, Some(id)) => Err(CmdError::not_found(format!(
                    "実行中の処理 {id} が見つかりません"
                ))),
                (0, None) => Err(CmdError::new(
                    ErrorCode::NothingToDo,
                    "実行中の処理が無いため、キャンセルできません",
                )),
                (n, _) => Ok(Output::with(
                    format!("処理 {n} 件をキャンセルしました"),
                    jobs(&v),
                )),
            }
        }
        n => Err(CmdError::invalid(format!(
            "{n} は処理のコマンドではありません"
        ))),
    }
}
