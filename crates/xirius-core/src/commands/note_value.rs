//! Numeric menu input uses the same validation as note.set.
use super::*;
use crate::id::IdKind;

pub(super) fn defs() -> Vec<CommandDef> {
    vec![edit(
        "note.value.set",
        "選択したノートの値を設定します",
        vec![
            ArgDef::ids(IdKind::Note, "ノート"),
            ArgDef::choice(
                "field",
                &["start", "length", "pitch", "velocity"],
                "変更する値",
            )
            .req(),
            ArgDef::int("value", "値").req(),
        ],
        |p, cx, a| {
            let field = a.str("field")?;
            let value = a.i64("value")?;
            let notes: Vec<Value> = a
                .require_ids::<NoteId>("ノート")?
                .into_iter()
                .map(|id| {
                    let mut row = json!({"id": id.to_string()});
                    row[field] = json!(value);
                    row
                })
                .collect();
            super::note::set_notes(p, cx, &Args::from_json(json!({"notes": notes})))
        },
    )]
}
