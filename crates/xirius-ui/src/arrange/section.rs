//! The section selected on the band, and what the keys do to it (ARR-002, ARR-004,
//! ARR-005).
//!
//! A press on a section selects it (the clips and the range let go, the playhead stays); a
//! press on the empty part of the band lets it go. Alt+D, Ctrl+C, Ctrl+X, Delete and P act
//! on it, and Ctrl+Alt+I / Ctrl+Alt+D put time in or take it out of the span it names.

use serde_json::{Value, json};
use xirius_core::id::SectionId;
use xirius_core::model::Section;
use xirius_core::time::Tick;

use crate::studio::{Geometry, Studio};

impl Studio {
    /// The selected section, when it is still in the song.
    pub(crate) fn selected_section(&self) -> Option<Section> {
        let id = self.arrange.section?;
        self.project().sections.iter().find(|s| s.id == id).cloned()
    }

    /// Selects a section: the clips and the range let go of their selection.
    pub(crate) fn select_section(&mut self, id: SectionId) {
        self.arrange.section = Some(id);
        self.arrange.clips.clear();
        self.arrange.range = None;
        self.arrange.ranges.clear();
        self.arrange.points.clear();
        self.arrange.touched();
    }

    /// Lets the section go.
    pub(crate) fn clear_section(&mut self) {
        if self.arrange.section.take().is_some() {
            self.arrange.touched();
        }
    }

    /// Selects the section a command made (`{"id": ...}`).
    pub(crate) fn select_section_made(&mut self, data: &Value) {
        if let Some(id) = data["id"].as_str().and_then(|s| s.parse().ok()) {
            self.select_section(id);
        }
    }

    /// The span the time keys act on: the range, else the selected section, else the loop.
    pub(crate) fn time_span(&self) -> Option<(Tick, Tick)> {
        if let Some((start, end)) = self.arrange.range
            && end > start
        {
            return Some((start, end));
        }
        if let Some(s) = self.selected_section() {
            return Some((s.start, s.start + s.length));
        }
        self.project()
            .loop_range
            .map(|l| (l.start, l.end))
            .filter(|(a, b)| b > a)
    }

    /// What a command that made or took a section leaves on the screen: the made section is
    /// the selected one; what was cut or deleted is not; what `select.in-section` chose
    /// becomes the clips selected.
    pub(crate) fn section_followed(&mut self, command: &str, data: &Value) {
        match command {
            "section.add" | "section.duplicate" | "section.paste" => {
                self.select_section_made(data);
            }
            "section.cut" | "section.delete" => self.clear_section(),
            "select.in-section" => {
                self.clear_section();
                self.take_clips();
            }
            _ => {}
        }
    }

    /// The keys that act on the selected section, and the time keys. `None` when the key is
    /// not one of them (or there is no section for it).
    pub(crate) fn section_command(&mut self, g: &Geometry, name: &str) -> Option<bool> {
        match name {
            "range.time.insert" | "range.time.delete" => {
                let Some((start, end)) = self.time_span() else {
                    return Some(false);
                };
                let ran = self
                    .arrange_run(name, json!({ "start": start, "end": end }))
                    .is_some();
                if ran {
                    // The song moved under the selection: it lets go of what it named.
                    self.arrange.range = None;
                    self.arrange.ranges.clear();
                }
                self.rebuild(g);
                Some(ran)
            }
            _ => {
                let s = self.selected_section().filter(|_| self.arrange.focus)?;
                let id = json!({ "id": s.id.to_string() });
                let command = match name {
                    "section.duplicate" => "section.duplicate",
                    "edit.copy" => "section.copy",
                    "edit.cut" => "section.cut",
                    "edit.delete" | "clip.delete" => "section.delete",
                    _ => return None,
                };
                let data = self.arrange_run(command, id);
                if let Some(data) = &data {
                    self.section_followed(command, data);
                }
                self.rebuild(g);
                Some(data.is_some())
            }
        }
    }
}
