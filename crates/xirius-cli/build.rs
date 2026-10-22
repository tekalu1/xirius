//! Windows: the version information of `xirius.exe` (LIC-022).

include!("../../packaging/windows/version_info.rs");

fn main() {
    version_info(&[("xirius", "Xirius command line")], None);
}
