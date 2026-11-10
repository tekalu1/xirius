//! PRJ-031: the UI owns both text routing and protection from external handoffs.
//! Add input owners here and in the interaction coverage test when adding a field.
use crate::{settings::field::Field, studio::Studio};
use std::ops::Range;
use xirius_design::{FontSize, Space, Weight};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImeTarget {
    Ask,
    Name,
    Pick,
    Settings,
    /// A value typed in a device's knob (UI-324).
    Device,
    /// A value typed in a track head's fader (UI-324).
    Head,
    /// A branch's level or pan typed on a strip or an editor's head (MIX-045).
    Branch,
    Palette,
    /// A version's or an export's name typed in its popup.
    Naming,
    /// The track browser's search box.
    Browser,
}

/// What a key did to the text field that has the keys.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TextKey {
    /// Not an editing key (or no field of text has the keys).
    Pass,
    Used,
    /// Ctrl+C / Ctrl+X: this goes to the clipboard.
    Copy(String),
    /// Ctrl+V: the screen gives the clipboard's text to [`Studio::text_paste`].
    Paste,
}

impl Studio {
    /// Runs `edit` on the field of text that has the keys and does what the owner does after
    /// a change (the palette's list, the menu's filter, a list's rows, the settings' query).
    /// `None` when none has them or the keys are a number's (a value typed in a knob or a
    /// menu's number field).
    pub(crate) fn edit_text<R>(&mut self, edit: impl FnOnce(&mut Field) -> R) -> Option<R> {
        let target = self.ime_target()?;
        match target {
            ImeTarget::Device | ImeTarget::Head | ImeTarget::Branch => None,
            ImeTarget::Ask => {
                if self.field_typing().is_some() {
                    return None;
                }
                let out = edit(self.ask_mut()?);
                self.filter_context();
                self.refresh_popup();
                Some(out)
            }
            ImeTarget::Name => Some(edit(&mut self.input_map.name.as_mut()?.field)),
            ImeTarget::Naming => Some(edit(self.naming_field_mut()?)),
            ImeTarget::Browser => {
                let out = edit(&mut self.browser.search);
                self.browser.scroll = 0.0;
                Some(out)
            }
            ImeTarget::Palette => {
                let mut out = None;
                self.palette.as_mut()?.edit(|f| out = Some(edit(f)));
                out
            }
            ImeTarget::Pick => {
                let mut out = None;
                self.pick_typing_mut()?.edit(|f| out = Some(edit(f)));
                self.pick_relayout();
                out
            }
            ImeTarget::Settings => {
                let ui = self.settings.as_mut()?;
                let f = ui.field.as_mut()?;
                let out = edit(f);
                if f.id == crate::settings::FieldId::Search {
                    ui.query = f.text();
                }
                Some(out)
            }
        }
    }

    /// The offset in the focused field's text nearest to the point `x`, when the press at
    /// (`x`, `y`) is on that field.
    pub(crate) fn text_index(
        &self,
        g: &crate::studio::Geometry,
        x: f32,
        y: f32,
        anywhere: bool,
    ) -> Option<usize> {
        let (r, size, weight) = self.text_place(g)?;
        let pad = Space::X2.px();
        let inside = x >= r.x - pad && x <= r.right() + pad && y >= r.y && y <= r.bottom();
        if !inside && !anywhere {
            return None;
        }
        let field = self.ime_field()?;
        let rel = x - r.x + field.scroll();
        Some(field.index_at(rel, |t| crate::fonts::measure(t, size, weight)))
    }

    /// A press on the text of the field that has the keys: the caret (Shift extends), a word
    /// (two presses), everything (three). True when it was one, so nothing under it acts.
    pub(crate) fn text_press(
        &mut self,
        g: &crate::studio::Geometry,
        x: f32,
        y: f32,
        clicks: u32,
        shift: bool,
    ) -> bool {
        let Some(at) = self.text_index(g, x, y, false) else {
            return false;
        };
        self.edit_text(|f| f.press(at, clicks as usize, shift));
        self.text_drag = true;
        true
    }

    /// The pointer moves with the button down after [`Studio::text_press`].
    pub(crate) fn text_drag_to(&mut self, g: &crate::studio::Geometry, x: f32, y: f32) -> bool {
        if !self.text_drag {
            return false;
        }
        if let Some(at) = self.text_index(g, x, y, true) {
            self.edit_text(|f| f.drag_to(at));
        }
        true
    }

