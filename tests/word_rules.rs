use std::fs;

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use suiko::{lint, morphology::Morphology};
use tempfile::tempdir;

const VALUE_RULE: &str = r#"
[[word_rules]]
id = "value-change"
message = "増減・変更・移動のどれを指すか確認してください。"
tokens = [
    { surface = "値", pos = "名詞" },
    { surface = "を", pos = "助詞" },
    { dictionary_form = "動かす", pos = "動詞" },
]
"#;

#[test]
fn word_rules_match_inflections_and_preserve_source_ranges() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".suiko.toml"),
        format!("version = 1\n{VALUE_RULE}"),
    )
    .unwrap();
    let body = "😀 前提を確認した。値を動かしました。値を動かさない。\n値札を動かした。サーバーを動かした。値を少し動かした。\n値を`計測して`動かした。\n値を\n動かした。\n値を 動かした。値を。動かした。\n# 値を動かした。\n> 値を動かした。\n- 値を動かした。\n```text\n値を動かした。\n```\n";
    fs::write(dir.path().join("draft.md"), body).unwrap();
    let output = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["lint", "draft.md", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&output).unwrap();
    let findings = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["category"] == "custom_wording/value-change")
        .collect::<Vec<_>>();
    assert_eq!(findings.len(), 2, "{report}");
    assert_eq!(
        report["stats"]["by_category"]["custom_wording/value-change"],
        2
    );
    assert_eq!(
        findings
            .iter()
            .map(|f| f["excerpt"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["値を動かし", "値を動かさ"]
    );
    for finding in findings {
        assert!(finding.get("rule_id").is_none());
        assert_eq!(finding["severity"], "info");
        assert!(finding.get("suggestion").is_none());
        let span = &finding["span"];
        assert_eq!(span["start_line"], 1);
        let start = span["start_byte"].as_u64().unwrap() as usize;
        let end = span["end_byte"].as_u64().unwrap() as usize;
        assert_eq!(&body[start..end], finding["excerpt"].as_str().unwrap());
        assert_eq!(
            span["start_column"].as_u64().unwrap() as usize,
            body[..start].chars().count() + 1
        );
    }
}

#[test]
fn word_rules_obey_settings_and_the_severity_gate() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("draft.md"), "値を動かしました。\n").unwrap();
    for (settings, rule, args, expected_count, exit_code) in [
        ("", VALUE_RULE.to_owned(), vec!["--fail-on", "warn"], 1, 0),
        (
            "",
            VALUE_RULE.replace("id =", "severity = \"warn\"\nid ="),
            vec!["--fail-on", "warn"],
            1,
            2,
        ),
        ("", VALUE_RULE.to_owned(), vec!["--fail-on", "info"], 1, 2),
        (
            "disabled_rules = [\"custom_wording\"]\n",
            VALUE_RULE.to_owned(),
            vec!["--fail-on", "info"],
            0,
            0,
        ),
        (
            "[[allow]]\ncategory = \"custom_wording\"\ntext = \"値を動かし\"\nreason = \"位置の移動を説明した\"\n",
            VALUE_RULE.to_owned(),
            vec!["--fail-on", "info"],
            0,
            0,
        ),
        ("", VALUE_RULE.to_owned(), vec!["--no-config"], 0, 0),
    ] {
        fs::write(
            dir.path().join(".suiko.toml"),
            format!("version = 1\n{settings}{rule}"),
        )
        .unwrap();
        let output = cargo_bin_cmd!("suiko")
            .current_dir(dir.path())
            .args(["lint", "draft.md", "--json"])
            .args(&args)
            .assert()
            .code(exit_code)
            .get_output()
            .stdout
            .clone();
        let report: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(
            report["findings"].as_array().unwrap().len(),
            expected_count,
            "{report}"
        );
        assert_eq!(report["stats"]["total_findings"], expected_count);
    }
}

#[test]
fn word_rule_identity_keeps_baseline_results_separate() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("draft.md"), "値を動かしました。\n").unwrap();
    let other_rule = VALUE_RULE.replace("value-change", "value-position");
    fs::write(
        dir.path().join(".suiko.toml"),
        format!("version = 1\n{VALUE_RULE}{other_rule}"),
    )
    .unwrap();
    let before = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["lint", "draft.md", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    fs::write(dir.path().join("before.json"), before).unwrap();
    fs::write(
        dir.path().join(".suiko.toml"),
        format!("version = 1\n{other_rule}"),
    )
    .unwrap();
    let after = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["lint", "draft.md", "--baseline", "before.json", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&after).unwrap();
    assert_eq!(
        report["baseline"]["summary"],
        serde_json::json!({"resolved": 1, "new": 0, "persisting": 1})
    );
    assert_eq!(
        report["baseline"]["resolved"][0]["category"],
        "custom_wording/value-change"
    );
    assert_eq!(
        report["findings"][0]["category"],
        "custom_wording/value-position"
    );
    assert_eq!(report["findings"][0]["status"], "persisting");
}

