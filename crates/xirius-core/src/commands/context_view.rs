//! View choices in the context menus. They do not change song history.
use super::*;
pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        read(
            "view.channel.width",
            "ミキサーのチャンネル幅を変更します",
            vec![ArgDef::choice("width", &["narrow", "wide"], "幅").req()],
            |_, s, _, a| {
                s.view.mixer_wide = a.str("width")? == "wide";
                Ok(Output::msg("チャンネル幅を変更しました"))
            },
        ),
        read(
            "view.ruler",
            "ルーラーの目盛りを変更します",
            vec![ArgDef::choice("unit", &["bars", "seconds"], "目盛り").req()],
            |_, s, _, a| {
                s.view.ruler_seconds = a.str("unit")? == "seconds";
                Ok(Output::msg("ルーラーの目盛りを変更しました"))
            },
        ),
    ]
}
