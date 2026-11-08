//! Search all descendants without changing the operation or its enabled state.
use super::{Item, Run};
use crate::settings::{FieldId, field::Field};
use crate::studio::Studio;

#[derive(Clone, Debug)]
pub struct Search {
    pub field: Field,
    pub pool: Vec<Item>,
    /// Complete classified tree, independent of the currently open child.
    pub index: Vec<Item>,
    pub paths: Vec<String>,
}
impl Search {
    pub fn of(items: &[Item]) -> Option<Search> {
        Some(Search {
            field: Field::new(FieldId::Search, String::new()),
            pool: items.to_vec(),
            index: items.to_vec(),
            paths: Vec::new(),
        })
    }
}

/// The path is separate from the label, so picking a result executes the original leaf.
pub fn find(items: &[Item], query: &str) -> Vec<(Item, String)> {
    fn visit(items: &[Item], path: &[String], query: &str, out: &mut Vec<(Item, String)>) {
        for item in items {
            if let Run::Sub(children) = &item.run {
                let mut next = path.to_vec();
                next.push(item.label.clone());
                let start = out.len();
                visit(children, &next, query, out);
                if !item.enabled {
                    for (leaf, _) in &mut out[start..] {
                        leaf.enabled = false;
                        if leaf.reason.is_empty() {
                            leaf.reason = item.reason.clone();
                        }
                    }
                }
            } else if !matches!(item.run, Run::Gap | Run::Title | Run::Back) {
                let breadcrumb = path.join(" › ");
                let label = item
                    .swatch
                    .map_or_else(|| item.label.clone(), |n| format!("色 {:02}", n + 1));
                if format!("{label} {breadcrumb}")
                    .to_lowercase()
                    .contains(query)
                {
                    let mut found = item.clone();
                    found.label = label;
                    out.push((
                        found,
                        if breadcrumb.is_empty() {
                            "直下".into()
                        } else {
                            breadcrumb
                        },
                    ));
                }
            }
        }
    }
    let mut found = Vec::new();
    visit(items, &[], &query.trim().to_lowercase(), &mut found);
    found
}

impl Studio {
    pub(crate) fn context_query(&self) -> Option<String> {
        self.ctx.as_ref()?.search.as_ref().map(|s| s.field.text())
    }
    pub(super) fn filter_context(&mut self) {
        let Some(st) = self.ctx.as_mut() else {
            return;
        };
        if st.ask.is_some() {
            return;
        }
        let Some(search) = st.search.as_mut() else {
            return;
        };
        let query = search.field.value.trim().to_lowercase();
        st.cursor = None;
        st.items = if query.is_empty() {
            search.paths.clear();
            search.pool.clone()
        } else {
            let (items, paths) = find(&search.index, &query).into_iter().unzip();
            search.paths = paths;
            items
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn results_are_leaves_without_group_separators() {
        let mut s = Studio::sample();
        let mut rows: Vec<_> = (0..12)
            .map(|n| s.ctx_as(&format!("Marker {n}"), "marker.add", json!({"at":n*3840})))
            .collect();
        rows.push(Item::gap());
        rows.push(s.ctx_as("Delete marker", "marker.delete", json!({"ids":["m999"]})));
        s.open_context(rows, 400.0, 300.0);
        s.ask_text(None, "marker", false);
        let items = &s.ctx.as_ref().unwrap().items;
        assert_eq!(items.last().unwrap().label, "Delete marker");
        assert!(items.iter().all(|i| !matches!(i.run, Run::Gap)));
    }

    #[test]
    fn deep_results_keep_arguments_shortcut_and_disabled_reason() {
        let mut leaf = Item::ask(
            "名前を変更…",
            "clip.rename",
            json!({"id":"c7"}),
            "name",
            "old",
        );
        leaf.key = "F2".into();
        leaf.enabled = false;
        leaf.reason = "フリーズ中".into();
        let tree = vec![Item::sub(
            "編集",
            vec![Item::sub("対象", vec![leaf.clone()])],
        )];
        let found = find(&tree, "編集");
        assert_eq!(found, vec![(leaf.clone(), "編集 › 対象".into())]);
        assert_eq!(find(&tree, "名前")[0].0, leaf);
        assert!(find(&tree, "見つからない").is_empty());
    }

    #[test]
    fn disabled_parent_never_turns_into_an_executable_result() {
        let leaf = Item::ask(
            "名前を変更…",
            "clip.rename",
            json!({"id":"c7"}),
            "name",
            "old",
        );
        let mut parent = Item::sub("編集", vec![leaf]).enabled(false);
        parent.reason = "対象なし".into();
        let found = find(&[parent], "名前");
        assert!(!found[0].0.enabled);
        assert_eq!(found[0].0.reason, "対象なし");
    }

    #[test]
    fn opening_and_searching_keeps_the_original_deep_label_and_runs_the_leaf() {
        let (mut s, g) = crate::context::tests::studio();
        let leaf = Item::ask(
            "名前を変更…",
            "clip.rename",
            json!({"id":"c7"}),
            "name",
            "old",
        );
        s.open_context(
            vec![Item::sub("編集", vec![Item::sub("対象", vec![leaf])])],
            400.0,
            300.0,
        );
        s.ask_text(None, "対象", false);
        let rows = s.context_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "名前を変更…");
        assert_eq!(rows[0].path, "編集 › 対象");
        assert!(s.context_pick(&g, 0));
        let ask = s.ctx.as_ref().unwrap().ask.as_ref().unwrap();
        assert_eq!(ask.name, "clip.rename");
        assert_eq!(ask.args, json!({"id":"c7"}));
    }

    #[test]
    fn root_search_is_always_present_and_escape_clears_before_closing() {
        let mut s = Studio::sample();
        let row = s.ctx_as("マーカーを追加", "marker.add", json!({"at":0}));
        s.open_context(vec![row], 400.0, 300.0);
        assert_eq!(s.context_query().as_deref(), Some(""));
        s.ask_text(None, "マーカー", false);
        s.ask_key("escape");
        assert!(s.context_open());
        assert_eq!(s.context_query().as_deref(), Some(""));
        s.ask_key("escape");
        assert!(!s.context_open());
    }
}
