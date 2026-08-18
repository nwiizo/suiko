# Changelog

Suikoの公開リリースを記録する。日付はJSTで、各項目は実測とテストに対応づける。

## [0.3.0] - 2026-08-19

テーマは「経験則の閾値と語彙を実測校正へ置き換える」。現代の人間文書81件（+青空文庫12件）を再現可能に取得・検証する基盤を作り、その実測でデフォルトの検出器構成を見直した。

### ハイライト

- **実測に基づくデフォルト構成の見直し**: 現代人間dev 75文書での校正により、`low_lexical_diversity_ttr`（文書長への構造依存。50k語の白書でTTR=0.094、fpr 0.613）、`repeated_sentence_lead`（絶対回数閾値の長さ交絡、fpr 0.613）、`low_lexical_diversity_mtld`（全候補閾値でAI検出0）の3検出器をEXPERIMENTAL（デフォルト無効、`--experimental`で利用可）へ降格した。判断ルールはeval/annotation-guide.mdに事前登録し、全実測はeval/calibration.mdに記録した
- **`suiko-eval calibrate` / `vocab`**: 人間fprのWilson 95%上限を制約にした閾値探索と、禁止語・誇張語彙の人間/AI出現実測（対数頻度比つき）。`--external`で非コミットの外部取得文書をSHA-256照合のうえ評価に使える。`low_specificity`の緩和候補-0.10は実測fpr 0.133で棄却、「のではないでしょうか」は人間6/65文書の実測で弱シグナル（info）へ変更
- **GitHub Releasesのビルド済みバイナリ配布**: macOS（Apple Silicon / Intel）、Linux（x86_64 / aarch64）、Windows（x86_64）の5ターゲットをタグpushで自動添付する

### 追加

- 検出器 `redundant_light_verb`: サ変名詞に隣接する「を行う/行なう」を確認候補（info）として指摘し、終止・連用・促音便の3活用に限って安全なsuggestion（「を行う」→「する」等）を付ける。受身（行われる）・使役（行わせる）・非隣接は対象外。ラベル付き14サンプル（detection 5/5、fpr 0/9）と実コーパス15件（真陽性15/15、全件で意味・声を保持）で事前登録した採用条件を満たした
- 読解負荷レーン（`--reading-load`）に `no_comma_sentence`: 60字以上の日本語散文に読点が1つもない文を指さす（岩淵悦太郎編『悪文』・本多勝一『日本語の作文技術』の句読法に基づく狭い下位事例。読点密度の検出はNO-GOのまま）。ラベル付き14サンプルと実コーパス真陽性2/2で確認した
- 文レベルの文頭接続詞率を`stats.conjunction`の観測値として追加した。本コーパスでは人間/AIを分離しなかったためfindingにはしない
- コーパス取得基盤: `eval/sources.toml`（人間93ソースのmanifest、coji/natural-japanese@0f1cc1cのsources.jsonをMIT出典明記で初期値化、unit単位のdev/holdout割当）、`scripts/fetch-corpus.py`（本文非コミットで取得し`external-lock.json`へSHA-256を記録。81/81件成功）、`scripts/generate-ai-corpus.sh`（未修正AI文書の生成と出典記録）。青空文庫の随筆12件を評価コーパスへ追加し、holdout splitを初めて充足した
- `suiko-eval report --split dev|holdout` と `--external`。閾値確定後のholdout一度きり評価を2026-08-19に実施した（退行なし。eval/calibration.md）
- Agent SkillにCLI不在時の自己導入手順（`cargo install suiko`）を追加し、READMEへ`gh skill install`とビルド済みバイナリでの導入方法を記載した

### 変更

- `low_lexical_diversity_ttr` / `low_lexical_diversity_mtld` / `repeated_sentence_lead` はEXPERIMENTALになった（上記ハイライト）。`--experimental`を付けない実行のfindingsからは出力されず、`suiko-eval labeled`のサンプル評価は従来どおり動く
- 「のではないでしょうか」を`forbidden_phrase`の弱シグナル（severity info）へ変更した

互換性: 出力JSONの形は不変。デフォルト実行では上記3カテゴリのfindingが出なくなる（`--experimental`で従来どおり出力）。`--baseline`比較では、旧baselineに含まれる3カテゴリのfindingがresolved扱いになる場合がある。

## [0.2.0] - 2026-08-18

### ハイライト