    /// The editing keys the field that has the keys takes before anything else: Ctrl (Cmd)
    /// with A, Z, Y, C, X, V, Backspace, Delete and the arrows, and Shift with the arrows,
    /// Home and End. Plain keys stay with their owners.
    pub(crate) fn text_key(&mut self, key: &str, mods: crate::tools::Mods) -> TextKey {
        let ctrl = mods.ctrl && !mods.alt;
        let editing = if ctrl {
            matches!(
                key,
                "a" | "z"
                    | "y"
                    | "c"
                    | "x"
                    | "v"
                    | "backspace"
                    | "delete"
                    | "left"
                    | "right"
                    | "home"
                    | "end"
            )
        } else {
            mods.shift && !mods.alt && matches!(key, "left" | "right" | "home" | "end")
        };
        if !editing || self.ime_field().is_none() {
            return TextKey::Pass;
        }
        let Some(keyed) = self.edit_text(|f| {
            if f.marked.is_empty() {
                f.key(key, mods.shift, ctrl)
            } else {
                // The composition's keys are the IME's own.
                crate::settings::field::Keyed::Used
            }
        }) else {
            return TextKey::Pass;
        };
        use crate::settings::field::Keyed;
        match keyed {
            Keyed::Pass => TextKey::Pass,
            Keyed::Used => TextKey::Used,
            Keyed::Copy(t) | Keyed::Cut(t) => TextKey::Copy(t),
            Keyed::Paste => TextKey::Paste,
        }
    }

    /// Ctrl+V: the clipboard's text goes into the field that has the keys.
    pub(crate) fn text_paste(&mut self, text: &str) {
        let line = text.lines().collect::<Vec<_>>().join(" ");
        self.edit_text(|f| f.paste(line.trim()));
    }

    /// Both the main and detached GPUI text handlers use this priority.
    /// Merely opening a field owns it,
    /// including an empty field or an unfinished IME composition.
    pub(crate) fn ime_target(&self) -> Option<ImeTarget> {
        if self.instruments.own_surface() {
            return if self.ask_field().is_some() {
                Some(ImeTarget::Ask)
            } else if self.popup.as_ref().is_some_and(|p| p.pick.is_some()) {
                Some(ImeTarget::Pick)
            } else if self.instruments.entry.as_ref().is_some_and(|e| {
                Some(e.plugin) == self.instruments.detached_target
                    || (self.instruments.detached_channel
                        && self.channel.as_ref().is_some_and(|c| {
                            self.project()
                                .tracks
                                .get(&c.track)
                                .is_some_and(|t| t.inserts.iter().any(|p| p.id == e.plugin))
                        }))
            }) {
                Some(ImeTarget::Device)
            } else if self.instruments.detached_channel
                && self
                    .head_fader
                    .entry
                    .as_ref()
                    .is_some_and(|e| self.channel.as_ref().is_some_and(|c| c.track == e.track))
            {
                Some(ImeTarget::Head)
            } else if self.instruments.detached_channel
                && self.mixer.value_entry.as_ref().is_some_and(|e| {
                    self.channel
                        .as_ref()
                        .is_some_and(|c| c.track == e.focus.track)
                        && e.focus.place == crate::branch_value::Place::Editor
                })
            {
                Some(ImeTarget::Branch)
            } else {
                None
            };
        }
        if self.ask_field().is_some() {
            Some(ImeTarget::Ask)
        } else if self.input_map.name.is_some() {
            Some(ImeTarget::Name)
        } else if self.naming_field().is_some() {
            Some(ImeTarget::Naming)
        } else if self.pick_typing().is_some() {
            Some(ImeTarget::Pick)
        } else if self.settings.as_ref().is_some_and(|s| s.field.is_some()) {
            Some(ImeTarget::Settings)
        } else if self.instruments.entry.is_some() {
            Some(ImeTarget::Device)
        } else if self.head_fader.entry.is_some() {
            Some(ImeTarget::Head)
        } else if self.mixer.value_entry.is_some() {
            Some(ImeTarget::Branch)
        } else if self.palette.is_some() {
            Some(ImeTarget::Palette)
        } else if self.browser.typing {
            Some(ImeTarget::Browser)
        } else {
            None
        }
    }

