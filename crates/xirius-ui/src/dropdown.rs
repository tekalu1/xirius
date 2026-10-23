//! The common dropdown: a closed box with its value and a ⌄, and the menu it opens.
//!
//! The mixer's input and output rows (「マスター ⌄」) and the settings use the same box
//! and the same menu (`docs/ui-approved.md`, 設定の画面).

use gpui::{Bounds, Corners, point, px, size};
use xirius_design::{FontSize, Layout, Radius, Weight};

use crate::draw::{Align, Pen};
use crate::popup::{Action, Cell, Look, PopupLayout};
use crate::studio::Rect;

/// What the closed box sits on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// Sunk into a strip (the mixer): small text, a small ⌄.
    Well,
    /// Raised on a panel (the settings).
    Raised,
}

/// The closed box: its value, then a ⌄ at its end. A box that cannot open is dim.
pub fn field(pen: &mut Pen, r: Rect, label: &str, face: Face, hot: bool, enabled: bool) {
    let c = pen.c;
    let hot = hot && enabled;
    let (fill, text, mark, pad, gap, size) = match face {
        Face::Well => (
            if hot { c.surface_3 } else { c.surface_2 },
            FontSize::Caption,
            10.0,
            6.0,
            2.0,
            16.0,
        ),
        Face::Raised => (
            if hot { c.thumb } else { c.surface_3 },
            if r.h < 28.0 {
                FontSize::Caption
            } else {
                FontSize::Small
            },
            14.0,
            8.0,
            4.0,
            20.0,
        ),
    };
    let ink = if enabled { c.ink } else { c.ink_on_toggle };
    pen.round(r, fill, Radius::S);
    let m = Rect::new(r.right() - size, r.y + (r.h - mark) / 2.0, mark, mark);
    pen.icon("chevron-down", m, ink);
    let end = m.x - gap;
    let t = Rect::new(r.x + pad, r.y, end - r.x - pad, r.h);
    let strong = if enabled && face == Face::Raised {
        c.ink_strong
    } else {
        ink
    };
    pen.mask(t, |pen| {
        pen.text(label, text, Weight::Regular, strong, t, Align::Left);
    });
}

/// The menu a box opens: 「その他」's operations (a list to pick from is a
/// [`crate::pick_list::PickList`], UI-338).
#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub layout: PopupLayout,
}

impl Menu {
    /// A menu with headings, icons and hints (the presets, 「その他」), `width` wide.
    pub fn rich(anchor: Rect, picks: &[Pick], width: f32, bounds: Rect) -> Menu {
        let pad = Layout::POPUP_PAD;
        let w = width.min(bounds.w - 12.0);
        let h: f32 = picks.iter().map(Pick::height).sum::<f32>() + pad * 2.0;
        let mut y = anchor.bottom() + 4.0;
        if y + h > bounds.bottom() - 6.0 && anchor.y - 4.0 - h >= bounds.y + 6.0 {
            y = anchor.y - 4.0 - h;
        }
        let y = y.min(bounds.bottom() - 6.0 - h).max(bounds.y + 6.0);
        // Opens under the box's end when the box is narrower than the menu.
        let x = (anchor.right() - w)
            .min(anchor.x)
            .min(bounds.right() - 6.0 - w)
            .max(bounds.x + 6.0);
        let mut at = y + pad;
        let cells = picks
            .iter()
            .map(|p| {
                let r = Rect::new(x + pad, at, w - pad * 2.0, p.height());
                at += p.height();
                match &p.value {
                    None => Cell::new(Rect::new(r.x + 2.0, r.y, r.w - 2.0, r.h), Look::Label)
                        .text(p.label.clone()),
                    Some(v) => {
                        let mut cell = Cell::new(r, Look::Row).text(p.label.clone()).on(p.on);
                        if let Some(i) = p.icon {
                            cell = cell.icon(i);
                        }
                        if let Some(h) = &p.hint {
                            cell = cell.hint(h.clone());
                        }
                        if p.enabled {
                            cell.act(Action::Pick(v.clone()))
                        } else {
                            cell.off()
                        }
                    }
                }
            })
            .collect();
        Menu {
            layout: PopupLayout {
                rect: Rect::new(x, y, w, h),
                cells,
            },
        }
    }

