//! The nonblocking contract between a device's frame and the song (UI-337).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditorBar {
    pub track: String,
    pub device: String,
    pub preset: String,
    pub presets: Vec<String>,
    pub menu: Vec<PresetRow>,
    /// Visible external editors, most recently touched first. These are not owners.
    pub stack: Vec<u64>,
    pub stack_base: u64,
    pub raise_ticket: u64,
    pub instrument: bool,
    pub powered: bool,
    pub pinned: bool,
    pub sidechain: bool,
    /// An ARA plugin's window (UI-380): the bar has only the name, the pin and the close box.
    pub ara: bool,
}

/// Whether the list can move backward or forward from its current preset.
pub fn preset_steps(count: usize, known: bool) -> (bool, bool) {
    (known && count > 1, count > 0 && (!known || count > 1))
}

/// A song-owned choice; the GUI host never interprets its key.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PresetRow {
    pub key: String,
    pub label: String,
    pub enabled: bool,
    pub selected: bool,
    pub footer: bool,
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
    PresetPrevious = 8,
    PresetNext = 9,
    PresetPick(String) = 10,
    EditorTouched { window: u64, time: u64 } = 11,
    EditorRaised(u64) = 12,
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
            Self::PresetPrevious => 8,
            Self::PresetNext => 9,
            Self::PresetPick(_) => 10,
            Self::EditorTouched { .. } => 11,
            Self::EditorRaised(_) => 12,
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
