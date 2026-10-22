# アプリの中の更新の鍵と、Release の出し方（UI-282）

アプリは、Release に載った **マニフェスト** `Xirius-<版>-win-x64-update.json` の署名を確かめ、
そのマニフェストが指す `Xirius-<版>-win-x64-setup.exe` を入れます。マニフェストは次の 5 つを
書いた短い JSON です（署名はこのファイルのバイトに掛かります）。

```json
{"channel":"stable","file":"Xirius-1.0.1-win-x64-setup.exe","min_version":"1.0.0","sha256":"…","version":"1.0.1"}
```

- `min_version` は、この版を入れてよい一番古い Xirius です。今の版がこれより古ければ断ります。
  古い中間の版を配り直すのを止めたいときは、`scripts/release.py --min-version` で上げます。

アプリは、署名に加えて次を確かめます。1 つでも合わなければ、そのインストーラーは捨てます。

- マニフェストの版が、Release のタグ（`v1.0.1` の `1.0.1`）と同じ。
- 今の版より新しい（古い版に下げる署名を作れない）。
- 今の版が `min_version` 以上。
- `SHA256SUMS.txt` の setup.exe の値と、マニフェストの `sha256` が同じ。
- ダウンロードした setup.exe の SHA-256 が、マニフェストの `sha256` と同じ。
- チャンネルが Release（プレリリースか）と、設定「ベータ版も受け取る」に合う。

- 秘密鍵は GitHub Actions の **environment `release` の secret** `XIRIUS_UPDATE_KEY` に置きます。
  リポジトリの secret には置きません（リポジトリに書ける人が、別の workflow から読めてしまうため）。
- 署名と確認は **Python の標準ライブラリだけ** の `scripts/update_sign.py`（ed25519、RFC 8032 の
  参照の実装）で行います。署名の job は、タグの checkout にあるこの .py だけを使い、build の job が
  作ったプログラムには鍵を渡しません。
- 公開鍵は `crates/xirius-session/src/update.rs` の `UPDATE_KEY` にあります。いまは仮の値
  （`PLACEHOLDER_KEY` と同じ 64 個の `0`）で、そのビルドは更新を入れず、これまでどおり
  Release のページをブラウザーで開きます。
- 鍵を作ったら `UPDATE_KEY` を差し替えて commit し、その後に出す版からアプリの中の更新が働きます。
  差し替えていないビルドでは働きません（CI の署名の job も、仮の鍵のままなら落ちます）。

## 1. 鍵を作る（利用者が行う）

鍵は 64 文字の 16 進数（32 バイトの種）です。**リポジトリの外**に書きます。次のどちらでも、
同じ形の鍵ができます（Rust の道具は、リポジトリの中への書き出しを断ります）。

```powershell
# リポジトリのフォルダーで
cargo run -p xirius-update-sign -- keygen --out D:\keys\xirius-update.key
# または
python scripts/update_sign.py keygen --out D:\keys\xirius-update.key
```

- `D:\keys` は先に作っておきます。どちらもフォルダーを作りません。
- 同じ名前のファイルがあると、**上書きせずに失敗**します。
- 出力の「公開鍵（public）」の 64 文字を控えます。
- 秘密鍵のファイルは、無くさない所に**2 か所以上**に控えます。
  - 無くすと、**古い版のアプリは新しい版を受け付けなくなります**。そのときは、利用者に
    Release から手で入れてもらう必要があります（同じ鍵を作り直すことはできません）。
  - 秘密鍵を commit しない・Issue に貼らない・ログに出さない。`.gitignore` に `*.key` を入れてある。

## 2. 公開鍵をアプリに入れる

`crates/xirius-session/src/update.rs` の

```rust
pub const UPDATE_KEY: &str = "0000000000000000000000000000000000000000000000000000000000000000";
```

を、鍵を作ったときに出た公開鍵（64 文字）に差し替えます。
`PLACEHOLDER_KEY` はそのまま残します（仮の鍵かどうかの判定に使います）。

```powershell
cargo test -p xirius-session update
```

## 3. environment `release` と secret（利用者が行う）

署名の job だけが鍵を読みます。鍵は environment に置き、environment に「承認する人」と
「タグ `v*` だけ」の規則を付けます（作るのは公開のとき）。

1. GitHub のリポジトリで **Settings → Environments → New environment**。名前は `release`。
2. **Deployment branches and tags**: `Selected branches and tags` にして、タグ `v*` だけを許す。
3. **Required reviewers** に承認する人を入れ、**Prevent self-review** を入れる（勧め）。
   - 1 人で出すときは、自分で承認することになります。可能なら承認する人を 2 人以上にします。
   - これで、署名の job は承認するまで始まりません。鍵を持つ job を人が見てから動かせます。
4. **Environment secrets → Add secret**:
   - Name: `XIRIUS_UPDATE_KEY`
   - Secret: 秘密鍵のファイルの中身（64 文字の 16 進数）

`gh` で入れる場合（environment を指定します）:

```powershell
gh secret set XIRIUS_UPDATE_KEY --env release --repo tekalu1/xirius --body (Get-Content -Raw D:\keys\xirius-update.key)
```

リポジトリの Settings → Secrets and variables → Actions にある `XIRIUS_UPDATE_KEY` は消します
（environment のものだけを使います）。

