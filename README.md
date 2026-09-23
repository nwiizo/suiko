# Suiko（推敲）

[![crates.io](https://img.shields.io/crates/v/suiko.svg)](https://crates.io/crates/suiko)
[![CI](https://github.com/nwiizo/suiko/actions/workflows/ci.yml/badge.svg)](https://github.com/nwiizo/suiko/actions/workflows/ci.yml)
[![license: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/nwiizo/suiko/blob/main/LICENSE)

日本語文書の翻訳調、近接した反復、単調なリズム、読解負荷を診断するRust CLIです。Markdownやテキストから、推敲で読み直す箇所とその理由を返します。

診断はローカルで実行します。形態素辞書はバイナリに埋め込み、実行時の辞書・モデル取得はありません。原稿を書き換えず、指摘を採用するかは書き手やエージェントが文脈から判断します。AIが書いた確率や、文章の品質を表す総合スコアは出しません。

## できること

| コマンド | 読み直す作業 |
|---|---|
| `lint` | 翻訳調、定型表現、反復、リズム、段落構造と、長い文や埋もれた列挙などの読解負荷を確認する |
| `outline` | 見出し・段落の先頭文・箇条書きから論旨を俯瞰する |
| `terms` | 専門用語候補、初出時の説明、複数ファイルの表記揺れを確認する |
| `lexical-audit` | 参照データを使って一般名詞複合語や語彙の揺れを確認する |
| `academic` | 執筆者が記録した方針と論証・引用・提出用成果物を照合する |

## インストール

### Cargo

Rust 1.97以降が必要です。

```sh
cargo install suiko --locked
suiko --version
```

### ビルド済みバイナリ

Rustを入れずに使う場合は、[GitHub Releases](https://github.com/nwiizo/suiko/releases)から取得できます。macOS（Apple Silicon / Intel）、Linux（x86_64 / aarch64）、Windows（x86_64）に対応し、各アーカイブにSHA-256ファイルが付きます。

```sh
# v0.3.10 / macOS（Apple Silicon）
curl -fLO https://github.com/nwiizo/suiko/releases/download/v0.3.10/suiko-v0.3.10-aarch64-apple-darwin.tar.gz
curl -fLO https://github.com/nwiizo/suiko/releases/download/v0.3.10/suiko-v0.3.10-aarch64-apple-darwin.tar.gz.sha256
shasum -a 256 -c suiko-v0.3.10-aarch64-apple-darwin.tar.gz.sha256
tar xzf suiko-v0.3.10-aarch64-apple-darwin.tar.gz
./suiko-v0.3.10-aarch64-apple-darwin/suiko --version
```

以降の例で`suiko`として実行するには、展開した実行ファイルをPATHの通ったディレクトリへ配置してください。

### ソースからのビルドと辞書

```sh
git clone https://github.com/nwiizo/suiko
cd suiko
cargo install --path . --locked
```

ビルド時にSudachiDict 20260723 coreのzip（約69MB）を取得し、zipと辞書本体のSHA-256を検証して埋め込みます。辞書が約207MBあるため、バイナリは200MB台になります。検証済みの辞書を`resources/system.dic`に置くか、環境変数`SUIKO_SUDACHI_DICT`で指定すれば、ビルド時の辞書取得も省けます。オフラインビルドでは、Rust依存のキャッシュに加えて、この辞書の配置が必要です。

形態素解析には[sudachi.rs](https://github.com/WorksApplications/sudachi.rs) v0.6.11を非公式に再配布した[suiko-sudachi](https://crates.io/crates/suiko-sudachi)を使います。再配布に関する説明は[同crateのREADME](https://github.com/nwiizo/suiko/blob/main/crates/suiko-sudachi/README.md)にあります。

## lintで原稿を確認する

```sh
suiko lint draft.md
suiko lint draft.md --genre tech --json
suiko lint docs/*.md --genre tech

# 標準入力は - で受け取る
printf '重要なのは、結論です。\n' | suiko lint - --json
```

`--genre`には`essay`、`tech`、`business`を指定でき、ジャンル別の閾値を使います。一部のルールはジャンルを明示した場合だけ有効です。各指摘（finding）にはカテゴリ、対象行、抜粋、理由、重要度が付きます。必要な比較や意図した反復は、そのまま残せます。

散文の検出では、コード、見出し、箇条書き、引用行、表、参考文献などを除外します。見出しや箇条書き自体を扱う構造ルールは、それぞれの対象を調べます。front matter、リンクURL、HTMLタグ・コメント、コード注釈行（`#A …`）も本文と区別します。参考文献行とコード注釈行の除外数は`stats.masking`で確認できます。

### 技術文書の近接した反復

次の2カテゴリは、`--genre tech`の通常検出です。近くに続く文末や挿入表現をまとめて読み直すために使います。

| category | 検出する状態 |
|---|---|
| `repeated_distinction` | 同じ節の5文以内に、`別物だ／です／である`で終わる文が3文以上ある。疑問・否定・引用への接続は除く |
| `repeated_em_dash` | 同じ節の5文以内に、文中の`—`・`―`を使う文が3文以上ある。数字同士の範囲や単独の罫線は除く |

各カテゴリを文書内の1件へまとめ、近接した反復の対象行を`related_lines`で返します。章ごとに一度ある説明を合算せず、太字の有無で判定を変えません。重要度は`info`で、自動修正の候補は付きません。`--fail-on info`を指定すると、これらの指摘も終了コードに影響します。

### 実験的な検出

```sh
suiko lint draft.md --genre tech --experimental --json
```

`--experimental`は、校正途中のルールも有効にします。表現の一致が有用な指摘になるか、文脈を読んで確認するための機能です。主な確認候補は次のとおりです。

| category | 検出する状態・対象ジャンル |
|---|---|
| `self_labeling_repetition` | 評価語を含む「〜のは」型の主題提示が文書内に3回以上ある |
| `short_topic_comma` | 文頭の5文字以内の名詞句＋「は」の直後に読点「、」がある。長い主題や動詞を含む節は除く |
| `negative_listing` | 同じ段落で否定文が2文続き、形態素8個以下の肯定文へ続く |
| `uniform_bullet_structure` | `essay`で、4項目以上の箇条書きの文末品詞がそろい、内容語数のばらつきが小さい |
| `demonstrative_reference` | `tech`で、同じ文の前方に動詞が2個以上ある位置に「このこと」等がある |
| `respectively_scope` | `tech`で、列挙の後に「それぞれ」があり、後方に対応する列挙が見えない |
| `repeated_explanation_preview` | `tech`で、同じ節の段落頭に「本節では〜説明します」等の予告が3回以上ある |
| `declared_item_count_mismatch` | 全ジャンルで、リスト直前の段落の最後の文が「次の3点」「以下の三つ」等と予告し、直後の箇条書きの同じ階層の項目数と一致しない。入れ子・項目内のコード・「以上」「のうち」・省略記号は数えない |
| `technical_jargon_metaphor` | `tech`で、CIの成功を「緑」、コードの公開を「出荷」と表すなど、技術現場の比喩的な言い回しがある |
| `repeated_sentence_mode` / `consecutive_nominal_endings` | 長さの近い明示的な文末や、短い体言止めが局所的に続く |

`technical_jargon_metaphor`は、技術対象に続く「静かに壊れる」「黙って捨てる／無視する」「地味に効く」「安全側／保守側に倒す」と、「時間を溶かす」も対象にします。活用と近くの名詞・助詞を確認し、候補語の出現だけでは判定しません。

通常の`abstract_metaphor`は、抽象的な対象を「地図」「土台」等の役割で表す用例を扱います。`--genre tech --experimental`では、「仕様は意図を実装へ運ぶ」のような抽象語の関係や、抽象的な「入口」「主役」等も加えます。比喩の追加分は、説明済みの内容や体験談にも一致するため、実験機能に留めています。

これらは`info`の読み直し候補です。形態素解析だけでは主語の省略、指示先、修飾先の正誤を決められません。[検出仕様と形態素解析の手順](https://github.com/nwiizo/suiko/blob/main/eval/technical-wording.md)に、候補一覧、採用理由、実測した分割、評価結果を記録しています。

### 読解負荷を確認する

```sh
suiko lint draft.md --genre tech --json
```

`lint`は自然度の診断と同時に、長すぎる一文、読点のない60字以上の一文、埋もれた列挙、疑問節の埋もれた列挙、長い連続漢字、二重否定、格助詞「の」の近接した連鎖、一つの名詞に前置された長い修飾節を確認します。v0.3.10から既定で有効です。結果は`findings`と分けた`reading_load`へ出力し、`--baseline`比較と`--fail-on`判定には含めません。不要な場合は`--no-reading-load`で出力から外せます。v0.3.9以前の`--reading-load`も互換のため受け付けます。

`long_attributive_span`は、述語を2つ以上含む30字以上の連体修飾節が一つの実質名詞に係り、その名詞句が「が」「を」「は」などで主節の項になっている文を指さします。読み手は名詞が出るまで修飾節全体を保留するため、被修飾名詞か述語を先に出すか、修飾節を独立した文に分けると読みやすくなります。「〜すること」「〜したとき」「〜する必要がある」のような形式名詞・枕の名詞と、「〜という関係である」の述語名詞は対象外です。連体形の個数で判定して人間文書で全発火した旧`nested_attributive`とは異なり、一つの名詞が背負う修飾節の長さと述語数を測ります。v0.3.8で追加し、採用の経緯と実測は[校正記録](https://github.com/nwiizo/suiko/blob/v0.3.8/eval/calibration.md)にまとめています。

`buried_question_list`は、「〜が増えたか、〜も短くなったかを追います」のように疑問節「〜か」を読点で並べ、末尾の「か」に「を」「が」「は」「も」を付けて一つの述語へ係らせる文を、並んだ節が35字以上・文全体が45字以上のときに指さします。読み手は最初の「〜か、」で節が終わったと受け取り、最後まで読んで全体が一つの項だったと分かります。係り先の述語を先に出すか、節ごとに文を切ると読みやすくなります。「はいか、いいえかを」のような述語のない選択肢、「〜かどうか」、推量の「〜かもしれない」と、「〜なのか、それとも〜なのかを」「〜なのか、〜なのかを分ける」のような2節の選択疑問は対象外です。v0.3.10で追加しました。

## 診断結果とCI連携

`lint --json`は、単一ファイルなら`file`、`suiko_version`、`stats`、`findings`を持つオブジェクト、複数ファイルなら同じレコードの配列を返します。

| findingのフィールド | 内容 |
|---|---|
| `category` / `severity` | ルール名と重要度（`info` / `warn` / `critical`） |
| `line` / `excerpt` / `detail` | 対象行、抜粋、確認する理由 |
| `span` | 一意に示せる原文の範囲。文書全体の指標では省略 |
| `related_lines` | 集約した反復などの対象行 |
| `suggestion` | 対応する縮約に付く`span`、`preimage`、`replacement` |

`span`の行・列は1始まりで、終端は最後の文字の次を指します。列はUnicode scalar単位で、全角文字も結合文字も各1と数えます。`start_byte`と`end_byte`は各行内のUTF-8 byte offsetで、0始まりです。同じ表現が一行に複数あっても別の範囲を返します。

「〜することができる」→「〜できる」、サ変名詞に隣接する「〜を行う」→「〜する」には、条件を満たした場合に`suggestion`が付きます。適用前に`preimage`と原文の一致を確認してください。Suiko自身は変更を適用しません。

反復を集約するfindingの件数は、一致箇所の総数とは異なります。`related_lines`と併せて確認してください。`stats.readability`の平均文長や品詞・文字種比率、`stats.rhythm.sentence_endings`の文末分類と連続数は、文章の特徴を知る観測値です。

```sh
# warn以上があれば終了コード2を返す
suiko lint docs/*.md --genre tech --fail-on warn

# GitHub Actionsのworkflowコマンドとして注釈を出す
suiko lint docs/*.md --genre tech --format github --fail-on warn

# SARIF 2.1.0に対応するエディタやコードスキャンへ渡す
suiko lint docs/*.md --genre tech --format sarif > suiko.sarif
```

`--format github`では`critical` / `warn` / `info`を`error` / `warning` / `notice`へ対応づけます。SARIFでは`error` / `warning` / `note`を使い、列の単位は`unicodeCodePoints`です。読解負荷の指摘は重要度にかかわらず`notice`（SARIFでは`note`）で出ます。注釈に含めない場合は`--no-reading-load`を付けてください。

`lint`の終了コードは次のとおりです。`--fail-on`は、実験機能を含む`findings`内の指摘を判定します。

| code | 意味 |
|---:|---|
| 0 | 実行成功。設定した閾値以上の指摘がない。閾値未設定なら指摘の有無は問わない |
| 1 | 入力、形態素解析、JSONなどの実行エラー |
| 2 | 設定した閾値以上の指摘を検出 |

## 前回の診断と比較する

```sh
suiko lint docs/*.md --genre tech --json > /tmp/suiko-before.json
# 原稿を推敲した後、同じファイル群を再検査する
suiko lint docs/*.md --genre tech --baseline /tmp/suiko-before.json --json
```

前回のJSONを渡すと、指摘を`resolved`（解消）、`new`（新規）、`persisting`（継続）へ分類します。`file`文字列の完全一致で対応づけるため、ファイルの指定方法をそろえてください。改名は推測しません。前回になかったファイルは`baseline.file_status = "added"`となり、今回の対象から外れたファイルはstderrへ警告します。

ジャンル、`--experimental`、Suikoのバージョンが一致しない比較は実行エラーになります。設定による除外も比較に影響するため、同じ設定で実行してください。文書単位の反復は、抜粋が変わっても同じ反復状態が続けば継続扱いします。`--fail-on`は新規分だけでなく、今回の`findings`全体を判定します。

## プロジェクト設定

`lint`はカレントディレクトリの`.suiko.toml`を自動的に読み込みます。親ディレクトリは探索しません。

```toml
version = 1
genre = "tech"
fail_on = "warn"
disabled_rules = ["low_specificity"]

[[allow]]
category = "forbidden_phrase"
text = "重要なのは"
reason = "連載で意図的に使う表現"
```

- `genre`と`fail_on`は省略可能な既定値で、CLI引数が優先されます。
- `disabled_rules`は`findings`と読解負荷の該当カテゴリを無効にします。
- `allow`は同じ`category`で、`excerpt`に`text`を含む指摘を除外します。`reason`は必須です。
- `--config <path>`で設定を指定でき、`--no-config`で読み込みを無効にできます。
- 未知のキーやルール、空の`text`・`reason`、`version = 1`以外は実行エラーです。

設定による除外は、指摘件数の集計、前回比較、終了コードの判定より前に適用します。この設定は`lint`用です。

### プロジェクト独自の形態素ルール

プロジェクトで読み直したい言い回しを、`.suiko.toml`の`word_rules`に追加できます。たとえば、値の「変更」と「移動」を使い分けたい場合は次のように記録します。

```toml
version = 1

[[word_rules]]
id = "value-change"
message = "増減・変更・移動のどれを指すか確認してください。"
severity = "info"
tokens = [
    { surface = "値", pos = "名詞" },
    { surface = "を", pos = "助詞" },
    { dictionary_form = "動かす", pos = "動詞" },
]
```

既存の設定へ追記する場合、`version = 1`は重ねて書きません。設定したルールは通常の`lint`で有効です。「値を動かしました」「値を動かさない」にも一致し、`category: "custom_wording/value-change"`、原文の範囲、指定したメッセージを返します。指摘は読み直し候補で、自動修正は付きません。

- `surface`は表層、`dictionary_form`は活用前の基本形、`pos`はSudachiの品詞大分類です。各トークンで指定した条件をすべて満たす、隣接した形態素列に一致します。
- 文・改行・空白・コード等の除外箇所をまたぎません。文字列の部分一致や正規表現は使わず、外部プリセットの辞書ファイルは直接読み込みません。
- `id`は英小文字・数字・ハイフン・アンダースコアで一意に付けます。`message`と1件以上の`tokens`は必須で、空の条件や未知のキー・品詞はエラーです。
- `severity`は`info`（省略時）、`warn`、`critical`から選べます。`--fail-on`、`allow`、`disabled_rules = ["custom_wording"]`にも対応します。個別ルールを止める場合はその定義を取り除きます。
- `allow`には`category = "custom_wording"`を指定します。`rule_id = "value-change"`も加えると、そのルールだけを個別許可できます。IDを省略すると、指定の抜粋に一致したすべての独自ルールが許可されます。
- 結果の`category`は`custom_wording/<id>`です。件数の集計、前回比較、GitHub注釈、SARIFもID別に扱い、同じ抜粋に一致した別ルールを混同しません。ルールを変更して比較する場合は、その変更も結果に含まれます。

独自ルールと短い主題の読点はv0.3.7で追加しました。既存の設定と通常ルールのJSONはそのまま使えます。[比較調査と検証記録](https://github.com/nwiizo/suiko/blob/v0.3.7/eval/word-rules.md)に、採用した機能と検証の限界をまとめています。

## 構成と用語を確認する

```sh
suiko outline draft.md --json
suiko terms draft.md --json
suiko terms --audit docs/*.md --json
```

`terms --audit`は用語候補を複数ファイルで集計し、Sudachiの正規化表記を使って「サーバー／サーバ」等の表記揺れをまとめます。置換や辞書への書き込みは行いません。

`lexical-audit`は、一般名詞複合語の頻度や語彙の揺れを、明示した参照JSONと照合します。[参照データの例](https://github.com/nwiizo/suiko/blob/main/data/lexical-reference-v1.json)は動作確認用の小標本です。次の例はリポジトリ直下で実行します。

```sh
suiko lexical-audit draft.md --reference data/lexical-reference-v1.json --json
```

参照JSONは`version: 1`、`source`、`known_compounds`、`corpus_counts`、`register_sets`を持ちます。未知の語を頻度0とはみなさず、新奇複合語は本文5回以下、参照コーパス1回以下、定義手掛かりなし、登録なしの条件がそろった場合だけ報告します。漢語・和語などの比較は、参照データに明示した集合に限ります。[語彙監査の設計](https://github.com/nwiizo/suiko/blob/main/docs/adr/0002-offline-lexical-audit.md)も参照してください。

## 学術稿と提出用成果物を確認する

`academic`は、執筆者が先に記録した監査方針と原稿を照合します。中心命題、説明対象、論証順序、用語の来歴、節間のつながり、引用と参考文献、注の分類を確認し、推敲で維持すべき事項の変更を見つけます。

```sh
suiko academic paper.md --contract academic-contract.json --json

# 提出用成果物まで確認する場合
suiko academic paper.md --contract academic-contract.json \
  --docx submission.docx --template official-template.docx \
  --pdf submission.pdf --export-record delivery-record.json --json
```

監査方針のJSONでは、論証順序、用語、段落第一文の順序、文体プロファイルを明示します。成果物の監査ではDOCX、PDF、公式テンプレート、出力記録をそろえ、本文・表・注・参考文献の同期も検査します。JSONの`passed`は指定した検査の合格、`delivery_ready`は提出用成果物まで検査が通った状態です。Wordからの出力とPDF目視の記録は自己申告であり、その実施事実をSuikoが証明するものではありません。

JSONの例と提出前の手順は[学術稿と提出用文書の監査](https://github.com/nwiizo/suiko/blob/main/skills/suiko/references/academic-delivery.md)にあります。

## Agent Skill

[`skills/suiko/SKILL.md`](https://github.com/nwiizo/suiko/blob/main/skills/suiko/SKILL.md)は、文書設計、執筆、診断結果の採否、再検査を扱います。各指摘を「直した」または「残す（理由）」へ分類しながら推敲を進めます。

CLIとSkillは別々に導入します。次のコマンドでSkillを導入できます。

```sh
npx skills add https://github.com/nwiizo/suiko --skill suiko
```

GitHub CLIの`gh skill`（preview）も利用できます。既定では最新のリリースタグから現在のプロジェクトへ導入し、`--scope user`を付けるとユーザー全体で使えます。

```sh
gh skill install nwiizo/suiko suiko --agent claude-code
```

導入後は、Skill対応エージェントで`$suiko`を指定します。CLIがなければSkillが`cargo install suiko`を試み、`cargo`もない場合は導入案内と手動チェックリストへ切り替えます。

Node.js 20.18以降とnpmがある環境では、Skillから[@textlint-ja/textlint-rule-preset-ai-writing](https://github.com/textlint-ja/textlint-rule-preset-ai-writing)による補助検査も実行できます。固定バージョンをnpmの一時環境で実行し、対象プロジェクトの依存関係や設定は変更しません。結果はSuikoのfindingや前回比較へ加算せず、自動修正も行いません。

## 検証と開発

検出位置、除外条件、JSON、前回比較、終了コードを回帰テストで確認します。技術文書の追加ルールは、自作10文書・24項目の評価で期待どおりの検出を確認しました。これは仕様の確認であり、一般的な誤検出率や文章の改善効果を実証した結果ではありません。編集時間の短縮や文章の改善率は未測定です。

評価の対象と限界は[技術文書の検出検証](https://github.com/nwiizo/suiko/blob/main/eval/technical-wording.md)、既存ルールの評価は[校正記録](https://github.com/nwiizo/suiko/blob/main/eval/calibration.md)に記録しています。誤字脱字や組織固有の表記統一は、既存の校正工程と組み合わせてください。

リポジトリのCIと同じ確認コマンドです。

```sh
cargo +1.97 fmt --check
cargo +1.97 clippy --locked --all-targets --all-features -- -D warnings
cargo +1.97 test --locked --all-features --all-targets
```

評価用CLIは`evaluation` featureを有効にした開発ビルドで使います。通常の配布バイナリには含めません。

```sh
cargo +1.97 run --locked --features evaluation --bin suiko-eval -- labeled eval/technical-wording.toml
cargo +1.97 run --locked --features evaluation --bin suiko-eval -- report eval/corpus.toml
```

評価データの準備と閾値の比較は[評価手順](https://github.com/nwiizo/suiko/blob/main/eval/README.md)、リリースごとの変更は[CHANGELOG](https://github.com/nwiizo/suiko/blob/main/CHANGELOG.md)を参照してください。

## ライセンス

MIT。第三者由来の資料とフィクスチャに必要な表示は[THIRD_PARTY_NOTICES.md](https://github.com/nwiizo/suiko/blob/main/THIRD_PARTY_NOTICES.md)に収録しています。
