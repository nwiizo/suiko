use assert_cmd::cargo::cargo_bin_cmd;
use suiko::{diff, lint, morphology::Morphology};

#[test]
fn bold_fix_previews_then_writes_only_unambiguous_formatting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("draft.md");
    let before = "**本書の特徴は具体例です。**著者は説明します。\r\nこれは**「重要」**です。\r\n**`lazy_static`**のマクロ\r\n**創造者から評論家への転身!?**を読む。\r\n**10%**を使う。\r\n> **引用。**本文\r\n`**コード。**本文`\r\n```text\r\n**コード。**本文\r\n```\r\n<!-- **コメント。**本文 -->\r\n    **コード。**本文\r\n\\**見本。**本文\r\n****有効****\r\n";
    let after = "**本書の特徴は具体例です**。著者は説明します。\r\nこれは **「重要」** です。\r\n**`lazy_static`** のマクロ\r\n**創造者から評論家への転身**!?を読む。\r\n**10%** を使う。\r\n> **引用。**本文\r\n`**コード。**本文`\r\n```text\r\n**コード。**本文\r\n```\r\n<!-- **コメント。**本文 -->\r\n    **コード。**本文\r\n\\**見本。**本文\r\n****有効****\r\n";
    std::fs::write(&path, before).unwrap();
    cargo_bin_cmd!("suiko")
        .arg("fix")
        .arg(&path)
        .arg("--bold")
        .assert()
        .success()
        .stdout(after);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    for _ in 0..2 {
        cargo_bin_cmd!("suiko")
            .arg("fix")
            .arg(&path)
            .args(["--bold", "--write"])
            .assert()
            .success();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), after);
    }
}

#[test]
fn rejected_fix_preserves_input() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("draft.md");
    let bytes = b"\xff **body.**text";
    std::fs::write(&path, bytes).unwrap();
    cargo_bin_cmd!("suiko")
        .arg("fix")
        .arg(&path)
        .args(["--bold", "--write"])
        .assert()
        .failure();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    #[cfg(unix)]
    {
        let link = dir.path().join("link.md");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        cargo_bin_cmd!("suiko")
            .arg("fix")
            .arg(&link)
            .args(["--bold", "--write"])
            .assert()
            .failure();
        assert!(link.is_symlink());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn formatting_does_not_change_sentence_functions_or_contrasts() {
    let morphology = Morphology::new().unwrap();
    for (before, after) in [
        (
            "**これは必要でしょう。**次を説明します。",
            "**これは必要でしょう**。次を説明します。",
        ),
        (
            "**インシデント対応の教訓は、誰のものなのか。**",
            "**インシデント対応の教訓は誰のものなのか**。",
        ),
        (
            "重要なのは、**速度**ではなく**安全性**だ。",
            "重要なのは速度ではなく安全性だ。",
        ),
        ("**「設定」**は、削除しない。", "「**設定**」は削除しない。"),
    ] {
        let report = diff::compare(before, after, &morphology).unwrap();
        assert!(
            report.findings.is_empty(),
            "{before} -> {after}: {:?}",
            report.findings
        );
    }
    for (before, after, category) in [
        ("値は`a b`です。", "値は`ab`です。", "identifier_changed"),
        (
            "**「許可する」**と説明した。",
            "「**許可しない**」と説明した。",
            "quotation_changed",
        ),
    ] {
        let report = diff::compare(before, after, &morphology).unwrap();
        assert!(
            report.findings.iter().any(|f| f.category == category),
            "{:?}",
            report.findings
        );
    }
}

#[test]
fn lint_sentence_boundaries_do_not_depend_on_emphasis_position() {
    let morphology = Morphology::new().unwrap();
    let counts = [
        "**これは必要でしょう。**著者は、保存します。",
        "**これは必要でしょう**。著者は、保存します。",
    ]
    .map(|text| {
        lint::analyze(text, &morphology, Some("tech"), false)
            .unwrap()
            .findings
            .into_iter()
            .filter(|f| f.category == "short_topic_comma")
            .count()
    });
    assert_eq!(counts, [1, 1]);
}

#[test]
fn specificity_exposes_both_sides_of_a_near_threshold() {
    let morphology = Morphology::new().unwrap();
    let text = "構造と価値の改善には課題があります。状況を分析して問題を考えます。機能と情報を確認して必要な作業を進めます。".repeat(3);
    let initial = lint::analyze(&text, &morphology, Some("tech"), true).unwrap();
    let score = initial.stats.measurements["low_specificity"]["paragraphs"][0]["score"]
        .as_f64()
        .unwrap();
    for delta in [-0.005, 0.005] {
        let threshold = score + delta;
        let report = lint::analyze_with_thresholds(
            &text,
            &morphology,
            Some("tech"),
            true,
            lint::AnalysisThresholds {
                low_specificity: threshold,
                ..Default::default()
            },
        )
        .unwrap();
        let measurement = &report.stats.measurements["low_specificity"]["paragraphs"][0];
        assert_eq!(measurement["near_threshold"], true);
        assert_eq!(measurement["threshold"], threshold);
        let findings = report
            .findings
            .iter()
            .filter(|f| f.category == "low_specificity")
            .collect::<Vec<_>>();
        assert_eq!(findings.len(), usize::from(delta > 0.0));
        if let Some(finding) = findings.first() {
            assert!(finding.detail.contains("境界付近"));
        }
    }
}

#[test]
fn bold_fix_preserves_delimiters_inside_links_quotes_and_escaped_examples() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("draft.md");
    for (before, after) in [
        ("**題名。**「引用」です。", "**題名。**「引用」です。"),
        (
            "特に、**「題名」**を読む。**2%**を使う。",
            "特に、**「題名」** を読む。**2%** を使う。",
        ),
        (
            "[外部](https://example.com/**名前。**続き)",
            "[外部](https://example.com/**名前。**続き)",
        ),
        (
            "「**名前。**続き」と書いた。",
            "「**名前。**続き」と書いた。",
        ),
        ("**1. 項目：**説明", "**1. 項目**：説明"),
        (
            "```**名前。**続き```というコード。",
            "```**名前。**続き```というコード。",
        ),
        ("値は**©**です。", "値は **©** です。"),
        (
            "<!-- suiko:ignore-start -->\n**名前。**続き\n<!-- suiko:ignore-end -->",
            "<!-- suiko:ignore-start -->\n**名前。**続き\n<!-- suiko:ignore-end -->",
        ),
    ] {
        std::fs::write(&path, before).unwrap();
        cargo_bin_cmd!("suiko")
            .arg("fix")
            .arg(&path)
            .arg("--bold")
            .assert()
            .success()
            .stdout(after);
    }
}

