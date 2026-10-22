//! Opens one plugin bundle and writes what it offers; see `xirius_plugins::child_main`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(xirius_plugins::child_main(&args));
}
