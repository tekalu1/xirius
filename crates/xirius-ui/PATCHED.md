# GPUI の上流への差分

GPUI（`gpui-pre =0.3.8`、Zed の `279fe07` の写し）に入れた修正を、1 件ずつここに書く。版の選び方と上げる手順は `GPUI.md`。

## いまの差分: 0 件

GPUI は crates.io の公開物をそのまま使っている。フォークも `[patch]` も git の依存も無く、`Cargo.lock` の `gpui-pre` と `gpui-pre-platform` は registry の版とチェックサムで固定されている。上流の癖は、Xirius の側のコードで避けている（`GPUI.md` の「この版で踏んだ癖」）。

`crates/xirius-ui/tests/patched_doc.rs` が、次のどれかが起きて、この表に 1 件も無いときに落とす。

- `Cargo.toml` に `[patch]` / `[replace]` が入る
- GPUI の依存が registry 以外（git・path）を指す
- `Cargo.lock` の `gpui-pre*` が registry でない、またはチェックサムが無い

## 1 件の書き方

フォークへ移るときは、修正ごとに 1 つの見出しを足す。見出しの下に次の 4 つの項目を、この順に書く。

```
### <修正の短い名前>

- 何を: 直した所（ファイルと関数）
- なぜ: 直さないと何が困るか（再現の手順か、起きる症状）
- 上流: Zed の issue か PR の番号（出していなければ「未提出」と、出さない理由）
- 戻せる条件: 上流のどの版で不要になるか（分かる範囲で）
```

## 修正の一覧

（まだ無い）
