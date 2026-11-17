# Suiko（推敲）

[![crates.io](https://img.shields.io/crates/v/suiko.svg)](https://crates.io/crates/suiko)
[![CI](https://github.com/nwiizo/suiko/actions/workflows/ci.yml/badge.svg)](https://github.com/nwiizo/suiko/actions/workflows/ci.yml)
[![license: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/nwiizo/suiko/blob/main/LICENSE)

日本語文書を決定的に診断し、自然で明晰な推敲を支援するRust CLIです。公開・共有する原稿の翻訳調、近接した反復、単調なリズム、読解負荷を手掛かりに、読み直す行と理由を返します。

診断はローカルで実行し、同じ入力・バージョン・設定には同じ結果を返します。形態素辞書はバイナリに埋め込んであり、実行時に辞書やモデルを取得しません。指摘を採用するかは書き手やエージェントが文脈から判断し、原稿を直します。診断コマンドは原稿を書き換えません。書き換えるのは、太字の書式を直す`fix --bold --write`だけです。AIが書いた確率や、文章の品質を表す総合スコアは出しません。

## 目的から選ぶ

| 進めたい作業 | コマンドと得られる手掛かり |
|---|---|
| 公開前に読み直す箇所を絞り、改稿後の変化を確かめたい | `lint`で行・抜粋・理由を確認し、`--baseline`で前回の指摘と比較する |
| 改稿で数値・否定・条件・文末の働きが変わっていないか確かめたい | `diff`で改稿前後を文単位に対応づけ、読み直す箇所と変更前後の本文を確認する |
| 提供資料も照合し、検査した最終稿をそのまま提出したい | `check`で検査をまとめて記録し、提出前に`verify`で本文・資料・設定の変更を確認する |
| 複数の記事を集計し、Gitの版と比べたい | `lint --summary json\|tsv`と`compare --rev HEAD`で自然さ・読解負荷の前後件数を集計する |
| 指定した記事の文体傾向を知りたい | `style-profile`で文頭・文末・語彙の分布と出典を記録する。傾向との差は誤りと判定しない |
| 説明の順序や結論の置き場所を見直したい | `outline`で見出し・段落の先頭文・箇条書きを俯瞰する |
| 読者に必要な用語説明を確かめたい | `terms`で専門用語候補と初出付近の説明の手掛かりを確認する。`--audit`で複数ファイルの表記揺れを集計する |
| チームの語彙・表現方針を原稿に反映したい | `lexical-audit`で参照データと照合し、`lint`の独自ルール・個別許可・CI出力でレビューを共有する |
| 学術稿と提出ファイルの整合を確かめたい | `academic`で、記録した方針と論証・引用・DOCX/PDFを照合する |
| 精読すると意味が定まらない比喩・標語を見つけたい | `meaning`で、指定したLLMのコマンドに確認させる（任意・非決定的） |

まず `suiko lint draft.md` で箇所を確認します。指摘ごとに原文と前後を読み、直すか、理由を付けて残すかを判断してください。書き手が説明したつもりでも、対象や条件が原稿にないことがあります。読み手が自分の経験で補わないと判断できない箇所を直し、事実や意図が保たれているかを通読して確かめます。[公開前の点検から改稿まで](https://github.com/nwiizo/suiko/blob/main/docs/revision-workflow.md)に、実行例と改稿時の判断をまとめています。機能ごとの[対応状況](docs/feature-status.md)と、[文書群の検査・Git比較・編集判断の集計](docs/batch-review.md)もあります。

## インストール

CargoとGitHub Releasesのどちらからも導入できます。導入したCLIで使える機能は`suiko capabilities --json`で確認できます。このコマンドがない古い版では、各サブコマンドの`--help`を見てください。

### Cargo

Rust 1.97以降が必要です。

```sh
cargo install suiko --locked
suiko --version
```

### ビルド済みバイナリ

Rustを入れずに使う場合は、[GitHub Releases](https://github.com/nwiizo/suiko/releases)から取得できます。対応する環境はmacOS（Apple Silicon / Intel）、Linux（x86_64 / aarch64）、Windows（x86_64）です。各アーカイブにSHA-256ファイルが付きます。

```sh
# v0.4.2 / macOS（Apple Silicon）
curl -fLO https://github.com/nwiizo/suiko/releases/download/v0.4.2/suiko-v0.4.2-aarch64-apple-darwin.tar.gz
curl -fLO https://github.com/nwiizo/suiko/releases/download/v0.4.2/suiko-v0.4.2-aarch64-apple-darwin.tar.gz.sha256
shasum -a 256 -c suiko-v0.4.2-aarch64-apple-darwin.tar.gz.sha256
tar xzf suiko-v0.4.2-aarch64-apple-darwin.tar.gz
./suiko-v0.4.2-aarch64-apple-darwin/suiko --version
```

以降の例で`suiko`として実行するには、展開した実行ファイルをPATHの通ったディレクトリへ配置してください。

### ソースからのビルドと辞書

```sh
git clone https://github.com/nwiizo/suiko
cd suiko
cargo install --path . --locked
```

ビルド時にSudachiDict 20260723.1 coreの公式wheel（ZIP形式、約77MB）を取得します。配布ファイルとV1形式の辞書本体のSHA-256を検証してから埋め込むので、Pythonの実行は不要です。辞書が約202MBあるため、バイナリは200MB台になります。検証済みのV1辞書を`resources/system.dic`に置くか、環境変数`SUIKO_SUDACHI_DICT`で指定すれば、ビルド時の辞書取得も省けます。オフラインビルドでは、Rust依存のキャッシュに加えて、この辞書の配置が必要です。v0.3.10以前に使っていたV0辞書は使えません。

形態素解析には[sudachi.rs](https://github.com/WorksApplications/sudachi.rs) v0.7.0を非公式に再配布した[suiko-sudachi](https://crates.io/crates/suiko-sudachi)を使います。再配布に関する説明は[同crateのREADME](https://github.com/nwiizo/suiko/blob/main/crates/suiko-sudachi/README.md)にあります。

## lintで原稿を確認する

```sh
suiko lint draft.md
suiko lint draft.md --genre tech --json
suiko lint docs/*.md --genre tech
suiko lint docs/*.md --genre tech --summary tsv
suiko compare --rev HEAD docs/*.md --genre tech --json
suiko compare --rev HEAD docs/*.md --genre tech --json --findings new

# 標準入力は - で受け取る
printf '重要なのは、結論です。\n' | suiko lint - --json
```

`--genre`には`essay`、`tech`、`business`を指定でき、ジャンル別の閾値を使います。一部のルールはジャンルを明示した場合だけ有効です。各指摘（finding）にはカテゴリ、対象行、抜粋、理由、重要度が付きます。必要な比較や意図した反復は、そのまま残せます。

指摘があると、lintは最初に「まず直す候補」を5件まで示します（JSONでは`first_fixes`）。重大度の高い順に、同じ重大度では規則の確度（high・medium・low）の高い順に並べ、一つの規則からは2件までにします。確度は[suiko-eval](https://github.com/nwiizo/suiko-eval)の品質表で決めています。人間の文書にほとんど付かず、ラベル付き標本の条件を満たした規則はhighです。人間の文書の多くに付く規則、評価中の規則、文書全体の統計はlowです。JSONの各指摘も、規則の族（`family`）と確度（`confidence`）を返します。生成文の評価で注釈と重ならなかった`sensory_role_mismatch`は、条件を満たしていてもmediumにしています。確度は規則の傾向で、個々の指摘が正しいかは本文で確かめてください。

`compare`・`lint --baseline`・`check --before`は、同じ方法で前後の指摘を対応づけます。本文がある`compare`と`check`は、両方に一度だけ現れる行を目印に行を対応づけ、変更した行は文字の近さで組みます。そのうえで、同じ文に同じ抜粋が残った（`same_sentence`）、書き換えた文に同じ規則の指摘が残った（`edited_sentence`）、同じ抜粋が別の位置へ移った（`moved`）、文書全体の統計が残った（`document`）の順に継続と判定します。根拠のない組は継続にせず、解消と新規に分けます。本文のないbaselineは、抜粋の一致と行の近さだけで照合します。`compare --json --findings new|resolved|persisting|all`は、個別の指摘を前後の行・抜粋・根拠付きで返します。

散文の検出では、コード、見出し、箇条書き、引用行、表、参考文献などを除外します。見出しや箇条書きの構造を見るルールだけは、見出しや箇条書きを調べます。front matter、リンクURL、HTMLタグ・コメント、コード注釈行（`#A …`）も本文と区別します。参考文献行とコード注釈行の除外数は`stats.masking`で確認できます。

`--genre tech`では、補助動詞表記・連用形名詞・修正表現・操作の条件・短い目的語等の読点を、見出し・箇条書き・表でも確認します。引用とコードは除外します。`operation_condition`は「ログの採取は、出力先を指定する必要があります」のような操作名と必要条件のつながりを指します。「ログを採取するには…」と開けるかを判断する候補で、自動置換はしません。エッセイにはこの追加検査を適用しません。

他者の文章などは、独立した行の`<!-- suiko:ignore-start -->`と`<!-- suiko:ignore-end -->`でlint・読解負荷の対象から外せます。不正な対応は警告し、コード内の例示コメントは指示として実行しません。はてな記法の書籍・画像・目次・埋め込みも除外し、`stats.masking`に除外量を返します。改稿時のコード・引用保護は除外しません。

技術文書では`word_usage`、見出し先頭の`heading_emoji`も確認できます。主述関係・述語句の反復・段落の文体混在は、`--genre tech`で評価中の候補（確度low）として返します。体験を述べる過去の可能表現は`evaluative_ability_past`へ分け、essayでは抑制します。可能動詞への言い換えは、局所置換の`suggestion`とは別の`review_suggestion`に返します。文脈を読まずに自動適用しないでください。

`bold_not_rendered`は、`**「用語」**の`や`**注意:**この`のように、太字の内側が括弧・記号・空白で、外側が文字に接している箇所を全ジャンルの通常検出として`warn`で返します。CommonMarkの区切り規則（左接合・右接合）に当たらないため、GitHubやmarkdown-it系の表示では`**`がそのまま出ます。`***…***`も同じ規則で見ます。`__…__`は語の途中（`__重要__です`）では強調にならないため、日本語の文字に接する形も返します。括弧を太字の外へ出すか、文字に接する側へ半角空白を置いてください。コードブロックとインラインコードの中は見ません。`disabled_rules`で無効にできます。

`suiko fix draft.md --bold`で太字の修正結果をプレビューし、`--write`で保存できます。語句の言い換えは適用せず、コード・引用・リンク先と曖昧な区切りを保持します。保存の制限と、文長・具体性スコアの計測は[書式修正と計測](docs/batch-review.md#太字の修正と計測)を参照してください。

### 技術文書の近接した反復

次の2カテゴリは、`--genre tech`の通常検出です。近くに続く文末や挿入表現をまとめて読み直すために使います。

| category | 検出する状態 |
|---|---|
| `repeated_distinction` | 同じ節の5文以内に、`別物だ／です／である`で終わる文が3文以上ある。疑問・否定・引用への接続は除く |
| `repeated_em_dash` | 同じ節の5文以内に、文中の`—`・`―`を使う文が3文以上ある。数字同士の範囲や単独の罫線は除く |

各カテゴリを文書内の1件へまとめ、近接した反復の対象行を`related_lines`で返します。章ごとに一度ある説明を合算せず、太字の有無で判定を変えません。重要度は`info`で、自動修正の候補は付きません。`--fail-on info`を指定すると、これらの指摘も終了コードに影響します。

### 標準の推敲候補

```sh
suiko lint draft.md --json
```

表現の修正候補は、オプションやジャンルを指定しなくても出ます。意味と文脈から直すか残すかを判断するための候補で、CLIが原稿を一括置換する機能ではありません。Agent Skillはこの結果と全文を読み、本文まで推敲します。

| category | 検出する状態・対象ジャンル |
|---|---|
| `auxiliary_kana` | 補助動詞の「して下さい」「して頂く」「して見る」「指定出来る」、形式名詞の「する事」「する為」。活用と原文位置を保ったかな表記の候補を付ける。引用・本来の名詞や動作は除く |
| `renyo_noun` | 「結果の確かめ」「案の比べ」等、一語の連用形を「の」でつないだ名詞。定着した複合語は除き、対象と動作を保つ修正を促す |
| `explanatory_metaphor` | 「休日を溶かす」「処理を別のスレッドに逃がす」「関数が事情を知る」等。比喩の含みと動作を確認し、推測で効果や仕組みを足さない |
| `revision_expression` | 「という見立て」の判断者、問題の中身、設定の適用範囲、「設定変更後」等の名詞の連続を読み直す候補 |
| `boilerplate_closing` | 全ジャンルで、本文の最後の文が「いかがでしたでしょうか」「ぜひ参考にしてみてください」等の定型句だけになっている。具体的な対象を伴う依頼や引用は除く |
| `vague_sensory_term` | 全ジャンルで、理解・根拠・関心の程度を「解像度を（一段）上げる／が高い」「腹落ち」「肌感（覚）として／では」「温度感を揃える」等の感覚語で表している。語ごとに集約し、対応行を返す。画面・写真・数値付きの画質や触覚の話題は除く |
| `sensory_role_mismatch` | 全ジャンルで、「雨音が窓を叩く」「温もりを見つめる」「静けさが鳴る」「温度が裾を暖める」「唸っていたその不在」「埃をかぶった光」等、感覚語と述語の組み合わせを読み直す候補。対象語・助詞・近くの述語を照合し、別の主語や節、否定、引用は除く。比喩の誤りとは判定しない |
| `quoted_character_count_mismatch` | 全ジャンルで、「『青空』の四文字」のように、引用した文字列の字数と説明の数字が一致しない。空白を含む数え方と除く数え方の両方を認め、実際の字数を示す |
| `quantity_predicate_mismatch` | 全ジャンルで、「速さを数える」「力を一つ抜く」「疲れを一枚ずつ映す」のように、測る量・形のない状態と、数える個数が混ざる可能性を示す。形態素・助詞・近くの動詞を照合し、別の述語・否定・禁止・引用は除く |
| `universal_exception_conflict` | 全ジャンルで、「扉は全部閉まっている。ただ、右の扉だけが開いていた」のように、全体を言い切った文の直後に、同じ状態の否定か反対の状態を「だけ」で例外として足す。隣接する2文と反対語の表に限り、2文目は逆接の接続詞で始まるか「〜だけが／だけは」で主語を限る形を見る。別の述語の例外・否定した全称・引用は除く |
| `instruction_residue` | 全ジャンルで、「この記事では、専門用語を使わず、飾らない言葉で説明します」のように依頼文の書き方の条件を本文で述べ直した文、「推敲後の文章は以下のとおりです」「ご要望のとおり敬体に統一して推敲しました」のような作業報告、`【修正版】`・`【要確認】`の印。引用・コードの中は除く |
| `copy_fragment` | 全ジャンルで、「資料を、全員へ。」「作業を、もっと確かに。」のように、8文字以内の名詞句＋格助詞の後に読点で間を置き、述語を書かずに格助詞や「〜に／〜で」で終える30文字以内の文。動詞・形容詞の述語がある文、見出し・引用・リストは除く |
| `self_labeling_repetition` | 評価語を含む「〜のは」型の主題提示が文書内に3回以上ある |
| `short_topic_comma` | 文頭の5文字以内の名詞句＋「は」の直後に読点「、」がある。長い主題や動詞を含む節は除く |
| `short_object_comma` | 文頭の5文字以内の名詞句＋格助詞「を」の直後に読点「、」がある。長い名詞句・動詞を含む節・引用内は除く |
| `decision_direction_metaphor` | 全ジャンルで、選択する動作を「残さない側に倒す」「採用する側に倒す」「共通側／厳格側／既定側に倒す」等で表している。物理的な方向は除く |
| `negative_listing` | 同じ段落で否定文が2文続き、形態素8個以下の肯定文へ続く |
| `demonstrative_reference` | 同じ文の前方に動詞が2個以上ある位置に「このこと」等がある |
| `respectively_scope` | 列挙の後に「それぞれ」があり、後方に対応する列挙が見えない |
| `repeated_explanation_preview` | 同じ節の段落頭に「本節では〜説明します」等の予告が3回以上ある |
| `declared_item_count_mismatch` | 全ジャンルで、リスト直前の段落の最後の文が「次の3点」「以下の三つ」等と予告し、直後の箇条書きの同じ階層の項目数と一致しない。入れ子・項目内のコード・「以上」「のうち」・省略記号は数えない |
| `technical_jargon_metaphor` | CIの成功を「緑」、コードの公開を「出荷」と表すなど、技術対象と比喩的な言い回しが組み合わさっている |

`technical_jargon_metaphor`は、技術対象に続く次の言い回しも対象にします。

- 「静かに壊れる／失われる／消える」「黙って捨てる／無視する／スキップする」「地味に効く」「安全側／保守側に倒す」「時間を溶かす」
- 効く対象を後に回して予告する「（地味に）効いてくるのが／のは」。「薬が効いてくるのは」のように、同じ節で主語を先に示す文は除きます。
- 「バグ・不具合・懸念点等を潰す」。「時間を潰す」「箱を潰す」と、主要部が不具合の語でない複合名詞（「バグ報告」）は除きます。

技術対象には「整合性」「アクセシビリティ」等の品質特性も加え、「設定が**静かに壊れる**」のように太字で囲んだ箇所も同じ文脈として扱います。活用と近くの名詞・助詞を確認し、候補語の出現だけでは判定しません。

`decision_direction_metaphor`と`short_object_comma`は、通常の`lint`で使えます。前者は選択の条件と実際の動作を、後者は名詞句と述語のつながりを確認するための指摘です。どちらも自動修正せず、説明済みの比喩や意図した間は残せます。`short_topic_comma`と`short_object_comma`は、読点の後から次の句読点までに別の主語や連体修飾がある形を除きます。「鍵を、なくした人に届けた」のように、読点が掛かり先を読み分ける場合があるためです。

通常の`abstract_metaphor`は、抽象的な対象を「地図」「土台」等の役割で表す用例に加え、「仕様は意図を実装へ運ぶ」のような抽象語の関係や、抽象的な「入口」「主役」等も扱います。説明済みの内容や体験談にも一致するため、文脈から採否を決めます。

これらは`info`の読み直し候補です。形態素解析だけでは主語の省略、指示先、修飾先の正誤を決められません。[検出仕様と形態素解析の手順](https://github.com/nwiizo/suiko-eval/blob/main/eval/technical-wording.md)に、候補一覧、採用理由、実測した分割、評価結果を記録しています。

`mixed_latin_spacing`は大文字・小文字が一致する2文字以上の英数字語を比較し、単語内のハイフン・アンダースコアを許容します。左右の境界は別々に比べ、コード・URL・英語の複合語・数字で始まる単位表記を除きます。`boilerplate_closing`はMarkdownの除外範囲を除いた最後の文を見ます。いずれも自動修正せず、媒体の表記方針や読者への働きかけが必要かを確認するための候補です。

`vague_sensory_term`、`copy_fragment`、`technical_jargon_metaphor`は、形態素の並びで対象範囲を指します。人間の技術記事にも「腹落ち」「原因の解像度を上げる」「企業の温度感」の用例があり、AIによる執筆の判定には使えません。抽象的な語が何を指すかを、書き手が具体的に書けるか確かめるための候補です。

語彙は対象と動作の関係ごとに管理し、活用形・助詞・否定・別の節を確認します。たとえば「連休を溶かした」は読み直す候補ですが、「休日を使って金属を溶かす」は対象と動詞の組が異なるため除きます。候補が出ても、後悔や実感が伝わる比喩は残せます。[形態素で扱う範囲と限界](docs/morphological-revision.md)を参照してください。

`sensory_role_mismatch`は、次の型に限っています。

- 音を主語にして、窓・床などを「叩く」
- 温度・匂いを「見つめる／眺める」の目的語にする
- 静けさ・静寂を「鳴る／響く」の主語にする
- 測った値としての温度を「移る／暖める／漂う」等の主語にする
- 動作を表す連体節が「不在」を修飾する
- 「かぶる／まとう」の連体節が光・音・匂いを修飾する
- 光・音・匂い・空気・時間・冷たさ等を「見送る／起きる／揺れる／這い上がる」等の動作主にする

家具や器が「待つ／抱える／受け止める／首を振る」等をする型は`explanatory_metaphor`で返します。音の発生源や感覚をもたらす物との取り違えを確認するための候補で、意図した比喩は残せます。意味の矛盾全般や、省略された内容の正誤は判定しません。箇条書き等は既存の散文ルールと同じく対象外です。[記事の例文による検証と追加範囲](https://github.com/nwiizo/suiko-eval/blob/main/eval/calibration.md#感覚語と述語の組み合わせ2026-10-13)に、測定条件と限界を記録しています。

`quoted_character_count_mismatch`は、同じ行の`「文字列」のN文字`・`『文字列』というN字`等が文末または肯定の断定で終わる形を照合します。数字は半角・全角・漢数字（一〜九十九）を読み取り、引用内は漢字・仮名・英数字・空白に限定します。上限・概数・一部分の指定・疑問・否定・発言中の引用は除き、絵文字・結合文字・装飾を含む字数は判定しません。

`quantity_predicate_mismatch`は、次の3つの型に限っています。

- 速度・距離・長さ・重さ・重量・質量・温度・気温を「数える」
- 力を「一つ／ひとつ／1つ／１つ」抜く
- 疲れ・機嫌・眠気・緊張・不安等を、「枚・本・粒・滴」等の形のある物の助数詞で数える（「疲れを一枚ずつ」「眠気が一粒ずつ」）

この2つはどちらも`info`の確認候補で、自動修正は付けません。箇条書き・見出し・コード等は、ほかの散文ルールと同じく対象外です。

### 全規則の表示と規則の族

v0.4.3から、`lint`・`check`・`compare`は評価中の規則も含めて全規則を既定で表示します。`--experimental`は互換のため受け付けますが、出力は変わらず、不要になったことを標準エラーへ1行で伝えます。評価中の規則は、語彙多様性、文頭・文型の反復、装飾・箇条書きの比率、`elaborate_simile`（たとえる側に動詞の連体節が付いた直喩）、`mixed_latin_spacing`などで、確度はlowです。`uniform_bullet_structure`は`--genre essay`を指定した場合だけ出ます。

規則は、直すときに書き手が決めることで7つの族に分けています。意味（`meaning`）、型（`pattern`）、語（`wording`）、区切り（`punctuation`）、読解負荷（`reading_load`）、文書の傾向（`profile`）、照合（`consistency`）です。`capabilities --json`の各規則と`lint --json`の各指摘が`family`と`confidence`を返し、規則一覧は範囲（`scope`: 箇所か文書全体か）も返します。族の定義は[規則の族](docs/rule-families.md)にあります。

全部の指摘を、直すか、理由を付けて残すかに分けてください。確度highとmediumは、残す条件に当たらなければ直します。確度lowと文書の傾向は、該当箇所を読んで判断します。v0.4.3で次の規則を足し、広げました。

- `repeated_sentence_ending`: 同じ段落で、同じ文末（です・ます・でした・ました・である・だ・だろう・でしょう・ですね）が3文以上続く箇所を、3文目の文末で1件にまとめて示します。過去の「た」、「ません」、「のだ」は数えません。
- `negated_contrast`: 「AではなくB」「Aではない。Bだ。」を箇所ごとに示します。`antithesis_repetition`は文書全体の要約になり、範囲を持たず対応行だけを返します。
- `short_topic_comma`・`short_object_comma`・`short_subject_comma`（新設）: 文の中のすべての読点を見て、15字以内の名詞句＋「は／も（には・では等）」「を」「が」の直後の読点を示します。並べた名詞の最後、同じ助詞の句の並び、「今回は」のような副詞にもなる名詞、指示語・人称だけの句、接続の句は除きます。
- `abstract_metaphor`（抽象的なものの「土台」「羅針盤」）、`vague_sensory_term`（言葉や組織の「体温」「熱量」）、`technical_jargon_metaphor`（仕組みやルールが「効いた」、処理の始まりを「入口」「窓口」で表す形）の条件を広げました。

規則ごとの件数は、生成文と人間の文書の1000字あたりで[suiko-evalの実行比較](https://github.com/nwiizo/suiko-eval/blob/main/eval/competitive-review-2026-11-15.md)に記録しています。

既定の検出範囲が広がったため、v0.4.2以前のbaselineは作り直してください。古いbaselineは`stats.experimental`の不一致で止まり、作り直しを案内します。JSONの構造は追加だけですが、候補の件数と`--fail-on info`の終了結果が変わることがあります。不要なルールは`.suiko.toml`の`disabled_rules`や理由付きの個別許可で調整できます。

### 読解負荷を確認する

```sh
suiko lint draft.md --genre tech --json
```

`lint`は自然度の診断と同時に、次の読みにくさを確認します。

- 長すぎる一文、読点のない60字以上の一文
- 埋もれた列挙、疑問節の埋もれた列挙
- 長い連続漢字、二重否定、格助詞「の」の近接した連鎖
- 一つの名詞に前置された長い修飾節
- 句点を含む括弧書き（`sentence_in_paren`）

結果は`findings`と分けた`reading_load`へ出力し、`--baseline`比較と`--fail-on`判定には含めません。不要な場合は`--no-reading-load`で出力から外せます。以前の版の`--reading-load`も、互換のため受け付けます。

`long_attributive_span`は、述語を2つ以上含む30字以上の連体修飾節が一つの実質名詞に係り、その名詞句が「が」「を」「は」などで主節の項になっている文を指さします。読み手は名詞が出るまで修飾節全体を保留するため、被修飾名詞か述語を先に出すか、修飾節を独立した文に分けると読みやすくなります。「〜すること」「〜したとき」「〜する必要がある」のような形式名詞・枕の名詞と、「〜という関係である」の述語名詞は対象外です。連体形の個数で判定して人間文書で全発火した旧`nested_attributive`とは異なり、一つの名詞が背負う修飾節の長さと述語数を測ります。採用の経緯と実測は[校正記録](https://github.com/nwiizo/suiko-eval/blob/main/eval/calibration.md)にあります。

`buried_question_list`は、「〜が増えたか、〜も短くなったかを追います」のような文を指さします。疑問節「〜か」を読点で並べ、末尾の「か」に「を」「が」「は」「も」を付けて一つの述語へ係らせる形で、並んだ節が35字以上、文全体が45字以上のときに限ります。読み手は最初の「〜か、」で節が終わったと受け取り、最後まで読んで全体が一つの項だったと分かります。係り先の述語を先に出すか、節ごとに文を切ると読みやすくなります。次の形は対象外です。述語のない選択肢（「はいか、いいえかを」）、「〜かどうか」、推量の「〜かもしれない」、2節の選択疑問（「〜なのか、それとも〜なのかを」「〜なのか、〜なのかを分ける」）。

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

ジャンル、Suikoのバージョン、記録された`stats.experimental`（v0.4.3から常に`true`）が一致しない比較は実行エラーになります。設定による除外も比較に影響するため、同じ設定で実行してください。文書単位の反復は、抜粋が変わっても同じ反復状態が続けば継続扱いします。`--fail-on`は新規分だけでなく、今回の`findings`全体を判定します。

## 改稿前後の意味の変化を確かめる

```sh
suiko diff before.md after.md
suiko diff before.md after.md --json
# warn以上の候補があれば終了コード2を返す
suiko diff before.md after.md --fail-on warn
```

`--baseline`は指摘の増減だけを比べます。`diff`は改稿前後の本文を文・箇条書き・見出し・引用行の単位で対応づけ、書き換えた箇所で次の変化を候補として返します。意味が同じかどうかは判定しません。候補が出ない変更も読み直す対象です。

| category | 重要度 | 見る変化 |
|---|---|---|
| `number_changed` | warn / info | 数値と単位・約・最大・以上などの修飾、「<」「≥」などの比較の記号、曜日。桁区切りと全角・半角の違いは同じ値として扱い、文書内の別の文へ移した値は数えない。「30日」→「30日間」のように期間を表す「間」の有無だけが違う場合はinfo |
| `negation_changed` | warn | 対応する文で「ない」「ず」「ません」「不要」「禁止」などの否定の数が変わり、文末の述語の肯定・否定が入れ替わった箇所（「未実装です」→「実装します」のような名詞の述語を含む）と、字面がほぼ同じで両方にある語の打ち消しが増減した箇所。「防ぐ」「避ける」「なくなる」「止まる」などの回避・消失・停止の語は文末で否定と同じ向きに数え、「防ぐ」→「しないようにする」は示さず「防ぐ」→「防がない」は示す。「かもしれない」「なければならない」「ではなく」などの慣用句は数えない |
| `sentence_function_changed` | warn / info | 対応する文の文末の働き（依頼・禁止・義務・勧め・可能・推量・疑問・過去・体言止め・受身・可能・説明）。「保存できます」→「保存してください」などはwarn、「〜する」→「〜した」のような時制だけの変化と、受身・可能・尊敬のどれにも読める「れる／られる」との行き来はinfo、体言止めを文にした変化は数えない。可能は「できる」のほか、「読める」「使える」のように五段動詞から派生した可能動詞を辞書で確かめて含める |
| `certainty_changed` | warn | 「予定」「暫定」「一部」などの留保、「かもしれない」「と思われる」「見込み」「おそらく」などの推量、「必ず」「全て」「確定」「必須」などの強め。形態素の境界で照合する。推量は1文に1つと数え、同じ強さの推量どうしの言い換え（かもしれない→恐れがある）は示さない |
| `condition_changed` | warn / info | 「ば」「たら」「なら」「場合」「とき」などの条件、「だけ」「のみ」「以外」「除く」「ただし」などの限定、「など」「といった」「をはじめ」の例示の数。条件・限定が減るか、例示の語がすべて消えて一覧が閉じればwarn、それ以外はinfo |
| `frequency_changed` | warn / info | 「毎日」「毎週」「随時」「ときどき」「いつも」「定期的に」などの頻度の語。替わるか消えればwarn、増えただけならinfo。「毎回」と「〜するたびに」などの同じ頻度の言い換えは示さない |
| `choice_changed` | warn | 「または」「いずれか」「どちらか」の一つを選ぶ言い方と、「両方」「すべて」「いずれも」の全部を求める言い方が入れ替わった箇所。片側に両方の言い方がある文は比べない |
| `identifier_changed` | warn | インラインコード、URL、数字や記号を含む英数字の語、大文字の略語 |
| `quotation_changed` | warn | かぎ括弧の中身。引用符を外しただけで本文に同じ文字列が残る場合は除く |
| `citation_changed` | warn | `[1]`、`[^note]`、（著者 年）、注番号 |
| `subject_added` | warn | 変更前にない、人・組織・システムを表す主語（担当者、運用チーム、CLIなど） |
| `recipient_added` | warn | 書き換えた範囲で、通知・報告・依頼の相手を「管理者に」「運用チームへ」などと補った箇所。原文や資料に相手の指定があるか確認する |
| `subject_object_swapped` | warn | 「AがBを呼ぶ」が「BがAを呼ぶ」になる入れ替わり |
| `sentence_removed` / `sentence_added` | warn | 対応する文がなく、内容語も相手側に残っていない文 |
| `protected_changed` | warn | コードブロックの中身 |
| `table_changed` | warn / info | 表の行を字面の近さで対応づけ、セルの数値・識別子が変わればwarn、文言だけならinfo。対応のない行は件数を示す |
| `contrast_changed` | warn / info | 「AではなくB」の向きが逆になればwarn、否定した側Aが消えればinfo |
| `proper_noun_added` | warn | 書き換えた文に、変更前の文書のどこにもない人名・地名が現れたもの。別の文から移しただけの名前は示さない。DB・フロントエンドのような「固有名詞・一般」は対象外 |
| `content_words_changed` | info | 書き換えで消えた語と増えた語の一覧 |
| `structure_changed` | info | 箇条書きと地の文の入れ替わり、見出しの階層の変化 |
| `causal_changed` | warn / info | 対応する文で「ため」「ので」「により」「原因」などの因果の接続が増減した箇所。「関連」「相関」「関係」だけを述べた文を因果の文にした場合はwarn、それ以外はinfo |
| `feeling_added` | info | 変更前にない「申し訳」「安心」「ぜひ」「興味深い」などの気持ち・評価の語 |
| `promise_added` | warn / info | 変更前にない「いたします」「徹底します」「二度と」「努めてまいります」「保証する」「担保される」などの書き手の約束。保証・担保・再発防止・「二度と」・「約束します」はwarn、それ以外はinfo |
| `nominalization_added` | info | 「減らせます」→「削減が可能です」のように、「が可能」「の実施」「を担保」などの名詞中心の表現が増えた箇所。正式な文体や定義語を保ちながら、硬くなっていないか読み直す |
| `register_changed` | info | 文書全体で敬体の割合が20ポイント以上変わった（5文以上の文書） |

文の対応づけは、空白を除いて一致する文を錨にし、残った範囲を文字2連と内容語の一致率で組にします。1文を2文に分けた、2文を1文にまとめた、段落の順序を入れ替えただけの変更は、削除や追加として数えません。`sentence_function_changed`は対応する文の組だけで判定し、見出しは除きます。

文末の「削除してもよい」「削除して構いません」などは許可として扱います。許可から可能・勧め・自動動作の説明への変化は`warn`で、許可表現同士の言い換えは文末の働きの変化に数えません。`recipient_added`は、格助詞「に／へ」の前の人・組織・システムを表す名詞と、同じ節の最初の動詞が通知・報告等を表す場合に限ります。受身・使役や「管理者に代わって送る」のような形は除きます。同じ名詞主要部が変更前の対応範囲にあれば、新しい相手には数えません。省略された相手や、遠い係り受け、資料による裏付けは判断しません。

JSONは`suiko_version`、`before`と`after`（`file`と本文の`sha256`）、`stats`、`findings`を持ちます。`sha256`は検査した本文と最終稿が同じかを照らすための値で、最終稿の`shasum -a 256`と比べられます。各findingは`category`、`severity`、`before_line`、`after_line`、`before_excerpt`、`after_excerpt`、`detail`を持ち、片側にしかない文では反対側の行が`null`になります。終了コードは`lint`と同じく、実行エラーが1、`--fail-on`到達が2です。

上の表に出ない変化もあります。語ごと言い換えた否定、引用符のない発言の変更、述語ごとの否定の範囲、節の途中の受身・使役の変化は拾いません。可能動詞は五段動詞の対になる他動詞・自動詞（入れる、進める、伝える等）を一覧で除いていますが、一覧にない語では可能と誤って読むことがあります。

## 意味が定まらない表現をLLMで確認する

「小さな水槽の月のように光っていた」のように比喩の対応が成り立たない文や、前後で補えない標語は、形態素の並びでは判定できません。[suiko-evalの生成文評価](https://github.com/nwiizo/suiko-eval/blob/main/eval/slop-2026-11-06/README.md)でも、既定の規則が拾えたのは注釈の数%でした。`meaning`は、[todesking「スロップ表現100連発」](https://zenn.dev/todesking/articles/5bc43f449ecdf6)の意味破綻型・過剰圧縮型を基準にした依頼文と本文を、`--` の後に指定したコマンドの標準入力へ渡します。

```sh
suiko meaning draft.md -- claude -p --model sonnet --tools "" --disable-slash-commands
suiko meaning draft.md --json -- codex exec -m gpt-6.1-sol -s read-only --ephemeral --skip-git-repo-check --enable omit_skill_descriptions -
```

- コマンドはJSON（`{"items":[{"text","kind","pattern","reason"}]}`）を標準出力へ返すものを指定します。suikoはAPIの鍵も通信も持たず、コマンドの認証と費用は利用者の環境に従います。
- 返った抜き出しは、本文に一度だけ改行なしで現れるものだけを`semantic_breakdown`・`over_compression`の指摘（`info`）にします。本文にない・複数回現れる・形式が不正な抜き出しは「照合できなかった抜き出し」として件数と理由を示します。
- 結果は実行ごとに変わり得るため、`lint`・`check`・`verify`には含めません。JSONは`"deterministic": false`と使ったコマンドを記録します。
- コマンドにはツールやスキルを使わせない指定（例: `--tools ""`）を付けてください。文書を読む以外の操作をさせる必要はありません。
- 依頼文は、残してよい比喩（読み取れる比喩、後の文で理由を述べる比喩、働きが一読で分かる擬人化）を[yomiyasu](https://github.com/nanaism/yomiyasu)から取り入れ、候補ごとに精読した読みを書かせます。読みを書けた候補は、意味が定まっているとして捨てます。
- suiko-evalの生成文180文書（Sonnet）では、注釈50件のうち13件を拾い、文体の指定のない対照の文書には指摘が付きませんでした。人間の文書21本では5本に計6件付き、どれも書き手が選んだ比喩でした。指摘は「意味が一つに定まるか」を確かめる候補で、AIが書いたかどうかの判定には使えません（[評価の記録](https://github.com/nwiizo/suiko-eval/blob/main/eval/slop-2026-11-06/README.md)）。

## 最終稿と資料をまとめて検査する

```sh
suiko check final.md --before original.md --genre tech
suiko check final.md --before original.md --material measurements.md --genre tech --json
suiko verify final.md.suiko-check.json
# 必要なら、保存時の設定で検査結果も再計算する
suiko verify final.md.suiko-check.json --replay --json
```

`check`は最終稿のlintと読解負荷、原文とのdiff、提供資料との照合をまとめて実行します。資料を指定すれば照合は既定で有効です。`--material`は繰り返し指定できます。原文のない新規執筆では`--before`を省略できます。原文や資料のない検査はJSONで`null`、通常出力で未実施と表示します。本文を書き換えず、検査記録だけを保存します。

資料照合では、数値・単位、対象・環境・測定指標、効果や保証の表現、資料にある未確認事項・予定・限定条件を手掛かりにします。候補には最終稿の行と、関連する資料のパス・行・抜粋が付きます。同じ数字があっても、別の環境や対象の裏付けとは扱いません。資料自体の正しさや、意味が同じかどうかは判定しません。単位換算、言い換えや別の段落で説明された対象などは人が確認します。

記録の既定の保存先は`<最終稿>.suiko-check.json`です。`--record <path>`で変更できます。`.suiko.toml`のジャンル・独自ルール・除外設定を使い、CLI指定を優先します。`--fail-on`はlint・読解負荷・diff・資料照合の全候補に適用し、該当した場合も記録を保存して終了コード2を返します。

`verify`は記録と現在の最終稿・原文・資料・設定・実行ファイルのSHA-256を照合します。自動検出先に設定がなかった場合も、あとから作られたことを検出します。記録後に変わったものがあれば終了コード2となり、`check`の再実行が必要です。`--replay`は一致した入力で検査も再実行して結果を比較します。別のディレクトリからも実行できますが、入力を移動・削除すると再確認できません。記録は検査の実行を示し、内容の承認や意味の正しさは保証しません。

入力・保存・記録の形式エラーは終了コード1です。既存の無関係なファイルや入力ファイルへ記録を上書きしません。記録には絶対パスと検出箇所の抜粋が入るため、原稿・資料と同じ範囲で保管してください。[記録形式と確認範囲](docs/revision-checks.md)に詳細があります。

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

独自ルールを足しても、既存の設定と通常ルールのJSONはそのまま使えます。採用した機能と検証の限界は[比較調査と検証記録](https://github.com/nwiizo/suiko-eval/blob/main/eval/word-rules.md)にあります。

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

通常の呼び出しで本文の推敲まで行い、`quick`でも主述・目的語・論理関係・文末・語彙・資料照合を省きません。検出が0件でも全文を読み、13用途と読み手に応じて直すか残すかを判断します。CLIの候補、Skillの文脈による修正、改稿前後の照合を合わせた[既定の推敲手順](skills/suiko/references/revision-pass.md)を使います。意味・確度・常体と敬体を保ち、文長や語尾の変化を作るためだけには書き換えません。

CLIとSkillは別々に導入します。次のコマンドでSkillを導入できます。

```sh
npx skills add https://github.com/nwiizo/suiko --skill suiko
```

GitHub CLIの`gh skill`（preview）も利用できます。既定では最新のリリースタグから現在のプロジェクトへ導入し、`--scope user`を付けるとユーザー全体で使えます。

```sh
gh skill install nwiizo/suiko suiko --agent claude-code
```

導入したら、Skill対応エージェントで`$suiko`を指定します。CLIがなければSkillが`cargo install suiko`を試み、`cargo`もない場合は導入案内と手動チェックリストへ切り替えます。

Skillからは、[@textlint-ja/textlint-rule-preset-ai-writing](https://github.com/textlint-ja/textlint-rule-preset-ai-writing)による補助検査も実行できます。Node.js 20.18以降とnpmが必要です。固定バージョンをnpmの一時環境で実行し、対象プロジェクトの依存関係や設定は変更しません。結果はSuikoのfindingや前回比較へ加算せず、自動修正も行いません。

## 検証と開発

検出位置、除外条件、JSON、前回比較、終了コードを回帰テストで確認します。技術文書の追加ルールは、自作10文書・24項目の評価で期待どおりの検出を確認しました。これは仕様の確認であり、一般的な誤検出率や文章の改善効果を実証した結果ではありません。編集時間の短縮や文章の改善率は未測定です。

評価の対象と限界は[技術文書の検出検証](https://github.com/nwiizo/suiko-eval/blob/main/eval/technical-wording.md)、既存ルールの評価は[校正記録](https://github.com/nwiizo/suiko-eval/blob/main/eval/calibration.md)に記録しています。誤字脱字や組織固有の表記統一は、既存の校正工程と組み合わせてください。

リポジトリのCIと同じ確認コマンドです。

```sh
cargo +1.97 fmt --check
cargo +1.97 clippy --locked --all-targets --all-features -- -D warnings
cargo +1.97 test --locked --all-features --all-targets
```

評価用CLI`suiko-eval`と評価データ（コーパス、ラベル付き標本、校正記録）は、このリポジトリを大きくしないよう[suiko-eval](https://github.com/nwiizo/suiko-eval)へ分けています。suiko-evalは隣に置いたsuikoの作業ツリーを参照してビルドするため、未リリースの規則もそのまま評価できます。

```sh
cd ../suiko-eval
cargo +1.97 run --locked -- quality eval/corpus.toml --external
cargo +1.97 run --locked -- labeled eval/technical-wording.toml
```

評価データの準備と閾値の比較は[評価手順](https://github.com/nwiizo/suiko-eval/blob/main/eval/README.md)、リリースごとの変更は[CHANGELOG](https://github.com/nwiizo/suiko/blob/main/CHANGELOG.md)を参照してください。

## ライセンス

MIT。第三者由来の資料とフィクスチャに必要な表示は[THIRD_PARTY_NOTICES.md](https://github.com/nwiizo/suiko/blob/main/THIRD_PARTY_NOTICES.md)に収録しています。
