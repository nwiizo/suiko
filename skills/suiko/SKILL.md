---
name: suiko
description: 日本語文書のAI由来の均一さ、翻訳調、不自然さ、論旨、読解負荷を、決定的なRust CLIと目視で診断し、依頼に応じて書く・直す。日本語の学術論文・研究報告では、中心命題、用語、論証、DOCX/PDF納品を監査契約で確認する。Use when the user explicitly mentions suiko, asks whether Japanese text looks AI-generated, requests a naturalness score or final readability pass, wants a Japanese meeting note, report, guide, proposal, email, slide outline, blog, note, essay, academic article, or research report made natural and readable. Do not use for English text, image review, Markdown formatting alone, spelling or terminology normalization alone, or generic argument and author-voice review without a writing or revision task.
license: MIT
---

# Suiko

公開・共有する原稿について、読者が書き手の補足なしに意味を追えるように推敲する。通常の「suikoで」「推敲して」は、意味・事実・著者の判断を保って本文を直す依頼として扱う。診断や採点だけを明示された場合は書き換えない。検出件数の報告だけで終えず、修正済みの本文まで仕上げる。

毎回、[既定の推敲手順](references/revision-pass.md)と[文書・読者・資料の扱い](references/revision-context.md)を読む。検出が0件でも、この手順の主述・目的語・論理関係・文末・語彙・資料照合を行う。「AI臭い」と言われたときだけの追加作業にしない。[表現別の修正判断](references/revision-catalog.md)を全文の見直しに使い、CLIに該当ルールがない表現も直す。

名前だけで呼ばれた場合は、直前に依頼されて書いた本文があればそれを直す。作業報告や指示ファイルは自動的に推敲対象にしない。本文がなければ作業ディレクトリ直下の`.md`・`.txt`から指示・設定・中間稿を除き、候補が1つならそれを選び、複数なら対象を確認する。名前だけで選んだファイルは原文を保ち、別名の最終稿へ保存する。ファイルの修正を依頼された場合は、その指定先へ反映する。

## モードを選ぶ

- 標準（既定、`quick`も同じ）: 全文を推敲し、`check`で本文・原文・資料を検査し、提出前に`verify`で確認する。`quick`は報告を短くする指定で、修正項目を省く指定ではない。
- `full`: 対外文書、経営向け文書、約1万字を超える文書。`lint`、`outline`、`terms` と目視レビューをすべて使う。
- `score`: 書き換えず自然度と理由だけを返す。最初に [diagnose.md](references/diagnose.md) を読む。
- `academic`: 学術論文・研究報告・投稿用DOCX/PDF。最初に[academic-delivery.md](references/academic-delivery.md)を読み、中心命題、説明対象、順序、用語、節間接続、注、成果物を監査する。

CLI が見つからない場合（`suiko --version` が失敗する場合）は、次の順で自分で導入する。

1. `cargo` があれば `cargo install suiko` を実行する（crates.io から取得。初回ビルドは形態素辞書の取得と埋め込みを含むため数分かかり、ネットワークが必要）。完了後に `suiko --version` で確認する。
2. `cargo` がない、またはインストールに失敗した場合は、導入手順（rustup で Rust 1.97 以降を入れてから `cargo install suiko`）をユーザーへ提示し、その回の診断は [manual-checklist.md](references/manual-checklist.md) で手動診断する。

リポジトリが手元にある場合に限り、`cargo run --release --` を `suiko` の代わりに使える。

`suiko capabilities --json`で利用できるコマンド・オプション・規則を確認する。未対応版だけ必要なサブコマンドの`--help`へ戻る。`check`・`verify`がなければ利用できる`lint`・`diff`と資料の読み合わせで仕上げ、未実施の検査を伝える。`diff`もなければ原文と手動で読み比べる。バージョン番号だけで機能の有無を決めない。

## 1. 読者と骨格を決める

読者、読後に起きてほしいこと、主メッセージを一文で特定する。不明で結果が大きく変わる場合だけユーザーに確認する。

文書タイプは[文書・読者・資料の扱い](references/revision-context.md)の13用途から判断する。用途が不明でも、主体・口調・専門性を推測で作らず標準の推敲を進める。既存の詳細な型が必要なら次を読む。

技術解説・仕様・操作手順・障害報告・技術的なPR説明は、[技術文書の推敲](references/technical-revision.md)を使い、`--genre tech`を指定する。本文だけでなく見出し・箇条書き・表も直す。動作や条件を読み違える表現は、原文・資料に根拠がある範囲で具体化する。エッセイの個人的な表現には同じ基準を一律に適用しない。

