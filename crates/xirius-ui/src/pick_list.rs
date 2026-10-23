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

use crate::popup::{Action, Cell, Choice, Look, Popup, PopupKind};
use crate::studio::{Geometry, Rect, Studio};
use xirius_design::{FontSize, Layout, Space};

/// The tallest a list menu grows: it also shrinks to the screen's edge.
pub const MAX_H: f32 = 480.0;
/// What a list with no matches says, unless it names its own.
pub const NO_MATCH: &str = "一致する項目はありません";
/// The heading a group shows: its height.
const HEAD_H: f32 = 22.0;

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

    /// How many rows the drawn list can show at the window `size`: [`MAX_H`]
    /// (and the screen's edge) less the search box, the group headings that
    /// stay with the gaps between them, and the footer. `cells` lays out
    /// with this same count, so every row the wheel or the keys reach is
    /// drawn.
    pub fn capacity(&self, size: (f32, f32)) -> usize {
        let query = self.query();
        let heads: f32 = self
            .groups
            .iter()
            .filter(|g| g.rows.iter().any(|r| r.matches(&query)))
            .enumerate()
            .map(|(i, g)| {
                (if i > 0 { Space::X2.px() } else { 0.0 })
                    + if g.heading.is_empty() { 0.0 } else { HEAD_H }
            })
            .sum();
        let foot = if self.footer.is_empty() {
            0.0
        } else {
            Space::X2.px() + self.footer.len() as f32 * Layout::MENU_ROW_H
        };
        let chrome = Layout::POPUP_PAD * 2.0 + Layout::MENU_ROW_H + heads + foot;
        let cap_h = MAX_H.min(size.1 - 12.0);
        ((cap_h - chrome) / Layout::MENU_ROW_H).floor().max(1.0) as usize
    }

    /// How far the wheel can move the list down at the window `size`.
    pub fn max_offset(&self, size: (f32, f32)) -> usize {
        self.visible().len().saturating_sub(self.capacity(size))
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
        self.selected = 0;
        self.offset = 0;
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
/// them than show: `n` rows from `offset` on, `cap` shown starting at `top`.
fn thumb(w: f32, top: f32, n: usize, offset: usize, cap: usize) -> Option<Rect> {
    let more = n.checked_sub(cap).filter(|m| *m > 0)?;
    let h = cap as f32 * Layout::MENU_ROW_H;
    let th = (h * cap as f32 / n as f32).max(16.0);
    let y = top + (h - th) * (offset.min(more) as f32 / more as f32);
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
    // The rows the window shows: `capacity` counts them the same way (the
    // search box, the headings still in view and their gaps, the footer).
    let cap = pick.capacity(size);

    let mut out = vec![
        Cell::new(Rect::new(pad, pad, w - pad * 2.0, row_h), Look::Input)
            .text(pick.query.clone())
            .icon("search")
            .on(true),
    ];
    let mut y = pad + row_h;

    let n = pick.visible().len();
    let offset = pick.offset.min(n.saturating_sub(cap));
    if n == 0 && typed {
        // Nothing matches: the rows hide and the status shows.
        out.push(
            Cell::new(Rect::new(pad, y, w - pad * 2.0, row_h * 2.0), Look::Status)
                .text(pick.empty.clone()),
        );
        y += row_h * 2.0;
    } else {
        let mut i = 0usize;
        let mut first = true;
        for group in &pick.groups {
            let rows: Vec<&PickRow> = group.rows.iter().filter(|r| r.matches(&query)).collect();
            if rows.is_empty() {
                continue;
            }
            let start = i; // the group's first row in the flat visible order
            i += rows.len();
            // Rows of the group the window `offset..offset+cap` shows.
            if i <= offset || start >= offset + cap {
                continue;
            }
            if !first {
                y += Space::X2.px();
            }
            first = false;
            // A heading shows only while its first row did not scroll past.
            if !group.heading.is_empty() && start >= offset {
                out.push(
                    Cell::new(Rect::new(pad, y, w - pad * 2.0, HEAD_H), Look::Label)
                        .text(group.heading.clone()),
                );
                y += HEAD_H;
            }
            for (k, row) in rows.iter().enumerate() {
                let flat = start + k;
                if !(offset..offset + cap).contains(&flat) {
                    continue;
                }
                let r = Rect::new(pad, y, w - pad * 2.0, row_h);
                y += row_h;
                if row.note {
                    out.push(Cell::new(r, Look::Note).text(row.label.clone()));
                    continue;
                }
                let icon = row.icon.or(pick.icon);
                let mut cell = Cell::new(r, Look::Row)
                    .text(row.label.clone())
                    .hint(row.hint.clone())
                    .on(row.on)
                    .cur(flat == pick.selected);
                if let Some(icon) = icon {
                    cell = cell.icon(icon);
                }
                if let Some(act) = &row.act {
                    cell = cell.act(act.clone());
                }
                if !row.enabled {
                    cell = cell.off();
                }
                if !row.tip.is_empty() {
                    cell = cell.tip(row.tip.clone());
                } else if crate::browser::text_width(&row.label, FontSize::Small)
                    > row_text_w(r, row.on, icon.is_some())
                {
                    // A name the row cuts with 「…」 is said in full in the balloon.
                    cell = cell.tip(row.label.clone());
                }
                out.push(cell);
            }
        }
        out.extend(thumb(w, pad + row_h, n, offset, cap).map(|r| Cell::new(r, Look::Thumb)));
    }

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
                    .cur(i == pick.selected)
            };
            if let Some(icon) = row.icon {
                cell = cell.icon(icon);
            }
            if let Some(act) = &row.act {
                cell = cell.act(act.clone());
            }
            if !row.enabled {
                cell = cell.off();
            }
            out.push(cell.foot());
        }
    }
    out
}

