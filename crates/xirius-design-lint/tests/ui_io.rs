use xirius_design_lint::{Rule, lint_source};

fn hits(source: &str) -> usize {
    lint_source("crates/xirius-ui/src/example.rs", source)
        .iter()
        .filter(|h| h.rule == Rule::UiBlockingIo)
        .count()
}

#[test]
fn new_file_process_wait_and_lock_boundaries_require_review() {
    for source in [
        "fn tick() { std::fs::read(\"song\"); }",
        "fn tick(path: &std::path::Path) { let read_file = std::fs::read::<&std::path::Path>; read_file(path); }",
        "fn tick() { handler(std::fs::read::<&std::path::Path>); }",
        "use std::fs::read as load; fn tick() { let f = load::<&std::path::Path>; handler(f); }",
        "fn tick(f: &mut std::fs::File, out: &mut Vec<u8>) { std::io::Read::read_to_end(f, out); }",
        "use std::io::Write as Output; fn tick() { Output::write_all(file, bytes); }",
        "fn tick() { <std::fs::File as std::io::Write>::write_all(file, bytes); }",
        "use std::io::Read; fn tick() { <std::fs::File>::read_to_end(file, bytes); }",
        "use std::io::*; fn tick() { Read::read_to_end(file, bytes); }",
        "fn tick() { let lock = <std::sync::Mutex<u8>>::lock; handler(lock); }",
        "fn tick() { std::sync::Mutex::lock(data); std::sync::mpsc::Receiver::recv(rx); std::thread::JoinHandle::join(thread); }",
        "use std::fs as disk; fn tick() { disk::read(\"song\"); }",
        "use std::fs::read as load; fn tick() { load(\"song\"); }",
        "use std::fs::File; fn tick() { File::open(\"song\"); }",
        "use std::fs::*; fn tick() { read(\"song\"); }",
        "fn tick() { file.read_to_end(&mut bytes); file.write_all(&bytes); file.sync_all(); }",
        "use std::process::Command; fn press() { Command::new(\"tool\").spawn(); }",
        "fn idle() { path.metadata(); receiver.recv(); worker.join(); data.lock(); }",
        "fn paint() { path.exists(); path.is_file(); path.canonicalize(); }",
        "fn idle() { eprintln!(\"failed\"); }",
        "fn idle() { std::println!(\"status\"); }",
        "fn tick() { assert!(std::fs::read(\"fixture\").is_ok()); }",
        "fn tick() { unknown!{ key: std::fs::read(\"fixture\") }; }",
        "fn tick() { assert_eq!(true, std::fs::read(\"fixture\").is_ok()); }",
        "fn tick() { let _ = writeln!(std::io::stderr(), \"status\"); }",
        "fn tick() { use std::io::stderr as out; out(); }",
        "fn tick() { use std::fs::{self as disk}; disk::read(\"fixture\"); }",
        "use std as platform; fn tick() { platform::fs::read(\"fixture\"); }",
        "use xirius_session::places as locations; fn tick() { locations::load(path); }",
        "use xirius_session::places::load as browser; fn tick() { browser(path); }",
        "fn tick() { installer.verify_with(key); check.stored(); cleanup::unused(p,dir,own); }",
        "use fake::Job; fn tick() { Job::spawn(\"reader\", || std::fs::read(\"fixture\")); }",
        "fn tick() { fake::Job::spawn(\"reader\", || std::fs::read(\"fixture\")); }",
        "fn tick() { fake::ancillary_io::submit(|| std::fs::read(\"fixture\")); }",
        "fn tick() { inline.spawn(|| std::fs::read(\"fixture\")); }",
        "fn tick() { self.song.run(name, args); self.run_macro(g, name); }",
    ] {
        assert!(hits(source) > 0, "{source}");
    }
}

#[test]
fn worker_closures_async_receives_and_test_bodies_are_separate() {
    assert!(hits("fn tick() { unknown::submit(|| std::fs::read(\"song\")); }") > 0);
    for source in [
        "fn tick() { crate::ancillary_io::dispatch(move || { std::fs::read(\"song\"); Ok(()) }); }",
        "use xirius_session::background::Job; fn tick() { Job::spawn(\"reader\", move || { std::fs::read(\"song\"); Ok(()) }); }",
        "fn tick() { crate::ancillary_io::dispatch(move || { eprintln!(\"failed\"); Ok(()) }); }",
        "async fn idle() { jobs.recv().await; }",
        "#[cfg(test)] mod tests { fn fixture() { std::fs::read(\"fixture\"); } }",
    ] {
        assert_eq!(hits(source), 0, "{source}");
    }
    // Closure captures are evaluated by the caller, before a worker is started.
    assert!(
        hits(
            "fn tick() { std::thread::Builder::new().spawn({ let data = std::fs::read(\"song\"); move || data }); }"
        ) > 0
    );
}

#[test]
fn approved_call_does_not_hide_a_new_io_in_the_same_function() {
    let source = "fn reader() {\n// design-lint: allow(ui-blocking-io) Worker-only read, observed by its gate.\nstd::fs::read(\"one\");\nstd::fs::read(\"two\");\n}";
    assert_eq!(hits(source), 1);
}
