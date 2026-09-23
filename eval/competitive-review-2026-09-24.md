# Natural Japaneseとの実行比較（2026-09-24）

同じ文書と同じ設定で両ツールを実行し、差を行単位で読んだ。前回の[競合調査](competitive-review-2026-09-06.md)は公開資料と差分の確認で、実行比較はしていない。今回の比較で見つかった差のうち、Suiko側の誤りは`no_chain`の1件で、v0.3.10で直した。Natural Japanese側にだけある検出器はなかった。

## 条件

- Natural Japanese: `coji/natural-japanese@9a78a42`（2026-09-04、2026-09-24時点の最新）。`skills/natural-japanese/scripts/lint.py`を`uv run`で実行した（SudachiPy、sudachidict-core）。
- Suiko: v0.3.10の作業ツリー（`--no-config`）と、公開済みv0.3.9のaarch64-apple-darwinバイナリ（SHA-256確認済み）。
- 文書: `eval/corpus.toml`の18文書（人間14・AI 4）と、取得済みの外部文書81件（`eval/sources.toml`、人間）。合計99文書、28,439文。各文書のジャンルを`--genre`に渡し、`--reading-load`を付けた。実験機能は付けない。
- ラベル付きサンプル: `eval/corpus.toml`の177件。こちらは両者とも`--experimental --reading-load`で実行した。

## 所要時間

99文書の逐次実行で、Suikoは14.9秒、Natural Japaneseは14.7秒だった（Apple Silicon、ウォームキャッシュ、1回の計測）。この規模では速度差を判断材料にしない。

## 既定の出力の差

文書単位の発火（人間95文書・AI 4文書）と件数。

| カテゴリ | Suiko 人間/AI文書・件数 | Natural Japanese 人間/AI文書・件数 |
|---|---|---|
| `repeated_sentence_lead` | 実験機能のため0 | 55/1・9,979件 |
| `low_lexical_diversity_ttr` | 実験機能のため0 | 58/1・59件 |
| `low_lexical_diversity_mtld` | 実験機能のため0 | 8/0・8件 |
| `antithesis_repetition` | 20/2・22件 | 22/2・115件 |
| `double_negative` | 35/1・126件 | 40/1・148件 |
| `translationese_morph` | 50/3・158件 | 52/3・174件 |
| `buried_list` | 49/1・236件 | 47/1・200件 |
| `no_chain` | v0.3.9: 50/0・161件、v0.3.10: 47/0・142件 | 47/0・142件 |
| `sentence_too_long` | 86/1・1,218件 | 86/1・1,223件 |

`forbidden_phrase`、`low_burstiness`、`kanji_run`、`inanimate_subject_morph`、`nominal_ending`は文書数・件数とも一致した。`redundant_light_verb`、`hype_expression`、`long_attributive_span`、`no_comma_sentence`、`buried_question_list`などはSuikoだけにある。

Natural Japaneseが既定で出す文頭反復と文書単位TTRは、人間文書の過半で発火する。Suikoは2026-08-19の実測でこの2件を実験機能へ下げており（`calibration.md`）、今回の結果もその判断と矛盾しない。`antithesis_repetition`の件数差は、Suikoが文書単位で1件に集約するためで、発火した文書数はほぼ同じである。

## 行単位で読んだ差

共通カテゴリの指摘を、文書・カテゴリ・行で突き合わせた。

- Natural Japaneseだけの`double_negative` 17件: 「負けず嫌い」「知らず知らず」「当たらず障らず」のような慣用句、「〜ない訳に行かない」の義務表現、「必要のない〜はない」型の名詞修飾が中心。Suikoが意図して除外している型で、Suikoの見落としではない。
- Natural Japaneseだけの`translationese_morph` 15件: 否定の「〜することはできない」が中心。肯定の「〜することができる」とは違い、日本語として自然な言い方として扱う。
- Suikoだけの`buried_list` 33件: 白書の「」付き項目を読点で並べた長い文が中心。判定手順は両者で同じで、差は文の区切り方から来る。指摘の説明は正確だった。
- Suikoだけの`no_chain` 17件: 「〜するのは世相の上皮だけの」「好みの問題なの」のように、準体助詞の「の」まで数えていた。メッセージとカタログは「格助詞の『の』」と書いており、実装の誤りだった。v0.3.10で格助詞に限り、99文書すべてで件数がNatural Japaneseと一致した。

## ラベル付きサンプル

Suikoのサンプルは、Suikoの検出器の境界を固定するために作ったものである。Natural Japaneseに対しては偏った比較になるため、差の方向だけを見る。

- 共通カテゴリでNatural Japaneseだけが誤発火したsilentサンプル: `repeated_sentence_lead`の名詞主題（今回追加したsilent-002）と、`translationese_morph`の2件（silent-002・003）。
- Suikoだけにあるカテゴリ（`long_attributive_span`、`declared_item_count_mismatch`、`buried_question_list`、`technical_jargon_metaphor`、`redundant_light_verb`など）は、Natural Japaneseでは検出0になる。

## Skillの参照文書

Natural Japaneseの参照文書のうち、Suikoにないのは`eval-rubric.md`（6軸の点数評価）だけで、前回の調査で点数化しないと判断済みである。今回の学びはSkillの`revision-guide.md`へ入れた。`buried_list`を箇条書きに開くときに「**項目**: 説明」の定型を作らないという注意は、Natural Japaneseの`buried_list`のメッセージにあり、Suikoでは`bullet_bold_label`との関係として既存カタログにだけ書かれていた。

## 採用した変更と採用しないもの

採用（v0.3.10）:

- `no_chain`を格助詞の「の」に限る（上記の実装誤り）。
- `revision-guide.md`に`buried_list`・`buried_question_list`・`no_chain`の行を足し、箇条書き化で太字ラベルを作らない注意を移した。

採用しない:

- 文頭反復と文書単位TTRを既定に戻すこと。人間文書の過半で発火する状態は変わっていない。
- 慣用句の二重否定と「〜することはできない」を指摘へ戻すこと。行単位で読んだ範囲では直す価値のある例がなかった。

## 限界

人手の正解ラベルは、行単位で読んだ差の分類だけで、実文書全体の見落とし率は測っていない。外部文書は取得時点の本文で、`external-lock.json`のSHA-256に対応する。AI文書は4件で、検出率の根拠にはならない。
