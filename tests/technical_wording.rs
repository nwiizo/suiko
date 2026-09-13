use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use suiko::{lint, morphology::Morphology};

#[test]
fn technical_wording_detects_context_and_inflections() {
    let morphology = Morphology::new().expect("initialize morphology");
    for (body, category, excerpt) in [
        (
            "設定が静かに壊れます。",
            "technical_jargon_metaphor",
            "静かに壊れ",
        ),
        (
            "データを黙って捨てられた。",
            "technical_jargon_metaphor",
            "黙って捨て",
        ),
        (
            "入力は黙って無視される。",
            "technical_jargon_metaphor",
            "黙って無視",
        ),
        (
            "この設定は地味に効いてくる。",
            "technical_jargon_metaphor",
            "地味に効い",
        ),
        (
            "調査に時間を溶かしました。",
            "technical_jargon_metaphor",
            "時間を溶かし",
        ),
        (
            "設定を安全側に倒した。",
            "technical_jargon_metaphor",
            "安全側に倒し",
        ),
        (
            "実装を保守側に倒す。",
            "technical_jargon_metaphor",
            "保守側に倒す",
        ),
        (
            "この資料は設計の入口として使える。",
            "abstract_metaphor",
            "設計の入口",
        ),
        (
            "運用の主役は担当者です。",
            "abstract_metaphor",
            "運用の主役",
        ),
    ] {
        // --nocaptureで、辞書更新や候補追加時に実際の分割・基本形・品詞を確認できる。
        let tokens = morphology.tokenize(body).unwrap();
        eprintln!("{body}");
        for token in &tokens {
            eprintln!(
                "{}\t{}\t{} / {} / {} / {}",
                token.surface,
                token.dictionary_form(),
                token.pos(0),
                token.pos(1),
                token.pos(2),
                token.pos(5)
            );
        }
        let report = lint::analyze(body, &morphology, Some("tech"), true).unwrap();
        let findings = report
            .findings
            .iter()
            .filter(|f| f.category == category)
            .collect::<Vec<_>>();
        assert_eq!(findings.len(), 1, "{body}: {:?}", tokens);
        let finding = findings[0];
        assert!(finding.excerpt.contains(excerpt), "{finding:?}");
        assert_eq!(finding.severity, "info");
        assert!(finding.suggestion.is_none());
        let span = finding.span.expect("source span");
        assert_eq!(&body[span.start_byte..span.end_byte], finding.excerpt);
        assert_eq!(
            span.start_column,
            body[..span.start_byte].chars().count() + 1
        );

        for (genre, experimental) in [
            (Some("tech"), false),
            (Some("essay"), true),
            (Some("business"), true),
            (None, true),
        ] {
            let report = lint::analyze(body, &morphology, genre, experimental).unwrap();
            assert!(
                report.findings.iter().all(|f| f.category != category),
                "{body}: {genre:?}, {experimental}"
            );
        }
    }
}

#[test]
fn wording_preserves_literal_technical_and_separate_clause_uses() {
    let morphology = Morphology::new().expect("initialize morphology");
    for body in [
        "薬が地味に効いてきた。",
        "機械が静かに動く。",
        "彼は黙って紙を捨てた。",
        "設定を確認した人が黙って帰る。",
        "データを確認した。静かに壊れる。",
        "設定を確認したが、薬は地味に効く。",
        "設定を確認した人が黙って捨てる。",
        "設定の説明は地味に効く。",
        "無視リストの設定を更新する。",
        "調査の時間を確保する。",
        "氷を溶かした。",
        "椅子を右側に倒す。",
        "会場の入口で待つ。",
        "映画の主役を紹介する。",
        "設計図に入口を描く。",
        "設定の既定値を実測結果と照合した。",
        "原因を切り分けて事故を防ぐ。",
    ] {
        let report = lint::analyze(body, &morphology, Some("tech"), true).unwrap();
        assert!(
            report.findings.iter().all(|f| !matches!(
                f.category.as_str(),
                "technical_jargon_metaphor" | "abstract_metaphor"
            )),
            "{body}: {:?}",
            report.findings
        );
    }
}

