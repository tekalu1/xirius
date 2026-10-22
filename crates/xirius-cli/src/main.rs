//! `xirius`: the command line and MCP front end. Every subcommand is a
//! command of the core registry; nothing here knows a command by name.

mod app;
mod args;
mod command_docs;
mod command_host;
mod midi;
mod play;
mod render;

fn main() -> std::process::ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    // The scan of plugins starts this program again for each bundle (`__scan-plugin`), so a
    // crashing plugin is one failure; it needs no other file next to this one.
    if argv
        .first()
        .is_some_and(|arg| arg == xirius_plugins::CHILD_ARG)
    {
        return std::process::ExitCode::from(xirius_plugins::child_main(&argv[1..]) as u8);
    }
    xirius_plugins::ChildProgram::use_this_program();
    if argv.first().is_some_and(|arg| arg == "--commands-doc") {
        return std::process::ExitCode::from(command_docs::run(&argv[1..]));
    }
    if argv == ["--command-host"] {
        return if command_host::serve().is_ok() {
            std::process::ExitCode::SUCCESS
        } else {
            std::process::ExitCode::FAILURE
        };
    }
    let code = app::main(argv);
    // An imported file's peaks are being kept in the song folder meanwhile (UI-037).
    xirius_session::peaks::wait();
    std::process::ExitCode::from(code)
}