/// Keeps the cursor's row inside the list's window on the keys' moves.
/// A row of the footer is always in view (it sits under the list); the
/// offset just settles at its end.
fn keep_on_screen(pick: &mut PickList, size: (f32, f32)) {
    let cap = pick.capacity(size);
    if pick.selected < pick.offset {
        pick.offset = pick.selected;
    } else if pick.selected >= pick.offset + cap {
        pick.offset = pick.selected + 1 - cap;
    }
    pick.offset = pick.offset.min(pick.max_offset(size));
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
        if !self.popup.as_ref().is_some_and(|p| p.pick.is_some()) {
            return false;
        }
        let act = {
            let pick = self
                .popup
                .as_mut()
                .and_then(|p| p.pick.as_mut())
                .expect("a pick list");
            match key {
                "escape" => {
                    if pick.query.is_empty() {
                        self.popup = None;
                        self.close_wheel_group();
                    } else {
                        pick.clear_query();
                    }
                    return true;
                }
                "enter" => pick
                    .nav()
                    .get(pick.selected)
                    .filter(|r| r.enabled)
                    .and_then(|r| r.act.clone()),
                "down" => {
                    let n = pick.nav().len();
                    pick.selected = (pick.selected + 1).min(n.saturating_sub(1));
                    keep_on_screen(pick, g.size);
                    return true;
                }
                "up" => {
                    pick.selected = pick.selected.saturating_sub(1);
                    keep_on_screen(pick, g.size);
                    return true;
                }
                "backspace" => {
                    pick.query.pop();
                    pick.selected = 0;
                    pick.offset = 0;
                    return true;
                }
                _ => {
                    if let Some(c) = ch.filter(|c| !c.chars().any(char::is_control)) {
                        pick.query.push_str(c);
                        pick.selected = 0;
                        pick.offset = 0;
                        return true;
                    }
                    return false;
                }
            }
        };
        if let Some(act) = act {
            self.popup_action(g, act);
        }
        true
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
        s.open_popup(kind, Rect::new(100.0, 100.0, 100.0, 28.0));
        if let Some(pick) = s.popup.as_ref().and_then(|p| p.pick.as_ref()) {
            let want = 1.min(pick.nav().len().saturating_sub(1));
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
}
