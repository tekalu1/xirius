//! The nonblocking contract between a device's frame and the song (UI-337).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditorBar {
    pub track: String,
    pub device: String,
    pub preset: String,
    pub presets: Vec<String>,
    pub instrument: bool,
    pub powered: bool,
    pub pinned: bool,
    pub sidechain: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EditorAction {
    Power = 0,
    PresetOpen = 1,
    PresetSave = 2,
    Pin = 3,
    Channel = 4,
    Sidechain = 5,
    PresetSelect(String) = 6,
    /// Physical screen position of the frame control; opening the host list never waits for IPC.
    PresetMenu {
        x: i32,
        y: i32,
        dpi: u32,
    } = 7,
    PresetPrevious = 8,
    PresetNext = 9,
}

impl EditorAction {
    pub fn tag(&self) -> u8 {
        match self {
            Self::Power => 0,
            Self::PresetOpen => 1,
            Self::PresetSave => 2,
            Self::Pin => 3,
            Self::Channel => 4,
            Self::Sidechain => 5,
            Self::PresetSelect(_) => 6,
            Self::PresetMenu { .. } => 7,
            Self::PresetPrevious => 8,
            Self::PresetNext => 9,
        }
    }
    pub fn from_byte(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Power,
            1 => Self::PresetOpen,
            2 => Self::PresetSave,
            3 => Self::Pin,
            4 => Self::Channel,
            5 => Self::Sidechain,
            8 => Self::PresetPrevious,
            9 => Self::PresetNext,
            _ => return None,
        })
    }
}
