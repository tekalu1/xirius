//! Files dragged over a Drum Kit's window (UI-334): before they fall, the pads they would
//! fill show what each gets (its name, its layers), their banks' letters light, and a chip
//! by the pointer says it in a word: `21 pads, A1–B5`, `A1: 30 layers`, `A1: Kick`.

use std::path::{Path, PathBuf};

use xirius_core::commands::samples::{kit_groups, kit_pads, kit_replaced, used_pads};
use xirius_core::id::PluginId;
use xirius_design::{FontSize, Radius, Weight};
use xirius_params::{PAD_NAMES, PADS, pad_key};
use xirius_session::sample_files::audio_files;

use super::window::{Part, live};
use crate::device::layout::{Body, Mark};
use crate::device::{play, sampler};
use crate::draw::{Align, Pen};
use crate::studio::{Geometry, Rect, Studio};
use crate::title_bar;

/// What a drop would do to a Drum Kit: the pads it fills, and the chip's words.
#[derive(Clone, Debug, PartialEq)]
pub struct Preview {
    pub plugin: PluginId,
    /// Each pad it fills: the pad, the name it would show, its layers (`Vel 3`, or none).
    pub pads: Vec<(usize, String, String)>,
    pub text: String,
    /// Where the pointer is.
    pub at: (f32, f32),
}

/// The paths a drag holds and the sounds they are, a folder's in the order the drop reads
/// them: read once for each drag, not on every move (N3-kit-1), and only once the pointer
/// is over a Drum Kit, the one place that shows them; the drag leaving and coming back
/// reads them no more (the fourth review's low 16), unless a folder's list changed since
/// (the fifth review's low 2: a drag that ended outside the window leaves them here, and the
/// next drag of the same folder may hold more). And where the pointer was, for Shift
/// pressed or let go without a move.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Held {
    paths: Vec<PathBuf>,
    names: Option<Vec<String>>,
    /// When each path last changed as it was read (a folder's time moves as files come
    /// and go).
    stamps: Vec<Option<std::time::SystemTime>>,
    at: Option<(f32, f32)>,
}

impl Held {
    fn of(&mut self, paths: &[PathBuf]) {
        if self.paths != paths {
            self.paths = paths.to_vec();
            self.names = None;
        } else if self.at.is_none() && self.names.is_some() && stamps(paths) != self.stamps {
            // Come into the window again: what was read is stale.
            self.names = None;
        }
    }

    fn names(&mut self) -> &[String] {
        if self.names.is_none() {
            self.stamps = stamps(&self.paths);
        }
        self.names.get_or_insert_with(|| sounds(&self.paths))
    }

    /// Whether the sounds of the paths were read.
    #[cfg(test)]
    pub(crate) fn read(&self) -> bool {
        self.names.is_some()
    }

    /// Lets the pointer go, keeping the paths and what was read of them.
    pub(crate) fn leave(&mut self) {
        self.at = None;
    }
}

/// The sounds `paths` are, as `plugin.samples.load` reads them: a folder's audio files by
/// name ([`audio_files`]), and each audio file.
fn sounds(paths: &[PathBuf]) -> Vec<String> {
    let leaf = |p: &Path| {
        p.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    };
    let mut names = Vec::new();
    for p in paths {
        // design-lint: allow(ui-blocking-io) N3-kit-1: one metadata read per path the OS drags in, and a folder's list (the drop's own `audio_files`), once for each drag (the moves after it use what was read).
        if p.is_dir() {
            if let Ok(files) = audio_files(p) {
                names.extend(files.iter().map(|f| leaf(f)));
            }
        } else if crate::drop_audio::is_audio(p) {
            names.push(leaf(p));
        }
    }
    names
}

/// When each of `paths` last changed.
fn stamps(paths: &[PathBuf]) -> Vec<Option<std::time::SystemTime>> {
    paths
        .iter()
        // design-lint: allow(ui-blocking-io) the fifth review's low 2: one metadata read per path the OS drags in, as the drag comes into the window again after the paths were read.
        .map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
        .collect()
}

/// How a pad picks among its layers, as its tag says it.
const TAGS: [&str; 4] = ["Vel", "RR", "Rnd", "Stack"];

