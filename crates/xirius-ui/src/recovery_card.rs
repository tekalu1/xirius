//! 前回の状態を復元しました (PRJ-012, PRJ-014): what a song opened after a crash says.
//!
//! The mock (§ 06) is a card on a veil with the last operation and one button. The plan
//! (OPEN-05, OPEN-23) adds that the person can always throw the recovered steps away, so the
//! card has three: 続ける (the default, Enter), 破棄 (back to the state last saved; the steps
//! stay as redo) and バージョン一覧… (the list the song keeps, to pick an older state).
//! Esc is 続ける: nothing is lost by closing the card.

use gpui::{Bounds, Corners, point, px, size};
use xirius_core::recovery::Recovery;
use xirius_design::{FontSize, Layout, Radius, Space, Weight};

use crate::draw::{Align, Pen};
use crate::popup::{Action, Cell, Look, PopupKind, PopupLayout, ago};
use crate::song::Song;
use crate::studio::{Geometry, Rect, Studio};
use crate::topbar::{BarButton, BarLayout};

pub const W: f32 = 420.0;
const PAD: f32 = 20.0;
const HEAD_H: f32 = 28.0;
const LINE_H: f32 = 22.0;
const BUTTON_H: f32 = 30.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Continue,
    Discard,
    Versions,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    /// How many operations came back.
    pub steps: usize,
    /// The last one, as the history names it.
    pub last: Option<String>,
    pub at_ms: u64,
    /// The song has never been saved: 破棄 goes back to where it began.
    pub draft: bool,
    /// The button the pointer is over (an index of [`Card::layout`]'s cells).
    pub hover: Option<usize>,
}

impl Card {
    pub fn new(r: &Recovery, last: Option<String>, draft: bool) -> Card {
        Card {
            steps: r.steps,
            last,
            at_ms: r.last_at_ms,
            draft,
            hover: None,
        }
    }

    pub fn title(&self) -> &'static str {
        "前回の状態を復元しました"
    }

    pub fn count_line(&self) -> String {
        format!("記録から {} 件の操作を戻しました", self.steps)
    }

    pub fn last_line(&self, now_ms: u64) -> Option<String> {
        self.last
            .as_ref()
            .map(|l| format!("最後の操作: 「{l}」（{}）", ago(now_ms, self.at_ms)))
    }

    pub fn note(&self) -> &'static str {
        if self.draft {
            // design-lint: allow(guidance) 破棄の確認。どこへ戻るかを言わないと押せない
            "破棄すると、曲を作り始めた状態に戻ります。"
        } else {
            // design-lint: allow(guidance) 破棄の確認。どこへ戻るかを言わないと押せない
            "破棄すると、最後に保存した状態に戻ります。"
        }
    }

    /// The card centred in a body of `size`, with its three buttons.
    pub fn layout(&self, size: (f32, f32)) -> PopupLayout {
        let h = PAD + HEAD_H + Space::X3.px() + LINE_H * 3.0 + Space::X4.px() + BUTTON_H + PAD;
        let x = ((size.0 - W) / 2.0).max(0.0);
        let y = ((size.1 - h) / 2.0).max(0.0);
        let by = y + h - PAD - BUTTON_H;
        let gap = Space::X2.px();
        let (cw, dw, vw) = (96.0, 72.0, 136.0);
        let mut right = x + W - PAD;
        let mut cells = Vec::new();
        let mut put = |w: f32, look, text: &str, icon, button, tip: &str| {
            right -= w;
            let mut c = Cell::new(Rect::new(right, by, w, BUTTON_H), look)
                .text(text)
                .icon(icon)
                .act(Action::Recovery(button));
            c.tip = tip.to_string();
            cells.push(c);
            right -= gap;
        };
        put(cw, Look::Primary, "続ける", "check", Button::Continue, "");
        put(dw, Look::Button, "破棄", "close", Button::Discard, "");
        put(
            vw,
            Look::Button,
            "バージョン一覧",
            "history",
            Button::Versions,
            "",
        );
        PopupLayout {
            rect: Rect::new(x, y, W, h),
            cells,
        }
    }
}

