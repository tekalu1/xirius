//! Window-independent placement, pointer intent and ancestor routing.
use super::Run;
use crate::popup::{Action, Popup, PopupLayout};
use crate::studio::{Geometry, Rect, Studio};
use std::time::{Duration, Instant};

pub const HOVER_DELAY: Duration = Duration::from_millis(240);

pub fn child_rect(parent: Rect, row: Rect, size: (f32, f32), viewport: (f32, f32)) -> Rect {
    let w = size.0.min((viewport.0 - 8.0).max(1.0));
    let h = size.1.min((viewport.1 - 8.0).max(1.0));
    let x = if parent.right() + w <= viewport.0 - 4.0 {
        parent.right()
    } else {
        parent.x - w
    };
    Rect::new(
        x.clamp(4.0, (viewport.0 - w - 4.0).max(4.0)),
        row.y.clamp(4.0, (viewport.1 - h - 4.0).max(4.0)),
        w,
        h,
    )
}

/// Width of `a` that `b` covers.
fn overlap(a: Rect, b: Rect) -> f32 {
    (a.right().min(b.right()) - a.x.max(b.x)).max(0.0)
}

/// Whether a child panel at depth 1 or more shows its "戻る" row (R6). Beside its parent the
/// parent is the way back, so the row is dropped and the child's first row lines up with the
/// parent row that opened it; a child that covers more than half of the parent keeps it.
pub fn back_shown(popup: &Popup, viewport: (f32, f32)) -> bool {
    popup.ctx.first().is_none_or(|i| !i.back)
        || popup
            .layout(viewport)
            .cells
            .iter()
            .any(|c| c.action == Some(Action::Ctx(0)))
}

/// Drops the "戻る" cell and the gap after it, lifting the rest of the panel by their height.
fn without_back(placed: &mut PopupLayout) {
    let lift = xirius_design::Layout::MENU_ROW_H + xirius_design::Space::X2.px();
    placed.cells.retain(|c| c.action != Some(Action::Ctx(0)));
    for cell in &mut placed.cells {
        cell.rect.y -= lift;
    }
    placed.rect.h -= lift;
}

pub fn layout(popup: &Popup, viewport: (f32, f32)) -> PopupLayout {
    let mut out = PopupLayout {
        rect: Rect::default(),
        cells: Vec::new(),
        panels: Vec::new(),
    };
    let mut parent: Option<(Rect, Rect)> = None;
    for depth in 0..=popup.ctx_parents.len() {
        let mut panel = popup.clone();
        panel.ctx_parents.clear();
        let ancestor = popup.ctx_parents.get(depth);
        if let Some((rows, _, offset)) = ancestor {
            panel.offset = *offset;
            panel.ctx = rows.clone();
            panel.ctx_query = (depth == 0).then(String::new);
            panel.ctx_search = false;
        }
        let mut placed = panel.single_layout(viewport);
        if let Some((parent, row)) = parent {
            let has_back = panel.ctx.first().is_some_and(|i| i.back)
                && panel.offset == 0
                && placed
                    .cells
                    .iter()
                    .any(|c| c.action == Some(Action::Ctx(0)));
            let mut rect = child_rect(parent, row, (placed.rect.w, placed.rect.h), viewport);
            if has_back {
                let mut bare = placed.clone();
                without_back(&mut bare);
                // The first row sits level with the parent row, not the panel's top edge.
                let mut at = row;
                at.y -= xirius_design::Layout::POPUP_PAD;
                let beside = child_rect(parent, at, (bare.rect.w, bare.rect.h), viewport);
                if overlap(parent, beside) * 2.0 <= parent.w {
                    placed = bare;
                    rect = beside;
                }
            }
            for cell in &mut placed.cells {
                cell.rect.x += rect.x - placed.rect.x;
                cell.rect.y += rect.y - placed.rect.y;
            }
            placed.rect = rect;
        }
        if let Some((_, selected, _)) = ancestor {
            let row = placed
                .cells
                .iter()
                .find(|c| c.action == Some(Action::Ctx(*selected)))
                .map_or(placed.rect, |c| c.rect);
            parent = Some((placed.rect, row));
            for cell in &mut placed.cells {
                if let Some(action) = cell.action.take() {
                    cell.action = Some(Action::CtxAncestor(depth, Box::new(action)));
                }
            }
        }
        out.cells.extend(placed.cells);
        out.panels.push((placed.rect, out.cells.len()));
        out.rect = if depth == 0 {
            placed.rect
        } else {
            let x = out.rect.x.min(placed.rect.x);
            let y = out.rect.y.min(placed.rect.y);
            Rect::new(
                x,
                y,
                out.rect.right().max(placed.rect.right()) - x,
                out.rect.bottom().max(placed.rect.bottom()) - y,
            )
        };
    }
    out
}

/// Inside the triangle from the previous pointer to the child's facing edge.
pub fn toward_child(from: (f32, f32), to: (f32, f32), child: Rect) -> bool {
    let edge = if child.x >= from.0 {
        child.x
    } else {
        child.right()
    };
    let dx = edge - from.0;
    if dx.abs() < 1.0 {
        return false;
    }
    let progress = (to.0 - from.0) / dx;
    if !(0.0..=1.0).contains(&progress) || progress == 0.0 {
        return false;
    }
    let top = from.1 + (child.y - from.1) * progress;
    let bottom = from.1 + (child.bottom() - from.1) * progress;
    to.1 >= top.min(bottom) && to.1 <= top.max(bottom)
}