    /// Where the text of the field that has the keys is drawn (body coordinates), and in which
    /// size and weight: what both the IME's window and the mouse measure by.
    pub(crate) fn text_place(
        &self,
        g: &crate::studio::Geometry,
    ) -> Option<(crate::studio::Rect, FontSize, Weight)> {
        let small = |r| Some((r, FontSize::Small, Weight::Regular));
        match self.ime_target()? {
            ImeTarget::Pick => {
                // The search box is the list's first cell (UI-338).
                let home = self.home_typing().and_then(|_| self.home_layout(g)?.search);
                let at = match self.popup.as_ref().filter(|p| p.pick.is_some()) {
                    Some(p) => p.layout(g.size).cells.first()?.text_room(),
                    None if home.is_some() => {
                        let h = home?;
                        crate::search_box::text_rect(h, Space::X2.px())
                    }
                    None => self
                        .settings
                        .as_ref()?
                        .menu
                        .as_ref()?
                        .menu
                        .layout
                        .cells
                        .first()?
                        .text_room(),
                };
                small(at)
            }
            ImeTarget::Ask => {
                let layout = self.popup.as_ref()?.layout(g.size);
                let cell = layout
                    .cells
                    .iter()
                    .find(|c| c.look == crate::popup::Look::Input && c.on)
                    .or_else(|| layout.cells.first())?;
                small(cell.text_room())
            }
            ImeTarget::Naming => {
                let layout = self.popup.as_ref()?.layout(g.size);
                small(layout.cells.iter().find(|c| c.field.is_some())?.text_room())
            }
            ImeTarget::Name | ImeTarget::Settings | ImeTarget::Head => small(
                self.name_rect(g)
                    .or_else(|| self.head_fader_field_rect(g))
                    .or_else(|| self.settings_scene(g).field)?,
            ),
            ImeTarget::Palette => Some((
                crate::palette::field_of(self.palette.as_ref()?.layout(g.size).search),
                FontSize::Lead,
                Weight::Regular,
            )),
            ImeTarget::Browser => small(crate::browser::search_text_rect(
                self.browser.layout(g.panel).search,
            )),
            ImeTarget::Device | ImeTarget::Branch => None,
        }
    }

    /// The one policy for MCP handoffs, first drafts and automatic external reloads.
    /// Persistent selection/focus on a track is not an unfinished interaction.
    pub(crate) fn human_interaction_active(&self) -> bool {
        // The start screen's search waits for keys from the start; only what is typed in it
        // is someone's unfinished work (UI-350). Any other field the IME writes in is.
        let idle_search = self.ime_target() == Some(ImeTarget::Pick) && self.home_typing_idle();
        (self.ime_target().is_some() && !idle_search)
            || self.browser.typing
            || self.inspector.field.is_some()
            || self.bar_edit.active()
            || self.rec_panel.focus.is_some()
            || self.palette.is_some()
            || self.popup.is_some()
            || self.settings.is_some()
            || self.unsaved.is_some()
            || self.recovery.is_some()
            || self.file_request.is_some()
            || self.view_read.picker.is_some()
            || !self.instruments.open.is_empty()
            // The CE's own window keeps its popup between callbacks in its GPUI view.
            || self.channel.is_some()
            || !self.mixer.editors.is_empty()
            || self.mixer.preset_panel.is_some()
            || self.pointer_held
            || self.drop_over
            || self.arrange.drag.is_some()
            || self.roll.drag.is_some()
            || self.browser.drag.is_some()
            || self.browser.file_drag.is_some()
            || self.mixer.drag.is_some()
            || self.mixer.bar_grab.is_some()
            || self.mixer.fx_drag.is_some()
            || self.mixer.name_drag.is_some()
            || self.mixer.insert_drag.is_some()
            || self.drawer_size.drag.is_some()
            || self.side_size.drag.is_some()
            || self.input_map.zoom_box.is_some()
            || self.input_map.middle_origin.is_some()
            || self.pan.is_some()
            || self.ruler_grab.is_some()
            || self.wheel_running
            || !self.pc_keys.down.is_empty()
            || !self.pc_keys.pc_waiting.is_empty()
            || self.pc_keys.roll.is_some()
            || self.pc_keys.waiting.is_some()
            || self.pc_keys.listening.is_some()
    }

    /// Automatic song replacement also waits for work and visible warnings.
    /// A save failure notification must still allow an explicit MCP retry.
    pub(crate) fn other_busy(&self) -> bool {
        self.human_interaction_active()
            || self.saving.is_some()
            || self.opening.is_some()
            || self.open_closing.is_some()
            || self.transport.recording
            || self.transport.take.is_some()
            || self.export.running()
            || self.sound_preparing()
            || self.toast_shown().is_some_and(|t| t.alert)
    }

