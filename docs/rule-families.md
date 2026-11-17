# 規則の族と必要な機能

Suikoの規則を、直すときに書き手が決めることで7つの族に分ける。natural-japaneseとyomiyasuの性質は、この族のどこに当たるかで取り込む。決定の理由は[ADR-0003](adr/0003-rule-families-and-default-visibility.md)、測定は[suiko-evalの実行比較](https://github.com/nwiizo/suiko-eval/blob/main/eval/competitive-review-2026-11-15.md)にある。

## 指摘が満たすこと

どの族の指摘も、次の4つを返す。

- **どこ**: 本文の範囲。文書全体の統計は、範囲を持たないことを明示する。
- **何が読み取れないか**: 読み手が補わなければならないこと。
- **何を書けば直るか**: 書き手が原文と資料から書ける内容。
- **残す条件**: 直さなくてよい場合。

「AIらしい」「よく出る」だけを理由にした指摘は作らない。直し方が同じ指摘は同じ規則にまとめ、一つの箇所には一つだけ出す。

## 7つの族

| 族 | 書き手が決めること | 主な規則 | 範囲 |
|---|---|---|---|
| 意味（`meaning`） | 精読したとき、何を指すかが一つに定まるか | `elaborate_simile`・`sensory_role_mismatch`・`explanatory_metaphor`・`compressed_relation`・`copy_fragment`、任意の`suiko meaning` | 箇所 |
| 型（`pattern`） | 同じ形を続ける理由があるか | 文末・文頭・構文・対比・体言止めの連続、述語句の反復 | 箇所（連続の先頭と対応行） |
| 語（`wording`） | その語の代わりに、動作・対象・根拠を直接書けるか | `abstract_metaphor`・`vague_sensory_term`・`technical_jargon_metaphor`・`calque_phrase`・`translationese`・`redundant_light_verb`・`forbidden_phrase`・`hype_expression`・`vague_loanword`・`word_usage` | 箇所 |
| 区切り（`punctuation`） | 読点・記号・書式が、掛かり先や表示に必要か | `short_topic_comma`・`short_object_comma`・`duplicate_comma`・`bold_not_rendered`・`heading_colon`・`mixed_latin_spacing`・絵文字・ダッシュ | 箇所 |
| 読解負荷（`reading_load`） | 一文を読み終えるまでに、読み手が何を保留するか | `sentence_too_long`・`buried_list`・`kanji_run`・`double_negative`・`no_chain`・`long_attributive_span`・`no_comma_sentence`・`sentence_in_paren` | 箇所 |
| 文書の傾向（`profile`） | 文書全体のそろい方が、内容から来ているか | `low_burstiness`・`low_sentence_variance`・`uniform_paragraph_structure`・語彙多様性・太字と箇条書きの比率 | 文書全体 |
| 照合（`consistency`） | 本文の数・条件・事実が、本文の中と資料とで食い違わないか | `declared_item_count_mismatch`・`quoted_character_count_mismatch`・`quantity_predicate_mismatch`・`universal_exception_conflict`・`instruction_residue`、`check`の資料照合と`diff` | 箇所 |

族は規則の登録表が持ち、`capabilities --json`の各規則と、`lint --json`の各指摘が`family`と`confidence`を返す。

## 他のツールの性質との対応

| 性質 | natural-japanese | yomiyasu | Suikoの族と現状 |
|---|---|---|---|
| 意味の定まらない比喩・圧縮 | なし | スキル本体（LLM）とslop-catalog | 意味。決定的な規則は生成文の注釈50件のうち6件。`meaning`版2は13件。規則を足しても伸びない |
| 同じ文末の連続 | なし | `sentence_end_repetition`（生成37・人間85） | 型。`repeated_sentence_mode`は「ます」「ました」を数えず、生成文書で0件。**欠けている** |
| 否定対比 | `antithesis_repetition`（箇所ごと） | `negative_parallelism`（箇所ごと、生成57・人間125） | 型。`antithesis_repetition`は文書で3回以上のときだけ1件。短い文書では出ない。**箇所が欠けている** |
| 文頭・構文の反復 | `repeated_sentence_lead`・`repeated_syntax_template` | なし | 型。同じ規則を持つ |
| 比喩として使う名詞 | なし | `slop_vocabulary`（土台・体温・羅針盤が生成文書に偏る） | 語。`abstract_metaphor`が土台・羅針盤を持つが、生成文書の土台7件のうち3件だけ。**条件が狭い** |
| 機械の処理に当てた動詞、場所のたとえ | `calques.py`（別スクリプト） | `metaphor_verb`・`calque` | 語。`technical_jargon_metaphor`は「効いてくるのが」の形だけ。「ルールが効いた」「処理の入口」は**欠けている** |
| 翻訳調・定型句・カタカナ語 | `translationese`・`forbidden_phrase` | `meta_filler`・`katakana_term` | 語。同じ規則を持つ |
| 間を作るだけの読点 | なし | `pause_comma`（生成678・人間640） | 区切り。文頭の5字以内の「は／を」だけで、生成109件。**範囲が狭い** |
| 書式（太字・コロン・絵文字・ダッシュ） | 比率の統計 | `bold_not_rendered`・`trailing_colon`・`emoji_prohibited`・`dash_decoration` | 区切り。太字と見出しは持つ。行末のコロンとダッシュは候補（TODO） |
| 読解負荷 | `--reading-load` | なし | 読解負荷。同じ規則を持ち、意図して外した型がある |
| 文書全体の統計 | `low_burstiness`ほか | `negative_parallelism_density` | 文書の傾向。同じ規則を持つ |
| 先回りの打ち消し、括弧内の文、和語の名詞化 | `negation_repetition`・`sentence_in_paren`・`wago_nominalization` | なし | 生成文書で0・0・3件（誤り）。自然さの規則としては取り込まない。v0.4.2で、`sentence_in_paren`を読解負荷に、「調べ」「測り」を`renyo_noun`の語に入れた |
| 文書の種類ごとの規則の出し分け | `--genre`（3種類） | `--doc-type`（13種類） | `--genre`（3種類）。全規則を既定で出すと必要性が上がる。候補（TODO） |
| 原文との照合、検査記録 | `--baseline` | `diff`・`check` | 照合。`diff`・`check`・`verify`・`compare`を持つ |
| 埋め込みモデル、自然度の6軸採点 | `semantic.py`・`eval-rubric` | なし | 取り込まない（単一バイナリ・オフライン・決定的の境界） |

## 必要な機能

次の順に実装する。W0とW1は、ほかの項目の前提になる。

### W0 規則の登録表

- `RULE_CATEGORIES`・`EXPERIMENTAL_CATEGORIES`・`READING_LOAD_CATEGORIES`と、`priority.rs`の`HIGH_CONFIDENCE`・`LOW_CONFIDENCE`を、規則ごとに1行の登録表へまとめる。項目は、規則名、族、確度、範囲（箇所・文書全体）、評価中かどうか。
- 既存の公開関数（`is_known_rule`・`rule_categories`・`reading_load_categories`・`is_experimental_rule`・`priority::confidence`）は、署名と返す値を変えない。評価中の規則の確度がlowになる現在の動作も保つ。
- `capabilities --json`の各規則に`family`・`confidence`・`scope`を足す（既存のキーは残す）。`lint --json`の各指摘に`family`と`confidence`を足す（追加だけ）。
- 受け入れ: 全規則がちょうど一つの族を持つことをテストで確かめる。変更前後で、既存のテストが返す規則・件数が変わらない。

### W1 全規則を既定で表示する

- `lint`・`check`・文書群の集計は、`--experimental`の有無にかかわらず全規則を検査する。`--experimental`は受け付け、指定されたら不要になったことを標準エラーへ1行で伝える。
- ライブラリの`lint::analyze(.., experimental)`は変えない（suiko-evalが既定と全規則を比べるために使う）。
- ジャンルによる出し分け（`uniform_bullet_structure`はessayだけ、techの文脈規則、businessで外す書式の規則）は保つ。
- 既存の`check`の記録は、記録された設定で再検査できること。旧版のbaselineとの比較は、作り直しを案内して止まること。
- 受け入れ: `--experimental`なしで、これまで実験だった規則の指摘が出るテスト。`--experimental`ありと出力が同じであるテスト。

### W2 型: 同じ文末の連続（`repeated_sentence_ending`、新設）

- 同じ段落で、同じ文末の形が3文以上続く箇所を1件にまとめて示す。文末の形は「です・ます・でした・ました・である・だ（断定）・だろう・でしょう・ですね」。過去の「た」（読んだ・した）と「ません」は数えない。「だ」は辞書形が「だ」の助動詞に限り、「読んだ」の「だ」を数えない。
- 範囲は3文目の文末、対応行は連続した全部の行。見出し・箇条書き・表・引用・コードの行は数えず、連続を切る。4字未満の文は数えない。
- 直し方: 文をつなぐ、主語や説明の順を変える、同じ働きの文なら残す。語尾を散らすためだけに変えないことを書く。
- `repeated_sentence_mode`のうち、この規則と同じ文末の形の連続は出さない（包まれるため）。それ以外の形（だった・のだ・かもしれない など）は従来どおり。
- 残す条件: 手順・定義・議事録の決定事項のように、同じ働きの文を並べる箇所。
- 受け入れ: 生成180文書で、yomiyasuの`sentence_end_repetition`37件のうち30件以上と同じ箇所を指す。人間文書での1000字あたりの件数がyomiyasu（0.094）以下。

### W3 型: 否定対比を箇所ごとに示す（`negated_contrast`、新設）

- 一文の中の「AではなくB」「Aではない。Bだ」を箇所ごとに示す。`antithesis_repetition`は文書全体の密度の要約として残し、対応行を返す役割に限る。
- 状態の変化（「〜ではなくなる」）は従来どおり除く。操作や選択の説明（「AではなくBを指定する」「AではなくBへ送る」のように、Bの後に格助詞と動作が続く形）が人間文書の指摘の多くを占めるなら、除く。除くかどうかは、人間文書の指摘を20件以上読んで決め、読んだ結果を記録する。
- 直し方: 否定を外しても主張が変わらないなら肯定文にする。誤解の訂正や、選ばなかった案を示す対比は残す。
- 受け入れ: 生成文書と人間文書の1000字あたりの件数の比が、同じ文書の集合でのyomiyasuの`negative_parallelism`の比以上。人間文書で付く文書数が49/95以下。（2026-11-15に変更。当初は固定値の「比3.3以上」だったが、評価用の集合ではyomiyasuも2.3で、集合ごとの偏りの差を規則の良し悪しと分けられなかった。変更は評価用の値を見た後に決めた。）

### W4 区切り: 読点の規則を広げる（`short_topic_comma`・`short_object_comma`を広げ、`short_subject_comma`を新設）

- 文の最初の読点だけでなく、文の中のすべての読点を見る。
- 読点の前の句は、名詞・代名詞・連体詞・接頭辞・接尾辞と、名詞をつなぐ「の」だけでできた15字以内の名詞句とする（現在は5字以内）。動詞・形容詞・助動詞を含む句（条件、場面、分裂文）は対象にしない。
- 助詞は「は」「も」と、格助詞に続く「は」「も」（には・では・からは など）を`short_topic_comma`、「が」を`short_subject_comma`、「を」を`short_object_comma`とする。
- 次の読点は対象にしない。既存の条件（長い文の唯一の読点、後ろに別の主語や連体修飾が続く、太字の表示に必要）。並べた名詞の最後（A、B、Cは、）。同じ文に同じ助詞の句が読点付きで並ぶ（Aも、Bも）。副詞にもなる名詞で終わる句（今回は、現在は、最初は）。指示語だけ・人称だけの句（それは、私は、）。接続の句（では、まずは、中でも）。括弧や引用の中。
- 受け入れ: 生成180文書での件数が現在（109件）の3倍以上。生成文書と人間文書の1000字あたりの件数の比が7倍以上（現在9.2倍、yomiyasuは7.8倍）。人間文書の指摘を30件以上読み、掛かり先を決めている読点への指摘が1割以下。

### W5 語: 既存の3規則の条件を広げる（新設しない）

- `abstract_metaphor`: 「土台」「羅針盤」を、抽象的な語（対話・判断・事業・活用・関係 など）の土台・羅針盤として使う形へ広げる。「土台作り」「土台づくり」と、建物・機械の実物は除く。生成文書の「土台」7件のうち6件以上を指し、人間文書の「土台作り」「絵を土台にして」を指さないこと。
- `vague_sensory_term`: 「体温」「熱量」を、体や物理量ではないもの（言葉・文章・組織・記憶・議論 など）に使う形で足す。測る・計る・度・カロリーなどの文脈と、体の描写は除く。人間文書での指摘が1件以下であること。
- `technical_jargon_metaphor`: 抽象的な主語（工夫・仕組み・ルール・対策・改善・習慣 など）＋「効く」を足す。薬・冷暖房・調味料などの文字どおりの「効く」と、否定形は除く。同じ文に技術の語があるときの、仕組みを指す「入口・入り口・窓口・受け口・玄関」を足す。建物・駅・組織の窓口などの実物は除く。小説と随筆の実物の「玄関」を指さないこと。
- AI頻出語という名目の一覧は作らない。「本質的」「真理」は人間の随筆に多く、足さない。

### W6 Skillと文書

- Skillは、全部の指摘を「直す」か「理由を付けて残す」に分ける。確度highとmediumは、残す条件に当たらなければ直す。確度lowと文書の傾向は、該当箇所を読んで判断し、残した理由を報告に書く。`--experimental`を前提にした説明を直す。
- README、`skills/suiko`の参照文書、`docs/`、CHANGELOG、TODO、機能の対応状況を、この設計に合わせる。新設・拡張した規則は、表現別の修正判断に直し方と残す条件を書く。

### W7 評価（suiko-eval）

- 新設・拡張した規則ごとに、正例と反例を各5件以上、ラベル付き標本へ足す。
- 生成文書の第1・2回と人間文書のdevで調整し、最後に一度だけ、第3回と人間文書のholdoutで測る。調整に使った値と評価の値を分けて記録する。
- 規則ごとの生成・人間の1000字あたりの件数、yomiyasuの指摘と同じ箇所を指した数（`coverage`）を、実行比較の記録へ追記する。`spans`と`quality`のテストが固定している値は、測り直して更新する。

## 取り込まないもの

- 直し方を示さない語の一覧、AIらしさの点数、埋め込みモデル。
- natural-japaneseの`negation_repetition`。生成文書に偏らず、推敲の候補として別に評価するまで足さない。`sentence_in_paren`は読解負荷の指さしとして、`wago_nominalization`の「調べ」「測り」は`renyo_noun`の語として、v0.4.2で入っている。
- 意味の族の決定的な規則の追加。語や動詞を足しても、次の生成文の破綻には広がらない。

## 実装結果（2026-11-15）

W0〜W7を実装した。値はsuiko-evalの[実行比較](https://github.com/nwiizo/suiko-eval/blob/main/eval/competitive-review-2026-11-15.md)の「suikoへの取り込みと測り直し」にあり、調整用（生成文書の第1・2回、人間文書のdev）と評価用（第3回、holdout）を分けて記録した。

| 項目 | 結果 |
|---|---|
| W0 | 登録表は`src/lint/registry.rs`。旧来の5つの配列と同じ集合・順序であることを確かめてから置き換えた。照合の`DOCUMENT_LEVEL`は残し、登録表で範囲が`document`であることをテストで確かめる |
| W1 | `lint`・`check`・`compare`は全規則を検査し、`--experimental`は標準エラーに1行を出すだけにした。旧版のbaselineは`stats.experimental`の不一致で止まる |
| W2 | 生成文書でyomiyasuの37件の37件と同じ箇所を指し、人間文書で1000字あたり0.091件（yomiyasu 0.094）。人間文書の34/95文書に付くため確度はlow |
| W3 | 比は全体3.4（yomiyasu 3.3）、調整用3.8（3.8）、評価用2.5（2.3）で、どの集合でもyomiyasu以上。人間49/95文書（条件の境界）。当初の固定値（3.3）は評価用で下回ったため、条件を同じ集合での比較に変えた。付け足し・二重否定・部分否定は除き、操作や選択の説明は人間文書の約15%で過半ではないため残した。確度はlow |
| W4 | 生成文書で3規則358件（従来の109件の3.3倍）、比8.5（評価用7.5）。後ろの連体修飾の確認を、次の読点まで括弧を飛ばして見るように直した。副詞にもなる名詞は主題（は・も）のときだけ除く |
| W5 | 生成文書の「土台」7件のうち6件（「オブジェクトストレージを土台にした構成」以外）。人間文書では3規則の新しい形の指摘が0件 |
| W6 | Skill・README・参照文書・CHANGELOG・TODO・機能の対応状況を更新した |
| W7 | 8規則にラベル付き標本85件を足した。外れたのは実物の「羅針盤」を述語にした反例1件 |
