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

署名の job だけが鍵を読みます。鍵は environment に置き、environment に「タグ `v*` だけ」の
規則を付けます。**承認する人（Required reviewers）は付けません**: 公開まで自動で進み、
公開してよいかは main の CI の成功で決めます（`docs/release-ci-gate.md`、LIC-039）。

1. GitHub のリポジトリで **Settings → Environments → New environment**。名前は `release`。
2. **Deployment branches and tags**: `Selected branches and tags` にして、タグ `v*` だけを許す。
3. **Required reviewers** と **Wait timer** は入れない（入っていれば外す）。入っていると、
   署名の job が承認を待って止まります。
4. **Environment secrets → Add secret**:
   - Name: `XIRIUS_UPDATE_KEY`
   - Secret: 秘密鍵のファイルの中身（64 文字の 16 進数）

`gh` で入れる場合（environment を指定します）:

```powershell
gh secret set XIRIUS_UPDATE_KEY --env release --repo tekalu1/xirius --body (Get-Content -Raw D:\keys\xirius-update.key)
```

リポジトリの Settings → Secrets and variables → Actions にある `XIRIUS_UPDATE_KEY` は消します
（environment のものだけを使います）。

**タグと main の規則**: 承認する人がいないので、次の 3 つが門になります。

- `release.yml` の job は、リポジトリの持主が push したタグでだけ動きます（`github.actor` と
  `github.triggering_actor`）。タグは main の上の commit でなければ落ちます。
- **Settings → Rules → Rulesets** で、タグ `v*` の作成・更新・削除を持主だけにします
  （Bypass list を持主だけにして、Restrict creations / updates / deletions を入れる）。
- **main を守る**: Rulesets（または Branch protection）で、main への force push と削除を禁じ、
  PR から入れるときは `rust` の check（`check (windows-latest)`・`cargo-deny`・`gitleaks`）の
  成功を必須にします。release は同じ commit の main の push の run を見るので、main に直接
  push した commit でも、CI が赤なら公開しません。

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

## 6. 出す（タグの push から公開まで自動）

1. `Cargo.toml` の `[workspace.package] version` を上げ、`docs/release-notes/<版>.md` を書き、
   `UPDATE_KEY` が本物の公開鍵になっていることを確かめて、main に commit して push します。
2. その commit（main の push で `rust.yml` が走る commit）にタグを付けて push します。Release は
   タグの push だけで作ります（手で流す道はありません）。

   ```powershell
   git tag v1.0.0
   git push origin v1.0.0
   ```

3. `release.yml` が次の順に進みます。人の承認は待ちません。
   - `build`: タグが版と同じか（違えば落ちる）、本文のファイルがあるかを確かめ、setup.exe・
     マニフェスト・SUMS を作ります（鍵はありません）。
   - `ci-gate`（`build` と並んで走る）: 同じ commit の main の `rust.yml` の run が揃って成功するのを
     待ちます（40 分まで）。赤・見つからない・取り消されたなら、ここで止まり、署名も公開も
     しません（`docs/release-ci-gate.md`）。
   - `sign`: secret をファイルに書き、タグの checkout の `scripts/update_sign.py` でマニフェストに
     署名し、すぐに鍵のファイルを消し、同じ .py でアプリの `UPDATE_KEY` により `.sig` と setup.exe
     を確かめます。仮の鍵のままならここで落ちます。
   - `publish`: SUMS を確かめ、下書きに載せてから公開します。
     - 正式版（`v1.0.1`）は **latest** にします。安定版のアプリは `/releases/latest` を読みます。
     - プレリリースの版（`v1.0.1-beta.1`。`-` の後ろに何かが付いた版）は **Pre-release** として
       出し、latest にしません。ベータ版を読むのは、設定「ベータ版も受け取る」を入れたアプリ
       だけです（`app.update.beta`、既定は切）。
4. 公開された Release を開いて、本文と配布物（setup.exe・`-update.json`・`.json.sig`・zip・
   LAME の tarball・`SHA256SUMS.txt`）を見ます。間違いに気付いたら、Release を消すのではなく、
   版を上げて出し直します（アプリは公開された版を既に読んでいるかもしれません）。
5. 次の版を出す前に、1.0 のアプリが知らせを出し、ダウンロード・署名の確認・終了時の
   入れ替えまで通ることを、別の利用者（または VM）で確かめます。本物の窓を開く試験
   （`scripts/ci_tests.py` の一覧）も、窓の大きさが決まった PC で流します。

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
