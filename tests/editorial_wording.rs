use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use suiko::{lint, morphology::Morphology};

#[test]
fn mixed_spacing_reports_only_conflicting_boundaries_for_the_same_term() {
    let morphology = Morphology::new().unwrap();
    for (body, term, lines) in [
        (
            "このAPIを呼びます。\nこの API を調べます。",
            "API",
            vec![1, 2],
        ),
        ("APIを呼び、API を調べます。", "API", vec![1]),
        ("このAPI。\nこの API。", "API", vec![1, 2]),
        (
            "このAPI を呼びます。\nこの API を調べます。",
            "API",
            vec![1, 2],
        ),
        (
            "このUTF-8を使います。\nこの UTF-8 を選びます。",
            "UTF-8",
            vec![1, 2],
        ),
    ] {
        let report = lint::analyze(body, &morphology, Some("tech"), true).unwrap();
        let findings = report
            .findings
            .iter()
            .filter(|f| f.category == "mixed_latin_spacing")
            .collect::<Vec<_>>();
        assert_eq!(findings.len(), 1, "{body}");
        let finding = findings[0];
        assert_eq!(finding.related_lines.as_ref(), Some(&lines));
        assert_eq!(finding.severity, "info");
        assert!(finding.suggestion.is_none());
        let span = finding.span.unwrap();
        let line = body.lines().nth(span.start_line - 1).unwrap();
        assert_eq!(&line[span.start_byte..span.end_byte], term);
        assert_eq!(
            span.start_column,
            line[..span.start_byte].chars().count() + 1
        );
    }
}

#[test]
fn mixed_spacing_preserves_consistent_styles_and_distinct_terms() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "このAPIを使い、このAPIを調べます。",
        "この API を使い、この API を調べます。",
        "APIを使います。CLI を調べます。",
        "APIを使います。api を調べます。",
        "このAPIを使います。API\nを調べます。",
        "このAPIを使います。API　を調べます。",
        "APIを使います。Web API を調べます。",
        "APIを使います。https://example.com/API を開きます。",
        "APIを使います。path/API を開きます。",
        "容量は20GBです。容量は20GB です。",
        "変数xを使い、変数x を調べます。",
        "このAPI。API を使う。", // 左側の空白と右側の空白は別々に比べる。
        "このAPIを使います。この`code` API を調べます。",
    ] {
        let report = lint::analyze(body, &morphology, Some("tech"), true).unwrap();
        assert!(
            report
                .findings
                .iter()
                .all(|f| f.category != "mixed_latin_spacing"),
            "{body}"
        );
    }
}

#[test]
fn editorial_patterns_respect_markdown_masks() {
    let morphology = Morphology::new().unwrap();
    let body = concat!(
        "---\ntitle: この API を使います。\n---\n",
        "# この API を使います。\n",
        "このAPIを使います。\n",
        "> この API を使います。ぜひ参考にしてみてください。\n",
        "- この API を使います。ぜひ参考にしてみてください。\n",
        "```text\nこの API を使います。ぜひ参考にしてみてください。\n```\n",
        "~~~text\nこの API を使います。ぜひ参考にしてみてください。\n~~~\n",
        "`この API を使います。ぜひ参考にしてみてください。`\n",
        "<!-- この API を使います。ぜひ参考にしてみてください。 -->\n",
        "[手順](https://example.com/この API を使います。)\n",
        "| この API を使います。 |\n",
        "# 参考文献\nこの API を使います。ぜひ参考にしてみてください。\n",
    );
    let report = lint::analyze(body, &morphology, Some("tech"), true).unwrap();
    assert!(report.findings.iter().all(|f| !matches!(
        f.category.as_str(),
        "mixed_latin_spacing" | "boilerplate_closing"
    )));
}

