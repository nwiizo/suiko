use std::fs;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::Value;
use suiko::{diff, morphology::Morphology};

fn categories(before: &str, after: &str, morphology: &Morphology) -> Vec<(String, String)> {
    diff::compare(before, after, morphology)
        .unwrap()
        .findings
        .into_iter()
        .map(|finding| (finding.category, finding.severity))
        .collect()
}

fn has(found: &[(String, String)], category: &str) -> bool {
    found.iter().any(|(name, _)| name == category)
}

fn pair(dir: &tempfile::TempDir, before: &str, after: &str) -> (String, String) {
    let before_path = dir.path().join("before.md");
    let after_path = dir.path().join("after.md");
    fs::write(&before_path, before).unwrap();
    fs::write(&after_path, after).unwrap();
    (
        before_path.to_string_lossy().into_owned(),
        after_path.to_string_lossy().into_owned(),
    )
}

#[test]
fn identical_documents_report_no_findings_and_no_changed_regions() {
    let morphology = Morphology::new().unwrap();
    let body =
        "# 手順\n\n失敗時だけ最大3回再試行する。ログは保存しない。\n\n- 設定を保存できます。\n";
    let report = diff::compare(body, body, &morphology).unwrap();
    assert!(report.findings.is_empty());
    assert_eq!(report.stats.changed_regions, 0);
    assert_eq!(report.stats.before_units, report.stats.matched_units);
}

#[test]
fn numbers_and_conditions_dropped_by_a_rewrite_are_reported() {
    let morphology = Morphology::new().unwrap();
    let found = categories(
        "失敗時だけ最大3回再試行する。\n",
        "3回再試行する。\n",
        &morphology,
    );
    assert!(has(&found, "number_changed"), "{found:?}");
    assert!(has(&found, "condition_changed"), "{found:?}");
    assert!(
        found
            .iter()
            .filter(|(name, _)| name == "number_changed" || name == "condition_changed")
            .all(|(_, severity)| severity == "warn")
    );
    // 表記だけの変更（全角・桁区切り）は数値の変化として扱わない。
    let found = categories(
        "処理は1,000件を上限とする。\n",
        "処理は１０００件を上限とする。\n",
        &morphology,
    );
    assert!(!has(&found, "number_changed"), "{found:?}");
}

#[test]
fn negation_flips_are_reported_but_fixed_idioms_are_not_counted() {
    let morphology = Morphology::new().unwrap();
    let found = categories("ログは保存しない。\n", "ログは保存する。\n", &morphology);
    assert!(has(&found, "negation_changed"), "{found:?}");
    // 「しなければならない」→「する必要がある」は義務の言い換えで、否定の増減ではない。
    let found = categories(
        "設定を確認しなければならない。\n",
        "設定を確認する必要がある。\n",
        &morphology,
    );
    assert!(!has(&found, "negation_changed"), "{found:?}");
    assert!(!has(&found, "sentence_function_changed"), "{found:?}");
    assert!(!has(&found, "condition_changed"), "{found:?}");
}

#[test]
fn sentence_function_changes_between_ability_request_and_statement_are_reported() {
    let morphology = Morphology::new().unwrap();
    for (before, after, detail) in [
        (
            "設定を保存できます。\n",
            "設定を保存してください。\n",
            "可能",
        ),
        ("設定を保存してください。\n", "設定を保存します。\n", "依頼"),
        (
            "障害が起きるかもしれません。\n",
            "障害が起きます。\n",
            "推量",
        ),
        (
            "再起動するとよいでしょう。\n",
            "再起動してください。\n",
            "勧め",
        ),
    ] {
        let report = diff::compare(before, after, &morphology).unwrap();
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.category == "sentence_function_changed")
            .unwrap_or_else(|| panic!("{before} -> {after}: {:?}", report.findings));
        assert_eq!(finding.severity, "warn");
        assert!(finding.detail.contains(detail), "{}", finding.detail);
        assert_eq!(finding.before_line, Some(1));
        assert_eq!(finding.after_line, Some(1));
    }
}

