//! Original positions retained by a packed folder, including across a save.
use crate::id::{FolderId, TrackId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub track: TrackId,
    pub parent: Option<FolderId>,
    pub index: usize,
}
