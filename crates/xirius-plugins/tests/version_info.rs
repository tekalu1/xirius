//! The version information of the programs (LIC-022): the name, the version, the publisher and
//! the copyright that Explorer's 「詳細」 and the Task Manager show.
#![cfg(windows)]

#[path = "../../../packaging/windows/read_version_info.rs"]
mod read_version_info;

#[test]
fn the_programs_say_what_they_are_their_version_publisher_and_copyright() {
    let scan = env!("CARGO_BIN_EXE_xirius-plugin-scan");
    read_version_info::check(scan.as_ref(), "xirius-plugin-scan", "Xirius plugin scanner");
}