#[test]
fn splitting_or_merging_sentences_is_not_reported_as_loss() {
    let morphology = Morphology::new().unwrap();
    for (before, after) in [
        (
            "設定ファイルを読み込み、値を検証した。\n",
            "設定ファイルを読み込んだ。値を検証した。\n",
        ),
        (
            "設定ファイルを読み込んだ。値を検証した。\n",
            "設定ファイルを読み込み、値を検証した。\n",
        ),
    ] {
        let found = categories(before, after, &morphology);
        assert!(!has(&found, "sentence_removed"), "{before}: {found:?}");
        assert!(!has(&found, "sentence_added"), "{before}: {found:?}");
        assert!(
            !has(&found, "sentence_function_changed"),
            "{before}: {found:?}"
        );
    }
}

#[test]
fn removed_and_added_sentences_and_new_subjects_are_reported() {
    let morphology = Morphology::new().unwrap();
    let found = categories(
        "通知を送る。失敗したら担当者へ連絡する。\n",
        "通知を送る。\n",
        &morphology,
    );
    assert!(has(&found, "sentence_removed"), "{found:?}");
    let found = categories("通知を送る。\n", "運用チームが通知を送る。\n", &morphology);
    assert!(has(&found, "subject_added"), "{found:?}");
    let found = categories(
        "通知を送る。\n",
        "通知を送る。処理はキャッシュ不足で遅くなる。\n",
        &morphology,
    );
    assert!(has(&found, "sentence_added"), "{found:?}");
    // 文の順序を入れ替えただけなら、削除や追加としては数えない。
    let report = diff::compare(
        "通知を送る。失敗したら担当者へ連絡する。\n",
        "失敗したら担当者へ連絡する。通知を送る。\n",
        &morphology,
    )
    .unwrap();
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    assert_eq!(report.stats.moved_units, 2);
}

#[test]
fn certainty_words_identifiers_quotes_and_citations_are_compared() {
    let morphology = Morphology::new().unwrap();
    let found = categories(
        "来週リリースする予定です。\n",
        "来週リリースします。\n",
        &morphology,
    );
    assert!(has(&found, "certainty_changed"), "{found:?}");
    let found = categories(
        "`--fail-on warn` を付けて実行する。\n",
        "`--fail-on info` を付けて実行する。\n",
        &morphology,
    );
    assert!(has(&found, "identifier_changed"), "{found:?}");
    let found = categories(
        "担当者は「来月まで対応する」と述べた。\n",
        "担当者は「来月までに対応を終える」と述べた。\n",
        &morphology,
    );
    assert!(has(&found, "quotation_changed"), "{found:?}");
    let found = categories(
        "この手法は有効だと報告されている[1]。\n",
        "この手法は有効だと報告されている。\n",
        &morphology,
    );
    assert!(has(&found, "citation_changed"), "{found:?}");
}

#[test]
fn rewrites_that_keep_values_and_polarity_are_not_reported() {
    let morphology = Morphology::new().unwrap();
    // 「正常に」の中の「常に」は確度の語ではない。
    let found = categories("処理は正常に終わる。\n", "処理は終わる。\n", &morphology);
    assert!(!has(&found, "certainty_changed"), "{found:?}");
    // 既にある値を繰り返しただけなら数値の変化ではない。
    let found = categories(
        "接続待ちがこれを超えた。タイムアウトは30秒だ。\n",
        "接続待ちが30秒を超えた。タイムアウトは30秒だ。\n",
        &morphology,
    );
    assert!(!has(&found, "number_changed"), "{found:?}");
    // 体言止めの項目を文にした変化は文末の働きの変化として扱わない。
    let found = categories(
        "日時：10月3日（土）\n",
        "10月3日（土）に実施します。\n",
        &morphology,
    );
    assert!(!has(&found, "sentence_function_changed"), "{found:?}");
    // 文全体を書き直して否定の数が変わっただけでは候補にしない。文末の肯定・否定が入れ替われば候補にする。
    let found = categories(
        "設定が静かに壊れる。\n",
        "設定が読み込まれなくても、画面にはエラーが出ない。\n",
        &morphology,
    );
    assert!(has(&found, "negation_changed"), "{found:?}");
    let found = categories(
        "接続が解放されないまま待機リクエストが積み上がった。\n",
        "接続が解放されず、待機リクエストが積み上がって、プールに空きがなくなった。\n",
        &morphology,
    );
    assert!(!has(&found, "negation_changed"), "{found:?}");
    // 条件が増えただけなら info、消えたら warn。
    let report = diff::compare("再起動する。\n", "失敗したら再起動する。\n", &morphology).unwrap();
    let added = report
        .findings
        .iter()
        .find(|finding| finding.category == "condition_changed")
        .unwrap();
    assert_eq!(added.severity, "info");
    let report = diff::compare("失敗したら再起動する。\n", "再起動する。\n", &morphology).unwrap();
    let lost = report
        .findings
        .iter()
        .find(|finding| finding.category == "condition_changed")
        .unwrap();
    assert_eq!(lost.severity, "warn");
}