    /// The choice under a point.
    pub fn at(&self, x: f32, y: f32) -> Option<usize> {
        self.layout.item_at(x, y)
    }

    /// What the choice `i` picks: none for a heading or a row that cannot
    /// run now (a dim row keeps its value to name it, UI-338).
    pub fn value(&self, i: usize) -> Option<&str> {
        let cell = self.layout.cells.get(i).filter(|c| c.live())?;
        match &cell.action {
            Some(Action::Pick(v)) => Some(v),
            _ => None,
        }
    }
}

/// A row of a [`Menu::rich`]: a choice, or a heading over the choices after it.
#[derive(Clone, Debug, PartialEq)]
pub struct Pick {
    /// None for a heading (or a gap, when the label is empty too).
    pub value: Option<String>,
    pub label: String,
    pub enabled: bool,
    pub icon: Option<&'static str>,
    pub hint: Option<String>,
    pub on: bool,
}

impl Pick {
    pub fn row(value: &str, label: &str) -> Pick {
        Pick {
            value: Some(value.to_string()),
            label: label.to_string(),
            enabled: true,
            icon: None,
            hint: None,
            on: false,
        }
    }

    pub fn heading(label: &str) -> Pick {
        Pick {
            value: None,
            ..Pick::row("", label)
        }
    }

    /// A little room between groups.
    pub fn gap() -> Pick {
        Pick::heading("")
    }

    pub fn on(mut self, on: bool) -> Pick {
        self.on = on;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Pick {
        self.enabled = enabled;
        self
    }

    pub fn icon(mut self, icon: &'static str) -> Pick {
        self.icon = Some(icon);
        self
    }

    pub fn hint(mut self, hint: String) -> Pick {
        self.hint = Some(hint);
        self
    }

    fn height(&self) -> f32 {
        match (&self.value, self.label.is_empty()) {
            (Some(_), _) => Layout::MENU_ROW_H,
            (None, false) => crate::popup::LABEL_H,
            (None, true) => 6.0,
        }
    }
}

/// A floating menu: the soft shadow all round it (UI-007), then the rows.
pub fn paint_menu(pen: &mut Pen, menu: &Menu, hover: Option<usize>) {
    let r = menu.layout.rect;
    let at = Bounds::new(
        point(px(pen.origin.0 + r.x), px(pen.origin.1 + r.y)),
        size(px(r.w), px(r.h)),
    );
    pen.window
        .paint_drop_shadows(at, Corners::all(px(Radius::M.px())), &pen.c.float);
    crate::popup::paint(pen, &menu.layout, hover);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choices(n: usize) -> Vec<Pick> {
        (0..n)
            .map(|i| {
                Pick::row(&format!("v{i}"), &format!("項目 {i}"))
                    .enabled(i != 2)
                    .on(i == 1)
            })
            .collect()
    }

    #[test]
    fn the_menu_opens_below_and_stays_inside() {
        let bounds = Rect::new(0.0, 0.0, 800.0, 600.0);
        let m = Menu::rich(
            Rect::new(100.0, 100.0, 240.0, 28.0),
            &choices(3),
            240.0,
            bounds,
        );
        assert_eq!(m.layout.rect.y, 132.0);
        assert_eq!(m.layout.rect.w, 240.0);
        assert!(m.layout.cells[1].on);
        // A dim choice cannot be picked.
        let c = m.layout.cells[2].rect;
        assert_eq!(m.at(c.x + 4.0, c.y + 4.0), None);
        let c = m.layout.cells[0].rect;
        assert_eq!(m.value(m.at(c.x + 4.0, c.y + 4.0).unwrap()), Some("v0"));

        // Near the bottom it opens above the box.
        let m = Menu::rich(
            Rect::new(700.0, 560.0, 80.0, 28.0),
            &choices(3),
            120.0,
            bounds,
        );
        assert!(m.layout.rect.bottom() <= 556.0);
        assert!(m.layout.rect.right() <= 794.0);
    }
}
