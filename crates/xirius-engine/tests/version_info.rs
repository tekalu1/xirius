//! The version information of the programs (LIC-022): the name, the version, the publisher and
//! the copyright that Explorer's 「詳細」 and the Task Manager show.
#![cfg(windows)]

#[path = "../../../packaging/windows/read_version_info.rs"]
mod read_version_info;

#[test]
fn the_programs_say_what_they_are_their_version_publisher_and_copyright() {
    let engine = env!("CARGO_BIN_EXE_xirius-engine");
    read_version_info::check(engine.as_ref(), "xirius-engine", "Xirius audio engine");
    let host = env!("CARGO_BIN_EXE_xirius-plugin-host");
    read_version_info::check(host.as_ref(), "xirius-plugin-host", "Xirius plugin host");
}
