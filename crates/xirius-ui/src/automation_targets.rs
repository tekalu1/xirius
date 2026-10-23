//! The lane head's add and target lists (AUT-004).
use crate::{
    arrange::{LaneParts, lane_name, targets_of},
    popup::{Choice, PopupKind},
    studio::{Geometry, Rect, Studio},
    tree::RowKind,
};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use xirius_core::{
    auto::AutoTarget,
    id::{PluginId, TrackId},
    model::{Project, Track},
};

pub(crate) fn lane_label(p: &Project, t: &Track, target: &AutoTarget) -> String {
    if let AutoTarget::Param(id, name) = target
        && let Some(plugin) = t.inserts.iter().find(|x| x.id == *id)
        && let Some(def) = plugin.builtin()
        && let Some(param) = def.params.iter().find(|p| p.id == name)
    {
        return format!("{} · {}", plugin.name, param.label);
    }
    lane_name(p, t, target)
}

struct Definitions {
    labels: HashMap<(PluginId, u32), String>,
    names: HashMap<(PluginId, String), u32>,
}
impl Definitions {
    fn new(titles: &[(PluginId, u32, String)]) -> Self {
        let mut out = Self {
            labels: HashMap::new(),
            names: HashMap::new(),
        };
        for (plugin, id, title) in titles {
            out.labels
                .entry((*plugin, *id))
                .or_insert_with(|| title.clone());
            out.names
                .entry((*plugin, title.to_ascii_lowercase()))
                .or_insert(*id);
        }
        out
    }
    fn parameter(&self, target: &AutoTarget) -> Option<(PluginId, u32)> {
        let AutoTarget::Param(plugin, key) = target else {
            return None;
        };
        let id = key
            .parse::<u32>()
            .ok()
            .filter(|id| self.labels.contains_key(&(*plugin, *id)))
            .or_else(|| {
                self.names
                    .get(&(*plugin, key.to_ascii_lowercase()))
                    .copied()
            })?;
        Some((*plugin, id))
    }
}

fn choices(
    p: &Project,
    track: TrackId,
    from: Option<&AutoTarget>,
    titles: &[(PluginId, u32, String)],
) -> Vec<Choice> {
    let Some(t) = p.tracks.get(&track) else {
        return Vec::new();
    };
    let defs = Definitions::new(titles);
    let mut preferred = HashMap::new();
    let mut occupied_exact = HashSet::new();
    let mut occupied_params = HashSet::new();
    for lane in &t.automation {
        if let Some(key) = defs.parameter(&lane.target) {
            preferred.entry(key).or_insert_with(|| lane.target.clone());
            if from.is_some() || !lane.hidden {
                occupied_params.insert(key);
            }
        }
        if from.is_some() || !lane.hidden {
            occupied_exact.insert(lane.target.clone());
        }
    }
    if let Some(from) = from
        && let Some(key) = defs.parameter(from)
    {
        preferred.insert(key, from.clone());
    }
    let plugins: HashSet<_> = t.inserts.iter().map(|p| p.id).collect();
    let extra = titles
        .iter()
        .filter(|(p, _, _)| plugins.contains(p))
        .map(|(p, id, _)| AutoTarget::Param(*p, id.to_string()));
    let mut seen = HashSet::new();
    targets_of(t)
        .into_iter()
        .chain(extra)
        .filter_map(|target| match defs.parameter(&target) {
            Some(key) if !seen.insert(key) => None,
            Some(key) => Some(preferred.get(&key).cloned().unwrap_or(target)),
            None => Some(target),
        })
        .map(|target| {
            let occupied = occupied_exact.contains(&target)
                || defs
                    .parameter(&target)
                    .is_some_and(|key| occupied_params.contains(&key));
            let mut label = lane_name(p, t, &target);
            if let Some((id, key)) = defs.parameter(&target)
                && let Some(title) = defs.labels.get(&(id, key))
                && let Some(plugin) = t.inserts.iter().find(|x| x.id == id)
            {
                label = format!("{} · {title}", plugin.name);
            }
            if let AutoTarget::Param(id, name) = &target
                && let Some(plugin) = t.inserts.iter().find(|x| x.id == *id)
                && let Some(def) = plugin.builtin()
                && let Some(param) = def.params.iter().find(|x| x.id == name)
            {
                label = format!("{} · {}", plugin.name, param.label);
            }
            Choice {
                to: format!("{}{target}", if occupied { "off:" } else { "" }),
                label,
            }
        })
        .collect()
}

