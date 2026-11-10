//! Approved context-menu vocabulary (2026-11-08) and shape (2026-11-10). Callers choose
//! placement explicitly, never the shape; there is deliberately no fallback category for an
//! unclassified operation. The shape follows from the row count alone:
//! R1 up to 12 root rows are laid out as the caller gave them, with no category groups;
//! R2 only 13 or more rows spanning two or more categories become groups;
//! R3 a group that would hold one row is not made;
//! R4 rows that all fall in one category are not grouped.
use super::{Item, Run};

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

/// Most rows a menu may show at its root.
pub const ROOT_ROWS: usize = 12;

/// The rows kept directly at the root of a grouped menu, in the one order every menu uses.
/// Disable and enable swap in the same place.
pub const DIRECT_ORDER: [&[&str]; 9] = [
    &["ピアノロールで開く"],
    &["画面を開く"],
    &["名前を変更…"],
    &["チャンネルエディターを開く"],
    &["クリップを結合"],
    &["クリップを分割"],
    &["クリップを複製"],
    &["無効にする", "有効にする"],
    &["クリップを削除"],
];

/// Place in the direct-row table; a row the table does not know goes after all of them.
pub fn direct_rank(label: &str) -> usize {
    DIRECT_ORDER
        .iter()
        .position(|names| names.contains(&label))
        .unwrap_or(DIRECT_ORDER.len())
}

/// Whether the rows fit one face (R1), or need category groups (R2, R4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Flat,
    Grouped,
}

/// `rows` is the caller's own root row count (see [`root_rows`]); `categories` is how many
/// categories the operations span.
pub fn shape(rows: usize, categories: usize) -> Shape {
    if rows <= ROOT_ROWS || categories < 2 {
        Shape::Flat
    } else {
        Shape::Grouped
    }
}

fn is_row(item: &Item) -> bool {
    !matches!(item.run, Run::Gap | Run::Title | Run::Back)
}

/// Root row count of what the caller gave: a row, a submenu and a run of swatches each count
/// once; gaps and titles do not count.
pub fn root_rows(items: &[Item]) -> usize {
    let mut rows = 0;
    let mut swatches = false;
    for item in items.iter().filter(|i| is_row(i)) {
        if item.swatch.is_some() {
            rows += usize::from(!swatches);
            swatches = true;
        } else {
            rows += 1;
            swatches = false;
        }
    }
    rows
}

/// Leading, trailing and consecutive gaps collapse to one, also inside each submenu (R7).
pub fn collapse_gaps(items: Vec<Item>) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    for mut item in items {
        if matches!(item.run, Run::Gap) {
            if out.last().is_none_or(|l| matches!(l.run, Run::Gap)) {
                continue;
            }
        } else if let Run::Sub(children) = item.run {
            item.run = Run::Sub(collapse_gaps(children));
        }
        out.push(item);
    }
    while out.last().is_some_and(|l| matches!(l.run, Run::Gap)) {
        out.pop();
    }
    out
}

/// A submenu a row sits in, below its category.
#[derive(Clone, Debug)]
pub struct Parent {
    pub label: String,
    pub mark: Option<usize>,
    pub key: String,
}

/// Classification is required even for a frequent operation placed directly at root.
pub struct Entry {
    pub category: Category,
    pub direct: bool,
    pub item: Item,
    /// Submenus between the category and the row.
    pub parents: Vec<Parent>,
    /// Caller gaps before the row; a gap is kept between two rows of one face that differ.
    pub gaps: usize,
    /// Caller titles standing right before the row.
    pub titles: Vec<Item>,
}