#[test]
fn swapped_subject_and_object_are_reported() {
    let morphology = Morphology::new().unwrap();
    let found = categories(
        "クライアントがサーバーを呼び出す。\n",
        "サーバーがクライアントを呼び出す。\n",
        &morphology,
    );
    assert!(has(&found, "subject_object_swapped"), "{found:?}");
}

#[test]
fn list_items_headings_and_code_blocks_are_compared() {
    let morphology = Morphology::new().unwrap();
    let found = categories(
        "- 3回まで再試行する\n- ログを残す\n",
        "- 5回まで再試行する\n- ログを残す\n",
        &morphology,
    );
    assert!(has(&found, "number_changed"), "{found:?}");
    let found = categories(
        "```sh\nsuiko lint a.md\n```\n",
        "```sh\nsuiko lint b.md\n```\n",
        &morphology,
    );
    assert!(has(&found, "protected_changed"), "{found:?}");
    let report = diff::compare(
        "## 導入\n\n本文。\n",
        "## 導入手順\n\n本文。\n",
        &morphology,
    )
    .unwrap();
    assert!(
        report
            .findings
            .iter()
            .all(|finding| finding.category != "sentence_function_changed"),
        "{:?}",
        report.findings
    );
    assert_eq!(report.stats.changed_regions, 1);
}