**タグの規則（勧め）**: Settings → Rules → Rulesets で、タグ `v*` を作れるのを持主だけにし、
消せないようにします。リポジトリに書ける人が、改変した `release.yml` を含む commit にタグを
付けて push すると、その workflow が走ります。残る門は environment の承認する人だけです。

## 4. 手元で署名する

`scripts/release.py` に鍵のパスを渡すと、setup.exe とマニフェストを作った後に、
`scripts/update_sign.py` でマニフェストへ署名します（CI と同じ .py です）。

```powershell
python scripts/release.py --tag v1.0.0 --sign-key D:\keys\xirius-update.key
```

`target\dist` に `Xirius-1.0.0-win-x64-update.json` と、その `.sig` ができます。
手元でも、アプリの公開鍵で通るかを確かめられます。

```powershell
python scripts/update_sign.py verify `
  --app crates/xirius-session/src/update.rs `
  --manifest target\dist\Xirius-1.0.0-win-x64-update.json `
  --sig target\dist\Xirius-1.0.0-win-x64-update.json.sig `
  --setup target\dist\Xirius-1.0.0-win-x64-setup.exe --version 1.0.0
```

`--min-version` は、この版を入れてよい一番古い Xirius です（既定は `1.0.0`）。

## 5. Release の本文

`docs/release-notes/<版>.md` を作ります（例: `docs/release-notes/1.0.0.md`）。
アプリの「変わったこと」は、この本文の `- `（または `* `）で始まる行を上から 8 行まで読みます。
`docs/release-notes/TEMPLATE.md` を写してください。本文のファイルが無い版は、`release.yml` が
作らずに落ちます。

## 6. 出す（下書き → 確かめる → 公開）

1. `Cargo.toml` の `[workspace.package] version` を上げ、`docs/release-notes/<版>.md` を書き、
   `UPDATE_KEY` が本物の公開鍵になっていることを確かめて、main に commit します。
2. タグを push します（Release はタグの push だけで作ります。手で流す道はありません）。

   ```powershell
   git tag v1.0.0
   git push origin v1.0.0
   ```

3. `release.yml` の `build` が setup.exe・マニフェスト・SUMS を作ります（鍵はありません）。
   `sign` は environment `release` の承認を待ち、secret をファイルに書いて、タグの checkout の
   `scripts/update_sign.py` でマニフェストに署名し、すぐに鍵のファイルを消し、同じ .py で
   アプリの `UPDATE_KEY` により `.sig` と setup.exe を確かめます。仮の鍵のままならここで落ちます。
   `publish` が**下書き**の Release を作って載せます。タグの push でも自動では公開しません。
4. Release の下書きを開いて確かめます。
   - 本文が `- ` の箇条で、アプリの知らせに出したい文になっている。
   - `Xirius-<版>-win-x64-setup.exe`・`Xirius-<版>-win-x64-update.json`・
     `Xirius-<版>-win-x64-update.json.sig` がある。
   - `SHA256SUMS.txt` の値が setup.exe と合う。
5. 確かめたら **Publish release** で公開します。最新の版にするときは
   「Set as the latest release」を選びます。
   - 安定版のアプリは `/releases/latest` を読むので、**ベータ版は latest にしない**でください。
     ベータ版は「Pre-release」の印を付けて公開します（版に `-beta.1` などを付けます）。
6. 次の版を出す前に、1.0 のアプリが知らせを出し、ダウンロード・署名の確認・終了時の
   入れ替えまで通ることを、別の利用者（または VM）で確かめます。

## 7. 気をつけること

- **鍵を替えると、古い版のアプリは新しい版を受け付けません。** 替えるときは、利用者に
  Release から手で入れてもらう版を 1 回出します（その版に新しい公開鍵を入れます）。
- ベータ版を受け取るのは、設定の「一般 → 更新 → ベータ版も受け取る」を入れた利用者だけです
  （既定は切）。ベータ版も同じ鍵で署名します。
- zip を展開して使っている（インストーラーが入れた場所以外の）アプリは、更新を入れずに
  Release のページをブラウザーで開きます。入れた場所は、HKCU の
  `Software\Microsoft\Windows\CurrentVersion\Uninstall\Xirius` の `InstallLocation` で見ます。
- 「更新して再起動」の前に、同じフォルダーの Xirius のプログラムのうち、**利用者が別に起こした
  もの**（`xirius.exe mcp`・別の `xirius-ui.exe` など）が動いていないかを見ます。動いていれば
  入れずにその名前を言います。この窓が起こした子（エンジン・プラグインのホスト・スキャナー）は
  数えません（窓が終わると一緒に終わります）。
- 30 秒待っても閉じないままインストーラーが止まったときは、古い Xirius を起こし直し、次の起動で
  「前回の更新を入れられませんでした」と言います。印は `%LOCALAPPDATA%\Xirius\keep\update-failed.txt`
  で、アプリのフォルダーを環境変数で変えていても読めるようにしてあります。ファイルの書き込みが
  途中で失敗したときも同じです。
- ダウンロードしたインストーラーは `%LOCALAPPDATA%\Xirius\keep\update\<版>\` に残り、次の起動でも
  「更新して再起動」を出します。今の版以下・壊れたもの・古い `.part` は起動のときに消しますが、
  ほかの窓がまだ書いている `.part` とフォルダーは残します。
