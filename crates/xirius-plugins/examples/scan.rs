//! Scans the standard folders and prints the counts and the failures.
//!
//! `cargo run -p xirius-plugins --example scan -- [--cache <file>] [--full]`

use xirius_plugins::{ScanOptions, scan_with};

fn main() {
    let mut opts = ScanOptions::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--cache" => opts.cache = args.next().expect("--cache <file>").into(),
            "--full" => opts.full = true,
            other => panic!("unknown argument {other}"),
        }
    }
    println!("program: {:?}", opts.program);
    println!("cache: {}", opts.cache.display());
    let r = scan_with(&opts, &|done, total| {
        if done % 25 == 0 || done == total {
            eprintln!("{done}/{total}");
        }
    })
    .expect("scan");
    println!(
        "bundles {} cached {} probed {} failed {} entries {} ({} instruments) in {:.2} s",
        r.bundles,
        r.cached,
        r.probed,
        r.failed,
        r.catalog.entries.len(),
        r.catalog
            .entries
            .iter()
            .filter(|e| e.kind == xirius_core::model::PluginKind::Instrument)
            .count(),
        r.elapsed.as_secs_f64()
    );
    for f in &r.catalog.failures {
        println!("failed {:?} {} : {}", f.kind, f.path.display(), f.error);
    }
}
