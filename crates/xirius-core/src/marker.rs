//! Named positions in the song, saved with the project.
use crate::id::MarkerId;
use crate::time::Tick;
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Marker {
    pub id: MarkerId,
    pub name: String,
    pub tick: Tick,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}
