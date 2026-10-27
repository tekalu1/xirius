//! PRJ-031: the UI owns both text routing and protection from external handoffs.
//! Add input owners here and in the interaction coverage test when adding a field.
use crate::{settings::field::Field, studio::Studio};
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImeTarget {
    Ask,
    Name,
    Pick,
    Settings,
    Palette,
}

impl Studio {
    /// The same priority used by GPUI's text handler. Merely opening a field owns it,
    /// including an empty field or an unfinished IME composition.
    pub(crate) fn ime_target(&self) -> Option<ImeTarget> {
        if self.ask_field().is_some() {
            Some(ImeTarget::Ask)
        } else if self.input_map.name.is_some() {
            Some(ImeTarget::Name)
        } else if self.pick_typing().is_some() {
            Some(ImeTarget::Pick)
        } else if self.settings.as_ref().is_some_and(|s| s.field.is_some()) {
            Some(ImeTarget::Settings)
        } else if self.palette.is_some() {
            Some(ImeTarget::Palette)
        } else {
            None
        }
    }

    /// The one policy for MCP handoffs, first drafts and automatic external reloads.
    /// Persistent selection/focus on a track is not an unfinished interaction.
    pub(crate) fn human_interaction_active(&self) -> bool {
        self.ime_target().is_some()
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
            ImeTarget::Pick | ImeTarget::Palette => None,
        }
    }

    pub(crate) fn ime_pick_field(&self) -> Option<Field> {
        (self.ime_target() == Some(ImeTarget::Pick))
            .then(|| self.pick_typing().map(|p| p.field()))
            .flatten()
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
            Some(ImeTarget::Palette) => {
                let palette = self.palette.as_mut().unwrap();
                if marked {
                    palette.mark_text(range, text);
                } else {
                    palette.replace_text(range, text);
                }
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
            Some(ImeTarget::Palette) => self.palette.as_mut().unwrap().unmark(),
            None => {}
        }
    }
}
