//! Tries real plugins for the table in `docs/plugin-test-matrix.md` (PLG-041).
//!
//! `xirius-plugin-trial --list <file> --work <dir> [--shots <dir>] [--timeout <seconds>]`
//! tries each plugin of the list (`path` or `path<TAB>name` per line) in a process of its own
//! on a hidden desktop, prints the table's rows and writes `trials.json` to the work folder.
//! `--one <args>` is that process; see `xirius_plugins::trial::trial_main`.

use std::path::PathBuf;
use std::time::Duration;

use xirius_plugins::trial::{Pick, markdown_rows, run_trials, trial_main};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--one") {
        std::process::exit(trial_main(&args[1..]));
    }
    let (mut list, mut work, mut shots, mut timeout) = (None, None, None, 90u64);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let v = it.next().cloned();
        match (a.as_str(), v) {
            ("--list", Some(v)) => list = Some(PathBuf::from(v)),
            ("--work", Some(v)) => work = Some(PathBuf::from(v)),
            ("--shots", Some(v)) => shots = Some(PathBuf::from(v)),
            ("--timeout", Some(v)) => timeout = v.parse().unwrap_or(timeout),
            _ => {
                eprintln!("usage: --list <file> --work <dir> [--shots <dir>] [--timeout <s>]");
                std::process::exit(2);
            }
        }
    }
    let (Some(list), Some(work)) = (list, work) else {
        eprintln!("usage: --list <file> --work <dir> [--shots <dir>] [--timeout <s>]");
        std::process::exit(2);
    };
    let text = std::fs::read_to_string(&list).expect("read the list");
    let picks = Pick::parse_list(&text);
    let exe = std::env::current_exe().expect("this program's path");
    let trials = run_trials(
        &exe,
        &["--one".to_string()],
        &picks,
        &work,
        shots.as_deref(),
        Duration::from_secs(timeout),
        &mut |t| eprint!("{}", markdown_rows(std::slice::from_ref(t))),
    )
    .expect("run the trials");
    let body = serde_json::to_vec_pretty(&trials).expect("the trials serialise");
    std::fs::write(work.join("trials.json"), body).expect("write trials.json");
    print!("{}", markdown_rows(&trials));
}
