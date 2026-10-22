//! What applying a track preset had to change to fit this song (PRJ-007, MIX-030).
//!
//! A preset applied with nothing to adjust says nothing. When the command's `adjusted` has
//! rows, the notice at the foot of the lanes says how many and offers 詳しく; that opens this
//! list, every row, in the order the command reported them. The CLI and MCP get the same rows
//! in the command's data.

use serde_json::Value;
use xirius_design::Layout;

use crate::popup::{Action, Cell, Look, PopupKind};
use crate::studio::{Geometry, Rect, Studio};

pub const W: f32 = 520.0;
const PAD: f32 = 12.0;
const ROW_H: f32 = 26.0;
/// The most rows shown at once; the wheel moves on.
pub const ROWS: usize = 12;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// `filled`, `dropped`, `rounded` or `rejected`.
    pub kind: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub title: String,
    pub rows: Vec<Row>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Close,
}

impl View {
    /// The rows of a command's `adjusted`; None when there are none.
    pub fn of(title: String, adjusted: &Value) -> Option<View> {
        let rows: Vec<Row> = adjusted
            .as_array()?
            .iter()
            .filter_map(|r| {
                let what = r.get("what")?.as_str()?;
                let reason = r.get("reason")?.as_str()?;
                Some(Row {
                    kind: r
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    text: format!("{what}: {reason}"),
                })
            })
            .collect();
        (!rows.is_empty()).then_some(View { title, rows })
    }

    /// How far the wheel can move the list.
    pub fn max_offset(&self) -> usize {
        self.rows.len().saturating_sub(ROWS)
    }
}

/// The list from (0, 0) = the pop-over's top-left corner.
pub fn cells(v: &View, offset: usize) -> Vec<Cell> {
    let inner = W - PAD * 2.0;
    let mut cells = vec![
        Cell::new(Rect::new(PAD, PAD, inner, Layout::MENU_ROW_H), Look::Label)
            .text(v.title.clone()),
    ];
    let mut y = PAD + Layout::MENU_ROW_H;
    for row in v.rows.iter().skip(offset).take(ROWS) {
        cells.push(Cell::new(Rect::new(PAD, y, inner, ROW_H), Look::Note).text(row.text.clone()));
        y += ROW_H;
    }
    y += PAD;
    let w = 96.0;
    cells.push(
        Cell::new(
            Rect::new(W - PAD - w, y, w, Layout::MENU_ROW_H),
            Look::Primary,
        )
        .text("閉じる")
        .act(Action::Report(Act::Close)),
    );
    cells
}

impl Studio {
    /// Opens the list of `view` in the middle of the window's upper half.
    pub(crate) fn open_report(&mut self, g: &Geometry, view: View) {
        let anchor = Rect::new((g.size.0 - W) / 2.0, 96.0, W, 0.0);
        self.popup = Some(crate::popup::Popup::new(PopupKind::Report, anchor));
        if let Some(p) = &mut self.popup {
            p.report = Some(view);
        }
    }

    pub(crate) fn report_action(&mut self, act: Act) -> bool {
        match act {
            Act::Close => self.popup = None,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rows_say_the_part_and_why_in_the_order_reported() {
        let adjusted = json!([
            {"kind": "filled", "what": "Compressor の「ニー」", "reason": "プリセットに無いため、既定の値にしました"},
            {"kind": "rejected", "what": "プラグイン 1 つ", "reason": "この PC に無いため、読み込みませんでした"},
        ]);
        let v = View::of("トラックのプリセット「ボーカル」".into(), &adjusted).unwrap();
        assert_eq!(v.rows.len(), 2);
        assert_eq!(
            v.rows[0].text,
            "Compressor の「ニー」: プリセットに無いため、既定の値にしました"
        );
        assert_eq!(v.rows[1].kind, "rejected");
        assert!(View::of("x".into(), &json!([])).is_none());
        assert!(View::of("x".into(), &json!(null)).is_none());
    }

    #[test]
    fn a_long_list_scrolls_and_the_close_button_stays_last() {
        let rows: Vec<Value> = (0..30)
            .map(|i| json!({"kind":"filled","what":format!("項目 {i}"),"reason":"既定の値にしました"}))
            .collect();
        let v = View::of("t".into(), &Value::Array(rows)).unwrap();
        assert_eq!(v.max_offset(), 30 - ROWS);
        let cells = cells(&v, 3);
        assert_eq!(cells.len(), 1 + ROWS + 1);
        assert_eq!(cells[1].text, "項目 3: 既定の値にしました");
        assert_eq!(cells.last().unwrap().text, "閉じる");
    }
}