- 議事録: [minutes.md](references/doctypes/minutes.md)
- 調査・分析レポート: [report.md](references/doctypes/report.md)
- 社内ガイド・マニュアル: [guide.md](references/doctypes/guide.md)
- リサーチメモ・企画書・提案書: [memo.md](references/doctypes/memo.md)
- スライド構成: [slide.md](references/doctypes/slide.md)

メール・チャットの依頼や返信、PR説明、障害連絡では[message.md](references/doctypes/message.md)を読む。挨拶・敬語・体言止めの変更項目・実際の質問を、定型表現という理由だけで削らない。

新規執筆では、見出しから必要な情報を探せる構成を選ぶ。改稿では、依頼された見出し・順序・まとめ・字数と原文の文体を保つ。内容に応じて説明の長さを決め、節の長短を作ること自体を目的にしない。判断に必要な情報が欠ける場合だけ、原文や提供資料に根拠を探す。

学術稿では、見出しを先に整えるより、`academic-delivery.md`の順に、説明対象、中心命題、ユーザーが示した論理順序、形式的RQの要否、用語のstatusを固定する。文章を整えるために章順、概念、結論を変えない。対象を説明してから名称を導入し、既存語は出典、造語は造語であることを本文へ示す。

## 2. 文体制約の下で書く

新規執筆や大きな改稿では [writing-constitution.md](references/writing-constitution.md) を読む。以下を基本とし、エッセイの結論の位置や意図した反復は [ジャンル別の方針](references/genre-notes.md) に従う。

- 読者に必要な答えを見つけやすくする。背景や実行順が必要なら先に置く。
- 原文にある主張・条件・否定・数量・確度・常体と敬体を保つ。原文にない担当者、効果、経験を足さない。
- 文末は説明・依頼・助言・許可・能力・予定の働きに合わせる。語尾を散らすためだけに変えない。
- 見出し、箇条書き、反復、専門用語は役割で判断する。件数や割合だけで消したり言い換えたりしない。
- まず主体・動作・条件を読み取り、説明のつながりを整えてから重複を縮める。比喩を別の抽象語へ置き換えるだけで済ませない。

ユーザー指定の `style-profile.md` があれば優先する。文体の学習を明示的に求められた場合だけ、[style-profile-template.md](assets/style-profile-template.md) を使って過去文書3〜5本から傾向を抽出する。

文書群の集計・Gitの版との比較・文体観測・編集判断の集計が必要なら[文書群の検査](references/batch-review.md)を読む。`style-profile`は指定された記事だけを使い、観測値から外れることを誤りとしない。

## 3. 決定的な検査を行う

対象をファイルへ保存し、ジャンルが分かる場合は `essay`、`tech`、`business` を指定する。

```sh
suiko lint <file> --json
suiko lint <file> --genre tech --json
```

`lint` は既定で読解負荷レーン（`reading_load`）も返す（v0.3.10以降。v0.3.9以前は `--reading-load` が必要）。自然度の `findings` と混ぜずに読む。読みやすさを対象にしない依頼では `--no-reading-load` で外してよい。

`reading_load` は一文長、埋もれた列挙、疑問節の埋もれた列挙（`buried_question_list`）、連続漢字、二重否定、「の」連鎖、一つの名詞に前置された長い修飾節（`long_attributive_span`）を見るための指さしであり、自然度 finding やベースラインには含まれない。

表現の修正候補は通常の`lint`で確認でき、`--experimental`やジャンル指定を必要としない。比喩、感覚語、主題・目的語の読点、コピー調、指示の残り、数量・列挙の不整合に加え、`auxiliary_kana`（補助動詞と形式名詞のかな表記）、`renyo_noun`（「結果の確かめ」のような連用形名詞）、`explanatory_metaphor`、`revision_expression`も読む。候補は命令ではなく、[用途別の残す条件](references/revision-context.md)と原文から採否を決める。`--experimental`は文書全体の反復・比率等の追加観測に使うもので、通常の推敲には不要。

`bold_not_rendered`（通常検出、`warn`）は、`**「用語」**の`のように太字がCommonMarkで表示されない書き方を指す。公開先がGitHubやmarkdown-it系なら直す。利用できる版では`suiko fix <file> --bold`で修正済み本文をプレビューし、ファイルの修正を依頼されている場合は`--write`で保存できる。保存後もlint・diffを確認する。曖昧な区切りは自動修正されないため個別に読む。通常検出の`instruction_residue`は、依頼文の書き方の条件や推敲の作業報告が本文に残った文を指す。読者に向けた文でなければ取り除く。

full では構造と用語も抽出する。

```sh
suiko outline <file> --json
suiko terms <file> --json
suiko lexical-audit <file> --reference <lexical-reference.json> --json
```

