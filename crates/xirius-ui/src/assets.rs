//! The SVGs the screen uses, compiled into the binary.
//!
//! GPUI draws an SVG as a mask and colours it with the text colour, so the
//! files carry no colour of their own: the icon takes the ink of whatever it
//! sits on. `logo-dark.svg` is the brand wordmark from `assets/brand/`.

use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_bytes!(concat!("../assets/icons/", $name, ".svg")) as &[u8])),*]
    };
}

const ICONS: &[(&str, &[u8])] = icons!(
    "audio",
    "chevron-down",
    "chevron-right",
    "chevron-up",
    "export",
    "folder",
    "instrument",
    "line",
    "loop",
    "mixer",
    "pencil",
    "play",
    "plus",
    "record",
    "redo",
    "roll",
    "select",
    "select-extended",
    "skip-back",
    "stop",
    "undo",
);

/// The icons of the top bar's menus, the track head, the browser and the settings.
const ICONS_HEAD: &[(&str, &[u8])] = icons!(
    "bolt",
    "range",
    "cut",
    "eraser",
    "follow",
    "mute",
    "listen",
    "solo",
    "lock",
    "zoom-in",
    "zoom-out",
    "wave-zoom",
    "ai",
    "ai-spark",
    "ai-star",
    "power",
    "gauge",
    "am-latch",
    "am-read",
    "am-touch",
    "am-write",
    "arrange",
    "bus",
    "cable",
    "check",
    "close",
    "drum",
    "ear",
    "monitor",
    "effects",
    "eye",
    "file",
    "gear",
    "grid",
    "magnet",
    "magnet-off",
    "history",
    "input",
    "keys",
    "metronome",
    "mic",
    "more",
    "open",
    "repo",
    "download",
    "person",
    "row-grow",
    "row-shrink",
    "output",
    "restore",
    "saved",
    "search",
    "send",
    "star",
    "star-fill",
    "tempo",
    "view",
    "volume",
    "wave",
    "sync",
    "spinner",
    "folder-open",
    "trash",
    "alert",
    "sun",
    "moon",
    "shield",
    "plug",
    "piano",
    "kbd",
    "latency",
    "swap",
    "unlink",
    "layers",
    "midi",
    "quant",
    "drop",
    "shape-free",
    "shape-line",
    "shape-parabola",
    "shape-square",
    "shape-triangle",
    "shape-saw",
    "shape-sine",
    "shape-transform",
    "freeze",
    "stretch",
    "reverse",
    "preroll",
    "sliders",
    "song-settings",
    "dot",
    "velocity",
    "unlock",
    "crosshair",
    "snap-to",
    "info",
    "inspector",
    "prompt",
    "chevron-left",
    "tone",
    "ripple",
    "pool",
    "pin",
    "route",
    "ins",
);

/// The Windows minimise, maximise, restore and close buttons (UI-005): filled shapes on a 10 px
/// grid in the size and weight of the system's own, so the buttons need no OS icon font.
const ICONS_CAPTION: &[(&str, &[u8])] = icons!(
    "caption-minimise",
    "caption-maximise",
    "caption-restore",
    "caption-close",
);

/// GitHub's mark, as Octicons draws it (MIT, `assets/octicons/`): the about panel's button to
/// the repository (UI-303).
const ICONS_OCTICONS: &[(&str, &[u8])] = &[(
    "github",
    include_bytes!("../assets/octicons/mark-github-16.svg"),
)];

/// The version of Octicons the mark is from, and its licence text.
pub const OCTICONS_VERSION: &str = "19.41.0";
pub const OCTICONS_LICENSE: &str = include_str!("../assets/octicons/LICENSE");

pub(crate) fn all_icons() -> impl Iterator<Item = &'static (&'static str, &'static [u8])> {
    ICONS
        .iter()
        .chain(ICONS_HEAD)
        .chain(ICONS_CAPTION)
        .chain(ICONS_OCTICONS)
}

const LOGO: &[u8] = include_bytes!("../../../assets/brand/logo-dark.svg");

/// Size of the wordmark's artwork, for working out its width at a given height.
pub const LOGO_ASPECT: f32 = 728.0 / 144.0;

pub const LOGO_PATH: &str = "brand/logo-dark.svg";

/// The brand's star in two colours (`assets/brand/mark.svg`). An SVG is drawn as a mask in one
/// colour, so each of its shapes is served on its own, `brand/mark-<n>.svg`, and drawn in its
/// colour over the one before (UI-303).
const MARK: &str = include_str!("../../../assets/brand/mark.svg");

pub fn mark_layer(n: usize) -> SharedString {
    SharedString::from(format!("brand/mark-{n}.svg"))
}

/// The `n`-th shape of the mark, alone in an SVG of the mark's size.
fn mark_svg(n: usize) -> Option<String> {
    let view = MARK.split("viewBox=\"").nth(1)?.split('"').next()?;
    let path = MARK.split("<path ").nth(n + 1)?.split("/>").next()?;
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{view}\"><path {path}/></svg>"
    ))
}

