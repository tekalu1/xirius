//! The app's menu (LIC-015). On macOS the bar at the top of the screen holds it under the app's
//! name, and 「Xirius について」 there opens the same panel as the logo. Windows has no menu bar:
//! GPUI keeps the menu and shows nothing, and the logo and the palette open the panel.

use gpui::{App, Menu, MenuItem, actions};

use crate::studio::Studio;

actions!(xirius, [AboutXirius]);

/// What the menu item says: the palette's row has the same words.
pub const ABOUT: &str = "Xirius について";

/// The app's menus.
pub fn menus() -> Vec<Menu> {
    vec![Menu {
        name: "Xirius".into(),
        items: vec![MenuItem::action(ABOUT, AboutXirius)],
        disabled: false,
    }]
}

/// Gives the app its menus.
pub fn install(cx: &mut App) {
    cx.set_menus(menus());
}

impl Studio {
    /// 「Xirius について」 from the app's menu: opens the panel, or leaves it open.
    pub(crate) fn about_from_menu(&mut self, g: &crate::studio::Geometry) -> bool {
        if self.popup.as_ref().map(|p| p.kind) == Some(crate::popup::PopupKind::About) {
            return false;
        }
        self.open_about(self.about_anchor(g));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::popup::PopupKind;

    #[test]
    fn the_menu_holds_about_and_it_opens_the_panel() {
        let menus = menus();
        assert_eq!(menus[0].name.as_ref(), "Xirius");
        let MenuItem::Action { name, action, .. } = &menus[0].items[0] else {
            panic!("an action");
        };
        assert_eq!(name.as_ref(), ABOUT);
        assert!(action.partial_eq(&AboutXirius));

        let mut s = Studio::sample();
        let g = s.geometry(1600.0, 900.0);
        assert!(s.about_from_menu(&g));
        assert_eq!(s.popup.as_ref().map(|p| p.kind), Some(PopupKind::About));
        assert!(!s.about_from_menu(&g), "a second time leaves it open");
        assert_eq!(s.popup.as_ref().map(|p| p.kind), Some(PopupKind::About));
        // The narrow window has no logo; the menu still opens it.
        s.popup = None;
        let narrow = s.geometry(1200.0, 800.0);
        assert!(s.about_from_menu(&narrow));
        assert_eq!(s.popup.as_ref().map(|p| p.kind), Some(PopupKind::About));
    }
}
