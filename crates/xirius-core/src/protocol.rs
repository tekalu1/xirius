//! Command clients negotiate this version before issuing requests.
use crate::{CmdError, ErrorCode};
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Hello {
    pub command_protocol: u32,
}

impl Default for Hello {
    fn default() -> Self {
        Self {
            command_protocol: VERSION,
        }
    }
}

impl Hello {
    pub fn check(&self) -> Result<(), CmdError> {
        if self.command_protocol == VERSION {
            Ok(())
        } else {
            Err(CmdError::new(
                ErrorCode::Conflict,
                format!(
                    "コマンドのプロトコルが一致しません（接続側: {}、Xirius: {VERSION}）",
                    self.command_protocol
                ),
            ))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptState {
    Accepted,
    Running,
    Completed,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub id: String,
    pub command: String,
    pub state: ReceiptState,
    pub command_protocol: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Hello(Hello),
    Run {
        command: String,
        args: serde_json::Map<String, serde_json::Value>,
        options: crate::session::RunOptions,
        save: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Response {
    Hello(Hello),
    Result(Box<crate::session::RunResult>),
    Error(CmdError),
}
