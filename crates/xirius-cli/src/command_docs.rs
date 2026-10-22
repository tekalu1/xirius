pub fn run(args: &[String]) -> u8 {
    let (check, path) = match args {
        [path] => (false, path),
        [flag, path] if flag == "--check" => (true, path),
        _ => {
            eprintln!("--commands-doc [--check] <path>");
            return 2;
        }
    };
    let text = xirius_core::commands::docs::markdown(&xirius_session::commands::registry());
    if check {
        match std::fs::read_to_string(path) {
            Ok(actual) if actual.replace("\r\n", "\n") == text => 0,
            _ => {
                eprintln!("コマンド一覧を再生成してください: {path}");
                1
            }
        }
    } else {
        match std::fs::write(path, text) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("{path}: {error}");
                1
            }
        }
    }
}
