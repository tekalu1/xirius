//! Windows: the version information of `xirius-ui.exe` (LIC-022), and `assets/brand/xirius.ico`
//! as resource 1, which is the icon GPUI gives the window (taskbar, Alt+Tab, window menu).

include!("../../packaging/windows/version_info.rs");

fn main() {
    version_info(
        &[("xirius-ui", "Xirius")],
        Some("../../assets/brand/xirius.ico"),
    );
}
