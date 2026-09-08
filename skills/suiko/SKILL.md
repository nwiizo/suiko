---
name: suiko
description: 日本語文書のAI由来の均一さ、翻訳調、不自然さ、論旨、読解負荷を、決定的なRust CLIと目視で診断し、依頼に応じて書く・直す。日本語の学術論文・研究報告では、中心命題、用語、論証、DOCX/PDF納品を監査契約で確認する。Use when the user explicitly mentions suiko, asks whether Japanese text looks AI-generated, requests a naturalness score or final readability pass, wants a Japanese meeting note, report, guide, proposal, email, slide outline, blog, note, essay, academic article, or research report made natural and readable. Do not use for English text, image review, Markdown formatting alone, spelling or terminology normalization alone, or generic argument and author-voice review without a writing or revision task.
license: MIT
---

# Suiko

仕事の日本語を「設計 → 執筆 → 検査 → 収束」の順で推敲する。機械検出を疑いの入口として使い、文脈を読んで直すか残すかを判断する。findingを機械的に消すのではなく、採否と理由を決める。

## モードを選ぶ

- `quick`（既定）: 日常の短い文書。該当する文書型を確認し、`suiko lint` と通読で仕上げる。
- `full`: 対外文書、経営向け文書、約1万字を超える文書。`lint`、`outline`、`terms` と目視レビューをすべて使う。
- `score`: 書き換えず自然度と理由だけを返す。最初に [diagnose.md](references/diagnose.md) を読む。
- `academic`: 学術論文・研究報告・投稿用DOCX/PDF。最初に[academic-delivery.md](references/academic-delivery.md)を読み、中心命題、説明対象、順序、用語、節間接続、注、成果物を監査する。

CLI が見つからない場合（`suiko --version` が失敗する場合）は、次の順で自分で導入する。

1. `cargo` があれば `cargo install suiko` を実行する（crates.io から取得。初回ビルドは形態素辞書の取得と埋め込みを含むため数分かかり、ネットワークが必要）。完了後に `suiko --version` で確認する。
2. `cargo` がない、またはインストールに失敗した場合は、導入手順（rustup で Rust 1.97 以降を入れてから `cargo install suiko`）をユーザーへ提示し、その回の診断は [manual-checklist.md](references/manual-checklist.md) で手動診断する。

リポジトリが手元にある場合に限り、`cargo run --release --` を `suiko` の代わりに使える。

## 1. 読者と骨格を決める

読者、読後に起きてほしいこと、主メッセージを一文で特定する。不明で結果が大きく変わる場合だけユーザーに確認する。

文書タイプに対応する型を読む。

- 議事録: [minutes.md](references/doctypes/minutes.md)
- 調査・分析レポート: [report.md](references/doctypes/report.md)
- 社内ガイド・マニュアル: [guide.md](references/doctypes/guide.md)
- リサーチメモ・企画書・提案書: [memo.md](references/doctypes/memo.md)
- スライド構成: [slide.md](references/doctypes/slide.md)

主メッセージを見出しへ落とし、見出しだけで論旨が通るようにする。重要な節を厚く、軽い節を短くし、全節を同じ型と長さに揃えない。素材が乏しい場合は言い回しを作る前に、固有名詞、数値、実例、一次情報を集める。

学術稿では、見出しを先に整えるより、`academic-delivery.md`の順に、説明対象、中心命題、ユーザーが示した論理順序、形式的RQの要否、用語のstatusを固定する。文章を整えるために章順、概念、結論を変えない。対象を説明してから名称を導入し、既存語は出典、造語は造語であることを本文へ示す。

## 2. 文体制約の下で書く

新規執筆や大きな改稿では [writing-constitution.md](references/writing-constitution.md) を読む。特に次を守る。

- 結論を先に置き、前置きで助走しない。
- 見出しを内容ラベルではなくメッセージにする。
- 箇条書きは真に並列な情報の圧縮にだけ使う。
- 専門用語は機能を説明してから名前を出す。
- 事実と意見、限界と推定を区別する。
- 固有名詞、数値、具体例で一般論を接地する。
- 同じ文型や対比構文を三度繰り返さない。