#[test]
fn word_rules_reject_invalid_configuration() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("draft.md"), "値を動かしました。").unwrap();
    for (rule, expected_error) in [
        (format!("{VALUE_RULE}{VALUE_RULE}"), "重複"),
        (VALUE_RULE.replace("value-change", ""), "id"),
        (
            VALUE_RULE.replace("増減・変更・移動のどれを指すか確認してください。", " "),
            "message",
        ),
        (
            VALUE_RULE.replace("{ surface = \"値\", pos = \"名詞\" }", "{}"),
            "条件",
        ),
        (
            VALUE_RULE.replace("surface = \"値\"", "surface = \" \""),
            "surface",
        ),
        (
            VALUE_RULE.replace("dictionary_form", "basic_form"),
            "basic_form",
        ),
        (
            VALUE_RULE.replace("pos = \"動詞\"", "pos = \"動詩\""),
            "pos",
        ),
        (
            VALUE_RULE.replace("id =", "severity = \"warning\"\nid ="),
            "severity",
        ),
        (
            "[[word_rules]]\nid = \"empty\"\nmessage = \"確認\"\ntokens = []\n".to_owned(),
            "tokens",
        ),
    ] {
        fs::write(
            dir.path().join(".suiko.toml"),
            format!("version = 1\n{rule}"),
        )
        .unwrap();
        let output = cargo_bin_cmd!("suiko")
            .current_dir(dir.path())
            .args(["lint", "draft.md", "--json"])
            .assert()
            .code(1)
            .get_output()
            .clone();
        assert!(output.stdout.is_empty());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains(expected_error), "{expected_error}: {error}");
    }
}

#[test]
fn short_topic_comma_is_an_opt_in_review_hint() {
    let morphology = Morphology::new().unwrap();
    for (body, expected) in [
        ("要点は、三つです。", 1),
        ("この値は、固定です。", 1),
        ("入力形式は、JSONです。", 1),
        (
            "前提を確認した。😀は、表示例です。要点は、後で述べます。",
            1,
        ),
        ("条件は、前述のとおりです。結果は、未確定です。", 2),
        ("前提を確認した\n要点は、後で述べます。", 1),
    ] {
        let report = lint::analyze(body, &morphology, Some("tech"), true).unwrap();
        let findings = report
            .findings
            .iter()
            .filter(|f| f.category == "short_topic_comma")
            .collect::<Vec<_>>();
        assert_eq!(
            findings.len(),
            expected,
            "{body}: {:?}",
            morphology.tokenize(body).unwrap()
        );
        for finding in findings {
            let span = finding.span.unwrap();
            let line = body.lines().nth(span.start_line - 1).unwrap();
            assert_eq!(&line[span.start_byte..span.end_byte], "、");
            assert_eq!(
                span.start_column,
                line[..span.start_byte].chars().count() + 1
            );
            assert_eq!(finding.severity, "info");
            assert!(finding.suggestion.is_none());
            assert!(finding.excerpt.contains("は、"));
        }
        assert!(
            lint::analyze(body, &morphology, Some("tech"), false)
                .unwrap()
                .findings
                .iter()
                .all(|f| f.category != "short_topic_comma")
        );
    }
}

#[test]
fn short_topic_comma_preserves_other_boundaries_and_markdown() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "入力ファイルは、JSONです。",
        "読むのは、明日にします。",
        "こんにちは、皆さん。",
        "そこで、条件を変えた。",
        "雨では、実行できません。",
        "要点は既に説明しました。",
        "「要点は、三つです」と話した。",
        "`要点`は、三つです。",
        "要点は`注釈`、三つです。",
        "# 要点は、三つです。\n> 条件は、二つです。\n- 結果は、未確定です。\n",
        "```text\n要点は、三つです。\n```\n",
    ] {
        let report = lint::analyze(body, &morphology, Some("tech"), true).unwrap();
        assert!(
            report
                .findings
                .iter()
                .all(|f| f.category != "short_topic_comma"),
            "{body}"
        );
    }
}

