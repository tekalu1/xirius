//! Windows: the version information of `xirius-engine.exe` and `xirius-plugin-host.exe`
//! (LIC-022).

include!("../../packaging/windows/version_info.rs");

fn main() {
    version_info(
        &[
            ("xirius-engine", "Xirius audio engine"),
            ("xirius-plugin-host", "Xirius plugin host"),
        ],
        None,
    );
}
