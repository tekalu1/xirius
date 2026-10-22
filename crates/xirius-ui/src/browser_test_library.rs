use super::{Browser, LIB, fx, inst};

pub(crate) fn example() -> Browser {
    let mut browser = Browser::new();
    let mut items = LIB.to_vec();
    items.extend([
        inst("Vital", "keys", "Vital Audio", "VST3", "シンセ"),
        inst("Surge XT", "keys", "Surge Synth Team", "CLAP", "シンセ"),
        inst("Dexed", "keys", "Digital Suburban", "AU", "シンセ"),
        fx("Valhalla Supermassive", "Valhalla DSP", "VST3", "空間"),
        fx("TDR Nova", "Tokyo Dawn Records", "VST3", "EQ"),
    ]);
    browser.items = Some(items);
    browser
}
