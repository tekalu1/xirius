//! Which plugins use ARA (PLG-025).
//!
//! An ARA plugin reads the song's audio through the host's ARA objects, which live in this
//! process; in a host process of its own it could not reach them. Until the ARA host is built
//! (docs/plugin-hosting.md, step 3), the scan only marks these plugins so they are never
//! placed in another process. The ARA SDK is not needed for that: a VST3 module that offers
//! ARA lists a class of the category [`ARA_FACTORY_CLASS`] next to its audio classes, and a
//! plugin that only works with ARA says `OnlyARA` among its sub-categories.

use xirius_vst3::ClassInfo;

/// The category of the class that hands out the ARA factory (`kARAMainFactoryClass`).
pub const ARA_FACTORY_CLASS: &str = "ARA Main Factory Class";

/// The sub-category of a plugin that cannot be used without ARA.
const ONLY_ARA: &str = "OnlyARA";

/// Whether the audio class `class` of a module with `classes` uses ARA.
pub(crate) fn uses_ara(classes: &[ClassInfo], class: &ClassInfo) -> bool {
    classes.iter().any(|c| c.category == ARA_FACTORY_CLASS)
        || class.sub_categories.split('|').any(|s| s == ONLY_ARA)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(category: &str, sub: &str) -> ClassInfo {
        ClassInfo {
            cid: [0; 16],
            name: "x".into(),
            category: category.into(),
            sub_categories: sub.into(),
            vendor: String::new(),
            version: String::new(),
        }
    }

    #[test]
    fn a_module_with_the_ara_factory_or_only_ara_is_ara() {
        let audio = class("Audio Module Class", "Fx|Pitch Shift");
        let factory = class(ARA_FACTORY_CLASS, "");
        assert!(uses_ara(&[audio.clone(), factory], &audio));
        assert!(!uses_ara(std::slice::from_ref(&audio), &audio));
        let only = class("Audio Module Class", "Fx|OnlyARA");
        assert!(uses_ara(std::slice::from_ref(&only), &only));
        let near = class("Audio Module Class", "Fx|OnlyARAish");
        assert!(!uses_ara(std::slice::from_ref(&near), &near));
    }
}