学術稿では、段落第一文と節間の橋を`outline`で確認した後、監査契約を作る。契約がある場合は、契約違反を直すか理由を記録し、提出前に`academic`を実行する。

```sh
suiko academic <paper.md> --contract <academic-contract.json> --json
suiko academic <paper.md> --contract <academic-contract.json> \
  --docx <submission.docx> --template <official-template.docx> \
  --pdf <submission.pdf> --export-record <delivery-record.json> --json
```

`academic`は意味の自動採点ではない。防御的留保、未登録ラベル、引用と参考文献、節間の共有対象、注分類、Markdown/DOCX/PDF同期、OOXML不変条件を確認する。注の本文・注・不要の判断と、段落・節の意味上の修正は、全文脈を読んで行う。

`lint`は複数ファイルを指定でき、`lint`・`diff`は`-`で標準入力を受け取る。`check`は再確認できるファイルを使う。CIで検出を失敗扱いにする必要がある場合だけ`--fail-on warn`などを使う。通常のfindingは終了コード0、入力・保存エラーは1、`--fail-on`到達は2。`verify`は検査後の変更がある場合も2を返す。

`lint` はカレントディレクトリの `.suiko.toml` を自動検出する。プロジェクトに設定がある場合は、既定ジャンル、severity gate、無効化ルール、理由付きの個別許可をその方針として扱う。CLIの `--genre` と `--fail-on` は設定より優先される。設定を切り分ける必要があるときだけ `--no-config`、別の設定を使うときは `--config <path>` を指定する。設定の新規作成や許可項目の追加は、ユーザーがプロジェクト方針の変更を求めた場合に限る。

Node.js 20.18以降とnpmが使える場合は、Suikoと同じ対象へtextlintのAI文章presetも実行する。プロジェクトのtextlint設定が同じpresetを有効にしている場合は、ロックされた依存関係と既存の許可設定を尊重して、そのプロジェクトの検査コマンドを一度だけ使う。設定されていない場合は、Skillディレクトリを基準に次の同梱スクリプトを使う。

```sh
sh <skill-directory>/scripts/run-textlint-ai-writing.sh <file>
```

この検査は固定したtextlintとpresetをnpmの一時環境で実行し、プロジェクトの依存関係や設定を変更しない。`--fix`は使わない。終了コード1でもJSONに`messages`があれば検出結果として扱う。Node/npmがない、ネットワークから取得できない、対象形式を処理できないなどJSONを取得できない場合は、理由を一言示してSuikoの検査だけを続ける。textlintの結果は別の検査結果として扱い、Suikoの自然度スコア、baseline、finding件数には加えない。

## 4. finding を判断する

finding は疑いであって命令ではない。該当行と周辺文脈を読み、各 finding を次のどちらかへ分類する。

- 直す: 意味と事実を保った修正案を作る。
- 残す: 固有名詞、技術用語、意図した反復、ジャンル上自然などの理由を一言記録する。

検出が0件でも、全文の主述・修飾・指示先・条件を確認する。無生物主語でも「サーバーが通知を送る」のような動作説明は残す。道具が人の事情を知るような擬人化や、名詞だけで動作を省いた説明は、原文・資料から分かる動作へ開く。確定できない内容を補わず、意味を保って直せる範囲を先に仕上げ、必要な確認を本文の後に分ける。

修正案は、何が起きるか、誰に何を求めるか、どの条件で成り立つかを原文と読み比べて採る。「重要なのは」の評価、「かもしれない」の留保、必要な否定対比を削らない。短い主題・目的語の直後でも、後ろの節の主語や修飾先を読み分ける読点は残す。判断例は[examples.md](references/examples.md)にある。

通常の推敲でも、検出の有無に加えて[改稿ガイド](references/revision-guide.md)の「意味の補完を読者へ押し付けていないか」を確認する。対象・条件・理由が本文にあるか、読み手の推測でつないでいないかを読む。省くと結論や次の行動が変わる情報を資料から補い、共有済みの前提や意図した余韻は残す。補う根拠がなければ、読みが分かれる箇所と確認事項を示す。著者の判断や経験を作り足さない。

カテゴリ別の判断に迷った場合だけ、必要な参照を読む。

- 禁止語・LLM常套句: [forbidden-patterns.md](references/forbidden-patterns.md)
- 抽象比喩: [revision-guide.md](references/revision-guide.md)
- 翻訳調・英語統語: [translationese.md](references/translationese.md)
- 修正方針と判断台帳: [revision-guide.md](references/revision-guide.md)
- 読みやすさの原則: [readability-principles.md](references/readability-principles.md)
- 読解負荷の詳細: [readability-antipatterns.md](references/readability-antipatterns.md)
- ジャンル差: [genre-notes.md](references/genre-notes.md)
- エッセイで反復・迷い・語調の変化を残す判断: [narrative-judgment.md](references/narrative-judgment.md)
- Before/After: [examples.md](references/examples.md)

