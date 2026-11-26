// 登録の無い操作（ops-coverage）: 呼び出しでも、間接の呼び出しの表でも、綴りの大小違いでも。
fn click(s: &mut Studio) { s.run_command("brand.new", args); } //~ ops-coverage, ui-blocking-io
// ----
const COMMANDS: &[&str] = &["note.unsupported"]; //~ ops-coverage
// ----
fn dispatch(s: &mut Studio) { s.run_binding("track.unknown --on"); } //~ ops-coverage
// ----
fn click(s: &mut Studio) { s.run_command("unregistered", args); } //~ ops-coverage, ui-blocking-io
// ----
fn click(s: &mut Studio) { s.run_command("Note.Quantize", args); } //~ ops-coverage, ui-blocking-io
// ----
// 設定の鍵の名前を、コマンドとして呼ぶと落ちる。
fn click(s: &mut Studio) { s.run_command("audio.device", args); } //~ ops-coverage, ui-blocking-io
// ----
// 設定の鍵とヒントの名前はデータで、コマンドの呼び出しではない（ops-coverage は出ない。出るのは画面の道の待ちだけ）。
fn paint(s: &mut Studio) { let key = "audio.device"; let tooltip = "plugin.open"; s.run_command("prefs.set", args); } //~ ui-blocking-io
// ----
// 登録した引数つきのコマンドと、試験の中の見本も ops-coverage にならない。
fn click(s: &mut Studio) { s.run_command("note.velocity.shape", args); } #[cfg(test)] mod tests { const BAD: &str = "note.fake"; } //~ ui-blocking-io
