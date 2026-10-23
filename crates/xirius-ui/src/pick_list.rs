//! The common part every list menu is made of (UI-320): a search box over
//! grouped rows, then a row of actions under the list that the search does
//! not filter. The search box sits outside the scrolling part, so what was
//! typed and its focus survive the list being laid out again.
//!
//! The part's behaviour (the agreed mock):
//!
//! - The search box at the head takes the keys as soon as the list opens.
//! - Names filter the rows, never mind the case; a group heading whose rows
//!   all fell out hides.
//! - ↑↓ moves the cursor, Enter picks its row; Esc clears the words first
//!   and shuts the list on a second press.
//! - Nothing matches: the rows hide and 「一致する…はありません」 shows
//!   (a UIA status, [`crate::a11y`]).
//! - The list shrinks to [`MAX_H`] and the screen's edge; only the rows
//!   under the search box scroll.

use crate::popup::{Action, Cell, Choice, Look, Popup, PopupKind, PopupLayout};
use crate::studio::{Geometry, Rect, Studio};
use xirius_design::{FontSize, Layout, Space};

/// The tallest a list menu grows: it also shrinks to the screen's edge.
pub const MAX_H: f32 = 480.0;
/// What a list with no matches says, unless it names its own.
pub const NO_MATCH: &str = "一致する項目はありません";
/// The heading a group shows: its height.
const HEAD_H: f32 = 22.0;

/// What the list's window holds at a height: a group's heading, or a row
/// with its place in the flat visible order.
enum Slot<'a> {
    Head(&'a str),
    Row(usize, &'a PickRow),
}

/// One row of a [`PickList`].
#[derive(Clone, Debug, PartialEq)]
pub struct PickRow {
    /// The name the search matches and the row shows.
    pub label: String,
    /// At the right: a key, a device's inputs and outputs, …
    pub hint: String,
    /// An icon at the left.
    pub icon: Option<&'static str>,
    /// The current row: lit, with a check at the right.
    pub on: bool,
    /// A row that cannot run now is dim and picks nothing (was `off:`).
    pub enabled: bool,
    /// Only says something (the scan's progress): no press, never picked.
    pub note: bool,
    /// What a press (or Enter on the cursor's row) runs.
    pub act: Option<Action>,
    /// Said in a balloon while the pointer is on it (a cut name in full).
    pub tip: String,
}

impl PickRow {
    pub fn new(label: impl Into<String>) -> PickRow {
        PickRow {
            label: label.into(),
            hint: String::new(),
            icon: None,
            on: false,
            enabled: true,
            note: false,
            act: None,
            tip: String::new(),
        }
    }

    pub fn act(mut self, act: Action) -> PickRow {
        self.act = Some(act);
        self
    }

    pub fn on(mut self, on: bool) -> PickRow {
        self.on = on;
        self
    }

    /// A row that cannot run now: dim, picking nothing (the `off:` rows).
    /// Its `act` stays so the row still names what it would run; the keys and
    /// the cells' `off()` keep it from running.
    pub fn disabled(mut self) -> PickRow {
        self.enabled = false;
        self
    }

    /// A line that only says something: no icon, no press, no cursor.
    pub fn note(mut self) -> PickRow {
        self.note = true;
        self
    }

    pub fn icon(mut self, icon: &'static str) -> PickRow {
        self.icon = Some(icon);
        self
    }

    pub fn hint(mut self, hint: impl Into<String>) -> PickRow {
        self.hint = hint.into();
        self
    }

    pub fn tip(mut self, tip: impl Into<String>) -> PickRow {
        self.tip = tip.into();
        self
    }

    /// The row's `to`: the value picking it runs (the tests' view of a row,
    /// and the lists' lit row).
    pub fn to(&self) -> String {
        match &self.act {
            Some(Action::Pick(to))
            | Some(Action::Input(to))
            | Some(Action::Output(to))
            | Some(Action::FolderBus(to))
            | Some(Action::RecQuantize(to))
            | Some(Action::Device(to)) => to.clone(),
            Some(Action::AutomationTarget(_, _, to)) => to.clone(),
            _ => String::new(),
        }
    }

    /// Do the typed words match the name, never mind the case?
    fn matches(&self, query: &str) -> bool {
        query.is_empty() || self.label.to_lowercase().contains(query)
    }
}

/// Rows under one small heading. An empty `heading` draws none (the group
/// still opens a gap after the one before it).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct PickGroup {
    pub heading: String,
    pub rows: Vec<PickRow>,
}

impl PickGroup {
    pub fn new(heading: impl Into<String>, rows: Vec<PickRow>) -> PickGroup {
        PickGroup {
            heading: heading.into(),
            rows,
        }
    }
}

/// The list menu: the rows, the footer under them, and the state that
/// survives the rows being built again (what was typed, the cursor, the
/// scroll).
#[derive(Clone, Debug, PartialEq)]
pub struct PickList {
    /// In the search box; kept while `refill` builds the rows again.
    pub query: String,
    /// The row the ↑↓ keys are on, counting the rows the search lets
    /// through.
    pub selected: usize,
    /// Rows scrolled past at the top.
    pub offset: usize,
    /// The rows, grouped (a heading hides when the query empties it).
    pub groups: Vec<PickGroup>,
    /// Under the list; the query never filters these (プリセットの保存 ほか).
    pub footer: Vec<PickRow>,
    /// What no matches says (「一致する…はありません」), read as a UIA status.
    pub empty: String,
    /// The row the kind calls current when `on` is not enough (kept over
    /// `refill`: the automation target being replaced).
    pub current: String,
    /// The icon every row uses unless a row names its own.
    pub icon: Option<&'static str>,
    /// What the IME is still composing after `query` (shown, not yet
    /// filtering).
    pub marked: String,
    /// Opened and not yet scrolled: the list shows the cursor's row (the
    /// current one) wherever it is (UI-338). The first key or wheel turn
    /// settles the scroll there.
    pub fresh: bool,
}

/// What a key did to a [`PickList`].
#[derive(Clone, Debug, PartialEq)]
pub enum PickKey {
    /// Esc with nothing typed: the list shuts.
    Shut,
    /// Enter on a row that runs: what it runs.
    Run(Action),
    /// The key moved the cursor or changed the words.
    Done,
    /// Not the list's key.
    Pass,
}

impl PickList {
    /// One unnamed group of `rows`.
    pub fn of(rows: Vec<PickRow>) -> PickList {
        PickList {
            query: String::new(),
            selected: 0,
            offset: 0,
            groups: vec![PickGroup::new("", rows)],
            footer: Vec::new(),
            empty: NO_MATCH.to_string(),
            current: String::new(),
            icon: None,
            marked: String::new(),
            fresh: false,
        }
    }

    /// Rows of `choices` (`to`/`label`), lit when `to == current`, each
    /// running `act(to)`: a [`Choice`] with nothing to pick only says so.
    pub fn rows_of(
        choices: Vec<Choice>,
        current: &str,
        icon: Option<&'static str>,
        act: &dyn Fn(&str) -> Action,
    ) -> Vec<PickRow> {
        choices
            .into_iter()
            .map(|c| {
                let mut row = PickRow::new(c.label).on(c.to == current);
                row.icon = icon;
                if c.to.is_empty() {
                    row.note()
                } else {
                    row.act(act(&c.to))
                }
            })
            .collect()
    }

