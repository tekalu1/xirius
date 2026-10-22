//! EDT-134: the names a plugin gives its keys, read through `IUnitInfo`.

use std::path::PathBuf;
use std::sync::Arc;

use xirius_vst3::{Module, Plugin};

fn fixture() -> Plugin {
    xirius_test_vst3::fixture_marker();
    let deps: PathBuf = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let module = Arc::new(Module::load(&deps.join("xirius_test_vst3.dll")).expect("load"));
    let class = module.classes().into_iter().next().expect("a class");
    Plugin::new(module, &class).expect("create")
}

#[test]
fn edt_134_the_note_names_are_the_selected_programs_pitch_names() {
    let want: Vec<(u8, String)> = xirius_test_vst3::NOTE_NAMES
        .iter()
        .map(|&(k, n)| (k, n.to_owned()))
        .collect();
    assert_eq!(fixture().note_names(), want);
}
