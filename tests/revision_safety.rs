use suiko::{diff, lint, morphology::Morphology};

#[test]
fn actual_questions_are_not_treated_as_empty_closing_phrases() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "先日お送りした見積もりはいかがでしたか。",
        "研修は、いかがでしたか。",
        "修正案について、いかがでしょうか。",
        "まずは小さなサービスを一つ選び、試してみてはいかがでしょうか。",
    ] {
        let report = lint::analyze(body, &morphology, Some("business"), true).unwrap();
        assert!(
            report.findings.iter().all(|finding| {
                finding.category != "forbidden_phrase" || !finding.excerpt.contains("いかが")
            }),
            "{body}: {:?}",
            report.findings
        );
    }
    for body in [
        "いかがでしたか。",
        "説明は終わりです。いかがでしたか。",
        "さて、いかがでしょうか。",
    ] {
        let report = lint::analyze(body, &morphology, Some("business"), true).unwrap();
        assert!(
            report.findings.iter().any(|finding| {
                finding.category == "forbidden_phrase" && finding.excerpt.contains("いかが")
            }),
            "{body}"
        );
    }
}

#[test]
fn comma_candidates_preserve_clause_boundaries_and_modifier_scope() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "店長は、慌てて逃げた客を追いかけた。",
        "鍵を、なくした人に届けた。",
        "資料を、担当者が確認した。",
        "私は、彼が戻るまで待った。",
        "設定を、変更したときに記録する。",
    ] {
        let report = lint::analyze(body, &morphology, Some("tech"), true).unwrap();
        let commas = report
            .findings
            .iter()
            .filter(|finding| {
                matches!(
                    finding.category.as_str(),
                    "short_topic_comma" | "short_object_comma"
                )
            })
            .collect::<Vec<_>>();
        assert!(commas.is_empty(), "{body}: {commas:?}");
    }
    for (body, category) in [
        ("設定を、保存します。", "short_object_comma"),
        ("私は、設定を保存した。", "short_topic_comma"),
        ("設定を、会議で説明した。", "short_object_comma"),
    ] {
        let report = lint::analyze(body, &morphology, Some("tech"), true).unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.category == category),
            "{body}"
        );
    }
}

#[test]
fn permission_is_distinct_from_ability_advice_and_automatic_action() {
    let morphology = Morphology::new().unwrap();
    for after in [
        "記録を削除する。",
        "記録を削除できる。",
        "記録を削除するとよい。",
    ] {
        let report = diff::compare("記録を削除してもよい。", after, &morphology).unwrap();
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.category == "sentence_function_changed")
            .expect("permission change must be reported");
        assert_eq!(finding.severity, "warn");
        assert!(
            finding.detail.contains("許可"),
            "{after}: {}",
            finding.detail
        );
    }
    for (before, after) in [
        ("記録を削除してもよい。", "記録を削除しても構わない。"),
        ("記録を削除してもよい。", "記録を削除してよい。"),
        (
            "記録を削除してもいいです。",
            "記録を削除しても差し支えありません。",
        ),
        ("記録を削除して構いません。", "記録を削除してもよいです。"),
        ("記録を読んでもよい。", "記録を読んで構わない。"),
        ("記録を削除することができます。", "記録を削除できます。"),
        ("仕上がりはとてもよい。", "仕上がりはよい。"),
    ] {
        let report = diff::compare(before, after, &morphology).unwrap();
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.category != "sentence_function_changed"),
            "{before} -> {after}: {:?}",
            report.findings
        );
    }
}

#[test]
fn diff_cli_exposes_new_recipient_with_existing_json_and_exit_codes() {
    use assert_cmd::cargo::cargo_bin_cmd;
    use serde_json::Value;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("before.md"), "結果を報告します。").unwrap();
    std::fs::write(dir.path().join("after.md"), "結果を担当者へ報告します。").unwrap();
    let output = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args([
            "diff",
            "before.md",
            "after.md",
            "--json",
            "--fail-on",
            "warn",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let finding = json["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["category"] == "recipient_added")
        .unwrap();
    assert_eq!(finding["severity"], "warn");
    assert_eq!(finding["after_line"], 1);
    assert_eq!(
        json["after"]["sha256"],
        diff::sha256_hex("結果を担当者へ報告します。")
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("after.md")).unwrap(),
        "結果を担当者へ報告します。"
    );
}

#[test]
fn newly_invented_recipients_are_reported_at_the_changed_sentence() {
    let morphology = Morphology::new().unwrap();
    for (before, after, actor) in [
        (
            "容量が減ったら知らせる。",
            "容量が減ったら管理者に知らせる。",
            "管理者",
        ),
        ("結果を報告します。", "結果を担当者へ報告します。", "担当者"),
        (
            "結果を送ります。",
            "運用チームに結果を送ります。",
            "運用チーム",
        ),
    ] {
        let report = diff::compare(before, after, &morphology).unwrap();
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.category == "recipient_added")
            .unwrap_or_else(|| {
                panic!(
                    "{before} -> {after}: {:?}; tokens: {:?}",
                    report.findings,
                    morphology.tokenize(after).unwrap()
                )
            });
        assert_eq!(finding.severity, "warn");
        assert_eq!(finding.after_line, Some(1));
        assert!(finding.detail.contains(actor), "{}", finding.detail);
    }
    for (before, after) in [
        ("管理者に結果を送ります。", "結果を管理者へ送ります。"),
        ("結果を送る。", "結果を翌日に送る。"),
        ("結果を送る。", "結果を管理者に代わって送る。"),
        ("設定を変更する。", "管理者になると設定を変更する。"),
        ("結果を送る。", "管理者に送られる。"),
        ("`管理者に知らせる`", "`担当者に知らせる`"),
    ] {
        let report = diff::compare(before, after, &morphology).unwrap();
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.category != "recipient_added"),
            "{before} -> {after}: {:?}",
            report.findings
        );
    }
}