    /// [`Self::of`] from `choices` (`to`/`label`): lit when `to == current`,
    /// each running `act(to)`. `icon` is every row's.
    pub fn of_choices(
        choices: Vec<Choice>,
        current: &str,
        icon: &'static str,
        act: impl Fn(&str) -> Action,
    ) -> PickList {
        let mut list = PickList::of(Self::rows_of(choices, current, Some(icon), &act));
        list.icon = Some(icon);
        list
    }

    /// [`Self::of_choices`] over `(heading, choices)` groups.
    pub fn grouped(
        groups: Vec<(&'static str, Vec<Choice>)>,
        icon: &'static str,
        act: impl Fn(&str) -> Action,
    ) -> PickList {
        let mut list = PickList::of(Vec::new());
        list.icon = Some(icon);
        list.groups = groups
            .into_iter()
            .filter(|(_, choices)| !choices.is_empty())
            .map(|(heading, choices)| {
                PickGroup::new(heading, Self::rows_of(choices, "", Some(icon), &act))
            })
            .collect();
        list
    }

    /// The `empty` text set on the new list.
    pub fn named(mut self, empty: impl Into<String>) -> PickList {
        self.empty = empty.into();
        self
    }

    /// The footer under the list: the query never filters it.
    pub fn footer(mut self, rows: Vec<PickRow>) -> PickList {
        self.footer = rows;
        self
    }

    /// Lowercase of the typed words (names match without case).
    fn query(&self) -> String {
        self.query.to_lowercase()
    }

    /// The rows the query lets through, flattened in show order.
    pub fn visible(&self) -> Vec<&PickRow> {
        let query = self.query();
        self.groups
            .iter()
            .flat_map(|g| &g.rows)
            .filter(|r| r.matches(&query))
            .collect()
    }

    /// The rows the ↑↓ keys walk, in order: the visible rows, then the
    /// footer's rows a press can run (a note-only line is skipped). The
    /// query never filters the footer.
    pub fn nav(&self) -> Vec<&PickRow> {
        self.visible()
            .into_iter()
            .chain(self.footer.iter().filter(|r| !r.note))
            .collect()
    }

    /// The footer's height under the list, with the gap over it.
    fn foot_h(&self) -> f32 {
        if self.footer.is_empty() {
            0.0
        } else {
            Space::X2.px() + self.footer.len() as f32 * Layout::MENU_ROW_H
        }
    }

    /// Every heading, gap and row the query lets through, stacked.
    fn content_h(&self) -> f32 {
        let query = self.query();
        let mut h = 0.0;
        let mut first = true;
        for group in &self.groups {
            let n = group.rows.iter().filter(|r| r.matches(&query)).count();
            if n == 0 {
                continue;
            }
            if !first {
                h += Space::X2.px();
            }
            first = false;
            if !group.heading.is_empty() {
                h += HEAD_H;
            }
            h += n as f32 * Layout::MENU_ROW_H;
        }
        if h == 0.0 && !self.query.is_empty() {
            // Nothing matches: the status in two rows' room.
            Layout::MENU_ROW_H * 2.0
        } else {
            h
        }
    }

    /// The height of the part that scrolls, between the search box and the
    /// footer, at the window `size`: what the rows need, at most [`MAX_H`]
    /// (and the screen's edge) less the search box and the footer. It stays
    /// the same while the list scrolls, so the window neither shrinks nor
    /// moves as headings scroll past (UI-338).
    pub fn body_h(&self, size: (f32, f32)) -> f32 {
        let chrome = Layout::POPUP_PAD * 2.0 + Layout::MENU_ROW_H + self.foot_h();
        let room = (MAX_H.min(size.1 - 12.0) - chrome).max(Layout::MENU_ROW_H);
        self.content_h().min(room)
    }

    /// The headings and rows from the visible row `offset` on that fit in
    /// `body` pixels, each with its top under the search box. A heading and
    /// its gap only go in with the group's first row under them.
    fn window(&self, offset: usize, body: f32) -> Vec<(f32, Slot<'_>)> {
        let query = self.query();
        let row_h = Layout::MENU_ROW_H;
        let fits = |y: f32| y <= body + 0.01;
        let mut out = Vec::new();
        let mut y = 0.0;
        let mut i = 0usize;
        let mut first = true;
        'groups: for group in &self.groups {
            let rows: Vec<&PickRow> = group.rows.iter().filter(|r| r.matches(&query)).collect();
            if rows.is_empty() {
                continue;
            }
            let start = i; // the group's first row in the flat visible order
            i += rows.len();
            if i <= offset {
                continue;
            }
            let gap = if first { 0.0 } else { Space::X2.px() };
            // A heading shows only while its first row did not scroll past.
            let head = !group.heading.is_empty() && start >= offset;
            let head_h = if head { HEAD_H } else { 0.0 };
            if !fits(y + gap + head_h + row_h) {
                break;
            }
            y += gap;
            first = false;
            if head {
                out.push((y, Slot::Head(&group.heading)));
                y += HEAD_H;
            }
            for (k, row) in rows.into_iter().enumerate() {
                let flat = start + k;
                if flat < offset {
                    continue;
                }
                if !fits(y + row_h) {
                    break 'groups;
                }
                out.push((y, Slot::Row(flat, row)));
                y += row_h;
            }
        }
        out
    }

    /// How many rows show from the visible row `offset` on at the window
    /// `size`.
    fn shown(&self, offset: usize, size: (f32, f32)) -> usize {
        let body = self.body_h(size);
        self.window(offset, body)
            .iter()
            .filter(|(_, s)| matches!(s, Slot::Row(..)))
            .count()
    }

    /// How many rows the drawn list shows now at the window `size` (from the
    /// scroll it is at). `cells` lays out the same window, so every row the
    /// wheel or the keys reach is drawn.
    pub fn capacity(&self, size: (f32, f32)) -> usize {
        self.shown(self.offset_at(size), size)
    }

    /// How far the wheel can move the list down at the window `size`: the
    /// first scroll at which every row left fits.
    pub fn max_offset(&self, size: (f32, f32)) -> usize {
        let n = self.visible().len();
        (0..n).find(|&o| self.shown(o, size) >= n - o).unwrap_or(0)
    }

    /// Rows built again by a refresh: what was typed, the cursor and the
    /// scroll stay (the search box is outside the list). The scroll clamp
    /// uses the tallest window; the next wheel or key turn clamps to the
    /// real one.
    pub fn refill(&mut self, other: PickList) {
        self.groups = other.groups;
        self.footer = other.footer;
        self.empty = other.empty;
        self.current = other.current;
        self.icon = other.icon;
        self.selected = self.selected.min(self.nav().len().saturating_sub(1));
        if self.nav().get(self.selected).is_some_and(|r| !r.enabled) {
            self.selected = self
                .next_live(self.selected, true)
                .or_else(|| self.next_live(self.selected, false))
                .unwrap_or(self.selected);
        }
        self.offset = self.offset.min(self.max_offset((f32::MAX, f32::MAX)));
    }

    /// The rows as the old flat `choices` (in the order they show: every
    /// group's rows, then the footer the query never filters).
    pub fn choices(&self) -> Vec<Choice> {
        self.groups
            .iter()
            .flat_map(|g| g.rows.iter())
            .chain(self.footer.iter())
            .map(|r| Choice {
                to: r.to(),
                label: r.label.clone(),
            })
            .collect()
    }