#[test]
fn wording_respects_markdown_masks_and_multiple_source_locations() {
    let morphology = Morphology::new().unwrap();
    let hidden = concat!(
        "---\ntitle: 設定が静かに壊れる。\n---\n",
        "# 設定が静かに壊れる\n",
        "> 設定が静かに壊れる。\n",
        "```text\n設定が静かに壊れる。\n```\n",
        "`設定が静かに壊れる`。\n",
        "<!-- 設定が静かに壊れる。 -->\n",
        "[手順](https://example.com/設定が静かに壊れる)\n",
        "| 設定が静かに壊れる |\n",
        "# 参考文献\n設定が静かに壊れる。\n",
    );
    let report = lint::analyze(hidden, &morphology, Some("tech"), true).unwrap();
    assert!(
        report
            .findings
            .iter()
            .all(|f| f.category != "technical_jargon_metaphor")
    );

    let body = "前置き。設定が静かに壊れる。データは黙って捨てられる。\n- この設定は地味に効く。\n";
    let report = lint::analyze(body, &morphology, Some("tech"), true).unwrap();
    let findings = report
        .findings
        .iter()
        .filter(|f| f.category == "technical_jargon_metaphor")
        .collect::<Vec<_>>();
    // 既存の散文マスクは箇条書きを除く。
    assert_eq!(findings.len(), 2);
    for finding in findings {
        let span = finding.span.unwrap();
        assert_eq!(
            &body.lines().nth(finding.line - 1).unwrap()[span.start_byte..span.end_byte],
            finding.excerpt
        );
    }
}

#[test]
fn distinction_repetition_requires_three_assertions() {
    let morphology = Morphology::new().unwrap();
    let body = "型と値は別物です。\n\n認証と認可は別物だ。\n\n保存と同期は別物である。\n";
    let report = lint::analyze(body, &morphology, Some("tech"), true).unwrap();
    let findings = report
        .findings
        .iter()
        .filter(|f| f.category == "repeated_distinction")
        .collect::<Vec<_>>();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].related_lines, Some(vec![1, 3, 5]));
    assert_eq!(findings[0].severity, "info");
    assert!(findings[0].suggestion.is_none());
    let span = findings[0].span.unwrap();
    assert_eq!(
        &body.lines().next().unwrap()[span.start_byte..span.end_byte],
        findings[0].excerpt
    );
    for text in [
        "型と値は別物です。認証と認可は別物だ。",
        "型と値は別物ですか。認証と認可は別物ですか。保存と同期は別物ですか。",
        "別物ではない。別物ではありません。別物とは思わない。",
        "別物ですと説明した。別物だと聞いた。別物であると書かれた。",
        "別物の資料だ。別物の画面だ。別物のデータだ。",
        "> 型と値は別物です。\n> 認証と認可は別物だ。\n> 保存と同期は別物である。",
    ] {
        let report = lint::analyze(text, &morphology, Some("tech"), true).unwrap();
        assert!(
            report
                .findings
                .iter()
                .all(|f| f.category != "repeated_distinction"),
            "{text}"
        );
    }
    for (genre, experimental) in [
        (Some("essay"), true),
        (Some("business"), true),
        (None, true),
    ] {
        let report = lint::analyze(body, &morphology, genre, experimental).unwrap();
        assert!(
            report
                .findings
                .iter()
                .all(|f| f.category != "repeated_distinction")
        );
    }
}

#[test]
fn em_dash_repetition_counts_sentences_and_ignores_other_dashes() {
    let morphology = Morphology::new().unwrap();
    let body = "設定—接続先を指定する。\n\n検証――結果を確かめる。\n\n保存—変更を書き込む。\n";
    let report = lint::analyze(body, &morphology, Some("tech"), true).unwrap();
    let findings = report
        .findings
        .iter()
        .filter(|f| f.category == "repeated_em_dash")
        .collect::<Vec<_>>();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].related_lines, Some(vec![1, 3, 5]));
    assert!(findings[0].suggestion.is_none());
    let span = findings[0].span.unwrap();
    assert_eq!(
        &body.lines().next().unwrap()[span.start_byte..span.end_byte],
        "—"
    );
    for body in [
        "設定—接続先—を指定する。検証—結果—を確かめる。",
        "範囲は1—3です。範囲は４―６です。範囲は7—9です。",
        "コマンドは--helpです。サーバーを起動する。A–Bを比較する。",
        "—\n――\n——\n",
        "`設定—接続先`。\n> 検証—結果。\n```\n保存—変更。\n```",
        "# 設定—接続先\n# 検証—結果\n# 保存—変更\n",
    ] {
        let report = lint::analyze(body, &morphology, Some("tech"), true).unwrap();
        assert!(
            report
                .findings
                .iter()
                .all(|f| f.category != "repeated_em_dash"),
            "{body}"
        );
    }
    for (genre, experimental) in [
        (Some("essay"), true),
        (Some("business"), true),
        (None, true),
    ] {
        let report = lint::analyze(body, &morphology, genre, experimental).unwrap();
        assert!(
            report
                .findings
                .iter()
                .all(|f| f.category != "repeated_em_dash")
        );
    }
}

