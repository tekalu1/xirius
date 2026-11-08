use super::Run;
use crate::studio::{Geometry, Studio};

impl Studio {
    pub(crate) fn context_navigation(&mut self, g: &Geometry, key: &str) -> bool {
        if !self.context_open() {
            return false;
        }
        let Some(state) = self.ctx.as_mut() else {
            return false;
        };
        if state.ask.is_some() || state.field.is_some() {
            return false;
        }
        if state
            .search
            .as_ref()
            .is_some_and(|s| !s.field.marked.is_empty())
        {
            return false;
        }
        let selectable: Vec<_> = state
            .items
            .iter()
            .enumerate()
            .filter(|(_, row)| row.enabled && !matches!(row.run, Run::Gap | Run::Title))
            .map(|(i, _)| i)
            .collect();
        match key {
            "up" | "down" => {
                if !selectable.is_empty() {
                    let at = state
                        .cursor
                        .and_then(|i| selectable.iter().position(|n| *n == i));
                    let next = match (key, at) {
                        ("up", Some(n)) => (n + selectable.len() - 1) % selectable.len(),
                        ("up", None) => selectable.len() - 1,
                        (_, Some(n)) => (n + 1) % selectable.len(),
                        _ => 0,
                    };
                    state.cursor = Some(selectable[next]);
                }
                self.refresh_popup();
            }
            "enter" | "right" => {
                if let Some(i) = state.cursor.or_else(|| selectable.first().copied()) {
                    if key == "enter" || matches!(state.items[i].run, Run::Sub(_)) {
                        self.context_pick(g, i);
                    }
                }
            }
            "left" | "escape" => {
                if key == "escape"
                    && state
                        .search
                        .as_ref()
                        .is_some_and(|s| !s.field.value.is_empty())
                {
                    return false; // The search field clears first.
                }
                if !state.trail.is_empty() {
                    if let Some(i) = state.items.iter().position(|i| i.run == Run::Back) {
                        self.context_pick(g, i);
                    }
                } else if key == "escape" {
                    return false; // The field closes the root.
                }
            }
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::context::Item;
    use serde_json::json;

    #[test]
    fn arrows_skip_disabled_rows_and_restore_parent_cursor() {
        let (mut s, g) = crate::context::tests::studio();
        let leaf = Item::ask("名前", "clip.rename", json!({"id":"c7"}), "name", "old");
        s.open_context(
            vec![leaf.clone().enabled(false), Item::sub("編集", vec![leaf])],
            400.0,
            300.0,
        );
        assert!(s.typed(&g, "down", None));
        assert_eq!(s.ctx.as_ref().unwrap().cursor, Some(1));
        assert!(s.typed(&g, "right", None));
        assert_eq!(s.ctx.as_ref().unwrap().trail.len(), 1);
        assert!(s.typed(&g, "left", None));
        assert_eq!(s.ctx.as_ref().unwrap().cursor, Some(1));
        assert!(s.typed(&g, "escape", None));
        assert!(!s.context_open());
    }
}