    /// The lit row's `to` (the old `p.current`), or the kept `current`.
    pub fn current(&self) -> String {
        if !self.current.is_empty() {
            return self.current.clone();
        }
        self.groups
            .iter()
            .flat_map(|g| &g.rows)
            .find(|r| r.on)
            .map(|r| r.to())
            .unwrap_or_default()
    }

    /// Esc's first job: the words go (its second shuts the list).
    fn clear_query(&mut self) {
        self.query.clear();
        self.marked.clear();
        self.retarget();
    }

    /// The words changed: the cursor goes to the first row that runs, the
    /// list back to its top.
    fn retarget(&mut self) {
        self.offset = 0;
        self.fresh = false;
        self.selected = self.next_live(0, true).unwrap_or(0);
    }

    /// The first row from `from` (in `nav` order) the cursor can sit on,
    /// walking down or up. A row that cannot run is never one (UI-338).
    fn next_live(&self, from: usize, down: bool) -> Option<usize> {
        let nav = self.nav();
        if down {
            (from..nav.len()).find(|&i| nav[i].enabled)
        } else {
            (0..=from.min(nav.len().checked_sub(1)?))
                .rev()
                .find(|&i| nav[i].enabled)
        }
    }

    /// Opens on the current row (the one with the check, else the kept
    /// `current`): the cursor starts there and the list shows it (UI-338).
    pub fn start_on_current(&mut self) {
        let current = self.current();
        let nav = self.nav();
        let at = nav.iter().position(|r| r.on && r.enabled).or_else(|| {
            (!current.is_empty())
                .then(|| nav.iter().position(|r| r.enabled && r.to() == current))
                .flatten()
        });
        self.selected = at.or_else(|| self.next_live(0, true)).unwrap_or(0);
        self.offset = 0;
        self.fresh = true;
    }

    /// The scroll the list is drawn at in the window `size`: while it is
    /// fresh, as far as the cursor's row needs to show.
    pub fn offset_at(&self, size: (f32, f32)) -> usize {
        let max = self.max_offset(size);
        if !self.fresh {
            return self.offset.min(max);
        }
        let mut o = self.offset.min(max);
        while o < max && self.selected >= o + self.shown(o, size) {
            o += 1;
        }
        o
    }

    /// The fresh scroll becomes the list's own (before a key or the wheel
    /// moves it on).
    pub fn settle(&mut self, size: (f32, f32)) {
        self.offset = self.offset_at(size);
        self.fresh = false;
    }

    /// The search box as a text field: the IME's view of it.
    pub fn field(&self) -> crate::settings::field::Field {
        let mut f = crate::settings::field::Field::new(
            crate::settings::FieldId::Search,
            self.query.clone(),
        );
        f.marked = self.marked.clone();
        f
    }

    /// Text the IME (or a paste) puts in the search box: `marked` while it
    /// is still being composed. New words move the cursor to the top.
    pub fn put_text(&mut self, range: Option<std::ops::Range<usize>>, text: &str, marked: bool) {
        let mut f = self.field();
        if marked {
            f.mark_text(range, text);
        } else {
            f.replace_text(range, text);
        }
        self.marked = f.marked;
        if f.value != self.query {
            self.query = f.value;
            self.retarget();
        }
    }

    /// The IME let go of what it was composing: it stays, typed.
    pub fn unmark(&mut self) {
        if !self.marked.is_empty() {
            let text = std::mem::take(&mut self.marked);
            self.put_text(None, &text, false);
        }
    }

    /// A key in the list at the window `size`: ↑↓ walk the rows that run,
    /// Enter picks the cursor's, Esc clears the words and then shuts,
    /// Backspace takes a letter back, and a typed letter `ch` searches.
    pub fn key(&mut self, key: &str, ch: Option<&str>, size: (f32, f32)) -> PickKey {
        // While the IME is composing, its keys are its own.
        if !self.marked.is_empty() {
            return PickKey::Done;
        }
        match key {
            "escape" if self.query.is_empty() => PickKey::Shut,
            "escape" => {
                self.clear_query();
                PickKey::Done
            }
            "enter" => self
                .nav()
                .get(self.selected)
                .filter(|r| r.enabled)
                .and_then(|r| r.act.clone())
                .map_or(PickKey::Done, PickKey::Run),
            "down" | "up" => {
                self.settle(size);
                let down = key == "down";
                let from = if down {
                    self.selected + 1
                } else {
                    match self.selected.checked_sub(1) {
                        Some(i) => i,
                        None => return PickKey::Done,
                    }
                };
                if let Some(i) = self.next_live(from, down) {
                    self.selected = i;
                }
                keep_on_screen(self, size);
                PickKey::Done
            }
            "backspace" => {
                if self.query.pop().is_some() {
                    self.retarget();
                }
                PickKey::Done
            }
            _ => match ch.filter(|c| !c.chars().any(char::is_control)) {
                Some(c) => {
                    self.put_text(None, c, false);
                    PickKey::Done
                }
                None => PickKey::Pass,
            },
        }
    }
}

/// Is `kind` a list menu? Every list menu is a [`PickList`]; a new one comes
/// here (and only here: a `PickList` is the only way a list menu is made).
pub fn is(kind: PopupKind) -> bool {
    matches!(
        kind,
        PopupKind::Insert(_)
            | PopupKind::Instrument(_)
            | PopupKind::Send(_)
            | PopupKind::Input(_)
            | PopupKind::Output(_)
            | PopupKind::AutomationTargets(..)
            | PopupKind::InstMenu(_)
            | PopupKind::FolderBus(_)
            | PopupKind::RecQuantize
            | PopupKind::Device(_)
            | PopupKind::BrowserView
            | PopupKind::AddTrack
    )
}

/// The width a list menu keeps, by kind.
pub fn width(kind: PopupKind) -> f32 {
    match kind {
        PopupKind::Insert(_) | PopupKind::Instrument(_) | PopupKind::Send(_) => 260.0,
        PopupKind::Device(_) => 280.0,
        PopupKind::BrowserView => 168.0,
        PopupKind::AddTrack => 184.0,
        _ => Layout::MENU_W,
    }
}

/// The rows the search lets through (was `fx_picker::found`).
pub fn found<'a>(p: &'a Popup) -> impl Iterator<Item = &'a PickRow> + 'a {
    p.pick.as_ref().into_iter().flat_map(|l| l.visible())
}

/// The scroll thumb at the right edge of the rows, when there are more of
/// them than show: `n` rows, `shown` of them from `offset` on in the `h`
/// pixels starting at `top`, scrolling as far as `max`.
fn thumb(
    w: f32,
    top: f32,
    h: f32,
    n: usize,
    offset: usize,
    shown: usize,
    max: usize,
) -> Option<Rect> {
    if max == 0 || shown >= n {
        return None;
    }
    let th = (h * shown as f32 / n as f32).max(16.0);
    let y = top + (h - th) * (offset.min(max) as f32 / max as f32);
    let pad = Layout::POPUP_PAD;
    Some(Rect::new(w - pad - 1.0 - 4.0, y, 4.0, th))
}

/// How wide the name of a row of `r` can be: from after its icon to before
/// its check.
fn row_text_w(r: Rect, on: bool, icon: bool) -> f32 {
    let pad = Space::X2.px();
    let check = if on { Layout::ICON_SIZE + pad } else { 0.0 };
    let icon = if icon { Layout::ICON_SIZE + pad } else { 0.0 };
    r.w - pad - icon - pad - check
}

