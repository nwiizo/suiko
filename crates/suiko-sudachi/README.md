# suiko-sudachi

[WorksApplications/sudachi.rs](https://github.com/WorksApplications/sudachi.rs)
**v0.6.11** の非公式なcrates.io再配布です。上流がcrates.ioへ未公開のため、
[suiko](https://github.com/nwiizo/suiko) CLIを `cargo install suiko` で導入
できるようにする目的だけで公開しています。**上流が公式にcrates.ioへ公開した
時点で、このcrateは非推奨にしてそちらへ乗り換えます。**

ライセンスはupstreamと同じ Apache-2.0 です（`LICENSE` を同梱）。著作権は
Works Applications Co., Ltd. にあります。

## 上流からの変更点

コードの動作には手を加えていません。変更は次のみです。

- `include_bytes!` の相対パスを、workspace直下の`resources/`前提から
  crate直下の`resources/`前提へ変更（`src/config.rs`、`src/dic/mod.rs`、
  `src/plugin/input_text/default_input_text/mod.rs`、
  `src/plugin/oov/mecab_oov/mod.rs` の計5箇所）
- workspace継承だったpackageメタデータをこのCargo.tomlへ展開
- 統合テスト（`tests/*.rs`）と、テスト専用のdynamic pluginへのpath依存を
  再配布に含めない（単体テストが参照する`tests/resources/char.def`は同梱）

## 利用について

一般利用にはこのcrateではなく上流リポジトリを参照してください。APIの質問や
不具合報告は、この再配布に起因するもの（上記変更点）だけを
[nwiizo/suiko](https://github.com/nwiizo/suiko) へ、それ以外は上流へお願いします。
