//! Long lists use the menu's input field to filter their rows.
use super::{Item, Run};
use crate::settings::{FieldId, field::Field};
use crate::studio::Studio;

#[derive(Clone, Debug)]
pub struct Search {
    pub field: Field,
    pub pool: Vec<Item>,
}
impl Search {
    pub fn of(items: &[Item]) -> Option<Search> {
        (items
            .iter()
            .filter(|i| !matches!(i.run, Run::Back | Run::Gap) && i.swatch.is_none())
            .count()
            > 10)
            .then(|| Search {
                field: Field::new(FieldId::Search, String::new()),
                pool: items.to_vec(),
            })
    }
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
        let Some(search) = st.search.as_ref() else {
            return;
        };
        let query = search.field.value.trim().to_lowercase();
        st.items = if query.is_empty() {
            search.pool.clone()
        } else {
            let mut found = Vec::new();
            let mut gap = false;
            for item in &search.pool {
                if matches!(item.run, Run::Gap) {
                    gap = !found.is_empty();
                } else if matches!(item.run, Run::Back)
                    || item.label.to_lowercase().contains(&query)
                {
                    if gap {
                        found.push(Item::gap());
                    }
                    found.push(item.clone());
                    gap = false;
                }
            }
            found
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn filtering_keeps_the_separator_before_the_delete_group() {
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
        assert!(matches!(items[items.len() - 2].run, Run::Gap));
        assert!(!matches!(items[0].run, Run::Gap));
    }
}
