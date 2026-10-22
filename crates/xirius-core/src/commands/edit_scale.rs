use super::*;

pub(super) fn defs() -> Vec<CommandDef> {
    vec![read(
        "view.scale",
        "ピアノロールのスケールを設定します",
        vec![
            ArgDef::int("root", "ルートの音名（C=0）")
                .default(json!(0))
                .range(0.0, 11.0),
            ArgDef::choice("scale", &["major", "minor", "off"], "スケール").default(json!("major")),
        ],
        |_, s, _, a| {
            s.view.scale = match a.opt_str("scale").unwrap_or("major") {
                "off" => None,
                "minor" => Some((a.opt_i64("root").unwrap_or(0) as u8, 0b010110101101)),
                _ => Some((a.opt_i64("root").unwrap_or(0) as u8, 0b101010110101)),
            };
            Ok(Output::msg("スケールを設定しました"))
        },
    )]
}
