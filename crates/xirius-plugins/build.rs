//! Windows: the version information of `xirius-plugin-scan.exe` (LIC-022), which ships with the
//! app. `xirius-plugin-trial` is a tool of the tests and does not ship.

include!("../../packaging/windows/version_info.rs");

fn main() {
    version_info(&[("xirius-plugin-scan", "Xirius plugin scanner")], None);
}