ユーザー指定の `style-profile.md` があれば優先する。文体の学習を明示的に求められた場合だけ、[style-profile-template.md](assets/style-profile-template.md) を使って過去文書3〜5本から傾向を抽出する。

## 3. 決定的な検査を行う

対象をファイルへ保存し、ジャンルが分かる場合は `essay`、`tech`、`business` を指定する。

```sh
suiko lint <file> --json
suiko lint <file> --genre tech --json
```

読みやすさも対象なら、自然度検出と混ぜず別レーンを追加する。

```sh
suiko lint <file> --genre tech --reading-load --json
```

`reading_load` は一文長、埋もれた列挙、連続漢字、二重否定、「の」連鎖を見るための指さしであり、自然度 finding やベースラインには含まれない。

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

複数ファイルも指定できる。標準入力は `<command> -` で受け取る。CIで検出を失敗扱いにする必要がある場合だけ `--fail-on warn` などを使う。通常の finding は exit code 0、入力エラーは1、`--fail-on` 到達は2である。

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

カテゴリ別の判断に迷った場合だけ、必要な参照を読む。

- 禁止語・LLM常套句: [forbidden-patterns.md](references/forbidden-patterns.md)
- 抽象比喩: [revision-guide.md](references/revision-guide.md)
- 翻訳調・英語統語: [translationese.md](references/translationese.md)
- 修正方針と判断台帳: [revision-guide.md](references/revision-guide.md)
- 読みやすさの原則: [readability-principles.md](references/readability-principles.md)
- 読解負荷の詳細: [readability-antipatterns.md](references/readability-antipatterns.md)
- ジャンル差: [genre-notes.md](references/genre-notes.md)
- Before/After: [examples.md](references/examples.md)

`outline` では論旨、見出し、反復、節の濃淡、結論の位置を見る。`terms` の `has_gloss_hint` は説明済みという判定ではなく、初出付近に説明マーカーがあるという手掛かりとして扱う。一般名詞複合語や漢語・和語等の統制を確認するときは`lexical-audit`を使う。新奇複合語とレジスター共起はinfoの確認候補であり、参照資源にない意味上の同義性を推測せず、原稿を書き換えない。

## 既存の校正工程との境界

既存の校正設定や用語辞書はプロジェクト規約として尊重し、Suikoで置き換えない。Suikoは形態素列、文書内の反復、構造、読解負荷を受け持つ。textlintのAI文章presetはMarkdown AST上の定型表現、強調、箇条書き、コロン接続を補う。両方が同じ箇所を指した場合は、判断台帳では一つの修正として扱い、同じ修正を二重に適用しない。一般的な表記規則や製品名の正規化は既存の工程へ残す。

## 5. ベースラインで収束させる

修正前のJSONを作業用ファイルへ保存し、修正後に比較する。

```sh
suiko lint <file> --genre tech --json > /tmp/suiko-before.json
suiko lint <file> --genre tech --baseline /tmp/suiko-before.json --json
```

`new` をゼロにし、`persisting` をすべて「直した」か「残す（理由）」へ分類する。同じ finding が二周続けて再発したら、その一文だけを往復修正せず、文・段落の構造から見直すか理由を付けて残す。

最後に初見の読者として通読し、声に出して読めるリズムか、主語述語と事実関係が修正前から壊れていないか確認する。作業用JSONや中間稿は完成時に破棄する。ユーザーが保存を求めた場合だけ、指定された場所へ残す。

意味的な話題の平板さは、埋め込みモデルを使う自動findingへ含めない。各段落が観測、理由、結果、具体例、判断、限界のいずれかを前段へ追加しているかを目視で確認する。モデル実験では二つの構成例を安定して区別する根拠が弱く、初回取得、配布量、メモリ使用量がオフラインの単一バイナリという境界に見合わなかった。
