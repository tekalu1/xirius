//! PRJ-030: only the person can discard a plugin snapshot that could not be saved.
//! PRJ-033: leaving an unsaved song asks 「変更を保存しますか」 (やめる・保存しない・保存).
use crate::draw::Pen;
use crate::studio::{Geometry, Rect, Studio};
use gpui::{accesskit, div, prelude::*, px};
use std::{cell::RefCell, rc::Rc};
use xirius_design::{FontSize, Layout, Radius, Weight};

pub(crate) enum Pending {
    Open(Box<crate::opening::Opening>),
    Close,
    /// PRJ-033: the song is unsaved and is being left.
    Save(SaveAsk),
    /// The song's own record could not be written (UI-288): closing loses the edits since.
    CloseUnwritten,
    /// UI-351: a draft on the start screen's list, to delete once the person says so.
    DeleteDraft(DraftAsk),
}

/// What waits for the answer to 「変更を保存しますか」.
pub(crate) enum After {
    /// The window closes (the end, an update, the window's ×).
    Close,
    /// Another song (or the start screen's empty one) is opened in its place.
    Open(Box<crate::opening::Opening>),
}

pub(crate) struct SaveAsk {
    pub after: After,
    /// The song has never been saved: 「保存…」 asks where, 「保存しない」 lets it go.
    pub new_song: bool,
    pub name: String,
}

/// The three answers to 「変更を保存しますか」, left to right (the owner's order).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    Cancel,
    Discard,
    Save,
}
const CHOICES: [Choice; 3] = [Choice::Cancel, Choice::Discard, Choice::Save];

/// The draft a deletion asks about.
pub(crate) struct DraftAsk {
    pub dir: std::path::PathBuf,
    pub name: String,
    /// When it was last edited, as the row says it.
    pub ago: String,
}
pub(crate) struct Question {
    pending: Pending,
    /// The button the keys are on, counted from the left.
    focus: usize,
}
pub const TITLE: &str = "保存せずに進みますか";
pub const PROCEED: &str = "保存せずに進む";
/// What the save question's title ends in, always shown; the song's name before it is cut.
pub const SAVE_TITLE_TAIL: &str = "の変更を保存しますか";
pub const SAVE_BODY: &str = "保存しない変更は下書きに残ります";
pub const NEW_SONG_TITLE: &str = "「新しい曲」を保存しますか";
pub const NEW_SONG_BODY: &str = "保存しないと、この曲は残りません";
pub const CANCEL: &str = "やめる";
pub const DISCARD: &str = "保存しない";
pub const SAVE: &str = "保存";
pub const SAVE_AS: &str = "保存…";
/// What the deletion's question ends in, always shown; the song's name before it is cut.
pub const DELETE_TITLE_TAIL: &str = "を削除しますか";
pub const DELETE: &str = "削除";
// design-lint: allow(guidance) 確認の文。戻せないことを言わないと、押す前に分からない
pub const DELETE_WARNING: &str = "削除すると元に戻せません。";

impl Studio {
    pub(crate) fn ask_unsaved(&mut self, pending: Pending) {
        if matches!(pending, Pending::Open(_) | Pending::Close) {
            self.keep.plugin_state_failed = true;
        }
        // 「保存」 is where the keys start (Enter saves); the others start on 「戻る」.
        let focus = if matches!(pending, Pending::Save(_)) {
            2
        } else {
            0
        };
        self.unsaved = Some(Question { pending, focus });
    }

    /// Asks 「変更を保存しますか」 before `after` (PRJ-033).
    pub(crate) fn ask_save(&mut self, after: After, new_song: bool) {
        // A draft an earlier 「保存しない」 was to let go of is the person's to say again.
        self.keep.trash = None;
        let name = self.project().name.clone();
        self.ask_unsaved(Pending::Save(SaveAsk {
            after,
            new_song,
            name,
        }));
    }

