//! ショートカット, マクロ: the macros as a group at the top of the list, and the editor of the
//! open one (KEY-015). A macro's key is a row of the keymap like any other; the editor changes
//! the macro only through `macro.set`, `macro.rename` and `macro.remove`.

use xirius_core::command::Registry;
use xirius_design::{FontSize, Radius, Weight};
use xirius_prefs::catalog::{self, Cat};
use xirius_prefs::chord::Os;
use xirius_prefs::keymap::{Issue, Row};
use xirius_prefs::macro_ext::{self, Candidate};
use xirius_prefs::macros::{Macro, run_line};
use xirius_prefs::{Settings, ext};

use super::keys;
use super::parts;
use super::scene::{self, B};
use super::{Do, FieldId, SettingsUi};
use crate::draw::Align;
use crate::studio::Rect;

/// What `keys::row` needs to know about a row that is a macro.
pub(super) struct MacroRow<'a> {
    pub name: &'a str,
    pub count: usize,
}

/// How many rows of commands the picker lists at once.
pub(super) const PICK_ROWS: usize = 8;

const STEP_H: f32 = 34.0;
const INPUT_H: f32 = 28.0;
const PAD: f32 = 10.0;

/// The commands the picker lists for what is typed: those of the macro's own place first, then
/// the catalog's order.
pub(super) fn pick_list<'a>(
    cands: &'a [Candidate],
    query: &str,
    first_context: &str,
) -> Vec<&'a Candidate> {
    let q = query.trim().to_lowercase();
    let mut out: Vec<&Candidate> = cands
        .iter()
        .filter(|c| {
            q.is_empty()
                || c.label.to_lowercase().contains(&q)
                || c.command.to_lowercase().contains(&q)
                || ctx_label(c.context).to_lowercase().contains(&q)
        })
        .collect();
    out.sort_by_key(|c| c.context != first_context);
    out
}

fn ctx_label(name: &str) -> &str {
    catalog::context(name).map(|c| c.label).unwrap_or(name)
}

/// The row of the macro in the list of keys: the keymap's, or one with no key yet.
fn key_row(rows: &[Row], m: &Macro) -> Row {
    let context = macro_ext::context_of(&m.steps);
    let line = run_line(&m.name);
    rows.iter()
        .find(|r| r.context == context && r.command == line)
        .cloned()
        .unwrap_or(Row {
            context: context.to_string(),
            command: line,
            cat: Cat::Edit,
            keys: Vec::new(),
            preset_keys: Vec::new(),
            listed: false,
        })
}

/// Is the row of this macro one of the rows the macro group shows (and not a context's)?
pub(super) fn is_group_row(s: &Settings, r: &Row) -> bool {
    let Some(name) = xirius_prefs::macros::name_of_line(&r.command) else {
        return false;
    };
    s.macros.get(&name).is_some_and(|m| {
        macro_ext::context_of(&m.steps) == r.context && r.command == run_line(&name)
    })
}

/// Does the macro show under the search?
fn matches(ui: &SettingsUi, m: &Macro, r: &Row) -> bool {
    if ui.legend.is_some() {
        return false;
    }
    if ui.key_mode {
        return ui.q_key.as_ref().is_none_or(|q| r.keys.contains(q));
    }
    let q = ui.query.trim().to_lowercase();
    q.is_empty() || m.name.to_lowercase().contains(&q)
}

/// The group: its heading, one row for each macro, and the editor under the open one. Gives the
/// bottom and how many rows it listed.
#[allow(clippy::too_many_arguments)]
pub(super) fn group(
    b: &mut B,
    ui: &SettingsUi,
    s: &Settings,
    registry: &Registry,
    rows: &[Row],
    labels: &[String],
    issues: &[Issue],
    os: Os,
    x: f32,
    mut y: f32,
    w: f32,
) -> (f32, usize) {
    let c = b.c;
    let shown: Vec<(&Macro, Row)> = s
        .macros
        .list()
        .iter()
        .map(|m| (m, key_row(rows, m)))
        .filter(|(m, r)| matches(ui, m, r))
        .collect();
    let filtering = ui.legend.is_some()
        || (ui.key_mode && ui.q_key.is_some())
        || (!ui.key_mode && !ui.query.trim().is_empty());
    if shown.is_empty() && filtering {
        return (y, 0);
    }
    y += 18.0 - 4.0;
    let hw = b.text(
        "マクロ",
        FontSize::Caption,
        Weight::Semibold,
        c.ink,
        Rect::new(x + 6.0, y, w, 18.0),
        Align::Left,
    );
    b.digits(
        &shown.len().to_string(),
        FontSize::Micro,
        Weight::Regular,
        c.ink,
        Rect::new(x + 6.0 + hw + 8.0, y + 1.0, 40.0, 18.0),
        Align::Left,
    );
    parts::icon_button(
        b,
        Rect::new(x + w - 4.0 - 22.0, y - 2.0, 22.0, 22.0),
        "plus",
        Do::MacroNew,
        Some("マクロを作成"),
    );
    y += 18.0 + 4.0;
    let n = shown.len();
    for (m, r) in shown {
        let label = m.name.clone();
        let mac = MacroRow {
            name: &m.name,
            count: m.steps.len(),
        };
        y = keys::row(
            b,
            ui,
            &r,
            &label,
            rows,
            labels,
            issues,
            os,
            x,
            y,
            w,
            Some(&mac),
        );
        if ui.macro_open.as_deref() == Some(m.name.as_str()) {
            y = editor(b, ui, s, registry, m, x + 20.0, y + 2.0, w - 20.0) + 6.0;
        }
    }
    (y, n)
}