#[test]
fn closing_detects_only_a_standalone_final_prose_sentence() {
    let morphology = Morphology::new().unwrap();
    for closing in [
        "いかがでしたでしょうか。",
        "ぜひ参考にしてみてください！",
        "ぜひ参考にしてください。",
        "ぜひ試してみてください。",
        "ぜひ活用してください。",
        "ぜひ活用してみてください",
    ] {
        let body = format!("手順を説明しました。\n  {closing}\n\n<!-- 校閲済み -->\n");
        let report = lint::analyze(&body, &morphology, Some("tech"), true).unwrap();
        let findings = report
            .findings
            .iter()
            .filter(|f| f.category == "boilerplate_closing")
            .collect::<Vec<_>>();
        assert_eq!(findings.len(), 1, "{closing}");
        let finding = findings[0];
        assert_eq!(finding.line, 2);
        assert_eq!(finding.severity, "info");
        assert!(finding.suggestion.is_none());
        let span = finding.span.unwrap();
        let line = body.lines().nth(1).unwrap();
        assert_eq!(
            &line[span.start_byte..span.end_byte],
            closing.trim_end_matches(['。', '！'])
        );
        assert_eq!(span.start_column, 3);
    }
    for body in [
        "ぜひ試してみてください。次に結果を保存します。",
        "結果の共有にぜひ活用してください。",
        "研修はいかがでしたでしょうか。",
        "「ぜひ参考にしてみてください」と書いた。",
        "いかがでしたか。", // 既存のforbidden_phraseに任せ、二重報告しない。
        "ぜひ参考にしてみてください。\n入力値を確認します。",
    ] {
        let report = lint::analyze(body, &morphology, Some("tech"), true).unwrap();
        assert!(
            report
                .findings
                .iter()
                .all(|f| f.category != "boilerplate_closing"),
            "{body}"
        );
    }
}

#[test]
fn editorial_patterns_are_opt_in_and_support_each_genre() {
    let morphology = Morphology::new().unwrap();
    let body = "このAPIを使います。この API を調べます。ぜひ参考にしてみてください。";
    for genre in [None, Some("tech"), Some("business"), Some("essay")] {
        for experimental in [false, true] {
            let report = lint::analyze(body, &morphology, genre, experimental).unwrap();
            for category in ["mixed_latin_spacing", "boilerplate_closing"] {
                assert_eq!(
                    report.findings.iter().any(|f| f.category == category),
                    experimental,
                    "{genre:?}: {category}"
                );
            }
        }
    }
}

#[test]
fn editorial_cli_supports_gate_config_and_baseline() {
    let temp = tempfile::tempdir().unwrap();
    let body = "このAPIを使います。この API を調べます。ぜひ参考にしてみてください。";
    let output = cargo_bin_cmd!("suiko")
        .current_dir(temp.path())
        .args([
            "lint",
            "-",
            "--experimental",
            "--no-reading-load",
            "--fail-on",
            "info",
            "--json",
        ])
        .write_stdin(body)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let before: Value = serde_json::from_slice(&output.stdout).unwrap();
    for category in ["mixed_latin_spacing", "boilerplate_closing"] {
        assert!(
            before["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["category"] == category)
        );
    }
    let baseline = temp.path().join("before.json");
    std::fs::write(&baseline, output.stdout).unwrap();
    let output = cargo_bin_cmd!("suiko")
        .current_dir(temp.path())
        .args([
            "lint",
            "-",
            "--experimental",
            "--no-reading-load",
            "--baseline",
        ])
        .arg(&baseline)
        .args(["--json"])
        .write_stdin("このAPIを使います。このAPIを調べます。")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let after: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(after["baseline"]["summary"]["resolved"], 2);
    std::fs::write(
        temp.path().join(".suiko.toml"),
        "version = 1\ndisabled_rules = [\"mixed_latin_spacing\", \"boilerplate_closing\"]\n",
    )
    .unwrap();
    cargo_bin_cmd!("suiko")
        .current_dir(temp.path())
        .args([
            "lint",
            "-",
            "--experimental",
            "--no-reading-load",
            "--fail-on",
            "info",
            "--json",
        ])
        .write_stdin(body)
        .assert()
        .success();
}
