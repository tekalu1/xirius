// 裏の仕事の閉包・async の受け取り・試験の中は、画面の道ではない。
fn tick() { crate::ancillary_io::dispatch(move || { std::fs::read("song"); Ok(()) }); }
// ----
use xirius_session::background::Job; fn tick() { Job::spawn("reader", move || { std::fs::read("song"); Ok(()) }); }
// ----
fn tick() { crate::ancillary_io::dispatch(move || { eprintln!("failed"); Ok(()) }); }
// ----
async fn idle() { jobs.recv().await; }
// ----
#[cfg(test)] mod tests { fn fixture() { std::fs::read("fixture"); } }