#[test]
fn cli_reports_hashes_json_shape_and_fail_on_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let before = "失敗時だけ最大3回再試行する。\n";
    let after = "3回再試行する。\n";
    let (before_path, after_path) = pair(&dir, before, after);

    let output = cargo_bin_cmd!("suiko")
        .args(["diff", &before_path, &after_path, "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["suiko_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(json["before"]["file"], before_path.as_str());
    assert_eq!(json["after"]["file"], after_path.as_str());
    assert_eq!(json["before"]["sha256"], diff::sha256_hex(before));
    assert_eq!(json["after"]["sha256"], diff::sha256_hex(after));
    assert!(json["stats"]["total_findings"].as_u64().unwrap() >= 2);
    assert_eq!(json["stats"]["changed_regions"], 1);
    let finding = &json["findings"][0];
    for key in [
        "category",
        "severity",
        "before_line",
        "after_line",
        "before_excerpt",
        "after_excerpt",
        "detail",
    ] {
        assert!(finding.get(key).is_some(), "{key} missing: {finding}");
    }

    cargo_bin_cmd!("suiko")
        .args(["diff", &before_path, &after_path])
        .assert()
        .success()
        .stdout(predicate::str::contains("number_changed"))
        .stdout(predicate::str::contains("意味が同じかは判定していません"));

    cargo_bin_cmd!("suiko")
        .args(["diff", &before_path, &after_path, "--fail-on", "warn"])
        .assert()
        .code(2);

    let (same_before, same_after) = pair(&dir, before, before);
    cargo_bin_cmd!("suiko")
        .args(["diff", &same_before, &same_after, "--fail-on", "info"])
        .assert()
        .success()
        .stdout(predicate::str::contains("候補 0件"));

    cargo_bin_cmd!("suiko")
        .args(["diff", "-", "-"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("標準入力"));
}

#[test]
fn table_cells_potential_verbs_register_and_structure_are_compared() {
    let morphology = Morphology::new().unwrap();
    // 表の中の数値は行ごとに対応づけて比べる。
    let report = diff::compare(
        "| 項目 | 値 |\n|---|---|\n| 再試行 | 3回 |\n| 待機 | 30秒 |\n",
        "| 項目 | 値 |\n|---|---|\n| 再試行 | 5回 |\n| 待機 | 30秒 |\n",
        &morphology,
    )
    .unwrap();
    let table = report
        .findings
        .iter()
        .find(|finding| finding.category == "table_changed")
        .unwrap_or_else(|| panic!("{:?}", report.findings));
    assert_eq!(table.severity, "warn");
    assert!(
        table.detail.contains("3回") && table.detail.contains("5回"),
        "{}",
        table.detail
    );
    assert_eq!(table.before_line, Some(3));
    assert_eq!(table.after_line, Some(3));
    // 可能動詞と「れる／られる」の文末。他動詞の「進める」は可能とみなさない。
    for (before, after, from, to) in [
        (
            "この設定でログを読めます。\n",
            "この設定でログを読んでください。\n",
            "可能",
            "依頼",
        ),
        (
            "ログは管理画面で見られます。\n",
            "ログは管理画面で見ます。\n",
            "受身・可能",
            "説明・断定",
        ),
        (
            "設定を進めます。\n",
            "設定を進めてください。\n",
            "説明・断定",
            "依頼",
        ),
    ] {
        let report = diff::compare(before, after, &morphology).unwrap();
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.category == "sentence_function_changed")
            .unwrap_or_else(|| panic!("{before}: {:?}", report.findings));
        assert!(
            finding.detail.contains(&format!("「{from}」から「{to}」")),
            "{before}: {}",
            finding.detail
        );
    }
    // 敬体と常体の割合が大きく変わった文書。
    let plain = "設定を読む。値を検証する。失敗したら止める。ログを残す。結果を返す。終了する。\n";
    let polite = "設定を読みます。値を検証します。失敗したら止めます。ログを残します。結果を返します。終了します。\n";
    let report = diff::compare(plain, polite, &morphology).unwrap();
    let register = report
        .findings
        .iter()
        .find(|finding| finding.category == "register_changed")
        .unwrap_or_else(|| panic!("{:?}", report.findings));
    assert_eq!(register.severity, "info");
    assert!(register.detail.contains("敬体"), "{}", register.detail);
    let found = categories(plain, plain, &morphology);
    assert!(!has(&found, "register_changed"));
    // 箇条書きと地の文の入れ替わりと、見出しの階層。
    let found = categories("- ログを残す\n", "ログを残す。\n", &morphology);
    assert!(has(&found, "structure_changed"), "{found:?}");
    let found = categories("## 導入\n\n本文。\n", "### 導入\n\n本文。\n", &morphology);
    assert!(has(&found, "structure_changed"), "{found:?}");
}

#[test]
fn reversed_contrast_added_feelings_causes_and_promises_are_reported() {
    let morphology = Morphology::new().unwrap();
    let report = diff::compare(
        "速さではなく正確さを優先する。\n",
        "正確さではなく速さを優先する。\n",
        &morphology,
    )
    .unwrap();
    let contrast = report
        .findings
        .iter()
        .find(|finding| finding.category == "contrast_changed")
        .unwrap_or_else(|| panic!("{:?}", report.findings));
    assert_eq!(contrast.severity, "warn");
    assert!(contrast.detail.contains("逆"), "{}", contrast.detail);
    let found = categories(
        "速さではなく正確さを優先する。\n",
        "正確さを優先する。\n",
        &morphology,
    );
    assert!(
        found
            .iter()
            .any(|(name, severity)| name == "contrast_changed" && severity == "info"),
        "{found:?}"
    );
    let found = categories(
        "障害が発生した。\n",
        "障害が発生し、大変申し訳なく思う。\n",
        &morphology,
    );
    assert!(has(&found, "feeling_added"), "{found:?}");
    let found = categories(
        "処理が遅い。キャッシュが足りない。\n",
        "キャッシュが足りないため、処理が遅い。\n",
        &morphology,
    );
    assert!(has(&found, "causal_changed"), "{found:?}");
    let found = categories(
        "再発防止策を検討する。\n",
        "再発防止を徹底いたします。\n",
        &morphology,
    );
    assert!(has(&found, "promise_added"), "{found:?}");
    for category in ["feeling_added", "causal_changed", "promise_added"] {
        assert!(
            found
                .iter()
                .all(|(name, severity)| name != category || severity == "info"),
            "{category} must be info: {found:?}"
        );
    }
}
