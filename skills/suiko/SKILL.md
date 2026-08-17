---
name: suiko
description: 日本語文書のAI由来の均一さ、翻訳調、不自然さ、論旨、読解負荷を、決定的なRust CLIと目視で診断し、依頼に応じて書く・直す。Use when the user explicitly mentions suiko, asks whether Japanese text looks AI-generated, requests a naturalness score or final readability pass, wants a Japanese meeting note, report, guide, proposal, email, slide outline, blog, note, or essay made natural and readable, or asks to learn a reusable style profile from 3–5 past Japanese documents for a concrete writing task. Do not use for English text, image review, Markdown formatting alone, spelling or terminology normalization alone, or generic argument and author-voice review without a writing or revision task.
license: MIT
---

# Suiko

仕事の日本語を「設計 → 執筆 → 検査 → 収束」の順で推敲する。機械検出を疑いの入口として使い、文脈を読んで直すか残すかを判断する。findingを機械的に消すのではなく、採否と理由を決める。

## モードを選ぶ

- `quick`（既定）: 日常の短い文書。該当する文書型を確認し、`suiko lint` と通読で仕上げる。
- `full`: 対外文書、経営向け文書、約1万字を超える文書。`lint`、`outline`、`terms` と目視レビューをすべて使う。
- `score`: 書き換えず自然度と理由だけを返す。最初に [diagnose.md](references/diagnose.md) を読む。

CLI が見つからない場合は、このSkillのリポジトリで `cargo run --release --` を `suiko` の代わりに使う。それもできなければ [manual-checklist.md](references/manual-checklist.md) を使う。

## 1. 読者と骨格を決める

読者、読後に起きてほしいこと、主メッセージを一文で特定する。不明で結果が大きく変わる場合だけユーザーに確認する。

文書タイプに対応する型を読む。

- 議事録: [minutes.md](references/doctypes/minutes.md)
- 調査・分析レポート: [report.md](references/doctypes/report.md)
- 社内ガイド・マニュアル: [guide.md](references/doctypes/guide.md)
- リサーチメモ・企画書・提案書: [memo.md](references/doctypes/memo.md)
- スライド構成: [slide.md](references/doctypes/slide.md)

主メッセージを見出しへ落とし、見出しだけで論旨が通るようにする。重要な節を厚く、軽い節を短くし、全節を同じ型と長さに揃えない。素材が乏しい場合は言い回しを作る前に、固有名詞、数値、実例、一次情報を集める。

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
```

複数ファイルも指定できる。標準入力は `<command> -` で受け取る。CIで検出を失敗扱いにする必要がある場合だけ `--fail-on warn` などを使う。通常の finding は exit code 0、入力エラーは1、`--fail-on` 到達は2である。

`lint` はカレントディレクトリの `.suiko.toml` を自動検出する。プロジェクトに設定がある場合は、既定ジャンル、severity gate、無効化ルール、理由付きの個別許可をその方針として扱う。CLIの `--genre` と `--fail-on` は設定より優先される。設定を切り分ける必要があるときだけ `--no-config`、別の設定を使うときは `--config <path>` を指定する。設定の新規作成や許可項目の追加は、ユーザーがプロジェクト方針の変更を求めた場合に限る。

## 4. finding を判断する

finding は疑いであって命令ではない。該当行と周辺文脈を読み、各 finding を次のどちらかへ分類する。

- 直す: 意味と事実を保った修正案を作る。
- 残す: 固有名詞、技術用語、意図した反復、ジャンル上自然などの理由を一言記録する。

カテゴリ別の判断に迷った場合だけ、必要な参照を読む。

- 禁止語・LLM常套句: [forbidden-patterns.md](references/forbidden-patterns.md)
- 翻訳調・英語統語: [translationese.md](references/translationese.md)
- 修正方針と判断台帳: [revision-guide.md](references/revision-guide.md)
- 読みやすさの原則: [readability-principles.md](references/readability-principles.md)
- 読解負荷の詳細: [readability-antipatterns.md](references/readability-antipatterns.md)
- ジャンル差: [genre-notes.md](references/genre-notes.md)
- Before/After: [examples.md](references/examples.md)

`outline` では論旨、見出し、反復、節の濃淡、結論の位置を見る。`terms` の `has_gloss_hint` は説明済みという判定ではなく、初出付近に説明マーカーがあるという手掛かりとして扱う。

## 既存の校正工程との境界

既存の校正設定や用語辞書はプロジェクト規約として尊重し、Suikoで置き換えない。Suikoは自然さ、構造、読解負荷を受け持ち、一般的な表記規則や製品名の正規化は既存の工程へ残す。結果が衝突した場合も件数だけで決めず、対象プロジェクトの規約、事実、意図した文体を先に確認し、同じ修正を二重に適用しない。

## 5. ベースラインで収束させる

修正前のJSONを作業用ファイルへ保存し、修正後に比較する。

```sh
suiko lint <file> --genre tech --json > /tmp/suiko-before.json
suiko lint <file> --genre tech --baseline /tmp/suiko-before.json --json
```

`new` をゼロにし、`persisting` をすべて「直した」か「残す（理由）」へ分類する。同じ finding が二周続けて再発したら、その一文だけを往復修正せず、文・段落の構造から見直すか理由を付けて残す。

最後に初見の読者として通読し、声に出して読めるリズムか、主語述語と事実関係が修正前から壊れていないか確認する。作業用JSONや中間稿は完成時に破棄する。ユーザーが保存を求めた場合だけ、指定された場所へ残す。

意味的な話題の平板さは、埋め込みモデルを使う自動findingへ含めない。各段落が観測、理由、結果、具体例、判断、限界のいずれかを前段へ追加しているかを目視で確認する。モデル実験では二つの構成例を安定して区別する根拠が弱く、初回取得、配布量、メモリ使用量がオフラインの単一バイナリという境界に見合わなかった。