`outline` では論旨、見出し、反復、節の濃淡、結論の位置を見る。`terms` の `has_gloss_hint` は説明済みという判定ではなく、初出付近に説明マーカーがあるという手掛かりとして扱う。一般名詞複合語や漢語・和語等の統制を確認するときは`lexical-audit`を使う。新奇複合語とレジスター共起はinfoの確認候補であり、参照資源にない意味上の同義性を推測せず、原稿を書き換えない。

## 既存の校正工程との境界

既存の校正設定や用語辞書はプロジェクト規約として尊重し、Suikoで置き換えない。Suikoは形態素列、文書内の反復、構造、読解負荷を受け持つ。textlintのAI文章presetはMarkdown AST上の定型表現、強調、箇条書き、コロン接続を補う。両方が同じ箇所を指した場合は、判断台帳では一つの修正として扱い、同じ修正を二重に適用しない。一般的な表記規則や製品名の正規化は既存の工程へ残す。

## 5. 最終稿を検査して提出する

最終稿を保存したら、`check`で検査をまとめて実行する。原文があれば`--before`、資料があれば`--material`を渡す。資料が複数なら繰り返し指定する。`check`も`.suiko.toml`を読み、独自ルールと除外設定を適用する。

```sh
suiko check <after> --before <before> --material <source> --genre tech --json
suiko verify <after>.suiko-check.json --json
```

`check`のlint・読解負荷・差分・資料照合の候補を読み、修正したら再検査する。提出直前に`verify`を実行する。記録後に本文・原文・資料・設定・実行ファイルが変わると一致しなくなる。`verify --replay`は検査結果も再計算する。完了報告には実施した検査と記録の場所を添える。記録は意味や事実の合格証明ではない。原文・資料がない検査を実施済みと報告しない。

指摘の増減を追いたい場合は、次のbaselineも使える。

修正前のJSONを作業用ファイルへ保存し、修正後に比較する。

```sh
suiko lint <file> --genre tech --json > /tmp/suiko-before.json
suiko lint <file> --genre tech --baseline /tmp/suiko-before.json --json
```

同じバージョン・ジャンル・`--experimental`指定・設定・ファイル名で比較する。v0.3.11へ更新したらbaselineを作り直す。`new`と`persisting`は、直すか、意図して残す理由を判断する。`resolved`も意味の改善を保証しないため、変更箇所を読み直す。`reading_load`は比較に含まれないので今回の出力を別に確認する。同じ指摘の往復が続く場合は、文・段落の構造を見直すか理由を付けて残す。

`check`に含まれる`diff`で、原文にあった条件・数値・例外の脱落と、原文にない主体・通知先・原因・効果の追加を別々に確かめる。手順や仕様では、依頼・許可・可能・自動動作の区別も保つ。`recipient_added`は通知・報告・依頼の相手の追加を指す。差分だけを見たい場合は`suiko diff <before> <after> --json`も使える。

`diff`は数値、否定、文末の働き、留保の語、条件・限定、識別子、引用、新しい主語などの変化を候補として返す。候補ごとに前後を読み、脱落や根拠のない追加なら修正し、意図した変更なら理由を記録する。候補が出ない変更も意味が同じとは限らない。判断例は[改稿ガイド](references/revision-guide.md)の「改稿前後を双方向に照合する」を参照する。

最後に初見の読者として通読し、主語述語と事実関係が修正前から壊れていないか確認する。`nominalization_added`も読み、短い動詞を硬い名詞の連続へ変えていないか確かめる。原文がなく資料から書いた場合も、資料中の値・担当・条件・未確認事項と最終稿を照合する。本文のみの指定なら本文だけを返す。それ以外は本文、実際の変更点、必要な確認事項、実行した検査を簡潔に示す。検査記録と参照する原文・資料は、再確認が必要な間は一緒に保管し、その他の自分が作った中間データだけを片付ける。保存先はユーザーの指定を優先する。

意味的な話題の平板さは、埋め込みモデルを使う自動findingへ含めない。各段落が観測、理由、結果、具体例、判断、限界のいずれかを前段へ追加しているかを目視で確認する。モデル実験では二つの構成例を安定して区別する根拠が弱く、初回取得、配布量、メモリ使用量がオフラインの単一バイナリという境界に見合わなかった。
