//! Values entered in the palette use the registry's argument types and validation.
use crate::palette::{Palette, Row, Way, target_kind};
use crate::studio::{Geometry, Studio};
use serde_json::{Map, Value, json};
use xirius_core::command::{ArgType, CommandDef};

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: &'static str,
    pub label: String,
    pub value: String,
    pub choices: Vec<(String, String)>,
}

impl Field {
    pub fn display(&self, value: &str) -> String {
        self.choices
            .iter()
            .find(|(raw, _)| raw == value)
            .map(|(_, label)| label.clone())
            .unwrap_or_else(|| value.to_owned())
    }

    /// An on/off field: a press flips it.
    pub fn is_switch(&self) -> bool {
        self.choices.len() == 2 && self.choices[0].0 == "true" && self.choices[1].0 == "false"
    }

    /// A field that takes a folder, with a button at its end that asks the OS for one (BRW-004).
    pub fn picks_folder(&self) -> bool {
        self.name == "media_folder"
    }
}

/// The folder button's tip.
pub const PICK_FOLDER: &str = "フォルダーを選択";

/// The folder button at the end of a field's row.
pub(crate) fn folder_button(row: crate::studio::Rect) -> crate::studio::Rect {
    use xirius_design::Layout;
    let s = Layout::ICON_BUTTON;
    crate::studio::Rect::new(row.right() - 6.0 - s, row.y + (row.h - s) / 2.0, s, s)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Form {
    pub command: &'static str,
    pub title: String,
    pub seed: Map<String, Value>,
    pub fields: Vec<Field>,
    pub output: bool,
    pub error: Option<String>,
}

impl Form {
    pub fn new(def: &CommandDef, seed: Value, project: &xirius_core::Project) -> Self {
        let seed = seed.as_object().cloned().unwrap_or_default();
        let fields = def
            .args
            .iter()
            .filter(|arg| !seed.contains_key(arg.name))
            .map(|arg| Field {
                name: arg.name,
                label: xirius_core::commands::labels::argument(arg.name).to_string(),
                value: arg
                    .default
                    .as_ref()
                    .map(text)
                    .or_else(|| current_value(def.name, arg.name, project))
                    .unwrap_or_default(),
                choices: match &arg.ty {
                    ArgType::Choice(choices) => choices
                        .iter()
                        .map(|value| {
                            (
                                (*value).to_owned(),
                                xirius_core::commands::labels::value(value).to_owned(),
                            )
                        })
                        .collect(),
                    ArgType::Bool => vec![
                        ("true".into(), "オン".into()),
                        ("false".into(), "オフ".into()),
                    ],
                    ArgType::Id(kind) => id_choices(project, *kind),
                    ArgType::Text if arg.name == "into" => {
                        std::iter::once(("root".into(), "最上位".into()))
                            .chain(
                                project
                                    .folders
                                    .values()
                                    .map(|folder| (folder.id.to_string(), folder.name.clone())),
                            )
                            .collect()
                    }
                    _ => Vec::new(),
                },
            })
            .collect();
        Self {
            command: def.name,
            title: def.label().unwrap_or(def.name).into(),
            seed,
            fields,
            output: false,
            error: None,
        }
    }

    pub fn args(&self) -> Value {
        let mut args = self.seed.clone();
        for field in &self.fields {
            if !field.value.is_empty() {
                args.insert(field.name.into(), json!(field.value));
            }
        }
        Value::Object(args)
    }

    pub fn rows(&self) -> Vec<Row> {
        let mut rows: Vec<_> = self
            .fields
            .iter()
            .map(|field| Row {
                name: "",
                label: field.label.clone(),
                key: None,
                icon: "prompt",
                enabled: true,
                reason: String::new(),
                way: Way::Arguments(json!({})),
            })
            .collect();
        if !self.output {
            rows.push(Row {
                name: "",
                label: "適用".into(),
                key: None,
                icon: "check",
                enabled: true,
                reason: String::new(),
                way: Way::Arguments(json!({})),
            });
        }
        rows
    }
}

/// What the song has now, for a field that has no default of its own: a form that changes a
/// setting starts from the setting.
/// A command the window runs itself (KEY-012): the screen's commands, and moving the playhead,
/// which the screen keeps.
pub(crate) fn on_screen(command: &str) -> bool {
    xirius_core::commands::screen_ops::NAMES.contains(&command)
        || matches!(command, "transport.seek" | "transport.goto")
}

fn current_value(command: &str, arg: &str, project: &xirius_core::Project) -> Option<String> {
    match (command, arg) {
        ("project.settings", "sample_rate") => Some(project.settings.sample_rate.to_string()),
        ("project.settings", "bit_depth") => Some(project.settings.bit_depth.to_string()),
        ("project.settings", "media_folder") => Some(project.media.folder.clone()),
        ("project.settings", "copy_imports") => Some(project.media.copy_imports.to_string()),
        _ => None,
    }
}

fn text(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}

/// A rack's layers, splitters and branches, named by where they are.
fn rack_choices(track: &str, rack: &xirius_core::rack::Rack, out: &mut Vec<(String, String)>) {
    use xirius_core::rack::{RackNode, Source};
    fn walk(at: &str, nodes: &[RackNode], out: &mut Vec<(String, String)>) {
        for (n, split) in nodes
            .iter()
            .filter_map(|node| match node {
                RackNode::Split(s) => Some(s),
                RackNode::Plugin(_) => None,
            })
            .enumerate()
        {
            let here = format!("{at} / スプリッター {}", n + 1);
            out.push((split.id.to_string(), here.clone()));
            for (i, b) in split.branches.iter().enumerate() {
                let branch = format!("{here} / 分岐 {}", i + 1);
                out.push((b.id.to_string(), branch.clone()));
                walk(&branch, &b.fx, out);
            }
        }
    }
    if let Some(Source::Multi { id, layers }) = &rack.source {
        out.push((id.to_string(), format!("{track} / マルチ")));
        for layer in layers {
            let at = format!("{track} / {}", layer.name);
            out.push((layer.id.to_string(), at.clone()));
            walk(&at, &layer.fx, out);
        }
    }
    walk(track, &rack.fx, out);
}

fn id_choices(
    project: &xirius_core::Project,
    kind: xirius_core::id::IdKind,
) -> Vec<(String, String)> {
    use xirius_core::id::IdKind;
    let mut choices = Vec::new();
    if matches!(
        kind,
        IdKind::Track | IdKind::Node | IdKind::TrackOrRackNode | IdKind::Any
    ) {
        choices.extend(
            project
                .tracks
                .values()
                .map(|track| (track.id.to_string(), track.shown_name().to_string())),
        );
    }
    if matches!(kind, IdKind::Folder | IdKind::Node | IdKind::Any) {
        choices.extend(
            project
                .folders
                .values()
                .map(|folder| (folder.id.to_string(), folder.name.clone())),
        );
    }
    for track in project.tracks.values() {
        if matches!(
            kind,
            IdKind::RackNode | IdKind::RackNodeOrPlugin | IdKind::TrackOrRackNode | IdKind::Any
        ) && let Some(rack) = &track.rack
        {
            rack_choices(track.shown_name(), rack, &mut choices);
        }
        if matches!(
            kind,
            IdKind::Plugin | IdKind::RackNodeOrPlugin | IdKind::Any
        ) {
            choices.extend(track.inserts.iter().map(|plugin| {
                (
                    plugin.id.to_string(),
                    format!("{} / {}", track.shown_name(), plugin.name),
                )
            }));
        }
        if matches!(kind, IdKind::Send | IdKind::Any) {
            choices.extend(track.sends.iter().map(|send| {
                (
                    send.id.to_string(),
                    format!(
                        "{} → {}",
                        track.shown_name(),
                        project.tracks[&send.to].shown_name()
                    ),
                )
            }));
        }
        for clip in &track.clips {
            if matches!(kind, IdKind::Clip | IdKind::NoteScope | IdKind::Any) {
                choices.push((
                    clip.id.to_string(),
                    format!("{} / {}", track.name, clip.name),
                ));
            }
            if matches!(kind, IdKind::Note | IdKind::NoteScope | IdKind::Any) {
                choices.extend(clip.notes().iter().map(|note| {
                    (
                        note.id.to_string(),
                        format!(
                            "{} / {} · {}",
                            track.name,
                            note.pitch,
                            project.time_map().format(clip.start + note.start)
                        ),
                    )
                }));
            }
        }
    }
    if matches!(kind, IdKind::Section | IdKind::Any) {
        choices.extend(
            project
                .sections
                .iter()
                .map(|section| (section.id.to_string(), section.name.clone())),
        );
    }
    if matches!(kind, IdKind::Tempo | IdKind::Any) {
        choices.extend(project.tempo.iter().map(|point| {
            (
                point.id.to_string(),
                format!(
                    "{} · {} BPM",
                    project.time_map().format(point.tick),
                    point.bpm
                ),
            )
        }));
    }
    if matches!(kind, IdKind::TimeSig | IdKind::Any) {
        choices.extend(project.timesig.iter().map(|point| {
            (
                point.id.to_string(),
                format!("{} · {}/{}", point.bar, point.num, point.den),
            )
        }));
    }
    choices
}

pub(crate) fn paint_field(
    pen: &mut crate::draw::Pen,
    field: &Field,
    row: crate::studio::Rect,
    selected: bool,
    folder_hot: bool,
) {
    use crate::{draw::Align, studio::Rect};
    use xirius_design::{FontSize, Radius, Weight};
    let colors = pen.c;
    if selected {
        pen.round(row, colors.sel, Radius::S);
    }
    let mut label = Rect::new(row.x + 12.0, row.y, row.w * 0.42 - 12.0, row.h);
    let mut value = Rect::new(row.x + row.w * 0.45, row.y, row.w * 0.55 - 12.0, row.h);
    if field.picks_folder() {
        let button = folder_button(row);
        if folder_hot {
            pen.round(button, colors.hover, Radius::S);
        }
        value.w = (button.x - 4.0 - value.x).max(0.0);
        let s = xirius_design::Layout::ICON_SIZE;
        pen.icon(
            "folder-open",
            Rect::new(
                button.x + (button.w - s) / 2.0,
                button.y + (button.h - s) / 2.0,
                s,
                s,
            ),
            colors.ink_strong,
        );
    }
    // A short value (「オン」) leaves its room to a long name.
    let shown = field.display(&field.value);
    let room = value.right() - pen.measure(&shown, FontSize::Body, Weight::Regular) - 16.0;
    label.w = label.w.max(room - label.x);
    pen.mask(label, |pen| {
        pen.text(
            &field.label,
            FontSize::Body,
            Weight::Regular,
            colors.ink_strong,
            label,
            Align::Left,
        );
    });
    pen.mask(value, |pen| {
        pen.text(
            &shown,
            FontSize::Body,
            Weight::Regular,
            colors.ink,
            value,
            Align::Right,
        );
    });
}

impl Palette {
    pub fn store_field(&mut self) {
        if let Some(form) = &mut self.form
            && !form.output
            && let Some(field) = form.fields.get_mut(self.selected)
        {
            field.value = self.query.clone();
            form.error = None;
            self.rows = form.rows();
        }
    }

    pub fn choose_field(&mut self, next: usize) {
        self.store_field();
        self.selected = next.min(self.rows.len().saturating_sub(1));
        self.query = self
            .form
            .as_ref()
            .and_then(|f| f.fields.get(self.selected))
            .map(|f| f.value.clone())
            .unwrap_or_default();
        self.marked.clear();
    }

    pub fn cycle_field(&mut self) {
        if let Some(field) = self
            .form
            .as_ref()
            .filter(|f| !f.output)
            .and_then(|f| f.fields.get(self.selected))
            && !field.choices.is_empty()
        {
            let next = field
                .choices
                .iter()
                .position(|(v, _)| *v == self.query)
                .map_or(0, |i| (i + 1) % field.choices.len());
            self.query = field.choices[next].0.clone();
            self.store_field();
        }
    }
}

impl Studio {
    /// The folder the OS's window gave for the settings form's 「録音・バウンスの置き場所」: under
    /// the song's folder it is kept relative to it, elsewhere as it is.
    pub(crate) fn set_media_folder(&mut self, folder: &std::path::Path) {
        let value = self
            .song
            .dir()
            .and_then(|dir| {
                folder
                    .strip_prefix(dir)
                    .ok()
                    .map(std::path::Path::to_path_buf)
            })
            .filter(|rel| !rel.as_os_str().is_empty())
            .map_or_else(
                || folder.to_string_lossy().into_owned(),
                |rel| rel.to_string_lossy().replace('\\', "/"),
            );
        let Some(palette) = &mut self.palette else {
            return;
        };
        let Some(form) = &mut palette.form else {
            return;
        };
        let Some(i) = form.fields.iter().position(Field::picks_folder) else {
            return;
        };
        form.fields[i].value = value.clone();
        form.error = None;
        if palette.selected == i {
            palette.query = value;
        }
    }

    pub(crate) fn palette_seed(&self, g: &Geometry, def: &CommandDef) -> Value {
        let mut args = Map::new();
        for arg in &def.args {
            if let Some(kind) = target_kind(arg.name, &arg.ty, arg.required) {
                let ids = self.targets(g, kind);
                if !ids.is_empty() {
                    args.insert(
                        arg.name.into(),
                        if matches!(arg.ty, ArgType::Ids(_)) {
                            json!(ids)
                        } else {
                            json!(ids[0])
                        },
                    );
                }
            }
        }
        Value::Object(args)
    }

    pub(crate) fn open_palette_form(&mut self, command: &'static str, seed: Value) -> bool {
        let Some(def) = self.song.registry().get(command) else {
            return false;
        };
        let form = Form::new(def, seed, &self.project());
        let mut palette = Palette::new(form.rows());
        palette.query = form
            .fields
            .first()
            .map(|f| f.value.clone())
            .unwrap_or_default();
        palette.form = Some(form);
        self.palette = Some(palette);
        true
    }

    pub(crate) fn palette_form_enter(&mut self, g: &Geometry) -> bool {
        let Some(palette) = &mut self.palette else {
            return false;
        };
        palette.store_field();
        let form = palette.form.as_ref().unwrap();
        if form.output {
            self.palette = None;
            return true;
        }
        let command = form.command;
        let args = form.args();
        let def = self.song.registry().get(command).unwrap();
        if let Err(error) = self
            .song
            .registry()
            .normalize(def, args.as_object().unwrap())
        {
            let label = def
                .args
                .iter()
                .find(|arg| error.message.contains(&format!("--{}", arg.name)))
                .map(|arg| match (arg.min, arg.max) {
                    (Some(min), Some(max)) => format!(
                        "{}: {min}–{max}",
                        xirius_core::commands::labels::argument(arg.name)
                    ),
                    _ => format!(
                        "{}: 入力値が無効",
                        xirius_core::commands::labels::argument(arg.name)
                    ),
                })
                .unwrap_or_else(|| "入力値が無効".into());
            self.palette.as_mut().unwrap().form.as_mut().unwrap().error = Some(label);
            self.error = None;
            return true;
        }
        if on_screen(command) {
            self.palette = None;
            let args = args.as_object().cloned().unwrap_or_default();
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            return self.run_parsed(g, command.into(), args);
        }
        if self
            .song
            .registry()
            .get(command)
            .is_some_and(|def| def.risk == xirius_core::Risk::Read)
        {
            return self.palette_query(g, command, args);
        }
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        let ran = self.run_command(command, args);
        if ran {
            self.palette = None;
            self.rebuild(g);
        } else if let Some(form) = self
            .palette
            .as_mut()
            .and_then(|palette| palette.form.as_mut())
        {
            form.error = Some("適用できません".into());
            self.error = None;
        }
        true
    }

    pub(crate) fn palette_query(
        &mut self,
        g: &Geometry,
        command: &'static str,
        args: Value,
    ) -> bool {
        if command == "app.update.check" {
            self.palette = None;
            self.open_about(self.about_anchor(g));
            self.manual_update_check();
            return true;
        }
        if self.transport.is_windowed() {
            if self.input_blocked() {
                return false;
            }
            let palette = self.palette.take();
            self.io_command_on_worker(command, args.as_object().cloned().unwrap_or_default());
            self.palette = palette;
            if let Some(save) = &mut self.saving {
                save.palette_query = Some(command);
            }
            return true;
        }
        // design-lint: allow(ui-blocking-io) PRJ-031 review: Song::ensure_inline refuses unreviewed Session handlers; model edits enqueue history, and reviewed status/list calls use window caches.
        let result = self.song.run(command, args);
        self.finish_palette_query(command, result)
    }

    pub(crate) fn finish_palette_query(
        &mut self,
        command: &'static str,
        result: Result<Value, String>,
    ) -> bool {
        match result {
            Ok(value) => {
                let def = self.song.registry().get(command).unwrap();
                let mut form = Form::new(def, json!({}), &self.project());
                form.output = true;
                let mut fields = Vec::new();
                result_fields(&value, "", &mut fields);
                let mut palette = Palette::new(Vec::new());
                palette.form = Some(form);
                palette.replace_result_fields(fields);
                self.palette = Some(palette);
                self.error = None;
                true
            }
            Err(error) => {
                self.error = None;
                if let Some(form) = self.palette.as_mut().and_then(|p| p.form.as_mut()) {
                    form.error = Some(format!("取得できません: {error}"));
                }
                false
            }
        }
    }
}

pub(crate) fn result_fields(value: &Value, label: &str, fields: &mut Vec<Field>) {
    match value {
        Value::Object(values) if !values.is_empty() => {
            for (key, value) in values {
                let name = xirius_core::commands::labels::argument(key);
                let path = if label.is_empty() {
                    name.to_string()
                } else {
                    format!("{label} / {name}")
                };
                result_fields(value, &path, fields);
            }
        }
        Value::Array(values) if !values.is_empty() => {
            for (i, value) in values.iter().enumerate() {
                let group = format!("{label} [{}]", i + 1);
                fields.push(Field {
                    name: "group",
                    label: group.clone(),
                    value: String::new(),
                    choices: Vec::new(),
                });
                result_fields(value, &group, fields);
            }
        }
        value => fields.push(Field {
            name: "value",
            label: label.into(),
            value: text(value),
            choices: if value.is_boolean() {
                vec![
                    ("true".into(), "オン".into()),
                    ("false".into(), "オフ".into()),
                ]
            } else {
                Vec::new()
            },
        }),
    }
}

/// Separate from editable fields: both columns wrap and every byte stays available to copy.
pub(crate) fn result_lines(field: &Field, width: f32) -> (Vec<String>, Vec<String>) {
    if field.name == "group" {
        return (
            wrap_result(&field.label, (width - 24.0).max(1.0), &|t| {
                crate::fonts::measure(
                    t,
                    xirius_design::FontSize::Body,
                    xirius_design::Weight::Medium,
                )
            }),
            Vec::new(),
        );
    }
    let (_, lw, _, vw) = result_columns(width);
    let wrap = |text: &str, room| {
        wrap_result(text, room, &|t| {
            crate::fonts::measure(
                t,
                xirius_design::FontSize::Body,
                xirius_design::Weight::Regular,
            )
        })
    };
    (
        wrap(&field.label, lw),
        wrap(&field.display(&field.value), vw),
    )
}
fn wrap_result(text: &str, width: f32, measure: &impl Fn(&str) -> f32) -> Vec<String> {
    text.split('\n')
        .flat_map(|line| {
            let lines = crate::opline::wrap(line, width, measure);
            if lines.is_empty() {
                vec![String::new()]
            } else {
                lines
            }
        })
        .collect()
}

/// Immutable output layout. Prefix positions are built in one pass and searched in O(log n).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ResultLayout {
    pub width: f32,
    pub lines: Vec<(Vec<String>, Vec<String>)>,
    pub tops: Vec<f32>,
}
impl ResultLayout {
    pub fn new(fields: &[Field], width: f32) -> Self {
        Self::build(fields, width, result_lines)
    }
    fn build(
        fields: &[Field],
        width: f32,
        mut wrap: impl FnMut(&Field, f32) -> (Vec<String>, Vec<String>),
    ) -> Self {
        let mut tops = Vec::with_capacity(fields.len() + 1);
        let mut lines = Vec::with_capacity(fields.len());
        tops.push(0.0);
        for field in fields {
            let row = wrap(field, width);
            tops.push(tops.last().unwrap() + result_line_height(&row));
            lines.push(row);
        }
        Self { width, lines, tops }
    }
    pub fn total(&self) -> f32 {
        *self.tops.last().unwrap()
    }
    pub fn visible(&self, offset: f32, height: f32) -> std::ops::Range<usize> {
        self.visible_probed(offset, height, || {})
    }
    fn visible_probed(
        &self,
        offset: f32,
        height: f32,
        mut probe: impl FnMut(),
    ) -> std::ops::Range<usize> {
        let start = self
            .tops
            .partition_point(|y| {
                probe();
                *y <= offset
            })
            .saturating_sub(1)
            .min(self.lines.len());
        let end = self
            .tops
            .partition_point(|y| {
                probe();
                *y < offset + height
            })
            .min(self.lines.len());
        start..end
    }
}
fn result_line_height(lines: &(Vec<String>, Vec<String>)) -> f32 {
    16.0 + lines.0.len().max(lines.1.len()).max(1) as f32 * 22.0
}
fn result_columns(width: f32) -> (f32, f32, f32, f32) {
    let label = (width * 0.34).min(180.0);
    (
        12.0,
        (label - 24.0).max(1.0),
        label,
        (width - label - 12.0).max(1.0),
    )
}
#[cfg(test)]
pub(crate) fn result_height(field: &Field, width: f32) -> f32 {
    let (labels, values) = result_lines(field, width);
    result_line_height(&(labels, values))
}
pub(crate) fn paint_result(
    pen: &mut crate::draw::Pen,
    field: &Field,
    row: crate::studio::Rect,
    selected: bool,
    lines: &(Vec<String>, Vec<String>),
) {
    use crate::{draw::Align, studio::Rect};
    use xirius_design::{FontSize, Radius, Weight};
    if field.name == "group" {
        pen.round(
            row,
            if selected { pen.c.sel } else { pen.c.surface_2 },
            Radius::S,
        );
        let (lines, _) = lines;
        for (i, line) in lines.iter().enumerate() {
            pen.text(
                line,
                FontSize::Body,
                Weight::Medium,
                pen.c.ink_strong,
                Rect::new(
                    row.x + 12.0,
                    row.y + 8.0 + i as f32 * 22.0,
                    row.w - 24.0,
                    22.0,
                ),
                Align::Left,
            );
        }
        return;
    }
    if selected {
        pen.round(row, pen.c.sel, Radius::S);
    }
    let (lx, lw, vx, vw) = result_columns(row.w);
    let (labels, values) = lines;
    for (lines, x, w) in [(labels, lx, lw), (values, vx, vw)] {
        for (i, line) in lines.iter().enumerate() {
            pen.text(
                line,
                FontSize::Body,
                Weight::Regular,
                pen.c.ink,
                Rect::new(row.x + x, row.y + 8.0 + i as f32 * 22.0, w, 22.0),
                Align::Left,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{studio::Hit, topbar::BarButton, tree::RowKind};

    #[test]
    fn the_master_track_is_listed_as_masuta_in_the_choices_of_a_command() {
        let project = crate::song::Song::sample().project();
        let choices = id_choices(&project, xirius_core::id::IdKind::Track);
        let master = xirius_core::id::TrackId::MASTER.to_string();
        let shown = choices.iter().find(|(id, _)| *id == master);
        assert_eq!(shown.map(|(_, n)| n.as_str()), Some("マスター"));
        assert!(!choices.iter().any(|(_, n)| n == "Master"));
    }

    #[test]
    fn a_rack_offers_its_splitters_and_branches_as_choices() {
        use xirius_core::session::{RunOptions, Session};
        let mut s = Session::new("rack");
        let t = s
            .run_value(
                "track.add.audio",
                serde_json::json!({ "name": "T" }),
                &RunOptions::human(),
            )
            .unwrap()
            .data["id"]
            .as_str()
            .unwrap()
            .to_string();
        let split = s
            .run_value(
                "rack.split.add",
                serde_json::json!({ "track": t }),
                &RunOptions::human(),
            )
            .unwrap()
            .data;
        let names = |kind| id_choices(s.project(), kind);
        let nodes = names(xirius_core::id::IdKind::RackNode);
        let branch = split["branches"][1].as_str().unwrap();
        assert!(
            nodes
                .iter()
                .any(|(id, n)| id == branch && n == "T / スプリッター 1 / 分岐 2"),
            "{nodes:?}"
        );
        let to = names(xirius_core::id::IdKind::TrackOrRackNode);
        assert!(to.iter().any(|(id, _)| *id == t), "{to:?}");
        assert!(to.iter().any(|(id, _)| id == branch), "{to:?}");
    }

    #[test]
    fn every_registered_command_is_listed_in_the_palette() {
        let mut studio = Studio::sample();
        let g = studio.geometry(1440.0, 820.0);
        let rows = studio.palette_rows(&g);
        for def in studio.song.registry().all() {
            assert!(rows.iter().any(|row| row.name == def.name), "{}", def.name);
        }
    }

    #[test]
    fn clicking_rename_enters_a_value_and_one_undo_restores_the_name() {
        let mut studio = Studio::sample();
        let g = studio.geometry(1440.0, 820.0);
        let track = studio
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Keys")
            .unwrap()
            .id;
        studio.select_row(Some(RowKind::Track(track)));
        let before = studio.song.steps();
        studio.press(&g, Hit::Bar(BarButton::Palette));
        studio
            .palette
            .as_mut()
            .unwrap()
            .replace_text(None, "track.rename");
        let row = studio.palette.as_ref().unwrap().layout(g.size).rows[0].1;
        assert!(studio.palette_press(&g, row.x + 10.0, row.y + 10.0));
        assert!(studio.palette.as_ref().unwrap().form.is_some());
        studio.palette.as_mut().unwrap().replace_text(None, "false");
        let form = studio.palette.as_ref().unwrap().form.as_ref().unwrap();
        assert_eq!(
            form.fields[0].display(&form.fields[0].value),
            "false",
            "a track name is literal text, not a boolean"
        );
        let apply = studio
            .palette
            .as_ref()
            .unwrap()
            .layout(g.size)
            .rows
            .last()
            .unwrap()
            .1;
        studio.palette_press(&g, apply.x + 10.0, apply.y + 10.0);
        assert!(studio.palette.is_none(), "{:?}", studio.error);
        assert_eq!(studio.project().tracks[&track].name, "false");
        assert_eq!(studio.song.steps(), before + 1);
        assert!(studio.undo(&g));
        assert_eq!(studio.project().tracks[&track].name, "Keys");
    }

    #[test]
    fn invalid_numeric_value_leaves_the_form_and_project_intact() {
        let mut studio = Studio::sample();
        let g = studio.geometry(1440.0, 820.0);
        let before = studio.project().content_hash();
        studio.open_palette_form("tempo.set", json!({"at": 0}));
        studio
            .palette
            .as_mut()
            .unwrap()
            .replace_text(Some(0..100), "-1");
        let last = studio.palette.as_ref().unwrap().rows.len() - 1;
        studio.palette.as_mut().unwrap().choose_field(last);
        studio.palette_form_enter(&g);
        assert!(studio.palette.is_some());
        assert!(studio.error.is_none());
        assert!(
            studio
                .palette
                .as_ref()
                .unwrap()
                .form
                .as_ref()
                .unwrap()
                .error
                .is_some()
        );
        assert_eq!(studio.project().content_hash(), before);
    }

    #[test]
    fn a_read_command_shows_values_and_does_not_make_a_history_step() {
        let mut studio = Studio::sample();
        let g = studio.geometry(1440.0, 820.0);
        let steps = studio.song.steps();
        let row = studio
            .palette_rows(&g)
            .into_iter()
            .find(|row| row.name == "project.get")
            .unwrap();
        studio.run_row(&g, &row);
        let form = studio.palette.as_ref().unwrap().form.as_ref().unwrap();
        assert!(form.output);
        assert!(
            form.fields
                .iter()
                .any(|field| field.value == crate::song::SAMPLE_NAME)
        );
        assert_eq!(studio.song.steps(), steps);
    }
    #[test]
    fn result_names_unknown_keys_arrays_and_wrapping_preserve_full_values() {
        let long = "https://example.invalid/".to_string() + &"長い結果/".repeat(80);
        let mut fields = Vec::new();
        result_fields(
            &json!({"name":"曲", "unregistered_key":long,"songs":[{"name":"一"},{"name":"二"}]}),
            "",
            &mut fields,
        );
        assert!(fields.iter().any(|f| f.label == "名前"));
        assert_eq!(fields.iter().filter(|f| f.name == "group").count(), 2);
        let unknown = fields
            .iter()
            .find(|f| f.label == "unregistered_key")
            .unwrap();
        assert_eq!(unknown.value, long);
        assert!(
            fields
                .iter()
                .any(|f| f.label.contains("[1]") && f.value == "一")
        );
        assert!(
            fields
                .iter()
                .any(|f| f.label.contains("[2]") && f.value == "二")
        );
        for width in [240.0, 440.0, 592.0] {
            let (labels, values) = result_lines(unknown, width);
            assert_eq!(values.concat(), long);
            let (_, lw, _, vw) = result_columns(width);
            for (lines, room) in [(labels, lw), (values, vw)] {
                for line in lines {
                    assert!(
                        crate::fonts::measure(
                            &line,
                            xirius_design::FontSize::Body,
                            xirius_design::Weight::Regular
                        ) <= room + 0.01
                    );
                }
            }
        }
        assert!(result_height(unknown, 240.0) > result_height(unknown, 592.0));
    }

    #[test]
    fn results_select_and_copy_full_text_and_do_not_change_input_fields() {
        let mut s = Studio::sample();
        let g = s.geometry(400.0, 600.0);
        s.palette_query(&g, "project.get", json!({}));
        let p = s.palette.as_mut().unwrap();
        p.replace_result_fields(vec![Field {
            name: "value",
            label: "結果".into(),
            value: "長い値".repeat(500),
            choices: vec![],
        }]);
        p.rows = p.form.as_ref().unwrap().rows();
        p.select(0);
        assert_eq!(p.selected_result_text().unwrap(), "長い値".repeat(500));
        s.palette_key(
            &g,
            "a",
            crate::tools::Mods {
                ctrl: true,
                ..Default::default()
            },
        );
        let p = s.palette.as_ref().unwrap();
        assert!(p.result_all);
        assert_eq!(
            p.selected_result_text().unwrap(),
            format!("結果: {}", "長い値".repeat(500))
        );
        let height = p.layout(g.size).rows[0].1.h;
        assert!(height > Palette::ROW_H);
        let l = p.layout(g.size);
        s.palette_wheel(&g, l.list.x + 20.0, l.list.y + 20.0, -100000.0);
        assert!(s.palette.as_ref().unwrap().offset > 0.0);
        s.open_palette_form("tempo.set", json!({"at":0}));
        let input = s.palette.as_ref().unwrap();
        assert!(!input.form.as_ref().unwrap().output);
        assert!(
            input
                .layout(g.size)
                .rows
                .iter()
                .all(|(_, r)| r.h == Palette::ROW_H)
        );
    }
    #[test]
    fn result_boolean_display_is_localized_but_strings_and_copy_stay_literal() {
        let mut fields = Vec::new();
        result_fields(
            &json!({"enabled":true,"disabled":false,"literal":"false"}),
            "",
            &mut fields,
        );
        assert_eq!(
            fields
                .iter()
                .find(|f| f.value == "true")
                .unwrap()
                .display("true"),
            "オン"
        );
        assert_eq!(
            fields
                .iter()
                .find(|f| f.label == "disabled")
                .unwrap()
                .display("false"),
            "オフ"
        );
        assert_eq!(
            fields
                .iter()
                .find(|f| f.label == "literal")
                .unwrap()
                .display("false"),
            "false"
        );
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 820.0);
        s.palette_query(&g, "project.get", json!({}));
        let p = s.palette.as_mut().unwrap();
        p.replace_result_fields(fields);
        p.result_all = true;
        let copied = p.selected_result_text().unwrap();
        assert!(copied.contains("disabled: false") && copied.contains("literal: false"));
    }

    #[test]
    fn result_wrap_matches_native_paint_font_for_wide_narrow_and_japanese_text() {
        use xirius_design::{FontSize, Weight};
        let native = std::sync::Arc::new(gpui::TextSystem::new(
            gpui_platform::current_platform(false).text_system(),
        ));
        native
            .add_fonts(
                crate::fonts::EMBEDDED
                    .iter()
                    .map(|(_, bytes)| std::borrow::Cow::Borrowed(*bytes))
                    .collect(),
            )
            .unwrap();
        let native = gpui::WindowTextSystem::new(native);
        let measured = |text: &str, weight| {
            f32::from(
                native
                    .shape_line(
                        text.to_owned().into(),
                        gpui::px(FontSize::Body.px()),
                        &crate::theme::text_runs(text, weight, false, gpui::black()),
                        None,
                    )
                    .width,
            )
        };
        for group in [false, true] {
            let content = "WWWWMMMMiiiillll日本語".repeat(12);
            let field = Field {
                name: if group { "group" } else { "value" },
                label: content.clone(),
                value: content.clone(),
                choices: vec![],
            };
            let weight = if group {
                Weight::Medium
            } else {
                Weight::Regular
            };
            assert!(
                (crate::fonts::measure(&content, FontSize::Body, weight)
                    - measured(&content, weight))
                .abs()
                    < 0.001
            );
            for width in [240.0, 440.0, 592.0] {
                let (labels, values) = result_lines(&field, width);
                let (_, lw, _, vw) = result_columns(width);
                for (lines, room) in [
                    (&labels, if group { width - 24.0 } else { lw }),
                    (&values, vw),
                ] {
                    for line in lines {
                        assert!(measured(line, weight) <= room + 0.001, "{line}");
                    }
                }
                assert_eq!(labels.concat(), content);
                if !group {
                    assert_eq!(values.concat(), content);
                }
                assert_eq!(
                    result_height(&field, width),
                    16.0 + labels.len().max(values.len()).max(1) as f32 * 22.0
                );
            }
        }
        assert!(measured("WWWW", Weight::Regular) > measured("iiii", Weight::Regular) * 3.0);
    }

    #[test]
    fn large_result_build_visits_each_row_once_and_visible_search_is_logarithmic() {
        for n in [1024, 65536] {
            let value = Value::Array(vec![json!({"name": "WMi日本語"}); n]);
            let mut fields = Vec::new();
            result_fields(&value, "結果", &mut fields);
            let count = fields.len();
            assert_eq!(count, n * 2, "each array item has a group and its value");
            let mut wraps = 0;
            let rows = ResultLayout::build(&fields, 592.0, |field, _| {
                wraps += 1;
                (vec![field.label.clone()], vec![field.value.clone()])
            });
            assert_eq!(wraps, count);
            assert_eq!(rows.tops.len(), count + 1);
            let mut probes = 0;
            let range = rows.visible_probed((count / 2) as f32 * 38.0, 300.0, || probes += 1);
            assert_eq!(range.start, count / 2);
            assert!(range.len() <= 9);
            assert!(probes <= 2 * (count.ilog2() as usize + 2), "{probes}");
        }
    }
}
