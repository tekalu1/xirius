# GPUI の版の固定

## 選んだ版

| crate | 版 | 書き方 |
|---|---|---|
| `gpui-pre` | `=0.3.8` | `gpui = { package = "gpui-pre", version = "=0.3.8" }` |
| `gpui-pre-platform` | `=0.3.8` | `gpui_platform = { package = "gpui-pre-platform", version = "=0.3.8", features = ["font-kit", "x11", "wayland", "runtime_shaders"] }` |

`=` で完全に固定する。`gpui-pre-*` は同じ番号どうしでしか組み合わさらないので、2 つの版は必ず同時に動かす。ライセンスは Apache-2.0。

## 理由

- crates.io の公式の `gpui` は 0.2.2（2025-10-22）で止まっている。Zed の本体はそれ以降も GPUI を変え続けており、公式版では窓まわり（クライアント側の装飾、窓の操作領域の当たり判定）や描画の改善が取れない。
- `gpui-pre` は Zed の main を定期的に crates.io へ出すスナップショット。gpui-kit（旧 gpui-component）の 0.7.x が同じ `=0.3.8` に乗っているので、将来 gpui-kit の部品を使うときに版が食い違わない。
- git の commit に固定する案もあるが、crates.io の版のほうが `Cargo.lock` にチェックサムが残り、`cargo deny` やオフラインのビルドが素直になる。必要になったら自前のフォークへ移る（差分は `PATCHED.md` に記録する）。

## Windows でのビルド

この PC（Windows 11、Ryzen 9 7950X、RTX 5070）で `cargo build -p xirius-ui --release` が通り、窓が開いて描画と入力が動くことを確かめた。

- Windows の描画は Direct3D 11 + DirectWrite。
- **ビルド時に Windows SDK の `fxc.exe` が要る**（`gpui-pre-windows` の build.rs が HLSL をコンパイルする）。`PATH` に無ければ SDK の `bin\<版>\x64` から探し、見つからないと `Failed to find fxc.exe` で止まる。別の場所に置くときは環境変数 `GPUI_FXC_PATH` に渡す。この PC は Visual Studio / Windows SDK 入りで、設定なしに通った。
- `font-kit` と `runtime_shaders` は macOS 用（`gpui_macos` へ渡る）、`x11` / `wayland` は Linux 用の feature で、Windows のビルドには関係しない。同じ Cargo.toml で全 OS をビルドするために付けている。
- CI の Windows ランナーには Windows SDK が入っているので、追加の準備は要らないはず（未確認）。

## 読む所（固定した版の写し）

名前を思い出して書かず、`~/.cargo/registry/src/index.crates.io-*/gpui-pre-0.3.8/`（無ければ `cargo fetch`）にある本物の定義を見る。Zed の main は先へ進んでいて、固定した版と食い違う。

- `README.md`: 起動の形と `gpui_platform` の feature。
- `docs/contexts.md`: `App` / `Context<T>` / `Window` の使い分け。**`ViewContext` / `WindowContext` は pre 系に無い**（`Context<T>` と `Window` を別々に受ける。残るのは `AsyncWindowContext` だけ）。
- `docs/key_dispatch.md`: キーの届き方（アクションとフォーカス）。
- `examples/`: 動く最小の使い方（`hello_world`・`input`・`list_example`・`drag_drop`・`popover`・`painting`・`animation`・`scrollable` ほか）。部品を足す前に、近い例を探す。
- `src/window.rs`（描画・入力・`paint_quad`・`paint_path`・`on_next_frame`）、`src/elements/`（`div`・`canvas`・`list`・`uniform_list`・`img`・`text`）、`src/style.rs` と `src/styled.rs`（スタイルの語）、`src/text_system/`（文字の計測と並べ）、`src/platform/`。
- 起動は `gpui_platform::application().run(|cx: &mut App| …)`。`Application::new()` は無い。
- 上流への差分は `PATCHED.md`（いまは 0 件）。