/// The cells: the search box, then the rows the search lets through (under
/// the group headings that still have one), then the footer it never
/// filters. `size` is the window's: the list shrinks to [`MAX_H`] and to its
/// edge, and only the rows scroll.
pub fn cells(pick: &PickList, w: f32, size: (f32, f32)) -> Vec<Cell> {
    let pad = Layout::POPUP_PAD;
    let row_h = Layout::MENU_ROW_H;
    let query = pick.query();
    let typed = !query.is_empty();
    let mut out = vec![
        Cell::new(Rect::new(pad, pad, w - pad * 2.0, row_h), Look::Input)
            .text(format!("{}{}", pick.query, pick.marked))
            .icon("search")
            .on(true),
    ];
    let top = pad + row_h;
    // The part that scrolls keeps its height at any scroll (UI-338).
    let body = pick.body_h(size);

    let n = pick.visible().len();
    if n == 0 && typed {
        // Nothing matches: the rows hide and the status shows.
        out.push(
            Cell::new(
                Rect::new(pad, top, w - pad * 2.0, row_h * 2.0),
                Look::Status,
            )
            .text(pick.empty.clone()),
        );
    } else {
        let max = pick.max_offset(size);
        let offset = pick.offset_at(size);
        let mut shown = 0;
        for (y, slot) in pick.window(offset, body) {
            let (flat, row) = match slot {
                Slot::Head(heading) => {
                    out.push(
                        Cell::new(Rect::new(pad, top + y, w - pad * 2.0, HEAD_H), Look::Label)
                            .text(heading.to_string()),
                    );
                    continue;
                }
                Slot::Row(flat, row) => (flat, row),
            };
            shown += 1;
            let r = Rect::new(pad, top + y, w - pad * 2.0, row_h);
            if row.note {
                out.push(Cell::new(r, Look::Note).text(row.label.clone()));
                continue;
            }
            let icon = row.icon.or(pick.icon);
            let mut cell = Cell::new(r, Look::Row)
                .text(row.label.clone())
                .hint(row.hint.clone())
                .on(row.on)
                // The cursor never sits on a row that cannot run (UI-338).
                .cur(flat == pick.selected && row.enabled);
            if let Some(icon) = icon {
                cell = cell.icon(icon);
            }
            if let Some(act) = &row.act {
                cell = cell.act(act.clone());
            }
            if !row.enabled {
                cell = disabled(cell, row);
            }
            if !row.tip.is_empty() {
                cell = cell.tip(row.tip.clone());
            } else if crate::browser::text_width(&row.label, FontSize::Small)
                > row_text_w(r, row.on || !row.enabled, icon.is_some())
            {
                // A name the row cuts with 「…」 is said in full in the balloon.
                cell = cell.tip(row.label.clone());
            }
            out.push(cell);
        }
        out.extend(thumb(w, top, body, n, offset, shown, max).map(|r| Cell::new(r, Look::Thumb)));
    }
    let mut y = top + body;

    // The footer under the list: the query never filters it. Its live rows
    // count on from the list's rows, so the ↑↓ cursor walks down to them.
    if !pick.footer.is_empty() {
        y += Space::X2.px();
        let mut flat = pick.visible().len();
        for row in &pick.footer {
            let r = Rect::new(pad, y, w - pad * 2.0, row_h);
            y += row_h;
            let mut cell = if row.note {
                Cell::new(r, Look::Note).text(row.label.clone())
            } else {
                let i = flat;
                flat += 1;
                Cell::new(r, Look::Row)
                    .text(row.label.clone())
                    .hint(row.hint.clone())
                    .on(row.on)
                    .cur(i == pick.selected && row.enabled)
            };
            if let Some(icon) = row.icon {
                cell = cell.icon(icon);
            }
            if let Some(act) = &row.act {
                cell = cell.act(act.clone());
            }
            if !row.enabled {
                cell = disabled(cell, row);
            }
            out.push(cell.foot());
        }
    }
    out
}

/// A row that cannot run now picks nothing, and a lock at its right says so
/// (UI-338: no fainter name). A row whose alert icon already says why
/// (「オーディオデバイスなし」) has no lock.
fn disabled(cell: Cell, row: &PickRow) -> Cell {
    if row.icon == Some("alert") {
        cell.off()
    } else {
        cell.locked()
    }
}

/// The list menu's height at the window `size`: the search box, the part
/// that scrolls and the footer, padded. The same at any scroll, so the
/// window stays put while the cursor walks down (UI-338).
pub fn height(pick: &PickList, size: (f32, f32)) -> f32 {
    Layout::POPUP_PAD * 2.0 + Layout::MENU_ROW_H + pick.body_h(size) + pick.foot_h()
}

/// The list dropped from a box at `anchor` (the settings' dropdowns,
/// UI-338): `w` wide, under the box or over it when there is no room, kept
/// inside the window `size`.
pub fn dropped(pick: &PickList, anchor: Rect, w: f32, size: (f32, f32)) -> PopupLayout {
    let h = height(pick, size);
    let w = w.min(size.0 - 12.0);
    let mut y = anchor.bottom() + 4.0;
    if y + h > size.1 - 6.0 && anchor.y - 4.0 - h >= 6.0 {
        y = anchor.y - 4.0 - h;
    }
    let y = y.min(size.1 - 6.0 - h).max(6.0);
    let x = anchor.x.min(size.0 - 6.0 - w).max(6.0);
    let cells = cells(pick, w, size)
        .into_iter()
        .map(|mut c| {
            c.rect.x += x;
            c.rect.y += y;
            c
        })
        .collect();
    PopupLayout {
        rect: Rect::new(x, y, w, h),
        cells,
    }
}

/// Keeps the cursor's row inside the list's window on the keys' moves.
/// A row of the footer is always in view (it sits under the list); the
/// offset just settles at its end.
pub(crate) fn keep_on_screen(pick: &mut PickList, size: (f32, f32)) {
    let max = pick.max_offset(size);
    if pick.selected < pick.offset {
        pick.offset = pick.selected;
    } else {
        // The rows that show vary with the headings in view: scroll on
        // until the cursor's row is one of them.
        while pick.offset < max && pick.selected >= pick.offset + pick.shown(pick.offset, size) {
            pick.offset += 1;
        }
    }
    pick.offset = pick.offset.min(max);
}

