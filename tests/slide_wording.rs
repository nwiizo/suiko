use assert_cmd::cargo::cargo_bin_cmd;
use suiko::{lint, morphology::Morphology};

#[test]
fn publication_revision_resolves_both_hints_with_the_same_baseline_settings() {
    let dir = tempfile::tempdir().unwrap();
    let draft = dir.path().join("draft.md");
    std::fs::write(
        &draft,
        "判断に迷うものは、残さない側に倒します。\n前提を、経路が代わりに添える。\n",
    )
    .unwrap();
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
    let report: serde_json::Value = serde_json::from_slice(&before).unwrap();
    assert_eq!(
        report["stats"]["by_category"]["decision_direction_metaphor"],
        1
    );
    assert_eq!(report["stats"]["by_category"]["short_object_comma"], 1);
    std::fs::write(dir.path().join("before.json"), before).unwrap();
    std::fs::write(&draft, "採否を判断できない項目は除外します。\n運用者は問い合わせ先に応じた前提条件を参照します。\n").unwrap();
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
    let report: serde_json::Value = serde_json::from_slice(&after).unwrap();
    assert_eq!(report["baseline"]["summary"]["resolved"], 2);
    assert_eq!(report["baseline"]["summary"]["new"], 0);
}

#[test]
fn short_object_comma_marks_only_the_comma_without_rewriting() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "前提を、経路が代わりに添える。",
        "条件を、先に示す。",
        "この値を、保存する。",
        "入力値を、確認する。",
        "結果を、担当者に渡す。",
        "確認した。前提を、説明する。",
        // 「を」が経路を示す場合も、読点の要否は断定せず確認候補にする。
        "道を、駅まで歩く。",
        "空を、鳥が飛ぶ。",
    ] {
        let report = lint::analyze(body, &morphology, Some("tech"), true).unwrap();
        let findings: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.category == "short_object_comma")
            .collect();
        assert_eq!(
            findings.len(),
            1,
            "{body}: {:?}",
            morphology.tokenize(body).unwrap()
        );
        let finding = findings[0];
        let span = finding.span.unwrap();
        assert_eq!(&body[span.start_byte..span.end_byte], "、");
        assert_eq!(
            span.start_column,
            body[..span.start_byte].chars().count() + 1
        );
        assert_eq!(finding.severity, "info");
        assert!(finding.suggestion.is_none());
        assert!(finding.excerpt.ends_with("を、"));
        assert!(
            lint::analyze(body, &morphology, Some("tech"), false)
                .unwrap()
                .findings
                .iter()
                .all(|f| f.category != "short_object_comma")
        );
    }
}

#[test]
fn short_object_comma_preserves_clause_and_quotation_boundaries() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "入力ファイルを、確認する。",
        "動くのを、確認する。",
        "彼が来るのを、待つ。",
        "前提と条件を、確認する。",
        "前提を確認する。",
        "前提を、",
        "「前提を、確認する」と話した。",
        "`前提`を、確認する。",
        "> 前提を、確認する。\n",
        "```text\n前提を、確認する。\n```\n",
    ] {
        let report = lint::analyze(body, &morphology, Some("tech"), true).unwrap();
        assert!(
            report
                .findings
                .iter()
                .all(|f| f.category != "short_object_comma"),
            "{body}"
        );
    }
}

#[test]
fn decision_side_metaphor_uses_inflected_action_not_physical_direction() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "判断に迷うものは、残さない側に倒します。",
        "迷う項目は採用しない側に倒した。",
        "不明な権限は許可しない側に倒す。",
        "判断できなければ削除する側に倒している。",
        "不明な入力は保存しない側に倒しました。",
        "判断に迷うスタイルは、あらかじめ共通側に倒します。",
        "曖昧な値は厳格側に倒した。",
    ] {
        let report = lint::analyze(body, &morphology, Some("tech"), true).unwrap();
        let findings: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.category == "decision_direction_metaphor")
            .collect();
        assert_eq!(
            findings.len(),
            1,
            "{body}: {:?}",
            morphology.tokenize(body).unwrap()
        );
        let finding = findings[0];
        let span = finding.span.unwrap();
        assert_eq!(&body[span.start_byte..span.end_byte], finding.excerpt);
        assert_eq!(finding.severity, "info");
        assert!(finding.suggestion.is_none());
    }
    for body in [
        "椅子を右側に倒す。",
        "木を人がいない側に倒す。",
        "機器を傷つかない側に倒した。",
        "設定を確認し、倒れない側に倒す。",
        "残さない側に立つ。",
        "残さない側に、椅子を倒す。",
        "椅子を共通の側に倒す。",
        "設定を安全側に倒す。",
        "採用しない方針にする。",
        "> 判断に迷うものは、残さない側に倒します。",
        "`残さない側に倒す`。",
        "```text\n残さない側に倒す。\n```",
    ] {
        let report = lint::analyze(body, &morphology, Some("tech"), true).unwrap();
        assert!(
            report
                .findings
                .iter()
                .all(|f| f.category != "decision_direction_metaphor"),
            "{body}"
        );
    }
    for genre in [None, Some("tech"), Some("essay"), Some("business")] {
        let report = lint::analyze("残さない側に倒す。", &morphology, genre, true).unwrap();
        assert_eq!(
            report
                .findings
                .iter()
                .filter(|f| f.category == "decision_direction_metaphor")
                .count(),
            1
        );
        let report = lint::analyze("残さない側に倒す。", &morphology, genre, false).unwrap();
        assert!(
            report
                .findings
                .iter()
                .all(|f| f.category != "decision_direction_metaphor")
        );
    }
}

#[test]
fn short_object_comma_can_be_disabled_and_compared_with_baseline() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("draft.md"), "前提を、確認する。").unwrap();
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
    std::fs::write(dir.path().join("before.json"), before).unwrap();
    for setting in [
        "disabled_rules = [\"short_object_comma\"]",
        "[[allow]]\ncategory = \"short_object_comma\"\ntext = \"前提を、\"\nreason = \"意図した間\"",
    ] {
        std::fs::write(
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
        let report: serde_json::Value = serde_json::from_slice(&after).unwrap();
        assert_eq!(report["baseline"]["summary"]["resolved"], 1);
        assert_eq!(report["stats"]["total_findings"], 0);
    }
}
