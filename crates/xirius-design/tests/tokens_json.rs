//! UI-042: what the code uses is what `tokens.json` says (the Rust is generated from it).

use serde_json::Value;
use xirius_design::{
    AI_LOOP_MS, Duration, Easing, Font, FontSize, Layout, LineHeight, Radius, Space, Weight,
    token_names,
};

fn tokens() -> Value {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tokens.json"))
        .expect("tokens.json");
    serde_json::from_str(&text).expect("tokens.json is JSON")
}

fn table(v: &Value, key: &str) -> Vec<(String, f64)> {
    v[key]
        .as_object()
        .unwrap_or_else(|| panic!("{key}"))
        .iter()
        .map(|(k, n)| (k.clone(), n.as_f64().unwrap()))
        .collect()
}

fn find(rows: &[(String, f64)], name: &str) -> f64 {
    rows.iter()
        .find(|(k, _)| k == name)
        .unwrap_or_else(|| panic!("{name} is not in tokens.json"))
        .1
}

#[test]
fn the_palette_has_the_colors_of_the_json_in_order() {
    let json: Vec<String> = tokens()["colors"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["name"].as_str().map(str::to_string))
        .collect();
    let code: Vec<String> = token_names().into_iter().map(str::to_string).collect();
    assert_eq!(code, json);
    assert!(code.len() > 60);
}

#[test]
fn spacing_radius_and_type_are_the_values_of_the_json() {
    let v = tokens();
    let space = table(&v, "space");
    for (s, name) in Space::ALL
        .into_iter()
        .zip(["X1", "X2", "X3", "X4", "X5", "X6"])
    {
        assert_eq!(s.px(), find(&space, name) as f32, "{name}");
    }
    let radius = table(&v, "radius");
    for (r, name) in Radius::ALL
        .into_iter()
        .zip(["Xs", "Clip", "S", "M", "L", "Pill"])
    {
        assert_eq!(r.px(), find(&radius, name) as f32, "{name}");
    }
    let sizes = table(&v, "font_size");
    for (s, name) in FontSize::ALL.into_iter().zip([
        "Micro", "Caption", "Small", "Body", "Lead", "Title", "Readout",
    ]) {
        assert_eq!(s.px(), find(&sizes, name) as f32, "{name}");
    }
    let lines = table(&v, "line_height");
    assert_eq!(LineHeight::Body.ratio(), find(&lines, "Body") as f32);
    assert_eq!(LineHeight::Dense.ratio(), find(&lines, "Dense") as f32);
    let weights = table(&v, "weight");
    assert_eq!(f64::from(Weight::Medium.value()), find(&weights, "Medium"));
    assert_eq!(v["font_family"]["NotoSansJp"], Font::NotoSansJp.family());
}

#[test]
fn motion_and_sizes_are_the_values_of_the_json() {
    let v = tokens();
    let ms = table(&v, "duration_ms");
    for (d, name) in Duration::ALL.into_iter().zip(["Fast", "Base", "Slow"]) {
        assert_eq!(f64::from(d.ms()), find(&ms, name), "{name}");
    }
    assert_eq!(f64::from(AI_LOOP_MS), v["ai_loop_ms"].as_f64().unwrap());
    let out = v["easing"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "OUT")
        .unwrap();
    let curve: Vec<f32> = out["curve"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_f64().unwrap() as f32)
        .collect();
    let Easing(code) = Easing::OUT;
    assert_eq!(code.to_vec(), curve);
    let layout = v["layout"].as_array().unwrap();
    let top = layout.iter().find(|e| e["name"] == "TOP_BAR_H").unwrap();
    assert_eq!(Layout::TOP_BAR_H, top["value"].as_f64().unwrap() as f32);
    assert!(layout.len() > 40);
}
