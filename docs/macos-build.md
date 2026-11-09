# macOS のビルドと試験（LIC-042・ENG-027）

macOS は段階 1・2（ビルド、窓を開かない試験、通常 CI）の対象です。
Rust は `rust-toolchain.toml` の版を使い、Xcode Command Line Tools が必要です。

試験には cargo-nextest `0.9.151` が必要です。準備と実行は [試験の手順](testing.md) を参照してください。

```sh
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/ci_tests.py
cargo run -p xirius-design-lint
cargo deny check
```

`rust.yml` は Windows と macOS の両方で検査し、Rust のキャッシュを使います。
手動起動で `use_cache` を false にすると、キャッシュ無しの所要時間を測れます。
macOS の検査も公開前に必須です（[公開前の関所](release-ci-gate.md)）。
これは macOS 用の配布物・署名・公証を追加するものではありません。

VST3 は `/Library/Audio/Plug-Ins/VST3` と `~/Library/Audio/Plug-Ins/VST3` を探します。
CoreFoundation が `Info.plist` の `CFBundleExecutable` を解決し、`bundleEntry` の後で
factory を取得します。`bundleEntry`・`bundleExit` が無いバンドルは拒みます。
既存のモジュール寿命の方針（ARC-043）に合わせ、プラグインを閉じてもコードはアンロードせず、
macOS の `bundleExit` はプロセス終了時に呼びます。

音の入出力は既存の cpal の CoreAudio バックエンドを使います。設定の既定は CoreAudio です。
Windows の設定ファイルの WASAPI 値も読み取れますが、Mac に WASAPI の実装は組み込みません。
試験はオフライン描画とドライバーの代役を使い、スピーカーや実機の入力を開きません。

窓なしの文字計測は、同梱フォントを CoreText で処理します。GPUI の MacPlatform は
メインスレッド専用なので、試験のワーカーからは生成しません。起動済みアプリでは
GPUI の登録済み文字システムを使います。実際の窓での文字配置の照合は段階 3 です。

段階 3 に残るものは、NSView のプラグイン画面、時間制約スレッドの設定
（[#61](https://github.com/tekalu1/xirius/issues/61)）、実機の入出力・
遅延・デバイス切替、画面やキー操作の確認です。VST3 のエディターを開く要求には、
Mac ではまだ利用できないというエラーを返します。

プラグインの子プロセスでは POSIX 共有メモリを使います。通知の set は atomic の書き込みだけで、
待機側は 1ms 間隔で確認します。実時間の待機処理の最適化は段階 3 です。
Mac では panic ログを残しますが、Windows の SEH・minidump 相当のネイティブ障害捕捉、
外部プロセスの CPU 時間計測はまだありません。
共有メモリ名は通常の破棄時に取り除きます。親プロセスが強制終了した場合の残存名の回収も
段階 3 で扱います。
