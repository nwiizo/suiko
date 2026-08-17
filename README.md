# Suiko（推敲）

日本語文書の自然さと読みやすさを、再現可能なルールで診断するRust CLIです。

名前は、文章を練り直す日本語の「推敲」から取りました。バイナリ、crate、Agent Skillの名前を `suiko` に統一しています。形態素辞書はバイナリへ埋め込まれるため、実行時に辞書やモデルをダウンロードしません。

## 特徴

- `lint`: 禁止語、翻訳調、定型的な対比、リズム、段落構造、語彙、英語統語の疑いを検出
- `outline`: 見出し、段落の先頭文、箇条書きを抽出して論旨を俯瞰
- `terms`: 略語、カタカナ複合語、固有名詞候補と初出時の説明手掛かりを抽出
- Markdownのfront matter、コードフェンス、インラインコード、リンクURL、埋め込み引用行、表、HTMLタグとコメントをマスク
- `essay` / `tech` / `business` のジャンル別閾値
- 修正前JSONとの `resolved` / `new` / `persisting` 比較
- 自然度とは分離したopt-inの読解負荷レーン
- 標準入力、複数ファイル、JSON、CI向けseverity gate
- プロジェクト設定による既定値、ルール無効化、理由付きの個別許可
- 執筆から収束までを扱うAgent Skill

## ビルド

Rust 1.97以降が必要です。

```sh
cargo install suiko
```

ソースからビルドする場合は次を実行します。

```sh
cargo build --release
cargo install --path .
```

リポジトリから直接試す場合は、以降の `suiko` を `cargo run --release --` に置き換えられます。

## 使い方

```sh
# 自然さを診断
suiko lint draft.md
suiko lint draft.md --genre tech --json

# 読解負荷の指さしも追加
suiko lint draft.md --reading-load --json

# 前回結果との差分
suiko lint draft.md --json > /tmp/suiko-before.json
suiko lint draft.md --baseline /tmp/suiko-before.json --json

# CIでwarn以上を終了コード2にする
suiko lint docs/*.md --fail-on warn

# 構造と用語を確認
suiko outline draft.md --json
suiko terms draft.md --json

# 標準入力
printf '重要なのは、結論です。\n' | suiko lint - --json
```

複数ファイルのJSONは、単一ファイルと同じレコードを配列で返します。単一ファイルの `lint --json` は `file`、`stats`、`findings` を持つオブジェクトです。

終了コードは次のとおりです。

| code | 意味 |
|---:|---|
| 0 | 実行成功。findingの有無は問わない |
| 1 | 入力、形態素解析、JSONなどの実行エラー |
| 2 | `--fail-on` で指定したseverity以上を検出 |

## プロジェクト設定

`lint` はカレントディレクトリの `.suiko.toml` を自動的に読み込みます。

```toml
version = 1
genre = "tech"
fail_on = "warn"
disabled_rules = ["low_specificity"]

[[allow]]
category = "forbidden_phrase"
text = "重要なのは"
reason = "連載固有の見出し"
```

- `genre` と `fail_on` は省略可能な既定値です。同名のCLI引数が常に優先されます。
- `disabled_rules` は通常のfindingと読解負荷レーンの該当カテゴリを無効にします。
- `allow` は同じ `category` のfindingについて、`excerpt` に `text` を含むものだけを除外します。意図を残すため `reason` は必須です。
- `--config <path>` は自動検出の代わりに指定ファイルを読み、`--no-config` は設定を読み込みません。
- 未知のキー、未知のルール、空の `text` / `reason`、`version = 1` 以外は実行エラーです。

設定による除外は、統計、baseline比較、`--fail-on` 判定より前に適用されます。`outline` と `terms` は設定の影響を受けません。

## 読解負荷レーン

`--reading-load` は、次の観点を `info` の指さしとして追加します。

- 長すぎる一文
- 文中に埋もれた列挙
- 長い連続漢字
- 読解時に符号計算を要する二重否定
- 格助詞「の」の近接した連鎖

これはAIらしさの推定ではありません。通常の `findings`、自然度スコア、`--baseline` 比較から分離した `reading_load` セクションへ出力します。

## Agent Skill

[`skills/suiko/SKILL.md`](skills/suiko/SKILL.md) は、診断だけでなく文書設計、執筆、findingの採否、再検査までを扱います。Skill対応エージェントでは `$suiko` として利用できます。

基本原則は「検出は機械、判断は文脈」です。findingを一律に消すのではなく、各項目を「直した」または「残す（理由）」へ分類します。

CLIとAgent Skillは別々に導入します。`cargo install`はCLIだけを、次のコマンドは`suiko` Skillだけを導入します。

```sh
cargo install suiko
npx skills add https://github.com/nwiizo/suiko --skill suiko
```

導入後は`suiko --version`でCLIを確認し、Skill対応エージェントでは`$suiko`を指定します。CLIがない環境では、Skillは導入案内を返すか、同梱の手動チェックリストで診断します。

## 対象範囲

Suikoは一般校正の網羅ではなく、均一なリズムや翻訳調、日本語文書の構造と読解負荷を再現可能に指さすことへ集中します。既存プロジェクトの表記規約や用語辞書は置き換えず、そのまま尊重します。誤字脱字、製品名の正規化、組織固有の表記統一は既存の工程へ残します。

## 品質基準

校正用フィクスチャを回帰テストに含めています。現時点の期待値は次のとおりです。

| fixture | 通常 | `--experimental` |
|---|---:|---:|
| AI的な文書 | 25 | 33 |
| 自然な文書 | 0 | 0 |

形態素解析にはLindera/IPADICを使います。形態素の分割結果そのものではなく、公開するJSON形状と校正フィクスチャに対するカテゴリ別の検出結果を回帰テストで固定します。

開発用評価集合には、出典と利用条件を記録した長い人間文書も含めています。現在の発火率、閾値を変更しなかった理由、評価集合が支えない結論は [eval/calibration.md](eval/calibration.md) に記録しています。

## 設計上の境界

初版には、パイプライン連携用の標準入力、複数ファイル入力、`--fail-on`、baseline比較、読解負荷レーン、プロジェクト設定を含めました。

一方、次の機能は意図的に含めません。

- 文埋め込みモデル: 構成した平板文と深掘り文で追加価値を実測したが、判別根拠が弱い一方で257 MiBのモデルキャッシュと初回取得が必要だったため採用しない。意味の進展は目視で確認する
- 自動修正: 事実、意図した反復、固有の文体を壊しうる判断は人間またはエージェントへ残す
- MCPサーバー: 標準入力とJSONで接続できる範囲を先に検証する
- 一般校正の網羅: 自然さと構造の診断へ集中し、表記統一などは既存の校正工程と組み合わせる
- コーパス評価・閾値校正CLI: 通常のバイナリには含めず、開発用`evaluation` featureへ分離する

## 開発

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features --all-targets
```

検出器の校正用CLIは通常の`suiko`へ含めません。開発時だけ`evaluation` featureを有効にして実行します。

```sh
cargo run --features evaluation --bin suiko-eval -- report eval/corpus.toml
cargo run --features evaluation --bin suiko-eval -- sweep eval/corpus.toml --rule repeated-sentence-lead --values 3,5,7
cargo run --features evaluation --bin suiko-eval -- length-analysis eval/corpus.toml
```

## ライセンス

MIT。第三者由来の資料とフィクスチャに必要な表示は [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) に収録しています。
