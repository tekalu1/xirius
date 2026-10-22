//! The Windows minimise, maximise / restore and close buttons at the right end of the top bar
//! (UI-005). Their icons are bundled SVGs, so the buttons draw the same on every machine and
//! name no OS font.

use gpui::{SharedString, WindowControlArea};
use xirius_design::FontSize;

use crate::assets::icon;

/// Side of the icon square: the em of the micro size, the size the system draws its own at.
pub fn icon_side() -> f32 {
    FontSize::Micro.px()
}

/// The three buttons left to right, each with its icon; maximise shows restore once maximised.
pub fn buttons(maximized: bool) -> [(WindowControlArea, SharedString); 3] {
    [
        (WindowControlArea::Min, icon("caption-minimise")),
        (
            WindowControlArea::Max,
            icon(if maximized {
                "caption-restore"
            } else {
                "caption-maximise"
            }),
        ),
        (WindowControlArea::Close, icon("caption-close")),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::Assets;
    use gpui::AssetSource;

    #[test]
    fn every_button_icon_is_bundled() {
        for maximized in [false, true] {
            for (_, path) in buttons(maximized) {
                assert!(Assets.load(&path).unwrap().is_some(), "{path}");
            }
        }
    }

    #[test]
    fn the_middle_button_restores_once_maximised() {
        assert!(buttons(false)[1].1.contains("maximise"));
        assert!(buttons(true)[1].1.contains("restore"));
    }
}
