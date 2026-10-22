# DAWproject の見本（参照実装が書いたもの）

DAWproject の読み書きの試験（`../../reference_tests.rs`）に使う見本です。

## 出典

- リポジトリ: https://github.com/bitwig/dawproject
- commit: `ee4dcdde75940f30e14e55401a26955a58b8322b`（2025-07-12）
- ライセンス: MIT（Copyright (c) 2020 Bitwig）。全文は同じフォルダの `LICENSE`。

## 中身

| ファイル | 元 |
|---|---|
| `Project.xsd`・`MetaData.xsd` | リポジトリの根にあるスキーマ（DAWproject 1.0）。`scripts/dawproject_xsd.py` が使う |
| `test.dawproject`・`test-complex.dawproject` | 参照実装の試験 `DawProjectTest` の `saveDawProject`・`saveComplexDawProject` が書いたもの（ノート・別名のクリップ・音量のオートメーション・マーカー） |
| `MIDI-*.dawproject` | 同じ試験の `createMIDIAutomationInClipsExample` が書いたもの（CC 1 とピッチベンドを、クリップの中とトラックの上に描く） |
| `AudioWARPEDWithFades.dawproject` | 同じ試験の `createAudioExample` が書いたもの。中の `white-glasses.wav` は、リポジトリの `test-data/white-glasses.wav` |
| `xml/Audio*.xml` | 同じ試験がオーディオの各場面について書いた `project.xml`。オーディオは上の `.dawproject` の中のものを使う（同じファイルを 12 回入れないため） |

## 作り方

上の commit を取り、JDK 23 で次を流して、`target/` にできたものを写しました（中身は直していません）。

```
./gradlew --no-daemon test --tests com.bitwig.dawproject.DawProjectTest
```

`xml/AudioFILE_WITH_ABSOLUTE_PATH.xml` の `File path` は、書いた PC の絶対パスを、個人のフォルダーの名前を含まない `C:\Music\dawproject\test-data\white-glasses.wav` に書き換えてあります（ほかは書いたままです）。試験では、そのパスを別の PC の無いパスに置き換えて読みます。

Bitwig Studio・Studio One が書いた実物の `.dawproject` は、このリポジトリにはありません（README の `project.xml` だけ、`../bitwig-5.0-readme.xml` にあります）。