    pub(crate) fn ime_field(&self) -> Option<&Field> {
        match self.ime_target()? {
            ImeTarget::Ask => self.ask_field(),
            ImeTarget::Name => self.input_map.name.as_ref().map(|n| &n.field),
            ImeTarget::Settings => self.settings.as_ref()?.field.as_ref(),
            ImeTarget::Device => self.instruments.entry.as_ref().map(|e| &e.field),
            ImeTarget::Head => self.head_fader.entry.as_ref().map(|e| &e.field),
            ImeTarget::Branch => self.mixer.value_entry.as_ref().map(|e| &e.field),
            ImeTarget::Pick => self.pick_typing().map(|p| &p.field),
            ImeTarget::Palette => self.palette.as_ref().map(|p| &p.field),
            ImeTarget::Naming => self.naming_field(),
            ImeTarget::Browser => Some(&self.browser.search),
        }
    }

    /// Does a list's search box have the keys?
    pub(crate) fn ime_picking(&self) -> bool {
        self.ime_target() == Some(ImeTarget::Pick)
    }

    /// The name being typed in a version or export popup (a value box is a number, not this).
    pub(crate) fn naming_field(&self) -> Option<&Field> {
        let p = self.popup.as_ref()?;
        matches!(
            p.kind,
            crate::popup::PopupKind::Versions | crate::popup::PopupKind::Export
        )
        .then_some(p.naming.as_ref())
        .flatten()
    }

    fn naming_field_mut(&mut self) -> Option<&mut Field> {
        self.naming_field()?;
        self.popup.as_mut()?.naming.as_mut()
    }

    pub(crate) fn ime_replace(&mut self, range: Option<Range<usize>>, text: &str, marked: bool) {
        match self.ime_target() {
            Some(ImeTarget::Ask) => self.ask_text(range, text, marked),
            Some(ImeTarget::Pick) => {
                self.pick_text(range, text, marked);
            }
            Some(ImeTarget::Name) => {
                let field = &mut self.input_map.name.as_mut().unwrap().field;
                if marked {
                    field.mark_text(range, text);
                } else {
                    field.replace_text(range, text);
                }
            }
            Some(ImeTarget::Settings) => self.settings_text(range, text, marked),
            Some(ImeTarget::Device) => {
                let entry = self.instruments.entry.as_mut().unwrap();
                entry.type_text(range, text, marked);
            }
            Some(ImeTarget::Head) => {
                let entry = self.head_fader.entry.as_mut().unwrap();
                entry.type_text(range, text, marked);
            }
            Some(ImeTarget::Branch) => {
                let entry = self.mixer.value_entry.as_mut().unwrap();
                entry.type_text(range, text, marked);
            }
            Some(ImeTarget::Palette) => {
                let palette = self.palette.as_mut().unwrap();
                if marked {
                    palette.mark_text(range, text);
                } else {
                    palette.replace_text(range, text);
                }
            }
            Some(ImeTarget::Naming) => {
                if let Some(field) = self.naming_field_mut() {
                    if marked {
                        field.mark_text(range, text);
                    } else {
                        field.replace_text(range, text);
                    }
                }
            }
            Some(ImeTarget::Browser) => {
                let field = &mut self.browser.search;
                if marked {
                    field.mark_text(range, text);
                } else {
                    field.replace_text(range, text);
                }
                self.browser.scroll = 0.0;
            }
            None => {}
        }
    }

    pub(crate) fn ime_unmark(&mut self) {
        match self.ime_target() {
            Some(ImeTarget::Ask) => self.ask_unmark(),
            Some(ImeTarget::Pick) => {
                self.pick_unmark();
            }
            Some(ImeTarget::Name) => self.input_map.name.as_mut().unwrap().field.unmark(),
            Some(ImeTarget::Settings) => self.settings_unmark(),
            Some(ImeTarget::Device) => self.instruments.entry.as_mut().unwrap().field.unmark(),
            Some(ImeTarget::Head) => self.head_fader.entry.as_mut().unwrap().field.unmark(),
            Some(ImeTarget::Branch) => self.mixer.value_entry.as_mut().unwrap().field.unmark(),
            Some(ImeTarget::Palette) => self.palette.as_mut().unwrap().unmark(),
            Some(ImeTarget::Naming) => {
                if let Some(field) = self.naming_field_mut() {
                    field.unmark();
                }
            }
            Some(ImeTarget::Browser) => self.browser.search.unmark(),
            None => {}
        }
    }
}
