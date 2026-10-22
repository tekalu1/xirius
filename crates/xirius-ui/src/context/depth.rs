use super::{Item, Run};

pub(super) fn flatten(rows: Vec<Item>, prefix: &str) -> Vec<Item> {
    let mut out = Vec::new();
    for mut row in rows {
        let label = if prefix.is_empty() {
            row.label.clone()
        } else {
            format!("{prefix} · {}", row.label)
        };
        if let Run::Sub(children) = row.run {
            if !out.is_empty() {
                out.push(Item::gap());
            }
            out.extend(flatten(children, &label));
        } else {
            if !matches!(row.run, Run::Gap) && row.swatch.is_none() {
                row.label = label;
            }
            out.push(row);
        }
    }
    out
}