impl Studio {
    /// Files dragged over the screen at `(x, y)`, Shift held or not: over a Drum Kit's
    /// window, what a drop there would do. True when that changed.
    pub fn window_drag(
        &mut self,
        g: &Geometry,
        x: f32,
        y: f32,
        paths: &[PathBuf],
        shift: bool,
    ) -> bool {
        self.instruments.dragged.of(paths);
        self.instruments.dragged.at = Some((x, y));
        // What takes the drop before a window does takes the plan too: a `.dawproject` and the
        // quantize panel's groove (low 14 of the fourth review).
        let first = paths
            .iter()
            .any(|p| xirius_core::dawproject::is_dawproject(p))
            || self.groove_over(g, x, y);
        let drop_on = self.drop_target(g, x, y).filter(|_| !first);
        let on_layers = drop_on.is_some_and(|d| d.1 == sampler::DropOn::Layers);
        let preview = if first || !self.over_kit(g, x, y) {
            None
        } else {
            let mut held = std::mem::take(&mut self.instruments.dragged);
            let plan = self.kit_plan(g, x, y, held.names(), shift || on_layers);
            self.instruments.dragged = held;
            plan
        };
        let changed = self.instruments.preview != preview || self.instruments.drop_on != drop_on;
        self.instruments.preview = preview;
        self.instruments.drop_on = drop_on;
        changed
    }

    /// Shift pressed or let go while files are dragged: the plan again where the pointer
    /// is, though it did not move (N3-kit-1). True when that changed.
    pub fn drag_shift(&mut self, g: &Geometry, shift: bool) -> bool {
        if self.instruments.drop_layers == shift {
            return false;
        }
        self.instruments.drop_layers = shift;
        let Some((x, y)) = self.instruments.dragged.at else {
            return false;
        };
        let paths = self.instruments.dragged.paths.clone();
        self.window_drag(g, x, y, &paths, shift)
    }

    /// Whether `(x, y)` is on a Drum Kit's window or face.
    fn over_kit(&self, g: &Geometry, x: f32, y: f32) -> bool {
        let p = self.project();
        self.instruments.at(&p, g, x, y).is_some_and(|(i, _)| {
            live(&p, &self.instruments.open[i]).is_some_and(|(_, _, def)| def.uid == "drum-kit")
        })
    }

    /// Where files dropped at `(x, y)` on a Sampler's or a Drum Kit's window go, and the
    /// place that lights for it: [`sampler::drop_on`].
    pub(crate) fn drop_target(
        &self,
        g: &Geometry,
        x: f32,
        y: f32,
    ) -> Option<(PluginId, sampler::DropOn, Rect)> {
        let p = self.project();
        let (i, _) = self.instruments.at(&p, g, x, y)?;
        let w = self.instruments.open[i];
        let (_, plugin, def) = live(&p, &w)?;
        if !sampler::has_files(def) {
            return None;
        }
        let r = self.instruments.frame(i, g);
        let body = self.instruments.body(r, plugin, def);
        let head = title_bar::BAR_H;
        let inside = Rect::new(r.x + 4.0, r.y + head, r.w - 8.0, r.h - head - 4.0);
        let (on, at) = sampler::drop_on(&body, def.uid == "drum-kit", inside, x, y)?;
        Some((w.plugin, on, at))
    }