impl Studio {
    /// Shows what came back when the song just opened was recovered, if anything did.
    /// Nothing to say about steps (the song was saved when it went down) is a notice.
    pub(crate) fn show_recovery(&mut self) {
        self.recovery = None;
        let Some(r) = self.song.recovery().cloned() else {
            return;
        };
        if r.steps == 0 {
            self.tell(r.line(|c| self.song.friendly(c)));
            self.song.clear_recovery();
            return;
        }
        let last = r.last_command.as_deref().map(|c| self.song.friendly(c));
        self.recovery = Some(Card::new(&r, last, self.song.is_draft()));
    }

    /// A press while the card is up. Every press belongs to the card.
    pub fn recovery_press(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(card) = &self.recovery else {
            return false;
        };
        let l = card.layout(g.size);
        if let Some(Action::Recovery(b)) = l
            .cells
            .iter()
            .find(|c| c.rect.contains(x, y))
            .and_then(|c| c.action.clone())
        {
            self.recovery_button(g, b);
        }
        true
    }

    pub fn recovery_hover(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(card) = &mut self.recovery else {
            return false;
        };
        let hover = card.layout(g.size).item_at(x, y);
        let changed = hover != card.hover;
        card.hover = hover;
        changed
    }

    /// Enter and Esc continue; every other key waits for an answer.
    pub fn recovery_key(&mut self, g: &Geometry, key: &str) -> bool {
        if self.recovery.is_none() {
            return false;
        }
        if matches!(key, "enter" | "escape") {
            self.recovery_button(g, Button::Continue);
        }
        true
    }

    pub fn recovery_button(&mut self, g: &Geometry, b: Button) {
        if self.recovery.take().is_none() {
            return;
        }
        self.song.clear_recovery();
        match b {
            Button::Continue => {}
            Button::Discard => {
                self.command(Song::discard_unsaved);
                self.follow_selection();
                self.rebuild(g);
            }
            Button::Versions => {
                let mut anchor =
                    BarLayout::new(g.size.0, &self.project().name).rect(BarButton::Saved);
                anchor.y -= Layout::TOP_BAR_H;
                self.open_popup(PopupKind::Versions, anchor);
            }
        }
    }
}