    /// The question's answer by its two-button sense: go on (`true`) or go back. For the save
    /// question, going on is 「保存」 and going back is 「やめる」.
    pub(crate) fn answer_unsaved(&mut self, proceed: bool) {
        if matches!(
            self.unsaved.as_ref().map(|q| &q.pending),
            Some(Pending::Save(_))
        ) {
            self.answer_save(if proceed {
                Choice::Save
            } else {
                Choice::Cancel
            });
            return;
        }
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
            Pending::Save(_) => unreachable!(),
        }
    }

    /// 「やめる」, 「保存しない」 or 「保存」 to 「変更を保存しますか」.
    pub(crate) fn answer_save(&mut self, choice: Choice) {
        let Some(Question {
            pending: Pending::Save(ask),
            ..
        }) = self.unsaved.take()
        else {
            return;
        };
        match choice {
            Choice::Cancel => match ask.after {
                After::Close => self.drop_update_install("更新を取りやめました".into()),
                After::Open(open) => self.abandon_state_open(*open),
            },
            Choice::Discard => {
                // The changes stay in the draft (the next open starts at the save); a new song's
                // draft goes to the trash once the song is gone.
                if ask.new_song {
                    self.keep.trash = self.song.record_dir();
                }
                match ask.after {
                    After::Close => self.close_without_plugin_state(false),
                    After::Open(open) => self.open_without_saving(*open),
                }
            }
            Choice::Save if ask.new_song => {
                self.save_as_after = Some(ask.after);
                self.file_request = Some(crate::studio::FileRequest::SaveAs);
            }
            Choice::Save => match ask.after {
                After::Close => self.save_before_close(),
                After::Open(open) => self.save_before_open(*open),
            },
        }
    }

    /// The button `i` from the left was pressed.
    fn answer_at(&mut self, i: usize) {
        if self.save_question().is_some() {
            if let Some(&choice) = CHOICES.get(i) {
                self.answer_save(choice);
            }
        } else {
            self.answer_unsaved(i == 1);
        }
    }

    /// The save question being asked, if it is that one.
    pub(crate) fn save_question(&self) -> Option<&SaveAsk> {
        match self.unsaved.as_ref().map(|q| &q.pending) {
            Some(Pending::Save(ask)) => Some(ask),
            _ => None,
        }
    }

    pub(crate) fn unsaved_key(&mut self, key: &str) -> bool {
        let Some(q) = self.unsaved.as_mut() else {
            return false;
        };
        let n = if matches!(q.pending, Pending::Save(_)) {
            3
        } else {
            2
        };
        match key {
            "escape" => self.answer_unsaved(false),
            "tab" | "right" => q.focus = (q.focus + 1) % n,
            "left" => q.focus = (q.focus + n - 1) % n,
            "enter" => {
                let i = q.focus;
                self.answer_at(i);
            }
            _ => {}
        }
        true
    }
    pub(crate) fn unsaved_press(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        if self.unsaved.is_none() {
            return false;
        }
        if let Some(i) = buttons_of(self, g).iter().position(|b| b.contains(x, y)) {
            self.answer_at(i);
        }
        true
    }

    /// Tests that leave a song with changes: 「保存しない」 to 「変更を保存しますか」 if it is
    /// asked, a new song's draft going to the test's own bin, and the draft's writes waited
    /// for. Whether it was asked.
    #[cfg(test)]
    pub(crate) fn discard_if_asked(&mut self) -> bool {
        if self.save_question().is_none() {
            return false;
        }
        if self.keep.test_bin.is_none() {
            self.keep.test_bin = self.keep.root().map(|r| r.join("bin"));
        }
        self.answer_save(Choice::Discard);
        if self.saving.is_some() {
            self.wait_for_save();
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
/// 「やめる」「保存しない」「保存」, left to right at the card's right, the mock's sizes.
pub(crate) fn save_buttons(g: &Geometry) -> [Rect; 3] {
    let r = card(g, 0.0);
    let y = r.bottom() - 44.0;
    [
        Rect::new(r.right() - 304.0, y, 64.0, 28.0),
        Rect::new(r.right() - 232.0, y, 96.0, 28.0),
        Rect::new(r.right() - 128.0, y, 112.0, 28.0),
    ]
}
/// The buttons of the question asked, left to right.
fn buttons_of(s: &Studio, g: &Geometry) -> Vec<Rect> {
    if s.save_question().is_some() {
        save_buttons(g).to_vec()
    } else {
        let (back, proceed) = buttons(g, extra(s));
        vec![back, proceed]
    }
}
/// The save question's card, its 「やめる」 and its 「保存」.
#[cfg(windows)]
pub(crate) fn question_rects(g: &Geometry) -> (Rect, Rect, Rect) {
    let [cancel, _, save] = save_buttons(g);
    (card(g, 0.0), cancel, save)
}

/// `「name」{tail}`, the name cut on its own side to leave the tail whole when the line is
/// `w` wide. `measure` is how wide a text comes out.
fn named_title(name: &str, tail: &str, w: f32, mut measure: impl FnMut(&str) -> f32) -> String {
    let tail = format!("」{tail}");
    let room = w - measure("「") - measure(&tail);
    let name = crate::draw::fit_text(name, room, &mut measure);
    format!("「{name}{tail}")
}

/// The deletion's question: `「name」を削除しますか`.
pub(crate) fn delete_title(name: &str, w: f32, measure: impl FnMut(&str) -> f32) -> String {
    named_title(name, DELETE_TITLE_TAIL, w, measure)
}

/// The save question's title: `「name」の変更を保存しますか`, or the new song's.
pub(crate) fn save_title(ask: &SaveAsk, w: f32, measure: impl FnMut(&str) -> f32) -> String {
    if ask.new_song {
        NEW_SONG_TITLE.into()
    } else {
        named_title(&ask.name, SAVE_TITLE_TAIL, w, measure)
    }
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
    let rects = buttons_of(s, g);
    let line = |y: f32| Rect::new(r.x + 16.0, r.y + y, r.w - 32.0, 24.0);
    let mut cells = vec![Cell::new(
        Rect::new(r.x + 16.0, r.y + 16.0, r.w - 32.0, 28.0),
        Look::Clean(crate::cleanup_dialog::CleanLook::Title),
    )];
    let labels: Vec<(&str, Look)> = match &q.pending {
        Pending::DeleteDraft(ask) => {
            let w = r.w - 32.0;
            let title = delete_title(&ask.name, w, |t| {
                pen.measure(t, FontSize::Lead, Weight::Semibold)
            });
            cells[0] = cells[0].clone().text(title);
            for (i, text) in delete_body(ask).into_iter().enumerate() {
                cells.push(Cell::new(line(48.0 + 20.0 * i as f32), Look::Label).text(text));
            }
            vec![("戻る", Look::Button), (DELETE, Look::Danger)]
        }
        Pending::Save(ask) => {
            let w = r.w - 32.0;
            let title = save_title(ask, w, |t| pen.measure(t, FontSize::Lead, Weight::Semibold));
            cells[0] = cells[0].clone().text(title);
            cells.push(Cell::new(line(48.0), Look::Label).text(save_body(ask)));
            vec![
                (CANCEL, Look::Button),
                (DISCARD, Look::Button),
                (save_label(ask), Look::Primary),
            ]
        }
        pending => {
            cells[0] = cells[0].clone().text(TITLE);
            cells.push(Cell::new(line(48.0), Look::Label).text(match pending {
                Pending::CloseUnwritten => "曲の変更を保存できません",
                _ => "プラグインの状態を保存できません",
            }));
            vec![("戻る", Look::Button), (PROCEED, Look::Danger)]
        }
    };
    for (&b, (label, look)) in rects.iter().zip(labels) {
        cells.push(Cell::new(b, look).text(label));
    }
    crate::popup::paint(
        pen,
        &PopupLayout {
            rect: r,
            cells,
            panels: Vec::new(),
        },
        None,
    );
    if let Some(&focused) = rects.get(q.focus) {
        pen.outline(focused, pen.c.ring, Radius::S, 2.0);
    }
    true
}
fn save_body(ask: &SaveAsk) -> &'static str {
    if ask.new_song {
        NEW_SONG_BODY
    } else {
        SAVE_BODY
    }
}
fn save_label(ask: &SaveAsk) -> &'static str {
    if ask.new_song { SAVE_AS } else { SAVE }
}
/// What a screen reader is told of the question: the dialog's name and place, and its
/// buttons left to right (label, place).
pub(crate) struct Said {
    pub title: String,
    pub card: Rect,
    pub buttons: Vec<(&'static str, Rect)>,
}

pub(crate) fn said(s: &Studio, g: &Geometry) -> Option<Said> {
    let rects = buttons_of(s, g);
    let (title, labels) = match s.unsaved.as_ref().map(|q| &q.pending)? {
        Pending::DeleteDraft(ask) => (
            format!("「{}」{DELETE_TITLE_TAIL}", ask.name),
            vec!["戻る", DELETE],
        ),
        Pending::Save(ask) => (
            save_title(ask, f32::INFINITY, |_| 0.0),
            vec![CANCEL, DISCARD, save_label(ask)],
        ),
        _ => (TITLE.to_string(), vec!["戻る", PROCEED]),
    };
    Some(Said {
        title,
        card: card(g, extra(s)),
        buttons: labels.into_iter().zip(rects).collect(),
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
    for (i, (label, b)) in said.buttons.into_iter().enumerate() {
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
                    s.borrow_mut().answer_at(i);
                    window.refresh();
                }),
        );
    }
    root
}

#[cfg(test)]
#[path = "unsaved_tests.rs"]
mod tests;
