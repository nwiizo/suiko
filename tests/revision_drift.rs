//! 改稿で気づかずに変わる文体・宛名・担当の語と、checkの事実の穴・指摘の変化を固定する。

use std::fs;

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use suiko::{diff, fact_gaps, morphology::Morphology};

fn categories(before: &str, after: &str, morphology: &Morphology) -> Vec<String> {
    diff::compare(before, after, morphology)
        .unwrap()
        .findings
        .into_iter()
        .map(|finding| finding.category)
        .collect()
}

#[test]
fn diff_reports_style_drift_and_fixed_values() {
    let morphology = Morphology::new().unwrap();
    for (before, after, category) in [
        (
            "最初の文です。\nすぐに書類を出してください。\n最後の文です。\n",
            "最初の文です。\n速やかに書類提出を願います。\n最後の文です。\n",
            "stiffness_increased",
        ),
        (
            "最初の文です。\n設定は今日中に終わります。\n最後の文です。\n",
            "最初の文です。\n設定は、たぶん、今日中に、終わります。\n最後の文です。\n",
            "comma_added",
        ),
        (
            "最初の文です。\n設定は今日中に終わります。\n最後の文です。\n",
            "最初の文です。\n設定は今日中に終わる。\n最後の文です。\n",
            "sentence_register_changed",
        ),
        (
            "最初の文です。\n## 使い方\n最後の文です。\n",
            "最初の文です。\n## 使い方の手順\n最後の文です。\n",
            "heading_rewritten",
        ),
        (
            "最初の文です。\n担当者が設定を確認します。\n最後の文です。\n",
            "最初の文です。\n運用担当者が設定を確認します。\n最後の文です。\n",
            "actor_narrowed",
        ),
        (
            "田中様\n\nお世話になっております。\n",
            "田中部長様\n\nお世話になっております。\n",
            "addressee_changed",
        ),
        (
            "とても便利です！\nぜひ使ってください！\n本当に助かります！\n",
            "便利です。\n使ってください。\n助かります。\n",
            "tone_flattened",
        ),
    ] {
        let found = categories(before, after, &morphology);
        assert!(
            found.iter().any(|found| found == category),
            "{category}: {found:?}"
        );
    }
}

#[test]
fn diff_keeps_quiet_for_unchanged_or_document_wide_changes() {
    let morphology = Morphology::new().unwrap();
    let quiet = [
        "stiffness_increased",
        "comma_added",
        "sentence_register_changed",
        "heading_rewritten",
        "actor_narrowed",
        "addressee_changed",
        "tone_flattened",
    ];
    for (before, after) in [
        ("田中様\n\n本文です。\n", "田中様\n\n本文を直しました。\n"),
        ("便利です！\n", "便利です。\n"),
        ("担当者が確認します。\n", "担当者がすぐ確認します。\n"),
        // 文書全体の文体を変えた場合は、文ごとの候補を重ねない。
        (
            "一つ目です。\n二つ目です。\n三つ目です。\n四つ目です。\n五つ目です。\n",
            "一つ目だ。\n二つ目だ。\n三つ目だ。\n四つ目だ。\n五つ目だ。\n",
        ),
    ] {
        let found = categories(before, after, &morphology);
        assert!(
            !found.iter().any(|found| quiet.contains(&found.as_str())),
            "{before:?} -> {after:?}: {found:?}"
        );
    }
}

#[test]
fn fact_gaps_skip_grounded_hedged_and_quoted_sentences() {
    let report = fact_gaps::analyze(
        "# 性能を大幅に改善しました\n\n再試行の間隔を短くしました。\nこの変更は運用の土台づくりです。\n- 混乱を最小限に抑えます。\n信頼性が向上します。\n",
        None,
    );
    let kinds = report
        .findings
        .iter()
        .map(|gap| (gap.kind, gap.line, gap.matched.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        [
            ("change", 3, "短くしました"),
            ("figure", 4, "土台づくり"),
            ("effect", 5, "混乱を最小限"),
            ("effect", 6, "信頼性が向上"),
        ]
    );
    assert_eq!(report.by_kind["effect"], 2);
    for text in [
        "性能を大幅に改善しました。時間は3秒です。",
        "性能を大幅に改善しました。\n時間は3秒です。",
        "本体より短くしました。",
        "たぶん大幅に減らします。",
        "仮に失敗すれば、大幅に減らします。",
        "詳細は省きますが、性能を大幅に改善しました。",
        "一部の利用者で大幅に改善しました。",
        "今後、性能を大幅に改善していきます。",
        "「大幅に改善しました。」と書かれています。",
        "`大幅に改善しました。` と表示します。",
        "大幅に改善されることはありません。",
        "大幅に改善しましたか？",
        "> 大幅に改善しました。",
        "| 大幅に改善しました。 |\n|---|",
        "```\n大幅に改善しました。\n```",
        "道を開けてください。",
        "基礎を固めました。",
    ] {
        let report = fact_gaps::analyze(text, None);
        assert_eq!(report.count, 0, "{text}: {:?}", report.findings);
    }
    let essay = fact_gaps::analyze("再試行の間隔を短くしました。", Some("essay"));
    assert_eq!(essay.skipped.as_deref(), Some("essay"));
    assert_eq!(essay.count, 0);
}

fn run_check(dir: &tempfile::TempDir, extra: &[&str]) -> Value {
    let output = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["check", "final.md", "--no-config", "--json"])
        .args(extra)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn check_records_fact_gaps_and_finding_changes_without_counting_gaps_for_fail_on() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("before.md"),
        "もちろん、速いです。\n本体の質感も上質です。\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("final.md"),
        "本体の質感も上質です。\n再試行の間隔を短くしました。\n",
    )
    .unwrap();

    let plain = run_check(&dir, &[]);
    assert!(plain["check"]["report"].get("fact_gaps").is_none());
    assert!(plain["check"]["report"].get("finding_changes").is_none());
    assert!(plain["check"]["options"].get("fact_gaps").is_none());

    let result = run_check(&dir, &["--before", "before.md", "--fact-gaps"]);
    let report = &result["check"]["report"];
    assert_eq!(result["check"]["options"]["fact_gaps"], true);
    assert_eq!(report["fact_gaps"]["count"], 1);
    assert_eq!(report["fact_gaps"]["findings"][0]["kind"], "change");
    assert_eq!(report["fact_gaps"]["findings"][0]["line"], 2);
    let changes = &report["finding_changes"];
    let has = |list: &str, category: &str| {
        changes[list]
            .as_array()
            .unwrap()
            .iter()
            .any(|key| key["category"] == category)
    };
    assert!(has("resolved", "short_concession"), "{changes}");
    assert!(has("kept", "thin_evaluation"), "{changes}");
    // 事実の穴の候補は --fail-on に数えない。
    fs::write(dir.path().join("gap.md"), "再試行の間隔を短くしました。\n").unwrap();
    let output = cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args([
            "check",
            "gap.md",
            "--no-config",
            "--json",
            "--fact-gaps",
            "--fail-on",
            "info",
        ])
        .output()
        .unwrap();
    let gap: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(gap["check"]["report"]["fact_gaps"]["count"], 1);
    assert_eq!(gap["check"]["threshold_exceeded"], false, "{gap}");
    assert!(output.status.success());

    cargo_bin_cmd!("suiko")
        .current_dir(dir.path())
        .args(["verify", "final.md.suiko-check.json", "--replay"])
        .assert()
        .success();

    let essay = run_check(&dir, &["--fact-gaps", "--genre", "essay"]);
    assert_eq!(essay["check"]["report"]["fact_gaps"]["skipped"], "essay");
}
