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
    ] {
        assert!(hits(source) > 0, "{source}");
    }
}

#[test]
fn worker_closures_async_receives_and_test_bodies_are_separate() {
    assert!(hits("fn tick() { unknown::submit(|| std::fs::read(\"song\")); }") > 0);
    for source in [
        "fn tick() { crate::ancillary_io::dispatch(move || { std::fs::read(\"song\"); Ok(()) }); }",
        "fn tick() { Job::spawn(\"reader\", move || { std::fs::read(\"song\"); Ok(()) }); }",
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
