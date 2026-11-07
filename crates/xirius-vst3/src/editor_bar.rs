//! The nonblocking contract between a device's frame and the song (UI-337).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditorBar {
    pub track: String,
    pub device: String,
    pub preset: String,
    pub instrument: bool,
    pub powered: bool,
    pub pinned: bool,
    pub sidechain: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EditorAction {
    Power = 0,
    PresetOpen = 1,
    PresetSave = 2,
    Pin = 3,
    Channel = 4,
    Sidechain = 5,
}

impl EditorAction {
    pub fn from_byte(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Power,
            1 => Self::PresetOpen,
            2 => Self::PresetSave,
            3 => Self::Pin,
            4 => Self::Channel,
            5 => Self::Sidechain,
            _ => return None,
        })
    }
}