impl Studio {
    /// The list menu of `kind` as it is now (UI-320). Every list menu is
    /// built here and only here: a [`PickList`] is the only way a list menu
    /// is made.
    pub(crate) fn pick_view(&mut self, kind: PopupKind) -> Option<PickList> {
        match kind {
            PopupKind::Insert(_) => Some(self.insert_pick()),
            PopupKind::Instrument(_) => Some(self.instrument_pick()),
            PopupKind::Send(t) => {
                let p = self.project();
                Some(
                    PickList::of_choices(crate::mixer::send_choices(&p, t), "", "cable", |to| {
                        Action::Pick(to.to_string())
                    })
                    .named("一致するバスはありません"),
                )
            }
            PopupKind::Input(t) | PopupKind::Output(t) => {
                let project = self.project();
                let track = project.tracks.get(&t)?;
                let input = kind == PopupKind::Input(t);
                let current = if input {
                    track.input.clone().unwrap_or_else(|| "none".into())
                } else if t == xirius_core::id::TrackId::MASTER {
                    let pair = self
                        .song
                        .io
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .view
                        .output;
                    format!("{}:{}", pair[0] + 1, pair[1] + 1)
                } else if let Some(m) = &track.midi_out {
                    format!("{}{}", crate::midi_out_row::PREFIX, m.port)
                } else {
                    match track.output {
                        None => "default".to_string(),
                        Some(xirius_core::model::Output::Master) => "master".to_string(),
                        Some(xirius_core::model::Output::Bus(id)) => id.to_string(),
                    }
                };
                let icon = if input { "input" } else { "output" };
                self.mixer_choices(kind).map(|choices| {
                    PickList::of_choices(choices, &current, icon, move |to| {
                        if input {
                            Action::Input(to.to_string())
                        } else {
                            Action::Output(to.to_string())
                        }
                    })
                    .named("一致する行き先はありません")
                })
            }
            PopupKind::AutomationTargets(t, replace) => {
                // The target being replaced, kept on the list itself so
                // `refill` carries it over every rebuild (UI-320).
                let current = self
                    .popup
                    .as_ref()
                    .and_then(|p| p.pick.as_ref())
                    .map(|p| p.current.clone())
                    .unwrap_or_default();
                let choices = self.auto_target_choices(t, replace.then_some(current.as_str()));
                let mut list = PickList::of(
                    choices
                        .into_iter()
                        .map(|c| {
                            let to = c.to.strip_prefix("off:").unwrap_or(&c.to);
                            let row = PickRow::new(c.label).on(replace && to == current);
                            let act = Action::AutomationTarget(
                                t,
                                replace.then(|| current.clone()),
                                to.to_string(),
                            );
                            if c.to.starts_with("off:") {
                                row.act(act).disabled()
                            } else {
                                row.act(act)
                            }
                        })
                        .collect(),
                );
                list.current = current;
                Some(list)
            }
            PopupKind::InstMenu(m) => {
                let (choices, current) = self.inst_menu_view(m);
                Some(crate::instrument::menu::pick(m, choices, &current))
            }
            PopupKind::FolderBus(f) => {
                let (choices, current) = self.folder_bus_view(f);
                Some(crate::folder_bus::pick(choices, &current))
            }
            PopupKind::RecQuantize => {
                let (choices, current) = self.rec_quantize_view();
                Some(PickList::of(PickList::rows_of(
                    choices,
                    &current,
                    None,
                    &|to| Action::RecQuantize(to.to_string()),
                )))
            }
            PopupKind::Device(only_inputs) => {
                let (choices, hints, current) = self.device_view(only_inputs);
                Some(if choices.is_empty() {
                    PickList::of(vec![
                        PickRow::new(crate::start::rig::NO_DEVICE)
                            .icon("alert")
                            .disabled(),
                    ])
                } else {
                    PickList::of(
                        choices
                            .into_iter()
                            .enumerate()
                            .map(|(i, c)| {
                                let on = c.to == current;
                                PickRow::new(c.label)
                                    .hint(hints.get(i).cloned().unwrap_or_default())
                                    .icon(if on { "check" } else { crate::popup::BLANK })
                                    .on(on)
                                    .act(Action::Device(c.to))
                            })
                            .collect(),
                    )
                })
            }
            PopupKind::BrowserView => {
                let (views, view) = self
                    .popup
                    .as_ref()
                    .map(|p| (p.views, p.view))
                    .unwrap_or((&crate::browser::View::ALL, crate::browser::View::Kind));
                Some(PickList::of(
                    views
                        .iter()
                        .map(|v| {
                            let row = PickRow::new(v.label()).on(*v == view).act(Action::View(*v));
                            match v.icon() {
                                Some(icon) => row.icon(icon),
                                None => row,
                            }
                        })
                        .collect(),
                ))
            }
            PopupKind::AddTrack => {
                let rows = |v: Vec<(&str, &'static str, Action)>| {
                    v.into_iter()
                        .map(|(label, icon, act)| PickRow::new(label).icon(icon).act(act))
                        .collect::<Vec<_>>()
                };
                let mut list = PickList::of(Vec::new());
                list.groups = vec![
                    PickGroup::new(
                        "",
                        rows(vec![
                            ("オーディオ", "wave", Action::AddTrack("audio")),
                            (
                                "インストゥルメント",
                                "instrument",
                                Action::AddTrack("instrument"),
                            ),
                            ("フォルダー", "folder", Action::AddTrack("folder")),
                            ("バス", "bus", Action::AddTrack("bus")),
                        ]),
                    ),
                    PickGroup::new("", rows(vec![("ファイルから…", "file", Action::FromFile)])),
                ];
                Some(list)
            }
            _ => None,
        }
    }

    /// The shots' name for the list menus' keys (was `fx_typed`).
    pub(crate) fn fx_typed(&mut self, key: &str, ch: Option<&str>) -> bool {
        let g = self.geometry(1600.0, 1000.0);
        self.pick_typed(&g, key, ch)
    }

    /// Keys a list menu's search box takes (UI-320): words filter the rows,
    /// ↑↓ moves the cursor, Enter picks its row, Esc clears the words first
    /// and shuts the list on a second press. Other popups are untouched.
    pub(crate) fn pick_typed(&mut self, g: &Geometry, key: &str, ch: Option<&str>) -> bool {
        if key == "escape" && self.mixer.fx_drag.take().is_some() {
            return true;
        }
        // With no list open, Enter on the effect the browser's keys are on
        // puts it on the chosen track (else the master), as it did before.
        if key == "enter"
            && self.browser.open
            && self.browser.tab == crate::browser::RailTab::Effects
            && self.popup.is_none()
            && !self.browser.typing
            && self.mixer.browser_fx_focus
            && let Some(name) = self.browser.selected_item
            && let Some(p) = self.effect_pick(name)
        {
            let t = match self.selected_row {
                Some(crate::tree::RowKind::Track(t)) => t,
                _ => xirius_core::id::TrackId::MASTER,
            };
            return self.add_effect(t, self.project().tracks[&t].inserts.len(), &p);
        }
        let Some(pick) = self.popup.as_mut().and_then(|p| p.pick.as_mut()) else {
            return false;
        };
        match pick.key(key, ch, g.size) {
            PickKey::Shut => {
                self.popup = None;
                self.close_wheel_group();
            }
            PickKey::Run(act) => {
                self.popup_action(g, act);
            }
            PickKey::Done => {}
            PickKey::Pass => return false,
        }
        true
    }

    /// The list whose search box has the keyboard: a list menu's, or the
    /// settings' open list (UI-338).
    pub(crate) fn pick_typing(&self) -> Option<&PickList> {
        self.popup
            .as_ref()
            .and_then(|p| p.pick.as_ref())
            .or_else(|| self.settings.as_ref()?.menu.as_ref()?.pick.as_ref())
    }

    fn pick_typing_mut(&mut self) -> Option<&mut PickList> {
        if let Some(p) = self.popup.as_mut().and_then(|p| p.pick.as_mut()) {
            return Some(p);
        }
        self.settings.as_mut()?.menu.as_mut()?.pick.as_mut()
    }

    /// Text the IME puts in the list's search box (UI-338): what it
    /// committed, or (`marked`) what it is still composing.
    pub(crate) fn pick_text(
        &mut self,
        range: Option<std::ops::Range<usize>>,
        text: &str,
        marked: bool,
    ) {
        if let Some(pick) = self.pick_typing_mut() {
            pick.put_text(range, text, marked);
        }
        self.pick_relayout();
    }

    /// A settings' list is laid out at its box: its cells follow the words.
    fn pick_relayout(&mut self) {
        if let Some(m) = self.settings.as_mut().and_then(|u| u.menu.as_mut()) {
            m.hover = None;
            m.relayout();
        }
    }