    /// What the sounds `names` dropped at `(x, y)` would do to the Drum Kit's window there:
    /// the pads `plugin.samples.load` would give them, all on the pad chosen as its layers
    /// with `layers` (Shift, or the layers' bar). None off one, and with no sound.
    fn kit_plan(
        &self,
        g: &Geometry,
        x: f32,
        y: f32,
        names: &[String],
        layers: bool,
    ) -> Option<Preview> {
        let p = self.project();
        let (i, part) = self.instruments.at(&p, g, x, y)?;
        let w = self.instruments.open[i];
        let (_, plugin, def) = live(&p, &w)?;
        if def.uid != "drum-kit" {
            return None;
        }
        let start = match part {
            Part::Pad(pitch) => play::pad_of(pitch),
            _ => w.pick.min(PADS - 1),
        };
        let used = used_pads(def, plugin);
        // Only the sounds go in.
        if names.is_empty() {
            return None;
        }
        let groups = kit_groups(names, layers);
        let pads = kit_pads(&used, Some(start), groups.len(), layers);
        // A pad that has files takes the new ones in their place, unless they are its layers.
        let replaced = kit_replaced(&used, &pads, layers);
        let mode = def.per_pad.iter().find(|s| s.id == "layer_mode");
        let shown: Vec<(usize, String, String)> = groups
            .iter()
            .zip(&pads)
            .map(|(group, &pad)| {
                // A pad the files are layers of keeps its name and how it picks; the new ones
                // add to its layers.
                let had = if replaced.contains(&pad) {
                    Vec::new()
                } else {
                    sampler::pad_files(def, plugin, pad)
                };
                let first = had.first().map_or_else(
                    || sampler::stem(&names[group.files[0]]),
                    |&f| sampler::stem(&plugin.samples[f]),
                );
                let tag = if had.is_empty() {
                    usize::from(group.round_robin)
                } else {
                    mode.and_then(|m| {
                        let v = plugin.params.get(&pad_key(pad, m.id))?;
                        Some(m.to_plain(*v).round() as usize)
                    })
                    .unwrap_or(0)
                };
                let n = had.len() + group.files.len();
                let sub = if n > 1 {
                    format!("{} {n}", TAGS[tag.min(3)])
                } else {
                    String::new()
                };
                (pad, first, sub)
            })
            .collect();
        // As layers, the chip says how many the pad has after it (the mock's `A1: n layers`).
        let total = |pad: usize| {
            let had = if replaced.contains(&pad) {
                0
            } else {
                sampler::pad_files(def, plugin, pad).len()
            };
            had + groups.first().map_or(0, |g| g.files.len())
        };
        let mut text = match (shown.first(), shown.last(), groups.first()) {
            (Some(a), Some(b), _) if shown.len() > 1 => {
                format!(
                    "{} pads, {}–{}",
                    shown.len(),
                    PAD_NAMES[a.0],
                    PAD_NAMES[b.0]
                )
            }
            (Some(a), _, Some(_)) if total(a.0) > 1 => {
                format!("{}: {} layers", PAD_NAMES[a.0], total(a.0))
            }
            (Some(a), _, Some(group)) => {
                format!(
                    "{}: {}",
                    PAD_NAMES[a.0],
                    sampler::stem(&names[group.files[0]])
                )
            }
            _ => return None,
        };
        // The sounds past H16 that a drop leaves out (N4-kit-1).
        let left: usize = groups.iter().skip(pads.len()).map(|g| g.files.len()).sum();
        if left > 0 {
            text.push_str(&format!("（{left} 個は入らない）"));
        }
        Some(Preview {
            plugin: w.plugin,
            pads: shown,
            text,
            at: (x, y),
        })
    }
}

/// The body of `id`'s window with what a drop would do (UI-334): the pads it fills show
/// their names and layers, without their M and S; their banks' letters light.
pub fn ghost(body: &mut Body, preview: &Preview) {
    let filled = |p: usize| preview.pads.iter().find(|g| g.0 == p);
    body.spots.retain(|s| match s.mark {
        Mark::Mute(p) | Mark::Solo(p) => filled(p).is_none(),
        _ => true,
    });
    for s in &mut body.spots {
        match s.mark {
            Mark::Pad(p) => {
                if let Some((_, name, sub)) = filled(p) {
                    s.text.clone_from(name);
                    s.sub.clone_from(sub);
                    s.ghost = true;
                }
            }
            Mark::Bank(n) => {
                s.ghost = preview.pads.iter().any(|g| g.0 / play::PADS == n);
            }
            _ => {}
        }
    }
}

/// The chip by the pointer while files are dragged over a Drum Kit: what the drop does.
pub fn paint_chip(pen: &mut Pen, preview: &Preview) {
    let c = pen.c;
    let w = pen.measure(&preview.text, FontSize::Small, Weight::Regular) + 20.0;
    let r = Rect::new(preview.at.0 + 14.0, preview.at.1 + 10.0, w, 24.0);
    let at = gpui::Bounds::new(
        gpui::point(gpui::px(pen.origin.0 + r.x), gpui::px(pen.origin.1 + r.y)),
        gpui::size(gpui::px(r.w), gpui::px(r.h)),
    );
    pen.window
        .paint_drop_shadows(at, gpui::Corners::all(gpui::px(Radius::S.px())), &c.float);
    pen.round(r, c.surface_1, Radius::S);
    pen.text(
        &preview.text,
        FontSize::Small,
        Weight::Regular,
        c.ink_blue,
        r,
        Align::Center,
    );
}
