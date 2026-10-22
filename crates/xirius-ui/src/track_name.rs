//! Inline track names own their text while the IME is editing it (KEY-002).
use crate::draw::{Align, Pen};
use crate::settings::{Field, FieldId};
use crate::studio::{Geometry, Hit, Rect, Studio};
use crate::tools::Mods;
use crate::tree::RowKind;
use serde_json::json;
use xirius_core::id::TrackId;
use xirius_design::{FontSize, Radius, Weight};

/// Where a track's name text sits in its row (row-local): a double click on it renames the
/// track. The width is measured the way the browser's buttons are placed, with a little room.
pub fn name_text(row: &crate::tree::Row, name: &str, mark: &crate::draw::MarkChoice) -> Rect {
    let area = crate::draw::track_name_rect(row, mark);
    let w = (crate::draw::estimated_text_width(name, FontSize::Body) + 6.0).min(area.w);
    Rect::new(area.x, area.y, w.max(8.0), area.h)
}

pub struct Name {
    pub track: TrackId,
    pub field: Field,
}

impl Studio {
    pub fn name_rect(&self, g: &Geometry) -> Option<Rect> {
        let id = self.input_map.name.as_ref()?.track;
        let row = self.tree().row_of(RowKind::Track(id))?;
        let p = self.project();
        let track = p.tracks.get(&id)?;
        let mark = self.row_mark(row, track);
        let area = crate::draw::track_name_rect(row, &mark);
        Some(Rect::new(
            g.column.x + area.x,
            g.column.y + row.y - self.scroll.1 + area.y,
            area.w.max(24.0),
            22.0,
        ))
    }

    pub fn name_press(&mut self, g: &Geometry, x: f32, y: f32, clicks: u32) -> bool {
        if self.input_map.name.is_some() {
            if self.name_rect(g).is_some_and(|r| r.contains(x, y)) {
                return true;
            }
            self.name_key("enter", Mods::NONE);
        }
        if clicks < 2 || !g.column.contains(x, y) {
            return false;
        }
        let t = match self.hit(g, x, y) {
            Hit::Row(RowKind::Track(t)) | Hit::TrackMark(t) => t,
            _ => return false,
        };
        let Some(row) = self.tree().row_of(RowKind::Track(t)) else {
            return false;
        };
        let top = g.column.y + row.y - self.scroll.1;
        let p = self.project();
        let track = &p.tracks[&t];
        let name = track.name.clone();
        let mark = self.row_mark(row, track);
        if !name_text(row, &name, &mark).contains(x - g.column.x, y - top) {
            // The rest of the head is the channel's: the mixer opens on it.
            self.field_blur();
            return self.run_binding(g, &format!("mixer.focus --id {t}"));
        }
        self.select_row(Some(RowKind::Track(t)));
        self.input_map.name = Some(Name {
            track: t,
            field: Field::new(FieldId::Search, self.project().tracks[&t].name.clone()),
        });
        true
    }

    pub fn name_key(&mut self, key: &str, _mods: Mods) -> bool {
        let Some(name) = &mut self.input_map.name else {
            return false;
        };
        if !name.field.marked.is_empty() {
            return true;
        }
        match key {
            "escape" => self.input_map.name = None,
            "backspace" => name.field.backspace(),
            "enter" | "tab" => {
                let name = self.input_map.name.take().expect("editing");
                if !name.field.value.trim().is_empty() {
                    self.run_command(
                        "track.rename",
                        json!({"id":name.track.to_string(),"name":name.field.value.trim()}),
                    );
                }
            }
            _ => {}
        }
        true
    }
}

