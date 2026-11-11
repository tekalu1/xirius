//! PRJ-030: only the person can discard a plugin snapshot that could not be saved.
use crate::draw::Pen;
use crate::studio::{Geometry, Rect, Studio};
use gpui::{accesskit, div, prelude::*, px};
use std::{cell::RefCell, rc::Rc};
use xirius_design::{FontSize, Layout, Radius, Weight};

pub(crate) enum Pending {
    Open(Box<crate::opening::Opening>),
    Close,
    /// The song's own record could not be written (UI-288): closing loses the edits since.
    CloseUnwritten,
    /// UI-351: a draft on the start screen's list, to delete once the person says so.
    DeleteDraft(DraftAsk),
}

/// The draft a deletion asks about.
pub(crate) struct DraftAsk {
    pub dir: std::path::PathBuf,
    pub name: String,
    /// When it was last edited, as the row says it.
    pub ago: String,
}
pub(crate) struct Question {
    pending: Pending,
    proceed_focus: bool,
}
pub const TITLE: &str = "保存せずに進みますか";
pub const PROCEED: &str = "保存せずに進む";
/// What the deletion's question ends in, always shown; the song's name before it is cut.
pub const DELETE_TITLE_TAIL: &str = "を削除しますか";
pub const DELETE: &str = "削除";
// design-lint: allow(guidance) 確認の文。戻せないことを言わないと、押す前に分からない
pub const DELETE_WARNING: &str = "削除すると元に戻せません。";

impl Studio {
    pub(crate) fn ask_unsaved(&mut self, pending: Pending) {
        if !matches!(pending, Pending::CloseUnwritten | Pending::DeleteDraft(_)) {
            self.keep.plugin_state_failed = true;
        }
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
            Pending::Close if proceed => self.close_without_plugin_state(false),
            Pending::CloseUnwritten if proceed => self.close_without_plugin_state(true),
            Pending::DeleteDraft(ask) => {
                if proceed {
                    self.begin_draft_delete(ask.dir);
                }
            }
            Pending::Close | Pending::CloseUnwritten => {
                self.drop_update_install("更新を取りやめました".into())
            }
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
        let (back, proceed) = buttons(g, extra(self));
        if back.contains(x, y) {
            self.answer_unsaved(false);
        } else if proceed.contains(x, y) {
            self.answer_unsaved(true);
        }
        true
    }
}
/// The question's card; the deletion's may be taller by extra (the approved mock's two lines fit the plain card).
fn card(g: &Geometry, extra: f32) -> Rect {
    let r = crate::opening_view::confirm_card(g);
    Rect::new(r.x, r.y - 16.0 - extra / 2.0, r.w, r.h + 32.0 + extra)
}
const DRAFT_EXTRA: f32 = 0.0;
/// The deletion's question, if that is what is asked.
#[cfg(test)]
pub(crate) fn draft_ask(s: &Studio) -> Option<&DraftAsk> {
    match s.unsaved.as_ref().map(|q| &q.pending) {
        Some(Pending::DeleteDraft(ask)) => Some(ask),
        _ => None,
    }
}
/// Where the deletion's 「戻る」 and 「削除」 are.
#[cfg(test)]
pub(crate) fn delete_buttons(g: &Geometry) -> (Rect, Rect) {
    buttons(g, DRAFT_EXTRA)
}
fn extra(s: &Studio) -> f32 {
    match s.unsaved.as_ref().map(|q| &q.pending) {
        Some(Pending::DeleteDraft(_)) => DRAFT_EXTRA,
        _ => 0.0,
    }
}
fn buttons(g: &Geometry, extra: f32) -> (Rect, Rect) {
    let r = card(g, extra);
    (
        Rect::new(r.right() - 184.0, r.bottom() - 44.0, 48.0, 28.0),
        Rect::new(r.right() - 128.0, r.bottom() - 44.0, 112.0, 28.0),
    )
}
/// The deletion's question: `「name」を削除しますか`, the name cut on its own side to leave
/// the tail whole when the line is `w` wide. `measure` is how wide a text comes out.
pub(crate) fn delete_title(name: &str, w: f32, mut measure: impl FnMut(&str) -> f32) -> String {
    let tail = format!("」{DELETE_TITLE_TAIL}");
    let room = w - measure("「") - measure(&tail);
    let name = crate::draw::fit_text(name, room, &mut measure);
    format!("「{name}{tail}")
}