- `cargo install suiko` を復旧した。crates.io未公開のsudachi.rsを、Apache-2.0の条件に従った非公式再配布crate [suiko-sudachi](https://crates.io/crates/suiko-sudachi) 0.6.11として公開し、git依存を解消した。上流が公式にcrates.ioへ公開した時点でそちらへ乗り換える
- 形態素解析器をLindera/IPADICから [sudachi.rs](https://github.com/WorksApplications/sudachi.rs) v0.6.11 + SudachiDict 20260723 core（Mode C）へ切り替えた。辞書はビルド時に一度だけSHA-256固定で取得して埋め込み、実行時のダウンロードなしを維持する。回帰fixture上の検出差は`low_specificity`の1件だけだった

### 追加

- finding位置の`span`。行、Unicode scalar数えの列（1始まり）、行内UTF-8 byte範囲（半開区間）を持ち、同じ表現が一行に複数あっても一意に指せる
- 機械的に安全と確認した縮約だけを出す`suggestion`（現在は「〜することができる」→「〜できる」の1種）。preimageが原文と一致する場合だけ付与し、Suiko自身はファイルを書き換えない
- `lint --format github`（GitHub Actionsのworkflowコマンド注釈）と`lint --format sarif`（SARIF 2.1.0、`columnKind: unicodeCodePoints`）
- `terms --audit`。複数ファイルの用語候補を集計し、SudachiDictの正規化表記で表記揺れ（サーバー/サーバ等）を一覧化する読み取り専用レポート
- 複数ファイルの`--baseline`。前回の`lint --json`出力（配列）をそのまま渡し、`file`完全一致で照合する。追加ファイルは`baseline.file_status = "added"`、削除ファイルはstderr警告、genre・`--experimental`・Suikoバージョンの不一致は実行エラー。全recordへ`suiko_version`を追加した
- 局所AIパターン4カテゴリ: `bullet_bold_label`、`bullet_emoji`、`predicate_colon_lead`（形態素で名詞ラベルと区別）、`hype_expression`（info確認候補）
- 参考文献リスト行（`[1] …`、`[^1]: …`）とコード注釈行（`#A …`）を本文からマスクし、抑制行数を`stats.masking`へ出力
- 読者観測値`stats.readability`（平均文長、動詞・助詞比率、文字種比率）。難易度スコアは校正データが揃うまで実装しない
- 評価基盤: `corpus.toml`の`[[sample]]`正解ラベル（29件・13カテゴリ）と`suiko-eval labeled`、sweep 6ルール、Wilson 95%区間・分母・`low_n`・評価集合の版（manifest SHA-256）の出力、`split = dev/holdout`契約（sweepはdevのみ）、`eval/annotation-guide.md`
- Agent Skill導入の検証（`scripts/verify-skill-install.sh`と構造テスト）、辞書取得の`scripts/fetch-dictionary.sh`

### 変更

- `antithesis_repetition`と`repeated_sentence_lead`を文書単位の集約findingへ変更した。件数は「一致した箇所の数」ではなく「反復状態の数」を意味し、全対応箇所は`related_lines`で示す。母数は一致数で統一した
- `translationese_morph`を「が」型だけに絞った。「は」型（ことはできない）と使役型（させることができる）は、技術書翻訳21件の正解ラベル（言い換えが妥当14%）に基づき対象外にした
- 禁止語は行ごとの最初の1件ではなく、行内の全出現を報告する
- 用語集・FAQの定型フィールド（ラベル+コロン、`Q.`/`A.`）を散文の無意識な反復と区別する
- 校正fixtureの期待値はAI的な文書21件（`--experimental` 29件）、自然な文書0件

### 互換性

- 公開JSONは追加フィールドのみ（`span`、`suggestion`、`suiko_version`、`baseline.file_status`、`stats.masking`、`stats.readability`）。既存フィールドは不変
- crates.ioの0.1.0は切り替え前のLindera/IPADIC版。検出結果は0.2.0と異なる
- バイナリは埋め込み辞書（約207MB）を含むため200MB台になる

### ライセンス・出典

- sudachi.rs、SudachiDict（いずれもApache-2.0）、評価コーパスの青空文庫テキストの表示は`THIRD_PARTY_NOTICES.md`にまとめた

## [0.1.0] - 2026-08-17

初回リリース。`lint` / `outline` / `terms`、ジャンル別閾値、`--baseline`比較（単一ファイル）、読解負荷レーン、`--fail-on`、`.suiko.toml`、Agent Skillを含む。形態素解析はLindera/IPADIC。
