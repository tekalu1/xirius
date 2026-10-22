# Release の本文

`docs/release-notes/<版>.md` に、その版の変わったことを書きます（例: `1.0.0.md` は `v1.0.0`）。

- ファイル名は `Cargo.toml` の `[workspace.package] version` と同じにします。
- `- `（または `* `）で始まる行だけが、アプリの「変わったこと」に出ます（上から 8 行まで）。
- 見出しや説明の段落は Release のページにだけ出ます。アプリには出ません。
- `release.yml` はこのファイルを Release の本文に使い、無い版は作らずに落ちます。

書き方は `TEMPLATE.md` を写してください。手順は `docs/update-signing.md` にあります。
