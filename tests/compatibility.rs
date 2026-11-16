use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;

#[test]
fn finding_keeps_its_existing_public_fields_and_json_shape() {
    let finding = suiko::lint::Finding {
        line: 1,
        category: "translationese".to_owned(),
        excerpt: "対象の文".to_owned(),
        severity: "info".to_owned(),
        detail: "表現を確認する".to_owned(),
        related_lines: None,
        status: None,
        span: None,
        suggestion: None,
        review_suggestion: None,
    };
    assert_eq!(
        serde_json::to_value(finding).unwrap(),
        serde_json::json!({
            "line": 1,
            "category": "translationese",
            "excerpt": "対象の文",
            "severity": "info",
            "detail": "表現を確認する",
            "related_lines": null,
        })
    );
}

fn lint_fixture(name: &str, experimental: bool) -> Value {
    let path = format!("tests/fixtures/{name}");
    let mut command = cargo_bin_cmd!("suiko");
    command.args(["lint", &path, "--json"]);
    if experimental {
        command.arg("--experimental");
    }
    let output = command.output().expect("run suiko lint");
    assert!(output.status.success());
    serde_json::from_slice(&output.stdout).expect("valid JSON output")
}

#[test]
fn calibrated_fixtures_keep_their_finding_counts() {
    let smelly = lint_fixture("ai-smelly.md", false);
    let smelly_experimental = lint_fixture("ai-smelly.md", true);
    let natural = lint_fixture("natural.md", false);
    let natural_experimental = lint_fixture("natural.md", true);

    // 同一範囲の表層・形態素検出2件を形態素側の1件へまとめるため、従来より2件減る。
    assert_eq!(smelly["stats"]["total_findings"], 19);
    // L12「この事実は、」の後ろに「変えること」の連体修飾があるため、
    // 読点の削除候補を控える。従来の29件から1件減る。
    assert_eq!(smelly_experimental["stats"]["total_findings"], 28);
    assert!(smelly_experimental["stats"]["by_category"]["short_topic_comma"].is_null());
    assert_eq!(natural["stats"]["total_findings"], 0);
    assert_eq!(natural_experimental["stats"]["total_findings"], 0);
}

// 一般校正の担当範囲外であることを固定する。詳細は tests/fixtures/boundary/README.md。
#[test]
fn general_proofreading_problems_stay_out_of_scope() {
    for fixture in ["typo", "misuse", "variant", "keigo", "risk"] {
        let path = format!("tests/fixtures/boundary/{fixture}.md");
        let mut command = cargo_bin_cmd!("suiko");
        let output = command
            .args(["lint", &path, "--experimental", "--reading-load", "--json"])
            .output()
            .expect("run suiko lint");
        assert!(output.status.success());
        let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
        assert_eq!(
            json["stats"]["total_findings"], 0,
            "boundary fixture {fixture} must stay quiet"
        );
        assert_eq!(json["reading_load"]["stats"]["total"], 0);
    }

    // 公用文では翻訳調に加え、「下さい」「出来」のかな表記を確認候補として示す。
    // 文体の誤りとは判定せず、すべてinfoにとどめる。
    let output = cargo_bin_cmd!("suiko")
        .args([
            "lint",
            "tests/fixtures/boundary/koyobun.md",
            "--experimental",
            "--json",
        ])
        .output()
        .expect("run suiko lint");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON output");
    assert_eq!(json["stats"]["total_findings"], 3);
    assert_eq!(json["stats"]["by_category"]["translationese_morph"], 1);
    assert_eq!(json["stats"]["by_category"]["auxiliary_kana"], 2);
    assert!(
        json["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|finding| finding["severity"] == "info")
    );
}

#[test]
fn calibrated_categories_match_the_expected_profile() {
    let report = lint_fixture("ai-smelly.md", false);
    assert_eq!(report["stats"]["by_category"]["forbidden_phrase"], 8);
    // antithesis_repetitionは一致数を母数にした文書単位の集約finding(1件)
    assert_eq!(report["stats"]["by_category"]["antithesis_repetition"], 1);
    assert_eq!(report["stats"]["by_category"]["translationese"], 5);
    // Sudachi(Mode C)の長単位化で1段落の内容語数と抽象名詞率が変わり、
    // Lindera/IPADIC時代の2件から1件になった。残る1件はL22の一般論段落。
    assert_eq!(report["stats"]["by_category"]["low_specificity"], 1);
    assert_eq!(report["stats"]["by_category"]["low_burstiness"], 1);
    assert_eq!(report["stats"]["by_category"]["inanimate_subject_morph"], 1);
}
