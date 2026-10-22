//! Typed failures. Commands never panic or throw: they return a `CmdError`
//! whose `code` a client (UI, CLI, MCP) can branch on (AI-009).

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    UnknownCommand,
    InvalidArgs,
    NotFound,
    /// The edit would break a model rule (output loop, folder inside itself, ...).
    Conflict,
    OutOfRange,
    /// The author's permission does not allow the command.
    Denied,
    NothingToDo,
    /// A one-time answer or token ran out of time.
    Expired,
    Io,
    Corrupt,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct CmdError {
    pub code: ErrorCode,
    pub message: String,
    /// The OS failure before a path or command context was added. Kept locally for the UI.
    #[serde(skip)]
    pub io_kind: Option<std::io::ErrorKind>,
    #[serde(skip)]
    pub os_message: Option<String>,
}

impl CmdError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        CmdError {
            code,
            message: message.into(),
            io_kind: None,
            os_message: None,
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidArgs, message)
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Conflict, message)
    }
    pub fn range(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::OutOfRange, message)
    }
    pub fn io(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Io, message)
    }

    /// Adds command context without losing the original OS kind or text.
    pub fn with_context(mut self, context: impl fmt::Display) -> Self {
        self.message = format!("{context}: {}", self.message);
        self
    }
}

impl fmt::Display for CmdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CmdError {}

impl From<std::io::Error> for CmdError {
    fn from(e: std::io::Error) -> Self {
        let mut error = CmdError::io(e.to_string());
        error.io_kind = Some(e.kind());
        error.os_message = Some(e.to_string());
        error
    }
}

pub type CmdResult<T> = Result<T, CmdError>;
