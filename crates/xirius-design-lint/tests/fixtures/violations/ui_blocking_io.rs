use std::fs as disk;

fn tick() {
    disk::read("fixture"); //~ ui-blocking-io
    file.read_to_end(&mut bytes); //~ ui-blocking-io
    path.metadata(); //~ ui-blocking-io
    receiver.recv(); //~ ui-blocking-io
    worker.join(); //~ ui-blocking-io
    state.lock(); //~ ui-blocking-io
    eprintln!("failed"); //~ ui-blocking-io
    let mut child = std::process::Command::new("helper"); //~ ui-blocking-io
    child.spawn(); //~ ui-blocking-io
}