impl Studio {
    pub(crate) fn context_restore(&mut self, depth: usize) {
        let Some(state) = &mut self.ctx else { return };
        let mut offset = self.popup.as_ref().map_or(0, |p| p.offset);
        while state.trail.len() > depth {
            state.items = state.trail.pop().unwrap();
            state.cursor = state.cursors.pop().flatten();
            state.parent_rows.pop();
            offset = state.parent_offsets.pop().unwrap_or(0);
        }
        state.search = if depth == 0 {
            super::search::Search::of(&state.items)
        } else {
            None
        };
        state.field = None;
        state.hover_pending = None;
        if let Some(popup) = &mut self.popup {
            popup.offset = offset;
        }
        self.refresh_popup();
    }

    pub(crate) fn context_hover(&mut self, g: &Geometry, x: f32, y: f32, now: Instant) -> bool {
        if !self.context_open() {
            return false;
        }
        let layout = self.popup.as_ref().unwrap().layout(g.size);
        let action = layout
            .item_at(x, y)
            .and_then(|i| layout.cells[i].action.as_ref());
        let state = self.ctx.as_mut().unwrap();
        let previous = state.last_pointer.replace((x, y));
        let selected = match action {
            Some(Action::CtxAncestor(depth, action)) => match **action {
                Action::Ctx(row) => Some((*depth, row)),
                _ => None,
            },
            Some(Action::Ctx(row)) => Some((state.trail.len(), *row)),
            _ => None,
        };
        let Some((depth, row)) = selected else {
            state.hover_pending = None;
            return false;
        };
        if let Some((child, _)) = layout.panels.get(depth + 1)
            && previous.is_some_and(|p| toward_child(p, (x, y), *child))
        {
            state.hover_pending = None;
            return false;
        }
        if state.parent_rows.get(depth) == Some(&row) {
            state.hover_pending = None;
            return false;
        }
        let rows = state.trail.get(depth).unwrap_or(&state.items);
        if rows.get(row).is_none_or(|i| {
            !i.enabled || (!matches!(i.run, Run::Sub(_)) && depth == state.trail.len())
        }) {
            state.hover_pending = None;
            return false;
        }
        if state
            .hover_pending
            .is_none_or(|(d, r, _)| (d, r) != (depth, row))
        {
            state.hover_pending = Some((depth, row, now));
        }
        true
    }

    pub(crate) fn context_hover_tick(&mut self, g: &Geometry, now: Instant) -> bool {
        if !self.context_open() {
            return false;
        }
        let Some((depth, row, start)) = self.ctx.as_ref().and_then(|s| s.hover_pending) else {
            return false;
        };
        if now.saturating_duration_since(start) >= HOVER_DELAY {
            self.context_restore(depth);
            if self
                .ctx
                .as_ref()
                .is_some_and(|s| matches!(s.items[row].run, Run::Sub(_)))
            {
                self.context_pick(g, row);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Item;
    use serde_json::json;

    #[test]
    fn children_fit_both_edges_and_pointer_intent_is_directional() {
        let parent = Rect::new(700.0, 500.0, 250.0, 200.0);
        let child = child_rect(
            parent,
            Rect::new(700.0, 680.0, 250.0, 28.0),
            (240.0, 300.0),
            (1000.0, 800.0),
        );
        assert_eq!(child.x, 460.0);
        assert_eq!(child.bottom(), 796.0);
        assert!(toward_child((750.0, 600.0), (725.0, 610.0), child));
        assert!(!toward_child((750.0, 600.0), (775.0, 610.0), child));
        let narrow = child_rect(parent, parent, (500.0, 1000.0), (320.0, 240.0));
        assert!(narrow.right() <= 316.0 && narrow.bottom() <= 236.0);
    }

    #[test]
    fn hover_waits_240ms_and_retains_clickable_ancestors() {
        let (mut s, g) = crate::context::tests::studio();
        s.open_context(
            vec![Item::sub(
                "編集",
                vec![Item::ask(
                    "名前",
                    "clip.rename",
                    json!({"id":"c1"}),
                    "name",
                    "old",
                )],
            )],
            500.0,
            300.0,
        );
        let layout = s.popup.as_ref().unwrap().layout(g.size);
        let row = layout
            .cells
            .iter()
            .find(|c| c.action == Some(Action::Ctx(0)))
            .unwrap()
            .rect;
        let now = Instant::now();
        assert!(s.context_hover(&g, row.x + 20.0, row.y + 14.0, now));
        s.context_hover_tick(&g, now + Duration::from_millis(239));
        assert!(s.ctx.as_ref().unwrap().trail.is_empty());
        s.context_hover_tick(&g, now + HOVER_DELAY);
        let layout = s.popup.as_ref().unwrap().layout(g.size);
        assert_eq!(layout.panels.len(), 2);
        assert!(
            layout
                .cells
                .iter()
                .any(|c| c.action == Some(Action::CtxAncestor(0, Box::new(Action::Ctx(0)))))
        );
        assert!(s.context_navigation(&g, "left"));
        assert!(s.ctx.as_ref().unwrap().trail.is_empty());
        assert!(s.context_choose(&g, "編集"));
        assert_eq!(s.ctx.as_ref().unwrap().trail.len(), 1);
    }
}
