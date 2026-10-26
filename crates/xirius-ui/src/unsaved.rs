//! PRJ-030: only the person can discard a plugin snapshot that could not be saved.
use crate::draw::Pen;
use crate::studio::{Geometry, Rect, Studio};
use gpui::{accesskit, div, prelude::*, px};
use std::{cell::RefCell, rc::Rc};
use xirius_design::{Layout, Radius};

pub(crate) enum Pending {
    Open(Box<crate::opening::Opening>),
    Close,
}
pub(crate) struct Question {
    pending: Pending,
    proceed_focus: bool,
}
pub const TITLE: &str = "保存せずに進みますか";
pub const PROCEED: &str = "保存せずに進む";

impl Studio {
    pub(crate) fn ask_unsaved(&mut self, pending: Pending) {
        self.keep.plugin_state_failed = true;
        self.unsaved = Some(Question {
            pending,
            proceed_focus: false,
        });
    }
    pub(crate) fn answer_unsaved(&mut self, proceed: bool) {
        let Some(question) = self.unsaved.take() else {
            return;
        };
        match question.pending {
            Pending::Open(open) => {
                if proceed {
                    self.resume_without_plugin_state(*open);
                } else {
                    self.abandon_state_open(*open);
                }
            }
            Pending::Close if proceed => self.close_without_plugin_state(),
            Pending::Close => {}
        }
    }
    pub(crate) fn unsaved_key(&mut self, key: &str) -> bool {
        let Some(q) = self.unsaved.as_mut() else {
            return false;
        };
        match key {
            "escape" => self.answer_unsaved(false),
            "tab" | "left" | "right" => q.proceed_focus = !q.proceed_focus,
            "enter" => {
                let proceed = q.proceed_focus;
                self.answer_unsaved(proceed);
            }
            _ => {}
        }
        true
    }
    pub(crate) fn unsaved_press(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        if self.unsaved.is_none() {
            return false;
        }
        let (back, proceed) = buttons(g);
        if back.contains(x, y) {
            self.answer_unsaved(false);
        } else if proceed.contains(x, y) {
            self.answer_unsaved(true);
        }
        true
    }
}
fn card(g: &Geometry) -> Rect {
    let r = crate::opening_view::confirm_card(g);
    Rect::new(r.x, r.y - 16.0, r.w, r.h + 32.0)
}
fn buttons(g: &Geometry) -> (Rect, Rect) {
    let r = card(g);
    (
        Rect::new(r.right() - 184.0, r.bottom() - 44.0, 48.0, 28.0),
        Rect::new(r.right() - 128.0, r.bottom() - 44.0, 112.0, 28.0),
    )
}
pub(crate) fn paint(pen: &mut Pen, s: &Studio, g: &Geometry) -> bool {
    let Some(q) = &s.unsaved else {
        return false;
    };
    crate::cleanup_dialog::paint_veil(pen, g);
    use crate::popup::{Cell, Look, PopupLayout};
    let r = card(g);
    let (back, proceed) = buttons(g);
    let cells = vec![
        Cell::new(
            Rect::new(r.x + 16.0, r.y + 16.0, r.w - 32.0, 28.0),
            Look::Clean(crate::cleanup_dialog::CleanLook::Title),
        )
        .text(TITLE),
        Cell::new(
            Rect::new(r.x + 16.0, r.y + 48.0, r.w - 32.0, 24.0),
            Look::Label,
        )
        .text("プラグインの状態を保存できません"),
        Cell::new(back, Look::Button).text("戻る"),
        Cell::new(proceed, Look::Danger).text(PROCEED),
    ];
    crate::popup::paint(pen, &PopupLayout { rect: r, cells }, None);
    pen.outline(
        if q.proceed_focus { proceed } else { back },
        pen.c.ring,
        Radius::S,
        2.0,
    );
    true
}
pub(crate) fn accessible(s: Rc<RefCell<Studio>>, g: &Geometry) -> impl IntoElement {
    let mut root = div().id("unsaved-question").absolute();
    if s.borrow().unsaved.is_none() {
        return root;
    }
    let r = card(g);
    root = root
        .left(px(r.x))
        .top(px(r.y + Layout::TOP_BAR_H))
        .w(px(r.w))
        .h(px(r.h))
        .role(gpui::Role::Dialog)
        .aria_label(TITLE);
    let (back, proceed) = buttons(g);
    for (label, b, discard) in [("戻る", back, false), (PROCEED, proceed, true)] {
        let s = s.clone();
        root = root.child(
            div()
                .id(label)
                .absolute()
                .left(px(b.x - r.x))
                .top(px(b.y - r.y))
                .w(px(b.w))
                .h(px(b.h))
                .role(gpui::Role::Button)
                .aria_label(label)
                .on_a11y_action(accesskit::Action::Click, move |_, window, _| {
                    s.borrow_mut().answer_unsaved(discard);
                    window.refresh();
                }),
        );
    }
    root
}