impl Entry {
    pub fn new(category: Category, item: Item) -> Self {
        Self {
            category,
            direct: false,
            item,
            parents: Vec::new(),
            gaps: 0,
            titles: Vec::new(),
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

/// A face before it becomes Items: submenus are made on demand from the rows' parents.
enum Node {
    Row(Box<Entry>),
    Sub {
        parent: Parent,
        gaps: usize,
        titles: Vec<Item>,
        rows: Vec<Node>,
    },
}

impl Node {
    fn gaps(&self) -> usize {
        match self {
            Node::Row(e) => e.gaps,
            Node::Sub { gaps, .. } => *gaps,
        }
    }
}

fn place(face: &mut Vec<Node>, mut parents: Vec<Parent>, mut entry: Entry) {
    if parents.is_empty() {
        face.push(Node::Row(Box::new(entry)));
        return;
    }
    let parent = parents.remove(0);
    let at = face
        .iter()
        .position(|n| matches!(n, Node::Sub { parent: p, .. } if p.label == parent.label))
        .unwrap_or_else(|| {
            face.push(Node::Sub {
                parent,
                gaps: entry.gaps,
                titles: std::mem::take(&mut entry.titles),
                rows: Vec::new(),
            });
            face.len() - 1
        });
    if let Node::Sub { rows, .. } = &mut face[at] {
        place(rows, parents, entry);
    }
}

/// Rows of one face keep the caller's order and the gaps the caller put between them.
fn render(face: Vec<Node>) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    let mut before = None;
    for node in face {
        let gaps = node.gaps();
        if before.is_some_and(|b| b < gaps) {
            out.push(Item::gap());
        }
        before = Some(gaps);
        match node {
            Node::Row(entry) => {
                out.extend(entry.titles);
                out.push(entry.item);
            }
            Node::Sub {
                parent,
                titles,
                rows,
                ..
            } => {
                out.extend(titles);
                let mut sub = Item::sub(&parent.label, render(rows)).enabled(true);
                sub.mark = parent.mark;
                sub.key = parent.key;
                out.push(sub);
            }
        }
    }
    out
}

fn face(entries: Vec<Entry>) -> Vec<Node> {
    let mut face = Vec::new();
    for mut entry in entries {
        let parents = std::mem::take(&mut entry.parents);
        place(&mut face, parents, entry);
    }
    face
}

/// Blocks separated by one gap; an empty block leaves no gap.
fn apart(blocks: Vec<Vec<Item>>) -> Vec<Item> {
    let mut out = Vec::new();
    for block in blocks.into_iter().filter(|b| !b.is_empty()) {
        if !out.is_empty() {
            out.push(Item::gap());
        }
        out.extend(block);
    }
    out
}

/// Moves each entry exactly once. Arguments, disabled reasons, shortcuts and state stay
/// attached to the original Item; a direct entry is never copied into its category.
///
/// Root = direct rows (the table order) ‖ rows of one-row categories ‖ category groups.
/// Rows that fall in a single category are not grouped at all (R4).
pub fn build(entries: Vec<Entry>) -> Result<Vec<Item>, Error> {
    if entries.iter().any(|e| !is_row(&e.item)) {
        return Err(Error::NonOperation);
    }
    let mut spans = entries.iter().map(|e| e.category).collect::<Vec<_>>();
    spans.sort();
    spans.dedup();
    let (mut direct, rest): (Vec<_>, Vec<_>) = entries.into_iter().partition(|e| e.direct);
    // A stable sort: rows the table does not know keep the caller's order.
    direct.sort_by_key(|e| direct_rank(&e.item.label));
    let direct: Vec<_> = direct.into_iter().map(|e| e.item).collect();
    if spans.len() < 2 {
        return Ok(apart(vec![direct, render(face(rest))]));
    }
    let mut by_category: [Vec<Entry>; 12] = std::array::from_fn(|_| Vec::new());
    for entry in rest {
        by_category[entry.category as usize].push(entry);
    }
    let (mut single, mut groups) = (Vec::new(), Vec::new());
    for (category, entries) in Category::ALL.into_iter().zip(by_category) {
        let mut rows = render(face(entries));
        match rows.iter().filter(|i| is_row(i)).count() {
            0 => {}
            // A group of one row is not made; the row goes to the root (R3).
            1 => single.append(&mut rows),
            // A disabled leaf still has to be reachable, including its reason.
            _ => groups.push(Item::sub(category.label(), rows).enabled(true)),
        }
    }
    let root = apart(vec![direct, single, groups]);
    let count = root.iter().filter(|i| is_row(i)).count();
    if count > ROOT_ROWS {
        return Err(Error::TooManyRootRows(count));
    }
    Ok(root)
}
