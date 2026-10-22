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

fn id_choices(
    project: &xirius_core::Project,
    kind: xirius_core::id::IdKind,
) -> Vec<(String, String)> {
    use xirius_core::id::IdKind;
    let mut choices = Vec::new();
    if matches!(kind, IdKind::Track | IdKind::Node | IdKind::Any) {
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
        if matches!(kind, IdKind::Plugin | IdKind::Any) {
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
        if let Some(field) = self.form.as_ref().and_then(|f| f.fields.get(self.selected))
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
            return self.run_parsed(g, command.into(), args);
        }
        if self
            .song
            .registry()
            .get(command)
            .is_some_and(|def| def.risk == xirius_core::Risk::Read)
        {
            return self.palette_query(command, args);
        }
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

    pub(crate) fn palette_query(&mut self, command: &'static str, args: Value) -> bool {
        match self.song.run(command, args) {
            Ok(value) => {
                let def = self.song.registry().get(command).unwrap();
                let mut form = Form::new(def, json!({}), &self.project());
                form.output = true;
                form.fields.clear();
                result_fields(&value, "", &mut form.fields);
                let mut palette = Palette::new(form.rows());
                palette.form = Some(form);
                self.palette = Some(palette);
                self.error = None;
                true
            }
            Err(error) => {
                let _ = error;
                self.error = None;
                if let Some(form) = self.palette.as_mut().and_then(|p| p.form.as_mut()) {
                    form.error = Some("取得できません".into());
                }
                false
            }
        }
    }
}

fn result_fields(value: &Value, label: &str, fields: &mut Vec<Field>) {
    if fields.len() >= 256 {
        return;
    }
    match value {
        Value::Object(values) => {
            for (key, value) in values {
                if !matches!(
                    key.as_str(),
                    "state_hash" | "id" | "uid" | "summary" | "description"
                ) {
                    result_fields(value, xirius_core::commands::labels::argument(key), fields);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                result_fields(value, label, fields);
            }
        }
        Value::Null => {}
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
}
