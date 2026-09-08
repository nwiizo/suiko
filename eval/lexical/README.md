# Lexical audit calibration fixtures

`novel_compound`を既定lintへ混ぜず、独立`lexical-audit`のinfo候補として採用するための決定的な小標本である。`../../data/lexical-reference-v1.json`を固定参照資源とし、各文書を独立に実行した。

- fire: 10文書中10文書で`novel_compound`を検出
- silent: 10文書中0文書でfindingを検出
- silent側FPR: 0.000、Wilson 95%上限0.278

これは実運用全体の性能値ではない。`eval/annotation-guide.md`の新カテゴリinfo採用に必要な最小標本とWilson上限を満たす回帰境界であり、warnへの昇格根拠には使わない。意味的な漢語・和語の揺れはこの標本から推測せず、参照JSONの`register_sets`に明示した集合だけを検査する。