/// How many shapes the mark has.
pub const MARK_LAYERS: usize = 2;

/// `icons/<name>.svg`
pub fn icon(name: &str) -> SharedString {
    SharedString::from(format!("icons/{name}.svg"))
}

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if path == LOGO_PATH {
            return Ok(Some(Cow::Borrowed(LOGO)));
        }
        if let Some(n) = path
            .strip_prefix("brand/mark-")
            .and_then(|rest| rest.strip_suffix(".svg"))
            .and_then(|n| n.parse::<usize>().ok())
            .filter(|n| *n < MARK_LAYERS)
        {
            return Ok(mark_svg(n).map(|svg| Cow::Owned(svg.into_bytes())));
        }
        let bytes = path
            .strip_prefix("icons/")
            .and_then(|rest| rest.strip_suffix(".svg"))
            .and_then(|name| all_icons().find(|(n, _)| *n == name))
            .map(|(_, bytes)| Cow::Borrowed(*bytes));
        Ok(bytes)
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        if path.trim_end_matches('/') != "icons" {
            return Ok(Vec::new());
        }
        Ok(all_icons()
            .map(|(name, _)| SharedString::from(format!("icons/{name}.svg")))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_loads_and_is_an_svg() {
        for (name, _) in all_icons() {
            let bytes = Assets
                .load(&format!("icons/{name}.svg"))
                .unwrap()
                .unwrap_or_else(|| panic!("{name} is not served"));
            assert!(bytes.starts_with(b"<svg"), "{name}");
        }
    }

    #[test]
    fn line_icons_use_the_approved_stroke_and_solo_has_a_drawing() {
        for (name, bytes) in all_icons() {
            let svg = std::str::from_utf8(bytes).unwrap();
            if svg.contains("stroke-width=") {
                assert!(svg.contains("stroke-width=\"1.6\""), "{name}");
            }
        }
        assert!(Assets.load("icons/solo.svg").unwrap().is_some());
    }

    #[test]
    fn no_two_icons_are_the_same_drawing() {
        // The inspector's icon is kept under its own name with the shape of `info` for now: the
        // rail and its balloon tell it from the notice's (UI-312).
        let allowed =
            |a: &str, b: &str| matches!((a, b), ("info", "inspector") | ("inspector", "info"));
        let all: Vec<_> = all_icons().collect();
        for (i, (a, abytes)) in all.iter().enumerate() {
            for (b, bbytes) in &all[i + 1..] {
                if allowed(a, b) {
                    continue;
                }
                assert_ne!(abytes, bbytes, "{a} and {b} are the same SVG");
            }
        }
    }

    #[test]
    fn the_icons_the_screen_names_for_settings_and_the_crash_pages_are_there() {
        for name in [
            "gear",
            "song-settings",
            "chevron-left",
            "chevron-right",
            "sun",
        ] {
            assert!(
                Assets.load(&format!("icons/{name}.svg")).unwrap().is_some(),
                "{name}"
            );
        }
    }

    #[test]
    fn the_mark_is_served_shape_by_shape() {
        for n in 0..MARK_LAYERS {
            let bytes = Assets.load(&mark_layer(n)).unwrap().unwrap();
            let svg = std::str::from_utf8(&bytes).unwrap();
            assert!(svg.starts_with("<svg") && svg.contains("viewBox=\"0 0 512 512\""));
            assert_eq!(svg.matches("<path ").count(), 1, "{svg}");
        }
        assert_eq!(MARK.matches("<path ").count(), MARK_LAYERS);
        assert!(Assets.load("brand/mark-2.svg").unwrap().is_none());
    }

    #[test]
    fn the_github_mark_is_octicons_unchanged_with_its_licence() {
        let svg =
            std::str::from_utf8(all_icons().find(|(n, _)| *n == "github").unwrap().1).unwrap();
        assert!(svg.contains("viewBox=\"0 0 16 16\""));
        assert!(OCTICONS_LICENSE.starts_with("MIT License"));
        assert!(OCTICONS_LICENSE.contains("GitHub Inc."));
    }

    #[test]
    fn the_logo_is_the_brand_wordmark() {
        let bytes = Assets.load(LOGO_PATH).unwrap().unwrap();
        assert!(bytes.starts_with(b"<svg"));
        assert!(
            std::str::from_utf8(&bytes)
                .unwrap()
                .contains("viewBox=\"0 0 728 144\"")
        );
    }

    #[test]
    fn unknown_paths_are_none_and_the_list_names_every_icon() {
        assert!(Assets.load("icons/nope.svg").unwrap().is_none());
        assert!(Assets.load("../Cargo.toml").unwrap().is_none());
        assert_eq!(Assets.list("icons").unwrap().len(), all_icons().count());
    }
}