/// The two lines under the deletion's title.
pub(crate) fn delete_body(ask: &DraftAsk) -> [String; 2] {
    [
        // 「たった今」 takes no 「に」, the others do (「5 分前に」).
        if ask.ago == "たった今" {
            format!("{}編集した保存前の曲です。", ask.ago)
        } else {
            format!("{}に編集した保存前の曲です。", ask.ago)
        },
        DELETE_WARNING.to_string(),
    ]
}

pub(crate) fn paint(pen: &mut Pen, s: &Studio, g: &Geometry) -> bool {
    let Some(q) = &s.unsaved else {
        return false;
    };
    crate::cleanup_dialog::paint_veil(pen, g);
    use crate::popup::{Cell, Look, PopupLayout};
    let r = card(g, extra(s));
    let (back, proceed) = buttons(g, extra(s));
    let line = |y: f32| Rect::new(r.x + 16.0, r.y + y, r.w - 32.0, 24.0);
    let mut cells = vec![Cell::new(
        Rect::new(r.x + 16.0, r.y + 16.0, r.w - 32.0, 28.0),
        Look::Clean(crate::cleanup_dialog::CleanLook::Title),
    )];
    let proceed_label = match &q.pending {
        Pending::DeleteDraft(ask) => {
            let w = r.w - 32.0;
            let title = delete_title(&ask.name, w, |t| {
                pen.measure(t, FontSize::Lead, Weight::Semibold)
            });
            cells[0] = cells[0].clone().text(title);
            for (i, text) in delete_body(ask).into_iter().enumerate() {
                cells.push(Cell::new(line(48.0 + 20.0 * i as f32), Look::Label).text(text));
            }
            DELETE
        }
        pending => {
            cells[0] = cells[0].clone().text(TITLE);
            cells.push(Cell::new(line(48.0), Look::Label).text(match pending {
                Pending::CloseUnwritten => "曲の変更を保存できません",
                _ => "プラグインの状態を保存できません",
            }));
            PROCEED
        }
    };
    cells.push(Cell::new(back, Look::Button).text("戻る"));
    cells.push(Cell::new(proceed, Look::Danger).text(proceed_label));
    crate::popup::paint(
        pen,
        &PopupLayout {
            rect: r,
            cells,
            panels: Vec::new(),
        },
        None,
    );
    pen.outline(
        if q.proceed_focus { proceed } else { back },
        pen.c.ring,
        Radius::S,
        2.0,
    );
    true
}
/// What a screen reader is told of the question: the dialog's name and place, and its two
/// buttons (label, place, whether it goes on).
pub(crate) struct Said {
    pub title: String,
    pub card: Rect,
    pub buttons: [(&'static str, Rect, bool); 2],
}

pub(crate) fn said(s: &Studio, g: &Geometry) -> Option<Said> {
    let (title, proceed_label, extra) = match s.unsaved.as_ref().map(|q| &q.pending)? {
        Pending::DeleteDraft(ask) => (
            format!("「{}」{DELETE_TITLE_TAIL}", ask.name),
            DELETE,
            DRAFT_EXTRA,
        ),
        _ => (TITLE.to_string(), PROCEED, 0.0),
    };
    let (back, proceed) = buttons(g, extra);
    Some(Said {
        title,
        card: card(g, extra),
        buttons: [("戻る", back, false), (proceed_label, proceed, true)],
    })
}

pub(crate) fn accessible(s: Rc<RefCell<Studio>>, g: &Geometry) -> impl IntoElement {
    let mut root = div().id("unsaved-question").absolute();
    let Some(said) = said(&s.borrow(), g) else {
        return root;
    };
    let r = said.card;
    root = root
        .left(px(r.x))
        .top(px(r.y + Layout::TOP_BAR_H))
        .w(px(r.w))
        .h(px(r.h))
        .role(gpui::Role::Dialog)
        .aria_label(said.title);
    for (label, b, discard) in said.buttons {
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
