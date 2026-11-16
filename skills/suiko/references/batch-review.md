# 文書群の検査

最初に`suiko capabilities --json`を読み、使えるコマンドとオプションを確認する。未対応版だけ必要な`--help`を調べる。新しい機能がない場合は、未実施の範囲を明示して使える検査を続ける。

```sh
suiko lint article-a.md article-b.md --genre tech --summary json
suiko compare --rev HEAD article-a.md article-b.md --genre tech --summary tsv
suiko style-profile past-a.md past-b.md --json
suiko-eval feedback decisions.json
```

`lint --summary`はファイル・カテゴリ別に自然さと読解負荷を分ける。個々の候補を読むときは従来の`--json`を使う。`compare`はGitの指定した版と作業ツリーを同じ実行ファイル・設定で検査し、リネーム・追加・削除を区別する。件数の減少を品質の向上とせず、意味の変化は`diff`・`check`と原文の読み合わせで確認する。

文体分析を依頼された場合だけ、指定された過去記事を`style-profile`へ渡す。出典のパスとSHA-256、文頭・文末・語彙・敬体率は観測値であり、違いを訂正理由にしない。エッセイの語りを技術手順へ一律に適用しない。

明示した他者の文章は、独立した行の`<!-- suiko:ignore-start -->`と`<!-- suiko:ignore-end -->`でlintと読解負荷から外せる。除外を指摘を減らす手段にしない。不正な対応は警告され、コード内の例示コメントは実行されない。`stats.masking`の除外量を確認する。diffのコード・引用保護はこの指定で無効にならない。

編集判断を評価へ戻す依頼では、`schema_version: 1`のJSONの`decisions`配列に、各判断の`id`・`file`・`source_sha256`・`category`・`line`・`decision`・`reason`を記録する。`decision`は`accepted`・`retained`・`deferred`・`unreviewed`・`unaddressable`。未確認以外には理由を付ける。`suiko-eval feedback`は別リポジトリ（nwiizo/suiko-eval）のCLIで、suikoの配布物には含まれない。

採用率の分母は採用＋維持で、保留・未確認・対応不能を混ぜない。判断済みの割合も読む。本文の差分がないことから不採用と推測せず、維持した指摘をすべて誤検出と呼ばない。通常の推敲で全指摘の台帳作成を追加の義務にしない。
