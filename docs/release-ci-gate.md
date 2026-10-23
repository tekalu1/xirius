# Release の前に、main の CI の成功を確かめる（LIC-039）

タグ `v*` を push すると、`.github/workflows/release.yml` が作る・署名する・公開するまで人の手を
待たずに進みます。公開してよいかを決めるのは、人の承認ではなく、**同じ commit を main に push した
ときの `rust.yml` の run** です。`release.yml` の `ci-gate` の job が、`scripts/release_ci_gate.py`
でそれを確かめます。通らなければ `sign`（鍵を読む job）と `publish` は走りません。

## 規則

- **使える成功**は、次のすべてを満たす run だけです。
  - タグの指す commit（注釈つきのタグは剥がした先）と同じ `head_sha`。
  - workflow が `.github/workflows/rust.yml`（id と path が一致し、`active`）。
  - `push` で起動し、枝は `main`、同じリポジトリ（fork でない）。
  - 最新の attempt で、run の結論が `success`、すべての job が `success`。
  - 必須の job（`REQUIRED_JOBS`: `check (windows-latest)`・`cargo-deny`・`gitleaks`）が
    ちょうど 1 つずつある。
- **赤なら止める**: run か job（必須でないものも）が `failure`・`timed_out`・`action_required`・
  `startup_failure` なら、すぐに止めます。run がまだ走っていても、終わった job が赤なら止めます。
  同じ commit の run が複数あれば、1 つでも赤なら止めます。
- **走っていれば待つ**: 30 秒ごとに見て、上限は 40 分（`ci-gate` の job の上限は 50 分）。
  run がまだ無いときも、初めの 5 分は待ちます（タグと main を続けて push すると、run が
  できるより先に release が始まるため）。
- **それ以外も止める**: run が無い（5 分を過ぎた）、`cancelled`・`skipped`・`neutral`、job の
  欠け・重複、40 分を過ぎた、API の失敗が 5 回続いた。判定できないときは公開しない側にします。
- **権限**: `ci-gate` は鍵を持たず、書く権限も持ちません（`actions: read`・`contents: read`）。
  GitHub の API へは GET だけを送ります。
- **同じ commit**: `sign` は、タグの checkout の commit が、`build` の作った commit と `ci-gate` の
  通した commit の両方と同じことを確かめてから鍵を読みます（途中でタグを付け替えても、
  確かめていない commit には署名しません）。
- **CI の仕様との結び付け**: `REQUIRED_JOBS` は `rust.yml` の job の名前と matrix に合わせます。
  `scripts/test_release_ci_gate.py` が `rust.yml` を読んで突き合わせ、`release.yml` が `sign` と
  `publish` の前に `ci-gate` を置いていることも確かめます。

## 理由

- 人の承認を外すと、公開の前に中身を見る人がいなくなります。その代わりに、配る commit そのものを
  main の CI が通したことを、機械で確かめます。同じ commit・同じ workflow・同じ runner の結果だけを
  使い、別の commit・PR・手動の起動・古い attempt の成功は使いません（配る中身を確かめていない）。
- 赤で止めるのは、CI が赤い commit を配らないためです。フレークかどうかは release からは
  分かりません。CI を再実行して最新の attempt が揃って成功すれば、release を再実行して公開できます。
- 待つのは、タグと main をほぼ同時に push するのがふつうの手順で、CI は release の build より
  遅れて終わることがあるためです。40 分は `rust.yml` の Windows の job（キャッシュの無いときも
  含む）が終わるのに足りる長さです。
- 欠け・`cancelled` でも止めるのは、release の中に CI の代わりになる試験が無いためです
  （release で試験をやり直す道を作ると、CI と同じものを二重に持つことになります）。
  `rust.yml` には `paths-ignore` が無く、main への push は取り消さない（`concurrency` の
  `cancel-in-progress` は PR だけ）ので、ふつうの手順で run が無くなることはありません。
- 採らなかった案:
  - 承認を外して、CI を見ずに公開する（赤い commit が配られる）。
  - CI の終わりを待たずに、その時点の結果だけを見る（走っている途中の commit が配られる）。
  - release の中で試験を全部やり直す（CI と二重になり、本物の窓を開く試験は runner で流せない）。

## 止まったときの対処

`ci-gate` の job のログの最後の `::error::` に理由があります。

| 理由 | 対処 |
|---|---|
| run か job が赤（`failure` など） | `rust.yml` の run を直すか再実行し、揃って成功したら、release の run の **Re-run failed jobs** |
| まだ走っていた（40 分を過ぎた） | CI が終わってから **Re-run failed jobs** |
| run が無い | タグが main の push の commit に付いているか確かめる。まとめて push した途中の commit には run が無いので、タグを main の先頭（run のある commit）に付け直す |
| `cancelled`・job の欠け | `rust.yml` の run を再実行してから **Re-run failed jobs** |
| API の失敗が続いた | 時間を置いて **Re-run failed jobs** |

- 再実行は失敗した job だけで足ります（`build` が通っていれば作り直しません）。
- `publish` の途中で落ちて下書きが残ったときは、再実行が下書きを消して作り直します。公開済みの
  Release があれば、`publish` は何もせずに落ちます（同じ版を出し直さない。版を上げて出す）。
- `REQUIRED_JOBS` と `rust.yml` が合わないときは、`scripts/test_release_ci_gate.py` が落ちます。
  `rust.yml` の job を足す・名前を変えるときは、`REQUIRED_JOBS` も一緒に直します。
