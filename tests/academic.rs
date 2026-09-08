use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;

fn academic_fixture(name: &str) -> (std::process::Output, Value) {
    let output = cargo_bin_cmd!("suiko")
        .args([
            "academic",
            &format!("tests/fixtures/{name}"),
            "--contract",
            "tests/fixtures/academic-contract.json",
            "--json",
        ])
        .output()
        .expect("run academic audit");
    let report = serde_json::from_slice(&output.stdout).expect("valid academic JSON");
    (output, report)
}

#[test]
fn academic_contract_accepts_a_grounded_manuscript() {
    let (output, report) = academic_fixture("academic-good.md");
    assert!(output.status.success());
    assert_eq!(report["passed"], true);
    assert!(
        report["checks"]
            .as_array()
            .expect("checks")
            .iter()
            .any(|item| item["id"] == "section_bridge" && item["status"] == "pass")
    );
    assert!(
        report["first_sentences"]
            .as_array()
            .expect("first sentences")
            .iter()
            .all(|item| !item["text"].as_str().expect("text").starts_with("[^"))
    );
    let human = cargo_bin_cmd!("suiko")
        .args([
            "academic",
            "tests/fixtures/academic-good.md",
            "--contract",
            "tests/fixtures/academic-contract.json",
        ])
        .output()
        .expect("run human academic audit");
    assert!(human.status.success());
    let stdout = String::from_utf8_lossy(&human.stdout);
    assert!(stdout.contains("原稿・指定成果物の監査: PASS"));
    assert!(stdout.contains("提出準備完了: NO"));
}

#[test]
fn academic_contract_rejects_rq_caveats_and_missing_bridges() {
    let (output, report) = academic_fixture("academic-bad.md");
    assert_eq!(output.status.code(), Some(2));
    let failed = report["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .filter(|item| item["status"] == "fail")
        .map(|item| item["id"].as_str().expect("id"))
        .collect::<Vec<_>>();
    assert!(failed.contains(&"research_question_policy"));
    assert!(failed.contains(&"term_provenance"));
    assert!(failed.contains(&"defensive_caveats"));
    assert!(failed.contains(&"section_bridge"));
    assert!(failed.contains(&"citation_reference_match"));
}

#[test]
fn style_profile_distinguishes_a_before_after_revision_pair() {
    let before = cargo_bin_cmd!("suiko")
        .args([
            "academic",
            "tests/fixtures/academic-style-before.md",
            "--contract",
            "tests/fixtures/academic-style-contract.json",
            "--json",
        ])
        .output()
        .expect("audit before");
    assert_eq!(before.status.code(), Some(2));

    let after = cargo_bin_cmd!("suiko")
        .args([
            "academic",
            "tests/fixtures/academic-style-after.md",
            "--contract",
            "tests/fixtures/academic-style-contract.json",
            "--json",
        ])
        .output()
        .expect("audit after");
    assert!(after.status.success());
}
