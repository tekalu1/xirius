//! Original timing and imported groove data saved with a song.
use crate::time::Tick;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timing {
    pub start: Tick,
    pub length: Tick,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Groove {
    pub name: String,
    pub period: Tick,
    pub hits: Vec<(Tick, u8)>,
}
