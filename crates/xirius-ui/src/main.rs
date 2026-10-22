//! `cargo run -p xirius-ui`: the real screen (top bar, track column,
//! arrangement, drawer) on the sample song.
//!
//! Flags of the screen:
//!   --park              put the window outside every display
//!   --at <x,y>          put the window at this logical position
//!   --size <w,h>        logical size of the window (default 1440,860)
//!   --shot <dir>        play the screenshot script, write one BMP per step, exit
//!   --script <name>     which script --shot plays: all (default) or another
//!                       name of `xirius_ui::SCRIPTS`
//!   --light             use the light colour theme
//!   --silent            play to a silent output instead of the sound device
//!
//! `cargo run -p xirius-ui --release -- --bench` is the measurement window of
//! UI-023 / UI-024 (the old behaviour). Its flags:
//!   --scenario <name>   run only this scenario (repeatable); see --list
//!   --secs <n>          length of every scenario in seconds
//!   --interactive       no script, keep the window open
//!   --hold <n>          seconds to keep the window open at the end (0 = until closed)
//!   --out <dir>         where bench.md / bench.json go (default target/xirius-ui-bench)
//!   --chrome            ask the window what its title bar / borders answer (writes chrome.md)
//!   --park, --at <x,y>  as above
//!   --list              print the scenario names and exit
//!   --roll              measure a 10,000-note drag on the real editor;
//!                       uses --out and always parks the window
//!   --mixer             measure 64 mixer strips playing (UI-024) to a silent
//!                       output; uses --out and --size and always parks the window
//!
//! Windows are shown without being activated, so they never take the
//! keyboard from whoever is using the PC.
//!
//! `xirius-ui __scan-plugin …` is the child the plugin scan starts for each
//! bundle (see `xirius_plugins::child_main`); it opens no window.

// Double-clicking opens no console window; started from a terminal, `console::attach` joins it
// so that --list, a usage line and a panic still print there (UI-290).
#![windows_subsystem = "windows"]

mod console;

use std::path::PathBuf;

use xirius_ui::app::{Options, Place, run};
use xirius_ui::screen;
use xirius_ui::script::{SCENARIOS, find};

fn usage_text() -> String {
    let scripts: Vec<&str> = xirius_ui::SCRIPTS.iter().map(|(name, _)| *name).collect();
    format!(
        "usage: xirius-ui [--park] [--at <x,y>] [--size <w,h>] [--shot <dir>] [--script {}] [--light] [--silent]
       xirius-ui --bench [--scenario <name>]... [--secs <n>] [--interactive] [--hold <n>] [--out <dir>] [--chrome] [--park] [--at <x,y>] [--list]",
        scripts.join("|")
    )
}

fn usage() -> ! {
    eprintln!("{}", usage_text());
    std::process::exit(2);
}

fn pair(v: &str) -> (f32, f32) {
    let (x, y) = v.split_once(',').unwrap_or_else(|| usage());
    let x = x.trim().parse().unwrap_or_else(|_| usage());
    let y = y.trim().parse().unwrap_or_else(|_| usage());
    (x, y)
}

fn main() {
    console::attach();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--help" | "-h") => {
            println!("{}", usage_text());
            return;
        }
        Some("--version" | "-V") => {
            println!("xirius-ui {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        _ => {}
    }
    if args.first().is_some_and(|a| a == xirius_plugins::CHILD_ARG) {
        std::process::exit(xirius_plugins::child_main(&args[1..]));
    }
    if args.iter().any(|a| a == "--bench") && args.iter().any(|a| a == "--roll") {
        let out = args
            .windows(2)
            .find(|a| a[0] == "--out")
            .map_or("target/roll-bench", |a| a[1].as_str());
        screen::run(screen::Options {
            place: Place::Parked,
            roll_bench: Some(PathBuf::from(out)),
            ..screen::Options::default()
        });
    } else if args.iter().any(|a| a == "--bench") && args.iter().any(|a| a == "--mixer") {
        let value = |name: &str| args.windows(2).find(|a| a[0] == name).map(|a| a[1].clone());
        screen::run(screen::Options {
            place: Place::Parked,
            window: value("--size").map_or((1440.0, 860.0), |v| pair(&v)),
            mixer_bench: Some(PathBuf::from(
                value("--out").unwrap_or_else(|| "target/mixer-bench".into()),
            )),
            ..screen::Options::default()
        });
    } else if args.iter().any(|a| a == "--bench") {
        bench(args.into_iter().filter(|a| a != "--bench"));
    } else {
        real(args.into_iter());
    }
}

fn real(mut args: impl Iterator<Item = String>) {
    let mut opts = screen::Options::default();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| -> String {
            args.next().unwrap_or_else(|| {
                eprintln!("{name} needs a value");
                usage()
            })
        };
        match arg.as_str() {
            "--park" => opts.place = Place::Parked,
            "--at" => {
                let (x, y) = pair(&value("--at"));
                opts.place = Place::At(x, y);
            }
            "--size" => opts.window = pair(&value("--size")),
            "--shot" => opts.shots = Some(PathBuf::from(value("--shot"))),
            "--script" => {
                let name = value("--script");
                opts.script = xirius_ui::SCRIPTS
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map_or_else(|| usage(), |(_, shots)| *shots);
            }
            "--light" => opts.light = true,
            "--silent" => opts.output = xirius_session::output::OutputMode::Silent { paced: true },
            _ => usage(),
        }
    }
    if opts.shots.is_some() && opts.place == Place::Centered {
        // A script never takes the screen of whoever is using the PC.
        opts.place = Place::Parked;
    }
    screen::run(opts);
}

fn bench(mut args: impl Iterator<Item = String>) {
    let mut opts = Options::default();
    let mut chosen = Vec::new();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| -> String {
            args.next().unwrap_or_else(|| {
                eprintln!("{name} needs a value");
                usage()
            })
        };
        match arg.as_str() {
            "--scenario" => {
                let name = value("--scenario");
                match find(&name) {
                    Some(s) => chosen.push(s),
                    None => {
                        eprintln!("unknown scenario {name:?}; try --list");
                        std::process::exit(2);
                    }
                }
            }
            "--secs" => opts.secs = value("--secs").parse().ok().or_else(|| usage()),
            "--hold" => opts.hold_secs = value("--hold").parse().unwrap_or_else(|_| usage()),
            "--out" => opts.out_dir = PathBuf::from(value("--out")),
            "--interactive" => opts.interactive = true,
            "--chrome" => opts.chrome = true,
            "--park" => opts.place = Place::Parked,
            "--at" => {
                let (x, y) = pair(&value("--at"));
                opts.place = Place::At(x, y);
            }
            "--list" => {
                for s in SCENARIOS {
                    println!("{}", s.name);
                }
                return;
            }
            _ => usage(),
        }
    }
    if !chosen.is_empty() {
        opts.scenarios = chosen;
    }
    run(opts);
}
