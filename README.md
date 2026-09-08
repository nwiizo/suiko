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
- `lexical-audit`: 一般名詞複合語の新奇性、表記正規化、明示したレジスター集合を読み取り専用で監査
- `academic`: 執筆者が記録した監査契約に照らして、論証と提出用成果物の不変条件を確認
- Markdownのfront matter、コードフェンス、インラインコード、リンクURL、埋め込み引用行、表、HTMLタグとコメント、参考文献リスト行（`[1] …`、`[^1]: …`）、「参考文献」「引用文献」「References」「Bibliography」見出し以下の行、コード注釈行（`#A …`）をマスク（抑制した行数は`stats.masking`に出力）
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

ビルド時に一度だけ、SudachiDict 20260723 core のzip（約69MB）を公式配布元からSHA-256固定で取得して埋め込みます。**実行時のダウンロードやファイル参照はありません。** 検証済みの `resources/system.dic` を配置するか、環境変数 `SUIKO_SUDACHI_DICT` で辞書ファイルを指定すれば、ビルド時の取得も行いません（オフラインビルド時は必須）。埋め込む辞書が約207MBあるため、バイナリは200MB台になります。

sudachi.rsはcrates.io未公開のため、Apache-2.0の条件に従った非公式再配布 [suiko-sudachi](https://crates.io/crates/suiko-sudachi)（v0.6.11そのまま、変更点はREADMEに明記）へ依存しています。上流が公式にcrates.ioへ公開した時点でそちらへ乗り換えます。

ソースから導入する場合は次のとおりです。

```sh
git clone https://github.com/nwiizo/suiko
cd suiko
cargo install --path .
```

リポジトリから直接試す場合は、以降の `suiko` を `cargo run --release --` に置き換えられます。

### ビルド済みバイナリ

Rustを入れずに使う場合は、[GitHub Releases](https://github.com/nwiizo/suiko/releases) の各リリースに添付されたビルド済みバイナリを使えます。対応ターゲットは macOS（Apple Silicon / Intel）、Linux（x86_64 / aarch64）、Windows（x86_64）で、各アーカイブに `SHA-256` ファイルが付きます。

```sh
# 例: macOS (Apple Silicon)
curl -LO https://github.com/nwiizo/suiko/releases/download/v0.3.4/suiko-v0.3.4-aarch64-apple-darwin.tar.gz
shasum -a 256 -c suiko-v0.3.4-aarch64-apple-darwin.tar.gz.sha256   # 事前に.sha256も取得した場合
tar xzf suiko-v0.3.4-aarch64-apple-darwin.tar.gz
./suiko-v0.3.4-aarch64-apple-darwin/suiko --version
```

macOSでは、ダウンロードしたバイナリに検疫属性（quarantine）が付くため初回実行がGatekeeperに止められます。`xattr -d com.apple.quarantine <suikoのパス>` で解除するか、確認ダイアログを避けたい場合は `cargo install suiko` で自分のマシンでビルドしてください（署名の出所が自分になるため、以降の確認が出ません）。

## 使い方

```sh
# 自然さを診断
suiko lint draft.md
suiko lint draft.md --genre tech --json

# 校正中の実験的な検出も加える
suiko lint draft.md --genre essay --experimental --json

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

# 固定参照資源に照らした一般語彙監査
suiko lexical-audit draft.md --reference data/lexical-reference-v1.json --json

# 標準入力
printf '重要なのは、結論です。\n' | suiko lint - --json
```

### 学術稿の論証・納品監査

`academic` は、学術論文の意味を自動採点する機能ではありません。中心命題、説明対象、用語の来歴、維持する論証順序、節間の橋、注の置き場所を、執筆者が先に記録した**監査契約**（Suiko内のローカルな機械可読作業記録）と照合します。契約があることで、推敲の都合だけで研究課題、章順、概念、結論を変えることを防ぎます。

```sh
suiko academic paper.md --contract academic-contract.json --json

# 提出用成果物まで確認する場合
suiko academic paper.md --contract academic-contract.json \
  --docx submission.docx --template official-template.docx \
  --pdf submission.pdf --export-record delivery-record.json
```

このレーンは、形式的なリサーチ・クエスチョンを置かない方針、用語の出典又は造語表示、防御的留保候補、本文引用と参考文献の著者・年又は`[@citation-key]`の双方向照合、段落第一文と節間の共有対象、本文・注・不要の注分類を確認します。引用符付きの分類名、公式値、短いラベルは`accepted_labels`へ登録できます。契約の論証順序、用語、第一文順、文体プロファイルは明示必須です。DOCXとPDFを片方でも指定した場合は、公式テンプレートと納品記録も必須になります。同期は本文・表・注・参考文献を双方向照合するため、旧文や余計な内容の残存も失敗になります。`delivery-record.json` のWord出力とPDF目視は自己申告であり、Suikoが実施事実を証明するものではありません。JSONの`passed`は指定された全チェックの合格、`delivery_ready`は提出可能な成果物監査まで通った状態を示します。

契約の完全な項目と、最終PDFの目視確認を含む手順は[academic-delivery.md](skills/suiko/references/academic-delivery.md)を参照してください。

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

機械的に安全と確認した縮約（現在は「〜することができる」→「〜できる」と、サ変名詞に隣接する「〜を行う」→「〜する」の2系統）には `suggestion`（`span`、`preimage`、`replacement`）が付きます。`preimage` が原文と一致する場合に限って適用できる条件で、Suiko自身はファイルを書き換えません。意味が変わりうるパターン（「することはできない」等）には候補を出しません。

`translationese_morph`は、形態素列に一致した読み直し候補を列挙します。たとえば抽象的な内容と「持つ」の組み合わせは、`意味+を+持つ`、`疑問節末のか+を+持つ`、`持てる+未決`だけを対象にします。「傘を持つ」「停止権限を持つ」「疑問を持つこと」は対象外です。findingは修正の要否を決めず、AIまたは人が周辺の文脈を読んで判断します。

`--format github` はfindingをGitHub Actionsのworkflowコマンド（`::warning file=...,line=...,col=...::`）として出力し、PRの該当行へ注釈を付けられます。severityは `critical→error` / `warn→warning` / `info→notice` に対応します。`--format sarif` はSARIF 2.1.0を出力し、`columnKind: unicodeCodePoints` を宣言して `span` の列をそのまま使います（severityは `error` / `warning` / `note`）。

`terms --audit` は複数ファイルの用語候補を集計し、SudachiDictの正規化表記で表記揺れ（サーバー/サーバ等）をクラスタして返します。読み取り専用で、置換や辞書の書き込みは行いません。

`lexical-audit`は既存`terms`のJSONを変更しない独立レーンです。固定フォーマットと小標本は`data/lexical-reference-v1.json`に収録しています。参照JSONは`version: 1`、`source`、`known_compounds`、`corpus_counts`、`register_sets`を持ちます。一般名詞・接尾辞・必要な形状詞の連続を候補化し、`forbidden_match`、`orthographic_variation`、`register_variation`、`novel_compound`を根拠別に返します。新奇複合語は本文5回以下、参照コーパス1回以下、定義手掛かりなし、登録なしをすべて満たし、参照資源にコーパス頻度が明記された場合だけinfoになります。参照資源にない語を頻度0とはみなしません。Sudachiだけでは意味的同義性を決められないため、漢語・和語等のレジスター比較は参照資源に明示した集合だけを扱います。自動置換や原稿の書き換えは行いません。

`lint --json` の `stats.readability` には平均文長、動詞・助詞比率、文字種比率の観測値が入ります。読者別の難易度スコアは、正解ラベル付きコーパスで校正できるまで実装しません（観測値のみを提供します）。

`stats.rhythm.sentence_endings` には、文末を `assertive`（明示的な断定）、`tentative`（推量・保留）、`question`（疑問）、`nominal`（体言止め）、`other` に近似分類した件数と、空行をまたがない最長連続数が入ります。これは文章の良否を決める値ではなく、局所的なリズムを確認するための観測値です。6文以上の文書で `--experimental` を指定すると、30モーラ以上で同じ明示的文末が3文以上続き、文長の変動係数が0.15以下の箇所を `repeated_sentence_mode`、25モーラ以下の体言止めが3文以上続く箇所を `consecutive_nominal_endings` として指さします。

`abstract_metaphor` は、地図、羅針盤、道標、土台、架け橋などの名詞が、抽象的な対象の役割を表す述語や「〜の〜」型で使われた箇所を `info` で指さします。候補語の出現だけでは発火せず、地理情報の表示や船具の点検など本来の意味での用例は対象外です。比喩かどうかを断定せず、判断対象、判断基準、具体的な効果を明記できるか確認するためのfindingです。CIで必ず止める場合は `--fail-on info` を使い、必要な用例は `.suiko.toml` の `allow` へ理由付きで記録します。

`--baseline` には前回の `lint --json` 出力（単一オブジェクトまたは配列）をそのまま渡せます。レコードは `file` 文字列の完全一致で対応づけ、改名は推測しません。baselineにないファイルは全findingを新規として `baseline.file_status = "added"` で示し、baselineにあって今回対象にないファイルはstderrへ警告します。genre、`--experimental`、Suikoバージョンが一致しない場合は実行エラーになります。`antithesis_repetition` や `low_burstiness` のような文書単位のfindingは、文章の言い換えで抜粋が変わっても同一カテゴリとして継続扱いします。

`antithesis_repetition` と `repeated_sentence_lead` は文書単位の集約findingです。同じ反復キーは1件にまとめ、全対象行を `related_lines` で示します。finding件数は「一致した箇所の数」ではなく「反復状態の数」を意味します。文頭のラベル+コロン（用語集やFAQの定型フィールド）は、散文の無意識な反復と区別して `detail` に明記します。

### `--experimental`で反復・指示範囲・技術現場の言い回しを確認する

次の6カテゴリは、形態素列から観測できる反復、指示範囲、技術現場の比喩的な言い回しを示す実験的なfindingです。文法的な誤りや文章の優劣を判定するものではありません。いずれもseverityは`info`で、通常の`lint`では出力されません。AIが書いたかどうかは判定せず、読み直す箇所を列挙するために使います。

| category | 検出する状態 | 対象外になる例 |
|---|---|---|
| `self_labeling_repetition` | `必要なのは`、`面白いのは`、`避けたいのは`など、評価語を含む「〜のは」型の主題提示が文書内に3回以上ある | 1〜2回だけの使用、`正直に言うと`など書き手の立場を示す表現 |
| `negative_listing` | 同じ段落で`Xではない。Yでもない。Zだ。`のように、否定が2文続いたあと形態素8個以下の肯定文へ焦点を移す | 空行をまたぐ3文、長い説明へ続く否定文 |
| `uniform_bullet_structure` | `--genre essay`を指定し、4項目以上の連続した箇条書きで文末品詞がすべて同じになり、内容語数の変動係数が0.25以下になる | ジャンル未指定、`tech`、`business`、3項目以下、文末品詞や内容語数のばらつきが大きい箇条書き |
| `demonstrative_reference` | `--genre tech`を指定し、同じ文の前方に動詞が2個以上ある位置で`このこと`、`そのこと`、`あのこと`を使う | 前方の動詞が1個以下、別の文で指示対象を受ける場合 |
| `respectively_scope` | `--genre tech`を指定し、列挙の後に`それぞれ`があり、後方に対応する列挙がない | `R、G、Bは、それぞれRed、Green、Blueに対応する`のように両側の列挙が形態素上で見える場合 |
| `technical_jargon_metaphor` | `--genre tech`を指定し、テストやCIの成功状態を`緑`で表すか、コードの公開を`出荷`で表す | アイコンや画面の実際の表示色、工場や製品の物理的な出荷 |

反復を扱う3カテゴリは文書内の該当箇所を1件へまとめ、対象行を`related_lines`で返します。`self_labeling_repetition`は「〜のは」型だけを扱い、書き手の立場を示す表現は混ぜません。`negative_listing`は意図的な対比にも使えるため、誤りではなく修辞を確認するきっかけとして示します。`uniform_bullet_structure`はMarkdownのコードフェンスと引用内を検査せず、明示的に`essay`を選んだ場合だけ出力します。文末品詞と内容語数の近さを測るもので、項目間の意味関係や構文上の並列性までは判定しません。

技術文書向けの3カテゴリは、読者が複数の解釈で迷う可能性がある箇所のうち、形態素列で位置を示せる指示語、`それぞれ`の範囲、技術現場の比喩的な言い回しに絞っています。`technical_jargon_metaphor`は同じ節にある対象語と状態語の組み合わせだけを扱い、表示や物理配送を示す語が同じ文にあれば除外します。いずれも表現の誤りとは断定せず、AIまたは人が前後関係を確認する候補だけを返します。

主語の省略、修飾先、照応先、節をまたぐ並列関係は、形態素列だけでは正誤を決められないためfindingにしません。

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

Node.js 20.18以降とnpmが使える環境では、Agent Skillから[@textlint-ja/textlint-rule-preset-ai-writing](https://github.com/textlint-ja/textlint-rule-preset-ai-writing)による補助検査も実行できます。同梱スクリプトが固定バージョンをnpmの一時環境で実行するため、対象プロジェクトの依存関係や設定ファイルは変更しません。結果はSuikoのfinding、自然度スコア、baselineへ加算せず、自動修正も行いません。利用できない環境ではSuikoだけで診断を続けます。

## 対象範囲

Suikoは一般校正の網羅ではなく、均一なリズムや翻訳調、日本語文書の構造と読解負荷を再現可能に指さすことへ集中します。既存プロジェクトの表記規約や用語辞書は置き換えず、そのまま尊重します。誤字脱字、製品名の正規化、組織固有の表記統一は既存の工程へ残します。

## 品質基準

校正用フィクスチャを回帰テストに含めています。現時点の期待値は次のとおりです。

| fixture | 通常 | `--experimental` |
|---|---:|---:|
| AI的な文書 | 19 | 28 |
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
