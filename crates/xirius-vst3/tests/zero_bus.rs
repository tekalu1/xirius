//! PLG-001: exercise the real VST3 ABI with plugins that overread a zero bus count.
use std::sync::Arc;
use xirius_vst3::{Module, Plugin};

#[test]
fn zero_bus_compatibility_preserves_negotiation_and_process_counts() {
    xirius_test_vst3::fixture_marker();
    let dll = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("xirius_test_vst3.dll");
    let module = Arc::new(Module::load(&dll).unwrap());
    let class = module.classes().into_iter().next().unwrap();
    for (mode, inputs, outputs) in [
        (2, vec![], vec![2]),
        (3, vec![2], vec![]),
        (4, vec![1], vec![1]),
        (5, vec![2, 2], vec![2]),
    ] {
        let mut class = class.clone();
        class.cid = xirius_test_vst3::bus_layout_cid(mode);
        let mut plugin = Plugin::new(module.clone(), &class).unwrap();
        let mut audio = plugin.audio().unwrap();
        // Repeat preparation to check the compatibility buffers survive reconfiguration.
        for _ in 0..2 {
            audio.prepare(48_000.0, 64, true).unwrap();
            assert_eq!(audio.input_channels(), inputs);
            assert_eq!(audio.output_channels(), outputs);
            for _ in 0..3 {
                let (mut left, mut right) = ([0.0; 64], [0.0; 64]);
                audio.process(&mut left, &mut right, &[]);
                assert_eq!(audio.counts().0, 0, "mode {mode}");
                if !outputs.is_empty() {
                    assert_eq!(left, [0.25; 64]);
                    assert_eq!(right, [0.25; 64]);
                }
            }
        }
    }
    // Ordinary stereo effect retains both its arrangement and its audio path.
    let mut plugin = Plugin::new(module, &class).unwrap();
    let mut audio = plugin.audio().unwrap();
    audio.prepare(48_000.0, 64, true).unwrap();
    assert_eq!(audio.input_channels(), [2]);
    assert_eq!(audio.output_channels(), [2]);
    let (mut left, mut right) = ([0.125; 64], [-0.25; 64]);
    audio.process(&mut left, &mut right, &[]);
    assert_eq!(audio.counts().0, 0);
    assert_eq!(left, [0.125; 64]);
    assert_eq!(right, [-0.25; 64]);
}