## ライセンス

`cargo deny --manifest-path crates/xirius-ui/Cargo.toml list -l license` で全依存を見た（2026-10-12）。

- 大半は MIT / Apache-2.0 / BSD / Zlib / ISC / Unicode 系。
- MPL-2.0 は `cbindgen`・`dwrote`・`option-ext`。計画（`docs/plan.toml`）が MPL-2.0 を許しているので問題にしない。
- GPL / LGPL は `self_cell`・`r-efi` の「どちらか選べる」二重ライセンスの片方にだけ現れる。こちらは Apache-2.0 / MIT を選べば使わない。
- `gpui-pre` 自体は Apache-2.0。

## 上げる手順

1. 出ている版を見る: `cargo search gpui-pre --limit 1`、または [crates.io/crates/gpui-pre](https://crates.io/crates/gpui-pre)。
2. `Cargo.toml` の 2 行の `=0.3.8` を同じ新しい番号に直す。**gpui-kit を使っているなら、gpui-kit が乗っている番号に合わせる**（食い違うと GPUI の型が 2 つできて型が合わなくなる）。
3. `cargo update -p gpui-pre -p gpui-pre-platform` で `Cargo.lock` を直す。
4. `cargo build -p xirius-ui --release` で壊れた API を直す。ここで変わりやすいのは次の所。
   - `WindowOptions`・`TitlebarOptions`・`WindowDecorations`（窓まわり）
   - `Window::on_next_frame`・`dispatch_event`・`paint_quad`（計測と描画）
   - `gpui_platform::application()`（アプリの起動）
5. `cargo fmt --check` / `cargo clippy -p xirius-ui --all-targets -- -D warnings` / `cargo test -p xirius-ui` を通す。
6. 計測をやり直して、数字が落ちていないことを確かめる:
   `cargo run -p xirius-ui --release -- --park --hold 1`（窓は表示の外に置かれ、前面を取らない）。
   窓の操作領域は `cargo run -p xirius-ui --release -- --park --chrome --scenario still --secs 9` で確かめる（結果は `chrome.md`）。
7. 新しい版の依存のライセンスを `docs/licenses.md` の禁止の一覧と突き合わせる。

## この版で踏んだ癖

- `Window::on_next_frame` は描画の直前に 1 回だけ走る。毎フレーム自分で張り直す。
- `WindowOptions.focus = false` で `SW_SHOWNOACTIVATE`（前面を取らない）になる。`inactive_frame_interval: None` を付けないと、前面でない窓は 30 fps に落とされる。
- 表示の外の位置は、窓を開く時点では指定できない（どの表示にも掛からない矩形は、既定の表示の矩形に直される）。開いた後に Win32 の `SetWindowPos` で動かす（`park.rs`）。
- 窓の当たり判定（`WindowControlArea`）は、最後のマウスの位置から決まる。位置を動かさずに `WM_NCHITTEST` だけ送っても、前の答えが返る。
- GPUI のカーソルは OS の形（`CursorStyle`）だけで、自前の絵を渡せない。道具のカーソルは窓を `SetWindowSubclass` で包んで出す（`pointer_os.rs`）。道具の絵を求められている間だけ `WM_SETCURSOR` に自分で答え、それ以外は GPUI の窓手続きへ渡す（GPUI は 1 行も変えない）。
- GPUI は `reset_cursor_style` で、窓が「ホバー中」（OS の `WM_MOUSEMOVE` で立つ旗）のときしか形を OS に渡さない。表示の外に置いた窓（`--park`）は一度もホバーにならないので、`NotAllowed`・`Arrow`・`Crosshair` のような GPUI 側の形は、OS が持つカーソルの握りからは見えない。試験は、窓が決めた形（`pointer::of_studio`）と、自前の絵を渡していないこと（`pointer_os::handed` が `None`）で確かめる。道具の絵は窓が自分で渡すので、OS の握り（`SetCursor` に渡った値）まで見る。
