# 2026-10-30の依存更新

Suiko本体の通常・ビルド・開発・評価用の依存を`cargo update`で更新した。ZIP処理はmanifestも8系から9系へ変更し、9.1.3のファイル名取得APIへ辞書の展開とDOCXの検査を対応させた。Markdownの有効な強調を解釈するため、`pulldown-cmark 0.13.5`を追加した。

主な更新は`ureq 3.5.0`、`sha2 0.11.1`、`serde_json 1.0.154`、`toml 1.1.8`、`rustls 0.23.46`、`memmap2 0.9.12`、`zip 9.1.3`。解決した全バージョンとchecksumは`Cargo.lock`で固定する。Rust 1.97、Edition 2024を維持する。

## 更新の制約

| 対象 | 確認結果と扱い |
|---|---|
| `generic-array 0.14.7` | PDF解析の`pdf-extract → lopdf → aes → cipher → crypto-common 0.1.7`が`=0.14.7`を要求する。0.14.9への指定更新はCargoが拒否した。上流の指定を外すpatchは加えない |
| 同梱`suiko-sudachi 0.7.0` | 上流の最新リリースを再配布している。共有する間接依存の互換更新は反映する。`nom 7`、`fancy-regex 0.13`、`itertools 0.13`、`libloading 0.8`、`thiserror 1`等の依存範囲は上流に合わせて維持する。上流コードの独自移植や再配布crateの別版公開は行っていない |
| SudachiDict | 固定済みの20260723.1が最新の公開リリース。辞書と配布ファイルのSHA-256検証を維持する |
| Skillのnpm依存 | `npm view`で確認した最新は`textlint 15.8.0`、`@textlint-ja/textlint-rule-preset-ai-writing 1.7.0`。同梱スクリプトの指定と一致するため変更しない |

依存をすべて任意の最新majorへ置き換えたわけではない。上流の制約が残るものは上表で区別し、次回は上流の更新と利用経路のテストを併せて確認する。今回の更新はリポジトリとSkillの依存が対象であり、ユーザー環境の全CLIを入れ替えるものではない。

ZIP 9への移行後、既存のDOCX検査テストが成功した。さらに空の一時ディレクトリでビルド処理の辞書取得・展開を実行し、辞書本体のSHA-256が固定値`b2d8c0c3ece5b5244c3db66c5be9e36dd1f7d73f6e7e6ce68c4ef2ee8640c95b`と一致した。`cargo update --dry-run --verbose`は追加更新0件で、上記の固定依存だけが候補として残った。