pub fn paint(pen: &mut Pen, s: &Studio, g: &Geometry) {
    let Some(r) = s.name_rect(g) else {
        return;
    };
    let name = s.input_map.name.as_ref().expect("editing");
    pen.round(r, pen.c.surface_3, Radius::S);
    let width = pen.text(
        &name.field.text(),
        FontSize::Small,
        Weight::Regular,
        pen.c.ink_strong,
        r,
        Align::Left,
    );
    pen.fill(
        Rect::new(
            (r.x + width + 1.0).min(r.right() - 2.0),
            r.y + 4.0,
            1.5,
            r.h - 8.0,
        ),
        pen.c.ink_strong,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renaming(s: &mut Studio, track_name: &str) {
        let track = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == track_name)
            .unwrap_or_else(|| panic!("no track {track_name}"))
            .id;
        s.input_map.name = Some(Name {
            track,
            field: Field::new(FieldId::Search, track_name.into()),
        });
    }

    /// The name's drawn area and its hit area leave the mark its place, and the mark stays
    /// inside the head's right buttons (UI-312).
    #[test]
    fn the_name_area_and_its_hit_leave_the_mark_its_room() {
        let mut s = Studio::sample();
        let t = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Keys")
            .unwrap()
            .id;
        s.song
            .run(
                "track.transpose",
                serde_json::json!({"ids": [t.to_string()], "semis": 12}),
            )
            .unwrap();
        s.song
            .run(
                "track.velocity",
                serde_json::json!({"ids": [t.to_string()], "percent": 10}),
            )
            .unwrap();
        // A long name with both values large: the name keeps its 3 characters, the mark turns
        // into the dot, and neither reaches the right buttons (UI-312).
        s.song
            .run(
                "track.rename",
                serde_json::json!({"id": t.to_string(), "name": "ピアノ旋律"}),
            )
            .unwrap();
        s.song
            .run(
                "track.transpose",
                serde_json::json!({"ids": [t.to_string()], "semis": -64}),
            )
            .unwrap();
        s.song
            .run(
                "track.velocity",
                serde_json::json!({"ids": [t.to_string()], "percent": 100}),
            )
            .unwrap();
        let row = *s.tree().row_of(RowKind::Track(t)).expect("the row");
        let p = s.project();
        let track = &p.tracks[&t];
        let mark = s.row_mark(&row, track);
        assert_eq!(
            mark.text.as_deref(),
            Some("−64"),
            "the one-value form fits, the whole one does not"
        );
        let area = crate::draw::track_name_rect(&row, &mark);
        assert!(
            area.w >= crate::draw::min_name_width(),
            "the name keeps its 3 characters: {area:?}"
        );
        let pill = crate::draw::track_mark_rect(&row, &mark);
        assert!(pill.x >= area.right(), "{pill:?} after {area:?}");
        assert!(
            pill.right() <= crate::studio::RowParts::of(&row).name_right + 0.01,
            "{pill:?} inside the buttons"
        );
        // The name's hit area starts and ends where it is drawn.
        let hit = name_text(&row, &track.name, &mark);
        assert_eq!(hit.x, area.x);
        assert!(hit.right() <= area.right() + 0.01);
        // The field that types the name stops where the mark starts (UI-312).
        let field = Name {
            track: t,
            field: Field::new(FieldId::Search, track.name.clone()),
        };
        s.input_map.name = Some(field);
        let g = s.geometry(1440.0, 860.0);
        let rect = s.name_rect(&g).expect("the field");
        assert!(
            rect.right()
                <= g.column.x + crate::studio::RowParts::of(&row).name_right - mark.width + 0.01,
            "the field ends before the mark: {rect:?}"
        );

        // A name longer than the whole head still leaves the mark inside the buttons (UI-312).
        s.song
            .run(
                "track.rename",
                serde_json::json!({"id": t.to_string(), "name": "とても長いピアノの旋律のトラック"}),
            )
            .unwrap();
        let p = s.project();
        let track = &p.tracks[&t];
        let mark = s.row_mark(&row, track);
        let area = crate::draw::track_name_rect(&row, &mark);
        assert!(
            area.w >= crate::draw::min_name_width(),
            "the long name still keeps 3 characters: {area:?}"
        );
        let pill = crate::draw::track_mark_rect(&row, &mark);
        assert!(
            pill.x >= area.right(),
            "the name and the mark do not overlap: {pill:?} {area:?}"
        );
        assert!(
            pill.right() <= crate::studio::RowParts::of(&row).name_right + 0.01,
            "{pill:?} inside the buttons"
        );

        // A short name and a short value keep the whole mark.
        s.song
            .run(
                "track.rename",
                serde_json::json!({"id": t.to_string(), "name": "Bass"}),
            )
            .unwrap();
        s.song
            .run(
                "track.velocity",
                serde_json::json!({"ids": [t.to_string()], "percent": 0}),
            )
            .unwrap();
        s.song
            .run(
                "track.transpose",
                serde_json::json!({"ids": [t.to_string()], "semis": 2}),
            )
            .unwrap();
        let p = s.project();
        let track = &p.tracks[&t];
        let mark = s.row_mark(&row, track);
        assert_eq!(mark.text.as_deref(), Some("+2"), "the whole mark fits");
    }

    /// The widths a text system measures for "Keys" and "−12 +10%" (Close to Inter): with
    /// them the whole mark fits when its padding is tightened, so the MSI's in02 keeps the
    /// 「−12 +10%」 the mock shows (UI-312).
    #[test]
    fn keys_keeps_the_whole_mark_with_the_measured_widths() {
        let mut s = Studio::sample();
        let t = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Keys")
            .unwrap()
            .id;
        s.song
            .run(
                "track.transpose",
                serde_json::json!({"ids": [t.to_string()], "semis": -12}),
            )
            .unwrap();
        s.song
            .run(
                "track.velocity",
                serde_json::json!({"ids": [t.to_string()], "percent": 10}),
            )
            .unwrap();
        s.set_mark_widths(
            t,
            crate::draw::MarkWidths {
                name: 30.0,
                full: 45.2,
                short: 17.4,
            },
        );
        let row = *s.tree().row_of(RowKind::Track(t)).expect("the row");
        let p = s.project();
        let track = &p.tracks[&t];
        let mark = s.row_mark(&row, track);
        assert_eq!(
            mark.text.as_deref(),
            Some("−12 +10%"),
            "the whole mark fits with the tightened padding"
        );
        let area = crate::draw::track_name_rect(&row, &mark);
        assert!(
            area.w >= crate::draw::estimated_text_width(&track.name, FontSize::Body),
            "the short name still fits: {area:?}"
        );
        let pill = crate::draw::track_mark_rect(&row, &mark);
        assert!(pill.x >= area.right(), "{pill:?} {area:?}");
        assert!(pill.right() <= crate::studio::RowParts::of(&row).name_right + 0.01);
    }

    // UI-026 / KEY-002: what the IME composes goes to the name field, no key does
    // anything while it composes, and Enter after the commit renames the track.
    #[test]
    fn the_ime_composes_and_enters_a_japanese_track_name() {
        let mut s = Studio::sample();
        renaming(&mut s, "Keys");
        let track = s.input_map.name.as_ref().unwrap().track;
        for _ in 0..4 {
            s.name_key("backspace", Mods::NONE);
        }
        s.input_map
            .name
            .as_mut()
            .unwrap()
            .field
            .mark_text(None, "ぴあの");
        assert!(s.name_key("enter", Mods::NONE));
        assert_eq!(
            s.project().tracks[&track].name,
            "Keys",
            "Enter while composing ends the edit, not the name"
        );
        s.input_map
            .name
            .as_mut()
            .unwrap()
            .field
            .replace_text(None, "ピアノ");
        assert!(s.name_key("enter", Mods::NONE));
        assert!(s.input_map.name.is_none());
        assert_eq!(s.project().tracks[&track].name, "ピアノ");
    }
}
