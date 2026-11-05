# 文書群の検査と編集判断

以下は0.4.1の作業ツリーで利用できる機能。導入先で使えるかは`suiko capabilities --json`で確認する。未対応のCLIでは必要なサブコマンドの`--help`を調べる。

## 集計とGit比較

```sh
suiko capabilities --json
suiko lint article-a.md article-b.md --genre tech --summary json
suiko lint article-a.md article-b.md --genre tech --summary tsv
suiko compare --rev HEAD article-a.md article-b.md --genre tech --json
suiko compare --rev HEAD article-a.md article-b.md --genre tech --summary tsv
```

機能一覧は`schema_version`、`suiko_version`、`commands`、`rules`、`check_record`を返す。コマンドと規則は実際の登録情報から取得する。規則には自然さ／読解負荷の区別と実験機能かどうかを含む。

集計JSONは`files`・`categories`・`total`のそれぞれに`natural`と`reading_load`を返す。TSVは`kind file category natural reading_load`の5列で、ファイル別・カテゴリ別・全体を区別する。既存の`lint --json`は個々のfindingを読むために残す。

`--no-reading-load`を指定した場合は、JSONで`reading_load_enabled: false`、各`reading_load`を`null`、TSVでは該当列を空欄にする。未計測を検出0件と区別する。

Git比較JSONの`natural`と`reading_load`は、各々`before`・`after`・`resolved`・`new`・`persisting`を持つ。`files`に`file`・`before_file`・`file_status`、全体に比較元のコミットIDを含む。`file_status`は`matched`・`renamed`・`added`・`deleted`。前後を現在の実行ファイルと同じ設定で再検査するため、古い版の診断JSONを用意する必要はない。件数の減少は品質の向上や意味の保持を示さない。

現在の継続判定は規則と抜粋を使うため、同じ箇所の言い換えでも「解消＋新規」になることがある。`new`を新たな欠陥の件数と解釈しない。文の対応づけと個別指摘の出力はTODOのN1・N2で管理する。

比較TSVの5列は次のとおり。パス内のバックスラッシュ、タブ、改行は`\\`・`\t`・`\r`・`\n`でエスケープし、一つの行に保つ。

```text
file  natural_before  natural_after  reading_load_before  reading_load_after
```

比較はGitリポジトリ内のUTF-8の通常ファイルを明示して使う。ディレクトリや標準入力は指定しない。Gitが認識したリネームを使い、ステージ済みのリネーム後に本文を編集した場合も対応づける。Gitが認識できない大幅変更や未追跡の移動は、追加・削除として扱われる場合がある。比較元または作業ツリーのシンボリックリンクとサブモジュールは対象外。入力ファイルとGitの状態は変更しない。

`compare`は`.suiko.toml`、`--config`、`--no-config`、`--genre`、`--experimental`に対応する。`--fail-on`は変更後の自然さと読解負荷の両方を判定し、到達すると2、入力エラーは1、それ以外は0を返す。`lint`の`--fail-on`が自然さだけを見る従来の動作は変わらない。

## 除外と修正候補

```markdown
<!-- suiko:ignore-start -->
ここに検査対象から外す他者の文章を置く。
<!-- suiko:ignore-end -->
ここから検査する。
```

コメントはそれぞれ独立した行に置く。コードや引用内のコメントは指示として実行しない。対応しない開始・終了、入れ子は`invalid_ignore_directive`として警告し、不正な範囲の本文は検査に残す。行番号とbyte位置を保つ。`stats.masking`の`ignored_regions`・`ignored_lines`・`hatena_spans`で除外量を確認できる。`diff`のコード・引用保護と資料照合は除外しない。

`[asin:…:detail]`・`[asin:…:title]`・`[f:id:…:plain]`・`[:contents]`・`[https://…:embed]`は行内でも除外する。画像のタグを除いても周囲の本文とキャプションは残す。

findingの`suggestion`は安全性を限定した局所置換、`review_suggestion`は文脈を確認して選ぶ候補。どちらも`span`・`preimage`・`replacement`を持つ。「行うことができます」から「行えます」などの可能動詞は後者であり、受身・可能・主語・時制が保たれるかを確認する。後者を前者と同じ自動置換の入力へ混ぜない。`fix --bold`もこれらの言い換え候補は適用しない。

## 太字の修正と計測

```sh
suiko fix draft.md --bold
suiko fix draft.md --bold --write
```

既定は修正済みの全文を標準出力へ返し、原文を保持する。`--write`は1ファイルを保存する。開閉が明確な`**`に限り、末尾の句読点を外へ移すか、文字に接する必要な側だけに空白を置く。コード・引用・リンク先・HTML・除外範囲は保護する。入れ子、3連以上、空白を含む曖昧な区切りは自動修正せず、残った候補をlintで確認する。

保存は同じディレクトリに一時ファイルを作り、書き込み完了後に置き換える。改行とパーミッションを保ち、原文の変化を検知した場合は中止する。リンクと読み取り専用ファイルは保存対象外。同時編集を完全に排他する機能や、OS固有のACL・拡張属性の複製はないため、それらが必要な文書はプレビューを使う。

文長は表示される本文の文字を数え、強調・リンク先・空白を除く。空白には英単語間も含み、英語の語数の指標ではない。コード等の除外は従来どおり。`stats.low_specificity.paragraphs`に段落の`line`・`score`・`threshold`・`distance`・`near_threshold`を追加した。`distance`は値−閾値、絶対値0.01以内を境界付近とする。発火しない側も記録し、境界付近では縮約だけでも結果が変わることを説明する。baselineのstatusは変更しない。

## 文体の観測

```sh
suiko style-profile past-a.md past-b.md --json > profile.json
suiko style-profile draft.md --against profile.json --json
```

指定したファイルだけを使い、同じ実ファイルは重複集計しない。`sources`にパスとSHA-256、`observations`に文数・敬体文数・平均文長・文頭2形態素・文末3形態素・内容語の分布を返す。コード等と明示した除外範囲は対象外。`--against`は平均文長と敬体率の差を返す。語彙の分布は参考値であり、著者らしさや文章の正誤を採点しない。ジャンルの異なる記事を混ぜて一律の文体ルールにしない。

## 編集判断を集計する

別リポジトリ[suiko-eval](https://github.com/nwiizo/suiko-eval)の`suiko-eval`を使う。

```sh
cd ../suiko-eval
cargo run --locked -- feedback decisions.json
```

入力例のSHA-256は、実際に判断した原稿の値へ置き換える。`id`はファイル内で一意、`line`は1以上、`category`はSuikoの規則名を指定する。

```json
{
  "schema_version": 1,
  "decisions": [{
    "id": "article-a-12-short-topic-comma",
    "file": "article-a.md",
    "source_sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    "category": "short_topic_comma",
    "line": 12,
    "decision": "retained",
    "reason": "後続の別主語との区切りを保つため"
  }]
}
```

`decision`は`accepted`（採用）・`retained`（維持）・`deferred`（保留）・`unreviewed`（未確認）・`unaddressable`（対応不能）。未確認以外は理由を必須とする。カテゴリごとに各件数、`reviewed`、`acceptance_rate`、`reviewed_fraction`を返す。採用率の分母は採用＋維持だけで、0件なら`null`。全判断件数に対する判断済みの割合も併記する。出典のパスと版は集計するが、原稿ファイルを開いてハッシュを再検証する機能ではない。

自由記述の作業メモや本文差分から採否を推測しない。維持した表現がすべて誤検出とは限らず、この割合をそのまま適合率として使わない。