/// A one-line text box; the one that has the keyboard shows its caret.
fn input(b: &mut B, ui: &SettingsUi, r: Rect, id: FieldId, text: &str, placeholder: &str) {
    let c = b.c;
    let typing = ui.field.as_ref().filter(|f| f.id == id);
    if typing.is_some() {
        b.ring(r, c.ring, c.paper, Radius::S, 1.5);
    } else {
        b.round(r, c.paper, Radius::S);
    }
    b.item(r, Do::Field(id), None);
    let tr = Rect::new(r.x + 8.0, r.y, r.w - 16.0, r.h);
    b.push_mask(tr);
    match typing {
        Some(f) => {
            let t = f.text();
            let tw = if t.is_empty() {
                b.text(
                    placeholder,
                    FontSize::Small,
                    Weight::Regular,
                    c.ink,
                    tr,
                    Align::Left,
                );
                0.0
            } else {
                b.text(
                    &t,
                    FontSize::Small,
                    Weight::Regular,
                    c.ink_strong,
                    tr,
                    Align::Left,
                )
            };
            b.fill(
                Rect::new(tr.x + tw + 1.0, r.y + 7.0, 1.5, r.h - 14.0),
                c.ink_strong,
            );
            b.scene.field = Some(Rect::new(tr.x, r.y, tw.max(1.0), r.h));
        }
        None if text.is_empty() => {
            b.text(
                placeholder,
                FontSize::Small,
                Weight::Regular,
                c.ink,
                tr,
                Align::Left,
            );
        }
        None => {
            b.text(
                text,
                FontSize::Small,
                Weight::Regular,
                c.ink_strong,
                tr,
                Align::Left,
            );
        }
    }
    b.pop_mask();
}

