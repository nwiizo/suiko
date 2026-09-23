# Changelog

Suikoの公開リリースを記録する。日付はJSTで、各項目は実測とテストに対応づける。

## [0.3.10] - 2026-09-24

### 追加

- 読解負荷レーン（`reading_load`）に`buried_question_list`を追加した。「〜が増えたか、〜も短くなったかを追います」のように、述語を含む疑問節「〜か、」を並べ、末尾の「か」＋を・が・は・も で一つの述語へ係らせる文を`info`で指さす。並んだ節が35字以上、文全体が45字以上のときに限る。「はいか、いいえかを」のような述語のない選択肢、6字未満の節、「〜かどうか」「〜か否か」、推量の「〜かもしれない」、「〜かで」と、「〜なのか、それとも〜なのかを」「〜なのか、〜なのかを分ける」のような2節の選択疑問は対象外。ラベル付きサンプル fire 10/10・silent 0/12（Wilson上限0.243）で事前登録条件を満たし、実文書99件では3件だった（eval/calibration.md、#34）

### 修正

- `terms`の`has_gloss_hint`が「と呼びます」「と言います」「と名づけました」のような活用形・丁寧体を説明の手掛かりとして数えなかった。助詞「と」＋「呼ぶ」「言う」「称する」「名付ける」を形態素で照合し、終止形以外も一致させた。`terms --audit`の`has_gloss_hint`にも反映される（#33）
- 読解負荷の`no_chain`が「〜するのは」「〜なの」の準体助詞まで「の」として数えていた。メッセージどおり格助詞の「の」だけを数える。実文書99件で161件から142件になった（Natural Japaneseとの[実行比較](https://github.com/nwiizo/suiko/blob/v0.3.10/eval/competitive-review-2026-09-24.md)で判明）
- 書籍原稿2冊の点検で見つかった誤検知を直した。`double_negative`は「〜ないかもしれません」の推量、疑問の「か」の節の中にある否定、「際限なく」のような副詞、理由の「から」で切れた節、「足りず区別できない」「確かめずに推測していない」を同じ命題の二重否定として数えない。`no_comma_sentence`と`long_attributive_span`は「A → B → C」の図式と括弧の外に「＝」がある式を対象から外す。`english_syntax_inanimate_subject`は「〜を証明することとは別です」のように名詞化した動詞を述語とみなさない。`forbidden_phrase`は「まとめるとき」の中の「まとめると」を数えない。評価コーパス99文書で`double_negative`は126件から106件、`no_comma_sentence`は155件から153件（eval/calibration.md）
- `buried_list`が「仕事、睡眠、食事、人間関係は」のような一語の並びや、「〜を試したところ」のような副詞節、書誌の「（著）、（訳）」まで指していた。最後の項目より前の並びが20字以上か6項目以上の列挙に限り、名詞化した「〜したこと」の並びは残す。この検出器に初めてラベル付きサンプル（fire 5/5・silent 0/10）を加えた。評価コーパス99文書で236件から149件

### 変更

- `--experimental`の`repeated_sentence_lead`は、「確認待ちが」のような普通名詞・固有名詞で始まる文頭（名詞主題）の反復を報告しない。「ただ、」「ここで」「今回は」のような接続詞・副詞・指示語・副詞可能の名詞の反復と、コロンやQ./A.の定型フィールドは従来どおり返す。実文書99件で人間文書の発火は55件から42件、AI文書は1件のまま（#35）
- clap 4.6.7、pdf-extract 0.12.1、ureq 3.4.2へ更新した（#27、#30、#31）
- `lint`は読解負荷レーン（`reading_load`）を既定で出力する。これまでは`--reading-load`を付けたときだけだった。自然度の`findings`、`--fail-on`判定、`--baseline`比較には従来どおり含めない。`--no-reading-load`で出力から外せ、`--reading-load`も互換のため受け付ける。`--format github`と`--format sarif`でも読解負荷の指摘が`notice`・`note`として出る
- 読解負荷の処理を速くした。文の長さを数えるたびに正規表現をコンパイルし直していたため、16万字の文書で読解負荷に約1.3秒かかっていた。一度だけコンパイルするようにし、同じ文書の`lint`全体が1.6秒から0.40秒、評価コーパス99文書が9.7秒から4秒前後になった
- 既存タグへリリースのワークフローを再実行したとき、アーカイブとSHA-256が公開済みのターゲットはビルドとアップロードを飛ばす。v0.3.9の再実行が`ReleaseAsset.name already exists`で失敗していた

互換性: JSONの各フィールドの形式、既存の設定、Rust公開型`Finding`のフィールドは不変。`--reading-load`を付けない`lint --json`にも`reading_load`が加わるため、キーの有無で判定しているスクリプトは`--no-reading-load`を付ける。`reading_load`へ新しいカテゴリが返ることがあり、`disabled_rules = ["buried_question_list"]`で無効にできる。`no_chain`、`double_negative`、`buried_list`、`no_comma_sentence`と実験機能の`repeated_sentence_lead`は件数が減ることがある。旧版のbaselineはバージョンの照合で拒否されるため、v0.3.10で作り直す必要がある。sudachi.rs v0.7.0は辞書形式が変わり、公式のV1辞書がまだないため、このリリースでは上げない（#37）。

## [0.3.9] - 2026-09-23

### 追加

- `--experimental`に`declared_item_count_mismatch`を追加した。リスト直前の段落の最後の文にある「次の／以下の／下記の＋数詞＋助数詞（点・つ・項目など）」の予告と、直後の箇条書きの同じ階層の項目数を照合し、一致しない場合に宣言の位置と項目の行を`info`で返す。算用数字・全角数字・漢数字に対応し、入れ子の子項目、項目内の補足段落やコード、空行を挟む項目の区切りを正しく扱う。「以上」「程度」「のうち」を伴う宣言、省略記号の項目、記号の種類が変わるリスト、引用・コード内は判定しない。自動修正は付けない。ラベル付きサンプル fire 10/10・silent 0/10（Wilson上限0.278）で事前登録条件を満たした（eval/calibration.md）

互換性: JSONの形式と既存ルールの結果は不変。`--experimental`を使う場合に新しいカテゴリが返ることがあり、`disabled_rules = ["declared_item_count_mismatch"]`で無効にできる。Agent Skillの`revision-guide.md`に直し方を追加した。旧版のbaselineはバージョンの照合で拒否されるため、v0.3.9で作り直す必要がある。

## [0.3.8] - 2026-09-17

### 追加

- 読解負荷レーン（`--reading-load`）に`long_attributive_span`を追加した。述語を2つ以上含む30字以上の連体修飾節が一つの実質名詞に係り、その名詞句が主節の項になる文を`info`で指さす。形式名詞や「〜する必要がある」の枕の名詞、「〜という関係である」の述語名詞は対象外にする。読点は引用の終止形の並列なら越え、連用中止は両側の節に述語が2つ以上あり手前の節に「が」の主語がないときに一度だけ越える。削除済みの`nested_attributive`（連体形の個数）とは異なり、一つの名詞が背負う修飾節の長さと述語数を測る。ラベル付きサンプル fire 10/10・silent 0/10（Wilson上限0.278）で事前登録条件を満たし、`suiko-eval sweep --rule long-attributive-span`で閾値を校正できる（eval/calibration.md）

互換性: JSONの形式、既存の設定、Rust公開型`Finding`のフィールドは不変。`--reading-load`を使う場合に`reading_load`へ新しいカテゴリが返ることがあり、`disabled_rules = ["long_attributive_span"]`で無効にできる。`findings`、`--fail-on`判定、`--baseline`比較の内容には影響しないが、旧版のbaselineはバージョンの照合で拒否されるため、v0.3.8で作り直す必要がある。採用の経緯と実測は[校正記録](https://github.com/nwiizo/suiko/blob/v0.3.8/eval/calibration.md)に記録する。

## [0.3.7] - 2026-09-14

### 追加

- `.suiko.toml`の`word_rules`で、表層・基本形・品詞の形態素列をプロジェクト独自のルールとして登録できる。活用形を含む一致に`custom_wording/<id>`、指定したメッセージ・重要度・原文範囲を返す。前回比較、個別許可、無効化、終了コード、CI出力に対応する
- `--experimental`に`short_topic_comma`を追加した。文頭の5文字以内の名詞句＋係助詞「は」に続く読点を、位置付きの`info`として返す。長い主題、動詞を含む節、引用行・コードなどは除外し、自動修正は付けない

互換性: 既存の設定、通常ルールのJSON、Rust公開型`Finding`のフィールドは不変。独自ルールのIDは`category`に含め、個別許可では`allow.rule_id`で指定する。旧版のbaselineはバージョンの照合で拒否されるため、v0.3.7で作り直す必要がある。一般語を一律に禁止する辞書は追加しない。採用理由と評価の範囲は[比較調査](https://github.com/nwiizo/suiko/blob/v0.3.7/eval/word-rules.md)に記録する。

## [0.3.6] - 2026-09-13

### 追加

- `--genre tech --experimental`の`technical_jargon_metaphor`へ、気づきにくい失敗、効果、時間の消費、判断の方向を表す限定した言い回しを追加した。形態素の基本形と直近の技術対象を確認し、別の主語や本来の用法を除外する
- 同モードの`abstract_metaphor`へ抽象的な`入口`・`主役`を追加した。説明済みの内容にも一致するため、比喩の追加分は実験機能として扱う
- `--genre tech`の通常検出に`repeated_distinction`と`repeated_em_dash`を追加した。同じ節の5文以内に3回ある文末・ダッシュの反復を`info`でまとめ、近接した対象行だけを返す。太字も扱い、自動修正は付けず、baselineで文書単位の継続を追跡する
- [検出仕様と形態素解析の手順](eval/technical-wording.md)に形態素の実測、利用場面に基づく採否、外部文書での点検、検証用サンプルと限界を記録した

### 更新

- quick-xmlを0.42.0、ureqを3.4.1、zipを8.6.0へ更新し、XMLの文字列API、HTTPの設定・ヘッダー・本文読み取りAPIへ対応した。辞書のハッシュ検証、HTTPエラーの扱い、タイムアウト、外部文書の文字コード変換を維持する
- encoding_rsを0.8.41、jiffを0.2.37、tomlを1.1.6へ更新し、3つのworkflowでactions/checkoutをv7へ更新した

互換性: JSONの形式は不変。`--genre tech`で新しい2カテゴリが返る場合があり、`--fail-on info`の終了コードとbaselineの件数に影響する。旧版のbaselineはバージョンの照合で拒否されるため、v0.3.6で作り直す必要がある。

## [0.3.5] - 2026-09-08

### 追加

- `--genre tech --experimental`に`repeated_explanation_preview`を追加した。文書を指す語と説明・紹介・解説の現在形を形態素列で確認し、同じ節の段落頭で3回以上繰り返す場合に`info`を返す。過去・否定・可能・義務や別主語の文は除外し、同じ文頭の品詞4-gramの指摘は重複させない
- 既存`terms`と互換な独立`lexical-audit`レーンを追加した。固定参照資源とSudachi形態素から、禁止語一致、表記揺れ、明示したレジスター集合の共起、低頻度で未定義・未登録の一般名詞複合語を根拠別に報告する
- `academic`コマンドを追加した。執筆者が作る監査契約と照合し、中心命題、説明対象、論証順序、用語の出典又は造語表示、段落第一文、節間接続、引用―参考文献、注の分類、著者文体プロファイルを確認する
- DOCX/PDF成果物に対し、Markdownの本文・表・注・参考文献との双方向同期、公式テンプレートの本文レイアウト不変条件、書誌の双方向照合、三成果物のSHA-256を確認する。Word書き出しとPDF全頁目視は自己申告の納品記録として扱い、原稿監査の`passed`と提出可能状態の`delivery_ready`を分ける
- Agent Skillへ学術稿の論証・提出工程を追加した。PDF全頁の画像目視は、機械検証とは別の必須工程として残す
- `--genre tech --experimental`に、テストやCIの成功状態を色で表す言い回しと、ソフトウェアの公開を物流語で表す言い回しを示す`technical_jargon_metaphor`を追加した。表示色と物理配送を示す語が同じ文にある場合は除外し、修正候補を付けない`info`として返す
- `--genre tech --experimental`の`abstract_metaphor`に、抽象的な主語・目的語・移動先を`運ぶ`でつなぐ形と、数量名詞の直後を`で効く`とする形を追加した。物理的な運搬、薬や機能の効能、別の節にある主語は対象外とし、自動修正候補は付けない

互換性: 既存の`lint`、`outline`、`terms`のJSON形式は不変。新しい2コマンドは独立したJSONを返す。`--genre tech --experimental`では`technical_jargon_metaphor`、`abstract_metaphor`、`repeated_explanation_preview`が新たに出力される場合がある。旧版のbaselineはv0.3.5で作り直す必要がある。

### 修正

- 学術監査で括弧内の著者・年引用を照合し、語彙監査ではコメント・コード・参考文献等の本文外を共起判定から除外する。用語の2回目以降にある定義も反映する
- 一般語「残る」は既定の禁止語へ追加せず、文脈に応じた目視確認の手引きとして扱う
- `antithesis_repetition`で「ではなくなる」の活用形を状態変化として除外する。同じ行に続く本来の対比は引き続き集計する（#13）
- `double_negative`で「聞こえず話せない」の別述語と、「根拠のない情報や信頼できない情報」「思わぬ落とし穴に遭わず」の別の対象に掛かる否定を除外する。「ないわけではない」「なくはない」「ずにはいられない」は候補に残す（#9）

### 整理

- `similarity-rs`で重複を確認し、学術監査の見出し正規化とMarkdown抽出、形態素ベースの検出結果に抜粋と位置情報を付ける処理を共通化した
- Agent Skillへ、エッセイの意図した反復や記憶の限界を推敲で保つ判断例を追加した
- Renovateを導入し、上流版を再配布する`crates/suiko-sudachi`は依存更新の対象外にした

## [0.3.4] - 2026-08-31

### 追加

- `--experimental`に、評価語を含む「〜のは」型の反復を示す`self_labeling_repetition`、否定2文から短い肯定文へ焦点を移す並びを示す`negative_listing`、essayで形態素上の揃った箇条書きを示す`uniform_bullet_structure`を追加した。いずれも文章の良否を決めず、読み直す箇所を`info`で列挙する
- `--genre tech --experimental`に、複数の動作後にある`このこと`、`そのこと`、`あのこと`を示す`demonstrative_reference`と、前方だけに列挙がある`それぞれ`を示す`respectively_scope`を追加した。解釈は決めずに形態素列の候補だけを列挙する
- `translationese_morph`に、`意味+を+持つ`、`疑問節末のか+を+持つ`、`持てる+未決`の3つの形態素列を追加した。自然な「傘を持つ」「停止権限を持つ」「疑問を持つこと」は対象外とし、修正の要否はAIまたは人が文脈から判断する

### 修正

- `english_syntax_inanimate_subject`と`inanimate_subject_morph`が同じ行の同一または包含範囲を示す場合、形態素側だけを利用者向けfindingとして残すようにした
- `double_negative`で、最初の否定が直後の名詞を修飾し、その名詞に`は`、`が`、`を`、`も`が続く場合は、後続述語の否定と別の対象に掛かるものとして除外した
- 「参考文献」「引用文献」「References」「Bibliography」のMarkdown見出し以下を、同じ階層以上の次の見出しまで本文外としてマスクするようにした。番号のない書誌行が読解負荷のfindingになる問題を防ぐ

### 変更

- 外部評価文書の取得処理をPythonから`suiko-eval fetch`へ移し、HTML/PDFの抽出、SHA-256の記録、取得日時の保存をRustだけで実行できるようにした。途中の書き込みに失敗しても、完了分と失敗内容をlockへ保存する
- SudachiDictの取得と検証を`build.rs`へ一本化し、重複していた辞書取得用シェルスクリプトとCIの事前取得手順を削除した
- sudachi.rsとSudachiDictの更新確認からPythonを除き、`gh`、または`curl`と`jq`で確認するようにした
- `cargo coupling`と`similarity-rs`で全Rustコードを解析し、見出し解析、出力形式ごとのfinding走査、形態素トークン走査、評価ファイルの読み込み処理にあった重複を整理した
- 抽象的な「持つ」は現代日本語で広く使われるため、`を持つ(こと|存在)`という広い文字列規則を削除した。対象を上記3形態素列へ限定し、手引きも一律な翻訳調判定ではなく読み直し候補の説明へ改めた

互換性: 公開JSONの形は不変。新しい5カテゴリは`--experimental`指定時だけ出力する。`translationese_morph`の候補追加、重複findingの抑制、否定の係り先判定、参考文献のマスクによって既存の件数とbaseline比較結果が変わる場合がある。旧版のbaselineは版の照合で拒否されるため、v0.3.4で作り直す必要がある。評価用の外部文書取得コマンドは`cargo run --features evaluation --bin suiko-eval -- fetch eval/sources.toml`へ変わる。

## [0.3.3] - 2026-08-25

### 追加

- 抽象的な対象を「地図」「羅針盤」などの名詞で説明している箇所を、具体的な判断対象・判断基準・効果へ書き換える候補として示す `abstract_metaphor` を追加した。形態素と周辺文脈を使い、字義どおりの用例を対象外にする。severityは`info`で、ラベル付き14サンプルでは検出対象5/5、除外対象0/9だった
互換性: 公開JSONの形は不変。通常実行の終了コードも変わらない。`--fail-on info`を指定した場合は、新しい`abstract_metaphor`によって終了コードが変わることがある。旧版のbaselineはSuikoのバージョン照合で拒否されるため、v0.3.3で作り直す必要がある。

## [0.3.2] - 2026-08-21

### 追加

- `stats.rhythm.sentence_endings` に、明示的な断定、推量・保留、疑問、体言止め、その他の件数と、空行をまたがない最長連続数を追加した
- [日本語技術文書の文章規範](https://gist.github.com/k16shikano/fd287c3133457c4fd8f5601d34aa817d) と [認知リズムを生むための日本語ライティング規範](https://gist.github.com/k16shikano/eb2929f13ed19c97188393d297be8432) を参考に、実験的検出器 `repeated_sentence_mode` と `consecutive_nominal_endings` を追加した。6文以上の文書を対象とし、前者は30モーラ以上で同じ明示的文末が3文以上続き、文長CVが0.15以下の場合、後者は25モーラ以下の体言止めが3文以上続く場合に、連続箇所を1件へ集約する

互換性: 公開JSONは `stats.rhythm.sentence_endings` の追加のみ。既存フィールドは不変。新しいfindingは `--experimental` を指定した場合だけ出力する。

## [0.3.1] - 2026-08-20

### 修正

- `nominal_ending` を文書単位findingとしてbaseline比較するようにした。体言止めの状態を変えずに文数や文字数が変わっても `persisting` になり、体言止めが加わってfinding自体が消えた場合だけ `resolved` になる

### 配布

- crates.ioで欠落していたv0.3系をv0.3.1として公開し、`cargo install suiko` とGitHub Releasesから同じ最新版を導入できるようにした

互換性: 出力JSONの形は不変。`nominal_ending` の状態が変わらない文書では、baseline比較の分類だけが従来の `resolved` + `new` から `persisting` へ変わる。

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
- コーパス取得基盤: `eval/sources.toml`（人間93ソースのmanifest、coji/natural-japanese@0f1cc1cのsources.jsonをMIT出典明記で初期値化、unit単位のdev/holdout割当）、外部文書取得処理（本文非コミットで取得し`external-lock.json`へSHA-256を記録。81/81件成功）、`scripts/generate-ai-corpus.sh`（未修正AI文書の生成と出典記録）。青空文庫の随筆12件を評価コーパスへ追加し、holdout splitを初めて充足した
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
- Agent Skill導入の検証（`scripts/verify-skill-install.sh`と構造テスト）、`build.rs`による辞書取得とSHA-256検証

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