    /// The IME let go of what it was composing in the list's search box.
    pub(crate) fn pick_unmark(&mut self) {
        if let Some(pick) = self.pick_typing_mut() {
            pick.unmark();
        }
        self.pick_relayout();
    }

    /// Ctrl+V in the list's search box: the clipboard's text goes in, on
    /// one line (UI-338). False with no list open.
    pub(crate) fn pick_paste(&mut self, text: &str) -> bool {
        let line: String = text.lines().collect::<Vec<_>>().join(" ");
        match self.pick_typing_mut() {
            Some(pick) => {
                pick.put_text(None, line.trim(), false);
                self.pick_relayout();
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::Studio;

    /// The sample song's mixer.
    fn studio() -> (Studio, Geometry) {
        let mut s = Studio::sample();
        s.drawer_open = true;
        s.tab = crate::studio::DrawerTab::Mixer;
        let g = s.geometry(1600.0, 1000.0);
        (s, g)
    }

    /// How many of the part's jobs a list menu shows, opened on `s`:
    /// the search box, the ↑↓ cursor and Enter, Esc's two, the empty state.
    pub fn counted(s: &mut Studio, g: &Geometry) -> usize {
        let mut n = 0;
        let Some(pick) = s.popup.as_ref().and_then(|p| p.pick.clone()) else {
            return 0;
        };
        let kind = s.popup.as_ref().unwrap().kind;
        // 1: the search box, over the list and lit.
        let l = s.popup.as_ref().unwrap().layout(g.size);
        if l.cells
            .first()
            .is_some_and(|c| c.look == Look::Input && c.icon == Some("search") && c.on)
        {
            n += 1;
        }
        // 2: words hide what they miss, and the empty state is a status.
        for c in "zzz".chars() {
            s.typed(g, &c.to_string(), Some(&c.to_string()));
        }
        let l2 = s.popup.as_ref().unwrap().layout(g.size);
        let empty = s
            .popup
            .as_ref()
            .and_then(|p| p.pick.as_ref())
            .is_some_and(|p| p.visible().is_empty());
        let status = l2
            .cells
            .iter()
            .any(|c| c.look == Look::Status && c.text.contains("はありません"));
        if status && empty && !pick.visible().is_empty() {
            n += 1;
        }
        // 3: Esc clears the words, then shuts the list.
        s.typed(g, "escape", None);
        let open = s.popup.is_some()
            && s.popup
                .as_ref()
                .and_then(|p| p.pick.as_ref())
                .is_some_and(|p| p.query.is_empty());
        s.typed(g, "escape", None);
        if open && s.popup.is_none() {
            n += 1;
        }
        // 4: ↓ moves the cursor (as far as the rows allow) and Enter picks.
        // The list opens on its current row (UI-338).
        s.open_popup(kind, Rect::new(100.0, 100.0, 100.0, 28.0));
        if let Some(pick) = s.popup.as_ref().and_then(|p| p.pick.as_ref()) {
            let from = pick.selected;
            let want = pick.next_live(from + 1, true).unwrap_or(from);
            let took_down = s.typed(g, "down", None);
            let moved = s
                .popup
                .as_ref()
                .and_then(|p| p.pick.as_ref())
                .is_some_and(|p| p.selected == want);
            let took_enter = s.typed(g, "enter", None);
            if took_down && moved && took_enter {
                n += 1;
            }
        }
        n
    }

    /// A list with more rows than the window keeps its search box put and
    /// scrolls only under it (UI-320).
    #[test]
    fn a_long_list_scrolls_under_the_search_box_and_shrinks_to_the_edge() {
        let rows: Vec<PickRow> = (0..40)
            .map(|i| PickRow::new(format!("row {i}")).act(Action::Pick(i.to_string())))
            .collect();
        let mut pick = PickList::of(rows);
        pick.selected = 20;
        keep_on_screen(&mut pick, (1600.0, 1000.0));
        let tall = cells(&pick, 260.0, (1600.0, 1000.0));
        // The search box is the first cell; every row is under it.
        assert_eq!(tall[0].look, Look::Input);
        let rows: Vec<f32> = tall
            .iter()
            .skip(1)
            .filter(|c| c.look == Look::Row)
            .map(|c| c.rect.y)
            .collect();
        assert!(rows.iter().all(|y| *y >= tall[0].rect.bottom()));
        assert!(rows.len() < 40 && rows.len() > 10);
        // The list never grows past MAX_H, and it shrinks to a small window.
        let total = tall.iter().map(|c| c.rect.bottom()).fold(0.0f32, f32::max) + Layout::POPUP_PAD;
        assert!(total <= MAX_H, "{total}");
        let small = cells(&pick, 260.0, (1600.0, 200.0));
        let small_rows = small.iter().filter(|c| c.look == Look::Row).count();
        assert!(small_rows < rows.len());
        assert!(small.iter().any(|c| c.look == Look::Thumb));
    }

    /// A group heading hides when the query leaves it empty; the footer is
    /// never filtered (UI-320).
    #[test]
    fn the_query_hides_empty_headings_and_never_filters_the_footer() {
        let pick = PickList {
            groups: vec![
                PickGroup::new(
                    "エフェクト",
                    vec![
                        PickRow::new("EQ").act(Action::Pick("a".into())),
                        PickRow::new("Delay").act(Action::Pick("b".into())),
                    ],
                ),
                PickGroup::new(
                    "プラグイン",
                    vec![PickRow::new("Reverb X").act(Action::Pick("c".into()))],
                ),
            ],
            footer: vec![PickRow::new("プラグインを再スキャン").act(Action::Pick("rescan".into()))],
            ..PickList::of(Vec::new())
        };
        let mut q = pick.clone();
        q.query = "eq".into();
        let l = cells(&q, 260.0, (1600.0, 1000.0));
        let texts: Vec<&str> = l.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"エフェクト") && !texts.contains(&"プラグイン"));
        // The footer stays.
        assert!(texts.contains(&"プラグインを再スキャン"));
        // Nothing matches: the status shows, the rows hide (the footer stays).
        q.query = "zzz".into();
        let l = cells(&q, 260.0, (1600.0, 1000.0));
        assert!(l.iter().any(|c| c.look == Look::Status));
        assert!(q.visible().is_empty());
        let rows: Vec<&str> = l
            .iter()
            .filter(|c| c.look == Look::Row && c.live())
            .map(|c| c.text.as_str())
            .collect();
        assert_eq!(rows, ["プラグインを再スキャン"]);
        assert_eq!(l[1].text, NO_MATCH);
    }

    /// The last row of a list taller than the window is drawn: `capacity`
    /// counts the rows exactly as `cells` does (the headings and the gaps
    /// between the groups, the footer), so the wheel and the cursor always
    /// reach it (UI-320).
    #[test]
    fn the_last_row_of_a_full_list_is_drawn() {
        let group = |name: &str, first: usize, n: usize| {
            PickGroup::new(
                name,
                (0..n)
                    .map(|i| {
                        PickRow::new(format!("row {}", first + i))
                            .act(Action::Pick(format!("{}", first + i)))
                    })
                    .collect(),
            )
        };
        for size in [(1600.0, 1000.0), (1600.0, 220.0)] {
            let mut pick = PickList::of(Vec::new());
            pick.groups = vec![group("最近", 0, 7), group("", 7, 8)];
            pick.footer = vec![
                PickRow::new("スキャンしています…").note(),
                PickRow::new("プラグインを再スキャン").act(Action::Pick("r".into())),
            ];
            pick.offset = pick.max_offset(size);
            let drawn: Vec<String> = cells(&pick, 260.0, size)
                .iter()
                .filter(|c| c.look == Look::Row && !c.foot)
                .map(|c| c.text.clone())
                .collect();
            assert_eq!(drawn.last().map(String::as_str), Some("row 14"), "{size:?}");
            // The cursor on the last row keeps it drawn (or, on the
            // footer, the list stays at its end).
            pick.selected = pick.nav().len() - 1;
            keep_on_screen(&mut pick, size);
            let l = cells(&pick, 260.0, size);
            let lit = l.iter().find(|c| c.cursor).expect("a lit row");
            assert!(
                lit.text == "row 14" || lit.text == "プラグインを再スキャン",
                "{size:?} {}",
                lit.text
            );
        }
    }

    /// The footer's rows are part of the cursor's walk (UI-320): ↓ steps
    /// onto them (a note-only line is skipped), Enter runs the row, and the
    /// lit row shows in the cells.
    #[test]
    fn the_cursor_walks_down_to_the_footer_and_enter_runs_its_row() {
        let mut pick = PickList::of(Vec::new());
        pick.groups = vec![PickGroup::new(
            "",
            vec![PickRow::new("EQ").act(Action::Pick("eq".into()))],
        )];
        pick.footer = vec![
            PickRow::new("スキャンしています…").note(),
            PickRow::new("スキャンを中止").act(Action::Pick("stop".into())),
        ];
        assert_eq!(pick.nav().len(), 2, "the note is not walked");
        pick.selected = 1;
        let l = cells(&pick, 260.0, (1600.0, 1000.0));
        let feet: Vec<&Cell> = l.iter().filter(|c| c.foot).collect();
        assert_eq!(feet.len(), 2);
        assert!(!feet[0].cursor);
        assert!(feet[1].cursor, "the cursor's footer row is lit");

        // On a real list (the insert menu's 再スキャン row), Enter runs it.
        let (mut s, g) = studio();
        let p = s.project();
        let keys = p.tracks.values().find(|t| t.name == "Keys").unwrap().id;
        drop(p);
        s.open_popup(
            PopupKind::Insert(keys),
            Rect::new(100.0, 100.0, 100.0, 28.0),
        );
        let nav = s
            .popup
            .as_ref()
            .and_then(|p| p.pick.as_ref())
            .map(|p| p.nav().len())
            .unwrap_or(0);
        assert!(nav > 1, "the insert list has a footer row");
        for _ in 0..nav {
            s.typed(&g, "down", None);
        }
        let pick = s.popup.as_ref().unwrap().pick.as_ref().unwrap();
        assert_eq!(pick.selected, nav - 1);
        assert_eq!(pick.nav()[nav - 1].label, "プラグインを再スキャン");
        let before = s.mixer.scan.running();
        assert!(s.typed(&g, "enter", None));
        // The footer row ran: the scan started or its error is shown.
        assert!(s.mixer.scan.running() != before || s.error.is_some());
    }

    /// Every list menu is opened on the same part (UI-320): the search box,
    /// ↑↓ and Enter, Esc's two, and the empty state are counted — all four.
    /// A kind `pick_list::is` names goes in the list here.
    #[test]
    fn every_list_menu_has_the_parts_four_jobs() {
        let (mut s, g) = studio();
        let p = s.project();
        let keys = p.tracks.values().find(|t| t.name == "Keys").unwrap().id;
        let vocal = p.tracks.values().find(|t| t.name == "Vocal").unwrap().id;
        let folder = *p.folders.keys().next().expect("a folder in the song");
        drop(p);
        let anchor = Rect::new(100.0, 100.0, 100.0, 28.0);
        // Every kind `pick_list::is` names but the instrument's menu (its
        // `Menu` argument is given under the loop).
        for kind in [
            PopupKind::Insert(keys),
            PopupKind::Instrument(vocal),
            PopupKind::Input(vocal),
            PopupKind::Output(vocal),
            PopupKind::Send(vocal),
            PopupKind::FolderBus(folder),
            PopupKind::RecQuantize,
            PopupKind::Device(false),
            PopupKind::BrowserView,
            PopupKind::AddTrack,
            PopupKind::AutomationTargets(keys, false),
        ] {
            assert!(crate::pick_list::is(kind), "{kind:?} is a list menu");
            s.open_popup(kind, anchor);
            let n = counted(&mut s, &g);
            assert_eq!(n, 4, "{kind:?}");
            s.popup = None;
        }
        // The instrument's menu (presets, the row's, the parameters).
        s.open_inst_menu(
            crate::instrument::menu::Menu::Preset(keys, s.project().tracks[&keys].inserts[0].id),
            Rect::new(100.0, 100.0, 100.0, 28.0),
        );
        assert_eq!(counted(&mut s, &g), 4, "preset menu");
        s.popup = None;
    }

    /// `n` rows that run, `row 0`…, the `on` one checked.
    fn long(n: usize, on: Option<usize>) -> PickList {
        PickList::of(
            (0..n)
                .map(|i| {
                    PickRow::new(format!("row {i}"))
                        .act(Action::Pick(i.to_string()))
                        .on(Some(i) == on)
                })
                .collect(),
        )
    }

    /// The lit row's name in `cells`.
    fn lit(cells: &[Cell]) -> Option<String> {
        cells.iter().find(|c| c.cursor).map(|c| c.text.clone())
    }

    /// The IME's text reaches the search box (UI-338): what it composes
    /// shows in the box without filtering yet, what it commits searches,
    /// what it lets go of stays typed, and Ctrl+V's text goes in on one line.
    #[test]
    fn the_ime_and_a_paste_type_in_the_search_box() {
        let (mut s, g) = studio();
        let anchor = Rect::new(100.0, 100.0, 100.0, 28.0);
        s.open_popup(PopupKind::RecQuantize, anchor);
        let all = s.pick_typing().unwrap().visible().len();
        let box_text = |s: &Studio| {
            s.popup.as_ref().unwrap().layout(g.size).cells[0]
                .text
                .clone()
        };

        s.pick_text(None, "1/1", true);
        let pick = s.pick_typing().unwrap();
        assert_eq!((pick.query.as_str(), pick.marked.as_str()), ("", "1/1"));
        assert_eq!(pick.visible().len(), all, "composing does not filter yet");
        assert_eq!(box_text(&s), "1/1");

        s.pick_text(None, "1/16", false);
        let pick = s.pick_typing().unwrap();
        assert_eq!((pick.query.as_str(), pick.marked.as_str()), ("1/16", ""));
        assert!(pick.visible().iter().all(|r| r.label.contains("1/16")));
        assert!(pick.visible().len() < all);
        // The cursor is on a row the words let through.
        assert_eq!(
            lit(&s.popup.as_ref().unwrap().layout(g.size).cells).as_deref(),
            Some(pick.visible()[0].label.as_str())
        );

        s.pick_text(None, "T", true);
        s.pick_unmark();
        let pick = s.pick_typing().unwrap();
        assert_eq!((pick.query.as_str(), pick.marked.as_str()), ("1/16T", ""));

        // Esc clears the words; a paste puts its text in, on one line.
        s.typed(&g, "escape", None);
        assert!(s.pick_paste("1/8\r\n"));
        assert_eq!(s.pick_typing().unwrap().query, "1/8");
        assert_eq!(box_text(&s), "1/8");
        // While the IME composes, its keys are its own (no row picked).
        s.pick_text(None, "x", true);
        assert!(s.typed(&g, "enter", None));
        assert!(s.popup.is_some());
        s.popup = None;
        assert!(!s.pick_paste("1/4"), "no list open");
    }

    /// The list stays put while the cursor walks down (UI-338): twenty ↓
    /// keep its place and size, opened under the box and over it, and the
    /// cursor reaches the last row.
    #[test]
    fn twenty_downs_keep_the_list_put_and_reach_the_last_row() {
        let (mut s, g) = studio();
        let n = 30;
        for (anchor, over) in [
            (Rect::new(100.0, 100.0, 100.0, 28.0), false),
            (Rect::new(100.0, g.size.1 - 60.0, 100.0, 28.0), true),
        ] {
            s.open_popup(PopupKind::RecQuantize, anchor);
            let p = s.popup.as_mut().unwrap();
            p.pick = Some(long(n, None));
            let first = p.layout(g.size).rect;
            assert_eq!(first.bottom() <= anchor.y, over, "{first:?}");
            let mut seen = Vec::new();
            for i in 0..n - 1 {
                let p = s.popup.as_mut().unwrap();
                p.pick.as_mut().unwrap().key("down", None, g.size);
                let l = p.layout(g.size);
                if i < 20 {
                    assert_eq!(l.rect, first, "after {} downs ({over})", i + 1);
                }
                seen.push(lit(&l.cells));
            }
            assert_eq!(
                seen.last().cloned().flatten().as_deref(),
                Some("row 29"),
                "{over}"
            );
            assert_eq!(s.popup.as_ref().unwrap().layout(g.size).rect, first);
            s.popup = None;
        }
        // The settings' lists drop from their box the same way.
        let size = (1200.0, 800.0);
        for (anchor, over) in [
            (Rect::new(300.0, 120.0, 200.0, 28.0), false),
            (Rect::new(300.0, 720.0, 200.0, 28.0), true),
        ] {
            let mut pick = long(n, None);
            let first = dropped(&pick, anchor, 240.0, size).rect;
            assert_eq!(first.bottom() <= anchor.y, over);
            for i in 0..n - 1 {
                pick.key("down", None, size);
                let l = dropped(&pick, anchor, 240.0, size);
                if i < 20 {
                    assert_eq!(l.rect, first);
                }
                assert!(
                    l.cells
                        .iter()
                        .all(|c| c.rect.bottom() <= first.bottom() + 0.5)
                );
            }
            assert_eq!(
                lit(&dropped(&pick, anchor, 240.0, size).cells).as_deref(),
                Some("row 29")
            );
        }
    }

    /// A list opens on its current row (UI-338): the cursor is on the
    /// checked row, scrolled into view, and the first ↓ walks on from it.
    #[test]
    fn a_list_reopens_on_its_current_row() {
        let size = (1600.0, 600.0);
        let mut pick = long(40, Some(33));
        pick.start_on_current();
        assert_eq!(pick.selected, 33);
        assert_eq!(lit(&cells(&pick, 260.0, size)).as_deref(), Some("row 33"));
        pick.key("down", None, size);
        assert_eq!(lit(&cells(&pick, 260.0, size)).as_deref(), Some("row 34"));
        // A checked row that cannot run is not one to start on.
        let mut pick = long(5, Some(2));
        pick.groups[0].rows[2].enabled = false;
        pick.start_on_current();
        assert_eq!(pick.selected, 0);

        // A real list: pick a row, open it again, the cursor is on it.
        let (mut s, g) = studio();
        let anchor = Rect::new(100.0, 100.0, 100.0, 28.0);
        s.open_popup(PopupKind::RecQuantize, anchor);
        let from = s.pick_typing().unwrap().selected;
        s.typed(&g, "down", None);
        s.typed(&g, "down", None);
        let pick = s.pick_typing().unwrap();
        assert_ne!(pick.selected, from);
        let want = pick.nav()[pick.selected].label.clone();
        assert!(s.typed(&g, "enter", None));
        s.popup = None;
        s.open_popup(PopupKind::RecQuantize, anchor);
        let pick = s.pick_typing().unwrap();
        assert!(pick.nav()[pick.selected].on, "the current row");
        assert_eq!(pick.nav()[pick.selected].label, want);
        assert_eq!(
            lit(&s.popup.as_ref().unwrap().layout(g.size).cells),
            Some(want)
        );
    }

    /// The scroll thumb shows on the floating paper and the rows' faces
    /// under it, 3:1 in both themes (UI-338).
    #[test]
    fn the_scroll_thumb_is_three_to_one_on_its_paper() {
        use xirius_design::{Surface, Theme, contrast_ratio};
        for theme in Theme::ALL {
            let thumb = crate::paint::list_thumb(theme);
            for face in [Surface::Paper, Surface::Hover, Surface::Sel] {
                let r = contrast_ratio(thumb, face.color(theme));
                assert!(r >= 3.0, "{theme:?} {face:?}: {r:.2}");
            }
        }
        // The list's thumb is drawn in that colour.
        let l = cells(&long(40, None), 260.0, (1600.0, 400.0));
        assert!(l.iter().any(|c| c.look == Look::Thumb));
    }

    /// A row that cannot run is told by a lock, not a fainter name, and the
    /// cursor never sits on it (UI-338). A row whose alert says why
    /// (「オーディオデバイスなし」) has no lock.
    #[test]
    fn a_row_that_cannot_run_has_a_lock_and_no_cursor() {
        let size = (1600.0, 1000.0);
        let mut pick = long(4, None);
        pick.groups[0].rows[0].enabled = false;
        pick.groups[0].rows[2].enabled = false;
        pick.footer = vec![
            PickRow::new("書き出し")
                .act(Action::Pick("f".into()))
                .disabled(),
        ];
        pick.start_on_current();
        assert_eq!(pick.selected, 1, "the cursor starts on the first live row");
        let l = cells(&pick, 260.0, size);
        let row = |name: &str| l.iter().find(|c| c.text == name).unwrap().clone();
        for name in ["row 0", "row 2", "書き出し"] {
            let c = row(name);
            assert!(c.locked && !c.live(), "{name}");
        }
        assert!(!row("row 1").locked && row("row 1").live());
        // ↓ steps over the locked row, and never onto the locked footer.
        for want in [3, 3] {
            pick.key("down", None, size);
            assert_eq!(pick.selected, want);
        }
        pick.key("up", None, size);
        assert_eq!(pick.selected, 1);
        pick.key("up", None, size);
        assert_eq!(pick.selected, 1, "row 0 is locked");
        assert!(
            cells(&pick, 260.0, size)
                .iter()
                .filter(|c| c.cursor)
                .all(|c| c.live())
        );

        // No device: the alert row says it, no lock, no cursor.
        let mut none = PickList::of(vec![
            PickRow::new(crate::start::rig::NO_DEVICE)
                .icon("alert")
                .disabled(),
        ]);
        none.start_on_current();
        let l = cells(&none, 280.0, size);
        let c = l
            .iter()
            .find(|c| c.text == crate::start::rig::NO_DEVICE)
            .unwrap();
        assert!(!c.locked && !c.live() && !c.cursor);
        assert_eq!(none.key("enter", None, size), PickKey::Done);
    }
}
