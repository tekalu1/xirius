//! Actual bundled-font measurements in the release window probe.
use super::*;
pub(crate) fn verify_widths(pen: &mut Pen) {
    thread_local! { static VERIFIED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
    if VERIFIED.with(|v| v.replace(true)) {
        return;
    }
    for name in [
        "history/42.json",
        "versions/2026-10-24/session/history/archive/alternate-takes/history/42.json",
        "media/recordings/Chorus_Lead_Vocal_Double_Alternate_Comp_Take_042_Final_96kHz.wav",
        "media/録音/春のストリングス🎻_第２テイク_音量調整前_最終確認用.wav",
        "media/か\u{3099}くふ_👨‍👩‍👧‍👦_e\u{301}_🇯🇵_long-long-name.wav",
    ] {
        for width in [328.0, 260.0, 80.0, 1.0] {
            let fitted = shorten(name, width, |t| {
                pen.measure(t, FontSize::Small, Weight::Regular)
            });
            assert!(
                pen.measure(&fitted, FontSize::Small, Weight::Regular) <= width,
                "{fitted}"
            );
            if let Some((left, right)) = fitted.split_once('…') {
                assert!(
                    name.grapheme_indices(true)
                        .any(|(i, _)| name[..i].ends_with(left))
                        || left.is_empty()
                );
                if !right.starts_with('/') && !right.is_empty() {
                    assert!(
                        name.grapheme_indices(true)
                            .any(|(i, _)| &name[i..] == right)
                    );
                }
            }
        }
    }
    eprintln!("PRJ-029: bundled-font width checks passed (20 cases)");
}
