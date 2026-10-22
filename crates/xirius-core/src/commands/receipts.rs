use super::*;

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        session(
            "ai.decision.get",
            "承認待ちの操作の結果を取得します",
            vec![ArgDef::text("token", "承認トークン").req()],
            Risk::Read,
            false,
        ),
        read(
            "ops.receipt",
            "操作の受領証を取得します",
            vec![ArgDef::text("id", "受領証 ID").req()],
            absent,
        ),
        read(
            "ops.get-result",
            "操作の結果をページごとに取得します",
            vec![
                ArgDef::text("id", "結果 ID").req(),
                ArgDef::int("page", "ページ（0 から）")
                    .default(json!(0))
                    .range(0.0, 1_000_000.0),
            ],
            absent,
        ),
        read(
            "ops.hello",
            "コマンドのプロトコルを照合します",
            vec![ArgDef::int("version", "プロトコルの版番号").req()],
            hello,
        ),
    ]
}

fn absent(
    _: &Project,
    _: &mut crate::session::SessionState,
    _: &crate::Registry,
    _: &Args,
) -> CmdResult<Output> {
    Err(CmdError::not_found(
        "この接続に対応する操作の結果がありません",
    ))
}

fn hello(
    _: &Project,
    _: &mut crate::session::SessionState,
    _: &crate::Registry,
    args: &Args,
) -> CmdResult<Output> {
    let version = args.opt_i64("version").unwrap_or(0);
    let hello = crate::protocol::Hello {
        command_protocol: u32::try_from(version).unwrap_or(0),
    };
    hello.check()?;
    Ok(Output::with(
        "接続しました",
        json!(crate::protocol::Hello::default()),
    ))
}
