// 画面の道のファイル・プロセス・待ち・ロック（ui-blocking-io）。関数の値・別名・use・UFCS を通しても見つかる。
fn tick() { std::fs::read("song"); } //~ ui-blocking-io
// ----
fn tick(path: &std::path::Path) { let read_file = std::fs::read::<&std::path::Path>; read_file(path); } //~ ui-blocking-io
// ----
fn tick() { handler(std::fs::read::<&std::path::Path>); } //~ ui-blocking-io
// ----
use std::fs::read as load; fn tick() { let f = load::<&std::path::Path>; handler(f); } //~ ui-blocking-io
// ----
fn tick(f: &mut std::fs::File, out: &mut Vec<u8>) { std::io::Read::read_to_end(f, out); } //~ ui-blocking-io
// ----
use std::io::Write as Output; fn tick() { Output::write_all(file, bytes); } //~ ui-blocking-io
// ----
fn tick() { <std::fs::File as std::io::Write>::write_all(file, bytes); } //~ ui-blocking-io
// ----
use std::io::Read; fn tick() { <std::fs::File>::read_to_end(file, bytes); } //~ ui-blocking-io
// ----
use std::io::*; fn tick() { Read::read_to_end(file, bytes); } //~ ui-blocking-io
// ----
fn tick() { let lock = <std::sync::Mutex<u8>>::lock; handler(lock); } //~ ui-blocking-io
// ----
fn tick() { std::sync::Mutex::lock(data); std::sync::mpsc::Receiver::recv(rx); std::thread::JoinHandle::join(thread); } //~ ui-blocking-io, ui-blocking-io, ui-blocking-io
// ----
use std::fs as disk; fn tick() { disk::read("song"); } //~ ui-blocking-io
// ----
use std::fs::read as load; fn tick() { load("song"); } //~ ui-blocking-io
// ----
use std::fs::File; fn tick() { File::open("song"); } //~ ui-blocking-io
// ----
use std::fs::*; fn tick() { read("song"); } //~ ui-blocking-io
// ----
fn tick() { file.read_to_end(&mut bytes); file.write_all(&bytes); file.sync_all(); } //~ ui-blocking-io, ui-blocking-io, ui-blocking-io
// ----
use std::process::Command; fn press() { Command::new("tool").spawn(); } //~ ui-blocking-io, ui-blocking-io
// ----
fn idle() { path.metadata(); receiver.recv(); worker.join(); data.lock(); } //~ ui-blocking-io, ui-blocking-io, ui-blocking-io, ui-blocking-io
// ----
fn paint() { path.exists(); path.is_file(); path.canonicalize(); } //~ ui-blocking-io, ui-blocking-io, ui-blocking-io
// ----
fn idle() { eprintln!("failed"); } //~ ui-blocking-io
// ----
fn idle() { std::println!("status"); } //~ ui-blocking-io
// ----
// マクロの中も見る。
fn tick() { assert!(std::fs::read("fixture").is_ok()); } //~ ui-blocking-io
// ----
fn tick() { unknown!{ key: std::fs::read("fixture") }; } //~ ui-blocking-io
// ----
fn tick() { assert_eq!(true, std::fs::read("fixture").is_ok()); } //~ ui-blocking-io
// ----
fn tick() { let _ = writeln!(std::io::stderr(), "status"); } //~ ui-blocking-io, ui-blocking-io
// ----
fn tick() { use std::io::stderr as out; out(); } //~ ui-blocking-io
// ----
fn tick() { use std::fs::{self as disk}; disk::read("fixture"); } //~ ui-blocking-io
// ----
use std as platform; fn tick() { platform::fs::read("fixture"); } //~ ui-blocking-io
// ----
// 曲のファイルを読む session の口。
use xirius_session::places as locations; fn tick() { locations::load(path); } //~ ui-blocking-io
// ----
use xirius_session::places::load as browser; fn tick() { browser(path); } //~ ui-blocking-io
// ----
fn tick() { installer.verify_with(key); check.stored(); cleanup::unused(p,dir,own); } //~ ui-blocking-io, ui-blocking-io, ui-blocking-io
// ----
fn tick() { installer.hold_with(key); } //~ ui-blocking-io
// ----
// 名前が似ているだけの、本物ではない裏の仕事の口。
use fake::Job; fn tick() { Job::spawn("reader", || std::fs::read("fixture")); } //~ ui-blocking-io
// ----
fn tick() { fake::Job::spawn("reader", || std::fs::read("fixture")); } //~ ui-blocking-io
// ----
fn tick() { fake::ancillary_io::submit(|| std::fs::read("fixture")); } //~ ui-blocking-io
// ----
fn tick() { inline.spawn(|| std::fs::read("fixture")); } //~ ui-blocking-io
// ----
fn tick() { unknown::submit(|| std::fs::read("song")); } //~ ui-blocking-io
// ----
fn tick() { self.song.run(name, args); self.run_macro(g, name); } //~ ui-blocking-io, ui-blocking-io
// ----
// 閉包が取り込む値は、裏の仕事が始まる前に呼んだ側で作られる。
fn tick() { std::thread::Builder::new().spawn({ let data = std::fs::read("song"); move || data }); } //~ ui-blocking-io
// ----
// 認めた呼び出しの印は、同じ関数の新しい読み込みを隠さない。
fn reader() {
// design-lint: allow(ui-blocking-io) Worker-only read, observed by its gate.
std::fs::read("one");
std::fs::read("two"); //~ ui-blocking-io
}