#[test]
fn visible_sentence_length_ignores_markup_and_spacing() {
    let morphology = Morphology::new().unwrap();
    let body = "あ".repeat(89);
    for text in [
        format!("{body}。"),
        format!("**{body}** 。"),
        format!("[{body}](https://example.com/long-url)。"),
    ] {
        let report = lint::analyze_reading_load(&text, &morphology, Some("tech")).unwrap();
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.category == "sentence_too_long"),
            "{text}: {:?}",
            report.findings
        );
    }
    let report =
        lint::analyze_reading_load(&format!("{}。", "あ".repeat(91)), &morphology, Some("tech"))
            .unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.category == "sentence_too_long")
    );
}

#[test]
fn comma_before_bold_and_long_colon_introductions_are_stable() {
    let morphology = Morphology::new().unwrap();
    let text = "あとは、**[「技術力が高い」という幻覚](https://example.com)** を読んで、考えます。";
    let report = lint::analyze(text, &morphology, Some("tech"), false).unwrap();
    assert!(
        !report
            .findings
            .iter()
            .any(|f| f.category == "short_topic_comma"),
        "{:?}",
        report.findings
    );
    for comma in ["、", ""] {
        let text = format!(
            "以下は{comma}WasmフィルターをデプロイするためのWasmPluginリソースの例です：\n```yaml\nkey: value\n```\n"
        );
        let report = lint::analyze(&text, &morphology, Some("tech"), false).unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.category == "predicate_colon_lead"),
            "{:?}",
            report.findings
        );
    }
}

#[test]
fn empty_previews_and_duplicate_commas_have_specific_diagnostics() {
    let morphology = Morphology::new().unwrap();
    for (text, severity) in [
        ("重要なのは以下の点です。", "warn"),
        ("重要なのは、速度ではなく安全性だ。", "info"),
    ] {
        let report = lint::analyze(text, &morphology, Some("tech"), false).unwrap();
        let finding = report
            .findings
            .iter()
            .find(|f| f.category == "forbidden_phrase")
            .unwrap();
        assert_eq!(finding.severity, severity, "{text}");
    }
    let text = "著者は、、設定を保存します。\n「、、」という表記と`、、`は見本です。\n> 著者は、、設定を保存します。\n";
    let report = lint::analyze(text, &morphology, Some("tech"), false).unwrap();
    let found = report
        .findings
        .iter()
        .filter(|f| f.category == "duplicate_comma")
        .collect::<Vec<_>>();
    assert_eq!(found.len(), 1, "{:?}", report.findings);
    let span = found[0].span.unwrap();
    assert_eq!(&text[span.start_byte..span.end_byte], "、、");
    let report = diff::compare(
        "著者は、設定を保存します。",
        "著者は、、設定を保存します。",
        &morphology,
    )
    .unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.category == "duplicate_comma_added")
    );
    for (before, after) in [
        (
            "「導入です。著者は、保存します」と書いた。",
            "「導入です。著者は、、保存します」と書いた。",
        ),
        (
            "「導入です。\n著者は、保存します\n終わりです」と書いた。",
            "「導入です。\n著者は、、保存します\n終わりです」と書いた。",
        ),
    ] {
        let report = diff::compare(before, after, &morphology).unwrap();
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.category == "duplicate_comma_added"),
            "{:?}",
            report.findings
        );
        let linted = lint::analyze(after, &morphology, Some("tech"), false).unwrap();
        assert!(
            !linted
                .findings
                .iter()
                .any(|f| f.category == "duplicate_comma")
        );
    }
}
