# 方針の追跡の仕組み

決めた方針が最後まで漏れなく実装されるよう、方針と進み具合を 1 つのファイル `docs/plan.toml` に置く。ほかに進み具合を写す場所（issue など）は持たない。

## 正本は `docs/plan.toml`

- `[[item]]`: 実装すること。`id`（`UI-012` のような領域名 + 番号）・`title`・`area`・`phase`・`source`（根拠の資料と節）・`done_when`（確かめられる 1 行の完了条件）を持つ。
- `status`（任意）: 進み具合。無ければ未着手。
  - `"done"`: `done_when` をすべて満たしたと、コード・試験・撮影で確かめた。
  - `"partial"`: 一部だけできている、または確かめられていない条件が残っている。足りない所を `remaining` に 1 行で書く（例 `remaining = "ループバックでの 1 ms 以内の実測が無い"`）。迷ったら `partial` にする。
- `phase`: `spike`（4〜6 週の検証）/ `mvp` / `v1`（1.0）/ `later`。
- `[[dropped]]`: 捨てた判断と、その理由。
- `[[open]]`: まだ決まっていないこと。推奨の案を添えてある。決まって実装に反映したものは、決めた内容と日付を `decided` に 1 行で書く（`decided = "YYYY-MM-DD: …"`）。未決のものは欄自体を置かない。
- 根拠の資料は `temporary/research/2026-10-11/`（調査の報告とモック）に保存する。調査資料は追跡・公開しない。`source` は日付ごとのフォルダーからの相対の資料名と節を記録し、資料がない環境でも検証できる。

## 進め方

1. 実装の PR・commit のメッセージの頭に、項目の id を書く（例: `UI-012: ...`）。
2. 項目の `done_when` を満たしたら、同じ commit か直後の commit で、その項目に `status = "done"` を書く。一部だけなら `status = "partial"` と `remaining` を書き、進むたびに `remaining` を直す。commit に id を書いただけでは済みにならない。
3. 方針が変わるとき（項目の追加・削除・決まった未決・捨てること）は、先に `docs/plan.toml` を直す。
4. `python scripts/plan.py check` が id の重複・書式・`phase`・必須の欄・根拠の記述（手元に資料があるときだけファイルの有無）・`status` の値・`partial` の `remaining` を検証する。`python scripts/test_plan.py` はスクリプト自身の試験。

## 進み具合の見方

```
python scripts/plan.py status                     # 全体: phase ごと・領域ごとの「済み / 一部 / 未着手」
python scripts/plan.py status --phase mvp         # MVP だけ
python scripts/plan.py status --phase mvp --open  # MVP の残り（一部は remaining 付き、未着手）を領域の順に並べる
python scripts/plan.py status --area UI --open    # 1 つの領域だけ
```

- 出力は Markdown（表と箇条書き）で、そのまま報告に貼れる。Windows で日本語が化けるときは `PYTHONUTF8=1` を付ける。
- 「一部」の数は、手を付けたが `done_when` に届いていない量。`--open` の `remaining` を読めば、何が残っているかが分かる。