#[test]
fn word_rules_use_all_conditions_and_support_a_single_condition() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("draft.md"),
        "`前置き`値を動かしました。値を動かさない。\n",
    )
    .unwrap();
    for (condition, expected) in [
        (
            r#"{ dictionary_form = "動かす" }"#,
            vec!["動かし", "動かさ"],
        ),
        (r#"{ surface = "動かし" }"#, vec!["動かし"]),
        (r#"{ dictionary_form = "動かす", pos = "名詞" }"#, vec![]),
        (
            r#"{ dictionary_form = "動かす", surface = "動かし", pos = "動詞" }"#,
            vec!["動かし"],
        ),
    ] {
        fs::write(dir.path().join(".suiko.toml"), format!("version = 1\n[[word_rules]]\nid = \"motion\"\nmessage = \"動作を確認する\"\ntokens = [{condition}]\n")).unwrap();
        let output = cargo_bin_cmd!("suiko")
            .current_dir(dir.path())
            .args(["lint", "draft.md", "--json"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let report: Value = serde_json::from_slice(&output).unwrap();
        let findings = report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["category"] == "custom_wording/motion")
            .collect::<Vec<_>>();
        assert_eq!(
            findings
                .iter()
                .map(|f| f["excerpt"].as_str().unwrap())
                .collect::<Vec<_>>(),
            expected
        );
        for finding in findings {
            let body = fs::read_to_string(dir.path().join("draft.md")).unwrap();
            let span = &finding["span"];
            assert_eq!(
                &body[span["start_byte"].as_u64().unwrap() as usize
                    ..span["end_byte"].as_u64().unwrap() as usize],
                finding["excerpt"].as_str().unwrap()
            );
        }
    }
}

#[test]
fn word_rules_flow_through_multi_file_json_and_ci_formats() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".suiko.toml"),
        format!("version = 1\n{VALUE_RULE}"),
    )
    .unwrap();
    fs::write(dir.path().join("first.md"), "値を動かした。\n").unwrap();
    fs::write(dir.path().join("second.md"), "値を動かさない。\n").unwrap();
    let output = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["lint", "first.md", "second.md", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let reports: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(reports.as_array().unwrap().len(), 2);
    for report in reports.as_array().unwrap() {
        assert_eq!(
            report["stats"]["by_category"]["custom_wording/value-change"],
            1
        );
        assert_eq!(
            report["findings"][0]["category"],
            "custom_wording/value-change"
        );
    }
    let github = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args([
            "lint",
            "first.md",
            "--format",
            "github",
            "--fail-on",
            "info",
        ])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    assert!(String::from_utf8(github).unwrap().contains(
        "::notice file=first.md,line=1,endLine=1,col=1,endColumn=6,title=suiko custom_wording/value-change::"
    ));
    let sarif = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["lint", "first.md", "--format", "sarif"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&sarif).unwrap();
    assert_eq!(
        report["runs"][0]["results"][0]["ruleId"],
        "custom_wording/value-change"
    );
    assert_eq!(report["runs"][0]["results"][0]["level"], "note");
}

#[test]
fn short_topic_comma_obeys_allow_disable_and_baseline() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("draft.md"), "要点は、三つです。\n").unwrap();
    let before = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args([
            "lint",
            "draft.md",
            "--experimental",
            "--json",
            "--fail-on",
            "info",
        ])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    fs::write(dir.path().join("before.json"), before).unwrap();
    for setting in [
        "disabled_rules = [\"short_topic_comma\"]",
        "[[allow]]\ncategory = \"short_topic_comma\"\ntext = \"要点は、\"\nreason = \"意図した間を保つ\"",
    ] {
        fs::write(
            dir.path().join(".suiko.toml"),
            format!("version = 1\n{setting}\n"),
        )
        .unwrap();
        let after = cargo_bin_cmd!("suiko")
            .current_dir(dir.path())
            .args([
                "lint",
                "draft.md",
                "--experimental",
                "--json",
                "--fail-on",
                "info",
                "--baseline",
                "before.json",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let report: Value = serde_json::from_slice(&after).unwrap();
        assert_eq!(report["stats"]["total_findings"], 0);
        assert_eq!(report["baseline"]["summary"]["resolved"], 1);
    }
}

#[test]
fn allowance_can_target_one_word_rule_without_suppressing_another() {
    let dir = tempdir().unwrap();
    let other_rule = VALUE_RULE.replace("value-change", "value-position");
    fs::write(dir.path().join("draft.md"), "値を動かしました。").unwrap();
    let allowance = "[[allow]]\ncategory = \"custom_wording\"\nrule_id = \"value-change\"\ntext = \"値を動かし\"\nreason = \"変更量を説明した\"\n";
    fs::write(
        dir.path().join(".suiko.toml"),
        format!("version = 1\n{VALUE_RULE}{other_rule}{allowance}"),
    )
    .unwrap();
    let output = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["lint", "draft.md", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(
        report["stats"]["by_category"]["custom_wording/value-position"],
        1
    );
    assert_eq!(
        report["findings"][0]["category"],
        "custom_wording/value-position"
    );

    fs::write(
        dir.path().join(".suiko.toml"),
        format!(
            "version = 1\n{VALUE_RULE}{}",
            allowance.replace("rule_id = \"value-change\"", "rule_id = \"value-typo\"")
        ),
    )
    .unwrap();
    let output = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["lint", "draft.md", "--json"])
        .assert()
        .code(1)
        .get_output()
        .clone();
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("value-typo")
    );
}
