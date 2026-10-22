//! Search and keyboard selection within the insert picker and the instrument picker (UI-297).
use crate::popup::{Action, Cell, Choice, Look, PICK_ROWS, Popup, PopupKind};
use crate::studio::{Rect, Studio};
use xirius_design::{FontSize, Layout, Space};

/// Is it a list with a search box over it: the effects of an insert row, or the instruments?
pub fn searched(kind: PopupKind) -> bool {
    matches!(kind, PopupKind::Insert(_) | PopupKind::Instrument(_))
}

/// The choices the search box lets through: those whose name or maker (both in the label,
/// 「{名前} — {作り手}」) holds what was typed, in any case.
pub fn found(p: &Popup) -> impl Iterator<Item = &Choice> {
    let query = p.fx_query.to_lowercase();
    p.choices
        .iter()
        .filter(move |c| query.is_empty() || c.label.to_lowercase().contains(&query))
}

/// The scroll thumb at the right edge of the rows, when there are more of them than show:
/// `n` choices from `offset` on, in rows starting at `top` (UI-297).
pub fn thumb(w: f32, top: f32, n: usize, offset: usize) -> Option<Rect> {
    let more = n.checked_sub(PICK_ROWS).filter(|m| *m > 0)?;
    let h = PICK_ROWS as f32 * Layout::MENU_ROW_H;
    let th = (h * PICK_ROWS as f32 / n as f32).max(16.0);
    let y = top + (h - th) * (offset.min(more) as f32 / more as f32);
    let pad = Layout::POPUP_PAD;
    Some(Rect::new(w - pad - 1.0 - 4.0, y, 4.0, th))
}

pub fn cells(p: &Popup, w: f32) -> Vec<Cell> {
    let pad = Layout::POPUP_PAD;
    let icon = match p.kind {
        PopupKind::Instrument(_) => "keys",
        _ => "effects",
    };
    let mut out = vec![
        Cell::new(
            Rect::new(pad, pad, w - pad * 2.0, Layout::MENU_ROW_H),
            Look::Input,
        )
        .text(p.fx_query.clone())
        .icon("search")
        .on(true),
    ];
    let top = pad + Layout::MENU_ROW_H;
    out.extend(
        found(p)
            .skip(p.offset)
            .take(PICK_ROWS)
            .enumerate()
            .map(|(i, c)| {
                let r = Rect::new(
                    pad,
                    top + i as f32 * Layout::MENU_ROW_H,
                    w - pad * 2.0,
                    Layout::MENU_ROW_H,
                );
                // A line with nothing to pick (the scan, or why the list is empty) only says so.
                if c.to.is_empty() {
                    return Cell::new(r, Look::Note).text(c.label.clone());
                }
                let on = i == p.selected.unwrap_or(0);
                let cell = Cell::new(r, Look::Row)
                    .text(c.label.clone())
                    .icon(icon)
                    .on(on)
                    .act(Action::Pick(c.to.clone()));
                // A name the row cuts with 「…」 is said in full in the balloon.
                if crate::browser::text_width(&c.label, FontSize::Small) > row_text_w(r, on) {
                    cell.tip(c.label.clone())
                } else {
                    cell
                }
            }),
    );
    out.extend(thumb(w, top, found(p).count(), p.offset).map(|r| Cell::new(r, Look::Thumb)));
    out
}

/// How wide the name of a row of `r` can be: from after its icon to before its check.
fn row_text_w(r: Rect, on: bool) -> f32 {
    let pad = Space::X2.px();
    let check = if on { Layout::ICON_SIZE + pad } else { 0.0 };
    r.w - pad - Layout::ICON_SIZE - pad - pad - check
}

impl Studio {
    pub fn fx_typed(&mut self, key: &str, ch: Option<&str>) -> bool {
        if key == "escape" && self.mixer.fx_drag.take().is_some() {
            return true;
        }
        if let Some(p) = &mut self.popup
            && searched(p.kind)
        {
            match key {
                "escape" => {
                    self.popup = None;
                }
                "enter" => {
                    let to = found(p)
                        .nth(p.offset + p.selected.unwrap_or(0))
                        .map(|c| c.to.clone())
                        .filter(|to| !to.is_empty());
                    let kind = p.kind;
                    if let Some(to) = to {
                        self.popup = None;
                        self.mixer_pick(kind, &to);
                    }
                }
                "down" => {
                    let count = found(p).count();
                    let n = (p.offset + p.selected.unwrap_or(0) + 1).min(count.saturating_sub(1));
                    p.offset = p.offset.max(n.saturating_sub(PICK_ROWS - 1));
                    p.selected = Some(n - p.offset);
                }
                "up" => {
                    let n = (p.offset + p.selected.unwrap_or(0)).saturating_sub(1);
                    p.offset = p.offset.min(n);
                    p.selected = Some(n - p.offset);
                }
                "backspace" => {
                    p.fx_query.pop();
                    p.offset = 0;
                    p.selected = Some(0);
                }
                _ => {
                    if let Some(ch) = ch.filter(|c| !c.chars().any(char::is_control)) {
                        p.fx_query.push_str(ch);
                        p.offset = 0;
                        p.selected = Some(0);
                    } else {
                        return false;
                    }
                }
            }
            return true;
        }
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
        false
    }
}
