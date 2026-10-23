# Xirius

- 開発はこの公開のリポジトリ（`tekalu1/xirius`）で直接行う。別の場所から写して出す手順は無い。commit・PR・Issue はそのまま公開されるので、個人の名前・メール・パス・PC の名前・社名・秘密の値を書かない（`CONTRIBUTING.md` の Leaks。`git config core.hooksPath .githooks` で push の前に確かめる）。
- 方針と進み具合の正本は `docs/plan.toml`（ほかに写す場所は無い）。仕組みは `docs/PLAN.md`。
- PR・commit には、対応する項目の id を書く（例: `UI-012: ...`）。項目の done_when を満たしたら、その項目に `status = "done"` を書く（一部なら `status = "partial"` と `remaining`）。進み具合は `python scripts/plan.py status` で見る。commit には DCO の `Signed-off-by` を付ける（`git commit -s`、`CONTRIBUTING.md`）。
- 方針が変わるときは、先に `docs/plan.toml` を直し、`python scripts/plan.py check` を通す。
- 根拠の資料は `temporary/research/`（`2026-10-11/` ほか）。手元にだけあり、追跡も公開もしない（`.gitignore` の `/temporary/`）。内容を直さない（写しのため）。無い clone でも `python scripts/plan.py check` は通る。
- ライセンスは `MIT OR Apache-2.0`。依存を足すときは、`docs/licenses.md` の禁止の一覧を確かめる。
- 並行の作業は git worktree で分け、1 つの worktree で 1 つの領域を扱う。ルートの Cargo.toml はメンバーを crates/* にしてあるので、新しいクレートを足すときに直さない。
- worktree での作業を終えるとき・`git worktree remove` の前には、その worktree の `target/` を消す（その場所で `cargo clean`）。各 worktree の `target/` は独立した debug ビルドで、1 本で数十 GiB になることがある。確認はフルの `cargo build`/`cargo test` を流す前に `cargo check`・`cargo clippy` で足りないかを先に見る。
- 試験は、この PC を使っている人を邪魔せずに走らせる。ブラウザーは headless、アプリの窓は前面を取らず（フォーカスを奪わない・画面の外）、音はスピーカーに出さない（オフラインの描画か無音の出力）、OS のキー入力は使わない。
- GitHub Actions は有効。`rust`（Windows の fmt・clippy・試験・design lint・release の scripts の試験、cargo-deny、gitleaks）と `plan` が push と PR で、`dco` が PR で走る。本物の窓を開く試験は CI から外してある（一覧は `scripts/ci_tests.py`）。それらは Release を出す前に、窓の大きさが決まった PC で流す。macOS のビルドの距離は `mac-build`（手で起こす）で測る。
- 手元の検査は `cargo fmt --check`・`cargo clippy --workspace --all-targets -- -D warnings`・`python scripts/ci_tests.py`・`cargo run -p xirius-design-lint`・`cargo deny check`。plan を直したら `python scripts/plan.py check` と `python scripts/test_plan.py` も流す。
- Release はタグ `v<版>` の push で、作る・署名する・公開するまで自動で進む。公開の前に、同じ commit の main の `rust` の成功を待つ（`docs/release-ci-gate.md`）。`-beta.1` などの付いた版は pre-release になる（`docs/update-signing.md`）。
- 画面の形は `docs/ui-approved.md` に書いた承認済みのモックに合わせる。
- `CLAUDE.md` と `AGENTS.md` は同じ中身。片方を直したら、もう片方も同じに直す。

## GPUI と Zed のソース（UI-033）

- GPUI は `gpui-pre =0.3.8`（Zed の `279fe07` の写し）。crates.io の公式の `gpui` 0.2.2 ではない。版の理由・上げる手順・踏んだ癖は `crates/xirius-ui/GPUI.md`、上流への差分は `crates/xirius-ui/PATCHED.md`（いまは 0 件）。
- **古い API（0.2 系以前）と新しい API（pre 系）を混ぜない。** ネットの記事・AI の記憶にある API の多くは古い。
  - pre 系に `ViewContext` / `WindowContext` は無い（`Context<T>` と `Window` を別々に受ける。残るのは `AsyncWindowContext` だけ）。
  - 起動は `gpui_platform::application().run(|cx: &mut App| …)`。`Application::new()` は無い。
  - 迷ったら、書く前に下の「読む所」で、いま固定している版の本物の定義を見る。名前を思い出して書かない。
- **読む所は Zed のリポジトリではなく、固定した版の写し。** `~/.cargo/registry/src/index.crates.io-*/gpui-pre-0.3.8/`（無ければ `cargo fetch`）。Zed の main は先へ進んでいて、固定した版と食い違う。
  - `README.md`: 起動の形と、`gpui_platform` の feature。
  - `docs/contexts.md`: `App` / `Context<T>` / `Window` の使い分け。`docs/key_dispatch.md`: キーの届き方（アクションとフォーカス）。
  - `examples/`: 動く最小の使い方（`hello_world`・`input`・`list_example`・`drag_drop`・`popover`・`painting`・`animation`・`scrollable` ほか）。部品を足す前に近い例を探す。
  - `src/window.rs`（描画・入力・`paint_quad`・`paint_path`・`on_next_frame`）、`src/elements/`（`div`・`canvas`・`list`・`uniform_list`・`img`・`text`）、`src/style.rs` と `src/styled.rs`（スタイルの語）、`src/text_system/`（文字の計測と並べ）、`src/platform/` と `gpui-pre-windows-0.3.8/`（Windows の窓・DirectWrite・Direct3D）。
  - 部品は `gpui-kit-0.7.1` / `gpui-component-0.7.1`（同じ registry に取ってある。同じ `=0.3.8` に乗っている）。ドック・仮想リスト・ドロップダウンの書き方の手本。使うときは版を合わせる。
- 窓の描画は、上の `window.rs` の `paint_*` を直に呼ぶ自前の `Pen`（`crates/xirius-ui/src/draw.rs`）に通す。画面の部品はまず `Pen` の語（`fill`・`round`・`text`・`icon`・`mask`…）で足りるかを見る。