impl Studio {
    pub(crate) fn open_auto_targets(
        &mut self,
        g: &Geometry,
        track: TrackId,
        target: &AutoTarget,
        replace: bool,
    ) {
        let p = self.project();
        let Some(t) = p.tracks.get(&track) else {
            return;
        };
        let tree = self.tree();
        let Some(row) = tree.row_of(RowKind::Track(track)) else {
            return;
        };
        let Some(lane) = self
            .arrange
            .lanes_of(t)
            .iter()
            .position(|l| l.target == *target)
        else {
            return;
        };
        let parts = LaneParts::of(row, lane == 0);
        let r = if replace { parts.param } else { parts.add };
        let anchor = Rect::new(
            g.column.x + r.x,
            self.lane_top(g, row, lane) + r.y,
            r.w,
            r.h,
        );
        let current = replace.then(|| target.to_string());
        self.open_popup(PopupKind::AutomationTargets(track, replace), anchor);
        if let Some(p) = &mut self.popup
            && let Some(list) = p.pick.as_mut()
        {
            // The target being replaced, kept for the list's rebuild: the
            // rows' lit one and their `from` need it (UI-320).
            list.current = current.unwrap_or_default();
        }
        self.refresh_popup();
    }

    pub(crate) fn auto_target_choices(&self, track: TrackId, from: Option<&str>) -> Vec<Choice> {
        let p = self.project();
        let Some(t) = p.tracks.get(&track) else {
            return Vec::new();
        };
        let target = from.and_then(|s| s.parse::<AutoTarget>().ok());
        let titles: Vec<_> = t
            .inserts
            .iter()
            .flat_map(|x| {
                let mut list = self
                    .transport
                    .host()
                    .map(|h| h.plugins().param_titles(x.id))
                    .unwrap_or_default();
                if list.is_empty()
                    && let Some(e) = self.mixer.editors.iter().find(|e| e.id == x.id)
                {
                    list = e.param_titles();
                }
                list.into_iter().map(|(id, title)| (x.id, id, title))
            })
            .collect();
        choices(&p, track, target.as_ref(), &titles)
    }

