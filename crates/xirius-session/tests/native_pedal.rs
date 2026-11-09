//! Continuous damper ownership through the real VST3 SDK parameter adapter.
use std::sync::Arc;
use xirius_engine::performance::{Input, Instrument, midi_source};
use xirius_engine::plugin::Vst3Processor;
use xirius_engine::processor::{AudioIo, NoteEvent, NoteKind, ProcessSetup, Processor, RenderMode};
use xirius_vst3::{Module, Plugin};

fn rendered_value(instrument: &mut Instrument, timeline: &[NoteEvent], expected: i32) {
    let mut left = [0.0; 512];
    let mut right = [0.0; 512];
    left[0] = 1.0;
    right[0] = 1.0;
    instrument.process(&mut AudioIo::new(&mut left, &mut right), timeline);
    let delay = (expected as f64 / 127.0 * 256.0).round() as usize;
    assert_eq!(
        left.iter().position(|v| *v > 0.5),
        Some(delay),
        "native CC64 value {expected}"
    );
    assert_eq!(left, right);
}

#[test]
fn continuous_pedal_reaches_native_processor_and_releases_only_the_departing_source() {
    xirius_test_vst3::fixture_marker();
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let module = Arc::new(Module::load(&xirius_test_vst3::fixture_path(&deps)).unwrap());
    let class = module
        .classes()
        .into_iter()
        .find(|c| c.is_audio_processor())
        .unwrap();
    let mut plugin = Plugin::new(module, &class).unwrap();
    let mut instrument = Instrument::new(7, Box::new(Vst3Processor::new(plugin.audio().unwrap())))
        .with_native_pedal();
    instrument.prepare(&ProcessSetup {
        sample_rate: 48_000.0,
        max_block: 512,
        mode: RenderMode::Offline,
    });
    let a = midi_source(0, 0);
    let b = midi_source(1, 3);
    let send = |p: &mut Instrument, source, number, value| {
        assert!(p.live_input(Input {
            source,
            plugin: None,
            kind: NoteKind::control(number, value)
        }));
    };
    for value in [1, 63, 64, 100, 127, 0] {
        send(&mut instrument, a, 64, value);
        rendered_value(&mut instrument, &[], value);
    }
    send(&mut instrument, a, 64, 100);
    send(&mut instrument, b, 64, 63);
    rendered_value(&mut instrument, &[], 100);
    send(&mut instrument, b, 64, 0);
    rendered_value(&mut instrument, &[], 100);
    send(&mut instrument, b, 64, 127);
    rendered_value(&mut instrument, &[], 127);
    send(&mut instrument, b, 123, 0);
    rendered_value(&mut instrument, &[], 100);
    send(&mut instrument, a, 64, 0);
    rendered_value(&mut instrument, &[], 0);
    for value in [1, 63, 64, 100, 127, 0] {
        rendered_value(
            &mut instrument,
            &[NoteEvent {
                offset: 0,
                kind: NoteKind::control(64, value),
            }],
            value,
        );
    }
    send(&mut instrument, b, 64, 127);
    instrument.release_live_inputs();
    rendered_value(&mut instrument, &[], 0);
}
