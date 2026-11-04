use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use suiko::{diff, lint, morphology::Morphology};

#[test]
fn ordinary_lint_includes_revision_candidates_without_flags_or_genre() {
    let morphology = Morphology::new().unwrap();
    for (body, category) in [
        (
            "判断に迷う項目は残さない側に倒します。",
            "decision_direction_metaphor",
        ),
        ("設定を、保存します。", "short_object_comma"),
        ("議論の解像度を上げます。", "vague_sensory_term"),
        ("資料を、全員へ。", "copy_fragment"),
        ("推敲後の文章は以下のとおりです。", "instruction_residue"),
        ("『青空』の四文字。", "quoted_character_count_mismatch"),
        ("仕様はチームの意図を実装へ運べます。", "abstract_metaphor"),
    ] {
        let report = lint::analyze(body, &morphology, None, false).unwrap();
        assert!(
            report.findings.iter().any(|f| f.category == category),
            "{category}: {body}"
        );
        assert!(!report.stats.experimental);
    }
}

#[test]
fn auxiliary_kana_suggestions_preserve_inflection_and_raw_spans() {
    let morphology = Morphology::new().unwrap();
    for (body, preimage, replacement) in [
        ("実行して下さい。", "下さい", "ください"),
        ("指定出来ます。", "出来", "でき"),
        ("確認して頂きます。", "頂き", "いただき"),
        ("試して見る。", "見る", "みる"),
        ("設定する事ができます。", "事", "こと"),
        ("確認する為に開きます。", "為", "ため"),
    ] {
        let report = lint::analyze(body, &morphology, None, false).unwrap();
        let finding = report
            .findings
            .iter()
            .find(|f| f.category == "auxiliary_kana")
            .unwrap_or_else(|| panic!("{body}: {:?}", morphology.tokenize(body).unwrap()));
        let suggestion = finding.suggestion.as_ref().expect("kana suggestion");
        assert_eq!(suggestion.preimage, preimage);
        assert_eq!(suggestion.replacement, replacement);
        assert_eq!(
            &body[suggestion.span.start_byte..suggestion.span.end_byte],
            preimage
        );
    }
    for body in [
        "事の経緯を話す。",
        "山の頂を見る。",
        "景色を見る。",
        "料理の出来がよい。",
        "料理が出来た。",
        "資料を持って見せる。",
        "「元の文章。\n実行して下さい。」",
        "`実行して下さい`",
        "> 実行して下さい。",
    ] {
        let report = lint::analyze(body, &morphology, None, false).unwrap();
        assert!(
            report
                .findings
                .iter()
                .all(|f| f.category != "auxiliary_kana"),
            "{body}"
        );
    }
}

#[test]
fn revision_wording_distinguishes_metaphors_from_literal_actions() {
    let morphology = Morphology::new().unwrap();
    for (body, category) in [
        ("調査に休日を溶かしてしまった。", "explanatory_metaphor"),
        ("処理を別のスレッドに逃がす。", "explanatory_metaphor"),
        ("データが静かに壊れた。", "technical_jargon_metaphor"),
        (
            "この関数は呼び出し元の事情を知っている。",
            "explanatory_metaphor",
        ),
        ("結果の確かめが必要です。", "renyo_noun"),
        ("二つの案の比べを行います。", "renyo_noun"),
        (
            "今回の数字は、売上が持ち直すという見立てです。",
            "revision_expression",
        ),
        (
            "プロジェクトレベルの設定を変更します。",
            "revision_expression",
        ),
    ] {
        let report = lint::analyze(body, &morphology, None, false).unwrap();
        let finding = report
            .findings
            .iter()
            .find(|f| f.category == category)
            .unwrap_or_else(|| panic!("{category}: {body}"));
        assert_eq!(finding.severity, "info");
        assert!(
            finding.suggestion.is_none(),
            "context-dependent edits must not be automatic"
        );
        let span = finding.span.expect("source span");
        assert_eq!(&body[span.start_byte..span.end_byte], finding.excerpt);
        assert_eq!(
            report
                .findings
                .iter()
                .filter(|f| matches!(
                    f.category.as_str(),
                    "explanatory_metaphor"
                        | "technical_jargon_metaphor"
                        | "renyo_noun"
                        | "revision_expression"
                ))
                .count(),
            1,
            "{body}"
        );
    }
    for body in [
        "氷を溶かす。",
        "魚を逃がす。",
        "時計が壊れた。",
        "薬が効いてくる。",
        "この薬は地味に効く。",
        "彼は黙って座った。",
        "資金集めを続ける。",
        "案の見直しを進める。",
        "住民票の写しを渡す。",
        "育ての親に会う。",
        "人の確かめた結果を読む。",
        "`休日を溶かす`",
        "> データが静かに壊れる。",
    ] {
        let report = lint::analyze(body, &morphology, None, false).unwrap();
        assert!(
            report.findings.iter().all(|f| !matches!(
                f.category.as_str(),
                "explanatory_metaphor" | "renyo_noun" | "revision_expression"
            )),
            "{body}"
        );
    }
}

#[test]
fn diff_reports_added_nominalization_without_changing_the_meaning_verdict() {
    let morphology = Morphology::new().unwrap();
    let report = diff::compare(
        "設定を見直せば待ち時間を減らせます。",
        "設定の見直しにより待ち時間の削減が可能です。",
        &morphology,
    )
    .unwrap();
    let finding = report
        .findings
        .iter()
        .find(|f| f.category == "nominalization_added")
        .expect("stiffer revision");
    assert_eq!(finding.severity, "info");
    for (before, after) in [
        ("処理を実施します。", "処理をします。"),
        ("保存が可能です。", "保存が可能です。"),
        ("`保存できます`", "`保存が可能です`"),
    ] {
        let report = diff::compare(before, after, &morphology).unwrap();
        assert!(
            report
                .findings
                .iter()
                .all(|f| f.category != "nominalization_added")
        );
    }
}

#[test]
fn default_cli_supports_json_gate_configuration_and_baseline_for_new_rules() {
    let dir = tempfile::tempdir().unwrap();
    let original = "確認して下さい。\n調査に休日を溶かした。\n";
    std::fs::write(dir.path().join("draft.md"), original).unwrap();
    let output = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["lint", "draft.md", "--json", "--fail-on", "info"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["stats"]["experimental"], false);
    assert_eq!(report["stats"]["by_category"]["auxiliary_kana"], 1);
    assert_eq!(report["stats"]["by_category"]["explanatory_metaphor"], 1);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("draft.md")).unwrap(),
        original
    );
    std::fs::write(dir.path().join("before.json"), output.stdout).unwrap();
    std::fs::write(
        dir.path().join("draft.md"),
        "確認してください。\n調査に休日を費やしてしまった。\n",
    )
    .unwrap();
    let output = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["lint", "draft.md", "--json", "--baseline", "before.json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["baseline"]["summary"]["resolved"], 2);
    std::fs::write(dir.path().join("draft.md"), original).unwrap();
    std::fs::write(
        dir.path().join(".suiko.toml"),
        "version = 1\ndisabled_rules = [\"auxiliary_kana\", \"explanatory_metaphor\"]\n",
    )
    .unwrap();
    cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["lint", "draft.md", "--json", "--fail-on", "info"])
        .assert()
        .success();
}