    pub(crate) fn pick_auto_target(
        &mut self,
        g: &Geometry,
        track: TrackId,
        from: Option<String>,
        to: String,
    ) -> bool {
        self.popup = None;
        let (command, args) = match from {
            Some(from) => (
                "auto.lane.target",
                json!({"track":track,"target":from,"to":to}),
            ),
            None => ("auto.lane.add", json!({"track":track,"target":to})),
        };
        let changed = self.arrange_run(command, args).is_some();
        if changed {
            self.arrange.open.insert(track);
            self.arrange.touched();
            self.rebuild(g);
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::popup::Action;
    use std::sync::Arc;
    use xirius_core::auto::{AutoLane, AutoPoint};

    /// The target being replaced stays the lit row and the rows keep their
    /// `from` over every rebuild of the list (UI-320): `refresh_popup` runs
    /// after any command, so the list carries `current` itself.
    #[test]
    fn the_target_being_replaced_stays_over_a_rebuild() {
        let mut s = Studio::sample();
        let g = s.geometry(1680.0, 1000.0);
        let track = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Keys")
            .unwrap()
            .id;
        s.toggle_lanes(&g, track);
        s.lane_button(
            &g,
            track,
            &AutoTarget::Volume,
            crate::arrange::LaneButton::Param,
        );
        s.refresh_popup();
        let pick = s.popup.as_ref().unwrap().pick.as_ref().unwrap();
        let want = AutoTarget::Volume.to_string();
        assert_eq!(pick.current(), want);
        let lit = pick
            .groups
            .iter()
            .flat_map(|g| &g.rows)
            .find(|r| r.on)
            .expect("a lit row");
        assert_eq!(lit.to(), want);
        assert!(pick.groups.iter().flat_map(|g| &g.rows).all(
            |r| matches!(&r.act, Some(Action::AutomationTarget(_, Some(from), _)) if from == &want)
        ));
    }

    #[test]
    fn external_definitions_and_hidden_lanes_are_offered_without_saving_initial_values() {
        let mut s = Studio::sample();
        let track = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Keys")
            .unwrap()
            .id;
        assert!(s.run_command("plugin.add", json!({"track":track,"name":"EQ"})));
        let mut p = s.project().as_ref().clone();
        let t = p.track_mut(track).unwrap();
        let plugin = t.inserts.last_mut().unwrap();
        plugin.format = "vst3".into();
        plugin.uid = "0123456789ABCDEF0123456789ABCDEF".into();
        plugin.name = "External".into();
        plugin.params.clear();
        let id = plugin.id;
        let target = AutoTarget::Param(id, "Cutoff".into());
        let mut lane = AutoLane::new(target.clone());
        lane.hidden = true;
        lane.points = Arc::new(vec![AutoPoint::new(480, 0.75)]);
        t.automation.push(lane);
        let titles = [(id, 23, "Cutoff".into()), (id, 42, "Resonance".into())];
        let list = choices(&p, track, None, &titles);
        assert_eq!(
            list.iter().filter(|c| c.to == target.to_string()).count(),
            1
        );
        assert!(
            list.iter()
                .any(|c| c.to == format!("param:{id}:42") && c.label == "External · Resonance")
        );
        assert!(!list.iter().any(|c| c.to == format!("param:{id}:23")));
        assert!(p.tracks[&track].inserts.last().unwrap().params.is_empty());
        let current = choices(&p, track, Some(&target), &titles);
        assert!(current.iter().any(|c| c.to == format!("off:{target}")));
        let t = p.track_mut(track).unwrap();
        t.inserts
            .last_mut()
            .unwrap()
            .params
            .insert("23".into(), 0.5);
        t.inserts
            .last_mut()
            .unwrap()
            .params
            .insert("Cutoff".into(), 0.5);
        for hidden in [true, false] {
            p.track_mut(track).unwrap().automation[0].hidden = hidden;
            for from in [None, Some(&target)] {
                let list = choices(&p, track, from, &titles);
                let matching: Vec<_> = list
                    .iter()
                    .filter(|c| c.label == "External · Cutoff")
                    .collect();
                assert_eq!(matching.len(), 1);
                let prefix = if !hidden || from.is_some() {
                    "off:"
                } else {
                    ""
                };
                assert_eq!(matching[0].to, format!("{prefix}{target}"));
                assert!(
                    !list
                        .iter()
                        .any(|c| c.to.ends_with(&format!("param:{id}:23")))
                );
            }
        }
    }

    #[test]
    fn a_hidden_parameter_without_a_saved_key_reopens_with_its_points() {
        let mut s = Studio::sample();
        let track = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Keys")
            .unwrap()
            .id;
        assert!(s.run_command("plugin.add", json!({"track":track,"name":"EQ"})));
        let id = s.project().tracks[&track].inserts.last().unwrap().id;
        let target = AutoTarget::Param(id, "Untouched".into());
        assert!(s.run_command(
            "auto.point.add",
            json!({"track":track,"target":target.to_string(),"at":480,"value":0.75})
        ));
        let before = s.project().tracks[&track]
            .automation
            .iter()
            .find(|l| l.target == target)
            .unwrap()
            .points
            .clone();
        assert!(s.run_command(
            "auto.lane.hide",
            json!({"track":track,"target":target.to_string()})
        ));
        let g = s.geometry(1680.0, 1000.0);
        s.toggle_lanes(&g, track);
        s.open_auto_targets(&g, track, &AutoTarget::Volume, false);
        assert!(
            s.popup
                .as_ref()
                .unwrap()
                .choices()
                .iter()
                .any(|c| c.to == target.to_string())
        );
        assert!(s.popup_action(
            &g,
            Action::AutomationTarget(track, None, target.to_string())
        ));
        let p = s.project();
        let lane = p.tracks[&track]
            .automation
            .iter()
            .find(|l| l.target == target)
            .unwrap();
        assert!(!lane.hidden);
        assert_eq!(lane.points, before);
        assert!(
            !p.tracks[&track]
                .inserts
                .last()
                .unwrap()
                .params
                .contains_key("Untouched")
        );
    }
}
