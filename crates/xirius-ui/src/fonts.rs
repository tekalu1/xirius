//! The bundled faces (UI-002).
//!
//! Inter and Noto Sans JP live in `assets/fonts/` and are compiled into the
//! binary, so the screen looks the same on every machine and never depends on
//! the fonts the OS happens to have. `xirius_design::FONT_FILES` is the table of
//! what must be there; the tests below fail when a file is missing from the
//! binary, differs from `SHA256SUMS`, or loses its licence text.

use std::borrow::Cow;

use gpui::App;

macro_rules! embed {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_bytes!(concat!("../../../assets/fonts/", $name)) as &[u8])),*]
    };
}

/// `(file name, bytes)` of every bundled face, in the order of `FONT_FILES`.
pub const EMBEDDED: &[(&str, &[u8])] = embed!(
    "Inter-Regular.ttf",
    "Inter-Medium.ttf",
    "Inter-SemiBold.ttf",
    "NotoSansJP-Regular.ttf",
    "NotoSansJP-Medium.ttf",
    "NotoSansJP-SemiBold.ttf",
);

/// The licence texts that ship with the faces.
pub const LICENSES: &[(&str, &str)] = &[
    (
        "OFL-Inter.txt",
        include_str!("../../../assets/fonts/OFL-Inter.txt"),
    ),
    (
        "OFL-NotoSansJP.txt",
        include_str!("../../../assets/fonts/OFL-NotoSansJP.txt"),
    ),
];

/// `SHA256SUMS` of `assets/fonts/` (`sha256sum` format).
pub const CHECKSUMS: &str = include_str!("../../../assets/fonts/SHA256SUMS");

/// Registers the bundled faces with GPUI. Call once, before the first window.
pub fn load(cx: &App) -> Result<(), String> {
    cx.text_system()
        .add_fonts(
            EMBEDDED
                .iter()
                .map(|(_, bytes)| Cow::Borrowed(*bytes))
                .collect(),
        )
        .map_err(|e| e.to_string())?;
    METRICS.with(|slot| {
        *slot.borrow_mut() = Some(gpui::WindowTextSystem::new(cx.text_system().clone()))
    });
    Ok(())
}

thread_local! {
    static METRICS: std::cell::RefCell<Option<gpui::WindowTextSystem>> = const { std::cell::RefCell::new(None) };
}

/// The same native shaping, bundled faces, size and runs as Pen::text.
/// Tests initialize a native text system without opening a window; the application uses its registered text system.
pub(crate) fn measure(
    text: &str,
    size: xirius_design::FontSize,
    weight: xirius_design::Weight,
) -> f32 {
    METRICS.with(|slot| {
        let mut slot = slot.borrow_mut();
        let system = slot.get_or_insert_with(|| {
            let system = std::sync::Arc::new(gpui::TextSystem::new(
                gpui_platform::current_platform(false).text_system(),
            ));
            system
                .add_fonts(
                    EMBEDDED
                        .iter()
                        .map(|(_, bytes)| Cow::Borrowed(*bytes))
                        .collect(),
                )
                .expect("bundled fonts");
            gpui::WindowTextSystem::new(system)
        });
        f32::from(
            system
                .shape_line(
                    text.to_owned().into(),
                    gpui::px(size.px()),
                    &crate::theme::text_runs(
                        text,
                        weight,
                        false,
                        crate::theme::paint(
                            xirius_design::Surface::Paper
                                .text()
                                .ink_color(crate::theme::THEME),
                        ),
                    ),
                    None,
                )
                .width,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use xirius_design::FONT_FILES;

    fn hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    #[test]
    fn every_face_in_the_design_table_is_embedded() {
        assert_eq!(EMBEDDED.len(), FONT_FILES.len());
        for (file, (name, _)) in FONT_FILES.iter().zip(EMBEDDED) {
            assert_eq!(file.file_name, *name);
        }
    }

    #[test]
    fn embedded_files_are_truetype() {
        for (name, bytes) in EMBEDDED {
            assert_eq!(
                bytes[..4],
                [0x00, 0x01, 0x00, 0x00],
                "{name} is not a TrueType file"
            );
            assert!(bytes.len() > 100_000, "{name} is suspiciously small");
        }
    }

    #[test]
    fn embedded_files_match_the_checksums() {
        for (name, bytes) in EMBEDDED {
            let line = CHECKSUMS
                .lines()
                .find(|l| l.ends_with(name))
                .unwrap_or_else(|| panic!("{name} is missing from SHA256SUMS"));
            let (sum, _) = line.split_once(' ').expect("sha256sum line");
            assert_eq!(sum, hex(bytes), "{name} differs from SHA256SUMS");
        }
        for (name, text) in LICENSES {
            let line = CHECKSUMS
                .lines()
                .find(|l| l.ends_with(name))
                .unwrap_or_else(|| panic!("{name} is missing from SHA256SUMS"));
            let (sum, _) = line.split_once(' ').expect("sha256sum line");
            assert_eq!(sum, hex(text.as_bytes()), "{name} differs from SHA256SUMS");
        }
    }

    #[test]
    fn each_face_ships_with_its_licence() {
        for (name, text) in LICENSES {
            assert!(
                text.to_uppercase().contains("SIL OPEN FONT LICENSE"),
                "{name} is not the OFL"
            );
        }
        assert!(LICENSES[0].1.contains("Inter"));
        assert!(LICENSES[1].1.contains("Adobe"));
    }
}
