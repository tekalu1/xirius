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
    pub preset: Option<PresetSave>,
    pub name: &'static str,
    pub args: Value,
    /// The command's argument that takes the name.
    pub arg: &'static str,
    pub input: Field,
    /// The box takes a number.
    pub number: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PresetSave {
    pub name: String,
    pub folder: String,
    pub folder_focus: bool,
    pub include_fx: bool,
    pub fx_editable: bool,
    pub fx_names: String,
    pub overwrite: bool,
    pub scope: String,
    pub folders: Vec<String>,
    pub folder_page: usize,
}

pub fn preset_cells(save: &PresetSave, text: &str) -> Vec<Cell> {
    use crate::popup::Action;
    let w = 280.0;
    let row = |y, label: &str, action| {
        Cell::new(Rect::new(12.0, y, w - 24.0, 32.0), Look::Row)
            .text(label.to_string())
            .act(Action::Ctx(action))
    };
    let mut cells = vec![
        Cell::new(Rect::new(12.0, 12.0, w - 24.0, 32.0), Look::Label).text(save.scope.clone()),
        Cell::new(Rect::new(12.0, 48.0, w - 24.0, 32.0), Look::Input)
            .text(if save.folder_focus {
                save.name.clone()
            } else {
                text.into()
            })
            .on(!save.folder_focus)
            .act(Action::Ctx(0)),
        {
            let c = Cell::new(Rect::new(12.0, 88.0, w - 24.0, 32.0), Look::Check)
                .text("FX を含める".to_string())
                .act(Action::Ctx(1))
                .on(save.include_fx);
            if save.fx_editable { c } else { c.off() }
        },
        Cell::new(Rect::new(12.0, 120.0, w - 24.0, 28.0), Look::Label).text(if save.include_fx {
            save.fx_names.clone()
        } else {
            String::new()
        }),
        Cell::new(Rect::new(12.0, 156.0, w - 24.0, 28.0), Look::Row)
            .text("Factory".to_string())
            .icon("chevron-right")
            .off(),
        row(184.0, "User", 5)
            .icon("chevron-down")
            .on(save.folder.is_empty()),
    ];
    let mut y = 216.0;
    for (i, folder) in save
        .folders
        .iter()
        .enumerate()
        .skip(save.folder_page)
        .take(6)
    {
        let depth = folder.split('/').count().saturating_sub(1) as f32;
        let left = 28.0 + depth * 10.0;
        cells.push(
            Cell::new(
                Rect::new(left, y, (w - left - 12.0).max(32.0), 28.0),
                Look::Row,
            )
            .text(folder.rsplit('/').next().unwrap_or(folder).to_string())
            .icon(
                if save
                    .folders
                    .iter()
                    .any(|f| f.starts_with(&format!("{folder}/")))
                {
                    "chevron-down"
                } else {
                    "folder"
                },
            )
            .on(save.folder == *folder)
            .act(Action::Ctx(10 + i)),
        );
        y += 28.0;
    }
    if save.folders.len() > 6 {
        let prev = Cell::new(Rect::new(12.0, y, 120.0, 28.0), Look::Button)
            .text("前のフォルダー")
            .act(Action::Ctx(6));
        cells.push(if save.folder_page == 0 {
            prev.off()
        } else {
            prev
        });
        let next = Cell::new(Rect::new(140.0, y, 128.0, 28.0), Look::Button)
            .text("次のフォルダー")
            .act(Action::Ctx(7));
        cells.push(if save.folder_page + 6 >= save.folders.len() {
            next.off()
        } else {
            next
        });
        y += 32.0;
    }
    y = y.max(300.0);
    if save.folder_focus {
        cells.push(
            Cell::new(Rect::new(12.0, y, w - 24.0, 28.0), Look::Input)
                .text(text.to_string())
                .on(true)
                .act(Action::Ctx(2)),
        );
    } else {
        cells.push(row(y, "新しいフォルダー…", 2));
    }
    y += 40.0;
    cells.push(
        Cell::new(Rect::new(80.0, y, 84.0, 28.0), Look::Button)
            .text("キャンセル".to_string())
            .act(Action::Ctx(4)),
    );
    cells.push(
        Cell::new(Rect::new(172.0, y, 96.0, 28.0), Look::Primary)
            .text(
                if save.overwrite {
                    "上書き保存"
                } else {
                    "保存"
                }
                .to_string(),
            )
            .act(Action::Ctx(3)),
    );
    cells
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
    pub(crate) fn open_track_preset_save(
        &mut self,
        command: &'static str,
        args: Value,
        initial: &str,
        scope: &str,
        include_fx: bool,
        fx_names: String,
    ) {
        self.ask_name(
            "プリセットを保存",
            command,
            args,
            "name",
            initial,
            (160.0, 100.0),
        );
        let folders = self
            .browser
            .track_library
            .as_ref()
            .into_iter()
            .flatten()
            .filter(|e| e.source == "User" && !e.folder.is_empty())
            .flat_map(|e| {
                (1..=e.folder.len())
                    .map(|n| e.folder[..n].join("/"))
                    .collect::<Vec<_>>()
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        if let Some(ask) = self.ctx.as_mut().and_then(|s| s.ask.as_mut()) {
            ask.preset = Some(PresetSave {
                name: initial.into(),
                folder: String::new(),
                folder_focus: false,
                include_fx,
                fx_editable: command == "browser.preset.save" && ask.args["kind"] == "instrument",
                fx_names,
                overwrite: false,
                scope: scope.into(),
                folders,
                folder_page: 0,
            });
        }
        self.refresh_popup();
    }

    pub(crate) fn preset_save_pick(&mut self, i: usize) -> bool {
        use xirius_core::track_browser::Category;
        let category = self
            .ctx
            .as_ref()
            .and_then(|s| s.ask.as_ref())
            .and_then(|a| {
                if a.name == "browser.preset.copy" {
                    return self
                        .browser
                        .track_library
                        .as_ref()?
                        .iter()
                        .find(|e| Some(e.id.as_str()) == a.args["entry"].as_str())
                        .map(|e| e.category);
                }
                Some(match a.args["kind"].as_str().unwrap_or("instrument") {
                    "instrument" | "instrument-fx" => Category::Instrument,
                    "multi-instrument" => Category::MultiInstrument,
                    "fx" => Category::Fx,
                    "fx-chain" => Category::FxChain,
                    "track" => Category::Track,
                    _ => return None,
                })
            });
        let device = self
            .ctx
            .as_ref()
            .and_then(|s| s.ask.as_ref())
            .and_then(|a| {
                if a.name == "browser.preset.copy" {
                    let id = a.args["entry"].as_str()?;
                    return self
                        .browser
                        .track_library
                        .as_ref()?
                        .iter()
                        .find(|e| e.id == id)
                        .map(|e| e.device.clone());
                }
                let id: xirius_core::id::PluginId = a.args["of"].as_str()?.parse().ok()?;
                self.project()
                    .tracks
                    .values()
                    .flat_map(|t| &t.inserts)
                    .find(|x| x.id == id)
                    .map(|x| x.builtin().map_or(x.uid.as_str(), |d| d.uid).to_owned())
            });
        let Some(ask) = self.ctx.as_mut().and_then(|s| s.ask.as_mut()) else {
            return false;
        };
        let Some(save) = ask.preset.as_mut() else {
            return false;
        };
        if save.folder_focus {
            save.folder = ask.input.value.clone();
        } else {
            save.name = ask.input.value.clone();
        }
        match i {
            0 | 2 => {
                save.folder_focus = i == 2;
                ask.input = Field::new(
                    FieldId::Search,
                    if save.folder_focus {
                        save.folder.clone()
                    } else {
                        save.name.clone()
                    },
                );
            }
            1 => {
                if !save.fx_editable {
                    return true;
                }
                save.include_fx = !save.include_fx;
                save.overwrite = false;
            }
            3 => {
                if save.name.trim().is_empty() {
                    return true;
                }
                let exists = self.browser.track_library.as_ref().is_some_and(|es| {
                    es.iter().any(|e| {
                        e.source == "User"
                            && category.is_none_or(|c| e.category == c)
                            && device.as_ref().is_none_or(|d| &e.device == d)
                            && e.name.eq_ignore_ascii_case(save.name.trim())
                            && e.folder.join("/") == save.folder.replace('\\', "/")
                    })
                });
                if exists && !save.overwrite {
                    save.overwrite = true;
                    self.refresh_popup();
                    return true;
                }
                let mut args = ask.args.clone();
                args["name"] = serde_json::json!(save.name.trim());
                args["folder"] = serde_json::json!(save.folder);
                args["overwrite"] = serde_json::json!(save.overwrite);
                if ask.name == "browser.preset.save" {
                    args["include_fx"] = serde_json::json!(save.include_fx);
                }
                let command = ask.name;
                self.ctx = None;
                self.popup = None;
                // design-lint: allow(ui-blocking-io) BRW-017: save/copy are handled by the song owner through io_command::handles.
                self.run_command(command, args);
                return true;
            }
            4 => {
                self.ctx = None;
                self.popup = None;
                return true;
            }
            5 => {
                save.folder.clear();
                save.folder_focus = false;
                ask.input = Field::new(FieldId::Search, save.name.clone());
                save.overwrite = false;
            }
            6 => save.folder_page = save.folder_page.saturating_sub(6),
            7 => {
                if save.folder_page + 6 < save.folders.len() {
                    save.folder_page += 6;
                }
            }
            n if n >= 10 => {
                if let Some(folder) = save.folders.get(n - 10) {
                    save.folder = folder.clone();
                }
                save.folder_focus = false;
                ask.input = Field::new(FieldId::Search, save.name.clone());
                save.overwrite = false;
            }
            _ => return false,
        }
        self.refresh_popup();
        true
    }

    /// Opens a menu that is only the name box at `at`: Enter runs `name` with what was
    /// typed as its `arg` (名前を付けて保存…, UI-332).
    pub(crate) fn ask_name(
        &mut self,
        label: &str,
        name: &'static str,
        args: Value,
        arg: &'static str,
        initial: &str,
        at: (f32, f32),
    ) {
        let item = super::Item::ask(label, name, args, arg, initial);
        let run = item.run.clone();
        self.open_context(vec![item], at.0, at.1);
        self.ask_open(run);
    }

    /// Turns the open menu into the name box, with `initial` in it.
    pub(super) fn ask_open(&mut self, run: Run) {
        let (number, (name, args, arg, initial)) = match run {
            Run::Ask(n, a, g, i) => (false, (n, a, g, i)),
            Run::AskNumber(n, a, g, i) => (true, (n, a, g, i)),
            _ => return,
        };
        if let Some(st) = self.ctx.as_mut() {
            st.ask = Some(Ask {
                preset: None,
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
        if self
            .ctx
            .as_ref()
            .is_some_and(|s| s.ask.is_none() && s.field.is_none() && !s.trail.is_empty())
        {
            self.context_restore(0);
        }
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
        if let Some(save) = self
            .ctx
            .as_mut()
            .and_then(|s| s.ask.as_mut())
            .and_then(|a| a.preset.as_mut())
        {
            save.overwrite = false;
        }
        self.filter_context();
        self.refresh_popup();
    }

    /// Gives the keys to the menu's search box (it keeps the search box's composition clear).
    pub(crate) fn context_search(&mut self) {
        if self.ctx.as_ref().is_some_and(|s| !s.trail.is_empty()) {
            self.context_restore(0);
        }
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
                if self.ctx.as_ref().is_some_and(|s| s.ask.is_none())
                    && let Some(search) = self.ctx.as_mut().and_then(|s| s.search.as_mut())
                    && !search.field.value.is_empty()
                {
                    search.field = Field::new(FieldId::Search, String::new());
                    self.filter_context();
                    self.refresh_popup();
                    return Some(true);
                }
                self.close_wheel_group();
                self.ctx = None;
                self.popup = None;
            }
            "enter" => {
                if self
                    .ctx
                    .as_ref()
                    .and_then(|s| s.ask.as_ref())
                    .is_some_and(|a| a.preset.is_some())
                {
                    self.preset_save_pick(3);
                } else {
                    self.ask_commit();
                }
            }
            "backspace" => {
                if let Some(f) = self.ask_mut() {
                    f.backspace();
                }
                if let Some(save) = self
                    .ctx
                    .as_mut()
                    .and_then(|s| s.ask.as_mut())
                    .and_then(|a| a.preset.as_mut())
                {
                    save.overwrite = false;
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

    /// The rename of a browser tab (`ui.place.rename`) and 名前を付けて保存… of a device's
    /// preset (`ui.preset.save`) or a track's chain (`ui.chain.save`), which ask before they
    /// write over a file.
    fn ask_own(&mut self, name: &str, args: &Value) {
        match name {
            "place.rename" => {
                let (Some(i), Some(to)) = (args["index"].as_u64(), args["name"].as_str()) else {
                    return;
                };
                match self.browser.rename_place(i as usize, to) {
                    Ok(name) => self.tell(format!("場所のタブを「{name}」にしました")),
                    Err(e) => self.error = Some(e),
                }
            }
            "preset.save" => {
                let (Some(id), Some(to)) = (
                    args["id"].as_str().and_then(|s| s.parse().ok()),
                    args["name"].as_str(),
                ) else {
                    return;
                };
                self.save_preset_named(id, to, args["over"].as_str());
            }
            "chain.save" => {
                let (Some(t), Some(to)) = (
                    args["track"].as_str().and_then(|s| s.parse().ok()),
                    args["name"].as_str(),
                ) else {
                    return;
                };
                self.save_chain_preset_named(t, to, args["over"].as_str());
            }
            _ => {}
        }
    }
}