/// The veil over the body and the card on it.
pub fn paint(pen: &mut Pen, card: &Card, g: &Geometry) {
    let c = pen.c;
    pen.fill(Rect::new(0.0, 0.0, g.size.0, g.size.1), c.veil);
    let l = card.layout(g.size);
    let at = Bounds::new(
        point(px(pen.origin.0 + l.rect.x), px(pen.origin.1 + l.rect.y)),
        size(px(l.rect.w), px(l.rect.h)),
    );
    pen.window
        .paint_drop_shadows(at, Corners::all(px(Radius::M.px())), &c.float);
    crate::popup::paint_with(pen, &l, card.hover, &|_, _| {});
    let x = l.rect.x + PAD;
    let w = l.rect.w - PAD * 2.0;
    let big = Layout::ICON_SIZE + 6.0;
    pen.icon(
        "restore",
        Rect::new(x, l.rect.y + PAD + (HEAD_H - big) / 2.0, big, big),
        c.ok,
    );
    pen.text(
        card.title(),
        FontSize::Lead,
        Weight::Medium,
        c.ink_strong,
        Rect::new(
            x + big + Space::X2.px(),
            l.rect.y + PAD,
            w - big - Space::X2.px(),
            HEAD_H,
        ),
        Align::Left,
    );
    let now = xirius_core::store::now_ms();
    let y = l.rect.y + PAD + HEAD_H + Space::X3.px();
    let row = |i: f32| Rect::new(x, y + LINE_H * i, w, LINE_H);
    pen.text(
        &card.count_line(),
        FontSize::Body,
        Weight::Regular,
        c.ink_strong,
        row(0.0),
        Align::Left,
    );
    if let Some(last) = card.last_line(now) {
        pen.text(
            &last,
            FontSize::Small,
            Weight::Regular,
            c.ink,
            row(1.0),
            Align::Left,
        );
    }
    pen.text(
        card.note(),
        FontSize::Caption,
        Weight::Regular,
        c.ink,
        row(2.0),
        Align::Left,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keep::Keep;
    use serde_json::json;

    /// A folder of the test's own, removed when dropped (also when the test fails).
    fn scratch(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("xirius-ui-card-{tag}-"))
            .tempdir()
            .unwrap()
    }

    /// A song with `n` edits after its save, whose process "went down": the session is
    /// leaked so its lock stays, then the lock is rewritten to name a process that is gone.
    /// The song's folder goes when the guard is dropped.
    fn crashed_song(tag: &str, edits: usize) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir_tmp = scratch(tag);
        let dir = dir_tmp.path().join("song");
        let mut s = Song::empty();
        s.run("track.add.audio", json!({ "name": "saved" }))
            .unwrap();
        s.save_as(&dir).unwrap();
        for i in 0..edits {
            s.run("track.add.audio", json!({ "name": format!("t{i}") }))
                .unwrap();
        }
        // It went down once the edits were on the disk (the writer's thread is still
        // working on them as the last edit returns).
        s.simulate_crash();
        std::fs::write(dir.join("session.lock"), u32::MAX.to_string()).unwrap();
        (dir_tmp, dir)
    }

    fn studio_opening(dir: &std::path::Path) -> Studio {
        let mut s = Studio::new(Song::empty());
        s.finish_file_request_sync(crate::studio::FileRequest::Open, dir);
        assert!(s.error.is_none(), "{:?}", s.error);
        s
    }

    #[test]
    fn a_crashed_song_opens_with_the_count_and_the_last_operation() {
        let (_tmp, dir) = crashed_song("count", 3);
        let s = studio_opening(&dir);
        let card = s.recovery.as_ref().expect("the card is up");
        assert_eq!(card.steps, 3);
        assert_eq!(card.count_line(), "記録から 3 件の操作を戻しました");
        let last = card.last_line(xirius_core::store::now_ms()).unwrap();
        assert!(last.starts_with("最後の操作: 「"), "{last}");
        assert!(
            last.ends_with("（たった今）") || last.contains("分前"),
            "{last}"
        );
        assert_eq!(s.project().tracks.len(), 5, "Master, saved and three more");
    }

    #[test]
    fn continuing_keeps_the_steps_and_closes_the_card() {
        let (_tmp, dir) = crashed_song("continue", 2);
        let mut s = studio_opening(&dir);
        let g = s.geometry(1440.0, 860.0);
        s.recovery_button(&g, Button::Continue);
        assert!(s.recovery.is_none());
        assert_eq!(s.project().tracks.len(), 4);
        assert!(s.song.recovery().is_none());
    }

    #[test]
    fn discarding_goes_back_to_the_save_and_the_steps_wait_as_redo() {
        let (_tmp, dir) = crashed_song("discard", 2);
        let mut s = studio_opening(&dir);
        let g = s.geometry(1440.0, 860.0);
        s.recovery_button(&g, Button::Discard);
        assert!(s.recovery.is_none());
        assert_eq!(s.project().tracks.len(), 2, "Master and saved");
        assert!(s.song.can_redo(), "the steps can be taken back");
    }

    #[test]
    fn the_versions_button_opens_the_list_of_the_song() {
        let (_tmp, dir) = crashed_song("versions", 1);
        let mut s = studio_opening(&dir);
        let g = s.geometry(1440.0, 860.0);
        s.recovery_button(&g, Button::Versions);
        assert!(s.recovery.is_none());
        let p = s.popup.as_ref().expect("the list is open");
        assert_eq!(p.kind, PopupKind::Versions);
        assert!(
            p.versions.iter().any(|v| v.name.contains("異常終了")
                || v.name.contains("終了前")
                || !v.name.is_empty()),
            "the state before the crash is a version"
        );
    }

    #[test]
    fn enter_and_escape_continue_and_a_letter_waits() {
        let (_tmp, dir) = crashed_song("keys", 1);
        let mut s = studio_opening(&dir);
        let g = s.geometry(1440.0, 860.0);
        assert!(s.recovery_key(&g, "a"));
        assert!(s.recovery.is_some());
        assert!(s.recovery_key(&g, "enter"));
        assert!(s.recovery.is_none());
        assert!(!s.recovery_key(&g, "enter"), "no card, no key taken");
    }

    #[test]
    fn a_press_on_a_button_answers_and_a_press_elsewhere_does_nothing() {
        let (_tmp, dir) = crashed_song("press", 1);
        let mut s = studio_opening(&dir);
        let g = s.geometry(1440.0, 860.0);
        assert!(s.recovery_press(&g, 2.0, 2.0));
        assert!(s.recovery.is_some(), "outside the card waits");
        let l = s.recovery.as_ref().unwrap().layout(g.size);
        let discard = l
            .cells
            .iter()
            .find(|c| c.action == Some(Action::Recovery(Button::Discard)))
            .unwrap()
            .rect;
        assert!(s.recovery_hover(&g, discard.x + 4.0, discard.y + 4.0));
        assert!(s.recovery_press(&g, discard.x + 4.0, discard.y + 4.0));
        assert!(s.recovery.is_none());
        assert_eq!(s.project().tracks.len(), 2);
    }

    #[test]
    fn a_song_that_was_saved_when_it_went_down_says_so_in_a_notice() {
        let (_tmp, dir) = crashed_song("nothing", 0);
        let s = studio_opening(&dir);
        assert!(s.recovery.is_none());
        assert!(s.notice_text().unwrap().contains("戻す操作はありません"));
    }

    #[test]
    fn a_song_closed_properly_shows_no_card() {
        let dir_tmp = scratch("clean");
        let dir = dir_tmp.path().join("song");
        let mut song = Song::empty();
        song.run("track.add.audio", json!({ "name": "x" })).unwrap();
        song.save_as(&dir).unwrap();
        drop(song);
        let s = studio_opening(&dir);
        assert!(s.recovery.is_none() && s.notice_text().is_none());
    }

    #[test]
    fn the_app_starts_on_the_song_a_crash_interrupted() {
        let root_tmp = scratch("start");
        let root = root_tmp.path().to_path_buf();
        let (_tmp, dir) = crashed_song("start-song", 2);
        let keep = Keep::at(root);
        keep.remember(&dir, "落ちた曲", false, 1);
        let mut s = Studio::new(Song::empty());
        keep.settle_recent();
        s.start_keeping(keep);
        s.wait_for_open_for_test();
        assert_eq!(s.recovery.as_ref().map(|c| c.steps), Some(2));
        assert_eq!(s.song.dir().as_deref(), Some(dir.as_path()));
    }

    #[test]
    fn the_card_is_inside_the_body_and_its_buttons_inside_the_card() {
        let (_tmp, dir) = crashed_song("layout", 1);
        let s = studio_opening(&dir);
        let l = s.recovery.as_ref().unwrap().layout((1440.0, 860.0));
        assert!(l.rect.x >= 0.0 && l.rect.right() <= 1440.0);
        assert_eq!(l.cells.len(), 3);
        for c in &l.cells {
            assert!(
                c.rect.x >= l.rect.x && c.rect.right() <= l.rect.right(),
                "{c:?}"
            );
            assert!(c.rect.bottom() <= l.rect.bottom());
        }
    }
}
