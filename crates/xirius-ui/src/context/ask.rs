//! A menu row that asks for a name before it runs (名前を変更): the menu turns into one
//! input, the IME types into it as it does into the palette's field, Enter runs the
//! command with the name and Esc closes the menu.

use std::ops::Range;

use serde_json::Value;
use xirius_design::Layout;

use super::Run;
use crate::popup::{Cell, Look};
use crate::settings::FieldId;
use crate::settings::field::Field;
use crate::studio::{Rect, Studio};

/// The name being typed, and the command it goes to.
#[derive(Clone, Debug)]
pub struct Ask {
    pub name: &'static str,
    pub args: Value,
    /// The command's argument that takes the name.
    pub arg: &'static str,
    pub input: Field,
    /// The box takes a number.
    pub number: bool,
}

/// The one input of the menu, from its top-left corner.
pub fn cells(text: &str, w: f32) -> Vec<Cell> {
    let pad = Layout::POPUP_PAD;
    vec![
        Cell::new(Rect::new(pad, pad, w - pad * 2.0, 32.0), Look::Input)
            .text(text.to_string())
            .on(true),
    ]
}

impl Studio {
    /// Turns the open menu into the name box, with `initial` in it.
    pub(super) fn ask_open(&mut self, run: Run) {
        let (number, (name, args, arg, initial)) = match run {
            Run::Ask(n, a, g, i) => (false, (n, a, g, i)),
            Run::AskNumber(n, a, g, i) => (true, (n, a, g, i)),
            _ => return,
        };
        if let Some(st) = self.ctx.as_mut() {
            st.ask = Some(Ask {
                name,
                args,
                arg,
                input: Field::new(FieldId::Search, initial),
                number,
            });
        }
        self.refresh_popup();
    }

    /// The box, while the menu asks for a name.
    pub(crate) fn ask_field(&self) -> Option<&Field> {
        if !self.context_open() {
            return None;
        }
        let ctx = self.ctx.as_ref()?;
        ctx.ask
            .as_ref()
            .map(|a| &a.input)
            .or_else(|| ctx.search.as_ref().map(|s| &s.field))
    }

    fn ask_mut(&mut self) -> Option<&mut Field> {
        let ctx = self.ctx.as_mut()?;
        if let Some(a) = ctx.ask.as_mut() {
            Some(&mut a.input)
        } else {
            ctx.search.as_mut().map(|s| &mut s.field)
        }
    }

    /// Text from the IME (`marked`: still being composed). While a value's field has the keys,
    /// the field takes the composition and the digits; a letter goes to the search box instead
    /// (UI-313).
    pub(crate) fn ask_text(&mut self, range: Option<Range<usize>>, text: &str, marked: bool) {
        if self.field_typing().is_some() {
            if marked {
                if self.field_marked(text) {
                    return;
                }
            } else if self.field_text(text) {
                return;
            }
            // A space or the 「\n」 a simulated Enter types after it leaves both the field and the
            // search box as they were.
            if text.chars().all(char::is_whitespace) {
                return;
            }
            // A letter is the search box's: give it the keys, so Enter and the arrows work
            // there as before (UI-313).
            if text.chars().any(|c| {
                let c = super::fields::halfwidth(c);
                !c.is_whitespace() && !matches!(c, '0'..='9' | '.' | '-' | '+')
            }) {
                self.context_search();
            }
        }
        if let Some(f) = self.ask_mut() {
            if marked {
                f.mark_text(range, text);
            } else {
                f.replace_text(range, text);
            }
        }
        self.filter_context();
        self.refresh_popup();
    }

    /// Gives the keys to the menu's search box (it keeps the search box's composition clear).
    pub(crate) fn context_search(&mut self) {
        if let Some(st) = self.ctx.as_mut() {
            st.field = None;
            st.turn = super::fields::WheelTurn::default();
        }
        self.refresh_popup();
    }

    pub(crate) fn ask_unmark(&mut self) {
        if let Some(t) = self.ctx.as_mut().and_then(|s| s.field.as_mut()) {
            t.marked.clear();
        }
        if let Some(f) = self.ask_mut() {
            f.unmark();
        }
        self.refresh_popup();
    }

    /// A key while the menu asks for a name: every key is the box's own. None when it
    /// does not ask.
    pub(crate) fn ask_key(&mut self, key: &str) -> Option<bool> {
        self.ask_field()?;
        // While the IME is composing, its keys are its own.
        if self.ask_field().is_some_and(|f| !f.marked.is_empty()) {
            return Some(true);
        }
        match key {
            "escape" => {
                self.close_wheel_group();
                self.ctx = None;
                self.popup = None;
            }
            "enter" => self.ask_commit(),
            "backspace" => {
                if let Some(f) = self.ask_mut() {
                    f.backspace();
                }
                self.filter_context();
                self.refresh_popup();
            }
            _ => {}
        }
        Some(true)
    }

    /// Runs the command with what was typed; an empty box closes the menu, and a number
    /// the command refuses leaves the box open.
    fn ask_commit(&mut self) {
        let Some(ask) = self.ctx.as_ref().and_then(|s| s.ask.clone()) else {
            return;
        };
        let text = ask.input.value.trim().to_string();
        let value = if text.is_empty() {
            None
        } else if ask.number {
            match text.parse::<f64>() {
                // A whole number goes as an integer, for the commands that read an int.
                Ok(n) if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e12 => {
                    Some(Value::from(n as i64))
                }
                Ok(n) if n.is_finite() => Some(Value::from(n)),
                _ => return,
            }
        } else {
            Some(Value::String(text))
        };
        self.ctx = None;
        self.popup = None;
        let Some(value) = value else {
            return;
        };
        let mut args = ask.args;
        args[ask.arg] = value;
        // A name that starts with `ui.` is a change of the screen's own, not a command.
        if let Some(own) = ask.name.strip_prefix("ui.") {
            self.ask_own(own, &args);
            return;
        }
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        if self.arrange_run(ask.name, args.clone()).is_some()
            && ask.name == "note.value.set"
            && args["field"] == "velocity"
            && let Some(v) = args["value"].as_u64()
        {
            self.roll.next_vel = v as u8;
        }
    }

    /// The rename of a browser tab (`ui.place.rename`).
    fn ask_own(&mut self, name: &str, args: &Value) {
        if name != "place.rename" {
            return;
        }
        let (Some(i), Some(to)) = (args["index"].as_u64(), args["name"].as_str()) else {
            return;
        };
        match self.browser.rename_place(i as usize, to) {
            Ok(name) => self.tell(format!("場所のタブを「{name}」にしました")),
            Err(e) => self.error = Some(e),
        }
    }
}