/// The editor of one macro: its name, its steps and the buttons. Gives its bottom.
#[allow(clippy::too_many_arguments)]
fn editor(
    b: &mut B,
    ui: &SettingsUi,
    s: &Settings,
    registry: &Registry,
    m: &Macro,
    x: f32,
    y: f32,
    w: f32,
) -> f32 {
    let c = b.c;
    let fc = super::faces(c);
    let cands = macro_ext::candidates(registry);
    let query = ui
        .field
        .as_ref()
        .filter(|f| f.id == FieldId::MacroPick)
        .map(|f| f.text())
        .unwrap_or_default();
    let list = if ui.macro_pick {
        pick_list(&cands, &query, macro_ext::context_of(&m.steps))
    } else {
        Vec::new()
    };
    let n = m.steps.len();
    let steps_h = if n == 0 {
        0.0
    } else {
        n as f32 * STEP_H + (n - 1) as f32 * 2.0
    };
    let listed = list.len().min(PICK_ROWS);
    let pick_h = if ui.macro_pick {
        8.0 + INPUT_H
            + 4.0
            + listed as f32 * INPUT_H
            + if list.is_empty() || list.len() > PICK_ROWS {
                22.0
            } else {
                0.0
            }
    } else {
        0.0
    };
    let h = PAD + INPUT_H + 8.0 + steps_h + if n == 0 { 0.0 } else { 8.0 } + INPUT_H + pick_h + PAD;
    let panel = Rect::new(x, y, w, h);
    b.round(panel, fc.well, Radius::M);
    b.item(panel, Do::Nothing, None);
    let ix = x + PAD;
    let iw = w - PAD * 2.0;
    let mut cy = y + PAD;
    // The name, and 削除 at its end.
    b.text(
        "名前",
        FontSize::Small,
        Weight::Regular,
        c.ink,
        Rect::new(ix, cy, 36.0, INPUT_H),
        Align::Left,
    );
    let del = Rect::new(ix + iw - 24.0, cy + 3.0, 22.0, 22.0);
    parts::icon_button(
        b,
        del,
        "trash",
        Do::MacroDelete(m.name.clone()),
        Some("マクロを削除"),
    );
    input(
        b,
        ui,
        Rect::new(ix + 46.0, cy, iw - 46.0 - 30.0, INPUT_H),
        FieldId::MacroName,
        &m.name,
        "名前",
    );
    cy += INPUT_H + 8.0;
    // The steps, in order.
    for (i, line) in m.steps.iter().enumerate() {
        let r = Rect::new(ix, cy, iw, STEP_H);
        b.round(r, c.surface_1, Radius::S);
        b.digits(
            &(i + 1).to_string(),
            FontSize::Caption,
            Weight::Regular,
            c.ink,
            Rect::new(r.x + 4.0, r.y, 22.0, r.h),
            Align::Right,
        );
        let mut ex = r.right() - 6.0;
        parts::icon_button(
            b,
            Rect::new(ex - 22.0, r.y + 6.0, 22.0, 22.0),
            "close",
            Do::MacroRemove(i),
            Some("削除"),
        );
        ex -= 24.0;
        if i + 1 < n {
            parts::icon_button(
                b,
                Rect::new(ex - 22.0, r.y + 6.0, 22.0, 22.0),
                "chevron-down",
                Do::MacroMove(i, true),
                Some("下へ"),
            );
        }
        ex -= 24.0;
        if i > 0 {
            parts::icon_button(
                b,
                Rect::new(ex - 22.0, r.y + 6.0, 22.0, 22.0),
                "chevron-up",
                Do::MacroMove(i, false),
                Some("上へ"),
            );
        }
        ex -= 24.0;
        let tr = Rect::new(r.x + 34.0, r.y, (ex - r.x - 34.0).max(0.0), r.h);
        b.push_mask(tr);
        b.text(
            &ext::command_label(line, registry),
            FontSize::Small,
            Weight::Regular,
            c.ink_strong,
            tr,
            Align::Left,
        );
        b.pop_mask();
        cy += STEP_H + 2.0;
    }
    if n > 0 {
        cy += 6.0;
    }
    // コマンドを追加 and 実行.
    let add = Rect::new(ix, cy, parts::button_w("コマンドを追加"), INPUT_H);
    parts::button(b, add, "plus", "コマンドを追加", Do::MacroPick);
    let run_w = parts::button_w("実行");
    let run = Rect::new(ix + iw - run_w, cy, run_w, INPUT_H);
    let can = n > 0;
    let face = if !can {
        c.surface_3
    } else if b.hot(run) {
        c.fill_hover
    } else {
        c.fill
    };
    b.round(run, face, Radius::S);
    let ink = if can { c.on_fill } else { c.ink };
    b.icon("play", parts::small_icon_at(run.x + 10.0, run), ink);
    b.text(
        "実行",
        FontSize::Small,
        Weight::Regular,
        ink,
        Rect::new(run.x + 30.0, run.y, run.w - 30.0, run.h),
        Align::Left,
    );
    if can {
        b.item(run, Do::MacroRun(m.name.clone()), None);
    }
    cy += INPUT_H;
    // The picker of commands.
    if ui.macro_pick {
        cy += 8.0;
        input(
            b,
            ui,
            Rect::new(ix, cy, iw, INPUT_H),
            FieldId::MacroPick,
            "",
            "コマンドを検索",
        );
        cy += INPUT_H + 4.0;
        for cand in list.iter().take(PICK_ROWS) {
            let r = Rect::new(ix, cy, iw, INPUT_H);
            if b.hot(r) {
                b.round(r, c.hover, Radius::S);
            }
            let hint = s
                .keymap
                .keys_of(cand.context, cand.command)
                .first()
                .map(|k| k.display(Os::current()))
                .unwrap_or_default();
            let ctx = ctx_label(cand.context);
            let cw = scene::width(ctx, FontSize::Caption);
            let hw = scene::width(&hint, FontSize::Caption);
            let right = r.right() - 8.0;
            if !hint.is_empty() {
                b.text(
                    &hint,
                    FontSize::Caption,
                    Weight::Regular,
                    c.ink,
                    Rect::new(right - hw, r.y, hw + 2.0, r.h),
                    Align::Left,
                );
            }
            let cx = right - hw - if hint.is_empty() { 0.0 } else { 12.0 } - cw;
            b.text(
                ctx,
                FontSize::Caption,
                Weight::Regular,
                c.ink,
                Rect::new(cx, r.y, cw + 2.0, r.h),
                Align::Left,
            );
            let tr = Rect::new(r.x + 8.0, r.y, (cx - r.x - 8.0 - 8.0).max(0.0), r.h);
            b.push_mask(tr);
            b.text(
                &cand.label,
                FontSize::Small,
                Weight::Regular,
                c.ink_strong,
                tr,
                Align::Left,
            );
            b.pop_mask();
            b.item(r, Do::MacroAdd(cand.command.to_string()), None);
            cy += INPUT_H;
        }
        if list.is_empty() {
            b.text(
                "見つかりません",
                FontSize::Caption,
                Weight::Regular,
                c.ink,
                Rect::new(ix + 8.0, cy, iw, 22.0),
                Align::Left,
            );
        } else if list.len() > PICK_ROWS {
            b.text(
                &format!("ほか {} 件", list.len() - PICK_ROWS),
                FontSize::Caption,
                Weight::Regular,
                c.ink,
                Rect::new(ix + 8.0, cy, iw, 22.0),
                Align::Left,
            );
        }
    }
    panel.bottom()
}
