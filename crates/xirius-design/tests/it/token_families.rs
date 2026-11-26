use xirius_design::token_names;

#[test]
fn every_token_belongs_to_an_allowed_family() {
    for token in token_names() {
        assert!(
            [
                "surface_", "fill_", "ink_", "line_", "meter_", "track_", "wave_", "alert_"
            ]
            .iter()
            .any(|family| token.starts_with(family)),
            "{token}"
        );
    }
}
