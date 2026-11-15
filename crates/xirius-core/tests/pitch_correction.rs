use xirius_core::model::Plugin;

#[test]
fn song_round_trip_keeps_relative_keyboard_and_all_parameters() {
    let def = xirius_params::find("pitch-correction").unwrap();
    let mask = 0b1010_1011_0100;
    let scale = xirius_params::pitch_correction::custom_norm(mask);
    let json = serde_json::json!({
        "id": "plg_1", "name": "Pitch Correction", "kind": "effect",
        "format": "builtin", "uid": "pitch-correction", "bypass": true,
        "params": {
            "key": 0.25, "scale": scale, "speed": 0.1,
            "transition": 0.2, "amount": 0.5, "formant": 1.0, "range": 0.5
        }
    });
    let saved: Plugin = serde_json::from_value(json).unwrap();
    assert_eq!(saved.builtin().unwrap().uid, def.uid);
    assert_eq!(saved.params.len(), def.params.len());
    let reopened: Plugin = serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    for (id, value) in &saved.params {
        assert!((reopened.params[id] - value).abs() < 1e-14, "{id}");
    }
    assert_eq!(reopened.uid, saved.uid);
    assert!(reopened.bypass);
    assert_eq!(
        xirius_params::pitch_correction::scale_mask(reopened.params["scale"]),
        mask
    );
}
