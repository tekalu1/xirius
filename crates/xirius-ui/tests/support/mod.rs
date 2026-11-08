//! Files used by the built-in sample, under each window test's own working directory.
pub fn sample_audio(root: &std::path::Path) {
    let silence = vec![0.0; 48_000];
    for name in ["vocal_take1.wav", "guitar_di.wav"] {
        xirius_session::export::write_wav(&root.join(name), &silence, &silence, 48_000, 16)
            .unwrap();
    }
}