#[test]
fn wording_categories_support_cli_gate_and_config() {
    let body = "型と値は別物です。認証と認可は別物だ。保存と同期は別物である。\n";
    let temp = tempfile::tempdir().unwrap();
    cargo_bin_cmd!("suiko")
        .current_dir(temp.path())
        .args([
            "lint",
            "-",
            "--genre",
            "tech",
            "--fail-on",
            "info",
            "--json",
        ])
        .write_stdin(body)
        .assert()
        .code(2);
    std::fs::write(
        temp.path().join(".suiko.toml"),
        "version = 1\ndisabled_rules = [\"repeated_distinction\", \"repeated_em_dash\"]\n",
    )
    .unwrap();
    let output = cargo_bin_cmd!("suiko")
        .current_dir(temp.path())
        .args(["lint", "-", "--genre", "tech", "--json"])
        .write_stdin(body)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        json["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["category"] != "repeated_distinction")
    );
}

#[test]
fn stable_repetition_points_to_local_patterns_including_emphasis() {
    let morphology = Morphology::new().unwrap();
    for (category, body) in [
        (
            "repeated_distinction",
            "型と値は別物です。認証と認可は別物だ。保存と同期は別物である。",
        ),
        (
            "repeated_distinction",
            "型と値は**別物です**。認証と認可は**別物**だ。保存と同期は別物である。",
        ),
        (
            "repeated_distinction",
            "型と値は別物です。型は値の種類を表す。認証と認可は別物だ。確認する対象が異なる。保存と同期は別物である。",
        ),
        (
            "repeated_em_dash",
            "設定—接続先を指定する。検証――結果を確かめる。保存—変更を書き込む。",
        ),
        (
            "repeated_em_dash",
            "**設定**—接続先を指定する。検証――**結果**を確かめる。保存—変更を書き込む。",
        ),
        (
            "repeated_em_dash",
            "設定—接続先を指定する。まず接続を試す。検証――結果を確かめる。応答を待つ。保存—変更を書き込む。",
        ),
    ] {
        for experimental in [false, true] {
            let report = lint::analyze(body, &morphology, Some("tech"), experimental).unwrap();
            let findings = report
                .findings
                .iter()
                .filter(|f| f.category == category)
                .collect::<Vec<_>>();
            assert_eq!(
                findings.len(),
                1,
                "{category}: {body}, experimental={experimental}"
            );
            assert_eq!(findings[0].severity, "info");
            assert!(findings[0].suggestion.is_none());
        }
    }
}

#[test]
fn repetition_does_not_accumulate_distant_explanations_or_sections() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "型と値は別物です。型は種類を示す。値には実体がある。認証と認可は別物だ。確認対象が異なる。保存と同期は別物である。",
        "設定—接続先を指定する。まず接続を試す。応答を待つ。検証—結果を確かめる。内容を読む。保存—変更を書き込む。",
        "# 型\n型と値は別物です。\n# 権限\n認証と認可は別物だ。\n# 保存\n保存と同期は別物である。",
        "# 設定\n設定—接続先を指定する。\n# 検証\n検証—結果を確かめる。\n# 保存\n保存—変更を書き込む。",
    ] {
        let report = lint::analyze(body, &morphology, Some("tech"), true).unwrap();
        assert!(
            report.findings.iter().all(|f| !matches!(
                f.category.as_str(),
                "repeated_distinction" | "repeated_em_dash"
            )),
            "{body}"
        );
    }
}

#[test]
fn repetition_reports_only_clustered_lines_and_ignores_headings_in_code() {
    let morphology = Morphology::new().unwrap();
    let body = "単発—ここだけ補足する。\n説明する。\nもう一度説明する。\n別の話をする。\n\n# 手順\n設定—接続先を指定する。\n```sh\n# この行は見出しではない\n```\n検証—結果を確かめる。\n保存—変更を書き込む。\n";
    let report = lint::analyze(body, &morphology, Some("tech"), false).unwrap();
    let finding = report
        .findings
        .iter()
        .find(|f| f.category == "repeated_em_dash")
        .expect("local repetition");
    assert_eq!(finding.line, 7);
    assert_eq!(finding.related_lines, Some(vec![7, 11, 12]));
}

#[test]
fn repetition_baselines_track_the_pattern_when_the_first_excerpt_changes() {
    let morphology = Morphology::new().unwrap();
    let before = "型と値は別物です。認証と認可は別物だ。保存と同期は別物である。\n設定—接続先。検証—結果。保存—変更。";
    let after = "型と値は別物である。認証と認可は別物です。保存と同期は別物だ。\n設定――接続先。検証――結果。保存――変更。";
    let baseline =
        serde_json::to_value(lint::analyze(before, &morphology, Some("tech"), true).unwrap())
            .unwrap();
    let mut report = lint::analyze(after, &morphology, Some("tech"), true).unwrap();
    lint::apply_baseline(&mut report.findings, &baseline, "draft.md".to_owned()).unwrap();
    for category in ["repeated_distinction", "repeated_em_dash"] {
        let finding = report
            .findings
            .iter()
            .find(|f| f.category == category)
            .unwrap();
        assert_eq!(finding.status.as_deref(), Some("persisting"), "{category}");
    }
}
