# Suiko（推敲）

[![crates.io](https://img.shields.io/crates/v/suiko.svg)](https://crates.io/crates/suiko)
[![CI](https://github.com/nwiizo/suiko/actions/workflows/ci.yml/badge.svg)](https://github.com/nwiizo/suiko/actions/workflows/ci.yml)
[![license: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

日本語文書の自然さと読みやすさを、再現可能なルールで診断するRust CLIです。[crates.io](https://crates.io/crates/suiko) から導入できます。

```sh
cargo install suiko
```

名前は、文章を練り直す日本語の「推敲」から取りました。バイナリ、crate、Agent Skillの名前を `suiko` に統一しています。形態素辞書はバイナリへ埋め込まれるため、実行時に辞書やモデルをダウンロードしません。

## 特徴

- `lint`: 禁止語、翻訳調、定型的な対比、リズム、段落構造、語彙、英語統語の疑いを検出
- `outline`: 見出し、段落の先頭文、箇条書きを抽出して論旨を俯瞰
- `terms`: 略語、カタカナ複合語、固有名詞候補と初出時の説明手掛かりを抽出
- Markdownのfront matter、コードフェンス、インラインコード、リンクURL、埋め込み引用行、表、HTMLタグとコメント、参考文献リスト行（`[1] …`、`[^1]: …`）、コード注釈行（`#A …`）をマスク（抑制した行数は`stats.masking`に出力）
- `essay` / `tech` / `business` のジャンル別閾値
- 修正前JSONとの `resolved` / `new` / `persisting` 比較
- 自然度とは分離したopt-inの読解負荷レーン
- 標準入力、複数ファイル、JSON、CI向けseverity gate
- プロジェクト設定による既定値、ルール無効化、理由付きの個別許可
- 執筆から収束までを扱うAgent Skill

## ビルド

Rust 1.97以降が必要です。形態素解析は [sudachi.rs](https://github.com/WorksApplications/sudachi.rs) と SudachiDict core を使い、辞書はビルド時にSHA-256を検証してバイナリへ埋め込みます。

```sh
cargo install suiko
```

ビルド時に一度だけ、SudachiDict 20260723 core のzip（約69MB）を公式配布元からSHA-256固定で取得して埋め込みます。**実行時のダウンロードやファイル参照はありません。** 事前に `./scripts/fetch-dictionary.sh` で `resources/system.dic` へ配置するか、環境変数 `SUIKO_SUDACHI_DICT` で辞書ファイルを指定すれば、ビルド時の取得も行いません（オフラインビルド時は必須）。埋め込む辞書が約207MBあるため、バイナリは200MB台になります。

sudachi.rsはcrates.io未公開のため、Apache-2.0の条件に従った非公式再配布 [suiko-sudachi](https://crates.io/crates/suiko-sudachi)（v0.6.11そのまま、変更点はREADMEに明記）へ依存しています。上流が公式にcrates.ioへ公開した時点でそちらへ乗り換えます。

ソースから導入する場合は次のとおりです。

```sh
git clone https://github.com/nwiizo/suiko
cd suiko
./scripts/fetch-dictionary.sh   # 任意(なければビルド時に取得)
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

# 前回結果との差分（複数ファイルも同じbaselineで比較できる）
suiko lint docs/*.md --json > /tmp/suiko-before.json
suiko lint docs/*.md --baseline /tmp/suiko-before.json --json

# CIでwarn以上を終了コード2にする
suiko lint docs/*.md --fail-on warn

# GitHub ActionsのPR注釈として出力する
suiko lint docs/*.md --format github --fail-on warn

# エディタやコードスキャン向けのSARIF 2.1.0
suiko lint docs/*.md --format sarif > suiko.sarif

# 構造と用語を確認
suiko outline draft.md --json
suiko terms draft.md --json

# 複数ファイルの用語集計と表記揺れの一覧（読み取り専用）
suiko terms --audit docs/*.md --json

# 標準入力
printf '重要なのは、結論です。\n' | suiko lint - --json
```

複数ファイルのJSONは、単一ファイルと同じレコードを配列で返します。単一ファイルの `lint --json` は `file`、`suiko_version`、`stats`、`findings` を持つオブジェクトです。

対象箇所を一意に指せるfindingは、`line` に加えて `span` を持ちます。

```jsonc
{
  "line": 12,
  "category": "forbidden_phrase",
  "excerpt": "…重要なのは、この点…",
  "severity": "warn",
  "span": {
    "start_line": 12, "start_column": 5,   // 列はUnicode scalar数え・1始まり(全角も1)
    "end_line": 12,   "end_column": 10,    // 終端は半開区間(最後の文字の次)
    "start_byte": 12, "end_byte": 27       // 各行内のUTF-8 byte offset・0始まり半開区間
  }
}
```

同じ表現が一行に複数ある場合も、findingごとに別の `span` が付きます。`low_burstiness` や語彙多様性のような文書全体の指標は特定の範囲を指さないため、`span` を省略します。列は結合文字も1と数えるUnicode scalar単位で、書記素クラスタではありません。

機械的に安全と確認した縮約（現在は「〜することができる」→「〜できる」と、サ変名詞に隣接する「〜を行う」→「〜する」の2系統）には `suggestion`（`span`、`preimage`、`replacement`）が付きます。`preimage` が原文と一致する場合に限って適用できる契約で、Suiko自身はファイルを書き換えません。意味が変わりうるパターン（「することはできない」等）には候補を出しません。

`--format github` はfindingをGitHub Actionsのworkflowコマンド（`::warning file=...,line=...,col=...::`）として出力し、PRの該当行へ注釈を付けられます。severityは `critical→error` / `warn→warning` / `info→notice` に対応します。`--format sarif` はSARIF 2.1.0を出力し、`columnKind: unicodeCodePoints` を宣言して `span` の列をそのまま使います（severityは `error` / `warning` / `note`）。

`terms --audit` は複数ファイルの用語候補を集計し、SudachiDictの正規化表記で表記揺れ（サーバー/サーバ等）をクラスタして返します。読み取り専用で、置換や辞書の書き込みは行いません。

`lint --json` の `stats.readability` には平均文長、動詞・助詞比率、文字種比率の観測値が入ります。読者別の難易度スコアは、正解ラベル付きコーパスで校正できるまで実装しません（観測値のみを提供します）。

`--baseline` には前回の `lint --json` 出力（単一オブジェクトまたは配列）をそのまま渡せます。レコードは `file` 文字列の完全一致で対応づけ、改名は推測しません。baselineにないファイルは全findingを新規として `baseline.file_status = "added"` で示し、baselineにあって今回対象にないファイルはstderrへ警告します。genre、`--experimental`、Suikoバージョンが一致しない場合は実行エラーになります。`antithesis_repetition` や `low_burstiness` のような文書単位のfindingは、文章の言い換えで抜粋が変わっても同一カテゴリとして継続扱いします。

`antithesis_repetition` と `repeated_sentence_lead` は文書単位の集約findingです。同じ反復キーは1件にまとめ、全対象行を `related_lines` で示します。finding件数は「一致した箇所の数」ではなく「反復状態の数」を意味します。文頭のラベル+コロン（用語集やFAQの定型フィールド）は、散文の無意識な反復と区別して `detail` に明記します。

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
- 読点が1つもない60字以上の一文
- 文中に埋もれた列挙
- 長い連続漢字
- 読解時に符号計算を要する二重否定
- 格助詞「の」の近接した連鎖

これはAIらしさの推定ではありません。通常の `findings`、自然度スコア、`--baseline` 比較から分離した `reading_load` セクションへ出力します。

## Agent Skill

[`skills/suiko/SKILL.md`](skills/suiko/SKILL.md) は、診断だけでなく文書設計、執筆、findingの採否、再検査までを扱います。Skill対応エージェントでは `$suiko` として利用できます。

基本原則は「検出は機械、判断は文脈」です。findingを一律に消すのではなく、各項目を「直した」または「残す（理由）」へ分類します。

CLIとAgent Skillは別々に導入します。上記のビルド手順はCLIだけを、次のコマンドは`suiko` Skillだけを導入します。

```sh
npx skills add https://github.com/nwiizo/suiko --skill suiko
```

GitHub CLI（`gh skill`、preview）でも導入できます。既定では最新のリリースタグが導入され、スコープは`project`（現在のリポジトリ内）です。ユーザー全体で使う場合は`--scope user`を付けます。

```sh
gh skill install nwiizo/suiko suiko --agent claude-code
```

導入後は`suiko --version`でCLIを確認し、Skill対応エージェントでは`$suiko`を指定します。Skillを先に導入した環境でCLIがない場合、Skillは`cargo install suiko`でCLIの導入を試み、`cargo`がない環境では導入手順の案内と同梱の手動チェックリストによる診断へ切り替えます。

## 対象範囲

Suikoは一般校正の網羅ではなく、均一なリズムや翻訳調、日本語文書の構造と読解負荷を再現可能に指さすことへ集中します。既存プロジェクトの表記規約や用語辞書は置き換えず、そのまま尊重します。誤字脱字、製品名の正規化、組織固有の表記統一は既存の工程へ残します。

## 品質基準

校正用フィクスチャを回帰テストに含めています。現時点の期待値は次のとおりです。

| fixture | 通常 | `--experimental` |
|---|---:|---:|
| AI的な文書 | 21 | 29 |
| 自然な文書 | 0 | 0 |

形態素解析にはsudachi.rsとSudachiDict core（版とSHA-256を`build.rs`で固定）を使います。形態素の分割結果そのものではなく、公開するJSON形状と校正フィクスチャに対するカテゴリ別の検出結果を回帰テストで固定します。

開発用評価集合には、出典と利用条件を記録した長い人間文書も含めています。現在の発火率、閾値を変更しなかった理由、評価集合が支えない結論は [eval/calibration.md](eval/calibration.md) に記録しています。

## 設計上の境界

初版には、パイプライン連携用の標準入力、複数ファイル入力、`--fail-on`、baseline比較、読解負荷レーン、プロジェクト設定を含めました。

一方、次の機能は意図的に含めません。

- 文埋め込みモデル: 構成した平板文と深掘り文で追加価値を実測したが、判別根拠が弱い一方で257 MiBのモデルキャッシュと初回取得が必要だったため採用しない。意味の進展は目視で確認する
- 自動修正: 事実、意図した反復、固有の文体を壊しうる判断は人間またはエージェントへ残す
- MCPサーバー・LSP: 連携3経路の実測（14ファイル約30万字をwarm 0.34秒で`--format github`/`--format sarif`、標準入力+JSONは1章0.05秒）で、常駐プロセスなしでもCI・エディタ・Agent利用が成立することを確認した。不足が実証されるまで採用しない
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
