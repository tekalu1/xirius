//! MCP clients without the extension speak command protocol v1.
use serde_json::Value;
use xirius_core::{CmdError, protocol::Hello};

pub(crate) fn check(meta: &Value) -> Result<(), CmdError> {
    let version = match meta.get("xirius").and_then(|x| x.get("command_protocol")) {
        Some(value) => value
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| CmdError::invalid("コマンドのプロトコルの版番号を読み取れません"))?,
        None => 1,
    };
    Hello {
        command_protocol: version,
    }
    .check()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn explicit_and_legacy_versions_are_both_checked() {
        assert!(check(&json!({})).is_ok());
        assert!(check(&json!({"xirius": {"command_protocol": 1}})).is_ok());
        for version in [
            json!(0),
            json!(2),
            json!(u64::MAX),
            json!(-1),
            json!("1"),
            Value::Null,
        ] {
            assert!(check(&json!({"xirius": {"command_protocol": version}})).is_err());
        }
    }
}
