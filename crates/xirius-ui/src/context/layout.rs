//! Approved context-menu vocabulary (2026-11-08). Callers choose placement explicitly;
//! there is deliberately no fallback category for an unclassified operation.
use super::Item;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    Edit,
    Selection,
    Clip,
    Audio,
    Midi,
    Track,
    Device,
    Automation,
    Time,
    View,
    Export,
    File,
}

impl Category {
    pub const ALL: [Self; 12] = [
        Self::Edit,
        Self::Selection,
        Self::Clip,
        Self::Audio,
        Self::Midi,
        Self::Track,
        Self::Device,
        Self::Automation,
        Self::Time,
        Self::View,
        Self::Export,
        Self::File,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Edit => "編集",
            Self::Selection => "選択",
            Self::Clip => "クリップ",
            Self::Audio => "オーディオ",
            Self::Midi => "MIDI",
            Self::Track => "トラック",
            Self::Device => "機器",
            Self::Automation => "オートメーション",
            Self::Time => "時間",
            Self::View => "表示",
            Self::Export => "書き出し",
            Self::File => "ファイル",
        }
    }
}

/// Classification is required even for a frequent operation placed directly at root.
pub struct Entry {
    pub category: Category,
    pub direct: bool,
    pub item: Item,
}

impl Entry {
    pub fn new(category: Category, item: Item) -> Self {
        Self {
            category,
            direct: false,
            item,
        }
    }

    pub fn direct(mut self) -> Self {
        self.direct = true;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    TooManyRootRows(usize),
    NonOperation,
}

/// Moves each entry exactly once. Arguments, disabled reasons, shortcuts and state stay
/// attached to the original Item; a direct entry is never copied into its category.
pub fn build(entries: Vec<Entry>) -> Result<Vec<Item>, Error> {
    use super::Run;
    let mut root = Vec::new();
    let mut groups: [Vec<Item>; 12] = std::array::from_fn(|_| Vec::new());
    for entry in entries {
        if matches!(entry.item.run, Run::Gap | Run::Title | Run::Back) {
            return Err(Error::NonOperation);
        }
        if entry.direct {
            root.push(entry.item);
        } else {
            groups[entry.category as usize].push(entry.item);
        }
    }
    for (category, items) in Category::ALL.into_iter().zip(groups) {
        if !items.is_empty() {
            // A disabled leaf still has to be reachable, including its reason.
            root.push(Item::sub(category.label(), items).enabled(true));
        }
    }
    if root.len() > 12 {
        return Err(Error::TooManyRootRows(root.len()));
    }
    Ok(root)
}
