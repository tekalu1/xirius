use crate::{
    browser::{Browser, RailTab},
    shot::{Act, Play, Shot, Target},
    studio::Studio,
};
use std::collections::BTreeSet;
use xirius_plugins::Catalog;

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "builtin01-instruments",
        acts: &[Act::Reset, Act::Stage(Play::Studio(prepare), 0)],
    },
    Shot {
        name: "builtin02-effects",
        acts: &[
            Act::Press(Target::Rail(RailTab::Effects)),
            Act::Stage(Play::Studio(effects), 0),
        ],
    },
    Shot {
        name: "builtin03-scanned",
        acts: &[Act::Stage(Play::Studio(scanned), 0)],
    },
];
fn instrument_names(b: &Browser) -> BTreeSet<&str> {
    b.library().iter().map(|i| i.name).collect()
}
fn prepare(s: &mut Studio, _: u8) {
    s.drawer_open = false;
    s.mixer.scan.auto = false;
    s.use_plugins(Catalog::default());
    assert_eq!(
        instrument_names(&s.browser),
        BTreeSet::from(["Sampler", "Drum Kit"])
    );
}
fn effects(s: &mut Studio, _: u8) {
    assert_eq!(s.browser.tab, RailTab::Effects);
    assert_eq!(
        instrument_names(&s.browser),
        BTreeSet::from(["EQ", "Compressor", "Reverb", "Delay", "Limiter"])
    );
}
fn catalog() -> Catalog {
    let base = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let direct = base.join("xirius_test_vst3.dll");
    let path = if direct.is_file() {
        direct
    } else {
        base.join("deps/xirius_test_vst3.dll")
    };
    assert!(path.is_file(), "build xirius-test-vst3 first");
    Catalog {
        entries: xirius_plugins::probe(&path).unwrap(),
        failures: Vec::new(),
    }
}
fn scanned(s: &mut Studio, _: u8) {
    s.use_plugins(catalog());
    assert_eq!(s.browser.library().len(), 6);
    assert!(instrument_names(&s.browser).contains("Xirius Test Latency"));
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::{LIB, OWN};
    #[test]
    fn a_fresh_browser_contains_only_the_seven_builtins_and_scan_adds_real_rows() {
        xirius_test_vst3::fixture_marker();
        assert_eq!(LIB.len(), 7);
        assert!(LIB.iter().all(|i| i.vendor == OWN && i.format == "内蔵"));
        let mut b = Browser::new();
        assert_eq!(
            instrument_names(&b),
            BTreeSet::from(["Sampler", "Drum Kit"])
        );
        b.press_rail(RailTab::Effects);
        assert_eq!(
            instrument_names(&b),
            BTreeSet::from(["EQ", "Compressor", "Reverb", "Delay", "Limiter"])
        );
        b.set_plugins(&catalog());
        assert_eq!(b.library().len(), 6);
        assert!(instrument_names(&b).contains("Xirius Test Latency"));
        b.set_plugins(&Catalog::default());
        assert_eq!(b.library().len(), 5);
    }
}
