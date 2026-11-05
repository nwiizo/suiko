use suiko::{lint, morphology::Morphology};

#[test]
fn technical_structured_text_uses_morphological_comma_checks() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "## 設定を、保存する",
        "- 設定を、保存します。",
        "| 手順 | 設定を、保存します。 |",
    ] {
        let report = lint::analyze(body, &morphology, Some("tech"), false).unwrap();
        let found = report
            .findings
            .iter()
            .find(|f| f.category == "short_object_comma")
            .unwrap_or_else(|| panic!("{body}"));
        let span = found.span.unwrap();
        assert_eq!(&body[span.start_byte..span.end_byte], "、");
        assert_eq!(found.excerpt, "設定を、");
    }
    for body in [
        "## 鍵を、なくした人に届けます。",
        "> 設定を、保存します。",
        "````md\n## 設定を、保存します。\n````",
    ] {
        assert!(
            lint::analyze(body, &morphology, Some("tech"), false)
                .unwrap()
                .findings
                .iter()
                .all(|f| f.category != "short_object_comma"),
            "{body}"
        );
    }
}

#[test]
fn technical_nominalizations_are_checked_in_headings_lists_and_cells() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "その確かめは必要です。",
        "## 結果の確かめ",
        "- 結果の確かめを行う",
        "| 作業 | 内容 |\n| --- | --- |\n| 手順 | 結果の確かめ |",
    ] {
        let report = lint::analyze(body, &morphology, Some("tech"), false).unwrap();
        assert!(
            report.findings.iter().any(|f| f.category == "renyo_noun"),
            "{body}"
        );
    }
    for body in [
        "その確かめた結果を読む。",
        "案の見直しを進めます。",
        "## 結果の確認",
        "> 結果の確かめ",
        "`結果の確かめ`",
        "```text\n## 結果の確かめ\n```",
        "<!-- 結果の確かめ -->",
    ] {
        let report = lint::analyze(body, &morphology, Some("tech"), false).unwrap();
        assert!(
            report.findings.iter().all(|f| f.category != "renyo_noun"),
            "{body}"
        );
    }
}

#[test]
fn technical_operations_expose_required_actions_without_inventing_them() {
    let morphology = Morphology::new().unwrap();
    for body in [
        "ログの一括削除は、保存先の権限も変える必要があります。",
        "ログの採取は、出力先を指定する必要がある。",
        "設定の変更は、管理者の許可が必要です。",
    ] {
        let report = lint::analyze(body, &morphology, Some("tech"), false).unwrap();
        let finding = report
            .findings
            .iter()
            .find(|f| f.category == "operation_condition")
            .unwrap_or_else(|| panic!("{body}"));
        assert!(finding.suggestion.is_none());
        assert!(finding.detail.contains("条件"));
        assert!(
            lint::analyze(body, &morphology, Some("essay"), false)
                .unwrap()
                .findings
                .iter()
                .all(|f| f.category != "operation_condition")
        );
    }
    for body in [
        "ログを一括削除するには、権限を変える必要があります。",
        "ログの一括削除は、不要です。",
        "管理者は、設定を変える必要があります。",
        "設定の変更は、管理者が担当します。",
        "設定の変更は必要です。",
        "「設定の変更は、許可が必要です。」",
    ] {
        assert!(
            lint::analyze(body, &morphology, Some("tech"), false)
                .unwrap()
                .findings
                .iter()
                .all(|f| f.category != "operation_condition"),
            "{body}"
        );
    }
}

#[test]
fn technical_rules_do_not_join_separate_table_cells_or_expand_essay_checks() {
    let morphology = Morphology::new().unwrap();
    for body in ["| 結果の | 確かめが必要 |", "| 設定の変更は | 許可が必要 |"] {
        let report = lint::analyze(body, &morphology, Some("tech"), false).unwrap();
        assert!(
            report
                .findings
                .iter()
                .all(|f| !matches!(f.category.as_str(), "renyo_noun" | "operation_condition")),
            "{body}"
        );
    }
    for body in [
        "その確かめは必要です。",
        "## 結果の確かめ",
        "- 設定を、保存します。",
    ] {
        let report = lint::analyze(body, &morphology, Some("essay"), false).unwrap();
        assert!(
            report
                .findings
                .iter()
                .all(|f| !matches!(f.category.as_str(), "renyo_noun" | "short_object_comma")),
            "{body}"
        );
    }
}
